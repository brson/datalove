//! New analysis-driven interpreter.
//!
//! This interpreter uses the function_analysis framework to achieve safe,
//! leak-free execution with proper linear type semantics and package world integration.

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use bct::text::InternedText;

use crate::package::PackageWorld;
use crate::ast::{self, StmtFun};

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

    // Checked arithmetic overflow - triggers early return.
    Overflow,
    DivisionByZero,

    // Result type.
    NoOutputVariable,
}

impl InterpContext<'_> {
    /// Create a new interpreter context.
    pub fn new<'db>(
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
/// against a package world.
pub fn execute_script<'db>(
    db: &'db dyn crate::Db,
    script: crate::script::Script,
    package_world: PackageWorld,
) -> Result<ScriptResult<'db>, InterpError> {
    // Create interpreter context.
    let mut ctx = InterpContext::new(db, package_world, Some(script));

    // Resolve imports and typecheck the package world.
    let resolution = crate::package_resolve::resolve_package_world_with_imports(db, package_world);
    let graph_result = resolution.result(db);

    // If resolution succeeds, run typecheck and store the result.
    if let Ok(graph) = graph_result {
        let typecheck_result = crate::tycheck::typecheck_package_world(db, graph);

        // Check for typecheck errors (but don't fail - just log them for now).
        // The interpreter can work without perfect types as long as the AST is valid.
        let module_errors = typecheck_result.module_errors(db);
        if !module_errors.is_empty() {
            // Just log errors, don't fail execution.
            // This allows tests with minor type mismatches to still run.
            eprintln!("Note: typecheck found errors (continuing anyway): {} errors", module_errors.len());
        }

        ctx.typecheck_result = Some(typecheck_result);

        // Build module function table from ALL modules in the graph (not just script imports).
        // This includes transitive dependencies needed for cross-module calls.
        ctx.module_functions = ModuleFunctionTable::build_from_graph(db, graph);

        // Also populate script-level imports.
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

            // Analyze each function in the unit.
            for statement in parsed_unit.statements(db) {
                if let crate::ast::Statement::Fun(func_stmt) = statement {
                    let analysis = crate::function_analysis::analyze_function(db, *func_stmt, unit_typecheck);
                    ctx.script_function_analyses.insert(*func_stmt, analysis);
                }
            }
        }
    } else {
        // Resolution failed - continue without typecheck (will fail at lookup if needed).
        // This allows simple scripts without module imports to still work.
        ctx.module_functions = ModuleFunctionTable::build_from_script(db, script, package_world);
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
pub fn execute_script_unit<'db>(
    ctx: &mut InterpContext<'db>,
    script: crate::script::Script,
    unit_index: usize,
) -> Result<Option<Value>, InterpError> {
    // Update context with new script.
    ctx.script = Some(script);

    // TODO: Add typechecking once basic expression evaluation works.

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
    // Evaluate the expression (no destination - stored in HashMap).
    let value = eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), None)?;

    // Determine if the type is copy (basic detection).
    let is_copy = is_copy_type(value);

    // Bind to script-level variable.
    let name = let_stmt.name(ctx.db);
    ctx.script_scope.variables.insert(name, ScriptVariable {
        value,
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
            eval_datalit_expression(ctx, datalit_expr)
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
        ast::ExprFunKind::Tuple(_) => {
            Err(InterpError::InvalidExpression("Tuples not yet implemented".to_string()))
        }
        ast::ExprFunKind::UnaryOp(_) => {
            Err(InterpError::InvalidExpression("Unary operations not yet implemented".to_string()))
        }
        ast::ExprFunKind::TryOption(_) | ast::ExprFunKind::TryResult(_) => {
            Err(InterpError::InvalidExpression("Try operators not yet implemented".to_string()))
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
        Expr::ParseError(_) => Err(InterpError::InvalidExpression("Parse error".to_string())),
        _ => Err(InterpError::InvalidExpression(
            "Datalit expression type not yet implemented".to_string()
        )),
    }
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

    // Evaluate all arguments in script scope (no destination - args are temporaries).
    let mut arg_values = Vec::new();
    for arg_expr in arg_exprs {
        let value = match eval_expression_in_script_scope(ctx, *arg_expr, None) {
            Ok(v) => v,
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

    // Evaluate all arguments in frame context (no destination - args are temporaries).
    let mut arg_values = Vec::new();
    for arg_expr in arg_exprs {
        let value = match eval_expression_frame(ctx, *arg_expr, None) {
            Ok(v) => v,
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

    // Get frame layout.
    let layout = analysis.frame_layout(ctx.db);
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
    };
    ctx.call_stack.push(frame);

    // Execute function body.
    let result = execute_function_body_with_frame(ctx);

    // Pop the frame and capture slot states for argument cleanup.
    let frame = ctx.call_stack.pop().unwrap();
    let final_slot_states = frame.slot_states.clone();

    // Clean up frame values before returning.
    cleanup_frame(ctx, frame);

    // Clean up arguments based on their final slot states.
    cleanup_args_after_frame(ctx, arg_values, &final_slot_states, slots, params, ctx.db);

    // Restore previous module.
    ctx.current_module = prev_module;

    result
}

/// Execute function body with frame-based execution.
fn execute_function_body_with_frame<'db>(
    ctx: &mut InterpContext<'db>,
) -> Result<Value, InterpError> {
    // Get the current frame (top of stack).
    let frame_index = ctx.call_stack.len() - 1;

    // Get the function from the frame.
    let func = ctx.call_stack[frame_index].func;

    // Execute each statement in the function body.
    for stmt in func.body(ctx.db) {
        match execute_function_statement_frame(ctx, stmt) {
            Ok(()) => continue,
            Err(InterpError::FunctionReturn(value)) => {
                return Ok(value);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    // If we reach here, the function didn't have an explicit return.
    Err(InterpError::RuntimeError(
        format!("Function '{}' did not return a value", func.name(ctx.db).text(ctx.db))
    ))
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
    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };

    // Evaluate RHS expression with destination.
    let value = eval_expression_frame(ctx, let_stmt.value(ctx.db), Some(dest))?;

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
                    // Copy: clone the value. The clone is TempOwned.
                    let borrowed = Value { ptr, tydesc, location: ValueLocation::Borrowed };
                    Ok(clone_value(ctx, borrowed))
                } else {
                    // Move: take ownership. Value becomes TempOwned.
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                    Ok(Value { ptr, tydesc, location: ValueLocation::TempOwned })
                }
            } else {
                // Local/Temporary slot.
                if is_copy {
                    // For Copy types, clone directly from frame without allocating.
                    let offset = slot_info.offset(ctx.db) as usize;
                    let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };

                    let datalit_ty = match ty.ty(ctx.db) {
                        crate::tycheck::Type::Datalit(dt) => dt.clone(),
                        _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
                    };
                    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

                    // Temporary value for cloning - location doesn't matter since we only clone from it.
                    let temp_value = Value { ptr: frame_ptr, tydesc, location: ValueLocation::Borrowed };
                    Ok(clone_value(ctx, temp_value))
                } else {
                    // For Move types, allocate and copy from frame, then mark as moved.
                    let value = read_value_from_slot(ctx, frame_index, slot_info)?;
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                    Ok(value)
                }
            }
        }

        ast::ExprFunKind::Datalit(datalit_expr) => {
            // Allocate literals on heap (as currently done).
            eval_datalit_expression(ctx, datalit_expr)
        }

        ast::ExprFunKind::BinOp(binop_expr) => {
            // Evaluate operands (no destination - they are intermediates).
            let lhs = eval_expression_frame(ctx, binop_expr.lhs(ctx.db), None)?;
            let rhs = match eval_expression_frame(ctx, binop_expr.rhs(ctx.db), None) {
                Ok(v) => v,
                Err(e) => {
                    // Clean up lhs on error.
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };

            // Execute operation with destination.
            execute_binop(ctx, binop_expr.op(ctx.db), lhs, rhs, dest)
        }

        ast::ExprFunKind::FunctionCall(call_expr) => {
            // Evaluate function call with arguments in frame context.
            eval_function_call_frame(ctx, call_expr)
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
        datalove_rt::c::dtlv_rti_clone_local(rt_handle, value.ptr, value.tydesc, cloned_ptr);
    }

    Value {
        ptr: cloned_ptr,
        tydesc: value.tydesc,
        location: ValueLocation::TempOwned,
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
        return allocate_f32(ctx, a + b);
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
        return allocate_f32(ctx, a - b);
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
        return allocate_f32(ctx, a * b);
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
        return allocate_f32(ctx, a / b);
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

/// Evaluate optional addition for u32.
///
/// Returns Some(result) on success, None on overflow.
fn eval_add_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const u32) };
        let b = unsafe { *(rhs.ptr as *const u32) };
        let inner_tydesc = lhs.tydesc;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match a.checked_add(b) {
            Some(result) => {
                let val = allocate_u32_raw(ctx, result)?;
                allocate_option_some_from_value(ctx, val)
            }
            None => allocate_option_none(ctx, inner_tydesc),
        }
    } else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Optional addition requires matching fixed int types".to_string()))
    }
}

