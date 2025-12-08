//! New analysis-driven interpreter.
//!
//! This interpreter uses the function_analysis framework to achieve safe,
//! leak-free execution with proper linear type semantics and package world integration.

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use bct::text::InternedText;

use crate::package::PackageWorld;
use crate::ast::{self, StmtFun};
use crate::function_analysis::{ControlFlowGraph, Terminator, BlockId};

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

/// Slot state for frame-based execution.
///
/// Tracks whether a slot is available for use or has been moved.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// Slot is available for reading (either reference or owned value).
    Available,
    /// Slot has been moved from (only applicable to non-copy types).
    Moved,
}

/// Control flow result from executing a CFG statement.
enum CfgControl {
    /// Continue to the next statement in the block.
    Continue,
    /// Return from the function with a value.
    Return(Value),
}

/// Stack frame for function execution.
///
/// Contains the packed frame data buffer and per-slot state tracking.
pub struct StackFrame<'db> {
    /// Packed frame data containing all slot values at computed offsets.
    pub frame_data: Vec<u8>,
    /// Per-slot state tracking for move semantics.
    pub slot_states: Vec<SlotState>,
    /// Function being executed (for debugging).
    pub func: crate::ast::StmtFun<'db>,
    /// Frame layout providing slot offsets and types.
    pub layout: crate::function_analysis::FrameLayout<'db>,
    /// Control flow graph for CFG-based execution.
    pub cfg: ControlFlowGraph<'db>,
}

/// Tracks whether a Value's memory needs freeing after use.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ValueLocation {
    /// Points to frame buffer or caller's data via reference. Don't free.
    Borrowed,
    /// Temp heap allocation for expression evaluation. Free structure after use.
    TempOwned,
}

/// Value representation.
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
    pub location: ValueLocation,
}

/// Destination for DPS (Destination-Passing Style) expression evaluation.
///
/// When provided, expression evaluation writes directly to this location
/// instead of allocating a temporary.
#[derive(Copy, Clone, Debug)]
pub struct Destination {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
}

impl Destination {
    /// Create a Value pointing to this destination (Borrowed, since caller owns memory).
    pub fn to_borrowed_value(self) -> Value {
        Value {
            ptr: self.ptr,
            tydesc: self.tydesc,
            location: ValueLocation::Borrowed,
        }
    }
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

/// Interpreter errors.
#[derive(Debug)]
pub enum InterpError {
    // Analysis-time errors.
    TypeErrors,
    TypecheckErrors(usize),  // Number of typecheck errors found.
    AnalysisErrors(Vec<String>),

    // Runtime errors.
    VariableNotFound(String),
    UseAfterMove(String),
    FunctionNotFound(String),
    ModuleNotFound(String),
    InvalidExpression(String),
    RuntimeError(String),

    // Control flow.
    ReturnOutsideFunction,
    IfOutsideFunction,
    FunctionReturn(Value),  // Used internally to propagate return values.
    EarlyReturn,  // Used for try operator (? or !) early return from CFG.

    // Checked arithmetic overflow - triggers early return with Err.
    Overflow,
    DivisionByZero,

    // Optional arithmetic overflow - triggers early return with None.
    OptionNone,
    // Result error - triggers early return with Err.
    // Carries the error value (tydesc + ptr) to be wrapped in Result::Err.
    ResultErr { tydesc: *const datalove_rt::rtdt::TyDesc, ptr: *mut u8 },

    // Result type.
    NoOutputVariable,
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

/// Parse a module and extract all function definitions.
///
/// This is a tracked function to satisfy Salsa's requirements.
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

    // Check if we need to coerce T → Option<T> or T → Result<T>.
    let final_value = if let Some(type_hint_and_heap) = let_stmt.type_hint(ctx.db) {
        let type_hint = type_hint_and_heap.type_hint(ctx.db);
        match type_hint {
            TypeHint::Option(_) | TypeHint::Result(_) => {
                // Evaluate expression first.
                let value = eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), None)?;

                // Get expected destination type.
                let dest_tydesc = type_hint_to_tydesc(ctx, type_hint_and_heap);
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
        ast::ExprFunKind::Datalit(datalit_expr) => {
            // Evaluate datalit expression (literals, tuples, etc.).
            // If a destination is provided, use type-guided evaluation.
            if let Some(d) = dest {
                write_datalit_to_dest(ctx, datalit_expr, d)
            } else {
                eval_datalit_expression(ctx, datalit_expr)
            }
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

/// Evaluate a datalit expression.
fn eval_datalit_expression<'db>(
    ctx: &mut InterpContext<'db>,
    expr: crate::datalit::ast::ExprFull<'db>,
) -> Result<Value, InterpError> {
    use crate::datalit::ast::{Expr, ExprAndHeap};

    let expr_and_heap: &ExprAndHeap<'db> = expr.expr(ctx.db);
    let expr_inner: Expr<'db> = expr_and_heap.expr(ctx.db).clone();

    match expr_inner {
        Expr::True => allocate_bool(ctx, true),
        Expr::False => allocate_bool(ctx, false),
        Expr::Int(int_expr) => allocate_int_literal(ctx, &int_expr),
        Expr::Float(float_expr) => allocate_float_literal(ctx, &float_expr),
        Expr::String(string_expr) => allocate_string(ctx, &string_expr),
        Expr::AnonTuple(tuple_expr) => {
            // Evaluate each element.
            let elements = tuple_expr.elements(ctx.db);
            let mut values = Vec::with_capacity(elements.len());

            for elem in elements {
                match eval_datalit_expression(ctx, elem) {
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
        Expr::AnonStruct(struct_expr) => {
            // Get and sort fields by name for canonical order.
            let expr_fields = struct_expr.fields(ctx.db);
            let mut sorted_fields: Vec<_> = expr_fields.iter()
                .map(|f| (f.name(ctx.db), f.value(ctx.db)))
                .collect();
            sorted_fields.sort_by_key(|(name, _)| name.as_str(ctx.db));

            // Evaluate each field value in sorted order.
            let mut field_values = Vec::with_capacity(sorted_fields.len());

            for (name, value_expr) in sorted_fields {
                match eval_datalit_expression(ctx, value_expr) {
                    Ok(v) => field_values.push((name, v)),
                    Err(e) => {
                        // Clean up already-evaluated values on error.
                        for (_, v) in field_values {
                            destroy_value(ctx, v);
                        }
                        return Err(e);
                    }
                }
            }

            allocate_struct_from_values(ctx, field_values)
        }
        Expr::List(list_expr) => {
            // Evaluate each element.
            let elements = list_expr.elements(ctx.db);
            let mut values = Vec::with_capacity(elements.len());

            for elem in elements {
                match eval_datalit_expression(ctx, elem) {
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

            allocate_list_from_values(ctx, values)
        }
        Expr::None => {
            // @none without type context - this should be handled by write_datalit_to_dest
            // when we have a destination with type information.
            Err(InterpError::InvalidExpression(
                "@none literal requires type context (use in typed slot or with type annotation)".to_string()
            ))
        }
        Expr::Err(_) => {
            // @error without type context - this should be handled by write_datalit_to_dest
            // when we have a destination with type information.
            Err(InterpError::InvalidExpression(
                "@error literal requires type context (use in typed slot or with type annotation)".to_string()
            ))
        }
        Expr::ParseError(_) => Err(InterpError::InvalidExpression("Parse error".to_string())),
        _ => Err(InterpError::InvalidExpression(
            "Datalit expression type not yet implemented".to_string()
        )),
    }
}

/// Write a datalit expression directly to a destination.
///
/// This writes values directly to the destination pointer without intermediate
/// heap allocation.
fn write_datalit_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    expr: crate::datalit::ast::ExprFull<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    use crate::datalit::ast::{Expr, ExprAndHeap};

    let expr_and_heap: &ExprAndHeap<'db> = expr.expr(ctx.db);
    let expr_inner: Expr<'db> = expr_and_heap.expr(ctx.db).clone();

    match expr_inner {
        Expr::True => {
            unsafe { *(dest.ptr as *mut u8) = 1; }
            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
        }
        Expr::False => {
            unsafe { *(dest.ptr as *mut u8) = 0; }
            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
        }
        Expr::Int(int_expr) => {
            write_typed_int_to_dest(ctx, &int_expr, dest)
        }
        Expr::Float(float_expr) => {
            let value_str = float_expr.value(ctx.db).as_str(ctx.db);
            let value: f32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse float: {}", e)))?;
            unsafe { *(dest.ptr as *mut f32) = value; }
            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
        }
        Expr::String(string_expr) => {
            write_string_to_dest(ctx, &string_expr, dest)
        }
        Expr::AnonTuple(tuple_expr) => {
            write_tuple_to_dest(ctx, &tuple_expr, dest)
        }
        Expr::AnonStruct(struct_expr) => {
            write_struct_to_dest(ctx, &struct_expr, dest)
        }
        Expr::List(list_expr) => {
            write_list_to_dest(ctx, &list_expr, dest)
        }
        Expr::None => {
            write_option_none_to_dest(dest)
        }
        Expr::Err(err_expr) => {
            write_result_err_to_dest(ctx, &err_expr, dest)
        }
        Expr::ParseError(_) => Err(InterpError::InvalidExpression("Parse error".to_string())),
        _ => Err(InterpError::InvalidExpression(
            "Datalit expression type not yet implemented".to_string()
        )),
    }
}

/// Write a typed integer literal to a destination, respecting the destination's type.
fn write_typed_int_to_dest<'db>(
    ctx: &InterpContext<'db>,
    int_expr: &crate::datalit::ast::ExprInt<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let value_str = int_expr.value(ctx.db).as_str(ctx.db);
    let dest_type_tag = unsafe { (*dest.tydesc).type_tag };

    match dest_type_tag {
        TyTag::U8 => {
            let value: u8 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u8: {}", e)))?;
            unsafe { *(dest.ptr as *mut u8) = value; }
        }
        TyTag::I8 => {
            let value: i8 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i8: {}", e)))?;
            unsafe { *(dest.ptr as *mut i8) = value; }
        }
        TyTag::U16 => {
            let value: u16 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u16: {}", e)))?;
            unsafe { *(dest.ptr as *mut u16) = value; }
        }
        TyTag::I16 => {
            let value: i16 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i16: {}", e)))?;
            unsafe { *(dest.ptr as *mut i16) = value; }
        }
        TyTag::U32 => {
            let value: u32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u32: {}", e)))?;
            unsafe { *(dest.ptr as *mut u32) = value; }
        }
        TyTag::I32 => {
            let value: i32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i32: {}", e)))?;
            unsafe { *(dest.ptr as *mut i32) = value; }
        }
        TyTag::U64 => {
            let value: u64 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u64: {}", e)))?;
            unsafe { *(dest.ptr as *mut u64) = value; }
        }
        TyTag::I64 => {
            let value: i64 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i64: {}", e)))?;
            unsafe { *(dest.ptr as *mut i64) = value; }
        }
        _ => {
            return Err(InterpError::RuntimeError(
                format!("Cannot write integer literal to destination type {:?}", dest_type_tag)
            ));
        }
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

/// Write a string literal directly to a destination.
fn write_string_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    string_expr: &crate::datalit::ast::ExprString<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let string_value_raw = string_expr.value(ctx.db).as_str(ctx.db);

    // Strip quotes if present.
    let string_value = if string_value_raw.starts_with('"') && string_value_raw.ends_with('"') {
        &string_value_raw[1..string_value_raw.len()-1]
    } else {
        string_value_raw
    };

    let rt_handle = ctx.runtime.handle();

    // Initialize the string structure at the destination.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            dest.ptr,
            dest.tydesc,
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create string".to_string()));
    }

    // Push the string bytes if non-empty.
    if !string_value.is_empty() {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                dest.ptr,
                dest.tydesc,
                string_value.as_ptr(),
                string_value.len() as u32,
            )
        };

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to push string bytes".to_string()));
        }
    }

    Ok(Value {
        ptr: dest.ptr,
        tydesc: dest.tydesc,
        location: ValueLocation::Borrowed,
    })
}

/// Write a tuple literal directly to a destination.
fn write_tuple_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    tuple_expr: &crate::datalit::ast::ExprAnonTuple<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let dest_tydesc = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(dest.tydesc) };
    let elements = tuple_expr.elements(ctx.db);

