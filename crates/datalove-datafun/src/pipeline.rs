//! Shared compiler pipeline for IR3 worldfile analysis.
//!
//! Provides common infrastructure for compiling module sections to IR,
//! used by both `worldfile_analysis_ir3` (mixed worldfiles with script units)
//! and `worldfile_analysis_modules_ir3` (module-only worldfiles).

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::{BTreeMap, HashMap};

use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_ir::{IrModuleId, FuncId, IrType, IrScriptUnit};
use datalove_datafun_compiler::lower;
use datalove_datafun_compiler::drop_analysis;
use datalove_datafun_tycheck::{
    typecheck_module_graph, type_check_script_units,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked,
};
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult, ModuleId,
    ParsedModuleGraph, parse_module_graph,
};
use datalove_datafun_interp::{ScriptEnvironment, UnitCompletion};
use datalove_rt::rust::AlignedBuffer;
use drop_analysis::FunctionDropAnalysis;

/// Typecheck result summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    ParseError {
        errors: Vec<String>,
    },
    Error {
        errors: Vec<String>,
    },
    Skipped,
}

/// Lowering result summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success {
        /// IR dump.
        ir: String,
    },
    Error {
        message: String,
    },
    Skipped,
}

/// Compiled module data ready for execution.
pub struct CompiledModules<'db> {
    /// Package resolution error (if any).
    pub resolution_error: Option<String>,
    /// Module graph.
    pub module_graph: ModuleGraph,
    /// Parsed module graph (graph + pre-parsed scripts).
    pub parsed_graph: ParsedModuleGraph<'db>,
    /// Graph typecheck result (for accessing expr_types).
    pub graph_typecheck: ModuleGraphTypecheckResult<'db>,
    /// Map from module path to typecheck errors.
    pub path_to_errors: BTreeMap<String, Vec<String>>,
    /// Map from function name to drop analysis errors.
    pub drop_analysis_errors: BTreeMap<String, Vec<String>>,
    /// Map from (salsa ModuleId, func_name) -> (IrModuleId, FuncId).
    /// Used to resolve typechecker's ResolvedCallTarget to IR function refs.
    pub func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    /// Execution environment with lowered functions.
    pub env: ScriptEnvironment,
    /// Per-module lowering results (IR dumps or errors).
    pub module_lowering_results: BTreeMap<String, Vec<String>>,
    /// Prototype analysis results (empty if analysis disabled).
    pub analysis: datalove_datafun_analysis::ModuleGraphAnalysis,
}

/// Pipeline for compiling worldfile modules to IR.
pub struct ModuleCompilationPipeline<'db> {
    db: &'db dyn salsa::Database,
    pkglib_system: BTreeMap<String, Package>,
    pkglib_local: BTreeMap<String, Package>,
    /// Enable prototype analysis passes (termination, refinement).
    enable_analysis: bool,
}

impl<'db> ModuleCompilationPipeline<'db> {
    /// Create a new pipeline.
    pub fn new(db: &'db dyn salsa::Database) -> Self {
        Self {
            db,
            pkglib_system: BTreeMap::new(),
            pkglib_local: BTreeMap::new(),
            enable_analysis: false,
        }
    }

    /// Create a new pipeline from worldfile sections.
    ///
    /// Convenience constructor that creates a pipeline and adds all module
    /// sections from the provided sections list.
    pub fn from_sections(
        db: &'db dyn salsa::Database,
        sections: &[datalove_datafun_pkg::package_load_worldfile::WorldfileSection],
    ) -> Self {
        let mut pipeline = Self::new(db);
        pipeline.add_modules_from_sections(sections);
        pipeline
    }

    /// Enable prototype analysis passes (termination detection, refinement types).
    pub fn enable_analysis(&mut self, enable: bool) -> &mut Self {
        self.enable_analysis = enable;
        self
    }

    /// Add a module section to the pipeline.
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

    /// Load the sys library from the specified directory.
    ///
    /// Loads all modules from the sys library into the pipeline.
    /// This is typically used by the CLI and REPL to provide standard library functions.
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

        // Add all sys modules to the pipeline.
        for (pkg_name, pkg) in &package_world_raw.pkglib_system {
            for (mod_name, pkg_module) in &pkg.modules {
                self.add_module("sys", pkg_name, mod_name, &pkg_module.text);
            }
        }

