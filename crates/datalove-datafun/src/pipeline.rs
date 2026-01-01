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
use datalove_datafun_compiler::tycheck::typecheck_module_graph;
use datalove_datafun_compiler::module_graph::{ModuleGraph, ModuleGraphTypecheckResult};
use ir::interp::ScriptEnvironment;
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
    /// Graph typecheck result (for accessing expr_types).
    pub graph_typecheck: ModuleGraphTypecheckResult<'db>,
    /// Map from module path to typecheck errors.
    pub path_to_errors: BTreeMap<String, Vec<String>>,
    /// Map from function name to drop analysis errors.
    pub drop_analysis_errors: BTreeMap<String, Vec<String>>,
    /// Map of function name -> (module_id, func_id).
    pub all_module_functions: HashMap<String, (IrModuleId, FuncId)>,
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
                return CompiledModules {
                    resolution_error: Some(format!("Package resolution failed: {:?}", e)),
                    module_graph: datalove_datafun_compiler::module_graph::ModuleGraphBuilder::new(self.db).build(),
                    graph_typecheck: typecheck_module_graph(
                        self.db,
                        datalove_datafun_compiler::module_graph::ModuleGraphBuilder::new(self.db).build()
                    ),
                    path_to_errors: BTreeMap::new(),
                    drop_analysis_errors: BTreeMap::new(),
                    all_module_functions: HashMap::new(),
                    env: ScriptEnvironment::new(),
                    module_lowering_results: BTreeMap::new(),
                };
            }
        };

        // Convert to ModuleGraph.
        let module_graph = datalove_datafun_pkg::to_module_graph(self.db, package_world, pkg_graph);

        let graph_typecheck = typecheck_module_graph(self.db, module_graph.clone());
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

        // Collect all function names and assign IDs.
        let mut all_module_functions: HashMap<String, (IrModuleId, FuncId)> = HashMap::new();
        let mut next_func_id: u32 = 0;
        for (ir_module_idx, module) in module_graph.iter_modules(self.db).enumerate() {
            let ir_module_id = IrModuleId(ir_module_idx as u32);
            let module_source = module.source(self.db);
            let parse_result = datalove_datafun_compiler::parser::parse(self.db, module_source);
            let script = parse_result.script(self.db);
            for statement in script.statements(self.db) {
                if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
                    let func_name = func.name(self.db).text(self.db).to_string();
                    let func_id = FuncId(next_func_id);
                    next_func_id += 1;
                    all_module_functions.insert(func_name, (ir_module_id, func_id));
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

                    let (_, func_id) = all_module_functions.get(&func_name).unwrap();

                    match ir::lower::lower_function_for_module(
                        self.db, combined_expr_types, &all_module_functions, *func, analysis
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
            graph_typecheck,
            path_to_errors,
            drop_analysis_errors,
            all_module_functions,
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