    // Write each element directly to its field offset.
    for (elem_expr, field) in elements.iter().zip(dest_tydesc.iter_tuple_fields()) {
        let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
        let field_dest = Destination { ptr: field_ptr, tydesc: field.tydesc().as_ptr() };

        // Recursively write element to field destination.
        write_datalit_to_dest(ctx, *elem_expr, field_dest)?;
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

/// Write a struct literal directly to a destination.
fn write_struct_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    struct_expr: &crate::datalit::ast::ExprAnonStruct<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let dest_tydesc = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(dest.tydesc) };

    // Get and sort fields by name for canonical order.
    let expr_fields = struct_expr.fields(ctx.db);
    let mut sorted_fields: Vec<_> = expr_fields.iter()
        .map(|f| (f.name(ctx.db), f.value(ctx.db)))
        .collect();
    sorted_fields.sort_by_key(|(name, _)| name.as_str(ctx.db));

    // Write each field directly to its offset.
    for ((_, value_expr), field) in sorted_fields.iter().zip(dest_tydesc.iter_struct_fields()) {
        let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
        let field_dest = Destination { ptr: field_ptr, tydesc: field.tydesc().as_ptr() };

        // Recursively write field value to field destination.
        write_datalit_to_dest(ctx, *value_expr, field_dest)?;
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

/// Write a list literal directly to a destination.
fn write_list_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    list_expr: &crate::datalit::ast::ExprList<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let dest_tydesc = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(dest.tydesc) };
    let elements = list_expr.elements(ctx.db);
    let rt_handle = ctx.runtime.handle();

    // Get the element type descriptor.
    let elem_tydesc = dest_tydesc.list_element_ty();
    let elem_size = elem_tydesc.size() as usize;

    // Allocate a temporary buffer for the element data.
    // dtlv_rti_list_create_from_slice_local clones from this buffer.
    let buffer = if !elements.is_empty() {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle,
                elem_size as u32,
                elem_tydesc.align(),
                elements.len() as u32,
            )
        }
    } else {
        std::ptr::null_mut()
    };

    // Write each element to the buffer.
    for (i, elem_expr) in elements.iter().enumerate() {
        let elem_ptr = if buffer.is_null() {
            std::ptr::null_mut()
        } else {
            unsafe { buffer.add(i * elem_size) }
        };
        let elem_dest = Destination { ptr: elem_ptr, tydesc: elem_tydesc.as_ptr() };

        if let Err(e) = write_datalit_to_dest(ctx, *elem_expr, elem_dest) {
            // Clean up already-written elements on error.
            for j in 0..i {
                let cleanup_ptr = unsafe { buffer.add(j * elem_size) };
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        rt_handle,
                        cleanup_ptr,
                        elem_tydesc.as_ptr(),
                    );
                }
            }
            if !buffer.is_null() {
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        rt_handle,
                        elem_tydesc.as_ptr(),
                        elements.len() as u32,
                        buffer,
                    );
                }
            }
            return Err(e);
        }
    }

    // Create the list structure at dest, cloning elements from the buffer.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_from_slice_local(
            rt_handle,
            buffer,
            elements.len() as u32,
            elem_tydesc.as_ptr(),
            dest.ptr,
            dest.tydesc,
        )
    };

    // Clean up the source buffer after cloning (regardless of success).
    for i in 0..elements.len() {
        let cleanup_ptr = unsafe { buffer.add(i * elem_size) };
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt_handle,
                cleanup_ptr,
                elem_tydesc.as_ptr(),
            );
        }
    }
    if !buffer.is_null() {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt_handle,
                elem_tydesc.as_ptr(),
                elements.len() as u32,
                buffer,
            );
        }
    }

    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create list".to_string()));
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

