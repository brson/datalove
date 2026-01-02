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
use datalove_datafun_compiler::ir;
use datalove_datafun_compiler::ir::{IrModuleId, FuncId};
use datalove_datafun_compiler::tycheck::{
    typecheck_module_graph, type_check_script_units,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, ScriptUnitKind,
    UnitTypecheckResultTracked,
};
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult, ModuleId,
    ParsedModuleGraph, parse_module_graph,
};
use ir::interp::{ScriptEnvironment, UnitCompletion};
use ir::drop_analysis::FunctionDropAnalysis;

/// Typecheck result summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
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
}

/// Pipeline for compiling worldfile modules to IR.
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
                };
            }
        };

        // Convert to ModuleGraph and parse all modules.
        let module_graph = datalove_datafun_pkg::to_module_graph(self.db, package_world, pkg_graph);
        let parsed_graph = parse_module_graph(self.db, module_graph.clone());

        let graph_typecheck = typecheck_module_graph(self.db, parsed_graph);
        let combined_expr_types = graph_typecheck.expr_types(self.db);

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
            let parse_result = datalove_datafun_compiler::parser::parse(self.db, module_source);
            let script = parse_result.script(self.db);
            for statement in script.statements(self.db) {
                if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
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
            let parse_result = datalove_datafun_compiler::parser::parse(self.db, module_source);
            let script = parse_result.script(self.db);

            for statement in script.statements(self.db) {
                if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
                    let func_name = func.name(self.db).text(self.db).to_string();
                    let analysis = ir::drop_analysis::analyze_function(self.db, *func, combined_expr_types);

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
            let parse_result = datalove_datafun_compiler::parser::parse(self.db, module_source);
            let script = parse_result.script(self.db);

            let mut ir_dumps = Vec::new();

            for statement in script.statements(self.db) {
                if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
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

                    match ir::lower::lower_function_for_module(
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
    /// Output value (for expressions) or execution status.
    pub output: String,
}

/// Context for compiling and executing script units.
///
/// Created from `CompiledModules::script_context()`, this provides incremental
/// compilation and execution of script units with shared state.
pub struct ScriptCompilationContext<'db> {
    db: &'db dyn salsa::Database,
    /// Lowering context (grows with each unit).
    pub script_ctx: ir::lower::ScriptLowerContext,
    /// Execution environment (grows with each unit).
    pub env: ScriptEnvironment,
    /// Accumulated unit specs for incremental typechecking.
    accumulated_unit_specs: Vec<ScriptUnitSpec<'db>>,
    /// Module specs for typechecking.
    module_specs: Vec<ModuleSpec<'db>>,
    /// Type descriptor table for interpreter.
    tydesc_table: ir::interp::IrTyDescTable,
    /// IR interpreter.
    interp: ir::interp::IrInterpreter,
    /// Map from (salsa ModuleId, func_name) -> (IrModuleId, FuncId).
    /// Used to resolve typechecker's ResolvedCallTarget to IR function refs.
    func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
}

impl<'db> CompiledModules<'db> {
    /// Create a script compilation context from compiled modules.
    ///
    /// Consumes the compiled modules and returns a context for incrementally
    /// compiling and executing script units.
    pub fn script_context(self, db: &'db dyn salsa::Database) -> ScriptCompilationContext<'db> {
        // Build ScriptLowerContext and module specs from pre-parsed scripts.
        let mut script_ctx = ir::lower::ScriptLowerContext::new();
        let mut module_specs = Vec::new();

        for (salsa_module_id, script) in self.parsed_graph.scripts(db) {
            let module_path = salsa_module_id.path(db).clone();
            // Get the source from the module graph.
            let module_source = self.module_graph.iter_modules(db)
                .find(|m| m.id(db) == *salsa_module_id)
                .map(|m| m.source(db))
                .expect("module should exist in graph");

            // Build module spec with pre-parsed script and ModuleId.
            module_specs.push(ModuleSpec::new(
                db,
                module_path.clone(),
                module_source,
                *script,
                *salsa_module_id,
            ));

            // Register module functions for execution and name lookup.
            for statement in script.statements(db) {
                if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
                    let func_name = func.name(db).text(db).to_string();
                    let qualified_name = format!("{}.{}", module_path, func_name);
                    if let Some((ir_module_id, func_id)) = self.func_id_map.get(&(*salsa_module_id, func_name)) {
                        if let Some(ir_func) = self.env.registry.get_module_function(*ir_module_id, *func_id) {
                            script_ctx.add_module_function(qualified_name, *ir_module_id, *func_id, ir_func.clone());
                        }
                    }
                }
            }
        }

        ScriptCompilationContext {
            db,
            script_ctx,
            env: self.env,
            accumulated_unit_specs: Vec::new(),
            module_specs,
            tydesc_table: ir::interp::IrTyDescTable::new(),
            interp: ir::interp::IrInterpreter::new(),
            func_id_map: self.func_id_map,
        }
    }
}

impl<'db> ScriptCompilationContext<'db> {
    /// Compile and execute a script fragment.
    pub fn eval_fragment(&mut self, source: &str) -> ScriptUnitResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        let parse_result = datalove_datafun_compiler::parser::parse(self.db, src);
        let script = parse_result.script(self.db);

        // Incremental typecheck with pre-parsed content.
        let unit_spec = ScriptUnitSpec::new(self.db, src, ScriptUnitKind::Fragment(script));
        self.accumulated_unit_specs.push(unit_spec);
        let batch_spec = ScriptBatchSpec::new(
            self.db,
            self.accumulated_unit_specs.clone(),
            self.module_specs.clone(),
        );
        let typecheck_results = type_check_script_units(self.db, batch_spec);
        let all_results = typecheck_results.results(self.db);
        let tycheck_result = *all_results.last().unwrap();

        self.process_fragment(script, tycheck_result)
    }

    /// Compile and execute a script expression.
    pub fn eval_expr(&mut self, source: &str) -> ScriptUnitResult {
        let src = bct::input::Source::new(self.db, source.to_string());
        let expr = datalove_datafun_compiler::parser::parse_expr(self.db, src);

        // Incremental typecheck with pre-parsed content.
        let unit_spec = ScriptUnitSpec::new(self.db, src, ScriptUnitKind::Expr(expr));
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

    /// Process a parsed script fragment through typecheck, lower, and execute.
    fn process_fragment(
        &mut self,
        script: datalove_datafun_compiler::ast::Script<'db>,
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
                output: String::new(),
            };
        }

        // Process require/import statements to populate import tracking.
        for statement in script.statements(self.db) {
            match statement {
                datalove_datafun_compiler::ast::Statement::Require(
                    datalove_datafun_compiler::ast::StmtRequire::Module(req)
                ) => {
                    let import_space = req.import_space(self.db).text(self.db).to_string();
                    let package_alias = req.package_alias(self.db).text(self.db).to_string();
                    let module_alias = req.module_alias(self.db).text(self.db).to_string();
                    let full_path = format!("{}/{}/{}", import_space, package_alias, module_alias);
                    self.script_ctx.add_module_alias(module_alias, full_path);
                }
                datalove_datafun_compiler::ast::Statement::Import(import) => {
                    let module_alias = import.module_name(self.db).text(self.db).to_string();
                    let item_name = import.item_name(self.db).text(self.db).to_string();
                    // Look up the full module path from the require statement.
                    if let Some(full_path) = self.script_ctx.module_aliases.get(&module_alias) {
                        let qualified_name = format!("{}.{}", full_path, item_name);
                        self.script_ctx.import_module_function(qualified_name);
                    } else {
                        // Alias not found - import won't resolve.
                        self.script_ctx.import_module_function(item_name);
                    }
                }
                _ => {}
            }
        }

        // Run drop analysis on all functions first.
        let expr_types = tycheck_result.expr_types(self.db);
        let stmts = script.statements(self.db).to_vec();
        let func_analyses = match ir::drop_analysis::analyze_script_functions(self.db, expr_types, &stmts) {
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
                    output: String::new(),
                };
            }
        };

        // Lower using the typecheck result's expr_types and call_targets.
        let call_targets = tycheck_result.call_targets(self.db);
        let ir_unit = match ir::lower::lower_script_fragment_raw(
            self.db,
            expr_types,
            call_targets,
            &self.func_id_map,
            self.script_ctx.clone(),
            stmts,
            func_analyses,
        ) {
            Ok(unit) => unit,
            Err(e) => {
                return ScriptUnitResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    output: String::new(),
                };
            }
        };

        // Format IR dump.
        let ir_dump = format!("{}", ir_unit);

        // Execute the fragment with shared environment.
        let ret_type = ir::IrType::Result(Box::new(ir::IrType::Unit));
        let ret_tydesc = self.tydesc_table.get_or_create(&ret_type);
        let ret_size = unsafe { (*ret_tydesc).size };
        let mut ret_buffer = vec![0u8; ret_size as usize];
        let ret_dest = ir::interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // Fragments have no expression result, so expr_dest is None.
        let output = match self.interp.execute_script_unit_in_env(&ir_unit, &mut self.env, ret_dest, None) {
            Ok(UnitCompletion::Normal) => "(fragment executed)".to_string(),
            Ok(UnitCompletion::EarlyReturn) => {
                let value = ir::interp::Value {
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
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptUnitResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            output,
        }
    }

    /// Process a parsed expression through typecheck, lower, and execute.
    fn process_expr(
        &mut self,
        expr: datalove_datafun_compiler::ast::ExprFun<'db>,
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
                output: String::new(),
            };
        }

        // Lower the expression as a script unit.
        let ir_unit = match ir::lower::lower_script_expr(
            self.db,
            tycheck_result.expr_types(self.db),
            tycheck_result.call_targets(self.db),
            &self.func_id_map,
            self.script_ctx.clone(),
            expr,
        ) {
            Ok(unit) => unit,
            Err(e) => {
                return ScriptUnitResult {
                    typecheck: TypecheckResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    output: String::new(),
                };
            }
        };

        let ir_dump = format!("{}", ir_unit);

        // Execute the script unit if it has a result.
        let output = if let Some(result_id) = ir_unit.result {
            // ret_dest is for early returns: always Result<(), Error>.
            let ret_type = ir::IrType::Result(Box::new(ir::IrType::Unit));
            let ret_tydesc = self.tydesc_table.get_or_create(&ret_type);
            let ret_size = unsafe { (*ret_tydesc).size };
            let mut ret_buffer = vec![0u8; ret_size as usize];
            let ret_dest = ir::interp::Destination {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };

            // expr_dest is for the expression result.
            let expr_type = &ir_unit.value_types[result_id.0 as usize];
            let expr_tydesc = self.tydesc_table.get_or_create(expr_type);
            let expr_size = unsafe { (*expr_tydesc).size };
            let mut expr_buffer = vec![0u8; expr_size as usize];
            let expr_dest = ir::interp::Destination {
                ptr: expr_buffer.as_mut_ptr(),
                tydesc: expr_tydesc,
            };

            // Execute with shared environment.
            match self.interp.execute_script_unit_in_env(&ir_unit, &mut self.env, ret_dest, Some(expr_dest)) {
                Ok(UnitCompletion::Normal) => {
                    let value = ir::interp::Value {
                        ptr: expr_buffer.as_mut_ptr(),
                        tydesc: expr_tydesc,
                    };
                    let output_str = self.interp.pretty_print_value(&value)
                        .unwrap_or_else(|e| format!("Error: {:?}", e));
                    let _ = self.interp.destroy_value(&value);
                    output_str
                }
                Ok(UnitCompletion::EarlyReturn) => {
                    let value = ir::interp::Value {
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
        self.script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.slot_types);
        self.script_ctx.current_unit += 1;

        ScriptUnitResult {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            output,
        }
    }

    /// Cleanup all allocated values.
    pub fn destroy_all(&mut self) {
        self.env.destroy_all(self.interp.runtime_handle());
    }
}
