//! Two-stage compilation pipeline: modules first, then scripts.
//!
//! # Overview
//!
//! Compilation proceeds in two stages:
//!
//! 1. **Module compilation** ([`ModuleCompilationPipeline`]): Parses, typechecks,
//!    analyzes drops, and lowers modules to IR. Produces [`CompiledModules`].
//!
//! 2. **Script execution** ([`ScriptCompilationContext`]): Incrementally compiles
//!    and executes script fragments against the compiled modules. Multiple
//!    independent script contexts can share the same module compilation.
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
//! let mut ctx = compiled.script_context(&db, DebugOutputMode::Stderr, None);
//! ctx.eval_fragment("let x = 42");
//! ctx.eval_expr("x + 1");
//! ```
//!
//! # Incremental Compilation
//!
//! After initial compilation with `compile_fresh` (requires `&db`), use
//! `update_source` and `compile` for incremental updates (requires `&mut db`).
//! The pipeline preserves salsa identity across updates, enabling memoization.
//!
//! ```ignore
//! let compiled1 = pipeline.compile_fresh(&db);
//!
//! pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
//! let (compiled2, db) = pipeline.compile(&mut db);
//! ```
//!
//! # Parallelism
//!
//! Set `DATALOVE_PARALLEL=1` to enable parallel compilation, or use the
//! `*_with_mode` methods for explicit control.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;
use datalove_datafun_ir::{IrModuleId, FuncId, IrType, IrScriptUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_compiler::ownership_analysis;
use datalove_datafun_compiler::ir_ext::IrTypeExt;
use datalove_datafun_compiler::compile::{
    ModuleCompilationInput, ModuleCompilationOutput,
    compile_modules as compiler_compile_modules,
};
use datalove_datafun_tycheck::{
    typecheck_module_graph,
    type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked,
    DbClone, ParallelMode, parallel_mode_from_env,
};
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult, ModuleId,
    ParsedModuleGraph,
};
use datalove_datafun_interp::{CallDispatcher, ModuleFunctionRegistry, ScriptEnvironment, UnitCompletion};
use datalove_rt::rust::AlignedBuffer;

use crate::incremental::{IncrementalModuleWorld, extract_dependencies};

// Re-export result types from compiler for backwards compatibility.
pub use datalove_datafun_compiler::compile::{TypecheckResult, LoweringResult};

// ============================================================================
// Module compilation pipeline
// ============================================================================

/// Module compilation pipeline with incremental recompilation support.
///
/// Compiles modules through parsing, typechecking, drop analysis, and IR lowering.
/// Use `compile_fresh` for initial compilation, then `compile` for incremental updates.
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
        let path_deps = extract_dependencies(&self.world, db.as_salsa_db());
        let (module_graph, resolved_requires) = self.world.build_fresh(db.as_salsa_db(), &path_deps);
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
        // Extract dependencies first (reads from db).
        let path_deps = extract_dependencies(&self.world, db);
        let (module_graph, resolved_requires) = self.world.prepare_for_compile(db, &path_deps);

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
        // Delegate to compiler for parsing, typechecking, and lowering.
        let input = ModuleCompilationInput {
            graph: module_graph,
            resolved_requires,
        };
        let output = compiler_compile_modules(db, input, mode);

        // Build ModuleFunctionRegistry from IR functions (interpreter-specific).
        self.wrap_compiler_output(db, output)
    }

    /// Wrap compiler output with interpreter-specific structures.
    fn wrap_compiler_output<'db>(
        &self,
        db: &'db dyn DbClone,
        output: ModuleCompilationOutput<'db>,
    ) -> CompiledModules<'db> {
        // Convert FuncIdMap to HashMap.
        let func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)> =
            output.lowering_result.func_id_map(db.as_salsa_db()).to_hashmap(db.as_salsa_db());

        // Build module registry from IR functions.
        let mut module_registry = ModuleFunctionRegistry::new();
        for (_module_id, result) in output.lowering_result.module_results(db.as_salsa_db()) {
            let ir_module_id = result.ir_module_id(db.as_salsa_db());
            for ir_func in result.functions(db.as_salsa_db()) {
                module_registry.add_module_function(ir_module_id, ir_func.id, ir_func.clone());
            }
        }

        let shared = Arc::new(SharedModuleContext {
            module_graph: output.module_graph,
            parsed_graph: output.parsed_graph,
            graph_typecheck: output.typecheck_result,
            func_id_map,
            module_registry: Arc::new(module_registry),
        });

        CompiledModules {
            shared,
            resolution_error: None,
            path_to_errors: output.typecheck_errors,
            ownership_errors: output.ownership_errors,
            lowering_errors: output.lowering_errors,
            module_ir_dumps: output.module_ir_dumps,
        }
    }
}