/// Write Option::None to a destination.
fn write_option_none_to_dest(dest: Destination) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, OptionTag};

    let dest_tydesc = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    // Verify the destination type is Option.
    if dest_tydesc.type_tag() != TyTag::Option {
        return Err(InterpError::RuntimeError(
            format!("Cannot write @none to non-Option type: {:?}", dest_tydesc.type_tag())
        ));
    }

    // Write None tag to destination.
    unsafe {
        *(dest.ptr as *mut u8) = OptionTag::None as u8;
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

/// Write Result::Err to a destination.
fn write_result_err_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    err_expr: &crate::datalit::ast::ExprErr<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, ResultTag};

    let dest_tydesc = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    // Verify the destination type is Result.
    if dest_tydesc.type_tag() != TyTag::Result {
        return Err(InterpError::RuntimeError(
            format!("Cannot write @error to non-Result type: {:?}", dest_tydesc.type_tag())
        ));
    }

    // Write Err tag to destination.
    unsafe {
        *(dest.ptr as *mut u8) = ResultTag::Err as u8;
    }

    // Compute payload offset.
    let layout = unsafe { datalove_rt::rtdt::layout::compute_result_layout(dest_tydesc) };
    let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };

    // Evaluate the inner error value.
    let inner_expr = err_expr.value(ctx.db);
    let inner_value = eval_datalit_expression(ctx, inner_expr)?;

    // Write Error struct at payload offset.
    // Error has same layout as Data: (tydesc ptr, value ptr).
    unsafe {
        let error_ptr = payload_ptr as *mut datalove_rt::rtdt::Data;
        std::ptr::write(
            error_ptr,
            datalove_rt::rtdt::Data::from_pointers(inner_value.tydesc, inner_value.ptr)
        );
    }

    // The inner value is now owned by the Error, don't free it separately.

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

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
            // For Option<T>/Result<T> parameters, try to evaluate as inner type T first.
            let inner_tydesc = if param_tag == datalove_rt::rtdt::TyTag::Option {
                let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(param_tydesc) };
                tydesc_ref.option_inner_ty().as_ptr()
            } else {
                let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(param_tydesc) };
                tydesc_ref.result_ok_ty().as_ptr()
            };

            // Allocate buffer for inner type.
            let inner_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(inner_tydesc) };
            let inner_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    ctx.runtime.handle(),
                    inner_ref.size(),
                    inner_ref.align(),
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
                let param_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(param_tydesc) };
                let param_ptr = unsafe {
                    datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                        ctx.runtime.handle(),
                        param_ref.size(),
                        param_ref.align(),
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
                // Get the if-statement and evaluate its condition.
                let if_stmt = cfg.get_stmt(ctx.db, *condition_stmt)
                    .ok_or_else(|| InterpError::RuntimeError(
                        format!("Invalid condition stmt ID {:?}", condition_stmt)
                    ))?;

                let (if_s, condition_value) = match if_stmt {
                    ast::Statement::If(if_s) => {
                        let value = eval_expression_frame(ctx, if_s.condition(ctx.db), None)?;
                        let tag = unsafe { (*value.tydesc).type_tag };
                        (if_s, value)
                    }
                    _ => {
                        return Err(InterpError::RuntimeError(
                            "Branch terminator without if-statement".to_string()
                        ));
                    }
                };

                // Handle condition based on type (bool, Option, or Result).
                let is_true = evaluate_branch_condition(
                    ctx,
                    condition_value,
                    if_s.then_binding(ctx.db),
                    if_s.else_binding(ctx.db),
                )?;

                current_block_id = if is_true { *then_block } else { *else_block };
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

        ast::ExprFunKind::Datalit(datalit_expr) => {
            // Use provided dest or own temp slot.
            let result_dest = match dest {
                Some(d) => d,
                None => get_destination_for_expr(ctx, expr)?,
            };
            let result = write_datalit_to_dest(ctx, datalit_expr, result_dest)?;
            // If using own temp slot, mark Available for cleanup.
            if dest.is_none() {
                mark_temp_slot_available(ctx, expr);
            }
            Ok(result)
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

        _ => Err(InterpError::InvalidExpression(
            "Expression type not yet implemented in frame mode".to_string()
        ))
    }
}

/// Clone a value (for copy types or explicit cloning).
fn clone_value<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) -> Value {
    // Allocate memory for the clone.
    let rt_handle = ctx.runtime.handle();
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(value.tydesc) };

    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    // Clone into the allocated memory.
    unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt_handle,
            value.ptr,
            value.tydesc,
            cloned_ptr,
            value.tydesc,
        );
    }

    Value {
        ptr: cloned_ptr,
        tydesc: value.tydesc,
        location: ValueLocation::TempOwned,
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
    if let ast::ExprFunKind::Datalit(datalit_expr) = expr.expr(ctx.db) {
        let inner = datalit_expr.expr(ctx.db);
        let needs_typed_dest = matches!(
            inner.expr(ctx.db),
            crate::datalit::ast::Expr::None | crate::datalit::ast::Expr::Err(_)
        );

        if needs_typed_dest {
            // Get the function's return type to provide as destination.
            let frame_index = ctx.call_stack.len() - 1;
            let func = ctx.call_stack[frame_index].func;
            if let Some(ret_type) = func.return_type(ctx.db) {
                let ret_tydesc = type_hint_to_tydesc(ctx, ret_type);
                let ret_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(ret_tydesc) };
                let ret_ptr = unsafe {
                    datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                        ctx.runtime.handle(),
                        ret_ref.size(),
                        ret_ref.align(),
                        1
                    )
                };
                if ret_ptr.is_null() {
                    return Err(InterpError::RuntimeError("Failed to allocate return buffer".to_string()));
                }

                let dest = Destination { ptr: ret_ptr, tydesc: ret_tydesc };
                let value = write_datalit_to_dest(ctx, datalit_expr, dest)?;

                // Convert Borrowed to TempOwned since this escapes the frame.
                if value.location == ValueLocation::Borrowed && value.ptr == ret_ptr {
                    return Ok(Value { ptr: ret_ptr, tydesc: ret_tydesc, location: ValueLocation::TempOwned });
                } else {
                    // This shouldn't happen for @none/@error.
                    return Ok(value);
                }
            }
        }
    }

    // For other expressions, evaluate without special destination.
    eval_expression_frame(ctx, expr, None)
}

/// Clone a value into a pre-allocated destination.
fn clone_value_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    dest: Destination,
) -> Value {
    use datalove_rt::rtdt::TyDescRef;

    // If tydescs are identical, use the runtime clone directly.
    if value.tydesc == dest.tydesc {
        let rt_handle = ctx.runtime.handle();
        unsafe {
            datalove_rt::c::dtlv_rti_clone_local(rt_handle, value.ptr, value.tydesc, dest.ptr, dest.tydesc);
        }
    } else {
        // Tydescs differ but might represent the same type. Check if they're compatible
        // (same type_tag and size) and use raw memcpy for simple copy types.
        let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
        let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

        if src_ref.type_tag() == dst_ref.type_tag() && src_ref.size() == dst_ref.size() {
            // Types are structurally compatible, use raw memcpy.
            unsafe {
                std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, src_ref.size() as usize);
            }
        } else {
            // Types are incompatible, this is an error.
            panic!(
                "clone_value_to_dest: incompatible types {:?} (size {}) vs {:?} (size {})",
                src_ref.type_tag(), src_ref.size(),
                dst_ref.type_tag(), dst_ref.size()
            );
        }
    }
    Value {
        ptr: dest.ptr,
        tydesc: dest.tydesc,
        location: ValueLocation::Borrowed,
    }
}

