//! Script-level execution for the interpreter.
//!
//! This module handles top-level script execution including:
//! - Entry points for batch and REPL mode
//! - Script-scope statement execution (let, fun)
//! - Script-scope expression evaluation with linear type semantics
//!
//! Script scope stores variables by name in a hashmap with move-state tracking.
//! Linear values are moved on use; copy values are cloned.

use bct::text::InternedText;

use crate::ast::{self, StmtFun};

use super::value::{Value, Destination, ValueOwnership, EvalContext};
use super::error::InterpError;
use super::memory::{destroy_value, clone_value};
use super::types::is_copy_type;
use super::alloc::{
    allocate_bool, allocate_f32, allocate_u32_raw,
};
use super::collections::allocate_tuple_from_values;
use super::literals::{
    allocate_inline_int_literal, write_inline_int_to_dest, write_option_none_to_dest,
    allocate_inline_string, write_bool_to_dest, write_f32_to_dest, write_u32_to_dest,
    write_string_to_dest,
};
use super::arith_widening::{execute_binop, execute_unop};
use super::coerce::coerce_value_to_dest;
use super::tydesc::type_hint_to_tydesc;
use super::context::{
    InterpContext, ScriptVariable, ScriptVarState, ScriptResult,
    cleanup_script_scope,
};
use super::{
    lookup_function, execute_function_body,
    eval_inline_list, eval_inline_set, eval_inline_map,
    eval_inline_anon_tuple, eval_inline_anon_struct,
};
use super::control::{eval_try_option, eval_try_result};
use super::{literals, alloc};

// ============================================================================
// Script Execution Entry Points
// ============================================================================

