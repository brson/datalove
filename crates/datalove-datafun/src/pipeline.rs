//! Datafun compilation pipeline.
//!
//! Two-stage compilation: modules first, then scripts.
//!
//! 1. [`ModuleCompilationPipeline`] compiles module definitions through parsing,
//!    typechecking, drop analysis, and IR lowering.
//!
//! 2. [`ScriptCompilationContext`] incrementally compiles and executes script
//!    units against those modules.
//!
//! # Example
//!
//! ```ignore
//! let mut pipeline = ModuleCompilationPipeline::new(&db);
//! pipeline.add_module("local", "mypackage", "main", source);
//! let compiled = pipeline.compile();
//!
//! let mut ctx = compiled.script_context(&db, DebugOutputMode::Stderr);
//! ctx.eval_fragment("let x = 42");
//! ctx.eval_expr("x + 1");
//! ```

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::{BTreeMap, HashMap};

use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_ir::{IrModuleId, FuncId, IrType, IrScriptUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_compiler::drop_analysis;
use datalove_datafun_tycheck::{
    typecheck_module_graph, type_check_script_units, create_batch_spec,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked,
};
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult, ModuleId,
    ParsedModuleGraph, parse_module_graph,
};
use datalove_datafun_interp::{CallDispatcher, ScriptEnvironment, UnitCompletion};
use datalove_rt::rust::AlignedBuffer;
use drop_analysis::FunctionDropAnalysis;

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

/// Pipeline for compiling modules to IR.
///
/// Runs three compilation phases:
/// 1. Build module graph, resolve imports, typecheck
/// 2. Run drop analysis on all functions
/// 3. Lower all functions to IR
pub struct ModuleCompilationPipeline<'db> {
    db: &'db dyn salsa::Database,
    pkglib_system: BTreeMap<String, Package>,
    pkglib_local: BTreeMap<String, Package>,
}

impl<'db> ModuleCompilationPipeline<'db> {
    /// Create a new pipeline.
    pub fn new(db: &'db dyn salsa::Database) -> Self {
        Self {
            db,
            pkglib_system: BTreeMap::new(),
            pkglib_local: BTreeMap::new(),
        }
    }

    /// Create from worldfile sections.
    pub fn from_sections(
        db: &'db dyn salsa::Database,
        sections: &[WorldfileSection],
    ) -> Self {
        let mut pipeline = Self::new(db);
        pipeline.add_modules_from_sections(sections);
        pipeline
    }

    /// Add a module to the pipeline.
    pub fn add_module(&mut self, library: &str, package: &str, module: &str, source: &str) {
        let library_map = match library {
            "sys" => &mut self.pkglib_system,
            "local" => &mut self.pkglib_local,
            _ => return,
        };

        let pkg = library_map.entry(package.to_string())
            .or_insert_with(|| Package {
                name: package.to_string(),
                modules: BTreeMap::new(),
            });

        let module_path = format!("{}/{}/{}", library, package, module);
        let pkg_module = PackageModule {
            name: module.to_string(),
            path: module_path.into(),
            text: source.to_string(),
        };
        pkg.modules.insert(module.to_string(), pkg_module);
    }

    /// Add modules from worldfile sections.
    pub fn add_modules_from_sections(&mut self, sections: &[WorldfileSection]) {
        for section in sections {
            if let WorldfileSection::Module { library, package, module, source } = section {
                self.add_module(library, package, module, source);
            }
        }
    }

