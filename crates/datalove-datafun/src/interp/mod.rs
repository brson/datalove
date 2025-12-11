//! Analysis-driven interpreter with linear type semantics.
//!
//! # Architecture
//!
//! The interpreter executes datafun code in two modes:
//!
//! - **Script scope**: Top-level statements (let bindings, function definitions)
//!   execute directly with script-level variable tracking.
//!
//! - **Frame-based execution**: Function bodies execute using a stack frame with
//!   slots computed by `function_analysis`. This enables destination-passing style
//!   (DPS) to minimize heap allocations.
//!
//! # Value Ownership
//!
//! Values track ownership via `ValueLocation`:
//! - `Borrowed`: Points into frame memory or caller's data. Don't free.
//! - `TempOwned`: Heap-allocated temporary. Must free structure after use.
//!
//! Linear types are enforced by marking slots as `Moved` after use.
//! Copy types are cloned transparently.
//!
//! # Key Types
//!
//! - [`InterpContext`]: Main interpreter state (runtime, package world, call stack)
//! - [`Value`]: Runtime value with pointer, type descriptor, and ownership
//! - [`StackFrame`]: Function execution frame with slot storage
//! - [`ScriptScope`]: Top-level variable bindings for REPL/script execution

mod value;
mod error;
mod frame;
mod memory;
mod types;
mod alloc;
mod arith;
mod arith_widening;
mod collections;
mod coerce;
mod literals;

pub use value::{Value, Destination, ValueLocation, EvalContext};
pub use error::InterpError;
pub use frame::{SlotState, StackFrame};
pub use memory::{destroy_value, destroy_value_contents_only, free_value_structure};
use frame::CfgControl;
use memory::{clone_value, clone_value_to_dest};
use types::{is_u32_value, is_int_value, is_f32_value, is_bool_value, is_copy_type};
use alloc::{
    allocate_bool, allocate_f32, allocate_u32_raw, allocate_bigint,
    allocate_option_none, allocate_option_some_from_value,
    allocate_result_ok_from_value, allocate_result_err, widen_u32_to_int,
};
use arith::{
    write_u32_result, write_f32_result, write_bool_result, write_option_u32_result,
    eval_add_checked, eval_sub_checked, eval_mul_checked, eval_div_checked,
    eval_add_optional, eval_sub_optional, eval_mul_optional, eval_div_optional,
    eval_comparison,
};
use collections::{
    allocate_tuple_from_values, allocate_struct_from_values,
    allocate_list_from_values, allocate_map_from_values, allocate_set_from_values,
};
use literals::{
    allocate_float_literal, allocate_int_literal, allocate_string,
    allocate_inline_int_literal, write_inline_int_to_dest, write_option_none_to_dest,
    allocate_inline_string,
};
use arith_widening::{execute_binop, execute_unop};
use coerce::{narrow_int_to_u32, coerce_value_to_dest};

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use bct::text::InternedText;

use crate::package::PackageWorld;
use crate::ast::{self, StmtFun};
use crate::function_analysis::{ControlFlowGraph, Terminator, BlockId};

// ============================================================================
// Context Types
// ============================================================================

/// Interpreter context for script execution.
pub struct InterpContext<'db> {
    db: &'db dyn crate::Db,
    runtime: datalove_rt::rust::Runtime,
    package_world: PackageWorld,
    script: Option<crate::script::Script>,
    pub script_scope: ScriptScope<'db>,
    module_functions: ModuleFunctionTable<'db>,
    /// Current module being executed (for module-internal function calls).
    current_module: Option<crate::package::PackageModule>,
    /// Typecheck result for the package world (includes module exports).
    typecheck_result: Option<crate::tycheck::PackageWorldTypecheckResult<'db>>,
    /// Script-level function analyses (for functions defined in the script).
    script_function_analyses: HashMap<ast::StmtFun<'db>, crate::function_analysis::FunctionAnalysis<'db>>,
    tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
    /// Call stack for frame-based execution.
    call_stack: Vec<StackFrame<'db>>,
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
    pub is_copy: bool,  // Cached from type analysis.
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
            let error_count = module_errors.values().map(|v| v.len()).sum::<usize>();
            return Err(InterpError::TypecheckErrors(error_count));
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
            // Get runtime handle.
            let rt_handle = self.runtime.handle();

            // Get string type descriptor from the tydesc_table.
            let string_tydesc = self.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::String);

            // Create output string.
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

            // Pretty-print value.
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

            // Extract string contents.
            let result = if output_string.data.is_null() || output_string.size == 0 {
                String::new()
            } else {
                let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
                String::from_utf8_lossy(bytes).to_string()
            };

            // Cleanup.
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

        // Build module alias map (module_alias → PackageModule).
        let module_alias_map = build_module_alias_map(db, script, package_world);

        // Parse all imported modules and cache their functions.
        for (_module_alias, module) in &module_alias_map {
            // Parse the module using tracked function.
            let functions = parse_module_functions(db, *module);

            // Store all functions from this module.
            let mut module_funcs = HashMap::new();
            for (func_name, func) in functions {
                module_funcs.insert(func_name, func);
            }
            table.module_all_functions.insert(*module, module_funcs);
        }

        // Process all units to find import statements and resolve functions.
        let units = script.units(db);
        for unit_index in 0..units.len() {
            let parsed = crate::parser::parse_script_unit(db, script, unit_index);

            for statement in parsed.statements(db) {
                if let ast::Statement::Import(import_stmt) = statement {
                    let module_name = import_stmt.module_name(db);
                    let item_name = import_stmt.item_name(db);

                    // Resolve the module and look up the function.
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

        // Parse all modules in the graph and cache their functions.
        for module in graph.map(db).keys() {
            let functions = parse_module_functions(db, *module);

            let mut module_funcs = HashMap::new();
            for (func_name, func) in functions {
                module_funcs.insert(func_name, func);
            }
            table.module_all_functions.insert(*module, module_funcs);
        }

        // Note: We don't populate imported_functions here because that's script-specific.
        // The script's import statements will be handled separately.

        table
    }

    /// Populate script-level imports (modifies the table in place).
    pub fn populate_script_imports(
        &mut self,
        db: &'db dyn crate::Db,
        script: crate::script::Script,
        package_world: PackageWorld,
    ) {
        // Build module alias map for the script.
        let module_alias_map = build_module_alias_map(db, script, package_world);

        // Process all units to find import statements and resolve functions.
        let units = script.units(db);
        for unit_index in 0..units.len() {
            let parsed = crate::parser::parse_script_unit(db, script, unit_index);

            for statement in parsed.statements(db) {
                if let ast::Statement::Import(import_stmt) = statement {
                    let module_name = import_stmt.module_name(db);
                    let item_name = import_stmt.item_name(db);

                    // Resolve the module and look up the function.
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
fn parse_module_functions<'db>(
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
fn build_module_alias_map<'db>(
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

// ============================================================================
// Script Execution Entry Points
// ============================================================================

/// Execute a complete script in batch mode.
///
/// This is the top-level entry point for running a complete script file
/// against a package world. The caller must have already typechecked the
/// package world and script; the interpreter will refuse to run if there
/// are any typecheck errors.
pub fn execute_script<'db>(
    db: &'db dyn crate::Db,
    script: crate::script::Script,
    package_world: PackageWorld,
    typecheck_result: crate::tycheck::PackageWorldTypecheckResult<'db>,
) -> Result<ScriptResult<'db>, InterpError> {
    // Create interpreter context (validates typecheck result has no errors).
    let mut ctx = InterpContext::new_with_typecheck(db, package_world, typecheck_result)?;
    ctx.script = Some(script);

    // Populate script-level imports.
    ctx.module_functions.populate_script_imports(db, script, package_world);

    // Typecheck and analyze script-level functions.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        let parsed_unit = crate::parser::parse_script_unit(db, script, unit_index);
        let unit_source = units[unit_index].source(db);

        // Typecheck the script unit with package world context.
        let unit_typecheck = crate::tycheck::type_check_with_package_world(
            db,
            unit_source,
            parsed_unit,
            package_world,
            typecheck_result,
        );

        // Check for script unit typecheck errors.
        let errors = unit_typecheck.errors(db);
        if !errors.is_empty() {
            return Err(InterpError::TypecheckErrors(errors.len()));
        }

        // Analyze each function in the unit.
        for statement in parsed_unit.statements(db) {
            if let crate::ast::Statement::Fun(func_stmt) = statement {
                let analysis = crate::function_analysis::analyze_function(db, *func_stmt, unit_typecheck);
                ctx.script_function_analyses.insert(*func_stmt, analysis);
            }
        }
    }

    // Helper to cleanup script scope variables.
    fn cleanup_script_scope(ctx: &mut InterpContext<'_>) {
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

    // Execute all script units.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        if let Err(e) = execute_unit(&mut ctx, script, unit_index) {
            cleanup_script_scope(&mut ctx);
            return Err(e);
        }
    }

    // Return the output variable if present.
    // Remove it from the HashMap to avoid double-free.
    let output_name = bct::text::InternedText::new(db, "output");
    let value = match ctx.script_scope.variables.remove(&output_name) {
        Some(var) => var.value,
        None => {
            cleanup_script_scope(&mut ctx);
            return Err(InterpError::NoOutputVariable);
        }
    };

    // Clean up any remaining variables before moving out runtime and tydesc_table.
    cleanup_script_scope(&mut ctx);

    // Return the value, runtime, and tydesc_table (which keeps the memory alive).
    Ok(ScriptResult {
        value,
        runtime: ctx.runtime,
        tydesc_table: ctx.tydesc_table,
    })
}

/// Pretty-print a value using the runtime pretty printer.
///
/// Returns a string representation in valid datalit syntax.
pub fn pretty_print_value<'db>(
    script_result: &mut ScriptResult<'db>,
) -> Result<String, InterpError> {
    use datalove_rt as rt;
    use datalove_rt::rtdt;

    unsafe {
        // Get runtime handle.
        let rt_handle = script_result.runtime.handle();

        // Get string type descriptor from the tydesc_table.
        let string_tydesc = script_result.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::String);

        // Create output string.
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

        // Pretty-print value.
        let status = rt::c::dtlv_rti_pretty_print_local(
            rt_handle,
            script_result.value.ptr,
            script_result.value.tydesc,
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

        // Extract string contents.
        let result = if output_string.data.is_null() || output_string.size == 0 {
            String::new()
        } else {
            let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
            String::from_utf8_lossy(bytes).to_string()
        };

        // Cleanup.
        rt::c::dtlv_rti_string_destroy_local(
            rt_handle,
            &mut output_string as *mut rtdt::String as *mut u8,
            string_tydesc,
        );

        Ok(result)
    }
}

/// Execute a single script unit in REPL mode.
///
/// This is used for incremental REPL execution where we only execute
/// the newly added unit while maintaining state from previous units.
///
/// The caller must have created the InterpContext with `new_with_typecheck`
/// to ensure the context has been properly initialized with typecheck results.
pub fn execute_script_unit<'db>(
    ctx: &mut InterpContext<'db>,
    script: crate::script::Script,
    unit_index: usize,
) -> Result<Option<Value>, InterpError> {
    // Verify context has typecheck result.
    if ctx.typecheck_result.is_none() {
        return Err(InterpError::RuntimeError(
            "InterpContext not initialized with typecheck result".to_string()
        ));
    }

    // Update context with new script.
    ctx.script = Some(script);

    // Execute only the new unit.
    execute_unit(ctx, script, unit_index)?;

    // Return the value of the last expression if any.
    Ok(None)
}

