//! Statement and control flow lowering.
//!
//! Handles lowering of statements (let, var, set, ret, if, loop, etc.)
//! and control flow constructs.

use bct::text::InternedText;
use datalove_datafun_ast::ast::{self, Statement, ExprFun};
use datalove_datafun_ir::{IrType, Operand, Instruction, Terminator, SlotDest, ParamMode, ValueId};
use super::context::LowerCtx;
use super::expr::lower_expression;
use super::LowerError;

/// Lower a statement (without index tracking, for compatibility).
pub fn lower_statement<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    lower_statement_indexed(ctx, stmt, 0)
}

/// Lower a statement with index tracking for drop schedule.
pub fn lower_statement_indexed<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name(ctx.db).text(ctx.db).to_string();
            let init_expr = let_stmt.value(ctx.db);
            let value_id = lower_expression(ctx, init_expr)?;
            let operand = Operand::Value(value_id);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name(ctx.db).text(ctx.db).to_string();
            // Get type from the initialization expression.
            let init_expr = var_stmt.value(ctx.db);
            let slot_type = ctx.expr_type(init_expr);
            let slot = ctx.fresh_slot(slot_type.clone());
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.emit(Instruction::SlotStore {
                dest: SlotDest::Local(slot),
                value: Operand::Value(value_id),
            });
            let operand = Operand::Slot(slot);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            Ok(())
        }
        Statement::Set(set_stmt) => {
            let name = set_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, set_stmt.value(ctx.db))?;
            match ctx.lookup_var(&name) {
                Some(Operand::Slot(slot)) => {
                    // Drop old value before storing new one.
                    if let Some(slot_type) = ctx.slot_type(slot).cloned() {
                        if !slot_type.is_copy() {
                            ctx.emit(Instruction::Drop { operand: Operand::Slot(slot) });
                        }
                    }
                    ctx.emit(Instruction::SlotStore {
                        dest: SlotDest::Local(slot),
                        value: Operand::Value(value_id),
                    });
                    Ok(())
                }
                Some(Operand::Param(param)) => {
                    // Only Mut/Out params can be assigned.
                    let mode = ctx.param_mode(param);
                    if mode == Some(ParamMode::Mut) || mode == Some(ParamMode::Out) {
                        // ParamStore handles destroying the old value internally.
                        ctx.emit(Instruction::ParamStore {
                            param,
                            value: Operand::Value(value_id),
                        });
                        Ok(())
                    } else {
                        Err(LowerError::VariableNotMutable(name))
                    }
                }
                _ => Err(LowerError::VariableNotMutable(name)),
            }
        }
        Statement::Ret(ret_stmt) => {
            let value = if let Some(expr) = ret_stmt.value(ctx.db) {
                let value_id = lower_expression(ctx, expr)?;
                Some(Operand::Value(value_id))
            } else {
                None
            };
            // Emit drops for all values in all scopes before return.
            ctx.emit_before_return_drops(stmt_idx);
            ctx.finish_block(Terminator::Return { value });
            // Start a new unreachable block (code after return).
            let new_block = ctx.fresh_block();
            ctx.start_unreachable_block(new_block);
            Ok(())
        }
        Statement::If(if_stmt) => {
            lower_if(ctx, *if_stmt, stmt_idx)
        }
        Statement::Loop(loop_stmt) => {
            lower_loop(ctx, *loop_stmt, stmt_idx)
        }
        Statement::Break(break_stmt) => {
            let loop_ctx = ctx.loop_stack.last()
                .ok_or(LowerError::BreakOutsideLoop)?;
            let break_target = loop_ctx.exit;

            // Lower break values and build args for the exit block.
            let break_values = break_stmt.values(ctx.db);
            let mut break_args = Vec::new();
            for value in break_values {
                let value_id = lower_expression(ctx, *value)?;
                break_args.push(Operand::Value(value_id));
            }

            // Emit drops for all scopes up to the loop.
            // This now includes carries that were NOT passed as break args.
            ctx.emit_before_break_drops(stmt_idx);

            ctx.finish_block(Terminator::Goto { target: break_target, args: break_args });
            // Start unreachable block for code after break.
            let dead_block = ctx.fresh_block();
            ctx.start_unreachable_block(dead_block);
            Ok(())
        }
        Statement::Continue(continue_stmt) => {
            let loop_ctx = ctx.loop_stack.last()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            let continue_target = loop_ctx.header;
            let old_carry_values = loop_ctx.carry_values.clone();

            // Lower continue values (new carry values) and build args.
            let continue_values = continue_stmt.values(ctx.db);
            let mut continue_args = Vec::new();

            if continue_values.is_empty() {
                // No values provided - use current carry values.
                for carry_value in &old_carry_values {
                    continue_args.push(Operand::Value(*carry_value));
                }
            } else {
                // Use provided values.
                for value in continue_values {
                    let value_id = lower_expression(ctx, *value)?;
                    continue_args.push(Operand::Value(value_id));
                }
            }

            // Emit drops for current loop iteration.
            // This now includes old carries when new values are provided.
            ctx.emit_before_continue_drops(stmt_idx);

            ctx.finish_block(Terminator::Goto { target: continue_target, args: continue_args });
            // Start unreachable block for code after continue.
            let dead_block = ctx.fresh_block();
            ctx.start_unreachable_block(dead_block);
            Ok(())
        }
        Statement::Fun(_) => {
            // Nested functions not supported in IR yet.
            Err(LowerError::NotImplemented("nested functions".to_string()))
        }
        Statement::Require(_) | Statement::Import(_) => {
            // These are module-level, not in function bodies.
            Ok(())
        }
        Statement::DebugLog(stmt) => {
            let value_id = lower_expression(ctx, stmt.value(ctx.db))?;
            ctx.emit(Instruction::DebugLog {
                operand: Operand::Value(value_id),
            });
            // Note: no drop - debuglog borrows, does not consume.
            Ok(())
        }
        Statement::ParseError(_) => {
            Err(LowerError::ParseError)
        }
    }
}