        Ok(())
    }

    /// Load the sys library from the default location.
    ///
    /// Finds the sys/ directory relative to CARGO_MANIFEST_DIR and loads it.
    /// This is a convenience method for CLI and REPL usage.
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

    /// Get reference to local packages (for validation).
    pub fn pkglib_local(&self) -> &BTreeMap<String, Package> {
        &self.pkglib_local
    }

    /// Build and typecheck modules, then lower to IR.
    ///
    /// Three-phase compilation:
    /// 1. Build graph, typecheck all modules, collect func IDs
    /// 2. Run drop analysis on all functions
    /// 3. Lower all functions (only after typecheck + drop analysis pass)
    pub fn compile(self) -> CompiledModules<'db> {
        // Phase 1: Build ModuleGraph via package resolution and typecheck.
        let raw_package_world = datalove_datafun_pkg::package_load::PackageWorld {
            pkglib_system: self.pkglib_system,
            pkglib_local: self.pkglib_local,
        };
        let package_world = datalove_datafun_pkg::import_from_loader(self.db, raw_package_world);

        // Resolve module dependencies.
        let resolution = crate::package_resolve::resolve_package_world_with_imports(self.db, package_world);

        // Handle resolution errors.
        let pkg_graph = match resolution.result(self.db) {
            Ok(graph) => graph,
            Err(e) => {
                // Return early with resolution error.
                let empty_graph = datalove_datafun_compiler::module_graph::ModuleGraphBuilder::new(self.db).build();
                let empty_parsed = parse_module_graph(self.db, empty_graph.clone());
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
                    analysis: Default::default(),
                };
            }
        };

        // Convert to ModuleGraph and parse all modules.
        let module_graph = datalove_datafun_pkg::to_module_graph(self.db, package_world, pkg_graph);
        let parsed_graph = parse_module_graph(self.db, module_graph.clone());

        let graph_typecheck = typecheck_module_graph(self.db, parsed_graph);
        let combined_expr_types = graph_typecheck.expr_types(self.db);
        let combined_call_targets = graph_typecheck.call_targets(self.db);

        // Build map from module path to typecheck errors.
        // Errors include the module path prefix for unified formatting.
        let module_errors = graph_typecheck.module_errors(self.db);
        let mut path_to_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (module_id, errors) in module_errors {
            let path = module_id.path(self.db).clone();
            let error_strings: Vec<String> = errors.iter()
                .map(|e| format!("{}: {:?}", path, e))
                .collect();
            path_to_errors.insert(path, error_strings);
        }

        // Collect all functions and assign IDs.
        // Key is (salsa ModuleId, func_name) for unambiguous lookup from ResolvedCallTarget.
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

        // Phase 2: Run drop analysis on all functions.
        // Maps (module_path, func_name) -> FunctionDropAnalysis or errors.
        let mut drop_analyses: HashMap<(String, String), FunctionDropAnalysis> = HashMap::new();
        let mut drop_analysis_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for module in module_graph.iter_modules(self.db) {
            let salsa_module_id = module.id(self.db);
            let module_path = salsa_module_id.path(self.db).clone();

            // Skip if module has typecheck errors.
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

        // Phase 2.5 (optional): Run prototype analysis passes.
        let analysis = if self.enable_analysis {
            datalove_datafun_analysis::analyze_module_graph(
                self.db,
                &parsed_graph,
                &path_to_errors,
            )
        } else {
            Default::default()
        };

        // Phase 3: Lower all functions.
        let mut env = ScriptEnvironment::new();
        let mut module_lowering_results: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for (ir_module_idx, module) in module_graph.iter_modules(self.db).enumerate() {
            let ir_module_id = IrModuleId(ir_module_idx as u32);
            let salsa_module_id = module.id(self.db);
            let module_path = salsa_module_id.path(self.db).clone();

            // Skip lowering if module has typecheck errors.
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

                    // Skip if drop analysis had errors.
                    let drop_key = format!("{}/{}", module_path, func_name);
                    if drop_analysis_errors.contains_key(&drop_key) {
                        ir_dumps.push(format!("Drop analysis error in {}", func_name));
                        continue;
                    }

                    // Get the pre-computed drop analysis.
                    let analysis = match drop_analyses.get(&(module_path.clone(), func_name.clone())) {
                        Some(a) => a.clone(),
                        None => {
                            ir_dumps.push(format!("Missing drop analysis for {}", func_name));
                            continue;
                        }
                    };

                    let (_, func_id) = func_id_map.get(&(salsa_module_id, func_name.clone())).unwrap();

                    // Get call_targets for resolving function calls.
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
            analysis,
        }
    }
}