/// Execute a single script unit.
fn execute_unit<'db>(
    ctx: &mut InterpContext<'db>,
    script: crate::script::Script,
    unit_index: usize,
) -> Result<(), InterpError> {
    // Parse the unit (Salsa will memoize this).
    let parsed = crate::parser::parse_script_unit(ctx.db, script, unit_index);

    // Execute each statement in the unit.
    for stmt in parsed.statements(ctx.db) {
        execute_statement(ctx, stmt)?;
    }

    Ok(())
}

// ============================================================================
// Statement Execution
// ============================================================================

/// Execute a single statement at script level.
fn execute_statement<'db>(
    ctx: &mut InterpContext<'db>,
    stmt: &ast::Statement<'db>,
) -> Result<(), InterpError> {
    match stmt {
        ast::Statement::Let(let_stmt) => {
            execute_let_statement(ctx, *let_stmt)?;
        }
        ast::Statement::Fun(fun_stmt) => {
            execute_fun_statement(ctx, *fun_stmt)?;
        }
        ast::Statement::Ret(_) => {
            return Err(InterpError::ReturnOutsideFunction);
        }
        ast::Statement::If(_) => {
            return Err(InterpError::IfOutsideFunction);
        }
        ast::Statement::Require(_) | ast::Statement::Import(_) => {
            // Already handled by package world loading.
            // Nothing to execute at runtime.
        }
        ast::Statement::ParseError(_) => {
            // Parse errors should have been caught by typechecking.
            return Err(InterpError::InvalidExpression("Parse error".to_string()));
        }
    }

    Ok(())
}

/// Execute a let statement at script level.
fn execute_let_statement<'db>(
    ctx: &mut InterpContext<'db>,
    let_stmt: ast::StmtLet<'db>,
) -> Result<(), InterpError> {
    use crate::datalit::ast::TypeHint;

    // Helper to check if expression is @none or @error.
    let is_none_or_error = |expr: ast::ExprFun<'db>, db: &'db dyn crate::Db| -> bool {
        matches!(
            expr.expr(db),
            ast::ExprFunKind::None(_) | ast::ExprFunKind::Err(_)
        )
    };

    // Check if we need to coerce T → Option<T> or T → Result<T>.
    let final_value = if let Some(type_hint_and_heap) = let_stmt.type_hint(ctx.db) {
        let type_hint = type_hint_and_heap.type_hint(ctx.db);
        match type_hint {
            TypeHint::Option(_) | TypeHint::Result(_) => {
                // Get expected destination type.
                let dest_tydesc = type_hint_to_tydesc(ctx, type_hint_and_heap);

                // Check if expression is @none or @error - these need the destination type for context.
                if is_none_or_error(let_stmt.value(ctx.db), ctx.db) {
                    // Allocate destination and evaluate with type context.
                    let dest_ptr = unsafe {
                        datalove_rt::c::dtlv_rti_mem_alloc_local(
                            ctx.runtime.handle(),
                            dest_tydesc,
                            1,
                        )
                    };
                    if dest_ptr.is_null() {
                        return Err(InterpError::RuntimeError(
                            "Failed to allocate destination for @none/@error".to_string()
                        ));
                    }
                    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };

                    // Evaluate with destination - @none/@error will use the type context.
                    let mut value = eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), Some(dest))?;
                    // We allocated the destination, so we own it - mark as TempOwned.
                    value.location = ValueLocation::TempOwned;
                    value
                } else {
                    // Evaluate expression first (not @none/@error).
                    let value = eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), None)?;

                    let dest_ptr = unsafe {
                        datalove_rt::c::dtlv_rti_mem_alloc_local(
                            ctx.runtime.handle(),
                            dest_tydesc,
                            1,
                        )
                    };
                    if dest_ptr.is_null() {
                        destroy_value(ctx, value);
                        return Err(InterpError::RuntimeError(
                            "Failed to allocate destination for let coercion".to_string()
                        ));
                    }
                    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };

                    // Coerce value to destination (T → Option<T> or T → Result<T>).
                    coerce_value_to_dest(ctx, value, dest)?
                }
            }
            _ => {
                // No coercion needed, evaluate normally.
                eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), None)?
            }
        }
    } else {
        // No type hint, evaluate normally.
        eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), None)?
    };

    // Determine if the type is copy (basic detection).
    let is_copy = is_copy_type(final_value);

    // Bind to script-level variable.
    let name = let_stmt.name(ctx.db);
    ctx.script_scope.variables.insert(name, ScriptVariable {
        value: final_value,
        state: ScriptVarState::Available,
        is_copy,
    });

    Ok(())
}

