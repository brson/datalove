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
    tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
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

/// Value representation.
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
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
            tydesc_table: datalove_datalit::tydesc_table::TyDescTable::new(db),
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
    let parsed = parse_result.script;

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

    // Build module function table from require/import statements.
    ctx.module_functions = ModuleFunctionTable::build_from_script(db, script, package_world);

    // TODO: For now, skip package world integration and typechecking.
    // We'll add this back once we have basic expression evaluation working.

    // Execute all script units.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        execute_unit(&mut ctx, script, unit_index)?;
    }

    // Return the output variable if present.
    // Remove it from the HashMap to avoid double-free.
    let output_name = bct::text::InternedText::new(db, "output");
    let value = ctx.script_scope.variables.remove(&output_name)
        .ok_or(InterpError::NoOutputVariable)?
        .value;

    // Clean up any remaining variables before moving out runtime and tydesc_table.
    // Only destroy Available variables - Moved variables have already been consumed.
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
        destroy_value(&mut ctx, value);
    }

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
    // Evaluate the expression.
    let value = eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db))?;

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
            // Evaluate binary operations.
            let lhs = eval_expression_in_script_scope(ctx, binop_expr.lhs(ctx.db))?;
            let rhs = eval_expression_in_script_scope(ctx, binop_expr.rhs(ctx.db))?;
            execute_binop(ctx, binop_expr.op(ctx.db), lhs, rhs)
        }
        ast::ExprFunKind::Tuple(_) => {
            // TODO: Implement tuple construction.
            Err(InterpError::InvalidExpression("Tuples not yet implemented".to_string()))
        }
        ast::ExprFunKind::UnaryOp(_) => {
            // TODO: Implement unary operations.
            Err(InterpError::InvalidExpression("Unary operations not yet implemented".to_string()))
        }
        ast::ExprFunKind::TryOption(_) | ast::ExprFunKind::TryResult(_) => {
            // TODO: Implement try operators.
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
        Expr::Int(int_expr) => allocate_int(ctx, &int_expr),
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
    }

    // Finally check imported module functions.
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

    // Evaluate all arguments in script scope.
    let mut arg_values = Vec::new();
    for arg_expr in arg_exprs {
        let value = eval_expression_in_script_scope(ctx, *arg_expr)?;
        arg_values.push(value);
    }

    // Execute the function body with arguments.
    // Set current_module if this is a module function.
    execute_function_body(ctx, func, func_module, arg_values)
}

/// Evaluate a function call from function scope.
fn eval_function_call_in_function_scope<'db>(
    ctx: &mut InterpContext<'db>,
    local_variables: &mut HashMap<InternedText<'db>, ScriptVariable>,
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

    // Evaluate all arguments in function scope (can access local variables).
    let mut arg_values = Vec::new();
    for arg_expr in arg_exprs {
        let value = eval_expression_in_function_scope(ctx, local_variables, *arg_expr)?;
        arg_values.push(value);
    }

    // Execute the function body with arguments.
    // Set current_module if this is a module function.
    execute_function_body(ctx, func, func_module, arg_values)
}