/// Lower an if statement.
///
/// Handles three cases based on condition type:
/// - Bool: regular if/else
/// - Option with then_binding: destructure Some value
/// - Result with then_binding and else_binding: destructure Ok/Err values
pub fn lower_if<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    let condition = if_stmt.condition(ctx.db);
    let then_binding = if_stmt.then_binding(ctx.db);
    let else_binding = if_stmt.else_binding(ctx.db);
    let cond_type = ctx.expr_type(condition);

    match (&cond_type, then_binding) {
        // Option destructuring: if opt_value |x| ... end if
        (IrType::Option(inner_type), Some(binding_name)) => {
            lower_if_option(ctx, if_stmt, condition, inner_type, binding_name, stmt_idx)
        }

        // Result destructuring: if result_value |ok_val| else |err_val| ... end if
        (IrType::Result(ok_type), Some(binding_name)) => {
            // Typechecker enforces else_binding for Result (F046).
            let err_binding = else_binding
                .ok_or_else(|| LowerError::NotImplemented(
                    "Result if-binding without else binding".to_string()
                ))?;
            lower_if_result(ctx, if_stmt, condition, ok_type, binding_name, err_binding, stmt_idx)
        }

        // Boolean condition (no binding).
        (IrType::Bool, None) => {
            lower_if_bool(ctx, if_stmt, condition, stmt_idx)
        }

        // Invalid combinations.
        (_, Some(_)) => {
            // Binding on non-Option/non-Result type.
            Err(LowerError::NotImplemented(format!(
                "if-binding requires Option or Result type, got {:?}",
                cond_type
            )))
        }
        (_, None) => {
            // Non-bool without binding - typechecker should catch this.
            Err(LowerError::NotImplemented(format!(
                "if condition must be Bool without binding, got {:?}",
                cond_type
            )))
        }
    }
}