/// Execute a function definition statement at script level.
fn execute_fun_statement<'db>(
    ctx: &mut InterpContext<'db>,
    fun_stmt: StmtFun<'db>,
) -> Result<(), InterpError> {
    // Add function to script scope.
    let name = fun_stmt.name(ctx.db);
    ctx.script_scope.functions.insert(name, fun_stmt);

    // Typecheck and analyze the function.
    // We need a typecheck result - use the package world typecheck if available.
    if let Some(typecheck_result) = ctx.typecheck_result {
        // Create a minimal script containing just this function for typechecking.
        let script = ctx.script.ok_or_else(|| {
            InterpError::RuntimeError("No script set in context".to_string())
        })?;

        // Find the unit that contains this function and get its source.
        let units = script.units(ctx.db);
        for unit_index in 0..units.len() {
            let parsed_unit = crate::parser::parse_script_unit(ctx.db, script, unit_index);
            for stmt in parsed_unit.statements(ctx.db) {
                if let crate::ast::Statement::Fun(f) = stmt {
                    if *f == fun_stmt {
                        let unit_source = units[unit_index].source(ctx.db);
                        let unit_typecheck = crate::tycheck::type_check_with_package_world(
                            ctx.db,
                            unit_source,
                            parsed_unit,
                            ctx.package_world,
                            typecheck_result,
                        );
                        let analysis = crate::function_analysis::analyze_function(
                            ctx.db,
                            fun_stmt,
                            unit_typecheck,
                        );
                        ctx.script_function_analyses.insert(fun_stmt, analysis);
                        return Ok(());
                    }
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Expression Evaluation - Script Scope
// ============================================================================

/// Evaluate an expression in script scope.
fn eval_expression_in_script_scope<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // Read variable from script scope.
            read_script_variable(ctx, name)
        }
        ast::ExprFunKind::FunctionCall(call_expr) => {
            // Evaluate function call in script scope.
            eval_function_call_in_script_scope(ctx, call_expr)
        }
        ast::ExprFunKind::BinOp(binop_expr) => {
            // Evaluate binary operations (operands don't have destinations).
            let lhs = eval_expression_in_script_scope(ctx, binop_expr.lhs(ctx.db), None)?;
            let rhs = match eval_expression_in_script_scope(ctx, binop_expr.rhs(ctx.db), None) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };
            execute_binop(ctx, binop_expr.op(ctx.db), lhs, rhs, dest)
        }
        ast::ExprFunKind::Tuple(tuple_expr) => {
            // Evaluate each element in script scope.
            let elements = tuple_expr.elements(ctx.db);
            let mut values = Vec::with_capacity(elements.len());

            for elem in elements {
                match eval_expression_in_script_scope(ctx, *elem, None) {
                    Ok(v) => values.push(v),
                    Err(e) => {
                        // Clean up already-evaluated values on error.
                        for v in values {
                            destroy_value(ctx, v);
                        }
                        return Err(e);
                    }
                }
            }

            allocate_tuple_from_values(ctx, values)
        }
        ast::ExprFunKind::UnaryOp(unary_expr) => {
            // Evaluate operand.
            let operand = eval_expression_in_script_scope(ctx, unary_expr.operand(ctx.db), None)?;
            // Execute unary operation.
            execute_unop(ctx, unary_expr.op(ctx.db), operand, dest)
        }
        ast::ExprFunKind::TryOption(try_op) => {
            // Evaluate operand.
            let operand = eval_expression_in_script_scope(ctx, try_op.operand(ctx.db), None)?;
            // Apply try-option operator.
            eval_try_option(ctx, operand)
        }
        ast::ExprFunKind::TryResult(try_op) => {
            // Evaluate operand.
            let operand = eval_expression_in_script_scope(ctx, try_op.operand(ctx.db), None)?;
            // Apply try-result operator.
            eval_try_result(ctx, operand)
        }
        ast::ExprFunKind::ParseError(_) => {
            Err(InterpError::InvalidExpression("Parse error in expression".to_string()))
        }

        // Inline literal variants.
        ast::ExprFunKind::True(_) => allocate_bool(ctx, true),
        ast::ExprFunKind::False(_) => allocate_bool(ctx, false),
        ast::ExprFunKind::None(_) => {
            // @none without destination - requires type context.
            if let Some(d) = dest {
                write_option_none_to_dest(d)
            } else {
                Err(InterpError::InvalidExpression(
                    "@none literal requires type context".to_string()
                ))
            }
        }
        ast::ExprFunKind::Int(int_expr) => {
            if let Some(d) = dest {
                write_inline_int_to_dest(ctx, &int_expr, d)
            } else {
                allocate_inline_int_literal(ctx, &int_expr)
            }
        }
        ast::ExprFunKind::Float(float_expr) => {
            let value_str = float_expr.value(ctx.db).as_str(ctx.db);
            let value: f32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse float: {}", e)))?;
            allocate_f32(ctx, value)
        }
        ast::ExprFunKind::Hex(hex_expr) => {
            let value_str = hex_expr.value(ctx.db).as_str(ctx.db);
            let hex_digits = value_str.trim_start_matches("0x").trim_start_matches("0X");
            let value: u32 = u32::from_str_radix(hex_digits, 16)
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse hex: {}", e)))?;
            allocate_u32_raw(ctx, value)
        }
        ast::ExprFunKind::String(string_expr) => {
            allocate_inline_string(ctx, &string_expr)
        }
        ast::ExprFunKind::List(list_expr) => {
            eval_inline_list(ctx, EvalContext::ScriptScope, &list_expr, dest)
        }
        ast::ExprFunKind::Set(set_expr) => {
            eval_inline_set(ctx, EvalContext::ScriptScope, &set_expr, dest)
        }
        ast::ExprFunKind::Map(map_expr) => {
            eval_inline_map(ctx, EvalContext::ScriptScope, &map_expr, dest)
        }
        ast::ExprFunKind::Tensor(_) => {
            Err(InterpError::InvalidExpression("Tensor not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonTuple(tuple_expr) => {
            eval_inline_anon_tuple(ctx, EvalContext::ScriptScope, &tuple_expr, dest)
        }
        ast::ExprFunKind::NamedTuple(_) => {
            Err(InterpError::InvalidExpression("Named tuple not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonStruct(struct_expr) => {
            eval_inline_anon_struct(ctx, EvalContext::ScriptScope, &struct_expr, dest)
        }
        ast::ExprFunKind::NamedStruct(_) => {
            Err(InterpError::InvalidExpression("Named struct not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonEnum(_) | ast::ExprFunKind::NamedEnum(_) => {
            Err(InterpError::InvalidExpression("Enum not yet implemented".to_string()))
        }
        ast::ExprFunKind::Data(_) => {
            Err(InterpError::InvalidExpression("Data wrapper not yet implemented".to_string()))
        }
        ast::ExprFunKind::Err(_) => {
            // @error without destination - requires type context.
            Err(InterpError::InvalidExpression(
                "@error literal requires type context".to_string()
            ))
        }
    }
}

/// Read a script-level variable with linear semantics enforcement.
fn read_script_variable<'db>(
    ctx: &mut InterpContext<'db>,
    name: InternedText<'db>,
) -> Result<Value, InterpError> {
    // Get variable info first.
    let (value, is_copy) = {
        let var = ctx.script_scope.variables.get(&name)
            .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

        // Check if already moved.
        if var.state == ScriptVarState::Moved {
            return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
        }

        (var.value, var.is_copy)
    };

    if is_copy {
        // Copy types: clone the value, keep state Available.
        Ok(clone_value(ctx, value))
    } else {
        // Linear types: move the value, mark as Moved.
        ctx.script_scope.variables.get_mut(&name).unwrap().state = ScriptVarState::Moved;
        Ok(value)
    }
}

// ============================================================================
// Function Calls and Execution
// ============================================================================

/// Look up a function by name in script scope or imported modules.
///
/// Returns the function definition and its source module (if from a module).
/// Looks in this order:
/// 1. Script-level functions
/// 2. Current module functions (if executing inside a module)
/// 3. Imported module functions
fn lookup_function<'db>(
    ctx: &InterpContext<'db>,
    name: InternedText<'db>,
) -> Result<(ast::StmtFun<'db>, Option<crate::package::PackageModule>), InterpError> {
    // First check script scope.
    if let Some(&func) = ctx.script_scope.functions.get(&name) {
        return Ok((func, None));
    }

    // Then check current module functions (for module-internal calls).
    if let Some(current_module) = ctx.current_module {
        if let Some(module_funcs) = ctx.module_functions.get_module_functions(current_module) {
            if let Some(&func) = module_funcs.get(&name) {
                return Ok((func, Some(current_module)));
            }
        }

        // Check what the current module imported from other modules using typecheck result.
        if let Some(typecheck_result) = &ctx.typecheck_result {
            let module_imports_map = typecheck_result.module_imports(ctx.db);

            if let Some(imports) = module_imports_map.get(&current_module) {
                // Look for the function in the imports.
                for (local_name, source_module, _source_name) in imports.functions(ctx.db) {
                    if *local_name == name {
                        // Found the import - look up the function AST from the source module.
                        if let Some(module_funcs) = ctx.module_functions.get_module_functions(*source_module) {
                            if let Some(&func_ast) = module_funcs.get(&name) {
                                return Ok((func_ast, Some(*source_module)));
                            }
                        }
                    }
                }
            }
        }
    }

    // Finally check imported module functions (script-level imports).
    if let Some((func, module)) = ctx.module_functions.get(name) {
        return Ok((func, Some(module)));
    }

    // Function not found.
    Err(InterpError::FunctionNotFound(name.text(ctx.db).to_string()))
}

/// Evaluate a function call from script scope.
fn eval_function_call_in_script_scope<'db>(
    ctx: &mut InterpContext<'db>,
    call_expr: ast::ExprFunctionCall<'db>,
) -> Result<Value, InterpError> {
    let name = call_expr.name(ctx.db);
    let arg_exprs = call_expr.args(ctx.db);

    // Look up the function (script or module).
    let (func, func_module) = lookup_function(ctx, name)?;

    let params = func.params(ctx.db);

    // Check argument count matches parameter count.
    if arg_exprs.len() != params.len() {
        return Err(InterpError::InvalidExpression(
            format!("Function '{}' expects {} arguments but {} provided",
                name.text(ctx.db), params.len(), arg_exprs.len())
        ));
    }

    // Evaluate all arguments and coerce to parameter types where needed.
    let mut arg_values = Vec::new();
    for (i, arg_expr) in arg_exprs.iter().enumerate() {
        // Get parameter type.
        let param_type_hint = params[i].type_hint(ctx.db);
        let param_tydesc = type_hint_to_tydesc(ctx, param_type_hint);

        // Check if parameter is Option or Result - these need special coercion handling.
        let param_tag = unsafe { (*param_tydesc).type_tag };
        let needs_coercion = param_tag == datalove_rt::rtdt::TyTag::Option
                          || param_tag == datalove_rt::rtdt::TyTag::Result;

        if needs_coercion {
            // Check if argument is @none or @error - these should be evaluated directly
            // with the parameter type, not the inner type.
            let is_none_or_error = matches!(
                arg_expr.expr(ctx.db),
                ast::ExprFunKind::None(_) | ast::ExprFunKind::Err(_)
            );

            if is_none_or_error {
                // Evaluate @none/@error directly with parameter type destination.
                let param_ptr = unsafe {
                    datalove_rt::c::dtlv_rti_mem_alloc_local(
                        ctx.runtime.handle(),
                        param_tydesc,
                        1
                    )
                };
                if param_ptr.is_null() {
                    for val in arg_values { destroy_value(ctx, val); }
                    return Err(InterpError::RuntimeError("Failed to allocate argument buffer".to_string()));
                }
                let param_dest = Destination { ptr: param_ptr, tydesc: param_tydesc };
                let value = match eval_expression_in_script_scope(ctx, *arg_expr, Some(param_dest)) {
                    Ok(v) => {
                        if v.location == ValueLocation::Borrowed && v.ptr == param_ptr {
                            // Expression wrote to destination and returned borrowed ref.
                            Value { ptr: param_ptr, tydesc: param_tydesc, location: ValueLocation::TempOwned }
                        } else {
                            // Expression returned a different value - free param buffer and use value.
                            unsafe {
                                datalove_rt::c::dtlv_rti_mem_free_local(
                                    ctx.runtime.handle(),
                                    param_tydesc,
                                    1,
                                    param_ptr,
                                );
                            }
                            v
                        }
                    }
                    Err(e) => {
                        unsafe {
                            datalove_rt::c::dtlv_rti_mem_free_local(
                                ctx.runtime.handle(),
                                param_tydesc,
                                1,
                                param_ptr,
                            );
                        }
                        for val in arg_values { destroy_value(ctx, val); }
                        return Err(e);
                    }
                };
                arg_values.push(value);
                continue;
            }

            // For Option<T>/Result<T> parameters, try to evaluate as inner type T first.
            let inner_tydesc = if param_tag == datalove_rt::rtdt::TyTag::Option {
                let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(param_tydesc) };
                tydesc_ref.option_inner_ty().as_ptr()
            } else {
                let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(param_tydesc) };
                tydesc_ref.result_ok_ty().as_ptr()
            };

            // Allocate buffer for inner type.
            let inner_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(
                    ctx.runtime.handle(),
                    inner_tydesc,
                    1
                )
            };
            if inner_ptr.is_null() {
                for val in arg_values { destroy_value(ctx, val); }
                return Err(InterpError::RuntimeError("Failed to allocate argument buffer".to_string()));
            }

            let inner_dest = Destination { ptr: inner_ptr, tydesc: inner_tydesc };

            // Try to evaluate to inner type.
            let inner_value = match eval_expression_in_script_scope(ctx, *arg_expr, Some(inner_dest)) {
                Ok(v) => {
                    if v.location == ValueLocation::Borrowed && v.ptr == inner_ptr {
                        Value { ptr: inner_ptr, tydesc: inner_tydesc, location: ValueLocation::TempOwned }
                    } else {
                        // Expression returned a different value - free inner buffer and use value directly.
                        unsafe {
                            datalove_rt::c::dtlv_rti_mem_free_local(
                                ctx.runtime.handle(),
                                inner_tydesc,
                                1,
                                inner_ptr,
                            );
                        }
                        v
                    }
                }
                Err(e) => {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_local(
                            ctx.runtime.handle(),
                            inner_tydesc,
                            1,
                            inner_ptr,
                        );
                    }
                    for val in arg_values { destroy_value(ctx, val); }
                    return Err(e);
                }
            };

            // Now check if we need to wrap in Option/Result.
            if inner_value.tydesc == inner_tydesc {
                // Value matches inner type - wrap in Option/Result.
                let param_ptr = unsafe {
                    datalove_rt::c::dtlv_rti_mem_alloc_local(
                        ctx.runtime.handle(),
                        param_tydesc,
                        1
                    )
                };
                if param_ptr.is_null() {
                    destroy_value(ctx, inner_value);
                    for val in arg_values { destroy_value(ctx, val); }
                    return Err(InterpError::RuntimeError("Failed to allocate argument buffer".to_string()));
                }

                let dest = Destination { ptr: param_ptr, tydesc: param_tydesc };
                let value = coerce_value_to_dest(ctx, inner_value, dest)?;
                arg_values.push(value);
            } else {
                // Value didn't match inner type - check if it's already the Option/Result type.
                let value_tag = unsafe { (*inner_value.tydesc).type_tag };
                if value_tag == param_tag {
                    // Value is already an Option/Result - check inner types match.
                    let value_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(inner_value.tydesc) };
                    let value_inner = if value_tag == datalove_rt::rtdt::TyTag::Option {
                        value_ref.option_inner_ty()
                    } else {
                        value_ref.result_ok_ty()
                    };
                    let param_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(param_tydesc) };
                    let param_inner = if param_tag == datalove_rt::rtdt::TyTag::Option {
                        param_ref.option_inner_ty()
                    } else {
                        param_ref.result_ok_ty()
                    };

                    // Compare inner types by type tag (allow same type even if different tydesc ptrs).
                    if value_inner.type_tag() == param_inner.type_tag() {
                        // Types are compatible, use directly.
                        arg_values.push(inner_value);
                    } else {
                        destroy_value(ctx, inner_value);
                        for val in arg_values { destroy_value(ctx, val); }
                        return Err(InterpError::RuntimeError("Argument inner type mismatch for Option/Result parameter".to_string()));
                    }
                } else {
                    // Type mismatch - cleanup and error.
                    destroy_value(ctx, inner_value);
                    for val in arg_values { destroy_value(ctx, val); }
                    return Err(InterpError::RuntimeError("Argument type mismatch for Option/Result parameter".to_string()));
                }
            }
        } else {
            // Non-Option/Result parameter - use original simple evaluation.
            let value = match eval_expression_in_script_scope(ctx, *arg_expr, None) {
                Ok(v) => v,
                Err(e) => {
                    for val in arg_values { destroy_value(ctx, val); }
                    return Err(e);
                }
            };
            arg_values.push(value);
        }
    }

    // Execute the function body with arguments.
    // Set current_module if this is a module function.
    execute_function_body(ctx, func, func_module, arg_values)
}