/// Execute a complete script in batch mode.
///
/// This is the top-level entry point for running a complete script file.
/// The caller must have already typechecked the module graph and script;
/// the interpreter will refuse to run if there are any typecheck errors.
pub fn execute_script_with_module_graph<'db>(
    db: &'db dyn crate::Db,
    script: crate::script::Script,
    graph_typecheck: crate::module_graph::ModuleGraphTypecheckResult<'db>,
) -> Result<ScriptResult<'db>, InterpError> {
    // Create interpreter context using ModuleGraph.
    let mut ctx = InterpContext::new_with_module_graph(db, graph_typecheck)?;
    ctx.script = Some(script);

    // Populate script-level imports using ModuleGraph.
    let graph = graph_typecheck.graph(db);
    ctx.populate_script_imports(script, graph);

    // Typecheck and analyze script-level functions.
    let units = script.units(db);
    for unit_index in 0..units.len() {
        let parsed_unit = crate::parser::parse_script_unit(db, script, unit_index);
        let unit_source = units[unit_index].source(db);

        // Typecheck the script unit with module graph context.
        let unit_typecheck = crate::tycheck::type_check_with_module_graph(
            db,
            unit_source,
            parsed_unit,
            graph,
            graph_typecheck,
        );

        // Check for script unit typecheck errors.
        let errors = unit_typecheck.errors(db);
        if !errors.is_empty() {
            let type_errors: Vec<_> = errors.iter().map(|e| e.error(db)).collect();
            return Err(InterpError::TypecheckErrors(type_errors));
        }

        // Store expression types for interpreter access.
        ctx.merge_expr_types(unit_typecheck);

        // Analyze each function in the unit.
        for statement in parsed_unit.statements(db) {
            if let crate::ast::Statement::Fun(func_stmt) = statement {
                let analysis = crate::function_analysis::analyze_function(db, *func_stmt, unit_typecheck);
                ctx.script_function_analyses.insert(*func_stmt, analysis);
            }
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
    if !ctx.is_typechecked() {
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
        ast::Statement::Loop(_) => {
            return Err(InterpError::RuntimeError("Loop outside function".to_string()));
        }
        ast::Statement::Break(_) => {
            return Err(InterpError::RuntimeError("Break outside loop".to_string()));
        }
        ast::Statement::Continue(_) => {
            return Err(InterpError::RuntimeError("Continue outside loop".to_string()));
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

    // Evaluate expression, using DPS when type hint is available.
    let final_value = if let Some(type_hint_and_heap) = let_stmt.type_hint(ctx.db) {
        let type_hint = type_hint_and_heap.type_hint(ctx.db);
        match type_hint {
            TypeHint::Option(_) | TypeHint::Result(_) => {
                // Allocate destination with expected type and evaluate with DPS.
                let dest_tydesc = type_hint_to_tydesc(ctx, type_hint_and_heap);
                let dest_ptr = unsafe {
                    datalove_rt::c::dtlv_rti_mem_alloc_local(
                        ctx.runtime.handle(),
                        dest_tydesc,
                        1,
                    )
                };
                if dest_ptr.is_null() {
                    return Err(InterpError::RuntimeError(
                        "Failed to allocate destination for typed let".to_string()
                    ));
                }
                let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };

                // Evaluate with destination.
                let mut value = eval_expression_in_script_scope(ctx, let_stmt.value(ctx.db), Some(dest))?;
                // We allocated the destination, so we own it.
                value.ownership = ValueOwnership::TempOwned;
                value
            }
            _ => {
                // No wrapper type, evaluate normally.
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

    // Get script.
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

                    // Typecheck the unit using appropriate mode.
                    let unit_typecheck = if let Some(graph_typecheck) = ctx.module_graph_typecheck {
                        // ModuleGraph mode - use function with imports.
                        let graph = graph_typecheck.graph(ctx.db);
                        crate::tycheck::type_check_with_module_graph(
                            ctx.db,
                            unit_source,
                            parsed_unit,
                            graph,
                            graph_typecheck,
                        )
                    } else {
                        // Standalone mode - basic typecheck.
                        crate::tycheck::type_check(ctx.db, unit_source, parsed_unit)
                    };

                    // Store expression types for interpreter access.
                    ctx.merge_expr_types(unit_typecheck);

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

    Ok(())
}

// ============================================================================
// Expression Evaluation - Script Scope
// ============================================================================

/// Evaluate an expression in script scope.
pub(super) fn eval_expression_in_script_scope<'db>(
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
            // Void functions can't be used in expression context (typechecker ensures this).
            eval_function_call_in_script_scope(ctx, call_expr)?
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Void function '{}' cannot be used in expression context",
                            call_expr.name(ctx.db).text(ctx.db))
                ))
        }
        ast::ExprFunKind::BinOp(binop_expr) => {
            // Evaluate operands (borrow context - we just read, don't consume variables).
            let lhs = eval_expression_in_script_scope_borrow(ctx, binop_expr.lhs(ctx.db))?;
            let rhs = match eval_expression_in_script_scope_borrow(ctx, binop_expr.rhs(ctx.db)) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };
            // Execute binop with borrowed operands.
            let result = execute_binop(ctx, binop_expr.op(ctx.db), &lhs, &rhs, dest);
            // Clean up temporary operand values.
            destroy_value(ctx, lhs);
            destroy_value(ctx, rhs);
            result
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
            // Evaluate operand (borrow context - we just read, don't consume variables).
            let operand = eval_expression_in_script_scope_borrow(ctx, unary_expr.operand(ctx.db))?;
            // Execute unary operation with borrowed operand.
            let result = execute_unop(ctx, unary_expr.op(ctx.db), &operand, dest);
            // Clean up temporary operand value.
            destroy_value(ctx, operand);
            result
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
        ast::ExprFunKind::True(_) => {
            if let Some(d) = dest {
                write_bool_to_dest(d, true)
            } else {
                allocate_bool(ctx, true)
            }
        }
        ast::ExprFunKind::False(_) => {
            if let Some(d) = dest {
                write_bool_to_dest(d, false)
            } else {
                allocate_bool(ctx, false)
            }
        }
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
            if let Some(d) = dest {
                write_f32_to_dest(d, value)
            } else {
                allocate_f32(ctx, value)
            }
        }
        ast::ExprFunKind::Hex(hex_expr) => {
            let value_str = hex_expr.value(ctx.db).as_str(ctx.db);
            let hex_digits = value_str.trim_start_matches("0x").trim_start_matches("0X");
            let value: u32 = u32::from_str_radix(hex_digits, 16)
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse hex: {}", e)))?;
            if let Some(d) = dest {
                write_u32_to_dest(d, value)
            } else {
                allocate_u32_raw(ctx, value)
            }
        }
        ast::ExprFunKind::String(string_expr) => {
            if let Some(d) = dest {
                write_string_to_dest(ctx, &string_expr, d)
            } else {
                allocate_inline_string(ctx, &string_expr)
            }
        }
        ast::ExprFunKind::List(list_expr) => {
            eval_inline_list(ctx, EvalContext::ScriptScope, expr, &list_expr, dest)
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
        ast::ExprFunKind::AnonStruct(struct_expr) => {
            eval_inline_anon_struct(ctx, EvalContext::ScriptScope, &struct_expr, dest)
        }
        ast::ExprFunKind::AnonEnum(_) => {
            Err(InterpError::InvalidExpression("Enum not yet implemented".to_string()))
        }
        ast::ExprFunKind::Some(some_expr) => {
            super::eval_wrapper_payload_dps(
                ctx, EvalContext::ScriptScope, super::WrapperKind::Some, dest, some_expr.payload(ctx.db)
            )
        }
        ast::ExprFunKind::Ok(ok_expr) => {
            super::eval_wrapper_payload_dps(
                ctx, EvalContext::ScriptScope, super::WrapperKind::Ok, dest, ok_expr.payload(ctx.db)
            )
        }
        ast::ExprFunKind::Er(er_expr) => {
            if dest.is_some() {
                let payload = eval_expression_in_script_scope(ctx, er_expr.payload(ctx.db), None)?;
                literals::write_result_er_from_value(ctx, payload, dest)
            } else {
                return Err(InterpError::RuntimeError(
                    "er expression requires type context (use type hint)".to_string()
                ));
            }
        }
        ast::ExprFunKind::Data(data_expr) => {
            // Evaluate inner expression.
            let inner_value = eval_expression_in_script_scope(ctx, data_expr.value(ctx.db), None)?;
            // Wrap in Data.
            alloc::allocate_data_from_value(ctx, inner_value)
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
        // Linear types: transfer ownership to caller.
        ctx.script_scope.variables.get_mut(&name).unwrap().state = ScriptVarState::Moved;
        // Return as TempOwned - caller takes full ownership.
        // The cleanup will skip Moved variables since ownership was transferred.
        Ok(Value {
            ptr: value.ptr,
            tydesc: value.tydesc,
            ownership: ValueOwnership::TempOwned,
        })
    }
}