/// Allocate a boolean value.
fn allocate_bool<'db>(
    ctx: &mut InterpContext<'db>,
    value: bool,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::Bool);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    unsafe {
        *ptr = if value { 1 } else { 0 };
    }

    Ok(Value {
        ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate an f32 value.
fn allocate_f32<'db>(
    ctx: &mut InterpContext<'db>,
    value: f32,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::F32);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    unsafe {
        *(ptr as *mut f32) = value;
    }

    Ok(Value {
        ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate an f32 value from a float literal.
fn allocate_float_literal<'db>(
    ctx: &mut InterpContext<'db>,
    float_expr: &crate::datalit::ast::ExprFloat<'db>,
) -> Result<Value, InterpError> {
    let value_str = float_expr.value(ctx.db).as_str(ctx.db);
    let value: f32 = value_str.parse()
        .map_err(|e| InterpError::RuntimeError(format!("Failed to parse float: {}", e)))?;
    allocate_f32(ctx, value)
}

/// Allocate an integer value.
///
/// For now, we only support u32 literals.
fn allocate_int_literal<'db>(
    ctx: &mut InterpContext<'db>,
    int_expr: &crate::datalit::ast::ExprInt<'db>,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    // Parse the integer value.
    let value_str = int_expr.value(ctx.db).as_str(ctx.db);
    let value: u32 = value_str.parse()
        .map_err(|e| InterpError::RuntimeError(format!("Failed to parse integer: {}", e)))?;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U32);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    unsafe {
        *(ptr as *mut u32) = value;
    }

    Ok(Value {
        ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a u32 value from a raw u32.
fn allocate_u32_raw<'db>(
    ctx: &mut InterpContext<'db>,
    value: u32,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U32);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    unsafe {
        *(ptr as *mut u32) = value;
    }

    Ok(Value {
        ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a string value.
fn allocate_string<'db>(
    ctx: &mut InterpContext<'db>,
    string_expr: &crate::datalit::ast::ExprString<'db>,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let string_value_raw = string_expr.value(ctx.db).as_str(ctx.db);

    // Strip quotes if present.
    let string_value = if string_value_raw.starts_with('"') && string_value_raw.ends_with('"') {
        &string_value_raw[1..string_value_raw.len()-1]
    } else {
        string_value_raw
    };

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::String);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();

    // Allocate memory for the string structure.
    let string_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    // Initialize the string structure.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            string_ptr,
            tydesc_ref.as_ptr(),
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create string".to_string()));
    }

    // Push the string bytes if non-empty.
    if !string_value.is_empty() {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                string_ptr,
                tydesc_ref.as_ptr(),
                string_value.as_ptr(),
                string_value.len() as u32,
            )
        };

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to push string bytes".to_string()));
        }
    }

    Ok(Value {
        ptr: string_ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}

/// Check if a value is a u32 type.
fn is_u32_value(value: Value) -> bool {
    unsafe {
        (*value.tydesc).type_tag == datalove_rt::rtdt::TyTag::U32
    }
}

/// Check if a value is an int (bigint) type.
fn is_int_value(value: Value) -> bool {
    unsafe {
        (*value.tydesc).type_tag == datalove_rt::rtdt::TyTag::Int
    }
}

/// Check if a value is an f32 type.
fn is_f32_value(value: Value) -> bool {
    unsafe {
        (*value.tydesc).type_tag == datalove_rt::rtdt::TyTag::F32
    }
}

/// Check if a value is a Bool type.
fn is_bool_value(value: Value) -> bool {
    unsafe {
        (*value.tydesc).type_tag == datalove_rt::rtdt::TyTag::Bool
    }
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
                if let Some(_binding_name) = else_binding {
                    // TODO: Extract Error payload and bind to else_binding slot.
                    // For now, we just skip error binding.
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

/// Check if a value is a copy type.
///
/// For now, we consider u32 and Bool as copy types.
/// Int, String and other types are non-copy (linear).
fn is_copy_type(value: Value) -> bool {
    unsafe {
        use datalove_rt::rtdt::TyTag;
        match (*value.tydesc).type_tag {
            TyTag::U32 | TyTag::Bool => true,
            _ => false,
        }
    }
}

/// Allocate a bigint value and initialize it to zero.
fn allocate_bigint<'db>(
    ctx: &mut InterpContext<'db>,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::Int);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    // Initialize to zero.
    unsafe {
        let int_ptr = ptr as *mut datalove_rt::rtdt::Int;
        (*int_ptr).data = std::ptr::null();
        (*int_ptr).size_and_sign = 0;
        (*int_ptr).capacity = 0;
    }

    Ok(Value {
        ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate an Option<T> with None value.
fn allocate_option_none<'db>(
    ctx: &mut InterpContext<'db>,
    inner_tydesc: *const datalove_rt::rtdt::TyDesc,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    // Create Option tydesc from inner tydesc.
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_tydesc);
    let option_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) };

    // Allocate memory.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            option_tydesc_ref.size(),
            option_tydesc_ref.align(),
            1
        )
    };

    if ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Option".to_string()));
    }

    // Write None tag.
    unsafe {
        *ptr = rtdt::OptionTag::None as u8;
    }

    Ok(Value {
        ptr,
        tydesc: option_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Wrap an existing value in Some, consuming the inner value.
fn allocate_option_some_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    // Create Option tydesc from inner value's tydesc.
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_value.tydesc);
    let option_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) };

    // Compute layout.
    let layout = unsafe { rtdt::layout::compute_option_layout(option_tydesc_ref) };

    // Allocate memory.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            option_tydesc_ref.size(),
            option_tydesc_ref.align(),
            1
        )
    };

    if ptr.is_null() {
        destroy_value(ctx, inner_value);
        return Err(InterpError::RuntimeError("Failed to allocate Option".to_string()));
    }

    unsafe {
        // Write Some tag.
        *ptr = rtdt::OptionTag::Some as u8;

        // Copy inner value to payload offset.
        let payload_ptr = ptr.add(layout.payload_offset as usize);
        let inner_size = (*inner_value.tydesc).size as usize;
        std::ptr::copy_nonoverlapping(inner_value.ptr, payload_ptr, inner_size);
    }

    // Free the inner value's container (but data has been copied to Option).
    if inner_value.location == ValueLocation::TempOwned {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                inner_value.tydesc,
                1,
                inner_value.ptr,
            );
        }
    }

    Ok(Value {
        ptr,
        tydesc: option_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Wrap an existing value in Ok, consuming the inner value.
fn allocate_result_ok_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    // Create Result tydesc from inner value's tydesc.
    let result_tydesc = ctx.tydesc_table.create_result_from_inner_tydesc(inner_value.tydesc);
    let result_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(result_tydesc) };

    // Compute layout.
    let layout = unsafe { rtdt::layout::compute_result_layout(result_tydesc_ref) };

    // Allocate memory.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            result_tydesc_ref.size(),
            result_tydesc_ref.align(),
            1
        )
    };

    if ptr.is_null() {
        destroy_value(ctx, inner_value);
        return Err(InterpError::RuntimeError("Failed to allocate Result".to_string()));
    }

    unsafe {
        // Write Ok tag.
        *ptr = rtdt::ResultTag::Ok as u8;

        // Copy inner value to payload offset.
        let payload_ptr = ptr.add(layout.payload_offset as usize);
        let inner_size = (*inner_value.tydesc).size as usize;
        std::ptr::copy_nonoverlapping(inner_value.ptr, payload_ptr, inner_size);
    }

    // Free the inner value's container (but data has been copied to Result).
    if inner_value.location == ValueLocation::TempOwned {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                inner_value.tydesc,
                1,
                inner_value.ptr,
            );
        }
    }

    Ok(Value {
        ptr,
        tydesc: result_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a Result::Err with an error value.
fn allocate_result_err<'db>(
    ctx: &mut InterpContext<'db>,
    ok_tydesc: *const datalove_rt::rtdt::TyDesc,
    err_tydesc: *const datalove_rt::rtdt::TyDesc,
    err_ptr: *mut u8,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    // Create Result tydesc from ok type.
    let result_tydesc = ctx.tydesc_table.create_result_from_inner_tydesc(ok_tydesc);
    let result_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(result_tydesc) };

    // Compute layout.
    let layout = unsafe { rtdt::layout::compute_result_layout(result_tydesc_ref) };

    // Allocate memory.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            result_tydesc_ref.size(),
            result_tydesc_ref.align(),
            1
        )
    };

    if ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Result".to_string()));
    }

    unsafe {
        // Write Err tag.
        *ptr = rtdt::ResultTag::Err as u8;

        // Write Error at payload offset.
        // Error has same layout as Data: (tydesc ptr, value ptr).
        let payload_ptr = ptr.add(layout.payload_offset as usize);
        let error_ptr = payload_ptr as *mut rtdt::Data;
        std::ptr::write(
            error_ptr,
            rtdt::Data::from_pointers(err_tydesc, err_ptr)
        );
    }

    Ok(Value {
        ptr,
        tydesc: result_tydesc,
        location: ValueLocation::TempOwned,
    })
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
    let inner_tydesc_ref = unsafe { TyDescRef::from_ptr(inner_tydesc) };
    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            inner_tydesc_ref.size(),
            inner_tydesc_ref.align(),
            1
        )
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
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle,
                err_tydesc_ref.size(),
                err_tydesc_ref.align(),
                1
            )
        };

        if !cloned_err_ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(err_value_ptr, cloned_err_ptr, err_size);
            }
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
    let ok_tydesc_ref = unsafe { TyDescRef::from_ptr(ok_tydesc) };
    let result_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            ok_tydesc_ref.size(),
            ok_tydesc_ref.align(),
            1
        )
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