/// Lower a boolean if statement (no binding).
fn lower_if_bool<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    condition: ExprFun<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    let cond_id = lower_expression(ctx, condition)?;

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    // Finish current block with branch.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(cond_id),
        then_block,
        then_args: Vec::new(),
        else_block,
        else_args: Vec::new(),
    });

    // Lower then branch.
    ctx.start_block(then_block);
    let then_body = if_stmt.then_body(ctx.db);
    for (idx, stmt) in then_body.iter().enumerate() {
        lower_statement_indexed(ctx, stmt, idx)?;
    }
    // Only emit Goto if branch didn't terminate early.
    let then_terminated = ctx.is_unreachable();
    if !then_terminated {
        ctx.emit_then_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // Lower else branch.
    ctx.start_block(else_block);
    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for (idx, stmt) in else_body.iter().enumerate() {
            lower_statement_indexed(ctx, stmt, idx)?;
        }
    }
    // Only emit Goto if branch didn't terminate early.
    let else_terminated = ctx.is_unreachable();
    if !else_terminated {
        ctx.emit_else_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // Start merge block. If both branches terminated, it's unreachable.
    if then_terminated && else_terminated {
        ctx.start_unreachable_block(merge_block);
    } else {
        ctx.start_block(merge_block);
    }
    Ok(())
}

/// Lower an Option if-binding: `if opt_value |x| ... end if`
///
/// Moves the inner value out of Some to the binding. If None, takes else branch.
fn lower_if_option<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    condition: ExprFun<'db>,
    inner_type: &IrType,
    binding_name: InternedText<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    // Lower the Option expression.
    let opt_id = lower_expression(ctx, condition)?;

    // Emit UnwrapOption instruction.
    // dest: receives inner value (only valid when is_some=true).
    // is_some: boolean flag for branching.
    let inner_dest = ctx.fresh_value(inner_type.clone());
    let is_some = ctx.fresh_value(IrType::Bool);

    ctx.emit(Instruction::UnwrapOption {
        dest: inner_dest,
        is_some,
        src: Operand::Value(opt_id),
    });

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_some),
        then_block,
        then_args: Vec::new(),
        else_block,
        else_args: Vec::new(),
    });

    // === Then branch: Some case ===
    ctx.start_block(then_block);

    // Bind the inner value to the binding name.
    let binding_str = binding_name.text(ctx.db);
    let old_binding = ctx.lookup_var(binding_str);
    ctx.bind_var(binding_str, Operand::Value(inner_dest));

    // Register binding with drop schedule system.
    ctx.record_binding_operand(Operand::Value(inner_dest));

    for stmt in if_stmt.then_body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // Restore old binding if we shadowed something.
    if let Some(old) = old_binding {
        ctx.bind_var(binding_str, old);
    } else {
        ctx.variables.remove(binding_str);
    }

    // Only emit Goto if branch didn't terminate early.
    let then_terminated = ctx.is_unreachable();
    if !then_terminated {
        ctx.emit_then_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // === Else branch: None case ===
    ctx.start_block(else_block);
    // No binding in else branch for Option.
    // inner_dest is NOT valid here - do NOT access or drop it.

    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }

    // Only emit Goto if branch didn't terminate early.
    let else_terminated = ctx.is_unreachable();
    if !else_terminated {
        ctx.emit_else_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // Start merge block. If both branches terminated, it's unreachable.
    if then_terminated && else_terminated {
        ctx.start_unreachable_block(merge_block);
    } else {
        ctx.start_block(merge_block);
    }
    Ok(())
}