/// Evaluate a function call from frame-based execution.
fn eval_function_call_frame<'db>(
    ctx: &mut InterpContext<'db>,
    call_expr: ast::ExprFunctionCall<'db>,
) -> Result<Value, InterpError> {
    let name = call_expr.name(ctx.db);
    let arg_exprs = call_expr.args(ctx.db);

    // Look up the function (script or module).
    let (func, func_module) = lookup_function(ctx, name)?;

    let params = func.params(ctx.db);

    // Check argument count matches parameter count.
    if arg_exprs.len() != params.len() {
        return Err(InterpError::InvalidExpression(
            format!("Function '{}' expects {} arguments but {} provided",
                name.text(ctx.db), params.len(), arg_exprs.len())
        ));
    }

    // Evaluate all arguments in frame context with temp slot destinations.
    let mut arg_values = Vec::new();
    for arg_expr in arg_exprs {
        // Get temp slot destination for this argument.
        let arg_dest = match get_destination_for_expr(ctx, *arg_expr) {
            Ok(d) => d,
            Err(e) => {
                for val in arg_values {
                    destroy_value(ctx, val);
                }
                return Err(e);
            }
        };
        let value = match eval_expression_frame(ctx, *arg_expr, Some(arg_dest)) {
            Ok(v) => {
                if v.location == ValueLocation::Borrowed {
                    mark_temp_slot_available(ctx, *arg_expr);
                }
                v
            }
            Err(e) => {
                // Clean up previously evaluated arguments on error.
                for val in arg_values {
                    destroy_value(ctx, val);
                }
                return Err(e);
            }
        };
        arg_values.push(value);
    }

    // Execute the function body with arguments.
    execute_function_body(ctx, func, func_module, arg_values)
}

/// Execute a function body and return its result.
///
/// This uses frame-based execution with analysis-driven slot allocation.
fn execute_function_body<'db>(
    ctx: &mut InterpContext<'db>,
    func: ast::StmtFun<'db>,
    func_module: Option<crate::package::PackageModule>,
    arg_values: Vec<Value>,
) -> Result<Value, InterpError> {
    // Helper to clean up arguments on early error (before frame execution).
    // All arguments must be destroyed since they were never used.
    fn cleanup_args_on_error(ctx: &mut InterpContext<'_>, arg_values: Vec<Value>) {
        for arg_value in arg_values {
            destroy_value(ctx, arg_value);
        }
    }

    // Helper to clean up arguments after successful frame execution.
    // For copy types: free the original structure (function cloned it).
    // For non-copy types: check if the slot was moved (ownership transferred).
    fn cleanup_args_after_frame(
        ctx: &mut InterpContext<'_>,
        arg_values: Vec<Value>,
        slot_states: &[SlotState],
        slots: &[crate::function_analysis::SlotInfo<'_>],
        params: &[ast::FunParam<'_>],
        db: &dyn crate::Db,
    ) {
        for (i, arg_value) in arg_values.into_iter().enumerate() {
            if is_copy_type(arg_value) {
                // Copy types were cloned by the function, free the original structure.
                free_value_structure(ctx, arg_value);
            } else {
                // Non-copy types: check if they were moved (consumed by function).
                let param_name = params[i].name(db);
                if let Some(slot_info) = slots.iter().find(|s| s.name(db) == Some(param_name)) {
                    let slot_id = slot_info.slot_id(db);
                    if slot_states[slot_id.0 as usize] != SlotState::Moved {
                        // Slot was never read/moved, so destroy the argument.
                        destroy_value(ctx, arg_value);
                    }
                    // If Moved, ownership was transferred, don't free.
                }
            }
        }
    }

    // Save previous module context and set current module.
    let prev_module = ctx.current_module;
    ctx.current_module = func_module;

    // Get function analysis.
    // First check script-level function analyses, then module analyses.
    let analysis = if let Some(analysis) = ctx.script_function_analyses.get(&func) {
        *analysis
    } else if let Some(typecheck_result) = &ctx.typecheck_result {
        let analyses = typecheck_result.function_analyses(ctx.db);
        match analyses.iter().find(|(f, _)| *f == func).map(|(_, a)| *a) {
            Some(a) => a,
            None => {
                ctx.current_module = prev_module;
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    format!("No analysis found for function '{}'", func.name(ctx.db).text(ctx.db))
                ));
            }
        }
    } else {
        ctx.current_module = prev_module;
        cleanup_args_on_error(ctx, arg_values);
        return Err(InterpError::RuntimeError(
            "No typecheck result available - cannot execute function".to_string()
        ));
    };

    // Check for critical analysis errors (ignore warnings like ValueNotUsed).
    let critical_errors: Vec<_> = analysis.errors(ctx.db)
        .iter()
        .filter(|e| !matches!(e, crate::function_analysis::AnalysisError::ValueNotUsed { .. }))
        .collect();

    if !critical_errors.is_empty() {
        ctx.current_module = prev_module;
        cleanup_args_on_error(ctx, arg_values);
        return Err(InterpError::RuntimeError(
            format!("Function '{}' has analysis errors: {:?}",
                func.name(ctx.db).text(ctx.db),
                critical_errors)
        ));
    }

    // Get frame layout and CFG.
    let layout = analysis.frame_layout(ctx.db);
    let cfg = analysis.control_flow(ctx.db);
    let total_size = layout.total_size(ctx.db) as usize;
    let slots = layout.slots(ctx.db);

    // Allocate frame data.
    let mut frame_data = vec![0u8; total_size];

    // Initialize slot states (all Available initially).
    let mut slot_states = vec![SlotState::Available; slots.len()];

    // Initialize parameters by writing argument values to their slot offsets.
    let params = func.params(ctx.db);
    for (i, param) in params.iter().enumerate() {
        // For now, only support In mode parameters.
        if param.mode(ctx.db) != ast::ParamMode::In {
            ctx.current_module = prev_module;
            cleanup_args_on_error(ctx, arg_values);
            return Err(InterpError::InvalidExpression(
                format!("Parameter mode {:?} not yet supported (function '{}')",
                    param.mode(ctx.db), func.name(ctx.db).text(ctx.db))
            ));
        }

        let param_name = param.name(ctx.db);
        let arg_value = arg_values[i];

        // Find the slot for this parameter.
        let slot_info = match slots.iter().find(|s| s.name(ctx.db) == Some(param_name)) {
            Some(s) => s,
            None => {
                ctx.current_module = prev_module;
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    format!("Parameter '{}' not found in frame layout", param_name.text(ctx.db))
                ));
            }
        };

        let offset = slot_info.offset(ctx.db) as usize;

        // Parameters are Reference slots (pointer-sized).
        // Store the pointer to the argument value.
        let ptr_bytes = arg_value.ptr as usize;
        frame_data[offset..offset + std::mem::size_of::<usize>()]
            .copy_from_slice(&ptr_bytes.to_ne_bytes());
    }

    // Create and push the stack frame.
    let frame = StackFrame {
        frame_data,
        slot_states,
        func,
        layout,
        cfg,
    };
    ctx.call_stack.push(frame);

    // Execute function body.
    let result = execute_function_body_with_frame(ctx);

    // Pop the frame and capture slot states for argument cleanup.
    let frame = ctx.call_stack.pop().unwrap();
    let final_slot_states = frame.slot_states.clone();

    // If result is Borrowed (pointing to frame memory), clone to heap before frame cleanup.
    // This is necessary because the frame memory will be deallocated.
    // We use proper cloning (not memcpy) to handle types with internal pointers.
    let result = match result {
        Ok(mut value) if value.location == ValueLocation::Borrowed => {
            // Allocate heap space and clone the value properly.
            unsafe {
                let heap_ptr = datalove_rt::c::dtlv_rti_mem_alloc_local(
                    ctx.runtime.handle(),
                    value.tydesc,
                    1,
                );
                if heap_ptr.is_null() {
                    return Err(InterpError::RuntimeError(
                        "Failed to allocate heap memory for return value".to_string()
                    ));
                }
                // Use proper clone to handle types with internal pointers (e.g., Int).
                let status = datalove_rt::c::dtlv_rti_clone_local(
                    ctx.runtime.handle(),
                    value.ptr,
                    value.tydesc,
                    heap_ptr,
                    value.tydesc,
                );
                if status != datalove_rt::c::RtStatus::Ok {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        ctx.runtime.handle(),
                        value.tydesc,
                        1,
                        heap_ptr,
                    );
                    return Err(InterpError::RuntimeError(
                        "Failed to clone return value to heap".to_string()
                    ));
                }
                value.ptr = heap_ptr;
                value.location = ValueLocation::TempOwned;
            }
            Ok(value)
        }
        other => other,
    };

    // Clean up frame values before returning.
    cleanup_frame(ctx, frame);

    // Clean up arguments based on their final slot states.
    cleanup_args_after_frame(ctx, arg_values, &final_slot_states, slots, params, ctx.db);

    // Restore previous module.
    ctx.current_module = prev_module;

    // Handle Option/Result return type wrapping.
    use crate::datalit::ast::TypeHint;
    let return_type = func.return_type(ctx.db);
    if let Some(ret_type) = return_type {
        match ret_type.type_hint(ctx.db) {
            TypeHint::Option(_) => {
                // If the function's return type is ?T, wrap result in Some or catch OptionNone as None.
                match result {
                    Ok(value) => {
                        // Check if value is already an Option (e.g., from @none literal).
                        let value_tag = unsafe { (*value.tydesc).type_tag };
                        if value_tag == datalove_rt::rtdt::TyTag::Option {
                            // Already an Option - return directly, don't double-wrap.
                            return Ok(value);
                        }
                        // Wrap non-Option result in Some.
                        return allocate_option_some_from_value(ctx, value);
                    }
                    Err(InterpError::OptionNone) => {
                        // Early return with None - allocate Option::None.
                        let inner_tydesc = value_tydesc_for_option(ctx, ret_type);
                        return allocate_option_none(ctx, inner_tydesc);
                    }
                    Err(e) => return Err(e),
                }
            }
            TypeHint::Result(_) => {
                // If the function's return type is !T, wrap result in Ok or catch ResultErr as Err.
                match result {
                    Ok(value) => {
                        // Check if value is already a Result (e.g., from @error literal).
                        let value_tag = unsafe { (*value.tydesc).type_tag };
                        if value_tag == datalove_rt::rtdt::TyTag::Result {
                            // Already a Result - return directly, don't double-wrap.
                            return Ok(value);
                        }
                        // Wrap non-Result value in Ok.
                        return allocate_result_ok_from_value(ctx, value);
                    }
                    Err(InterpError::ResultErr { tydesc, ptr }) => {
                        // Early return with Err - allocate Result::Err.
                        let ok_tydesc = value_tydesc_for_result(ctx, ret_type);
                        return allocate_result_err(ctx, ok_tydesc, tydesc, ptr);
                    }
                    Err(e) => return Err(e),
                }
            }
            _ => {}
        }
    }

    result
}