/// Allocate a tuple from a vector of evaluated values.
///
/// Takes ownership of all element values, copying their data into the tuple
/// and freeing their original containers.
fn allocate_tuple_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    values: Vec<Value>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if values.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty tuple".to_string()));
    }

    // Collect element tydescs from the values.
    let element_tydescs: Vec<*const rtdt::TyDesc> = values.iter()
        .map(|v| v.tydesc)
        .collect();

    // Create tuple tydesc.
    let tuple_tydesc = ctx.tydesc_table.get_or_create_tuple(&element_tydescs);
    let tuple_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(tuple_tydesc) };

    // Compute layout to get field offsets.
    let layout = unsafe { rtdt::layout::compute_tuple_layout(tuple_tydesc_ref) };

    // Allocate memory for tuple.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tuple_tydesc_ref.size(),
            tuple_tydesc_ref.align(),
            1
        )
    };

    if ptr.is_null() {
        // Clean up all values on allocation failure.
        for value in values {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to allocate tuple".to_string()));
    }

    // Copy each element to its field offset in the tuple.
    for (i, value) in values.into_iter().enumerate() {
        let field_offset = layout.field_offsets[i] as usize;
        let element_size = unsafe { (*value.tydesc).size as usize };

        unsafe {
            let field_ptr = ptr.add(field_offset);
            std::ptr::copy_nonoverlapping(value.ptr, field_ptr, element_size);
        }

        // Free the element's container (data has been copied to tuple).
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
    }

    Ok(Value {
        ptr,
        tydesc: tuple_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a struct from field names and evaluated values.
///
/// Takes ownership of all field values, copying their data into the struct
/// and freeing their original containers. Fields must be provided in sorted
/// order by name for canonical representation.
fn allocate_struct_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    fields: Vec<(bct::text::InternedText<'db>, Value)>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if fields.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty struct".to_string()));
    }

    // Collect field names and tydescs from the values.
    let field_names_and_tydescs: Vec<(bct::text::InternedText<'db>, *const rtdt::TyDesc)> = fields.iter()
        .map(|(name, value)| (*name, value.tydesc))
        .collect();

    // Create struct tydesc.
    let struct_tydesc = ctx.tydesc_table.get_or_create_struct(&field_names_and_tydescs);
    let struct_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(struct_tydesc) };

    // Compute layout to get field offsets.
    let layout = unsafe { rtdt::layout::compute_struct_layout(struct_tydesc_ref) };

    // Allocate memory for struct.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            struct_tydesc_ref.size(),
            struct_tydesc_ref.align(),
            1
        )
    };

    if ptr.is_null() {
        // Clean up all values on allocation failure.
        for (_, value) in fields {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to allocate struct".to_string()));
    }

    // Copy each field value to its offset in the struct.
    for (i, (_, value)) in fields.into_iter().enumerate() {
        let field_offset = layout.field_offsets[i] as usize;
        let field_size = unsafe { (*value.tydesc).size as usize };

        unsafe {
            let field_ptr = ptr.add(field_offset);
            std::ptr::copy_nonoverlapping(value.ptr, field_ptr, field_size);
        }

        // Free the field's container (data has been copied to struct).
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
    }

    Ok(Value {
        ptr,
        tydesc: struct_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a list from a vector of evaluated values.
///
/// Takes ownership of all element values, copying their data into the list
/// and freeing their original containers. All elements must have the same type.
fn allocate_list_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    values: Vec<Value>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if values.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty list".to_string()));
    }

    // All elements must have same type - use first element's tydesc.
    let element_tydesc = values[0].tydesc;
    let element_size = unsafe { (*element_tydesc).size as usize };

    // Create list tydesc.
    let list_tydesc = ctx.tydesc_table.create_list_from_element_tydesc(element_tydesc);
    let list_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(list_tydesc) };

    // Build contiguous buffer of element data.
    let mut buffer = Vec::with_capacity(values.len() * element_size);
    for value in &values {
        unsafe {
            let slice = std::slice::from_raw_parts(value.ptr, element_size);
            buffer.extend_from_slice(slice);
        }
    }

    // Allocate list value.
    let rt_handle = ctx.runtime.handle();
    let list_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            list_tydesc_ref.size(),
            list_tydesc_ref.align(),
            1
        )
    };

    if list_ptr.is_null() {
        for value in values {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to allocate list".to_string()));
    }

    // Create list from slice.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_from_slice_local(
            rt_handle,
            buffer.as_ptr(),
            values.len() as u32,
            element_tydesc,
            list_ptr,
            list_tydesc,
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        // Free allocated memory and element values.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, list_tydesc, 1, list_ptr);
        }
        for value in values {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to create list".to_string()));
    }

    // Destroy original elements (list cloned them).
    for value in values {
        destroy_value(ctx, value);
    }

    Ok(Value {
        ptr: list_ptr,
        tydesc: list_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Widen a u32 value to an int (bigint) value.
fn widen_u32_to_int<'db>(
    ctx: &mut InterpContext<'db>,
    u32_value: Value,
) -> Result<Value, InterpError> {
    // Read the u32 value.
    let value_u32 = unsafe { *(u32_value.ptr as *const u32) };

    // Allocate the Int structure.
    let int_val = allocate_bigint(ctx)?;
    let int_ptr = int_val.ptr as *mut datalove_rt::rtdt::Int;

    unsafe {
        if value_u32 == 0 {
            // Zero: no limbs needed.
            (*int_ptr).data = std::ptr::null();
            (*int_ptr).size_and_sign = 0;
            (*int_ptr).capacity = 0;
        } else {
            // Non-zero: allocate one limb.
            let rt_handle = ctx.runtime.handle();
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle,
                4,  // size of u32
                4,  // alignment of u32
                1   // count
            ) as *mut u32;

            *limb_ptr = value_u32;

            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = 1;  // 1 limb, positive
            (*int_ptr).capacity = 1;
        }
    }

    Ok(int_val)
}

/// Narrow an Int value to u32.
///
/// This reads the Int value and converts it to a u32. If the Int value is
/// too large to fit in a u32 or is negative, returns an error.
fn narrow_int_to_u32<'db>(
    ctx: &mut InterpContext<'db>,
    int_value: Value,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    // Read the Int structure.
    let int_ptr = int_value.ptr as *const datalove_rt::rtdt::Int;

    let u32_value = unsafe {
        let size_and_sign = (*int_ptr).size_and_sign;
        let size = size_and_sign.unsigned_abs() as usize;
        let is_negative = size_and_sign < 0;

        if is_negative {
            return Err(InterpError::RuntimeError(
                "Cannot narrow negative Int to u32".to_string()
            ));
        }

        if size == 0 {
            // Zero.
            0u32
        } else if size == 1 {
            // Single limb - just read it.
            let limb_ptr = (*int_ptr).data as *const u32;
            *limb_ptr
        } else {
            // Multiple limbs - too large for u32.
            return Err(InterpError::RuntimeError(
                "Int value too large to fit in u32".to_string()
            ));
        }
    };

    // Allocate u32 result.
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U32);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    unsafe {
        *(ptr as *mut u32) = u32_value;
    }

    // Destroy the original Int value.
    destroy_value(ctx, int_value);

    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Destroy a value using the runtime's destroy function.