/// Lower a Result if-binding: `if result_value |ok_val| else |err_val| ... end if`
///
/// Moves the Ok payload to then_binding, or Error to else_binding.
fn lower_if_result<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
    condition: ExprFun<'db>,
    ok_type: &IrType,
    ok_binding: InternedText<'db>,
    err_binding: InternedText<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    // Lower the Result expression.
    let result_id = lower_expression(ctx, condition)?;

    // Emit UnwrapResult instruction.
    // ok_dest: receives Ok payload (only valid when is_ok=true).
    // err_dest: receives Error (only valid when is_ok=false).
    // is_ok: boolean flag for branching.
    let ok_dest = ctx.fresh_value(ok_type.clone());
    let err_dest = ctx.fresh_value(IrType::Error);
    let is_ok = ctx.fresh_value(IrType::Bool);

    ctx.emit(Instruction::UnwrapResult {
        ok_dest,
        err_dest,
        is_ok,
        src: Operand::Value(result_id),
    });

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_ok),
        then_block,
        then_args: Vec::new(),
        else_block,
        else_args: Vec::new(),
    });

    // === Then branch: Ok case ===
    ctx.start_block(then_block);

    // Bind ok_dest to the ok_binding name.
    let ok_binding_str = ok_binding.text(ctx.db);
    let old_ok_binding = ctx.lookup_var(ok_binding_str);
    ctx.bind_var(ok_binding_str, Operand::Value(ok_dest));

    // Register binding with drop schedule system.
    ctx.record_binding_operand(Operand::Value(ok_dest));

    for stmt in if_stmt.then_body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // Restore old binding.
    if let Some(old) = old_ok_binding {
        ctx.bind_var(ok_binding_str, old);
    } else {
        ctx.variables.remove(ok_binding_str);
    }

    // Only emit Goto if branch didn't terminate early.
    let then_terminated = ctx.is_unreachable();
    if !then_terminated {
        ctx.emit_then_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // === Else branch: Error case ===
    ctx.start_block(else_block);

    // Bind err_dest to the err_binding name.
    let err_binding_str = err_binding.text(ctx.db);
    let old_err_binding = ctx.lookup_var(err_binding_str);
    ctx.bind_var(err_binding_str, Operand::Value(err_dest));

    // Register binding with drop schedule system.
    ctx.record_binding_operand(Operand::Value(err_dest));

    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }

    // Restore old binding.
    if let Some(old) = old_err_binding {
        ctx.bind_var(err_binding_str, old);
    } else {
        ctx.variables.remove(err_binding_str);
    }

    // Only emit Goto if branch didn't terminate early.
    let else_terminated = ctx.is_unreachable();
    if !else_terminated {
        ctx.emit_else_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // Start merge block. If both branches terminated, it's unreachable.
    if then_terminated && else_terminated {
        ctx.start_unreachable_block(merge_block);
    } else {
        ctx.start_block(merge_block);
    }
    Ok(())
}

