//! Interpreter context types and module loading.
//!
//! Core types for interpreter state management:
//! - [`InterpContext`]: Main interpreter state (runtime, module graph, call stack)
//! - [`ScriptScope`]: Top-level variable bindings for REPL/script execution
//! - [`ModuleFunctionTableGraph`]: Tracks imported functions from modules using ModuleId
//! - [`ScriptResult`]: Result of script execution with value and runtime

use rmx::std::collections::HashMap;
use bct::text::InternedText;

use crate::module_graph::{ModuleId, ModuleGraph, ModuleGraphTypecheckResult};
use crate::ast::{self, StmtFun};

use super::{Value, InterpError, StackFrame, destroy_value};

// ============================================================================
// Context Types
// ============================================================================

/// Interpreter context for script execution.
pub struct InterpContext<'db> {
    pub(super) db: &'db dyn crate::Db,
    pub(super) runtime: datalove_rt::rust::Runtime,
    pub(super) script: Option<crate::script::Script>,
    pub script_scope: ScriptScope<'db>,
    /// Script-level function analyses (for functions defined in the script).
    pub(super) script_function_analyses: HashMap<ast::StmtFun<'db>, crate::function_analysis::FunctionAnalysis<'db>>,
    pub(super) tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
    /// Call stack for frame-based execution.
    pub(super) call_stack: Vec<StackFrame<'db>>,
    /// Module function table for ModuleGraph mode.
    pub(super) module_functions_graph: ModuleFunctionTableGraph<'db>,
    /// Current module ID for module-internal function calls.
    pub(super) current_module_id: Option<ModuleId>,
    /// Typecheck result for the module graph.
    pub(super) module_graph_typecheck: Option<ModuleGraphTypecheckResult<'db>>,
    /// Expression types from typechecking, indexed by ExprFun salsa ID.
    pub(super) expr_types: Vec<Option<crate::tycheck::TypeAndHeap<'db>>>,
}

/// Script-level scope for REPL incremental execution.
pub struct ScriptScope<'db> {
    /// Script-level let bindings with move tracking.
    pub variables: HashMap<InternedText<'db>, ScriptVariable>,
    /// Script-level functions.
    pub functions: HashMap<InternedText<'db>, StmtFun<'db>>,
}

/// Module function table for tracking imported functions.
///
/// Maps imported function names to their function definitions and source modules.
pub struct ModuleFunctionTableGraph<'db> {
    /// Maps imported function name → (function definition, source module ID).
    imported_functions: HashMap<InternedText<'db>, (ast::StmtFun<'db>, ModuleId)>,
    /// Cache of all functions in each module.
    module_all_functions: HashMap<ModuleId, HashMap<InternedText<'db>, ast::StmtFun<'db>>>,
}

/// Script-level variable with move tracking for linear semantics.
pub struct ScriptVariable {
    pub value: Value,
    pub state: ScriptVarState,
    pub is_copy: bool,
}

/// Move state for script-level variables.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ScriptVarState {
    Available,
    Moved,
}

/// Result of script execution containing the value and runtime.
///
/// The runtime and tydesc_table must be kept alive for the value pointer to remain valid.
pub struct ScriptResult<'db> {
    pub value: Value,
    pub runtime: datalove_rt::rust::Runtime,
    pub tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
}

impl std::fmt::Debug for ScriptResult<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptResult")
            .field("value", &self.value)
            .field("runtime", &"<Runtime>")
            .field("tydesc_table", &"<TyDescTable>")
            .finish()
    }
}

impl Drop for ScriptResult<'_> {
    fn drop(&mut self) {
        // NOTE: Value cleanup is now done manually before dropping ScriptResult.
        // This is because the Drop implementation was running too late,
        // after the runtime had already started shutting down.
        // The manual cleanup happens in worldfile_analysis.rs.
    }
}

impl<'db> InterpContext<'db> {
    /// Create a new interpreter context with ModuleGraph typecheck result.
    ///
    /// The caller must provide a valid ModuleGraphTypecheckResult with no errors.
    pub fn new_with_module_graph(
        db: &'db dyn crate::Db,
        typecheck_result: ModuleGraphTypecheckResult<'db>,
    ) -> Result<InterpContext<'db>, InterpError> {
        // Check for typecheck errors.
        if !typecheck_result.is_ok(db) {
            let all_errors: Vec<_> = typecheck_result.all_errors(db).into_iter().cloned().collect();
            return Err(InterpError::TypecheckErrors(all_errors));
        }

        let graph = typecheck_result.graph(db);
        let module_functions_graph = ModuleFunctionTableGraph::build_from_graph(db, graph);

