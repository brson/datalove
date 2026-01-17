//! Datafun compilation pipeline.
//!
//! Two-stage compilation: modules first, then scripts.
//!
//! - [`ModuleCompilationPipeline`]: compiles modules through parsing, typechecking,
//!   drop analysis, and IR lowering. Supports incremental recompilation.
//!
//! - [`ScriptCompilationContext`]: incrementally compiles and executes script
//!   fragments and expressions against compiled modules.
//!
//! # Example
//!
//! ```ignore
//! // Stage 1: compile modules.
//! let mut pipeline = ModuleCompilationPipeline::new();
//! pipeline.add_module(&db, "local", "mypackage", "main", source);
//! let compiled = pipeline.compile_fresh(&db);
//!
//! // Stage 2: run scripts.
//! let mut ctx = compiled.script_context(&db, DebugOutputMode::Stderr);
//! ctx.eval_fragment("let x = 42");
//! ctx.eval_expr("x + 1");
//! ```
//!
//! # Incremental Compilation
//!
//! The pipeline supports incremental recompilation after source changes. Use
//! `compile_fresh` for the first compilation (only needs `&db`), then
//! `update_source` and `compile` for subsequent updates (needs `&mut db`).
//!
//! ```ignore
//! let mut pipeline = ModuleCompilationPipeline::new();
//! pipeline.add_module(&db, "local", "pkg", "main", source_v1);
//! let compiled1 = pipeline.compile_fresh(&db);
//!
//! // Edit a module and recompile incrementally.
//! pipeline.update_source(&mut db, "local", "pkg", "main", source_v2);
//! let (compiled2, db) = pipeline.compile(&mut db);
//! ```
//!
//! The `&mut db` requirement for `compile` comes from salsa: updating existing
//! tracked structs requires mutable access. The pipeline preserves module and
//! graph identity across updates, enabling salsa to skip recomputing unchanged
//! portions of the compilation.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::{BTreeMap, HashMap};

use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;
use datalove_datafun_ir::{IrModuleId, FuncId, IrType, IrScriptUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_compiler::drop_analysis;
use datalove_datafun_compiler::tracked_lower::lower_module_graph_with_mode;
use datalove_datafun_tycheck::{
    typecheck_module_graph, typecheck_module_graph_with_mode,
    type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked,
    DbClone, ParallelMode, parallel_mode_from_env,
};
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult, ModuleId,
    ParsedModuleGraph, parse_module_graph_with_mode,
};
use datalove_datafun_interp::{CallDispatcher, ScriptEnvironment, UnitCompletion};
use datalove_rt::rust::AlignedBuffer;

use crate::incremental::IncrementalModuleWorld;

// ============================================================================
// Result types
// ============================================================================

/// Typecheck result summary (serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    ParseError { errors: Vec<String> },
    Error { errors: Vec<String> },
    Skipped,
}

/// Lowering result summary (serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success { ir: String },
    Error { message: String },
    Skipped,
}

// ============================================================================
// Module compilation pipeline
// ============================================================================

/// Compiles modules through parsing, typechecking, drop analysis, and IR lowering.
///
/// Supports both one-shot compilation (`compile_fresh`) and incremental
/// recompilation (`compile`) after source updates.
pub struct ModuleCompilationPipeline {
    world: IncrementalModuleWorld,
}