/// Lower a loop statement with optional carry/bring and while condition.
pub fn lower_loop<'db>(
    ctx: &mut LowerCtx<'db>,
    loop_stmt: ast::StmtLoop<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    use super::context::LoopLowerContext;

    let carries = loop_stmt.carries(ctx.db);
    let brings = loop_stmt.brings(ctx.db);
    let condition = loop_stmt.condition(ctx.db);

    let loop_header = ctx.fresh_block();
    let loop_exit = ctx.fresh_block();

    // Lower carry init expressions and collect types.
    let mut carry_init_values = Vec::new();
    let mut carry_types = Vec::new();
    for carry in carries {
        let init = carry.init(ctx.db);
        let value_id = lower_expression(ctx, init)?;
        let ty = ctx.expr_type(init);
        carry_init_values.push(Operand::Value(value_id));
        carry_types.push(ty);
    }

    // Get bring types from type hints.
    let mut bring_types = Vec::new();
    for bring in brings {
        let ty = if let Some(type_hint) = bring.type_hint(ctx.db) {
            IrType::from_type_hint(ctx.db, &type_hint)
        } else {
            // Should be caught by type checker.
            IrType::Unit
        };
        bring_types.push(ty);
    }

    // Jump to loop header with carry init values.
    ctx.finish_block(Terminator::Goto {
        target: loop_header,
        args: carry_init_values,
    });

    // Start loop header block with params for carries.
    ctx.start_block(loop_header);
    let mut carry_param_values = Vec::new();
    for (i, ty) in carry_types.iter().enumerate() {
        let param_value = ctx.fresh_value(ty.clone());
        ctx.current_block_params.push(param_value);
        carry_param_values.push(param_value);

        // Bind carry to variable name.
        let carry_name = carries[i].name(ctx.db).text(ctx.db).to_string();
        let operand = Operand::Value(param_value);
        ctx.bind_var(&carry_name, operand);
        // Register binding operand so drop schedule can find it.
        ctx.record_binding_operand(operand);
    }

    // Allocate bring values for the exit block params.
    let mut bring_param_values = Vec::new();
    for ty in &bring_types {
        let bring_value = ctx.fresh_value(ty.clone());
        bring_param_values.push(bring_value);
    }

    // Handle while condition if present.
    // Creates: if cond goto body_block else goto while_false_block (which drops carries).
    let body_block = if let Some(cond_expr) = condition {
        let body_block = ctx.fresh_block();
        let while_false_block = ctx.fresh_block();

        // Lower the condition expression.
        let cond_value = lower_expression(ctx, cond_expr)?;

        // Pass carry values to while_false_block so it can drop them.
        let carry_args: Vec<Operand> = carry_param_values.iter()
            .map(|v| Operand::Value(*v))
            .collect();

        ctx.finish_block(Terminator::Branch {
            cond: Operand::Value(cond_value),
            then_block: body_block,
            then_args: Vec::new(),
            else_block: while_false_block,
            else_args: carry_args,
        });

        // Build while_false_block: receives carries, evaluates else_break, drops carries, goes to exit.
        ctx.start_block(while_false_block);
        let while_false_carries: Vec<ValueId> = carry_types.iter().map(|ty| {
            let v = ctx.fresh_value(ty.clone());
            ctx.current_block_params.push(v);
            v
        }).collect();

        // Rebind carry names to while_false_block params for else_break evaluation.
        for (i, carry) in carries.iter().enumerate() {
            let carry_name = carry.name(ctx.db).text(ctx.db).to_string();
            ctx.bind_var(&carry_name, Operand::Value(while_false_carries[i]));
        }

        // Lower else_break expressions for bring values.
        // Type checker ensures else_break values match bring_types when brings are present.
        let else_break = loop_stmt.else_break(ctx.db).unwrap_or_default();
        let mut exit_args: Vec<Operand> = Vec::with_capacity(else_break.len());
        for expr in &else_break {
            let value = lower_expression(ctx, *expr)?;
            exit_args.push(Operand::Value(value));
        }

        // Drop carry values that weren't consumed by else_break expressions.
        // A carry is consumed if it appears directly in exit_args.
        for carry_value in &while_false_carries {
            let was_consumed = exit_args.iter().any(|arg| {
                matches!(arg, Operand::Value(v) if *v == *carry_value)
            });
            if !was_consumed {
                ctx.emit(Instruction::Drop { operand: Operand::Value(*carry_value) });
            }
        }

        ctx.finish_block(Terminator::Goto {
            target: loop_exit,
            args: exit_args,
        });

        // Restore carry bindings to loop header values for the body block.
        for (i, carry) in carries.iter().enumerate() {
            let carry_name = carry.name(ctx.db).text(ctx.db).to_string();
            ctx.bind_var(&carry_name, Operand::Value(carry_param_values[i]));
        }

        ctx.start_block(body_block);
        Some(body_block)
    } else {
        None
    };

    // Push loop context for break/continue.
    ctx.loop_stack.push(LoopLowerContext {
        header: loop_header,
        exit: loop_exit,
        carry_values: carry_param_values.clone(),
        carry_types: carry_types.clone(),
        bring_values: bring_param_values.clone(),
        bring_types: bring_types.clone(),
    });

    // Lower loop body with proper statement indexing.
    for (idx, stmt) in loop_stmt.body(ctx.db).iter().enumerate() {
        lower_statement_indexed(ctx, stmt, idx)?;
    }

    // Emit drops before looping back (excludes carries).
    ctx.emit_loop_body_end_drops(stmt_idx);

    // Loop back to header with current carry values.
    // For implicit continue at body end, use the same values.
    let continue_args: Vec<Operand> = carry_param_values.iter()
        .map(|v| Operand::Value(*v))
        .collect();
    ctx.finish_block(Terminator::Goto {
        target: loop_header,
        args: continue_args,
    });

    // Pop loop context.
    ctx.loop_stack.pop();

    // Start loop exit block with params for brings.
    ctx.start_block(loop_exit);
    for bring_value in &bring_param_values {
        ctx.current_block_params.push(*bring_value);
    }

    // Bind bring names in scope after the loop.
    // Also record binding-to-operand mapping so drops work correctly.
    for (i, bring) in brings.iter().enumerate() {
        let bring_name = bring.name(ctx.db).text(ctx.db).to_string();
        let operand = Operand::Value(bring_param_values[i]);
        ctx.bind_var(&bring_name, operand);
        ctx.record_binding_operand(operand);
    }

    let _ = body_block; // Silence unused warning.
    Ok(())
}