/// Execute a function body and return its result.
///
/// This is a simplified implementation that doesn't use frames or analysis.
/// It will be replaced with proper frame-based execution in Phase 6.
fn execute_function_body<'db>(
    ctx: &mut InterpContext<'db>,
    func: ast::StmtFun<'db>,
    func_module: Option<crate::package::PackageModule>,
    arg_values: Vec<Value>,
) -> Result<Value, InterpError> {
    // Save previous module context and set current module.
    let prev_module = ctx.current_module;
    ctx.current_module = func_module;

    // Create a local variable scope for the function.
    let mut local_variables: HashMap<InternedText<'db>, ScriptVariable> = HashMap::new();

    // Initialize parameters from arguments.
    let params = func.params(ctx.db);
    for (i, param) in params.iter().enumerate() {
        // For now, only support In mode parameters.
        if param.mode(ctx.db) != ast::ParamMode::In {
            return Err(InterpError::InvalidExpression(
                format!("Parameter mode {:?} not yet supported (function '{}')",
                    param.mode(ctx.db), func.name(ctx.db).text(ctx.db))
            ));
        }

        let param_name = param.name(ctx.db);
        let param_value = arg_values[i];
        let is_copy = is_copy_type(param_value);

        local_variables.insert(param_name, ScriptVariable {
            value: param_value,
            state: ScriptVarState::Available,
            is_copy,
        });
    }

    // Execute each statement in the function body.
    for stmt in func.body(ctx.db) {
        match execute_function_statement(ctx, &mut local_variables, stmt) {
            Ok(()) => continue,
            Err(InterpError::FunctionReturn(value)) => {
                // Return statement encountered - clean up locals and return.
                cleanup_local_variables(ctx, local_variables);
                ctx.current_module = prev_module;
                return Ok(value);
            }
            Err(e) => {
                // Error occurred - clean up locals before propagating.
                cleanup_local_variables(ctx, local_variables);
                ctx.current_module = prev_module;
                return Err(e);
            }
        }
    }

    // If we reach here, the function didn't have an explicit return.
    cleanup_local_variables(ctx, local_variables);
    ctx.current_module = prev_module;
    Err(InterpError::RuntimeError(
        format!("Function '{}' did not return a value", func.name(ctx.db).text(ctx.db))
    ))
}

/// Execute a statement within a function body.
fn execute_function_statement<'db>(
    ctx: &mut InterpContext<'db>,
    local_variables: &mut HashMap<InternedText<'db>, ScriptVariable>,
    stmt: &ast::Statement<'db>,
) -> Result<(), InterpError> {
    match stmt {
        ast::Statement::Let(let_stmt) => {
            // Evaluate expression in function scope.
            let value = eval_expression_in_function_scope(ctx, local_variables, let_stmt.value(ctx.db))?;

            // Bind to local variable.
            let is_copy = is_copy_type(value);
            let name = let_stmt.name(ctx.db);
            local_variables.insert(name, ScriptVariable {
                value,
                state: ScriptVarState::Available,
                is_copy,
            });
            Ok(())
        }
        ast::Statement::Ret(ret_stmt) => {
            // Evaluate the return expression and signal return.
            let value = eval_expression_in_function_scope(ctx, local_variables, ret_stmt.value(ctx.db))?;
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

/// Evaluate an expression in function scope (can access local variables).
fn eval_expression_in_function_scope<'db>(
    ctx: &mut InterpContext<'db>,
    local_variables: &mut HashMap<InternedText<'db>, ScriptVariable>,
    expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // First check local variables, then script variables.
            if let Some(var) = local_variables.get(&name) {
                if var.state == ScriptVarState::Moved {
                    return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
                }
                let value = var.value;
                let is_copy = var.is_copy;

                if is_copy {
                    Ok(clone_value(ctx, value))
                } else {
                    local_variables.get_mut(&name).unwrap().state = ScriptVarState::Moved;
                    Ok(value)
                }
            } else {
                // Fall back to script scope.
                read_script_variable(ctx, name)
            }
        }
        ast::ExprFunKind::Datalit(datalit_expr) => {
            eval_datalit_expression(ctx, datalit_expr)
        }
        ast::ExprFunKind::FunctionCall(call_expr) => {
            eval_function_call_in_function_scope(ctx, local_variables, call_expr)
        }
        ast::ExprFunKind::BinOp(binop_expr) => {
            let lhs = eval_expression_in_function_scope(ctx, local_variables, binop_expr.lhs(ctx.db))?;
            let rhs = eval_expression_in_function_scope(ctx, local_variables, binop_expr.rhs(ctx.db))?;
            execute_binop(ctx, binop_expr.op(ctx.db), lhs, rhs)
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

/// Clean up local variables by destroying their values.
fn cleanup_local_variables<'db>(
    ctx: &mut InterpContext<'db>,
    local_variables: HashMap<InternedText<'db>, ScriptVariable>,
) {
    for (_, var) in local_variables {
        if var.state == ScriptVarState::Available {
            destroy_value(ctx, var.value);
        }
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
    })
}

/// Allocate an integer value.
///
/// For now, we only support u32 literals.
fn allocate_int<'db>(
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

/// Destroy a value using the runtime's destroy function.
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

        // Free the value structure itself.
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
) -> Result<Value, InterpError> {
    // Both u32: widen to Int and add.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;

        // Destroy the original u32 values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        // Allocate result Int.
        let result_int = allocate_bigint(ctx)?;

        // Perform bigint addition.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        // Clean up temporary widened values.
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    // Both Int: add directly.
    else if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        // Destroy input values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    // Mixed u32 and Int: widen u32 side.
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);  // Destroy original u32.
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs);  // Destroy rhs Int.

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);  // Destroy original u32.

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs);  // Destroy lhs Int.
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    else {
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
) -> Result<Value, InterpError> {
    // Both u32: widen to Int and subtract.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;

        // Destroy the original u32 values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    // Both Int: subtract directly.
    else if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        // Destroy input values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    // Mixed cases.
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);  // Destroy original u32.
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);  // Destroy original u32.

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression(
            "Unsupported types for subtraction".to_string()
        ))
    }
}