    /// Load sys library from directory.
    pub async fn load_sys_library_from_dir(
        &mut self,
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
                self.add_module("sys", pkg_name, mod_name, &pkg_module.text);
            }
        }

        Ok(())
    }

    /// Load sys library from default location.
    pub async fn load_sys_library_default(&mut self) -> AnyResult<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = std::path::PathBuf::from(manifest_dir);
        let parent = manifest_path.parent()
            .ok_or_else(|| anyhow!("Failed to get parent directory"))?;
        let grandparent = parent.parent()
            .ok_or_else(|| anyhow!("Failed to get grandparent directory"))?;
        let sys_dir = grandparent.join("sys");

        self.load_sys_library_from_dir(sys_dir).await
    }

    /// Get local packages (for validation).
    pub fn pkglib_local(&self) -> &BTreeMap<String, Package> {
        &self.pkglib_local
    }

    /// Compile all modules.
    pub fn compile(self) -> CompiledModules<'db> {
        // Phase 1: Build module graph and typecheck.
        let raw_package_world = datalove_datafun_pkg::package_load::PackageWorld {
            pkglib_system: self.pkglib_system,
            pkglib_local: self.pkglib_local,
        };
        let package_world = datalove_datafun_pkg::import_from_loader(self.db, raw_package_world);

        let resolution = crate::package_resolve::resolve_package_world_with_imports(self.db, package_world);

        let pkg_graph = match resolution.result(self.db) {
            Ok(graph) => graph,
            Err(e) => {
                let empty_graph = datalove_datafun_compiler::module_graph::ModuleGraphBuilder::new(self.db).build();
                let empty_parsed = parse_module_graph(self.db, empty_graph.clone(), BTreeMap::new());
                return CompiledModules {
                    resolution_error: Some(format!("Package resolution failed: {:?}", e)),
                    module_graph: empty_graph,
                    parsed_graph: empty_parsed,
                    graph_typecheck: typecheck_module_graph(self.db, empty_parsed),
                    path_to_errors: BTreeMap::new(),
                    drop_analysis_errors: BTreeMap::new(),
                    func_id_map: HashMap::new(),
                    env: ScriptEnvironment::new(),
                    module_lowering_results: BTreeMap::new(),
                };
            }
        };

        let graph_with_requires = datalove_datafun_pkg::to_module_graph(self.db, package_world, pkg_graph);
        let module_graph = graph_with_requires.graph;
        let parsed_graph = parse_module_graph(self.db, module_graph.clone(), graph_with_requires.resolved_requires);

        let graph_typecheck = typecheck_module_graph(self.db, parsed_graph);
        let combined_expr_types = graph_typecheck.expr_types(self.db);
        let combined_call_targets = graph_typecheck.call_targets(self.db);

        // Collect typecheck errors.
        let module_errors = graph_typecheck.module_errors(self.db);
        let mut path_to_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (module_id, errors) in module_errors {
            let path = module_id.path(self.db).clone();
            let error_strings: Vec<String> = errors.iter()
                .map(|e| format!("{}: {:?}", path, e))
                .collect();
            path_to_errors.insert(path, error_strings);
        }

        // Assign function IDs.
        let mut func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)> = HashMap::new();
        let mut next_func_id: u32 = 0;
        for (ir_module_idx, module) in module_graph.iter_modules(self.db).enumerate() {
            let ir_module_id = IrModuleId(ir_module_idx as u32);
            let salsa_module_id = module.id(self.db);
            let module_source = module.source(self.db);
            let parse_result = datalove_datafun_parser::parse(self.db, module_source);
            let parsed = parse_result.parsed(self.db);
            for statement in parsed.statements(self.db) {
                if let datalove_datafun_ast::ast::Statement::Fun(func) = statement {
                    let func_name = func.name(self.db).text(self.db).to_string();
                    let func_id = FuncId(next_func_id);
                    next_func_id += 1;
                    func_id_map.insert((salsa_module_id, func_name), (ir_module_id, func_id));
                }
            }
        }

        // Phase 2: Drop analysis.
        let mut drop_analyses: HashMap<(String, String), FunctionDropAnalysis> = HashMap::new();
        let mut drop_analysis_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for module in module_graph.iter_modules(self.db) {
            let salsa_module_id = module.id(self.db);
            let module_path = salsa_module_id.path(self.db).clone();

            if path_to_errors.get(&module_path).map_or(false, |e| !e.is_empty()) {
                continue;
            }

            let module_source = module.source(self.db);
            let parse_result = datalove_datafun_parser::parse(self.db, module_source);
            let parsed = parse_result.parsed(self.db);

            for statement in parsed.statements(self.db) {
                if let datalove_datafun_ast::ast::Statement::Fun(func) = statement {
                    let func_name = func.name(self.db).text(self.db).to_string();
                    let analysis = drop_analysis::analyze_function(self.db, *func, combined_expr_types, combined_call_targets);

                    if !analysis.errors.is_empty() {
                        let error_msgs: Vec<String> = analysis.errors.iter()
                            .map(|e| format!("{:?}", e))
                            .collect();
                        let key = format!("{}/{}", module_path, func_name);
                        drop_analysis_errors.insert(key, error_msgs);
                    } else {
                        drop_analyses.insert((module_path.clone(), func_name), analysis);
                    }
                }
            }
        }

        // Phase 3: Lower to IR.
        let mut env = ScriptEnvironment::new();
        let mut module_lowering_results: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for (ir_module_idx, module) in module_graph.iter_modules(self.db).enumerate() {
            let ir_module_id = IrModuleId(ir_module_idx as u32);
            let salsa_module_id = module.id(self.db);
            let module_path = salsa_module_id.path(self.db).clone();

            if path_to_errors.get(&module_path).map_or(false, |e| !e.is_empty()) {
                continue;
            }

            let module_source = module.source(self.db);
            let parse_result = datalove_datafun_parser::parse(self.db, module_source);
            let parsed = parse_result.parsed(self.db);

            let mut ir_dumps = Vec::new();

            for statement in parsed.statements(self.db) {
                if let datalove_datafun_ast::ast::Statement::Fun(func) = statement {
                    let func_name = func.name(self.db).text(self.db).to_string();

                    let drop_key = format!("{}/{}", module_path, func_name);
                    if drop_analysis_errors.contains_key(&drop_key) {
                        ir_dumps.push(format!("Drop analysis error in {}", func_name));
                        continue;
                    }

                    let analysis = match drop_analyses.get(&(module_path.clone(), func_name.clone())) {
                        Some(a) => a.clone(),
                        None => {
                            ir_dumps.push(format!("Missing drop analysis for {}", func_name));
                            continue;
                        }
                    };

                    let (_, func_id) = func_id_map.get(&(salsa_module_id, func_name.clone())).unwrap();
                    let call_targets = graph_typecheck.call_targets(self.db);

                    match lower::lower_function_for_module(
                        self.db, combined_expr_types, call_targets, &func_id_map, *func, analysis
                    ) {
                        Ok(ir_func) => {
                            ir_dumps.push(format!("{}", ir_func));
                            env.add_module_function(ir_module_id, *func_id, ir_func);
                        }
                        Err(e) => {
                            ir_dumps.push(format!("Error lowering {}: {}", func_name, e));
                        }
                    }
                }
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

// ============================================================================
// Compiled modules
// ============================================================================

/// Result of module compilation.
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

    /// Create a script compilation context.
    ///
    /// The `debug_mode` controls debuglog behavior:
    /// - `Disabled`: no output
    /// - `Stderr`: output to stderr
    /// - `Buffer`: output to internal buffer (for testing)
    pub fn script_context(
        self,
        db: &'db dyn salsa::Database,
        debug_mode: datalove_rt::c::DebugOutputMode,
    ) -> ScriptCompilationContext<'db> {
        let script_ctx = lower::ScriptLowerContext::new();
        let mut module_specs = Vec::new();

        for (salsa_module_id, parsed, spans) in self.parsed_graph.parsed_statements(db) {
            let module_path = salsa_module_id.path(db).clone();
            let module_source = self.module_graph.iter_modules(db)
                .find(|m| m.id(db) == *salsa_module_id)
                .map(|m| m.source(db))
                .expect("module should exist in graph");

            module_specs.push(ModuleSpec::new(
                module_path.clone(),
                module_source,
                spans.clone(),
                *parsed,
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

/// Context for incremental script compilation and execution.
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
    // --- Evaluation (compile + execute) ---

    /// Compile and execute a script fragment.
    pub fn eval_fragment(&mut self, source: &str) -> ScriptUnitResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed(self.db);

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
        let unit_spec = ScriptUnitSpec::new(src, spans, ScriptUnitKind::Fragment(parsed));
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

    /// Compile and execute a script expression.
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

    // --- Lowering only (for AOT) ---

    /// Lower a fragment without executing.
    pub fn lower_fragment(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_fragment_impl(source, false)
    }

    /// Lower a fragment for AOT (emits drops for script bindings).
    pub fn lower_fragment_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_fragment_impl(source, true)
    }

    /// Lower an expression without executing.
    pub fn lower_expr(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_expr_impl(source, false)
    }

    /// Lower an expression for AOT.
    pub fn lower_expr_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        self.lower_expr_impl(source, true)
    }

    fn lower_fragment_impl(&mut self, source: &str, for_aot: bool) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed(self.db);

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
        let unit_spec = ScriptUnitSpec::new(src, spans, ScriptUnitKind::Fragment(parsed));
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

    // --- Internal processing ---

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
        let stmts = parsed.statements(self.db).to_vec();

        let func_analyses = match drop_analysis::analyze_script_functions(self.db, expr_types, call_targets, &stmts) {
            Ok(analyses) => analyses,
            Err(errors) => {
                let error_msgs: Vec<String> = errors.into_iter()
                    .map(|(func_name, errs)| {
                        let errs_str: Vec<String> = errs.iter().map(|e| format!("{:?}", e)).collect();
                        format!("{}: {}", func_name, errs_str.join("; "))
                    })
                    .collect();
                return ScriptLowerResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error {
                        message: format!("Drop analysis errors: {}", error_msgs.join(", ")),
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
        let stmts = parsed.statements(self.db).to_vec();

        let func_analyses = match drop_analysis::analyze_script_functions(self.db, expr_types, call_targets, &stmts) {
            Ok(analyses) => analyses,
            Err(errors) => {
                let error_msgs: Vec<String> = errors.into_iter()
                    .map(|(func_name, errs)| {
                        let errs_str: Vec<String> = errs.iter().map(|e| format!("{:?}", e)).collect();
                        format!("{}: {}", func_name, errs_str.join("; "))
                    })
                    .collect();
                return ScriptUnitResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error {
                        message: format!("Drop analysis errors: {}", error_msgs.join(", ")),
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

    // --- Environment access ---

    /// Get type and value for a binding.
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

    /// Get all environment bindings.
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

    // --- Diagnostics ---

    /// Get parse diagnostics from last eval.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Get type diagnostics from last eval.
    pub fn get_type_diagnostics(&self) -> Vec<&datalove_diagnostic::TypeDiagnostic> {
        if let Some(batch_spec) = self.last_batch_spec {
            type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(self.db, batch_spec)
        } else {
            Vec::new()
        }
    }

    /// Get database reference.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    // --- Debug output ---

    /// Get debug buffer contents.
    pub fn get_debug_buffer(&self) -> String {
        self.interp.get_debug_buffer()
    }

    /// Clear debug buffer.
    pub fn clear_debug_buffer(&self) {
        self.interp.clear_debug_buffer();
    }

    // --- JIT configuration ---

    /// Set a call dispatcher for JIT compilation.
    ///
    /// When set, function calls are routed through the dispatcher, which can
    /// decide to execute JIT-compiled code or fall back to interpretation.
    pub fn set_call_dispatcher(&mut self, dispatcher: Box<dyn CallDispatcher>) {
        self.interp.set_call_dispatcher(dispatcher);
    }

    // --- Cleanup ---

    /// Destroy all allocated values.
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
    pub ty: Option<String>,
    pub output: String,
}

/// Result of lowering a script unit (without execution).
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
// AOT compilation utilities
// ============================================================================

/// AOT compilation, linking, and execution utilities.
pub mod aot {
    use rmx::prelude::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::OnceLock;

    use datalove_datafun_aot_cranelift::AotCompiler;
    use datalove_datafun_ir::{IrScriptUnit, FunctionRegistry, IrFunction};

    // --- Error types ---

    /// AOT linking error.
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

    /// AOT execution error.
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

    /// Execution output.
    pub struct ExecOutput {
        pub exit_code: i32,
        pub stdout: String,
        pub stderr: String,
    }

    // --- Runtime library ---

    static RUNTIME_LIB_DIR: OnceLock<PathBuf> = OnceLock::new();

    /// Build runtime library and return path to lib directory.
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

    // --- Compilation ---

    /// Compile IR to object bytes.
    pub fn compile_script_to_object(unit: &IrScriptUnit) -> AnyResult<Vec<u8>> {
        let mut compiler = AotCompiler::new_for_host()
            .map_err(|e| anyhow!("failed to create AOT compiler: {}", e))?;
        let product = compiler.compile_script_unit(unit)
            .map_err(|e| anyhow!("AOT compilation failed: {}", e))?;
        let obj_bytes = product.emit()
            .map_err(|e| anyhow!("failed to emit object: {}", e))?;
        Ok(obj_bytes)
    }

    /// Compile IR with world types to object bytes.
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

    // --- Linking ---

    /// Link object to executable in temp directory.
    ///
    /// Returns the executable path and the TempDir. Caller must keep the TempDir
    /// alive to prevent the executable from being deleted.
    pub fn link_object_to_temp_executable(
        obj_bytes: &[u8],
    ) -> Result<(PathBuf, tempfile::TempDir), LinkError> {
        let dir = tempfile::tempdir().map_err(LinkError::TempDir)?;
        let exe_path = dir.path().join("script");
        link_object_to_path(obj_bytes, &exe_path)?;
        Ok((exe_path, dir))
    }

    /// Link object to specified path.
    pub fn link_object_to_path(obj_bytes: &[u8], output_path: &Path) -> Result<(), LinkError> {
        let dir = tempfile::tempdir().map_err(LinkError::TempDir)?;
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

    // --- Execution ---

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

    // --- Convenience functions ---

    /// Compile, link, and run.
    pub fn compile_link_run(unit: &IrScriptUnit) -> AnyResult<ExecOutput> {
        let obj_bytes = compile_script_to_object(unit)?;
        let (exe_path, _dir) = link_object_to_temp_executable(&obj_bytes)
            .map_err(|e| anyhow!("{}", e))?;
        run_executable(&exe_path).map_err(|e| anyhow!("{}", e))
    }

    /// Compile, link, and run with world types.
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