/// Destroy only the contents of a value without freeing its memory.
///
/// Use this for values stored inline in frame buffers, where the memory
/// is owned by the frame Vec<u8> and should not be freed individually.
pub fn destroy_value_contents_only<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();

        // Destroy contents using runtime's type-specific destroy logic.
        // This handles Int limbs, String buffers, and other complex types.
        // Does NOT free the value structure itself.
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );
    }
}

/// Destroy a value, respecting its location.
///
/// For HeapOwned values: destroys contents AND frees the memory structure.
/// For FrameSlot values: destroys contents only (frame owns the memory).
pub fn destroy_value<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();

        // Destroy contents using runtime's type-specific destroy logic.
        // This handles Int limbs, String buffers, and other complex types.
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );

        // Only free the structure for heap-owned values.
        // Frame slot values are freed when the frame is dropped.
        if value.location == ValueLocation::TempOwned {
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt_handle,
                value.tydesc,
                1,
                value.ptr,
            );
        }
    }
}

/// Free only the value structure without destroying contents.
///
/// Use this when a value's bytes have been copied to a frame slot,
/// and the frame now owns the pointers. This frees the temporary
/// heap-allocated structure but leaves sub-allocations intact.
pub fn free_value_structure<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    // Only free temp allocations. Borrowed values point into frames.
    if value.location != ValueLocation::TempOwned {
        return;
    }

    unsafe {
        let rt_handle = ctx.runtime.handle();

        // Free only the structure memory, not the contents.
        // The frame slot now owns any pointers in the structure.
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt_handle,
            value.tydesc,
            1,
            value.ptr,
        );
    }
}

/// Evaluate addition with automatic widening to int.
fn eval_add<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: add and return f32.
    if is_f32_value(lhs) && is_f32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return write_f32_result(ctx, a + b, dest);
    }

    // Both u32: widen to Int and add.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = match widen_u32_to_int(ctx, rhs) {
            Ok(v) => v,
            Err(e) => {
                // Clean up lhs_int on error.
                destroy_value(ctx, lhs_int);
                destroy_value(ctx, lhs);
                destroy_value(ctx, rhs);
                return Err(e);
            }
        };

        // Destroy the original u32 values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        // Get result buffer - either from dest or allocate.
        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = match allocate_bigint(ctx) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs_int);
                    destroy_value(ctx, rhs_int);
                    return Err(e);
                }
            };
            (result_int.ptr, result_int.tydesc, false)
        };

        // Perform bigint addition.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_ptr,
                result_tydesc,
            )
        };

        // Clean up temporary widened values.
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                // Only free if we allocated it.
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    // Both Int: add directly.
    else if is_int_value(lhs) && is_int_value(rhs) {
        // Get result buffer - either from dest or allocate.
        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = allocate_bigint(ctx)?;
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_ptr,
                result_tydesc,
            )
        };

        // Destroy input values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    // Mixed u32 and Int: widen u32 side.
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);  // Destroy original u32.

        // Get result buffer - either from dest or allocate.
        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = allocate_bigint(ctx)?;
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_ptr,
                result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs);  // Destroy rhs Int.

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);  // Destroy original u32.

        // Get result buffer - either from dest or allocate.
        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = allocate_bigint(ctx)?;
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_ptr,
                result_tydesc,
            )
        };

        destroy_value(ctx, lhs);  // Destroy lhs Int.
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    else {
        // Clean up values before returning error.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression(
            "Unsupported types for addition".to_string()
        ))
    }
}

/// Evaluate subtraction with automatic widening to int.
fn eval_sub<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: subtract and return f32.
    if is_f32_value(lhs) && is_f32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return write_f32_result(ctx, a - b, dest);
    }

    // Helper to get result buffer.
    let get_result = |ctx: &mut InterpContext<'db>| -> Result<(*mut u8, *const datalove_rt::rtdt::TyDesc, bool), InterpError> {
        if let Some(d) = dest {
            Ok((d.ptr, d.tydesc, true))
        } else {
            let v = allocate_bigint(ctx)?;
            Ok((v.ptr, v.tydesc, false))
        }
    };

    // Both u32: widen to Int and subtract.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs_int.ptr, lhs_int.tydesc,
                rhs_int.ptr, rhs_int.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_int_value(rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs_int.ptr, lhs_int.tydesc,
                rhs.ptr, rhs.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs_int.ptr, rhs_int.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Unsupported types for subtraction".to_string()))
    }
}

/// Evaluate multiplication with automatic widening to int.
fn eval_mul<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: multiply and return f32.
    if is_f32_value(lhs) && is_f32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return write_f32_result(ctx, a * b, dest);
    }

    let get_result = |ctx: &mut InterpContext<'db>| -> Result<(*mut u8, *const datalove_rt::rtdt::TyDesc, bool), InterpError> {
        if let Some(d) = dest { Ok((d.ptr, d.tydesc, true)) } else { let v = allocate_bigint(ctx)?; Ok((v.ptr, v.tydesc, false)) }
    };

    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_int_value(rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Unsupported types for multiplication".to_string()))
    }
}

/// Evaluate division with automatic widening to int.
fn eval_div<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: divide and return f32.
    if is_f32_value(lhs) && is_f32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return write_f32_result(ctx, a / b, dest);
    }

    let get_result = |ctx: &mut InterpContext<'db>| -> Result<(*mut u8, *const datalove_rt::rtdt::TyDesc, bool), InterpError> {
        if let Some(d) = dest { Ok((d.ptr, d.tydesc, true)) } else { let v = allocate_bigint(ctx)?; Ok((v.ptr, v.tydesc, false)) }
    };

    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_int_value(lhs) && is_int_value(rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Unsupported types for division".to_string()))
    }
}

/// Evaluate checked addition (u32 only, no widening).
///
/// Returns the u32 result on success, or Overflow error on overflow.
fn eval_add_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(lhs) || !is_u32_value(rhs) {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return Err(InterpError::InvalidExpression("Checked addition only supports u32 operands".to_string()));
    }

    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    match lhs_val.checked_add(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::Overflow),
    }
}

/// Evaluate checked subtraction (u32 only, no widening).
///
/// Returns the u32 result on success, or Overflow error on underflow.
fn eval_sub_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(lhs) || !is_u32_value(rhs) {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return Err(InterpError::InvalidExpression("Checked subtraction only supports u32 operands".to_string()));
    }

    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    match lhs_val.checked_sub(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::Overflow),
    }
}

/// Evaluate checked multiplication (u32 only, no widening).
///
/// Returns the u32 result on success, or Overflow error on overflow.
fn eval_mul_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(lhs) || !is_u32_value(rhs) {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return Err(InterpError::InvalidExpression("Checked multiplication only supports u32 operands".to_string()));
    }

    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    match lhs_val.checked_mul(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::Overflow),
    }
}

/// Evaluate checked division.
///
/// For u32: returns the u32 result on success, or DivisionByZero error.
/// For int: returns the int result on success, or DivisionByZero error.
fn eval_div_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both int: use runtime checked division.
    if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_int.ptr, result_int.tydesc,
            )
        };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            return Ok(result_int);
        } else {
            destroy_value(ctx, result_int);
            return Err(InterpError::DivisionByZero);
        }
    }

    // Both u32: use Rust checked_div.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_val = unsafe { *(lhs.ptr as *const u32) };
        let rhs_val = unsafe { *(rhs.ptr as *const u32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match lhs_val.checked_div(rhs_val) {
            Some(result) => write_u32_result(ctx, result, dest),
            None => Err(InterpError::DivisionByZero),
        }
    } else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Checked division requires matching operand types".to_string()))
    }
}

/// Write u32 result to destination or allocate new value.
fn write_u32_result(ctx: &mut InterpContext<'_>, value: u32, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_u32_raw(ctx, value)
    }
}