impl ModuleCompilationPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self {
            world: IncrementalModuleWorld::new(),
        }
    }

    /// Create a pipeline from worldfile sections.
    pub fn from_sections(
        db: &dyn salsa::Database,
        sections: &[WorldfileSection],
    ) -> Self {
        let mut pipeline = Self::new();
        pipeline.add_modules_from_sections(db, sections);
        pipeline
    }

    /// Add a module to the pipeline.
    pub fn add_module(
        &mut self,
        db: &dyn salsa::Database,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.add_module(db, &path, source);
    }

    /// Add modules from worldfile sections.
    pub fn add_modules_from_sections(
        &mut self,
        db: &dyn salsa::Database,
        sections: &[WorldfileSection],
    ) {
        for section in sections {
            if let WorldfileSection::Module { library, package, module, source } = section {
                self.add_module(db, library, package, module, source);
            }
        }
    }

    /// Load sys library from directory.
    pub async fn load_sys_library_from_dir(
        &mut self,
        db: &dyn salsa::Database,
        sys_dir: std::path::PathBuf,
    ) -> AnyResult<()> {
        use datalove_datafun_pkg::package_load;

        let config = package_load::PackageWorldConfig {
            dir_pkglib_system: sys_dir,
            dir_pkglib_local: None,
        };

        let package_world_raw = package_load::load_world(config).await?;

        for (pkg_name, pkg) in &package_world_raw.pkglib_system {
            for (mod_name, pkg_module) in &pkg.modules {
                self.add_module(db, "sys", pkg_name, mod_name, &pkg_module.text);
            }
        }

        Ok(())
    }

    /// Load sys library from default location.
    pub async fn load_sys_library_default(&mut self, db: &dyn salsa::Database) -> AnyResult<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = std::path::PathBuf::from(manifest_dir);
        let parent = manifest_path.parent()
            .ok_or_else(|| anyhow!("Failed to get parent directory"))?;
        let grandparent = parent.parent()
            .ok_or_else(|| anyhow!("Failed to get grandparent directory"))?;
        let sys_dir = grandparent.join("sys");

        self.load_sys_library_from_dir(db, sys_dir).await
    }

    /// Check if a module exists.
    pub fn contains_module(&self, library: &str, package: &str, module: &str) -> bool {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.contains(&path)
    }

    /// Remove a module from the pipeline.
    pub fn remove_module(&mut self, library: &str, package: &str, module: &str) {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.remove_module(&path);
    }

    /// Update a module's source text for incremental recompilation.
    pub fn update_source(
        &mut self,
        db: &mut dyn salsa::Database,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.update_source(db, &path, source);
    }

    /// Compile all modules (first compilation).
    ///
    /// Uses `DATALOVE_PARALLEL` env var to determine parallelism mode.
    pub fn compile_fresh<'db>(&mut self, db: &'db dyn DbClone) -> CompiledModules<'db> {
        let mode = parallel_mode_from_env();
        self.compile_fresh_with_mode(db, mode)
    }

    /// Compile all modules with explicit parallelism mode.
    pub fn compile_fresh_with_mode<'db>(
        &mut self,
        db: &'db dyn DbClone,
        mode: ParallelMode,
    ) -> CompiledModules<'db> {
        let (module_graph, resolved_requires) = self.world.build_fresh(db.as_salsa_db());
        self.compile_impl_with_mode(db, module_graph, resolved_requires, mode)
    }

    /// Compile all modules (incremental, needs `&mut db`).
    ///
    /// Uses `DATALOVE_PARALLEL` env var to determine parallelism mode.
    /// Requires a concrete type implementing `DbClone` for potential parallelism.
    pub fn compile<'db, D: DbClone>(
        &mut self,
        db: &'db mut D,
    ) -> (CompiledModules<'db>, &'db D) {
        let mode = parallel_mode_from_env();
        self.compile_with_mode(db, mode)
    }

    /// Compile all modules with explicit parallelism mode (incremental).
    pub fn compile_with_mode<'db, D: DbClone>(
        &mut self,
        db: &'db mut D,
        mode: ParallelMode,
    ) -> (CompiledModules<'db>, &'db D) {
        let (module_graph, resolved_requires) = self.world.prepare_for_compile(db);

        // Reborrow as immutable for the rest of compilation.
        let db_ref: &'db D = &*db;

        let compiled = self.compile_impl_with_mode(db_ref, module_graph, resolved_requires, mode);
        (compiled, db_ref)
    }

    /// Internal compilation implementation with configurable parallelism.
    fn compile_impl_with_mode<'db>(
        &self,
        db: &'db dyn DbClone,
        module_graph: ModuleGraph,
        resolved_requires: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
        mode: ParallelMode,
    ) -> CompiledModules<'db> {
        let parsed_graph = parse_module_graph_with_mode(db, module_graph.clone(), resolved_requires, mode);
        let graph_typecheck = typecheck_module_graph_with_mode(db, parsed_graph, mode);
        self.compile_from_typecheck_result(db, module_graph, parsed_graph, graph_typecheck, mode)
    }

    /// Compile from parsed and typechecked module graph.
    fn compile_from_typecheck_result<'db>(
        &self,
        db: &'db dyn DbClone,
        module_graph: ModuleGraph,
        parsed_graph: ParsedModuleGraph<'db>,
        graph_typecheck: ModuleGraphTypecheckResult<'db>,
        mode: ParallelMode,
    ) -> CompiledModules<'db> {
        // Collect typecheck errors.
        let module_errors = graph_typecheck.module_errors(db.as_salsa_db());
        let mut path_to_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (module_id, errors) in module_errors {
            let path = module_id.path(db.as_salsa_db()).clone();
            let error_strings: Vec<String> = errors.iter()
                .map(|e| format!("{}: {:?}", path, e))
                .collect();
            path_to_errors.insert(path, error_strings);
        }

        // Lower to IR using tracked lowering API.
        let lowering_result = lower_module_graph_with_mode(db, parsed_graph, graph_typecheck, mode);

        // Convert FuncIdMap to HashMap.
        let func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)> =
            lowering_result.func_id_map(db.as_salsa_db()).to_hashmap(db.as_salsa_db());

        // Build ScriptEnvironment and collect results.
        let mut env = ScriptEnvironment::new();
        let mut module_lowering_results: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut drop_analysis_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for (module_id, result) in lowering_result.module_results(db.as_salsa_db()) {
            let module_path = module_id.path(db.as_salsa_db()).clone();
            let ir_module_id = result.ir_module_id(db.as_salsa_db());

            // Collect errors (includes both drop analysis and lowering errors).
            let errors = result.errors(db.as_salsa_db());
            if !errors.is_empty() {
                // Separate drop analysis errors from other lowering errors.
                for error in errors {
                    if error.contains("Drop analysis error") {
                        let key = format!("{}", module_path);
                        drop_analysis_errors.entry(key).or_default().push(error.clone());
                    }
                }
            }

            // Add functions to environment and collect IR dumps.
            let mut ir_dumps = Vec::new();
            for ir_func in result.functions(db.as_salsa_db()) {
                ir_dumps.push(format!("{}", ir_func));
                env.add_module_function(ir_module_id, ir_func.id, ir_func.clone());
            }

            // Add any errors to the IR dumps for backwards compatibility.
            for error in errors {
                ir_dumps.push(error.clone());
            }

            module_lowering_results.insert(module_path, ir_dumps);
        }

        CompiledModules {
            resolution_error: None,
            module_graph,
            parsed_graph,
            graph_typecheck,
            path_to_errors,
            drop_analysis_errors,
            func_id_map,
            env,
            module_lowering_results,
        }
    }
}