        // Initialize expr_types from module graph typecheck.
        let expr_types = typecheck_result.expr_types(db).clone();

        Ok(InterpContext {
            db,
            runtime: datalove_rt::rust::Runtime::new(),
            script: None,
            script_scope: ScriptScope {
                variables: HashMap::new(),
                functions: HashMap::new(),
            },
            script_function_analyses: HashMap::new(),
            tydesc_table: datalove_datalit::tydesc_table::TyDescTable::new(db),
            call_stack: Vec::new(),
            module_functions_graph,
            current_module_id: None,
            module_graph_typecheck: Some(typecheck_result),
            expr_types,
        })
    }

    /// Set the current script for execution.
    pub fn set_script(&mut self, script: crate::script::Script) {
        self.script = Some(script);
    }

    /// Check if this context has a typecheck result.
    pub fn is_typechecked(&self) -> bool {
        self.module_graph_typecheck.is_some()
    }

    /// Merge expression types from a typecheck result.
    ///
    /// This extends the expr_types vector with types from the given TypecheckResult.
    /// Should be called after typechecking each script unit.
    pub fn merge_expr_types(&mut self, typecheck_result: crate::tycheck::TypecheckResult<'db>) {
        let new_types = typecheck_result.expr_types(self.db);
        // Extend our vector if needed and copy types.
        if new_types.len() > self.expr_types.len() {
            self.expr_types.resize(new_types.len(), None);
        }
        for (i, ty) in new_types.iter().enumerate() {
            if ty.is_some() {
                self.expr_types[i] = *ty;
            }
        }
    }

    /// Look up the type of an expression.
    ///
    /// Returns the type if it was recorded during typechecking.
    pub fn get_expr_type(&self, expr: ast::ExprFun<'db>) -> Option<crate::tycheck::TypeAndHeap<'db>> {
        use salsa::plumbing::AsId;
        let id = expr.as_id();
        let index = id.index() as usize;
        self.expr_types.get(index).copied().flatten()
    }

    /// Populate script-level imports using ModuleGraph.
    ///
    /// Parses require/import statements from the script and resolves them
    /// against the available modules in the graph.
    pub fn populate_script_imports(&mut self, script: crate::script::Script, graph: ModuleGraph) {
        self.module_functions_graph.populate_script_imports_for_graph(self.db, script, graph);
    }

    /// Get the runtime handle.
    pub fn runtime_handle(&self) -> datalove_rt::c::LocalRtHandle {
        self.runtime.handle()
    }

    /// Get the module function graph.
    pub fn module_function_graph(&self) -> &ModuleFunctionTableGraph<'db> {
        &self.module_functions_graph
    }

    /// Pretty-print a value using this context's runtime and tydesc_table.
    pub fn pretty_print_value(&mut self, value: &Value) -> Result<String, InterpError> {
        use datalove_rt as rt;
        use datalove_rt::rtdt;

        unsafe {
            let rt_handle = self.runtime.handle();
            let string_tydesc = self.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::String);

            let mut output_string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = rt::c::dtlv_rti_string_create_local(
                rt_handle,
                output_string.as_mut_ptr() as *mut u8,
                string_tydesc,
            );

            if status != rt::c::RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to create output string".to_string(),
                ));
            }

            let mut output_string = output_string.assume_init();

            let status = rt::c::dtlv_rti_pretty_print_local(
                rt_handle,
                value.ptr,
                value.tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                string_tydesc,
            );

            if status != rt::c::RtStatus::Ok {
                rt::c::dtlv_rti_string_destroy_local(
                    rt_handle,
                    &mut output_string as *mut rtdt::String as *mut u8,
                    string_tydesc,
                );
                return Err(InterpError::RuntimeError(
                    "Failed to pretty-print value".to_string(),
                ));
            }

            let result = if output_string.data.is_null() || output_string.size == 0 {
                String::new()
            } else {
                let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
                String::from_utf8_lossy(bytes).to_string()
            };

            rt::c::dtlv_rti_string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                string_tydesc,
            );

            Ok(result)
        }
    }
}

impl ScriptScope<'_> {
    /// Create a new empty script scope.
    pub fn new<'db>() -> ScriptScope<'db> {
        ScriptScope {
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
    }
}