/// Get the inner type descriptor for an Option type hint.
fn value_tydesc_for_option<'db>(
    ctx: &mut InterpContext<'db>,
    type_hint: crate::datalit::ast::TypeHintAndHeap<'db>,
) -> *const datalove_rt::rtdt::TyDesc {
    use crate::datalit::ast::TypeHint;

    if let TypeHint::Option(opt) = type_hint.type_hint(ctx.db) {
        let inner = opt.inner_type(ctx.db);
        type_hint_to_tydesc(ctx, inner)
    } else {
        // Fallback - shouldn't happen.
        ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::U32)
    }
}

/// Get the ok type descriptor for a Result type hint.
fn value_tydesc_for_result<'db>(
    ctx: &mut InterpContext<'db>,
    type_hint: crate::datalit::ast::TypeHintAndHeap<'db>,
) -> *const datalove_rt::rtdt::TyDesc {
    use crate::datalit::ast::TypeHint;

    if let TypeHint::Result(res) = type_hint.type_hint(ctx.db) {
        let inner = res.inner_type(ctx.db);
        type_hint_to_tydesc(ctx, inner)
    } else {
        // Fallback - shouldn't happen.
        ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::U32)
    }
}

/// Convert a type hint to a tydesc.
fn type_hint_to_tydesc<'db>(
    ctx: &mut InterpContext<'db>,
    type_hint: crate::datalit::ast::TypeHintAndHeap<'db>,
) -> *const datalove_rt::rtdt::TyDesc {
    use crate::datalit::ast::TypeHint;
    use crate::datalit::tycheck::Type;

    match type_hint.type_hint(ctx.db) {
        TypeHint::Bool => ctx.tydesc_table.get_or_create(&Type::Bool),
        TypeHint::U8 => ctx.tydesc_table.get_or_create(&Type::U8),
        TypeHint::I8 => ctx.tydesc_table.get_or_create(&Type::I8),
        TypeHint::U16 => ctx.tydesc_table.get_or_create(&Type::U16),
        TypeHint::I16 => ctx.tydesc_table.get_or_create(&Type::I16),
        TypeHint::U32 => ctx.tydesc_table.get_or_create(&Type::U32),
        TypeHint::I32 => ctx.tydesc_table.get_or_create(&Type::I32),
        TypeHint::U64 => ctx.tydesc_table.get_or_create(&Type::U64),
        TypeHint::I64 => ctx.tydesc_table.get_or_create(&Type::I64),
        TypeHint::F32 => ctx.tydesc_table.get_or_create(&Type::F32),
        TypeHint::Int => ctx.tydesc_table.get_or_create(&Type::Int),
        TypeHint::String => ctx.tydesc_table.get_or_create(&Type::String),
        TypeHint::Option(opt) => {
            let inner_tydesc = type_hint_to_tydesc(ctx, opt.inner_type(ctx.db));
            ctx.tydesc_table.create_option_from_inner_tydesc(inner_tydesc)
        }
        TypeHint::Result(res) => {
            let inner_tydesc = type_hint_to_tydesc(ctx, res.inner_type(ctx.db));
            ctx.tydesc_table.create_result_from_inner_tydesc(inner_tydesc)
        }
        _ => {
            // Default fallback for complex types.
            ctx.tydesc_table.get_or_create(&Type::U32)
        }
    }
}

/// Execute function body with CFG-based execution.
fn execute_function_body_with_frame<'db>(
    ctx: &mut InterpContext<'db>,
) -> Result<Value, InterpError> {
    // Get the current frame (top of stack).
    let frame_index = ctx.call_stack.len() - 1;

    // Get the function and CFG from the frame.
    let func = ctx.call_stack[frame_index].func;
    let cfg = ctx.call_stack[frame_index].cfg;

    // Start at block 0 (entry block).
    let mut current_block_id = BlockId(0);

    loop {
        let block = cfg.get_block(ctx.db, current_block_id)
            .ok_or_else(|| InterpError::RuntimeError(
                format!("Invalid block ID {:?}", current_block_id)
            ))?;

        // Execute all statements in the current block.
        for stmt_id in &block.statements {
            let stmt = cfg.get_stmt(ctx.db, *stmt_id)
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Invalid stmt ID {:?}", stmt_id)
                ))?;

            // Execute statement.
            match execute_cfg_statement(ctx, stmt)? {
                CfgControl::Continue => continue,
                CfgControl::Return(value) => return Ok(value),
            }
        }

        // Handle terminator.
        match &block.terminator {
            Terminator::Return => {
                // Should have returned via CfgControl::Return above.
                return Err(InterpError::RuntimeError(
                    format!("Function '{}' reached Return terminator without ret statement",
                            func.name(ctx.db).text(ctx.db))
                ));
            }
            Terminator::Branch { condition_stmt, then_block, else_block } => {
                // Get the statement that caused the branch.
                let stmt = cfg.get_stmt(ctx.db, *condition_stmt)
                    .ok_or_else(|| InterpError::RuntimeError(
                        format!("Invalid condition stmt ID {:?}", condition_stmt)
                    ))?;

                match stmt {
                    ast::Statement::If(if_s) => {
                        // If-statement: evaluate condition and branch based on result.
                        let condition_value = eval_expression_frame(ctx, if_s.condition(ctx.db), None)?;

                        // Handle condition based on type (bool, Option, or Result).
                        let is_true = evaluate_branch_condition(
                            ctx,
                            condition_value,
                            if_s.then_binding(ctx.db),
                            if_s.else_binding(ctx.db),
                        )?;

                        current_block_id = if is_true { *then_block } else { *else_block };
                    }
                    ast::Statement::Let(_) => {
                        // Let-statement with try operator: branching decision already made.
                        // If we reached this point, the try succeeded (otherwise an error
                        // would have propagated). Go to then_block (continuation).
                        current_block_id = *then_block;
                    }
                    _ => {
                        return Err(InterpError::RuntimeError(
                            "Branch terminator with unexpected statement type".to_string()
                        ));
                    }
                }
            }
            Terminator::Goto(next_block) => {
                current_block_id = *next_block;
            }
            Terminator::TryReturn => {
                // Early return from ? operator - propagate.
                return Err(InterpError::EarlyReturn);
            }
        }
    }
}

/// Execute a statement within CFG-based execution.
///
/// In CFG mode, if-statements don't execute their bodies here - the CFG
/// terminator handles branching. The condition is evaluated when handling
/// the Branch terminator.
fn execute_cfg_statement<'db>(
    ctx: &mut InterpContext<'db>,
    stmt: &ast::Statement<'db>,
) -> Result<CfgControl, InterpError> {
    match stmt {
        ast::Statement::Let(let_stmt) => {
            // Evaluate expression and store in frame slot.
            execute_let_statement_frame(ctx, *let_stmt)?;
            Ok(CfgControl::Continue)
        }
        ast::Statement::Ret(ret_stmt) => {
            // Evaluate the return expression (no dest - value escapes frame).
            // Note: @none/@error in return position require typed context from function return type.
            let value = eval_return_expression_frame(ctx, ret_stmt.value(ctx.db))?;
            Ok(CfgControl::Return(value))
        }
        ast::Statement::If(_) => {
            // In CFG mode, if-statements are handled by the Branch terminator.
            // We don't execute the body here - just continue.
            // The condition will be evaluated when we reach the Block's terminator.
            Ok(CfgControl::Continue)
        }
        ast::Statement::Fun(_) => {
            Err(InterpError::RuntimeError("Nested functions not supported".to_string()))
        }
        ast::Statement::Require(_) | ast::Statement::Import(_) => {
            // These are handled at the module level.
            Ok(CfgControl::Continue)
        }
        ast::Statement::ParseError(_) => {
            Err(InterpError::RuntimeError("Parse error in function".to_string()))
        }
    }
}

/// Clean up a stack frame by destroying all Available (non-moved) values.
fn cleanup_frame<'db>(
    ctx: &mut InterpContext<'db>,
    frame: StackFrame<'db>,
) {
    let layout = frame.layout;
    let slots = layout.slots(ctx.db);

    for (slot_index, slot_info) in slots.iter().enumerate() {
        // Only destroy if the slot is still Available (not moved).
        if frame.slot_states[slot_index] == SlotState::Available {
            // Skip Reference slots (parameters) - caller owns the data.
            if slot_info.kind(ctx.db) == crate::function_analysis::SlotKind::Reference {
                continue;
            }

            // Destroy Local and Temporary slots.
            let offset = slot_info.offset(ctx.db) as usize;
            let slot_ptr = unsafe { frame.frame_data.as_ptr().add(offset) as *mut u8 };

            // Get the type descriptor for this slot.
            let ty = slot_info.ty(ctx.db);
            let datalit_ty = match ty.ty(ctx.db) {
                crate::tycheck::Type::Datalit(dt) => dt.clone(),
                _ => {
                    // Skip non-datalit types for now.
                    continue;
                }
            };
            let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

            // Destroy the value contents only (not the structure itself).
            // The memory is part of the frame buffer and will be freed with the frame.
            let value = Value {
                ptr: slot_ptr,
                tydesc,
                location: ValueLocation::Borrowed,
            };
            destroy_value_contents_only(ctx, value);
        }
    }
}

/// Execute a statement within a function body using frame-based execution.
fn execute_function_statement_frame<'db>(
    ctx: &mut InterpContext<'db>,
    stmt: &ast::Statement<'db>,
) -> Result<(), InterpError> {
    match stmt {
        ast::Statement::Let(let_stmt) => {
            // Evaluate expression and store in frame slot.
            execute_let_statement_frame(ctx, *let_stmt)
        }
        ast::Statement::Ret(ret_stmt) => {
            // Evaluate the return expression (no dest - value escapes frame).
            let value = eval_expression_frame(ctx, ret_stmt.value(ctx.db), None)?;
            Err(InterpError::FunctionReturn(value))
        }
        ast::Statement::If(_) => {
            Err(InterpError::InvalidExpression("If statements in functions not yet implemented".to_string()))
        }
        ast::Statement::Fun(_) => {
            Err(InterpError::InvalidExpression("Nested functions not yet implemented".to_string()))
        }
        ast::Statement::Require(_) | ast::Statement::Import(_) => {
            // These are handled at the module level.
            Ok(())
        }
        ast::Statement::ParseError(_) => {
            Err(InterpError::InvalidExpression("Parse error in function".to_string()))
        }
    }
}

