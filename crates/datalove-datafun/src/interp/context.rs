//! Interpreter context types and module loading.
//!
//! Core types for interpreter state management:
//! - [`InterpContext`]: Main interpreter state (runtime, package world, call stack)
//! - [`ScriptScope`]: Top-level variable bindings for REPL/script execution
//! - [`ModuleFunctionTable`]: Tracks imported functions from modules
//! - [`ScriptResult`]: Result of script execution with value and runtime

use rmx::std::collections::HashMap;
use bct::text::InternedText;

use crate::package::PackageWorld;
use crate::ast::{self, StmtFun};

use super::{Value, InterpError, StackFrame, destroy_value};

// ============================================================================
// Context Types
// ============================================================================

/// Interpreter context for script execution.
pub struct InterpContext<'db> {
    pub(super) db: &'db dyn crate::Db,
    pub(super) runtime: datalove_rt::rust::Runtime,
    pub(super) package_world: PackageWorld,
    pub(super) script: Option<crate::script::Script>,
    pub script_scope: ScriptScope<'db>,
    pub(super) module_functions: ModuleFunctionTable<'db>,
    /// Current module being executed (for module-internal function calls).
    pub(super) current_module: Option<crate::package::PackageModule>,
    /// Typecheck result for the package world (includes module exports).
    pub(super) typecheck_result: Option<crate::tycheck::PackageWorldTypecheckResult<'db>>,
    /// Script-level function analyses (for functions defined in the script).
    pub(super) script_function_analyses: HashMap<ast::StmtFun<'db>, crate::function_analysis::FunctionAnalysis<'db>>,
    pub(super) tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
    /// Call stack for frame-based execution.
    pub(super) call_stack: Vec<StackFrame<'db>>,
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
pub struct ModuleFunctionTable<'db> {
    /// Maps imported function name → (function definition, source module).
    imported_functions: HashMap<InternedText<'db>, (ast::StmtFun<'db>, crate::package::PackageModule)>,
    /// Cache of all functions in each module.
    module_all_functions: HashMap<crate::package::PackageModule, HashMap<InternedText<'db>, ast::StmtFun<'db>>>,
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