impl Default for ModuleCompilationPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Compiled modules
// ============================================================================

/// Result of compiling modules, ready for script execution.
pub struct CompiledModules<'db> {
    pub resolution_error: Option<String>,
    pub module_graph: ModuleGraph,
    pub parsed_graph: ParsedModuleGraph<'db>,
    pub graph_typecheck: ModuleGraphTypecheckResult<'db>,
    pub path_to_errors: BTreeMap<String, Vec<String>>,
    pub drop_analysis_errors: BTreeMap<String, Vec<String>>,
    pub func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    pub env: ScriptEnvironment,
    pub module_lowering_results: BTreeMap<String, Vec<String>>,
}

impl<'db> CompiledModules<'db> {
    /// Check if compilation succeeded.
    pub fn is_successful(&self) -> bool {
        self.resolution_error.is_none()
            && self.path_to_errors.values().all(|errors| errors.is_empty())
            && self.drop_analysis_errors.is_empty()
            && !self.has_lowering_errors()
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.is_successful()
    }

    /// Collect all errors.
    pub fn all_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if let Some(err) = &self.resolution_error {
            errors.push(format!("Resolution error: {}", err));
        }

        for error_list in self.path_to_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        for (func_name, error_list) in &self.drop_analysis_errors {
            for error in error_list {
                errors.push(format!("Drop analysis error in {}: {}", func_name, error));
            }
        }

        for error_list in self.module_lowering_results.values() {
            for error in error_list {
                if is_lowering_error(error) {
                    errors.push(error.clone());
                }
            }
        }

        errors
    }

    fn has_lowering_errors(&self) -> bool {
        self.module_lowering_results.values()
            .flatten()
            .any(|s| is_lowering_error(s))
    }

    /// Get all typecheck errors.
    pub fn all_typecheck_errors(&self) -> Vec<String> {
        self.path_to_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all drop analysis errors.
    pub fn all_drop_analysis_errors(&self) -> Vec<String> {
        self.drop_analysis_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get module type diagnostics with spans for rendering.
    pub fn get_module_type_diagnostics(&self, db: &'db dyn salsa::Database) -> Vec<&'db datalove_diagnostic::TypeDiagnostic> {
        typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, self.parsed_graph)
    }

    /// Create a context for compiling and executing scripts against these modules.
    pub fn script_context(
        self,
        db: &'db dyn salsa::Database,
        debug_mode: datalove_rt::c::DebugOutputMode,
    ) -> ScriptCompilationContext<'db> {
        let script_ctx = lower::ScriptLowerContext::new();
        let mut module_specs = Vec::new();

        // Build a map of spans for quick lookup.
        let spans_map: std::collections::HashMap<_, _> = self.parsed_graph.spans(db).iter()
            .map(|(id, spans)| (*id, spans.clone()))
            .collect();

        for (salsa_module_id, parsed) in self.parsed_graph.statements_only(db) {
            let module_path = salsa_module_id.path(db).clone();
            let module_source = self.module_graph.iter_modules(db)
                .find(|m| m.id(db) == *salsa_module_id)
                .map(|m| m.source(db))
                .expect("module should exist in graph");
            let spans = spans_map.get(salsa_module_id).cloned()
                .expect("spans should exist for module");

            module_specs.push(ModuleSpec::new(
                module_path.clone(),
                module_source,
                spans,
                parsed.clone(),
                *salsa_module_id,
            ));
        }

        ScriptCompilationContext {
            db,
            script_ctx,
            env: self.env,
            accumulated_unit_specs: Vec::new(),
            module_specs,
            interp: datalove_datafun_interp::IrInterpreter::new_with_debug_mode(debug_mode),
            func_id_map: self.func_id_map,
            last_source: None,
            last_batch_spec: None,
        }
    }
}