impl<'db> ModuleFunctionTableGraph<'db> {
    /// Create a new empty module function table.
    pub fn new() -> ModuleFunctionTableGraph<'db> {
        ModuleFunctionTableGraph {
            imported_functions: HashMap::new(),
            module_all_functions: HashMap::new(),
        }
    }

    /// Build module function table from a ModuleGraph.
    ///
    /// Parses ALL modules in the graph to extract function definitions.
    pub fn build_from_graph(
        db: &'db dyn crate::Db,
        graph: ModuleGraph,
    ) -> ModuleFunctionTableGraph<'db> {
        let mut table = ModuleFunctionTableGraph::new();

        for module in graph.iter_modules(db) {
            let module_id = module.id(db);
            let source = module.source(db);

            let parse_result = crate::parser::parse(db, source);
            let parsed = parse_result.script(db);

            let mut module_funcs = HashMap::new();
            for statement in parsed.statements(db) {
                if let ast::Statement::Fun(func) = statement {
                    let func_name = func.name(db);
                    module_funcs.insert(func_name, *func);
                }
            }
            table.module_all_functions.insert(module_id, module_funcs);
        }

        table
    }

    /// Look up an imported function by name.
    ///
    /// Returns the function and its source module ID.
    pub fn get(&self, name: InternedText<'db>) -> Option<(ast::StmtFun<'db>, ModuleId)> {
        self.imported_functions.get(&name).copied()
    }

    /// Get all functions from a module by ID.
    pub fn get_module_functions(&self, module_id: ModuleId) -> Option<&HashMap<InternedText<'db>, ast::StmtFun<'db>>> {
        self.module_all_functions.get(&module_id)
    }

    /// Add an imported function.
    pub fn add_import(&mut self, name: InternedText<'db>, func: ast::StmtFun<'db>, source_module: ModuleId) {
        self.imported_functions.insert(name, (func, source_module));
    }

    /// Populate script-level imports using ModuleGraph.
    ///
    /// Parses require/import statements from the script and resolves them
    /// against the available modules in the graph.
    pub fn populate_script_imports_for_graph(
        &mut self,
        db: &'db dyn crate::Db,
        script: crate::script::Script,
        graph: ModuleGraph,
    ) {
        // Build a map from module alias to ModuleId.
        let module_alias_map = build_module_alias_map_for_graph(db, script, graph);

        // Process all units to find import statements.
        let units = script.units(db);
        for unit_index in 0..units.len() {
            let parsed = crate::parser::parse_script_unit(db, script, unit_index);

            for statement in parsed.statements(db) {
                if let ast::Statement::Import(import_stmt) = statement {
                    let module_name = import_stmt.module_name(db);
                    let item_name = import_stmt.item_name(db);

                    if let Some(&module_id) = module_alias_map.get(&module_name) {
                        if let Some(module_funcs) = self.module_all_functions.get(&module_id) {
                            if let Some(&func) = module_funcs.get(&item_name) {
                                self.imported_functions.insert(item_name, (func, module_id));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Build module alias map for a script using ModuleGraph.
///
/// Maps module aliases (from require statements) to ModuleId.
fn build_module_alias_map_for_graph<'db>(
    db: &'db dyn crate::Db,
    script: crate::script::Script,
    graph: ModuleGraph,
) -> HashMap<InternedText<'db>, ModuleId> {
    use crate::ast::{Statement, StmtRequire};

    let mut alias_map = HashMap::new();

    // Build a map from path string to ModuleId.
    let mut path_to_module: HashMap<String, ModuleId> = HashMap::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let path = module_id.path(db).clone();
        path_to_module.insert(path, module_id);
    }

    // Process all units to find require module statements.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        let parsed = crate::parser::parse_script_unit(db, script, unit_index);

        for statement in parsed.statements(db) {
            if let Statement::Require(StmtRequire::Module(require_mod)) = statement {
                // Extract import space, package, and module from the require statement.
                let import_space = require_mod.import_space(db).as_str(db);
                let package_alias = require_mod.package_alias(db).as_str(db);
                let module_alias_text = require_mod.module_alias(db);

                // Build the path string (e.g., "sys/std/u32").
                let path = format!("{}/{}/{}", import_space, package_alias, module_alias_text.as_str(db));

                // Look up the ModuleId.
                if let Some(&module_id) = path_to_module.get(&path) {
                    alias_map.insert(module_alias_text, module_id);
                }
            }
        }
    }

    alias_map
}

/// Helper to clean up script scope variables.
pub(super) fn cleanup_script_scope(ctx: &mut InterpContext<'_>) {
    let vars: Vec<_> = ctx.script_scope.variables.drain().collect();
    for (_, var) in vars {
        if var.state == ScriptVarState::Available {
            // Available: destroy contents and free structure.
            destroy_value(ctx, var.value);
        }
        // Moved: ownership was transferred to consumer, nothing to do.
    }
}