/// Format a module's lowering result from IR dumps.
pub fn format_module_lowering_result(
    ir_dumps: &[String],
    has_typecheck_errors: bool,
) -> LoweringResult {
    if has_typecheck_errors {
        return LoweringResult::Skipped;
    }

    let has_errors = ir_dumps.iter().any(|s|
        s.starts_with("Error") || s.starts_with("Drop analysis error") || s.starts_with("Missing drop analysis")
    );

    if has_errors {
        let errors: Vec<_> = ir_dumps.iter()
            .filter(|s| s.starts_with("Error") || s.starts_with("Drop analysis error") || s.starts_with("Missing drop analysis"))
            .cloned()
            .collect();
        LoweringResult::Error { message: errors.join("\n") }
    } else if ir_dumps.is_empty() {
        LoweringResult::Skipped
    } else {
        LoweringResult::Success { ir: ir_dumps.join("\n") }
    }
}

/// Result of compiling a single script unit.
pub struct ScriptUnitResult {
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Type of the result (for expressions).
    pub ty: Option<String>,
    /// Output value (for expressions) or execution status.
    pub output: String,
}

/// Result of lowering a script unit (without execution).
pub struct ScriptLowerResult {
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// The lowered IR (if successful).
    pub ir_unit: Option<IrScriptUnit>,
}

/// Context for compiling and executing script units.
///
/// Created from `CompiledModules::script_context()`, this provides incremental
/// compilation and execution of script units with shared state.
pub struct ScriptCompilationContext<'db> {
    db: &'db dyn salsa::Database,
    /// Lowering context (grows with each unit).
    pub script_ctx: lower::ScriptLowerContext,
    /// Execution environment (grows with each unit).
    pub env: ScriptEnvironment,
    /// Accumulated unit specs for incremental typechecking.
    accumulated_unit_specs: Vec<ScriptUnitSpec<'db>>,
    /// Module specs for typechecking.
    module_specs: Vec<ModuleSpec<'db>>,
    /// IR interpreter (owns the tydesc_table and runtime).
    interp: datalove_datafun_interp::IrInterpreter,
    /// Map from (salsa ModuleId, func_name) -> (IrModuleId, FuncId).
    /// Used to resolve typechecker's ResolvedCallTarget to IR function refs.
    func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    /// Last Source used for parsing (for diagnostic retrieval).
    last_source: Option<bct::input::Source>,
    /// Last batch spec used (for diagnostic retrieval).
    last_batch_spec: Option<ScriptBatchSpec<'db>>,
}

impl<'db> CompiledModules<'db> {
    /// Check if compilation succeeded without errors.
    ///
    /// Returns true only if there are no resolution, typecheck, drop analysis, or lowering errors.
    pub fn is_successful(&self) -> bool {
        self.resolution_error.is_none()
            && self.path_to_errors.values().all(|errors| errors.is_empty())
            && self.drop_analysis_errors.is_empty()
            && !self.has_lowering_errors()
    }

    /// Check if there are any errors.
    ///
    /// Returns true if there are resolution, typecheck, drop analysis, or lowering errors.
    pub fn has_errors(&self) -> bool {
        !self.is_successful()
    }