// ============================================================================
// Script compilation context
// ============================================================================

/// Incrementally compiles and executes script fragments and expressions.
///
/// Maintains state across evaluations: bindings from `let` and `var` statements
/// persist and can be used in subsequent expressions.
pub struct ScriptCompilationContext<'db> {
    db: &'db dyn salsa::Database,
    pub script_ctx: lower::ScriptLowerContext,
    pub env: ScriptEnvironment,
    accumulated_unit_specs: Vec<ScriptUnitSpec<'db>>,
    module_specs: Vec<ModuleSpec<'db>>,
    interp: datalove_datafun_interp::IrInterpreter,
    func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    last_source: Option<bct::input::Source>,
    last_batch_spec: Option<ScriptBatchSpec<'db>>,
}

impl<'db> ScriptCompilationContext<'db> {
    /// Compile and execute a script fragment (statements like `let x = 1`).
    pub fn eval_fragment(&mut self, source: &str) -> ScriptUnitResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).to_string()
                })
                .collect();
            return ScriptUnitResult {
                typecheck: TypecheckResult::ParseError { errors: parse_errors },
                lowering: LoweringResult::Skipped,
                ty: None,
                output: String::new(),
            };
        }

        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(src, spans, ScriptUnitKind::Fragment(parsed.clone()));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = create_batch_spec(
            self.db,
            src,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.process_fragment(parsed, tycheck_result)
    }

    /// Compile and execute an expression, returning its value.
    pub fn eval_expr(&mut self, source: &str) -> ScriptUnitResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        let parse_diags = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).to_string()
                })
                .collect();
            return ScriptUnitResult {
                typecheck: TypecheckResult::ParseError { errors: parse_errors },
                lowering: LoweringResult::Skipped,
                ty: None,
                output: String::new(),
            };
        }

        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(src, spans, ScriptUnitKind::Expr(expr));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = create_batch_spec(
            self.db,
            src,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.process_expr(expr, tycheck_result)
    }

    /// Lower a fragment to IR without executing.
    pub fn lower_fragment(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_fragment_impl(source, false)
    }

    /// Lower a fragment to IR for AOT compilation.
    pub fn lower_fragment_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_fragment_impl(source, true)
    }

    /// Lower an expression to IR without executing.
    pub fn lower_expr(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_expr_impl(source, false)
    }

    /// Lower an expression to IR for AOT compilation.
    pub fn lower_expr_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_expr_impl(source, true)
    }

    fn lower_fragment_impl(&mut self, source: &str, for_aot: bool) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).to_string()
                })
                .collect();
            return ScriptLowerResult {
                typecheck: TypecheckResult::ParseError { errors: parse_errors },
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            };
        }

        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(src, spans, ScriptUnitKind::Fragment(parsed.clone()));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = create_batch_spec(
            self.db,
            src,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.lower_fragment_inner(parsed, tycheck_result, for_aot)
    }

    fn lower_expr_impl(&mut self, source: &str, for_aot: bool) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        let parse_diags = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).to_string()
                })
                .collect();
            return ScriptLowerResult {
                typecheck: TypecheckResult::ParseError { errors: parse_errors },
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            };
        }

        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(src, spans, ScriptUnitKind::Expr(expr));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = create_batch_spec(
            self.db,
            src,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.lower_expr_inner(expr, tycheck_result, for_aot)
    }

    fn lower_fragment_inner(
        &mut self,
        parsed: datalove_datafun_ast::ast::ParsedStatements<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
        for_aot: bool,
    ) -> ScriptLowerResult {
        let tycheck_errors: Vec<_> = tycheck_result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !tycheck_errors.is_empty() {
            return ScriptLowerResult {
                typecheck: TypecheckResult::Error { errors: tycheck_errors },
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            };
        }

        let expr_types = tycheck_result.expr_types(self.db);
        let call_targets = tycheck_result.call_targets(self.db);
        let stmts = parsed.statements.to_vec();

        let func_analyses = match drop_analysis::analyze_script_functions(self.db, expr_types, call_targets, &stmts) {
            Ok(analyses) => analyses,
            Err(errors) => {
                // Format error messages for function-level drop analysis errors.
                let error_msgs: Vec<String> = errors.into_iter()
                    .map(|(func_name, errs)| {
                        format!("{}: {}", func_name, drop_analysis::format_analysis_errors(&errs))
                    })
                    .collect();
                return ScriptLowerResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error {
                        message: error_msgs.join("\n"),
                    },
                    ir_unit: None,
                };
            }
        };

        let ir_unit = match lower::lower_script_fragment_raw(
            self.db,
            expr_types,
            call_targets,
            &self.func_id_map,
            self.script_ctx.clone(),
            stmts,
            func_analyses,
            for_aot,
        ) {
            Ok(unit) => unit,
            Err(e) => {
                return ScriptLowerResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    ir_unit: None,
                };
            }
        };

        let ir_dump = format!("{}", ir_unit);

        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptLowerResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    fn lower_expr_inner(
        &mut self,
        expr: datalove_datafun_ast::ast::ExprFun<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
        for_aot: bool,
    ) -> ScriptLowerResult {
        let tycheck_errors: Vec<_> = tycheck_result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !tycheck_errors.is_empty() {
            return ScriptLowerResult {
                typecheck: TypecheckResult::Error { errors: tycheck_errors },
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            };
        }

        let ir_unit = match lower::lower_script_expr(
            self.db,
            tycheck_result.expr_types(self.db),
            tycheck_result.call_targets(self.db),
            &self.func_id_map,
            self.script_ctx.clone(),
            expr,
            for_aot,
        ) {
            Ok(unit) => unit,
            Err(e) => {
                return ScriptLowerResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    ir_unit: None,
                };
            }
        };

        let ir_dump = format!("{}", ir_unit);

        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptLowerResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    fn process_fragment(
        &mut self,
        parsed: datalove_datafun_ast::ast::ParsedStatements<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
    ) -> ScriptUnitResult {
        let tycheck_errors: Vec<_> = tycheck_result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !tycheck_errors.is_empty() {
            return ScriptUnitResult {
                typecheck: TypecheckResult::Error { errors: tycheck_errors },
                lowering: LoweringResult::Skipped,
                ty: None,
                output: String::new(),
            };
        }

        let expr_types = tycheck_result.expr_types(self.db);
        let call_targets = tycheck_result.call_targets(self.db);
        let stmts = parsed.statements.to_vec();

        let func_analyses = match drop_analysis::analyze_script_functions(self.db, expr_types, call_targets, &stmts) {
            Ok(analyses) => analyses,
            Err(errors) => {
                // Format error messages for function-level drop analysis errors.
                let error_msgs: Vec<String> = errors.into_iter()
                    .map(|(func_name, errs)| {
                        format!("{}: {}", func_name, drop_analysis::format_analysis_errors(&errs))
                    })
                    .collect();
                return ScriptUnitResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error {
                        message: error_msgs.join("\n"),
                    },
                    ty: None,
                    output: String::new(),
                };
            }
        };

        let call_targets = tycheck_result.call_targets(self.db);
        let ir_unit = match lower::lower_script_fragment_raw(
            self.db,
            expr_types,
            call_targets,
            &self.func_id_map,
            self.script_ctx.clone(),
            stmts,
            func_analyses,
            false,
        ) {
            Ok(unit) => unit,
            Err(e) => {
                return ScriptUnitResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    ty: None,
                    output: String::new(),
                };
            }
        };

        let ir_dump = format!("{}", ir_unit);

        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
        let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
        let ret_dest = datalove_datafun_interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        let output = match self.interp.execute_script_unit_in_env(&ir_unit, &mut self.env, ret_dest, None) {
            Ok(UnitCompletion::Normal) => "(fragment executed)".to_string(),
            Ok(UnitCompletion::EarlyReturn) => {
                let value = datalove_datafun_interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let output_str = self.interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));
                let _ = self.interp.destroy_value(&value);
                output_str
            }
            Err(e) => format!("Error: {:?}", e),
        };

        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptUnitResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ty: None,
            output,
        }
    }

    fn process_expr(
        &mut self,
        expr: datalove_datafun_ast::ast::ExprFun<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
    ) -> ScriptUnitResult {
        let tycheck_errors: Vec<_> = tycheck_result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !tycheck_errors.is_empty() {
            return ScriptUnitResult {
                typecheck: TypecheckResult::Error { errors: tycheck_errors },
                lowering: LoweringResult::Skipped,
                ty: None,
                output: String::new(),
            };
        }

        let ir_unit = match lower::lower_script_expr(
            self.db,
            tycheck_result.expr_types(self.db),
            tycheck_result.call_targets(self.db),
            &self.func_id_map,
            self.script_ctx.clone(),
            expr,
            false,
        ) {
            Ok(unit) => unit,
            Err(e) => {
                return ScriptUnitResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    ty: None,
                    output: String::new(),
                };
            }
        };

        let ir_dump = format!("{}", ir_unit);

        let result_ty = ir_unit.result
            .map(|id| format!("{}", &ir_unit.value_types[id.0 as usize]));

        let output = if let Some(result_id) = ir_unit.result {
            let ret_type = IrType::Result(Box::new(IrType::Unit));
            let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
            let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
            let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
            let ret_dest = datalove_datafun_interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            let expr_type = &ir_unit.value_types[result_id.0 as usize];
            let expr_tydesc = self.interp.tydesc_table_mut().get_or_create(expr_type);
            let (expr_size, expr_align) = unsafe { ((*expr_tydesc).size, (*expr_tydesc).align) };
            let mut expr_buffer = AlignedBuffer::with_align(expr_size as usize, expr_align as usize);
            let expr_dest = datalove_datafun_interp::Destination {
                ptr: expr_buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            };

            match self.interp.execute_script_unit_in_env(&ir_unit, &mut self.env, ret_dest, Some(expr_dest)) {
                Ok(UnitCompletion::Normal) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: expr_buffer.as_mut_ptr(),
                        tydesc: expr_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    let _ = self.interp.destroy_value(&value);
                    output_str
                }
                Ok(UnitCompletion::EarlyReturn) => {
                    let value = datalove_datafun_interp::Value {
                        ptr: ret_buffer.as_mut_ptr(),
                        tydesc: ret_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    let _ = self.interp.destroy_value(&value);
                    output_str
                }
                Err(e) => format!("Error: {:?}", e),
            }
        } else {
            "(fragment executed)".to_string()
        };

        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptUnitResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ty: result_ty,
            output,
        }
    }

    /// Get the type and value of a binding by name.
    pub fn get_binding(&mut self, name: &str) -> Option<(String, String)> {
        use datalove_datafun_interp::InterpError;

        if let Some((unit, value_id)) = self.script_ctx.values.get(name) {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".to_string());
            let val = match self.env.frames.external_value(*unit, *value_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedValue(_)) => "<moved>".to_string(),
                Err(e) => format!("<error: {:?}>", e),
            };
            return Some((ty, val));
        }

        if let Some((unit, slot_id)) = self.script_ctx.slots.get(name) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".to_string());
            let val = match self.env.frames.external_slot(*unit, *slot_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedSlot(_)) => "<moved>".to_string(),
                Err(e) => format!("<error: {:?}>", e),
            };
            return Some((ty, val));
        }

        None
    }

    /// Get all bindings as (name, kind, type, value) tuples.
    pub fn get_environment(&mut self) -> Vec<(String, String, String, String)> {
        use datalove_datafun_interp::InterpError;
        let mut result = Vec::new();

        for (name, (unit, value_id)) in &self.script_ctx.values {
            let ty = self.script_ctx.value_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".to_string());
            let val = match self.env.frames.external_value(*unit, *value_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedValue(_)) => "<moved>".to_string(),
                Err(e) => format!("<error: {:?}>", e),
            };
            result.push((name.clone(), "let".to_string(), ty, val));
        }

        for (name, (unit, slot_id)) in &self.script_ctx.slots {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".to_string());
            let val = match self.env.frames.external_slot(*unit, *slot_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedSlot(_)) => "<moved>".to_string(),
                Err(e) => format!("<error: {:?}>", e),
            };
            result.push((name.clone(), "var".to_string(), ty, val));
        }

        for (name, _) in &self.script_ctx.functions {
            result.push((name.clone(), "fun".to_string(), "function".to_string(), "-".to_string()));
        }

        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Get parse diagnostics from the last evaluation.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Get type diagnostics from the last evaluation.
    pub fn get_type_diagnostics(&self) -> Vec<&datalove_diagnostic::TypeDiagnostic> {
        if let Some(batch_spec) = self.last_batch_spec {
            type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(self.db, batch_spec)
        } else {
            Vec::new()
        }
    }

    /// Get the database reference.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// Get buffered debug output.
    pub fn get_debug_buffer(&self) -> String {
        self.interp.get_debug_buffer()
    }

    /// Clear buffered debug output.
    pub fn clear_debug_buffer(&self) {
        self.interp.clear_debug_buffer();
    }

    /// Set a call dispatcher for JIT compilation.
    pub fn set_call_dispatcher(&mut self, dispatcher: Box<dyn CallDispatcher>) {
        self.interp.set_call_dispatcher(dispatcher);
    }

    /// Destroy all allocated runtime values.
    pub fn destroy_all(&mut self) {
        self.env.destroy_all(self.interp.runtime_handle());
    }
}