/// Write Option<u32> result to destination or allocate new value.
fn write_option_u32_result(ctx: &mut InterpContext<'_>, value: Option<u32>, dest: Option<Destination>) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{OptionTag, TyDescRef};
    use datalove_rt::rtdt::layout::compute_option_layout;

    

    // Get or create the Option<u32> type descriptor.
    let u32_tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::U32);
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(u32_tydesc);
    let option_ref = unsafe { TyDescRef::from_ptr(option_tydesc) };
    let layout = unsafe { compute_option_layout(option_ref) };

    

    // Check if destination is properly sized for Option.
    let dest_size = dest.map(|d| unsafe { (*d.tydesc).size });
    let use_dest = dest.is_some() && dest_size == Some(option_ref.size());

    if let (Some(d), true) = (dest, use_dest) {
        // Write to provided destination (properly sized).
        unsafe {
            match value {
                Some(v) => {
                    *(d.ptr as *mut u8) = OptionTag::Some as u8;
                    let payload_ptr = d.ptr.add(layout.payload_offset as usize);
                    *(payload_ptr as *mut u32) = v;
                }
                None => {
                    *(d.ptr as *mut u8) = OptionTag::None as u8;
                }
            }
        }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        // Allocate new Option<u32>.
        let rt_handle = ctx.runtime.handle();
        let ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle,
                option_ref.size(),
                option_ref.align(),
                1
            )
        };

        if ptr.is_null() {
            return Err(InterpError::RuntimeError("Failed to allocate Option<u32>".to_string()));
        }

        unsafe {
            match value {
                Some(v) => {
                    *(ptr as *mut u8) = OptionTag::Some as u8;
                    let payload_ptr = ptr.add(layout.payload_offset as usize);
                    *(payload_ptr as *mut u32) = v;
                }
                None => {
                    *(ptr as *mut u8) = OptionTag::None as u8;
                }
            }
        }

        Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
    }
}

/// Write f32 result to destination or allocate new value.
fn write_f32_result(ctx: &mut InterpContext<'_>, value: f32, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut f32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_f32(ctx, value)
    }
}

/// Write bool result to destination or allocate new value.
fn write_bool_result(ctx: &mut InterpContext<'_>, value: bool, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u8) = if value { 1 } else { 0 }; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_bool(ctx, value)
    }
}

/// Evaluate optional addition for u32.
///
/// Returns result on success, OptionNone error on overflow.
fn eval_add_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(lhs) || !is_u32_value(rhs) {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return Err(InterpError::InvalidExpression("Optional addition only supports u32 operands".to_string()));
    }

    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    match lhs_val.checked_add(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::OptionNone),
    }
}

/// Evaluate optional subtraction for u32.
///
/// Returns result on success, OptionNone error on underflow.
fn eval_sub_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(lhs) || !is_u32_value(rhs) {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return Err(InterpError::InvalidExpression("Optional subtraction only supports u32 operands".to_string()));
    }

    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    match lhs_val.checked_sub(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::OptionNone),
    }
}

/// Evaluate optional multiplication for u32.
///
/// Returns result on success, OptionNone error on overflow.
fn eval_mul_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(lhs) || !is_u32_value(rhs) {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        return Err(InterpError::InvalidExpression("Optional multiplication only supports u32 operands".to_string()));
    }

    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    match lhs_val.checked_mul(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::OptionNone),
    }
}

/// Evaluate optional division.
///
/// For int: returns result on success, OptionNone error on div-by-zero.
/// For u32: returns result on success, OptionNone error on div-by-zero.
fn eval_div_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both int: use runtime checked division.
    if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_int.ptr, result_int.tydesc,
            )
        };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            return Ok(result_int);
        } else {
            destroy_value(ctx, result_int);
            return Err(InterpError::OptionNone);
        }
    }

    // Both u32: use Rust checked_div.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_val = unsafe { *(lhs.ptr as *const u32) };
        let rhs_val = unsafe { *(rhs.ptr as *const u32) };
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match lhs_val.checked_div(rhs_val) {
            Some(result) => write_u32_result(ctx, result, dest),
            None => Err(InterpError::OptionNone),
        }
    } else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Optional division requires matching operand types".to_string()))
    }
}

/// Evaluate a comparison operation.
fn eval_comparison<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::BinOp,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use crate::ast::BinOp;
    use datalove_rt::c::RtOrdering;

    let ordering = unsafe {
        datalove_rt::c::dtlv_rti_cmp_total_local(
            ctx.runtime.handle(),
            lhs.ptr,
            lhs.tydesc,
            rhs.ptr,
            rhs.tydesc,
        )
    };

    destroy_value(ctx, lhs);
    destroy_value(ctx, rhs);

    let result = match (op, ordering) {
        (BinOp::Lt, RtOrdering::Less) => true,
        (BinOp::Gt, RtOrdering::Greater) => true,
        (BinOp::Le, RtOrdering::Less | RtOrdering::Equal) => true,
        (BinOp::Ge, RtOrdering::Greater | RtOrdering::Equal) => true,
        (BinOp::Eq, RtOrdering::Equal) => true,
        (BinOp::Ne, RtOrdering::Less | RtOrdering::Greater) => true,
        (_, RtOrdering::Error) => {
            return Err(InterpError::RuntimeError("Comparison failed: type mismatch".into()));
        }
        _ => false,
    };

    write_bool_result(ctx, result, dest)
}

/// Execute a binary operation.
fn execute_binop<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::BinOp,
    lhs: Value,
    rhs: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use crate::ast::BinOp;

    match op {
        // Bare operators: widen to Int.
        BinOp::Add => eval_add(ctx, lhs, rhs, dest),
        BinOp::Sub => eval_sub(ctx, lhs, rhs, dest),
        BinOp::Mul => eval_mul(ctx, lhs, rhs, dest),
        BinOp::Div => eval_div(ctx, lhs, rhs, dest),

        // Checked operators: preserve type, early-return on overflow.
        BinOp::AddChecked => eval_add_checked(ctx, lhs, rhs, dest),
        BinOp::SubChecked => eval_sub_checked(ctx, lhs, rhs, dest),
        BinOp::MulChecked => eval_mul_checked(ctx, lhs, rhs, dest),
        BinOp::DivChecked => eval_div_checked(ctx, lhs, rhs, dest),

        // Comparison operators.
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            eval_comparison(ctx, op, lhs, rhs, dest)
        }

        // Optional operators: preserve type, early-return on overflow/div0.
        BinOp::AddOptional => eval_add_optional(ctx, lhs, rhs, dest),
        BinOp::SubOptional => eval_sub_optional(ctx, lhs, rhs, dest),
        BinOp::MulOptional => eval_mul_optional(ctx, lhs, rhs, dest),
        BinOp::DivOptional => eval_div_optional(ctx, lhs, rhs, dest),
    }
}

/// Execute a unary operation.
fn execute_unop<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::UnaryOp,
    operand: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use crate::ast::UnaryOp;

    match op {
        UnaryOp::Neg => eval_neg(ctx, operand, dest),
        UnaryOp::NegOptional => eval_neg_optional(ctx, operand, dest),
        UnaryOp::NegResult => eval_neg_result(ctx, operand, dest),
    }
}

/// Evaluate negation for Int type.
fn eval_neg<'db>(
    ctx: &mut InterpContext<'db>,
    operand: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Only Int (bigint) supports bare negation.
    if !is_int_value(operand) {
        destroy_value(ctx, operand);
        return Err(InterpError::InvalidExpression(
            "Negation only supports Int type".to_string()
        ));
    }

    // Get result buffer - either from dest or allocate.
    let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
        (d.ptr, d.tydesc, true)
    } else {
        let result_int = allocate_bigint(ctx)?;
        (result_int.ptr, result_int.tydesc, false)
    };

    // Call runtime negation.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_neg(
            ctx.runtime.handle(),
            operand.ptr,
            operand.tydesc,
            result_ptr,
            result_tydesc,
        )
    };

    // Clean up operand.
    destroy_value(ctx, operand);

    if status == datalove_rt::c::RtStatus::Ok {
        Ok(Value {
            ptr: result_ptr,
            tydesc: result_tydesc,
            location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
        })
    } else {
        if !is_borrowed {
            let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
            destroy_value(ctx, result_val);
        }
        Err(InterpError::RuntimeError("Int negation failed".to_string()))
    }
}