// ============================================================================
// Frame-based execution helpers
// ============================================================================

/// Find a slot by variable name in the frame layout.
fn find_slot_by_name<'db>(
    db: &'db dyn crate::Db,
    layout: crate::function_analysis::FrameLayout<'db>,
    name: InternedText<'db>,
) -> Option<crate::function_analysis::SlotInfo<'db>> {
    layout.slots(db).iter()
        .find(|s| s.name(db) == Some(name))
        .copied()
}

/// Get a Destination for an expression's temporary slot.
fn get_destination_for_expr<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Destination, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    let slot_info = layout.get_temp_slot_for_expr(ctx.db, expr)
        .ok_or_else(|| InterpError::RuntimeError(
            "No temp slot allocated for expression".to_string()
        ))?;

    let offset = slot_info.offset(ctx.db) as usize;
    let ptr = unsafe {
        ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset)
    };
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => return Err(InterpError::RuntimeError(
            "Non-datalit type in temp slot".to_string()
        )),
    };
    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);
    Ok(Destination { ptr, tydesc })
}

/// Mark a temporary slot as Available after writing a value to it.
fn mark_temp_slot_available<'db>(ctx: &mut InterpContext<'db>, expr: ast::ExprFun<'db>) {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    if let Some(slot_info) = layout.get_temp_slot_for_expr(ctx.db, expr) {
        let slot_id = slot_info.slot_id(ctx.db);
        ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;
    }
}

/// Read a pointer value from a Reference slot (parameter).
fn read_reference_slot<'db>(
    frame: &StackFrame<'db>,
    slot_info: crate::function_analysis::SlotInfo<'db>,
    db: &'db dyn crate::Db,
) -> *mut u8 {
    let offset = slot_info.offset(db) as usize;
    let ptr_bytes = &frame.frame_data[offset..offset + std::mem::size_of::<usize>()];
    let ptr_value = usize::from_ne_bytes(ptr_bytes.try_into().unwrap());
    ptr_value as *mut u8
}

/// Read a value from a Local or Temporary slot (zero-copy move).
///
/// Returns a Value pointing directly into the frame buffer.
/// The slot should be marked as Moved after this call.
fn read_value_from_slot<'db>(
    ctx: &mut InterpContext<'db>,
    frame_index: usize,
    slot_info: crate::function_analysis::SlotInfo<'db>,
) -> Result<Value, InterpError> {
    let offset = slot_info.offset(ctx.db) as usize;
    let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };

    // Get the type descriptor from the slot's type.
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
    };
    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

    // Return a Value pointing directly into the frame (zero-copy).
    // The caller must mark this slot as Moved to prevent double-use.
    Ok(Value {
        ptr: frame_ptr,
        tydesc,
        location: ValueLocation::Borrowed,
    })
}

/// Write a value to a Local or Temporary slot.
fn write_value_to_slot<'db>(
    frame: &mut StackFrame<'db>,
    slot_info: crate::function_analysis::SlotInfo<'db>,
    value: Value,
    db: &'db dyn crate::Db,
) -> Result<(), InterpError> {
    let offset = slot_info.offset(db) as usize;
    let size = unsafe { (*value.tydesc).size as usize };

    unsafe {
        // Copy the actual value bytes into the slot.
        std::ptr::copy_nonoverlapping(
            value.ptr,
            frame.frame_data.as_mut_ptr().add(offset),
            size
        );
    }

    Ok(())
}

/// Execute a let statement in frame-based mode.
fn execute_let_statement_frame<'db>(
    ctx: &mut InterpContext<'db>,
    let_stmt: ast::StmtLet<'db>,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::TyTag;

    // Find destination slot FIRST so we can pass it to expression evaluation.
    let frame_index = ctx.call_stack.len() - 1;
    let name = let_stmt.name(ctx.db);
    let layout = ctx.call_stack[frame_index].layout;
    let slot_info = match find_slot_by_name(ctx.db, layout, name) {
        Some(s) => s,
        None => {
            return Err(InterpError::RuntimeError(
                format!("Let binding '{}' not found in frame", name.text(ctx.db))
            ));
        }
    };

    let slot_id = slot_info.slot_id(ctx.db);

    // Create destination from slot.
    let offset = slot_info.offset(ctx.db) as usize;
    let dest_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset) };
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => {
            return Err(InterpError::RuntimeError(
                format!("Non-datalit type in slot '{}'", name.text(ctx.db))
            ));
        }
    };
    let dest_tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);
    let dest_tag = unsafe { (*dest_tydesc).type_tag };

    // Check if slot is Option/Result and may need coercion.
    let needs_coercion_check = matches!(dest_tag, TyTag::Option | TyTag::Result);

    let value = if needs_coercion_check {
        // Evaluate without destination first to allow coercion.
        let value = eval_expression_frame(ctx, let_stmt.value(ctx.db), None)?;

        // Check if coercion needed (value type doesn't match dest type).
        let value_tag = unsafe { (*value.tydesc).type_tag };
        if value.tydesc != dest_tydesc && value_tag != dest_tag {
            // Need to coerce T → Option<T> or T → Result<T>.
            // coerce_value_to_dest writes directly to dest and cleans up value.
            let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
            coerce_value_to_dest(ctx, value, dest)?;

            // Mark slot as Available and return early - value already written.
            ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;
            return Ok(());
        } else {
            // No coercion needed, write to slot.
            if value.location == ValueLocation::TempOwned {
                if let Err(e) = write_value_to_slot(&mut ctx.call_stack[frame_index], slot_info, value, ctx.db) {
                    destroy_value(ctx, value);
                    return Err(e);
                }
                free_value_structure(ctx, value);
            } else {
                // Value is Borrowed - need to clone to slot.
                let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
                clone_value_to_dest(ctx, value, dest);
            }
            ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;
            return Ok(());
        }
    } else {
        // No coercion possible, use standard DPS.
        let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
        eval_expression_frame(ctx, let_stmt.value(ctx.db), Some(dest))?
    };

    // If DPS was used (Borrowed), the value was written directly to slot.
    // If not (TempOwned), we need to write and free.
    if value.location == ValueLocation::TempOwned {
        // Write value to slot.
        if let Err(e) = write_value_to_slot(&mut ctx.call_stack[frame_index], slot_info, value, ctx.db) {
            destroy_value(ctx, value);
            return Err(e);
        }
        // Free the heap-allocated value structure after copying to frame.
        free_value_structure(ctx, value);
    }

    // Mark slot as Available.
    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;

    Ok(())
}