impl InterpContext<'_> {
    /// Create a new interpreter context with typecheck result.
    ///
    /// The caller must provide a valid typecheck result with no errors.
    /// The interpreter will use this to set up module function tables.
    pub fn new_with_typecheck<'db>(
        db: &'db dyn crate::Db,
        package_world: PackageWorld,
        typecheck_result: crate::tycheck::PackageWorldTypecheckResult<'db>,
    ) -> Result<InterpContext<'db>, InterpError> {
        // Check for typecheck errors.
        let module_errors = typecheck_result.module_errors(db);
        if !module_errors.is_empty() {
            let all_errors: Vec<_> = module_errors.values().flatten().cloned().collect();
            return Err(InterpError::TypecheckErrors(all_errors));
        }

        let graph = typecheck_result.graph(db);
        let module_functions = ModuleFunctionTable::build_from_graph(db, graph);

        Ok(InterpContext {
            db,
            runtime: datalove_rt::rust::Runtime::new(),
            package_world,
            script: None,
            script_scope: ScriptScope {
                variables: HashMap::new(),
                functions: HashMap::new(),
            },
            module_functions,
            current_module: None,
            typecheck_result: Some(typecheck_result),
            script_function_analyses: HashMap::new(),
            tydesc_table: datalove_datalit::tydesc_table::TyDescTable::new(db),
            call_stack: Vec::new(),
        })
    }

    /// Create a new interpreter context without typecheck (for testing only).
    ///
    /// WARNING: This creates a context without typechecking. The interpreter
    /// may fail at runtime if it encounters untypechecked code.
    #[doc(hidden)]
    pub fn new_unchecked<'db>(
        db: &'db dyn crate::Db,
        package_world: PackageWorld,
        script: Option<crate::script::Script>,
    ) -> InterpContext<'db> {
        InterpContext {
            db,
            runtime: datalove_rt::rust::Runtime::new(),
            package_world,
            script,
            script_scope: ScriptScope {
                variables: HashMap::new(),
                functions: HashMap::new(),
            },
            module_functions: ModuleFunctionTable::new(),
            current_module: None,
            typecheck_result: None,
            script_function_analyses: HashMap::new(),
            tydesc_table: datalove_datalit::tydesc_table::TyDescTable::new(db),
            call_stack: Vec::new(),
        }
    }

    /// Set the current script for execution.
    pub fn set_script(&mut self, script: crate::script::Script) {
        self.script = Some(script);
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

impl<'db> ModuleFunctionTable<'db> {
    /// Create a new empty module function table.
    pub fn new() -> ModuleFunctionTable<'db> {
        ModuleFunctionTable {
            imported_functions: HashMap::new(),
            module_all_functions: HashMap::new(),
        }
    }

    /// Build module function table from script statements and package world.
    ///
    /// This processes require module and import statements to resolve imported functions.
    /// Eagerly parses imported modules to extract function definitions.
    pub fn build_from_script(
        db: &'db dyn crate::Db,
        script: crate::script::Script,
        package_world: PackageWorld,
    ) -> ModuleFunctionTable<'db> {
        let mut table = ModuleFunctionTable::new();

        let module_alias_map = build_module_alias_map(db, script, package_world);

        for (_module_alias, module) in &module_alias_map {
            let functions = parse_module_functions(db, *module);

            let mut module_funcs = HashMap::new();
            for (func_name, func) in functions {
                module_funcs.insert(func_name, func);
            }
            table.module_all_functions.insert(*module, module_funcs);
        }

        let units = script.units(db);
        for unit_index in 0..units.len() {
            let parsed = crate::parser::parse_script_unit(db, script, unit_index);

            for statement in parsed.statements(db) {
                if let ast::Statement::Import(import_stmt) = statement {
                    let module_name = import_stmt.module_name(db);
                    let item_name = import_stmt.item_name(db);

                    if let Some(&module) = module_alias_map.get(&module_name) {
                        if let Some(module_funcs) = table.module_all_functions.get(&module) {
                            if let Some(&func) = module_funcs.get(&item_name) {
                                table.imported_functions.insert(item_name, (func, module));
                            }
                        }
                    }
                }
            }
        }

        table
    }

    /// Build module function table from a module dependency graph.
    ///
    /// Parses ALL modules in the graph, including transitive dependencies.
    /// This is needed for cross-module function calls.
    pub fn build_from_graph(
        db: &'db dyn crate::Db,
        graph: bct::package_resolve2::PackageWorldModuleGraph<'db>,
    ) -> ModuleFunctionTable<'db> {
        let mut table = ModuleFunctionTable::new();

        for module in graph.map(db).keys() {
            let functions = parse_module_functions(db, *module);

            let mut module_funcs = HashMap::new();
            for (func_name, func) in functions {
                module_funcs.insert(func_name, func);
            }
            table.module_all_functions.insert(*module, module_funcs);
        }

        table
    }

    /// Populate script-level imports (modifies the table in place).
    pub fn populate_script_imports(
        &mut self,
        db: &'db dyn crate::Db,
        script: crate::script::Script,
        package_world: PackageWorld,
    ) {
        let module_alias_map = build_module_alias_map(db, script, package_world);

        let units = script.units(db);
        for unit_index in 0..units.len() {
            let parsed = crate::parser::parse_script_unit(db, script, unit_index);

            for statement in parsed.statements(db) {
                if let ast::Statement::Import(import_stmt) = statement {
                    let module_name = import_stmt.module_name(db);
                    let item_name = import_stmt.item_name(db);

                    if let Some(&module) = module_alias_map.get(&module_name) {
                        if let Some(module_funcs) = self.module_all_functions.get(&module) {
                            if let Some(&func) = module_funcs.get(&item_name) {
                                self.imported_functions.insert(item_name, (func, module));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Look up an imported function by name.
    ///
    /// Returns the function and its source module.
    pub fn get(&self, name: InternedText<'db>) -> Option<(ast::StmtFun<'db>, crate::package::PackageModule)> {
        self.imported_functions.get(&name).copied()
    }

    /// Get all functions from a module.
    pub fn get_module_functions(&self, module: crate::package::PackageModule) -> Option<&HashMap<InternedText<'db>, ast::StmtFun<'db>>> {
        self.module_all_functions.get(&module)
    }
}

// ============================================================================
// Module Loading
// ============================================================================

/// Parse a module and extract all function definitions.
#[salsa::tracked]
pub(super) fn parse_module_functions<'db>(
    db: &'db dyn crate::Db,
    module: crate::package::PackageModule,
) -> Vec<(InternedText<'db>, ast::StmtFun<'db>)> {
    let module_source = module.text(db);
    let parse_result = crate::parser::parse(db, module_source);
    let parsed = parse_result.script(db);

    let mut functions = Vec::new();
    for statement in parsed.statements(db) {
        if let ast::Statement::Fun(func) = statement {
            let func_name = func.name(db);
            functions.push((func_name, *func));
        }
    }

    functions
}

/// Build module alias map for a script.
///
/// Maps module aliases (from require statements) to actual package modules.
pub(super) fn build_module_alias_map<'db>(
    db: &'db dyn crate::Db,
    script: crate::script::Script,
    package_world: PackageWorld,
) -> HashMap<InternedText<'db>, crate::package::PackageModule> {
    use crate::ast::{Statement, StmtRequire};

    let mut alias_map = HashMap::new();

    // Build hierarchy map: (import_space, package_name, module_name) → PackageModule.
    let mut hierarchy_map = HashMap::new();
    let world_map = crate::package::package_world_map(db, package_world);

    for (import_space, packages) in world_map.map(db) {
        for (package_name, package) in packages {
            for (module_name, module) in package.modules(db) {
                let key = (
                    import_space.as_str().to_string(),
                    package_name.as_str().to_string(),
                    module_name.as_str().to_string(),
                );
                hierarchy_map.insert(key, *module);
            }
        }
    }

    // Process all units to find require module statements.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        let parsed = crate::parser::parse_script_unit(db, script, unit_index);

        for statement in parsed.statements(db) {
            if let Statement::Require(StmtRequire::Module(req)) = statement {
                let import_space = req.import_space(db);
                let package_alias = req.package_alias(db);
                let module_alias = req.module_alias(db);

                let key = (
                    import_space.as_str(db).to_string(),
                    package_alias.as_str(db).to_string(),
                    module_alias.as_str(db).to_string(),
                );

                if let Some(&module) = hierarchy_map.get(&key) {
                    alias_map.insert(module_alias, module);
                }
            }
        }
    }

    alias_map
}

/// Helper to clean up script scope variables.
pub(super) fn cleanup_script_scope(ctx: &mut InterpContext<'_>) {
    let remaining_vars: Vec<_> = ctx.script_scope.variables.drain()
        .filter_map(|(_, var)| {
            if var.state == ScriptVarState::Available {
                Some(var.value)
            } else {
                None
            }
        })
        .collect();
    for value in remaining_vars {
        destroy_value(ctx, value);
    }
}