/// Read a script-level variable in borrow context (for binop/unop operands).
///
/// Always clones the value, never moves. This implements ref semantics for operators.
fn read_script_variable_borrow<'db>(
    ctx: &mut InterpContext<'db>,
    name: InternedText<'db>,
) -> Result<Value, InterpError> {
    let value = {
        let var = ctx.script_scope.variables.get(&name)
            .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

        // Check if already moved.
        if var.state == ScriptVarState::Moved {
            return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
        }

        var.value
    };

    // Always clone - never move in borrow context.
    Ok(clone_value(ctx, value))
}

/// Evaluate an expression in script scope with borrow semantics.
///
/// Used for binop/unop operands where variables should be cloned, not moved.
fn eval_expression_in_script_scope_borrow<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // Read variable in borrow context (always clones).
            read_script_variable_borrow(ctx, name)
        }
        ast::ExprFunKind::BinOp(binop_expr) => {
            // Nested binop: stay in borrow context.
            let lhs = eval_expression_in_script_scope_borrow(ctx, binop_expr.lhs(ctx.db))?;
            let rhs = match eval_expression_in_script_scope_borrow(ctx, binop_expr.rhs(ctx.db)) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };
            let result = execute_binop(ctx, binop_expr.op(ctx.db), &lhs, &rhs, None);
            destroy_value(ctx, lhs);
            destroy_value(ctx, rhs);
            result
        }
        ast::ExprFunKind::UnaryOp(unary_expr) => {
            // Nested unary: stay in borrow context.
            let operand = eval_expression_in_script_scope_borrow(ctx, unary_expr.operand(ctx.db))?;
            let result = execute_unop(ctx, unary_expr.op(ctx.db), &operand, None);
            destroy_value(ctx, operand);
            result
        }
        ast::ExprFunKind::FunctionCall(call_expr) => {
            // Function calls use normal semantics (arguments may be moved).
            // Void functions can't be used in expression context (typechecker ensures this).
            eval_function_call_in_script_scope(ctx, call_expr)?
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Void function '{}' cannot be used in expression context",
                            call_expr.name(ctx.db).text(ctx.db))
                ))
        }
        // For other expressions, delegate to normal evaluation.
        _ => eval_expression_in_script_scope(ctx, expr, None),
    }
}