/// Evaluate an expression in frame-based mode.
///
/// If `dest` is provided, the result is written directly to that location
/// and a Borrowed value is returned. Otherwise, a temp is allocated.
fn eval_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // Find slot by name.
            let layout = ctx.call_stack[frame_index].layout;
            let slot_info = find_slot_by_name(ctx.db, layout, name)
                .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

            let slot_id = slot_info.slot_id(ctx.db);

            // Check slot state.
            if ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] == SlotState::Moved {
                return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
            }

            // Check copyability.
            let ty = slot_info.ty(ctx.db);
            let is_copy = crate::function_analysis::is_copy_type(ctx.db, ty);

            // Read value from slot.
            let kind = slot_info.kind(ctx.db);

            if kind == crate::function_analysis::SlotKind::Reference {
                // Reference slot: read pointer to caller's value.
                let ptr = read_reference_slot(&ctx.call_stack[frame_index], slot_info, ctx.db);

                // Get tydesc from slot's type info (not from the data pointer).
                let datalit_ty = match ty.ty(ctx.db) {
                    crate::tycheck::Type::Datalit(dt) => dt.clone(),
                    _ => return Err(InterpError::RuntimeError("Non-datalit type in reference slot".to_string())),
                };
                let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

                if is_copy {
                    // Copy: clone to dest.
                    let borrowed = Value { ptr, tydesc, location: ValueLocation::Borrowed };
                    let result_dest = match dest {
                        Some(d) => d,
                        None => get_destination_for_expr(ctx, expr)?,
                    };
                    let result = clone_value_to_dest(ctx, borrowed, result_dest);
                    if dest.is_none() {
                        mark_temp_slot_available(ctx, expr);
                    }
                    Ok(result)
                } else {
                    // Move: take ownership. Value becomes TempOwned.
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                    Ok(Value { ptr, tydesc, location: ValueLocation::TempOwned })
                }
            } else {
                // Local/Temporary slot.
                if is_copy {
                    // For Copy types, clone to dest.
                    let offset = slot_info.offset(ctx.db) as usize;
                    let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };

                    let datalit_ty = match ty.ty(ctx.db) {
                        crate::tycheck::Type::Datalit(dt) => dt.clone(),
                        _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
                    };
                    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

                    let source_value = Value { ptr: frame_ptr, tydesc, location: ValueLocation::Borrowed };
                    let result_dest = match dest {
                        Some(d) => d,
                        None => get_destination_for_expr(ctx, expr)?,
                    };
                    let result = clone_value_to_dest(ctx, source_value, result_dest);
                    if dest.is_none() {
                        mark_temp_slot_available(ctx, expr);
                    }
                    Ok(result)
                } else {
                    // For Move types, allocate and copy from frame, then mark as moved.
                    let value = read_value_from_slot(ctx, frame_index, slot_info)?;
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                    Ok(value)
                }
            }
        }

        ast::ExprFunKind::BinOp(binop_expr) => {
            // Get temp slot destinations for subexpressions.
            let lhs_expr = binop_expr.lhs(ctx.db);
            let rhs_expr = binop_expr.rhs(ctx.db);
            let lhs_dest = get_destination_for_expr(ctx, lhs_expr)?;
            let rhs_dest = get_destination_for_expr(ctx, rhs_expr)?;

            // Evaluate lhs with destination.
            let lhs = eval_expression_frame(ctx, lhs_expr, Some(lhs_dest))?;
            if lhs.location == ValueLocation::Borrowed {
                mark_temp_slot_available(ctx, lhs_expr);
            }

            // Evaluate rhs with destination.
            let rhs = match eval_expression_frame(ctx, rhs_expr, Some(rhs_dest)) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };
            if rhs.location == ValueLocation::Borrowed {
                mark_temp_slot_available(ctx, rhs_expr);
            }

            // Use provided dest or own temp slot.
            let result_dest = match dest {
                Some(d) => d,
                None => get_destination_for_expr(ctx, expr)?,
            };
            let result = execute_binop(ctx, binop_expr.op(ctx.db), lhs, rhs, Some(result_dest))?;

            // If result went to our temp slot (not caller's dest), mark Available for cleanup.
            if dest.is_none() && result.location == ValueLocation::Borrowed {
                mark_temp_slot_available(ctx, expr);
            }

            Ok(result)
        }

        ast::ExprFunKind::FunctionCall(call_expr) => {
            // Evaluate function call with arguments in frame context.
            eval_function_call_frame(ctx, call_expr)
        }

        ast::ExprFunKind::UnaryOp(unary_expr) => {
            // Get temp slot for operand.
            let operand_expr = unary_expr.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;

            // Evaluate operand with destination.
            let operand = eval_expression_frame(ctx, operand_expr, Some(operand_dest))?;
            if operand.location == ValueLocation::Borrowed {
                mark_temp_slot_available(ctx, operand_expr);
            }

            // Use provided dest or own temp slot.
            let result_dest = match dest {
                Some(d) => d,
                None => get_destination_for_expr(ctx, expr)?,
            };
            let result = execute_unop(ctx, unary_expr.op(ctx.db), operand, Some(result_dest))?;

            // If result went to our temp slot, mark Available for cleanup.
            if dest.is_none() && result.location == ValueLocation::Borrowed {
                mark_temp_slot_available(ctx, expr);
            }

            Ok(result)
        }

        ast::ExprFunKind::Tuple(tuple_expr) => {
            // Get result destination (from caller or own temp slot).
            let result_dest = match dest {
                Some(d) => d,
                None => get_destination_for_expr(ctx, expr)?,
            };

            let dest_tydesc = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(result_dest.tydesc) };
            let elements = tuple_expr.elements(ctx.db);

            // Evaluate each element directly to its field offset in the tuple.
            for (elem_expr, field) in elements.iter().zip(dest_tydesc.iter_tuple_fields()) {
                let field_ptr = unsafe { result_dest.ptr.add(field.offset() as usize) };
                let field_dest = Destination { ptr: field_ptr, tydesc: field.tydesc().as_ptr() };

                // Evaluate element directly to field destination.
                let elem_value = eval_expression_frame(ctx, *elem_expr, Some(field_dest))?;

                // If element used its own temp slot, it's been written to our field now.
                // The element's temp slot is no longer needed.
                if elem_value.location == ValueLocation::Borrowed {
                    mark_temp_slot_available(ctx, *elem_expr);
                }
            }

            // If using own temp slot, mark Available for cleanup.
            if dest.is_none() {
                mark_temp_slot_available(ctx, expr);
            }

            Ok(Value { ptr: result_dest.ptr, tydesc: result_dest.tydesc, location: ValueLocation::Borrowed })
        }

        ast::ExprFunKind::TryOption(try_op) => {
            // Evaluate operand.
            let operand = eval_expression_frame(ctx, try_op.operand(ctx.db), None)?;
            // Apply try-option operator.
            eval_try_option(ctx, operand)
        }

        ast::ExprFunKind::TryResult(try_op) => {
            // Evaluate operand.
            let operand = eval_expression_frame(ctx, try_op.operand(ctx.db), None)?;
            // Apply try-result operator.
            eval_try_result(ctx, operand)
        }

        // Inline literal variants - delegate to script scope evaluation.
        ast::ExprFunKind::True(_) => allocate_bool(ctx, true),
        ast::ExprFunKind::False(_) => allocate_bool(ctx, false),
        ast::ExprFunKind::None(_) => {
            if let Some(d) = dest {
                write_option_none_to_dest(d)
            } else {
                Err(InterpError::InvalidExpression(
                    "@none literal requires type context".to_string()
                ))
            }
        }
        ast::ExprFunKind::Int(int_expr) => {
            if let Some(d) = dest {
                write_inline_int_to_dest(ctx, &int_expr, d)
            } else {
                allocate_inline_int_literal(ctx, &int_expr)
            }
        }
        ast::ExprFunKind::Float(float_expr) => {
            let value_str = float_expr.value(ctx.db).as_str(ctx.db);
            let value: f32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse float: {}", e)))?;
            allocate_f32(ctx, value)
        }
        ast::ExprFunKind::Hex(hex_expr) => {
            let value_str = hex_expr.value(ctx.db).as_str(ctx.db);
            let hex_digits = value_str.trim_start_matches("0x").trim_start_matches("0X");
            let value: u32 = u32::from_str_radix(hex_digits, 16)
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse hex: {}", e)))?;
            allocate_u32_raw(ctx, value)
        }
        ast::ExprFunKind::String(string_expr) => {
            allocate_inline_string(ctx, &string_expr)
        }
        ast::ExprFunKind::List(list_expr) => {
            eval_inline_list(ctx, EvalContext::Frame, &list_expr, dest)
        }
        ast::ExprFunKind::Set(set_expr) => {
            eval_inline_set(ctx, EvalContext::Frame, &set_expr, dest)
        }
        ast::ExprFunKind::Map(map_expr) => {
            eval_inline_map(ctx, EvalContext::Frame, &map_expr, dest)
        }
        ast::ExprFunKind::Tensor(_) => {
            Err(InterpError::InvalidExpression("Tensor not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonTuple(tuple_expr) => {
            eval_inline_anon_tuple(ctx, EvalContext::Frame, &tuple_expr, dest)
        }
        ast::ExprFunKind::NamedTuple(_) => {
            Err(InterpError::InvalidExpression("Named tuple not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonStruct(struct_expr) => {
            eval_inline_anon_struct(ctx, EvalContext::Frame, &struct_expr, dest)
        }
        ast::ExprFunKind::NamedStruct(_) => {
            Err(InterpError::InvalidExpression("Named struct not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonEnum(_) | ast::ExprFunKind::NamedEnum(_) => {
            Err(InterpError::InvalidExpression("Enum not yet implemented".to_string()))
        }
        ast::ExprFunKind::Data(_) => {
            Err(InterpError::InvalidExpression("Data wrapper not yet implemented".to_string()))
        }
        ast::ExprFunKind::Err(_) => {
            Err(InterpError::InvalidExpression(
                "@error literal requires type context".to_string()
            ))
        }
        ast::ExprFunKind::ParseError(_) => {
            Err(InterpError::InvalidExpression("Parse error in expression".to_string()))
        }
    }
}

/// Evaluate a return expression with the function's return type as context.
///
/// This is needed for @none/@error literals which require a typed destination.
fn eval_return_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    // Check if this is a @none or @error literal that needs typed context.
    let needs_typed_dest = matches!(
        expr.expr(ctx.db),
        ast::ExprFunKind::None(_) | ast::ExprFunKind::Err(_)
    );

    if needs_typed_dest {
        // Get the function's return type to provide as destination.
        let frame_index = ctx.call_stack.len() - 1;
        let func = ctx.call_stack[frame_index].func;
        if let Some(ret_type) = func.return_type(ctx.db) {
            let ret_tydesc = type_hint_to_tydesc(ctx, ret_type);
            let ret_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(
                    ctx.runtime.handle(),
                    ret_tydesc,
                    1
                )
            };
            if ret_ptr.is_null() {
                return Err(InterpError::RuntimeError("Failed to allocate return buffer".to_string()));
            }

            let dest = Destination { ptr: ret_ptr, tydesc: ret_tydesc };
            // Evaluate expression with the typed destination.
            let value = eval_expression_frame(ctx, expr, Some(dest))?;

            // Convert Borrowed to TempOwned since this escapes the frame.
            if value.location == ValueLocation::Borrowed && value.ptr == ret_ptr {
                return Ok(Value { ptr: ret_ptr, tydesc: ret_tydesc, location: ValueLocation::TempOwned });
            } else {
                return Ok(value);
            }
        }
    }

    // For other expressions, evaluate without special destination.
    eval_expression_frame(ctx, expr, None)
}

// ============================================================================
// Unified Expression Evaluation
// ============================================================================

/// Evaluate an expression in the given context.
///
/// This is the unified entry point for expression evaluation that dispatches
/// to context-specific implementations for variable lookup while sharing
/// code for literals and operations.
fn eval_expression<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    expr: ast::ExprFun<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    match eval_ctx {
        EvalContext::ScriptScope => eval_expression_in_script_scope(ctx, expr, dest),
        EvalContext::Frame => eval_expression_frame(ctx, expr, dest),
    }
}

/// Evaluate inline list expression in the given context.
fn eval_inline_list<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    list_expr: &ast::ExprList<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let elements = list_expr.elements(ctx.db);
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        match eval_expression(ctx, eval_ctx, *elem, None) {
            Ok(v) => values.push(v),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    allocate_list_from_values(ctx, values)
}

/// Evaluate inline set expression in the given context.
fn eval_inline_set<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    set_expr: &ast::ExprSet<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let elements = set_expr.elements(ctx.db);
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        match eval_expression(ctx, eval_ctx, *elem, None) {
            Ok(v) => values.push(v),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    allocate_set_from_values(ctx, values)
}

/// Evaluate inline map expression in the given context.
fn eval_inline_map<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    map_expr: &ast::ExprMap<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let entries = map_expr.entries(ctx.db);
    let mut kv_pairs = Vec::with_capacity(entries.len());

    for entry in entries {
        let key = match eval_expression(ctx, eval_ctx, entry.key(ctx.db), None) {
            Ok(v) => v,
            Err(e) => {
                for (k, v) in kv_pairs {
                    destroy_value(ctx, k);
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        };

        let value = match eval_expression(ctx, eval_ctx, entry.value(ctx.db), None) {
            Ok(v) => v,
            Err(e) => {
                destroy_value(ctx, key);
                for (k, v) in kv_pairs {
                    destroy_value(ctx, k);
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        };

        kv_pairs.push((key, value));
    }

    allocate_map_from_values(ctx, kv_pairs)
}

/// Evaluate inline anonymous tuple expression in the given context.
fn eval_inline_anon_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    tuple_expr: &ast::ExprAnonTuple<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let elements = tuple_expr.elements(ctx.db);
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        match eval_expression(ctx, eval_ctx, *elem, None) {
            Ok(v) => values.push(v),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    allocate_tuple_from_values(ctx, values)
}

/// Evaluate inline anonymous struct expression in the given context.
fn eval_inline_anon_struct<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    struct_expr: &ast::ExprAnonStruct<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let expr_fields = struct_expr.fields(ctx.db);
    let mut sorted_fields: Vec<_> = expr_fields.iter()
        .map(|f| (f.name(ctx.db), f.value(ctx.db)))
        .collect();
    sorted_fields.sort_by_key(|(name, _)| name.as_str(ctx.db));

    let mut field_values = Vec::with_capacity(sorted_fields.len());

    for (name, value_expr) in sorted_fields {
        match eval_expression(ctx, eval_ctx, value_expr, None) {
            Ok(v) => field_values.push((name, v)),
            Err(e) => {
                for (_, v) in field_values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    allocate_struct_from_values(ctx, field_values)
}

/// Extract a boolean value from a Bool-typed Value.
///
/// Returns an error if the value is not a Bool type.
/// Destroys the value after extraction.
fn extract_bool<'db>(ctx: &mut InterpContext<'db>, value: Value) -> Result<bool, InterpError> {
    if !is_bool_value(value) {
        let type_tag = unsafe { (*value.tydesc).type_tag };
        destroy_value(ctx, value);
        return Err(InterpError::RuntimeError(
            format!("Expected Bool in condition, got {:?}", type_tag)
        ));
    }

    let result = unsafe { *(value.ptr as *const bool) };
    destroy_value(ctx, value);
    Ok(result)
}

/// Evaluate a branch condition for if-statements.
///
/// Handles three condition types:
/// - Bool: simple true/false
/// - Option: Some is true, None is false; payload bound to then_binding
/// - Result: Ok is true, Err is false; payload bound to then_binding, error to else_binding
///
/// Returns true if the condition is truthy (bool=true, Option=Some, Result=Ok).
fn evaluate_branch_condition<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    then_binding: Option<InternedText<'db>>,
    else_binding: Option<InternedText<'db>>,
) -> Result<bool, InterpError> {
    use datalove_rt::rtdt::{TyTag, OptionTag, ResultTag, TyDescRef};
    use datalove_rt::rtdt::layout::{compute_option_layout, compute_result_layout};

    let type_tag = unsafe { (*value.tydesc).type_tag };

    match type_tag {
        TyTag::Bool => {
            // Simple bool condition.
            let result = unsafe { *(value.ptr as *const bool) };
            destroy_value(ctx, value);
            Ok(result)
        }
        TyTag::Option => {
            // Option condition: Some = true, None = false.
            let tag = unsafe { *(value.ptr as *const u8) };
            let is_some = tag == OptionTag::Some as u8;

            if is_some {
                if let Some(binding_name) = then_binding {
                    // Extract payload and bind to then_binding slot.
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = unsafe { compute_option_layout(tydesc_ref) };
                    let inner_tydesc = tydesc_ref.option_inner_ty();

                    // Get the slot for then_binding.
                    let frame_index = ctx.call_stack.len() - 1;
                    let frame_layout = ctx.call_stack[frame_index].layout;
                    if let Some(slot_info) = find_slot_by_name(ctx.db, frame_layout, binding_name) {
                        let slot_offset = slot_info.offset(ctx.db) as usize;
                        let slot_ptr = unsafe {
                            ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                        };


                        // Copy payload to slot.
                        let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };
                        let inner_size = inner_tydesc.size() as usize;

                        unsafe {
                            std::ptr::copy_nonoverlapping(payload_ptr, slot_ptr, inner_size);
                        }

                        // Mark slot as available.
                        let slot_index = frame_layout.slots(ctx.db)
                            .iter()
                            .position(|s| s.slot_id(ctx.db) == slot_info.slot_id(ctx.db))
                            .unwrap_or(0);
                        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Available;
                    }
                }
            }

            // Destroy the Option container if it was heap-allocated.
            // Frame slot values are stack memory and shouldn't be freed here.
            if value.location == ValueLocation::TempOwned {
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        ctx.runtime.handle(),
                        value.tydesc,
                        1,
                        value.ptr,
                    );
                }
            }

            Ok(is_some)
        }
        TyTag::Result => {
            // Result condition: Ok = true, Err = false.
            let tag = unsafe { *(value.ptr as *const u8) };
            let is_ok = tag == ResultTag::Ok as u8;

            if is_ok {
                if let Some(binding_name) = then_binding {
                    // Extract Ok payload and bind to then_binding slot.
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = unsafe { compute_result_layout(tydesc_ref) };
                    let ok_tydesc = tydesc_ref.result_ok_ty();

                    // Get the slot for then_binding.
                    let frame_index = ctx.call_stack.len() - 1;
                    let frame_layout = ctx.call_stack[frame_index].layout;
                    if let Some(slot_info) = find_slot_by_name(ctx.db, frame_layout, binding_name) {
                        let slot_offset = slot_info.offset(ctx.db) as usize;
                        let slot_ptr = unsafe {
                            ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                        };

                        // Copy Ok payload to slot.
                        let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };
                        let ok_size = ok_tydesc.size() as usize;
                        unsafe {
                            std::ptr::copy_nonoverlapping(payload_ptr, slot_ptr, ok_size);
                        }

                        // Mark slot as available.
                        let slot_index = frame_layout.slots(ctx.db)
                            .iter()
                            .position(|s| s.slot_id(ctx.db) == slot_info.slot_id(ctx.db))
                            .unwrap_or(0);
                        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Available;
                    }
                }
            } else {
                // Err case - bind error to else_binding if present.
                if let Some(binding_name) = else_binding {
                    // Extract Error from Result payload and bind to else_binding slot.
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = unsafe { compute_result_layout(tydesc_ref) };

                    // Get the slot for else_binding.
                    let frame_index = ctx.call_stack.len() - 1;
                    let frame_layout = ctx.call_stack[frame_index].layout;
                    if let Some(slot_info) = find_slot_by_name(ctx.db, frame_layout, binding_name) {
                        let slot_offset = slot_info.offset(ctx.db) as usize;
                        let slot_ptr = unsafe {
                            ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                        };

                        // Copy Error payload (16 bytes: tydesc ptr + value ptr) to slot.
                        let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };
                        let error_size = std::mem::size_of::<datalove_rt::rtdt::Error>();
                        unsafe {
                            std::ptr::copy_nonoverlapping(payload_ptr, slot_ptr, error_size);
                        }

                        // Mark slot as available.
                        let slot_index = frame_layout.slots(ctx.db)
                            .iter()
                            .position(|s| s.slot_id(ctx.db) == slot_info.slot_id(ctx.db))
                            .unwrap_or(0);
                        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Available;
                    }
                }
            }

            // Destroy the Result container.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(
                    ctx.runtime.handle(),
                    value.tydesc,
                    1,
                    value.ptr,
                );
            }

            Ok(is_ok)
        }
        _ => {
            destroy_value(ctx, value);
            Err(InterpError::RuntimeError(
                format!("Expected Bool, Option, or Result in condition, got {:?}", type_tag)
            ))
        }
    }
}