impl Default for ModuleCompilationPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Shared module context
// ============================================================================

/// Compiled module data shared across script contexts.
///
/// Contains the module graph, typecheck results, and function registry.
/// Thread-safe via `Arc` wrapping.
pub struct SharedModuleContext<'db> {
    pub module_graph: ModuleGraph,
    pub parsed_graph: ParsedModuleGraph<'db>,
    pub graph_typecheck: ModuleGraphTypecheckResult<'db>,
    pub func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    pub module_registry: Arc<ModuleFunctionRegistry>,
}

// ============================================================================
// Compiled modules
// ============================================================================

/// Result of module compilation.
///
/// Call `script_context()` to create script execution contexts. Multiple
/// independent contexts can share the same compilation.
pub struct CompiledModules<'db> {
    pub shared: Arc<SharedModuleContext<'db>>,
    pub resolution_error: Option<String>,
    pub path_to_errors: BTreeMap<String, Vec<String>>,
    pub ownership_errors: BTreeMap<String, Vec<String>>,
    pub lowering_errors: BTreeMap<String, Vec<String>>,
    pub module_ir_dumps: BTreeMap<String, Vec<String>>,
}

impl<'db> CompiledModules<'db> {
    /// Check if compilation succeeded.
    pub fn is_successful(&self) -> bool {
        self.resolution_error.is_none()
            && self.path_to_errors.values().all(|errors| errors.is_empty())
            && self.ownership_errors.values().all(|errors| errors.is_empty())
            && self.lowering_errors.values().all(|errors| errors.is_empty())
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

        for error_list in self.ownership_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        for error_list in self.lowering_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        errors
    }