/// Evaluate optional negation (-?x).
///
/// Performs checked negation on signed integers.
/// Returns the raw negated value on success, or OptionNone error on overflow.
fn eval_neg_optional<'db>(
    ctx: &mut InterpContext<'db>,
    operand: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let type_tag = unsafe { (*operand.tydesc).type_tag };
    let operand_tydesc = operand.tydesc;

    // Read value and perform checked negation based on type.
    let raw_value = unsafe { *(operand.ptr as *const u32) };

    let negated_result: Option<u32> = match type_tag {
        TyTag::I8 => {
            let val = raw_value as i8;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I16 => {
            let val = raw_value as i16;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I32 => {
            let val = raw_value as i32;
            val.checked_neg().map(|r| r as u32)
        }
        _ => {
            destroy_value(ctx, operand);
            return Err(InterpError::InvalidExpression(
                format!("Optional negation not supported for type {:?}", type_tag)
            ));
        }
    };

    // Clean up operand.
    destroy_value(ctx, operand);

    match negated_result {
        Some(result) => write_typed_int_result(ctx, result, operand_tydesc, dest),
        None => Err(InterpError::OptionNone),
    }
}

/// Evaluate result negation (-!x).
///
/// Performs checked negation on fixed-width integers.
/// Returns the raw negated value on success, or ResultErr with "overflow" on overflow.
fn eval_neg_result<'db>(
    ctx: &mut InterpContext<'db>,
    operand: Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let type_tag = unsafe { (*operand.tydesc).type_tag };
    let operand_tydesc = operand.tydesc;

    // Read value and perform checked negation based on type.
    let raw_value = unsafe { *(operand.ptr as *const u32) };

    let negated_result: Option<u32> = match type_tag {
        TyTag::I8 => {
            let val = raw_value as i8;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I16 => {
            let val = raw_value as i16;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I32 => {
            let val = raw_value as i32;
            val.checked_neg().map(|r| r as u32)
        }
        TyTag::U8 => {
            let val = raw_value as u8;
            val.checked_neg().map(|r| r as u32)
        }
        TyTag::U16 => {
            let val = raw_value as u16;
            val.checked_neg().map(|r| r as u32)
        }
        TyTag::U32 => {
            raw_value.checked_neg()
        }
        _ => {
            destroy_value(ctx, operand);
            return Err(InterpError::InvalidExpression(
                format!("Result negation not supported for type {:?}", type_tag)
            ));
        }
    };

    // Clean up operand.
    destroy_value(ctx, operand);

    match negated_result {
        Some(result) => write_typed_int_result(ctx, result, operand_tydesc, dest),
        None => {
            // Allocate an "overflow" error string and return ResultErr.
            let err_string = allocate_error_string(ctx, "overflow")?;
            Err(InterpError::ResultErr {
                tydesc: err_string.tydesc,
                ptr: err_string.ptr,
            })
        }
    }
}

/// Coerce a value to a destination type.
///
/// Handles T → Option<T> (wrap in Some) and T → Result<T> (wrap in Ok).
/// If the types are compatible, copies the value to the destination.
fn coerce_value_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    dest: Destination,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyTag, TyDescRef, OptionTag, ResultTag};
    use datalove_rt::rtdt::layout::{compute_option_layout, compute_result_layout};

    let value_tag = unsafe { (*value.tydesc).type_tag };
    let dest_tag = unsafe { (*dest.tydesc).type_tag };

    // If types match, clone the value to dest (not shallow copy - types may have internal pointers).
    if value.tydesc == dest.tydesc {
        let clone_status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                ctx.runtime.handle(),
                value.ptr,
                value.tydesc,
                dest.ptr,
                dest.tydesc,
            )
        };
        if clone_status != datalove_rt::c::RtStatus::Ok {
            destroy_value(ctx, value);
            return Err(InterpError::RuntimeError("Failed to clone value in coercion".to_string()));
        }
        destroy_value(ctx, value);
        return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
    }

    // Coerce T → Option<T>
    if dest_tag == TyTag::Option {
        let dest_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let inner_tydesc = dest_ref.option_inner_ty();

        // Check if value type matches inner type.
        if value.tydesc == inner_tydesc.as_ptr() {
            // Wrap value in Some.
            let layout = unsafe { compute_option_layout(dest_ref) };

            // Write Some tag.
            unsafe { *(dest.ptr as *mut u8) = OptionTag::Some as u8; }

            // Clone payload (not shallow copy - types may have internal pointers).
            let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
            let clone_status = unsafe {
                datalove_rt::c::dtlv_rti_clone_local(
                    ctx.runtime.handle(),
                    value.ptr,
                    value.tydesc,
                    payload_ptr,
                    inner_tydesc.as_ptr(),
                )
            };
            if clone_status != datalove_rt::c::RtStatus::Ok {
                destroy_value(ctx, value);
                return Err(InterpError::RuntimeError("Failed to clone value in Option coercion".to_string()));
            }

            // Clean up the original value (we cloned it).
            destroy_value(ctx, value);

            return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
        }
    }

    // Coerce T → Result<T>
    if dest_tag == TyTag::Result {
        let dest_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let inner_tydesc = dest_ref.result_ok_ty();

        // Check if value type matches inner type.
        if value.tydesc == inner_tydesc.as_ptr() {
            // Wrap value in Ok.
            let layout = unsafe { compute_result_layout(dest_ref) };

            // Write Ok tag.
            unsafe { *(dest.ptr as *mut u8) = ResultTag::Ok as u8; }

            // Clone payload (not shallow copy - types may have internal pointers).
            let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
            let clone_status = unsafe {
                datalove_rt::c::dtlv_rti_clone_local(
                    ctx.runtime.handle(),
                    value.ptr,
                    value.tydesc,
                    payload_ptr,
                    inner_tydesc.as_ptr(),
                )
            };
            if clone_status != datalove_rt::c::RtStatus::Ok {
                destroy_value(ctx, value);
                return Err(InterpError::RuntimeError("Failed to clone value in Result coercion".to_string()));
            }

            // Clean up the original value (we cloned it).
            destroy_value(ctx, value);

            return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
        }
    }

    // No coercion available - type mismatch.
    destroy_value(ctx, value);
    Err(InterpError::RuntimeError(
        format!("Type mismatch: cannot coerce {:?} to {:?}", value_tag, dest_tag)
    ))
}

/// Write a typed integer result to destination or allocate new value.
///
/// Preserves the original type (i8, i16, i32, u8, u16, u32) from the tydesc.
fn write_typed_int_result(
    ctx: &mut InterpContext<'_>,
    value: u32,
    tydesc: *const datalove_rt::rtdt::TyDesc,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        // Write to destination.
        unsafe { *(d.ptr as *mut u32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        // Allocate new value with the correct type.
        let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc) };
        let ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                ctx.runtime.handle(),
                tydesc_ref.size(),
                tydesc_ref.align(),
                1
            )
        };

        if ptr.is_null() {
            return Err(InterpError::RuntimeError("Failed to allocate integer".to_string()));
        }

        unsafe { *(ptr as *mut u32) = value; }
        Ok(Value { ptr, tydesc, location: ValueLocation::TempOwned })
    }
}

/// Allocate a string value with the given content for use as an error.
fn allocate_error_string<'db>(
    ctx: &mut InterpContext<'db>,
    content: &str,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::String);
    let tydesc_ref = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(tydesc_ptr) };

    // Allocate memory for string.
    let rt_handle = ctx.runtime.handle();
    let string_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
            rt_handle,
            tydesc_ref.size(),
            tydesc_ref.align(),
            1
        )
    };

    if string_ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate error string".to_string()));
    }

    // Initialize string structure.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            string_ptr,
            tydesc_ref.as_ptr(),
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create error string".to_string()));
    }

    // Push the string bytes.
    if !content.is_empty() {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                string_ptr,
                tydesc_ref.as_ptr(),
                content.as_ptr(),
                content.len() as u32,
            )
        };

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to push error string bytes".to_string()));
        }
    }

    Ok(Value {
        ptr: string_ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}