// ============================================================================
// Script result types
// ============================================================================

/// Result of compiling and executing a script unit.
pub struct ScriptUnitResult {
    pub typecheck: TypecheckResult,
    pub lowering: LoweringResult,
    /// Type of the result expression (if any).
    pub ty: Option<String>,
    /// Pretty-printed output value or error message.
    pub output: String,
}

/// Result of lowering a script unit to IR (without execution).
pub struct ScriptLowerResult {
    pub typecheck: TypecheckResult,
    pub lowering: LoweringResult,
    pub ir_unit: Option<IrScriptUnit>,
}

// ============================================================================
// Helper functions
// ============================================================================

/// Check if a lowering result string represents an error.
pub fn is_lowering_error(s: &str) -> bool {
    s.starts_with("Error")
        || s.starts_with("Drop analysis error")
        || s.starts_with("Missing drop analysis")
}

/// Format lowering result from IR dumps.
pub fn format_module_lowering_result(
    ir_dumps: &[String],
    has_typecheck_errors: bool,
) -> LoweringResult {
    if has_typecheck_errors {
        return LoweringResult::Skipped;
    }

    let has_errors = ir_dumps.iter().any(|s| is_lowering_error(s));

    if has_errors {
        let errors: Vec<_> = ir_dumps.iter()
            .filter(|s| is_lowering_error(s))
            .cloned()
            .collect();
        LoweringResult::Error { message: errors.join("\n") }
    } else if ir_dumps.is_empty() {
        LoweringResult::Skipped
    } else {
        LoweringResult::Success { ir: ir_dumps.join("\n") }
    }
}