// ============================================================================
// Function Calls from Script Scope
// ============================================================================

/// Evaluate a function call from script scope.
/// Returns `None` for void functions.
pub(super) fn eval_function_call_in_script_scope<'db>(
    ctx: &mut InterpContext<'db>,
    call_expr: ast::ExprFunctionCall<'db>,
) -> Result<Option<Value>, InterpError> {
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
                        if v.ownership == ValueOwnership::Borrowed && v.ptr == param_ptr {
                            // Expression wrote to destination and returned borrowed ref.
                            Value { ptr: param_ptr, tydesc: param_tydesc, ownership: ValueOwnership::TempOwned }
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
                    if v.ownership == ValueOwnership::Borrowed && v.ptr == inner_ptr {
                        Value { ptr: inner_ptr, tydesc: inner_tydesc, ownership: ValueOwnership::TempOwned }
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
            // Non-Option/Result parameter - use DPS with parameter type destination.
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
                    if v.ownership == ValueOwnership::Borrowed && v.ptr == param_ptr {
                        // Expression wrote to destination and returned borrowed ref.
                        Value { ptr: param_ptr, tydesc: param_tydesc, ownership: ValueOwnership::TempOwned }
                    } else {
                        // Expression didn't use dest - free unused param buffer and use returned value.
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
        }
    }

    // Execute the function body with arguments.
    // Set current_module if this is a module function.

    // Get return type to allocate destination buffer.
    let return_type = func.return_type(ctx.db);

    // Allocate return destination if function has a return type.
    let (return_dest, return_ptr, return_tydesc) = if let Some(ret_type) = return_type {
        let ret_tydesc = type_hint_to_tydesc(ctx, ret_type);
        let ret_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(
                ctx.runtime.handle(),
                ret_tydesc,
                1
            )
        };
        if ret_ptr.is_null() {
            for val in arg_values { destroy_value(ctx, val); }
            return Err(InterpError::RuntimeError("Failed to allocate return buffer".to_string()));
        }
        let dest = Destination { ptr: ret_ptr, tydesc: ret_tydesc };
        (Some(dest), ret_ptr, ret_tydesc)
    } else {
        (None, std::ptr::null_mut(), std::ptr::null())
    };

    // Execute with return destination.
    let result = execute_function_body(ctx, func, func_module, arg_values, return_dest);

    // Handle result - convert to TempOwned if we provided a destination.
    match result {
        Ok(Some(_value)) if return_dest.is_some() => {
            // Return value was written to our buffer via DPS.
            // Return as TempOwned since script scope owns this memory.
            Ok(Some(Value {
                ptr: return_ptr,
                tydesc: return_tydesc,
                ownership: ValueOwnership::TempOwned,
            }))
        }
        Ok(Some(value)) => {
            // Function returned a value without DPS - return as-is.
            Ok(Some(value))
        }
        Ok(None) => {
            // Void function - no return value.
            Ok(None)
        }
        Err(e) => {
            // Error - free return buffer if allocated.
            if return_dest.is_some() {
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        ctx.runtime.handle(),
                        return_tydesc,
                        1,
                        return_ptr,
                    );
                }
            }
            Err(e)
        }
    }
}