    /// Get all errors as a flat vector of error strings.
    ///
    /// Collects resolution, typecheck, drop analysis, and lowering errors.
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
                if error.starts_with("Error") || error.starts_with("Drop analysis error") || error.starts_with("Missing drop analysis") {
                    errors.push(error.clone());
                }
            }
        }

        errors
    }

    /// Check if there are any lowering errors in the module results.
    fn has_lowering_errors(&self) -> bool {
        self.module_lowering_results.values()
            .flatten()
            .any(|s| s.starts_with("Error") || s.starts_with("Drop analysis error") || s.starts_with("Missing drop analysis"))
    }

    /// Get all typecheck errors as a flat vector.
    pub fn all_typecheck_errors(&self) -> Vec<String> {
        self.path_to_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all drop analysis errors as a flat vector.
    pub fn all_drop_analysis_errors(&self) -> Vec<String> {
        self.drop_analysis_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Create a script compilation context from compiled modules.
    ///
    /// Consumes the compiled modules and returns a context for incrementally
    /// compiling and executing script units.
    ///
    /// The `debug_mode` parameter controls debug output behavior:
    /// - `Disabled`: debuglog statements do nothing (default for production)
    /// - `Stderr`: debuglog outputs to stderr
    /// - `Buffer`: debuglog outputs to an internal buffer (for testing)
    pub fn script_context(
        self,
        db: &'db dyn salsa::Database,
        debug_mode: datalove_rt::c::DebugOutputMode,
    ) -> ScriptCompilationContext<'db> {
        // Build ScriptLowerContext and module specs from pre-parsed statements.
        let script_ctx = lower::ScriptLowerContext::new();
        let mut module_specs = Vec::new();

        for (salsa_module_id, parsed, spans) in self.parsed_graph.parsed_statements(db) {
            let module_path = salsa_module_id.path(db).clone();
            // Get the source from the module graph.
            let module_source = self.module_graph.iter_modules(db)
                .find(|m| m.id(db) == *salsa_module_id)
                .map(|m| m.source(db))
                .expect("module should exist in graph");

            // Build module spec with pre-parsed statements, spans, and ModuleId.
            module_specs.push(ModuleSpec::new(
                db,
                module_path.clone(),
                module_source,
                *spans,
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

impl<'db> ScriptCompilationContext<'db> {
    /// Compile and execute a script fragment.
    pub fn eval_fragment(&mut self, source: &str) -> ScriptUnitResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed(self.db);

        // Collect parse diagnostics.
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

        // Incremental typecheck with pre-parsed content.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(self.db, src, spans, ScriptUnitKind::Fragment(parsed));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
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
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        // Collect parse diagnostics.
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

        // Incremental typecheck with pre-parsed content.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(self.db, src, spans, ScriptUnitKind::Expr(expr));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.process_expr(expr, tycheck_result)
    }

    /// Lower a script fragment without executing (for AOT compilation).
    pub fn lower_fragment(&mut self, source: &str) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed(self.db);

        // Collect parse diagnostics.
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

        // Incremental typecheck with pre-parsed content.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(self.db, src, spans, ScriptUnitKind::Fragment(parsed));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.lower_fragment_inner(parsed, tycheck_result, false)
    }

    /// Lower a script fragment for AOT compilation.
    ///
    /// Like `lower_fragment` but emits Drop instructions for script-level bindings
    /// at unit end. Use this for AOT compilation where bindings don't persist.
    pub fn lower_fragment_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        self.last_source = Some(src);
        let parse_result = datalove_datafun_parser::parse(self.db, src);
        let parsed = parse_result.parsed(self.db);

        // Collect parse diagnostics.
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

        // Incremental typecheck with pre-parsed content.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(self.db, src, spans, ScriptUnitKind::Fragment(parsed));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.lower_fragment_inner(parsed, tycheck_result, true)
    }

    /// Lower a script expression without executing (for AOT compilation).
    pub fn lower_expr(&mut self, source: &str) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        // Collect parse diagnostics.
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

        // Incremental typecheck with pre-parsed content.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(self.db, src, spans, ScriptUnitKind::Expr(expr));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.lower_expr_inner(expr, tycheck_result, false)
    }

    /// Lower a script expression for AOT compilation.
    ///
    /// Like `lower_expr` but with `for_aot=true` for API consistency.
    /// Note: Expressions don't create script-level bindings, so the flag has no effect.
    pub fn lower_expr_for_aot(&mut self, source: &str) -> ScriptLowerResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        let expr = datalove_datafun_parser::parse_expr(self.db, src);

        // Collect parse diagnostics.
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

        // Incremental typecheck.
        let spans = datalove_datafun_parser::datafun_spans(self.db, src);
        let unit_spec = ScriptUnitSpec::new(self.db, src, spans, ScriptUnitKind::Expr(expr));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        self.last_batch_spec = Some(batch_spec);
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.lower_expr_inner(expr, tycheck_result, true)
    }

    /// Lower a fragment to IR without execution.
    fn lower_fragment_inner(
        &mut self,
        parsed: datalove_datafun_ast::ast::ParsedStatements<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
        for_aot: bool,
    ) -> ScriptLowerResult {
        // Check for typecheck errors.
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

        // Run drop analysis.
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

        // Lower to IR.
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

        // Update script context with exports from this unit.
        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptLowerResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    /// Lower an expression to IR without execution.
    fn lower_expr_inner(
        &mut self,
        expr: datalove_datafun_ast::ast::ExprFun<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
        for_aot: bool,
    ) -> ScriptLowerResult {
        // Check for typecheck errors.
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

        // Lower the expression.
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

        // Update script context with exports from this unit.
        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptLowerResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    /// Process a parsed fragment through typecheck, lower, and execute.
    fn process_fragment(
        &mut self,
        parsed: datalove_datafun_ast::ast::ParsedStatements<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
    ) -> ScriptUnitResult {
        // Check for typecheck errors.
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

        // Run drop analysis on all functions first.
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

        // Lower using the typecheck result's expr_types and call_targets.
        // Use for_aot=false since this is for REPL execution where bindings persist.
        let call_targets = tycheck_result.call_targets(self.db);
        let ir_unit = match lower::lower_script_fragment_raw(
            self.db,
            expr_types,
            call_targets,
            &self.func_id_map,
            self.script_ctx.clone(),
            stmts,
            func_analyses,
            false, // for_aot
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

        // Format IR dump.
        let ir_dump = format!("{}", ir_unit);

        // Execute the fragment with shared environment.
        let ret_type = IrType::Result(Box::new(IrType::Unit));
        let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
        let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
        let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
        let ret_dest = datalove_datafun_interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // Fragments have no expression result, so expr_dest is None.
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

        // Update script context with exports from this unit.
        let unit_index = self.script_ctx.current_unit;
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptUnitResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ty: None, // Fragments don't have a result type.
            output,
        }
    }

    /// Process a parsed expression through typecheck, lower, and execute.
    fn process_expr(
        &mut self,
        expr: datalove_datafun_ast::ast::ExprFun<'db>,
        tycheck_result: UnitTypecheckResultTracked<'db>,
    ) -> ScriptUnitResult {
        // Check for typecheck errors.
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

        // Lower the expression as a script unit.
        // Use for_aot=false since this is for REPL execution where bindings persist.
        let ir_unit = match lower::lower_script_expr(
            self.db,
            tycheck_result.expr_types(self.db),
            tycheck_result.call_targets(self.db),
            &self.func_id_map,
            self.script_ctx.clone(),
            expr,
            false, // for_aot
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

        // Get the result type if there is one.
        let result_ty = ir_unit.result
            .map(|id| format!("{}", &ir_unit.value_types[id.0 as usize]));

        // Execute the script unit if it has a result.
        let output = if let Some(result_id) = ir_unit.result {
            // ret_dest is for early returns: always Result<(), Error>.
            let ret_type = IrType::Result(Box::new(IrType::Unit));
            let ret_tydesc = self.interp.tydesc_table_mut().get_or_create(&ret_type);
            let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
            let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
            let ret_dest = datalove_datafun_interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            // expr_dest is for the expression result.
            let expr_type = &ir_unit.value_types[result_id.0 as usize];
            let expr_tydesc = self.interp.tydesc_table_mut().get_or_create(expr_type);
            let (expr_size, expr_align) = unsafe { ((*expr_tydesc).size, (*expr_tydesc).align) };
            let mut expr_buffer = AlignedBuffer::with_align(expr_size as usize, expr_align as usize);
            let expr_dest = datalove_datafun_interp::Destination {
                ptr: expr_buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            };

            // Execute with shared environment.
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

        // Update script context with exports from this unit.
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

    /// Cleanup all allocated values.
    pub fn destroy_all(&mut self) {
        self.env.destroy_all(self.interp.runtime_handle());
    }

    /// Get type and value for a specific binding by name.
    ///
    /// Returns Some((type, value)) if the binding exists, None otherwise.
    pub fn get_binding(&mut self, name: &str) -> Option<(String, String)> {
        use datalove_datafun_interp::InterpError;

        // Check let bindings (values).
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

        // Check var bindings (slots).
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

    /// Get environment bindings with types and values.
    ///
    /// Returns a list of (name, kind, type, value) tuples, sorted by name.
    /// - kind is "let", "var", or "fun"
    /// - type is the IrType formatted as a string
    /// - value is the pretty-printed value (or "<moved>" if consumed)
    pub fn get_environment(&mut self) -> Vec<(String, String, String, String)> {
        use datalove_datafun_interp::InterpError;
        let mut result = Vec::new();

        // Let bindings (values).
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

        // Var bindings (slots).
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

        // Functions.
        for (name, _) in &self.script_ctx.functions {
            result.push((name.clone(), "fun".to_string(), "function".to_string(), "-".to_string()));
        }

        // Sort by name.
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Get parse diagnostics from the last eval call.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Get type diagnostics from the last eval call.
    pub fn get_type_diagnostics(&self) -> Vec<&datalove_diagnostic::TypeDiagnostic> {
        if let Some(batch_spec) = self.last_batch_spec {
            type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(self.db, batch_spec)
        } else {
            Vec::new()
        }
    }

    /// Get database reference for rendering diagnostics.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// Get the contents of the debug buffer.
    pub fn get_debug_buffer(&self) -> String {
        self.interp.get_debug_buffer()
    }

    /// Clear the debug buffer.
    pub fn clear_debug_buffer(&self) {
        self.interp.clear_debug_buffer();
    }
}
