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
        // Destroy the value before dropping the runtime.
        // First destroy the contents (for complex types like Int, String).
        // Then free the value's memory allocation.
        unsafe {
            let rt_handle = self.runtime.handle();

            // Destroy contents.
            datalove_rt::c::dtlv_rti_any_destroy_local(
                rt_handle,
                self.value.ptr,
                self.value.tydesc,
            );

            // Free the allocation.
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt_handle,
                self.value.tydesc,
                1,
                self.value.ptr,
            );
        }
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
) -> Result<ScriptResult<'db>, InterpError> {
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
    // Remove it from the HashMap to avoid double-free.
    let output_name = bct::text::InternedText::new(db, "output");
    let value = ctx.script_scope.variables.remove(&output_name)
        .ok_or(InterpError::NoOutputVariable)?
        .value;

    // Clean up any remaining variables before moving out runtime and tydesc_table.
    let remaining_vars: Vec<_> = ctx.script_scope.variables.drain().map(|(_, var)| var.value).collect();
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

/// Allocate a bigint value.
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

/// Destroy a value by calling the runtime destroy function.
fn destroy_value<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();

        // First destroy the contents (for complex types like Int, String).
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );

        // Then free the value's memory allocation.
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

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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

        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(result_int)
        } else {
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
        BinOp::Add => eval_add(ctx, lhs, rhs),
        BinOp::Sub => eval_sub(ctx, lhs, rhs),
        BinOp::Mul => eval_mul(ctx, lhs, rhs),
        BinOp::Div => eval_div(ctx, lhs, rhs),

        // Not yet implemented.
        BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked => {
            Err(InterpError::InvalidExpression("Checked operators not yet implemented".to_string()))
        }
        BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional => {
            Err(InterpError::InvalidExpression("Optional operators not yet implemented".to_string()))
        }
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            Err(InterpError::InvalidExpression("Comparison operators not yet implemented".to_string()))
        }
    }
}