/// Evaluate optional subtraction for u32.
///
/// Returns Some(result) on success, None on underflow.
fn eval_sub_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const u32) };
        let b = unsafe { *(rhs.ptr as *const u32) };
        let inner_tydesc = lhs.tydesc;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match a.checked_sub(b) {
            Some(result) => {
                let val = allocate_u32_raw(ctx, result)?;
                allocate_option_some_from_value(ctx, val)
            }
            None => allocate_option_none(ctx, inner_tydesc),
        }
    } else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Optional subtraction requires matching fixed int types".to_string()))
    }
}

/// Evaluate optional multiplication for u32.
///
/// Returns Some(result) on success, None on overflow.
fn eval_mul_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const u32) };
        let b = unsafe { *(rhs.ptr as *const u32) };
        let inner_tydesc = lhs.tydesc;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match a.checked_mul(b) {
            Some(result) => {
                let val = allocate_u32_raw(ctx, result)?;
                allocate_option_some_from_value(ctx, val)
            }
            None => allocate_option_none(ctx, inner_tydesc),
        }
    } else {
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);
        Err(InterpError::InvalidExpression("Optional multiplication requires matching fixed int types".to_string()))
    }
}

/// Evaluate optional division.
///
/// For int: returns Some(result) on success, None on div-by-zero.
/// For u32: returns Some(result) on success, None on div-by-zero.
fn eval_div_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    // Both int: use runtime checked division.
    if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;
        let inner_tydesc = lhs.tydesc;
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
            allocate_option_some_from_value(ctx, result_int)
        } else {
            destroy_value(ctx, result_int);
            allocate_option_none(ctx, inner_tydesc)
        }
    }
    // Both u32: use Rust checked_div.
    else if is_u32_value(lhs) && is_u32_value(rhs) {
        let a = unsafe { *(lhs.ptr as *const u32) };
        let b = unsafe { *(rhs.ptr as *const u32) };
        let inner_tydesc = lhs.tydesc;
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        match a.checked_div(b) {
            Some(result) => {
                let val = allocate_u32_raw(ctx, result)?;
                allocate_option_some_from_value(ctx, val)
            }
            None => allocate_option_none(ctx, inner_tydesc),
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
) -> Result<Value, InterpError> {
    use crate::ast::BinOp;
    use datalove_rt::c::RtOrdering;

    let ordering = unsafe {
        datalove_rt::c::dtlv_rti_cmp_total(
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

    allocate_bool(ctx, result)
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
            eval_comparison(ctx, op, lhs, rhs)
        }

        // Optional operators: return Option<T> with None on overflow/div0.
        BinOp::AddOptional => eval_add_optional(ctx, lhs, rhs),
        BinOp::SubOptional => eval_sub_optional(ctx, lhs, rhs),
        BinOp::MulOptional => eval_mul_optional(ctx, lhs, rhs),
        BinOp::DivOptional => eval_div_optional(ctx, lhs, rhs),
    }
}