    /// Get all typecheck errors.
    pub fn all_typecheck_errors(&self) -> Vec<String> {
        self.path_to_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all ownership analysis errors.
    pub fn all_ownership_errors(&self) -> Vec<String> {
        self.ownership_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all lowering errors.
    pub fn all_lowering_errors(&self) -> Vec<String> {
        self.lowering_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get module type diagnostics with spans for rendering.
    pub fn get_module_type_diagnostics(&self, db: &'db dyn salsa::Database) -> Vec<&'db datalove_diagnostic::TypeDiagnostic> {
        typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, self.shared.parsed_graph)
    }

    /// Create a script execution context.
    ///
    /// Can be called multiple times to create independent contexts sharing the
    /// same module compilation. The optional `call_dispatcher` enables JIT or
    /// custom call dispatch.
    pub fn script_context(
        &self,
        db: &'db dyn salsa::Database,
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> ScriptCompilationContext<'db> {
        let script_ctx = lower::ScriptLowerContext::new();
        let mut module_specs = Vec::new();

        // Build a map of spans for quick lookup.
        let spans_map: std::collections::HashMap<_, _> = self.shared.parsed_graph.spans(db).iter()
            .map(|(id, spans)| (*id, spans.clone()))
            .collect();

        for (salsa_module_id, parsed) in self.shared.parsed_graph.statements_only(db) {
            let module_path = salsa_module_id.path(db).clone();
            let module_source = self.shared.module_graph.iter_modules(db)
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

        // Create a new ScriptEnvironment that shares the module registry.
        let env = ScriptEnvironment::with_module_registry(Arc::clone(&self.shared.module_registry));

        ScriptCompilationContext {
            db,
            script_ctx,
            env,
            accumulated_unit_specs: Vec::new(),
            module_specs,
            interp: datalove_datafun_interp::IrInterpreter::new_with_options(debug_mode, call_dispatcher),
            func_id_map: self.shared.func_id_map.clone(),
            last_source: None,
            last_batch_spec: None,
        }
    }
}

// ============================================================================
// Script compilation context
// ============================================================================

/// Script execution context with persistent bindings.
///
/// Compiles and executes script fragments (`eval_fragment`) and expressions
/// (`eval_expr`). Bindings from `let` and `var` statements persist across
/// evaluations.
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
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).S()
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
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        let parse_diags = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).S()
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
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed;

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).S()
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
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        let parse_diags = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if !parse_diags.is_empty() {
            let parse_errors: Vec<String> = parse_diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(self.db);
                    diag.message.as_str(self.db).S()
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

        // Build map of function name -> resolved param types for type alias support.
        let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
        for (name, func_type) in tycheck_result.function_types(self.db) {
            let param_types: Vec<IrType> = func_type.param_types(self.db)
                .iter()
                .map(|ty| IrType::from_tycheck(self.db, ty))
                .collect();
            func_param_types.insert(name.text(self.db).S(), param_types);
        }

        let func_analyses = match ownership_analysis::analyze_script_functions(self.db, expr_types, call_targets, &stmts, Some(&func_param_types)) {
            Ok(analyses) => analyses,
            Err(errors) => {
                // Format error messages for function-level drop analysis errors.
                let error_msgs: Vec<String> = errors.into_iter()
                    .map(|(func_name, errs)| {
                        format!("{}: {}", func_name, ownership_analysis::format_analysis_errors(&errs))
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
            Some(&func_param_types),
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

        // Build map of function name -> resolved param types for type alias support.
        let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
        for (name, func_type) in tycheck_result.function_types(self.db) {
            let param_types: Vec<IrType> = func_type.param_types(self.db)
                .iter()
                .map(|ty| IrType::from_tycheck(self.db, ty))
                .collect();
            func_param_types.insert(name.text(self.db).S(), param_types);
        }

        let func_analyses = match ownership_analysis::analyze_script_functions(self.db, expr_types, call_targets, &stmts, Some(&func_param_types)) {
            Ok(analyses) => analyses,
            Err(errors) => {
                // Format error messages for function-level drop analysis errors.
                let error_msgs: Vec<String> = errors.into_iter()
                    .map(|(func_name, errs)| {
                        format!("{}: {}", func_name, ownership_analysis::format_analysis_errors(&errs))
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
            Some(&func_param_types),
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
            Ok(UnitCompletion::Normal) => "(fragment executed)".S(),
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
            "(fragment executed)".S()
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
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_value(*unit, *value_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedValue(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            return Some((ty, val));
        }

        if let Some((unit, slot_id)) = self.script_ctx.slots.get(name) {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_slot(*unit, *slot_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedSlot(_)) => "<moved>".S(),
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
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_value(*unit, *value_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedValue(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            result.push((name.C(), "let".S(), ty, val));
        }

        for (name, (unit, slot_id)) in &self.script_ctx.slots {
            let ty = self.script_ctx.slot_types.get(name)
                .map(|t| format!("{}", t))
                .unwrap_or_else(|| "?".S());
            let val = match self.env.frames.external_slot(*unit, *slot_id) {
                Ok(v) => self.interp.pretty_print_value(&v)
                    .unwrap_or_else(|e| format!("<print error: {:?}>", e)),
                Err(InterpError::UninitializedSlot(_)) => "<moved>".S(),
                Err(e) => format!("<error: {:?}>", e),
            };
            result.push((name.C(), "var".S(), ty, val));
        }

        for (name, _) in &self.script_ctx.functions {
            result.push((name.C(), "fun".S(), "function".S(), "-".S()));
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

    /// Destroy all allocated runtime values.
    pub fn destroy_all(&mut self) {
        self.env.destroy_all(self.interp.runtime_handle());
    }
}

// ============================================================================
// Script result types
// ============================================================================

/// Result of `eval_fragment` or `eval_expr`.
pub struct ScriptUnitResult {
    pub typecheck: TypecheckResult,
    pub lowering: LoweringResult,
    /// Type of the result expression, if any.
    pub ty: Option<String>,
    /// Pretty-printed output value or execution error.
    pub output: String,
}

/// Result of `lower_fragment` or `lower_expr` (IR without execution).
pub struct ScriptLowerResult {
    pub typecheck: TypecheckResult,
    pub lowering: LoweringResult,
    /// The lowered IR unit, if successful.
    pub ir_unit: Option<IrScriptUnit>,
}

// Re-export helper functions from compiler.
pub use datalove_datafun_compiler::compile::format_lowering_result;

// ============================================================================
// AOT compilation
// ============================================================================

/// AOT compilation: compile scripts to native executables via Cranelift.
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
            // Use index-64 feature if this crate was compiled with it.
            #[cfg(feature = "index-64")]
            let args = ["build", "-p", "datalove-rt", "--features", "index-64", "--quiet"];
            #[cfg(not(feature = "index-64"))]
            let args = ["build", "-p", "datalove-rt", "--quiet"];

            let output = Command::new("cargo")
                .args(args)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_rt::c::DebugOutputMode;

    fn make_db() -> crate::Database {
        crate::Database::default()
    }

    /// Test creating multiple script contexts from the same compiled modules.
    #[test]
    fn test_multiple_script_contexts() {
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        // Compile with no modules - just testing script context isolation.
        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful(), "compilation should succeed");

        // Create first script context.
        let mut ctx1 = compiled.script_context(&db, DebugOutputMode::Disabled, None);
        let result1 = ctx1.eval_fragment("let x = @10");
        assert!(matches!(result1.typecheck, TypecheckResult::Success), "ctx1 fragment should typecheck");

        // Create second script context from the same compiled modules.
        let mut ctx2 = compiled.script_context(&db, DebugOutputMode::Disabled, None);
        let result2 = ctx2.eval_fragment("let y = @20");
        assert!(matches!(result2.typecheck, TypecheckResult::Success), "ctx2 fragment should typecheck");

        // Each context should have independent state.
        let expr1 = ctx1.eval_expr("x");
        assert_eq!(expr1.output, "@10");

        let expr2 = ctx2.eval_expr("y");
        assert_eq!(expr2.output, "@20");

        // ctx1 should not see y, ctx2 should not see x.
        let bad1 = ctx1.eval_expr("y");
        assert!(matches!(bad1.typecheck, TypecheckResult::Error { .. }), "ctx1 should not see y");

        let bad2 = ctx2.eval_expr("x");
        assert!(matches!(bad2.typecheck, TypecheckResult::Error { .. }), "ctx2 should not see x");

        ctx1.destroy_all();
        ctx2.destroy_all();
    }

    /// Test interleaved execution of script units from multiple contexts.
    #[test]
    fn test_interleaved_execution() {
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful());

        let mut ctx_a = compiled.script_context(&db, DebugOutputMode::Disabled, None);
        let mut ctx_b = compiled.script_context(&db, DebugOutputMode::Disabled, None);

        // Define simple identity functions in each context.
        let r1 = ctx_a.eval_fragment("fun id_a(n: @u32): @u32\n  ret n\nend fun");
        assert!(matches!(r1.typecheck, TypecheckResult::Success), "ctx_a fn def failed: {:?}", r1.typecheck);

        let r2 = ctx_b.eval_fragment("fun id_b(n: @u32): @u32\n  ret n\nend fun");
        assert!(matches!(r2.typecheck, TypecheckResult::Success), "ctx_b fn def failed: {:?}", r2.typecheck);

        // Interleave: A defines values, B defines values.
        let _ = ctx_a.eval_fragment("let a1: @u32 = @5");
        let _ = ctx_b.eval_fragment("let b1: @u32 = @10");

        // A uses its function, B uses its function.
        let _ = ctx_a.eval_fragment("let a2 = id_a(a1)");
        let _ = ctx_b.eval_fragment("let b2 = id_b(b1)");

        // Verify values.
        let result_a = ctx_a.eval_expr("a2");
        assert_eq!(result_a.output, "@5");

        let result_b = ctx_b.eval_expr("b2");
        assert_eq!(result_b.output, "@10");

        // Each context's function is isolated.
        let bad_a = ctx_a.eval_expr("id_b(@1)");
        assert!(matches!(bad_a.typecheck, TypecheckResult::Error { .. }), "ctx_a should not see id_b");

        let bad_b = ctx_b.eval_expr("id_a(@1)");
        assert!(matches!(bad_b.typecheck, TypecheckResult::Error { .. }), "ctx_b should not see id_a");

        ctx_a.destroy_all();
        ctx_b.destroy_all();
    }

    /// Test running scripts in parallel using threads with shared compiled modules.
    ///
    /// Compiles modules once, then shares the compilation across threads.
    /// Each thread gets a cloned database via `DbClone::dyn_clone()` and creates
    /// its own script context from the shared compiled modules.
    #[test]
    fn test_parallel_script_execution() {
        use std::sync::Arc;
        use std::thread;

        // Compile once on the main thread.
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        // Add a module with a function all threads will use.
        pipeline.add_module(&db, "local", "pkg", "math", r#"
fun square(n: int): int
  ret n * n
end fun
"#);

        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful(), "compilation failed: {:?}", compiled.all_errors());

        // Clone the compiled modules into an Arc for sharing.
        // (CompiledModules already has Arc<SharedModuleContext> internally.)
        let compiled = Arc::new(compiled);

        // Clone databases upfront - one per thread for parallel execution.
        // Can't clone inside parallel section since &dyn DbClone isn't Sync.
        let work: Vec<_> = (0..4)
            .map(|i| (db.dyn_clone(), Arc::clone(&compiled), i))
            .collect();

        let results = std::sync::Mutex::new(Vec::new());

        thread::scope(|s| {
            for (db_clone, compiled, i) in work {
                let results = &results;
                s.spawn(move || {
                    // Use the cloned database for this thread.
                    let db_ref = db_clone.as_salsa_db();

                    // Create a script context from the shared compiled modules.
                    let mut ctx = compiled.script_context(db_ref, DebugOutputMode::Disabled, None);

                    // Import and use the shared module function.
                    let r = ctx.eval_fragment("require module local/pkg/math\nimport math.square");
                    assert!(matches!(r.typecheck, TypecheckResult::Success),
                        "import failed: {:?}", r.typecheck);

                    // Define local variable and compute.
                    let val = (i + 1) * 10;
                    let _ = ctx.eval_fragment(&format!("let n: int = @{}", val));
                    let _ = ctx.eval_fragment("let result = square(n)");

                    let result = ctx.eval_expr("result");
                    ctx.destroy_all();

                    let expected = val * val;
                    assert_eq!(result.output, format!("@{}", expected),
                        "thread {} expected {} but got {}", i, expected, result.output);
                    results.lock().unwrap().push((i, expected));
                });
            }
        });

        // Verify all threads completed.
        let mut results = results.into_inner().unwrap();
        results.sort_by_key(|(i, _)| *i);
        let values: Vec<_> = results.into_iter().map(|(_, v)| v).collect();
        assert_eq!(values, vec![100, 400, 900, 1600]);
    }

    /// Test compile-run-compile-run pattern with incremental compilation.
    #[test]
    fn test_compile_run_compile_run() {
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        // First compilation: add a module with a simple identity function.
        pipeline.add_module(&db, "local", "pkg", "v1", r#"
fun value(x: @u32): @u32
  ret x
end fun
"#);

        let compiled1 = pipeline.compile_fresh(&db);
        assert!(compiled1.is_successful(), "first compilation failed: {:?}", compiled1.all_errors());

        // First run: import and call module function.
        {
            let mut ctx = compiled1.script_context(&db, DebugOutputMode::Disabled, None);
            let r = ctx.eval_fragment("require module local/pkg/v1\nimport v1.value");
            assert!(matches!(r.typecheck, TypecheckResult::Success), "import failed: {:?}", r.typecheck);
            let result = ctx.eval_expr("value(@100)");
            assert_eq!(result.output, "@100");
            ctx.destroy_all();
        }

        // Create a second context from the same compilation.
        {
            let mut ctx2 = compiled1.script_context(&db, DebugOutputMode::Disabled, None);
            let r = ctx2.eval_fragment("require module local/pkg/v1\nimport v1.value");
            assert!(matches!(r.typecheck, TypecheckResult::Success), "second import failed: {:?}", r.typecheck);
            let result = ctx2.eval_expr("value(@200)");
            assert_eq!(result.output, "@200");
            ctx2.destroy_all();
        }
    }

    /// Test that unit functions are isolated between contexts.
    #[test]
    fn test_isolated_unit_functions() {
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful());

        let mut ctx1 = compiled.script_context(&db, DebugOutputMode::Disabled, None);
        let mut ctx2 = compiled.script_context(&db, DebugOutputMode::Disabled, None);

        // Define functions with the same name returning different values.
        let r1_def = ctx1.eval_fragment("fun local_fn(x: @u32): @u32\n  ret @10\nend fun");
        assert!(matches!(r1_def.typecheck, TypecheckResult::Success), "ctx1 fn def failed: {:?}", r1_def.typecheck);

        let r2_def = ctx2.eval_fragment("fun local_fn(x: @u32): @u32\n  ret @20\nend fun");
        assert!(matches!(r2_def.typecheck, TypecheckResult::Success), "ctx2 fn def failed: {:?}", r2_def.typecheck);

        // Local functions are isolated to their context.
        let r1_local = ctx1.eval_expr("local_fn(@5)");
        assert!(matches!(r1_local.typecheck, TypecheckResult::Success), "ctx1 fn call failed: {:?}", r1_local.typecheck);
        assert_eq!(r1_local.output, "@10");

        let r2_local = ctx2.eval_expr("local_fn(@5)");
        assert!(matches!(r2_local.typecheck, TypecheckResult::Success), "ctx2 fn call failed: {:?}", r2_local.typecheck);
        assert_eq!(r2_local.output, "@20");

        ctx1.destroy_all();
        ctx2.destroy_all();
    }

    /// Test creating many script contexts doesn't cause issues.
    #[test]
    fn test_many_script_contexts() {
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful());

        // Create many contexts.
        for i in 0..20u32 {
            let mut ctx = compiled.script_context(&db, DebugOutputMode::Disabled, None);
            let r = ctx.eval_fragment("fun id(x: @u32): @u32\n  ret x\nend fun");
            assert!(matches!(r.typecheck, TypecheckResult::Success), "fn def failed: {:?}", r.typecheck);
            let result = ctx.eval_expr(&format!("id(@{})", i));
            assert_eq!(result.output, format!("@{}", i));
            ctx.destroy_all();
        }
    }

    /// Test that module functions are shared across contexts.
    #[test]
    fn test_shared_module_functions() {
        let db = make_db();
        let mut pipeline = ModuleCompilationPipeline::new();

        pipeline.add_module(&db, "local", "pkg", "math", r#"
fun id(x: @u32): @u32
  ret x
end fun
"#);

        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful(), "compilation failed: {:?}", compiled.all_errors());

        // Create two contexts that both use the module function.
        let mut ctx1 = compiled.script_context(&db, DebugOutputMode::Disabled, None);
        let mut ctx2 = compiled.script_context(&db, DebugOutputMode::Disabled, None);

        // Both contexts import and use the module function.
        let r1 = ctx1.eval_fragment("require module local/pkg/math\nimport math.id");
        assert!(matches!(r1.typecheck, TypecheckResult::Success), "ctx1 import failed: {:?}", r1.typecheck);
        let r2 = ctx2.eval_fragment("require module local/pkg/math\nimport math.id");
        assert!(matches!(r2.typecheck, TypecheckResult::Success), "ctx2 import failed: {:?}", r2.typecheck);

        let r1 = ctx1.eval_expr("id(@10)");
        assert_eq!(r1.output, "@10");

        let r2 = ctx2.eval_expr("id(@20)");
        assert_eq!(r2.output, "@20");

        ctx1.destroy_all();
        ctx2.destroy_all();
    }
}
