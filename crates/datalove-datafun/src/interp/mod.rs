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
    script_scope: ScriptScope<'db>,
    tydesc_table: datalove_datalit::tydesc_table::TyDescTable<'db>,
}

/// Script-level scope for REPL incremental execution.
pub struct ScriptScope<'db> {
    /// Script-level let bindings with move tracking.
    variables: HashMap<InternedText<'db>, ScriptVariable>,
    /// Script-level functions.
    functions: HashMap<InternedText<'db>, StmtFun<'db>>,
}

/// Script-level variable with move tracking for linear semantics.
pub struct ScriptVariable {
    value: Value,
    state: ScriptVarState,
    is_copy: bool,  // Cached from type analysis.
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
/// The runtime must be kept alive for the value pointer to remain valid.
pub struct ScriptResult {
    pub value: Value,
    pub runtime: datalove_rt::rust::Runtime,
}

impl std::fmt::Debug for ScriptResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptResult")
            .field("value", &self.value)
            .field("runtime", &"<Runtime>")
            .finish()
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
            tydesc_table: datalove_datalit::tydesc_table::TyDescTable::new(db),
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

/// Execute a complete script in batch mode.
///
/// This is the top-level entry point for running a complete script file
/// against a package world.
pub fn execute_script<'db>(
    db: &'db dyn crate::Db,
    script: crate::script::Script,
    package_world: PackageWorld,
) -> Result<ScriptResult, InterpError> {
    // Create interpreter context.
    let mut ctx = InterpContext::new(db, package_world, Some(script));

    // TODO: For now, skip package world integration and typechecking.
    // We'll add this back once we have basic expression evaluation working.

    // Execute all script units.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        execute_unit(&mut ctx, script, unit_index)?;
    }

    // Return the output variable if present.
    let output_name = bct::text::InternedText::new(db, "output");
    let value = ctx.script_scope.variables.get(&output_name)
        .ok_or(InterpError::NoOutputVariable)?
        .value;

    // Return both the value and the runtime (which keeps the memory alive).
    Ok(ScriptResult {
        value,
        runtime: ctx.runtime,
    })
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

    // Determine if the type is copy (for now, assume non-copy).
    // TODO: Use actual type analysis to determine if copy.
    let is_copy = false;

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
        ast::ExprFunKind::FunctionCall(_) => {
            // TODO: Implement function calls.
            Err(InterpError::InvalidExpression("Function calls not yet implemented".to_string()))
        }
        ast::ExprFunKind::BinOp(_) => {
            // TODO: Implement binary operations.
            Err(InterpError::InvalidExpression("Binary operations not yet implemented".to_string()))
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
/// For now, we default to u32 since we don't have type information yet.
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