/// Evaluate multiplication with automatic widening to int.
fn eval_mul<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    // Both u32: widen to Int and multiply.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;

        // Destroy the original u32 values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_mul(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    // Both Int: multiply directly.
    else if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_mul(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        // Destroy input values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    // Mixed cases.
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);  // Destroy original u32.

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_mul(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs);  // Destroy rhs Int.

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);  // Destroy original u32.

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_mul(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs);  // Destroy lhs Int.
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression(
            "Unsupported types for multiplication".to_string()
        ))
    }
}

/// Evaluate division with automatic widening to int.
fn eval_div<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    // Both u32: widen to Int and divide.
    if is_u32_value(lhs) && is_u32_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        let rhs_int = widen_u32_to_int(ctx, rhs)?;

        // Destroy the original u32 values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    // Both Int: divide directly.
    else if is_int_value(lhs) && is_int_value(rhs) {
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        // Destroy input values.
        destroy_value(ctx, lhs);
        destroy_value(ctx, rhs);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    // Mixed cases.
    else if is_u32_value(lhs) && is_int_value(rhs) {
        let lhs_int = widen_u32_to_int(ctx, lhs)?;
        destroy_value(ctx, lhs);  // Destroy original u32.
        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs_int.ptr,
                lhs_int.tydesc,
                rhs.ptr,
                rhs.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_int_value(lhs) && is_u32_value(rhs) {
        let rhs_int = widen_u32_to_int(ctx, rhs)?;
        destroy_value(ctx, rhs);  // Destroy original u32.

        let result_int = allocate_bigint(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr,
                lhs.tydesc,
                rhs_int.ptr,
                rhs_int.tydesc,
                result_int.ptr,
                result_int.tydesc,
            )
        };

        destroy_value(ctx, lhs);  // Destroy lhs Int.
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
            destroy_value(ctx, result_int);
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression(
            "Unsupported types for division".to_string()
        ))
    }
}

/// Execute a binary operation.
fn execute_binop<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::BinOp,
    lhs: Value,
    rhs: Value,
) -> Result<Value, InterpError> {
    use crate::ast::BinOp;

    match op {
        BinOp::Add | BinOp::AddChecked => eval_add(ctx, lhs, rhs),
        BinOp::Sub | BinOp::SubChecked => eval_sub(ctx, lhs, rhs),
        BinOp::Mul | BinOp::MulChecked => eval_mul(ctx, lhs, rhs),
        BinOp::Div | BinOp::DivChecked => eval_div(ctx, lhs, rhs),

        // Not yet implemented - clean up values before returning error.
        BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional => {
            destroy_value(ctx, lhs);
            destroy_value(ctx, rhs);
            Err(InterpError::InvalidExpression("Optional operators not yet implemented".to_string()))
        }
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            destroy_value(ctx, lhs);
            destroy_value(ctx, rhs);
            Err(InterpError::InvalidExpression("Comparison operators not yet implemented".to_string()))
        }
    }
}