// ============================================================================
// AOT compilation
// ============================================================================

/// Ahead-of-time compilation to native executables.
pub mod aot {
    use rmx::prelude::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::OnceLock;

    use datalove_datafun_aot_cranelift::AotCompiler;
    use datalove_datafun_ir::{IrScriptUnit, FunctionRegistry, IrFunction};

    /// Linking error.
    #[derive(Debug)]
    pub enum LinkError {
        TempDir(std::io::Error),
        WriteObject(std::io::Error),
        RuntimeNotFound { debug_path: PathBuf, release_path: PathBuf },
        LinkerExec(std::io::Error),
        LinkerFailed(String),
    }

    impl std::fmt::Display for LinkError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                LinkError::TempDir(e) => write!(f, "failed to create temp directory: {}", e),
                LinkError::WriteObject(e) => write!(f, "failed to write object file: {}", e),
                LinkError::RuntimeNotFound { debug_path, release_path } => {
                    write!(f, "runtime library not found at {} or {}", debug_path.display(), release_path.display())
                }
                LinkError::LinkerExec(e) => write!(f, "failed to execute linker: {}", e),
                LinkError::LinkerFailed(msg) => write!(f, "linker failed: {}", msg),
            }
        }
    }

    impl std::error::Error for LinkError {}

    /// Execution error.
    #[derive(Debug)]
    pub enum ExecError {
        Exec(std::io::Error),
        ExitCode { code: i32, stderr: String },
    }

    impl std::fmt::Display for ExecError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ExecError::Exec(e) => write!(f, "failed to execute: {}", e),
                ExecError::ExitCode { code, stderr } => write!(f, "exit code {}: {}", code, stderr),
            }
        }
    }

    impl std::error::Error for ExecError {}

    /// Output from executing an AOT-compiled binary.
    pub struct ExecOutput {
        pub exit_code: i32,
        pub stdout: String,
        pub stderr: String,
    }

    static RUNTIME_LIB_DIR: OnceLock<PathBuf> = OnceLock::new();

    /// Ensure the runtime library is built and return its directory.
    pub fn ensure_runtime_lib() -> &'static Path {
        RUNTIME_LIB_DIR.get_or_init(|| {
            let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
                .unwrap_or_else(|_| ".".to_string());
            let manifest_path = PathBuf::from(manifest_dir);
            let workspace_root = manifest_path.join("../..").canonicalize()
                .expect("failed to find workspace root");
            let lib_dir = workspace_root.join("target/debug");

            // Build quietly to avoid polluting test output.
            let output = Command::new("cargo")
                .args(["build", "-p", "datalove-rt", "--quiet"])
                .current_dir(&workspace_root)
                .output()
                .expect("failed to run cargo build");
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                panic!("Failed to build datalove-rt: {}", stderr);
            }

            lib_dir
        })
    }

    /// Compile a script unit to object bytes.
    pub fn compile_script_to_object(unit: &IrScriptUnit) -> AnyResult<Vec<u8>> {
        let mut compiler = AotCompiler::new_for_host()
            .map_err(|e| anyhow!("failed to create AOT compiler: {}", e))?;
        let product = compiler.compile_script_unit(unit)
            .map_err(|e| anyhow!("AOT compilation failed: {}", e))?;
        let obj_bytes = product.emit()
            .map_err(|e| anyhow!("failed to emit object: {}", e))?;
        Ok(obj_bytes)
    }

    /// Compile a script unit with module functions to object bytes.
    pub fn compile_script_to_object_with_world<'a>(
        unit: &IrScriptUnit,
        world_funcs: impl Iterator<Item = &'a IrFunction>,
        registry: &FunctionRegistry,
    ) -> AnyResult<Vec<u8>> {
        let mut compiler = AotCompiler::new_for_host()
            .map_err(|e| anyhow!("failed to create AOT compiler: {}", e))?;
        let product = compiler.compile_script_unit_with_world_types(unit, world_funcs, registry)
            .map_err(|e| anyhow!("AOT compilation failed: {}", e))?;
        let obj_bytes = product.emit()
            .map_err(|e| anyhow!("failed to emit object: {}", e))?;
        Ok(obj_bytes)
    }

    /// Link object bytes to an executable in a temp directory.
    pub fn link_object_to_temp_executable(
        obj_bytes: &[u8],
    ) -> Result<(PathBuf, rmx::tempfile::TempDir), LinkError> {
        let dir = rmx::tempfile::tempdir().map_err(LinkError::TempDir)?;
        let exe_path = dir.path().join("script");
        link_object_to_path(obj_bytes, &exe_path)?;
        Ok((exe_path, dir))
    }

    /// Link object bytes to an executable at the specified path.
    pub fn link_object_to_path(obj_bytes: &[u8], output_path: &Path) -> Result<(), LinkError> {
        let dir = rmx::tempfile::tempdir().map_err(LinkError::TempDir)?;
        let obj_path = dir.path().join("script.o");
        std::fs::write(&obj_path, obj_bytes).map_err(LinkError::WriteObject)?;

        let lib_dir = ensure_runtime_lib();
        let lib_path = lib_dir.join("libdatalove_rt.a");

        let output = Command::new("cc")
            .args([
                obj_path.to_str().unwrap(),
                lib_path.to_str().unwrap(),
                "-ldl", "-lpthread", "-lm",
                "-o", output_path.to_str().unwrap(),
            ])
            .output()
            .map_err(LinkError::LinkerExec)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(LinkError::LinkerFailed(stderr));
        }

        Ok(())
    }

    /// Run an AOT-compiled executable.
    pub fn run_executable(exe_path: &Path) -> Result<ExecOutput, ExecError> {
        let output = Command::new(exe_path)
            .output()
            .map_err(ExecError::Exec)?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            return Err(ExecError::ExitCode { code: exit_code, stderr });
        }

        Ok(ExecOutput { exit_code, stdout, stderr })
    }

    /// Compile, link, and run a script unit.
    pub fn compile_link_run(unit: &IrScriptUnit) -> AnyResult<ExecOutput> {
        let obj_bytes = compile_script_to_object(unit)?;
        let (exe_path, _dir) = link_object_to_temp_executable(&obj_bytes)
            .map_err(|e| anyhow!("{}", e))?;
        run_executable(&exe_path).map_err(|e| anyhow!("{}", e))
    }

    /// Compile, link, and run a script unit with module functions.
    pub fn compile_link_run_with_world<'a>(
        unit: &IrScriptUnit,
        world_funcs: impl Iterator<Item = &'a IrFunction>,
        registry: &FunctionRegistry,
    ) -> AnyResult<ExecOutput> {
        let obj_bytes = compile_script_to_object_with_world(unit, world_funcs, registry)?;
        let (exe_path, _dir) = link_object_to_temp_executable(&obj_bytes)
            .map_err(|e| anyhow!("{}", e))?;
        run_executable(&exe_path).map_err(|e| anyhow!("{}", e))
    }
}