/// Evaluate try-option operator (`val?`).
///
/// If the operand is None, returns `InterpError::OptionNone` for early return.
/// If the operand is Some(value), extracts and returns the inner value.
fn eval_try_option<'db>(
    ctx: &mut InterpContext<'db>,
    operand_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, OptionTag, layout::compute_option_layout};

    let tydesc_ref = unsafe { TyDescRef::from_ptr(operand_value.tydesc) };

    // Verify operand is Option type.
    if tydesc_ref.type_tag() != TyTag::Option {
        destroy_value(ctx, operand_value);
        return Err(InterpError::RuntimeError(
            format!("Try-option operator (?) requires Option type, got {:?}", tydesc_ref.type_tag())
        ));
    }

    // Read tag.
    let tag = unsafe { *(operand_value.ptr as *const u8) };

    if tag == OptionTag::None as u8 {
        // Free the Option container and return early.
        destroy_value(ctx, operand_value);
        return Err(InterpError::OptionNone);
    }

    // Some case: extract the payload.
    let layout = unsafe { compute_option_layout(tydesc_ref) };
    let payload_ptr = unsafe { operand_value.ptr.add(layout.payload_offset as usize) };

    // Get inner type descriptor.
    let inner_tydesc = tydesc_ref.option_inner_ty().as_ptr();
    let inner_size = unsafe { (*inner_tydesc).size as usize };

    // Clone the payload to a new allocation.
    let rt_handle = ctx.runtime.handle();
    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner_tydesc, 1)
    };

    if result_ptr.is_null() {
        destroy_value(ctx, operand_value);
        return Err(InterpError::RuntimeError("Failed to allocate unwrapped value".to_string()));
    }

    // Copy payload to result.
    unsafe {
        std::ptr::copy_nonoverlapping(payload_ptr, result_ptr, inner_size);
    }

    // Free the Option container (payload has been copied).
    if operand_value.location == ValueLocation::TempOwned {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                operand_value.tydesc,
                1,
                operand_value.ptr,
            );
        }
    }

    Ok(Value {
        ptr: result_ptr,
        tydesc: inner_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Evaluate try-result operator (`val!`).
///
/// If the operand is Err, returns `InterpError::ResultErr` for early return.
/// If the operand is Ok(value), extracts and returns the inner value.
fn eval_try_result<'db>(
    ctx: &mut InterpContext<'db>,
    operand_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, ResultTag, Data, layout::compute_result_layout};

    let tydesc_ref = unsafe { TyDescRef::from_ptr(operand_value.tydesc) };

    // Verify operand is Result type.
    if tydesc_ref.type_tag() != TyTag::Result {
        destroy_value(ctx, operand_value);
        return Err(InterpError::RuntimeError(
            format!("Try-result operator (!) requires Result type, got {:?}", tydesc_ref.type_tag())
        ));
    }

    // Read tag.
    let tag = unsafe { *(operand_value.ptr as *const u8) };

    let layout = unsafe { compute_result_layout(tydesc_ref) };
    let payload_ptr = unsafe { operand_value.ptr.add(layout.payload_offset as usize) };

    if tag == ResultTag::Err as u8 {
        // Err case: extract error and return early.
        // Error is a Data struct (tydesc ptr + value ptr).
        let error_data = unsafe { std::ptr::read(payload_ptr as *const Data) };
        let err_tydesc = error_data.tydesc();
        let err_value_ptr = error_data.value_ptr();

        // Clone the error value.
        let err_tydesc_ref = unsafe { TyDescRef::from_ptr(err_tydesc) };
        let err_size = err_tydesc_ref.size() as usize;

        let rt_handle = ctx.runtime.handle();
        let cloned_err_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, err_tydesc, 1)
        };

        if !cloned_err_ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(err_value_ptr, cloned_err_ptr, err_size);
            }
        }

        // Free the original error value allocation (data has been shallow-copied to clone).
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                err_tydesc,
                1,
                err_value_ptr as *mut u8,
            );
        }

        // Free the Result container.
        if operand_value.location == ValueLocation::TempOwned {
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(
                    ctx.runtime.handle(),
                    operand_value.tydesc,
                    1,
                    operand_value.ptr,
                );
            }
        }

        return Err(InterpError::ResultErr {
            tydesc: err_tydesc,
            ptr: cloned_err_ptr,
        });
    }

    // Ok case: extract the payload.
    let ok_tydesc = tydesc_ref.result_ok_ty().as_ptr();
    let ok_size = unsafe { (*ok_tydesc).size as usize };

    // Clone the payload to a new allocation.
    let rt_handle = ctx.runtime.handle();
    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, ok_tydesc, 1)
    };

    if result_ptr.is_null() {
        destroy_value(ctx, operand_value);
        return Err(InterpError::RuntimeError("Failed to allocate unwrapped value".to_string()));
    }

    // Copy payload to result.
    unsafe {
        std::ptr::copy_nonoverlapping(payload_ptr, result_ptr, ok_size);
    }

    // Free the Result container (payload has been copied).
    if operand_value.location == ValueLocation::TempOwned {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                operand_value.tydesc,
                1,
                operand_value.ptr,
            );
        }
    }

    Ok(Value {
        ptr: result_ptr,
        tydesc: ok_tydesc,
        location: ValueLocation::TempOwned,
    })
}

