//! Statement and control flow lowering.
//!
//! Handles lowering of statements (let, var, set, ret, if, loop, etc.)
//! and control flow constructs.

use bct::text::InternedText;
use datalove_datafun_ast::ast::{self, Statement, ExprFun};
use datalove_datafun_ir::{IrType, Operand, Instruction, Terminator, SlotDest, ParamMode};
use super::context::LowerCtx;
use super::expr::{lower_expression, lower_expression_for_ref};
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
            let name = let_stmt.name.text(ctx.db).to_string();
            let init_expr = let_stmt.value;
            let value_id = lower_expression(ctx, init_expr)?;
            let operand = Operand::Value(value_id);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name.text(ctx.db).to_string();
            // Get type from the initialization expression.
            let init_expr = var_stmt.value;
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
            lower_set(ctx, set_stmt)
        }
        Statement::Ret(ret_stmt) => {
            let value = if let Some(expr) = ret_stmt.value {
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
            lower_if(ctx, if_stmt, stmt_idx)
        }
        Statement::Loop(loop_stmt) => {
            lower_loop(ctx, loop_stmt, stmt_idx)
        }
        Statement::Break(_) => {
            let loop_ctx = ctx.loop_stack.last()
                .ok_or(LowerError::BreakOutsideLoop)?;
            let break_target = loop_ctx.exit;

            // Emit drops for all scopes up to the loop.
            ctx.emit_before_break_drops(stmt_idx);

            ctx.finish_block(Terminator::Goto { target: break_target, args: Vec::new() });
            // Start unreachable block for code after break.
            let dead_block = ctx.fresh_block();
            ctx.start_unreachable_block(dead_block);
            Ok(())
        }
        Statement::Continue(_) => {
            let loop_ctx = ctx.loop_stack.last()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            let continue_target = loop_ctx.header;

            // Emit drops for current loop iteration.
            ctx.emit_before_continue_drops(stmt_idx);

            ctx.finish_block(Terminator::Goto { target: continue_target, args: Vec::new() });
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
            // Use lower_expression_for_ref to handle field projections with GetFieldRef.
            // This borrows the value instead of copying, avoiding shallow-copy issues.
            let operand = lower_expression_for_ref(ctx, stmt.value)?;
            ctx.emit(Instruction::DebugLog { operand });
            // Note: no drop - debuglog borrows, does not consume.
            // Refs are Copy and don't need drops.
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
    if_stmt: &ast::StmtIf<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    let condition = if_stmt.condition;
    let then_binding = if_stmt.then_binding;
    let else_binding = if_stmt.else_binding;
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
    if_stmt: &ast::StmtIf<'db>,
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
    for (idx, stmt) in if_stmt.then_body.iter().enumerate() {
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
    if let Some(else_body) = &if_stmt.else_body {
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
    if_stmt: &ast::StmtIf<'db>,
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

    for stmt in &if_stmt.then_body {
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

    if let Some(else_body) = &if_stmt.else_body {
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
    if_stmt: &ast::StmtIf<'db>,
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

    for stmt in &if_stmt.then_body {
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

    if let Some(else_body) = &if_stmt.else_body {
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

/// Lower a loop statement with optional while condition.
pub fn lower_loop<'db>(
    ctx: &mut LowerCtx<'db>,
    loop_stmt: &ast::StmtLoop<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    use super::context::LoopLowerContext;

    let condition = loop_stmt.condition;

    let loop_header = ctx.fresh_block();
    let loop_exit = ctx.fresh_block();

    // Jump to loop header.
    ctx.finish_block(Terminator::Goto {
        target: loop_header,
        args: Vec::new(),
    });

    // Start loop header block.
    ctx.start_block(loop_header);

    // Handle while condition if present.
    let body_block = if let Some(cond_expr) = condition {
        let body_block = ctx.fresh_block();

        // Lower the condition expression.
        let cond_value = lower_expression(ctx, cond_expr)?;

        ctx.finish_block(Terminator::Branch {
            cond: Operand::Value(cond_value),
            then_block: body_block,
            then_args: Vec::new(),
            else_block: loop_exit,
            else_args: Vec::new(),
        });

        ctx.start_block(body_block);
        Some(body_block)
    } else {
        None
    };

    // Push loop context for break/continue.
    ctx.loop_stack.push(LoopLowerContext {
        header: loop_header,
        exit: loop_exit,
    });

    // Lower loop body with proper statement indexing.
    for (idx, stmt) in loop_stmt.body.iter().enumerate() {
        lower_statement_indexed(ctx, stmt, idx)?;
    }

    // Emit drops before looping back.
    ctx.emit_loop_body_end_drops(stmt_idx);

    // Loop back to header.
    ctx.finish_block(Terminator::Goto {
        target: loop_header,
        args: Vec::new(),
    });

    // Pop loop context.
    ctx.loop_stack.pop();

    // Start loop exit block.
    ctx.start_block(loop_exit);

    let _ = body_block; // Silence unused warning.
    Ok(())
}

/// Lower a set statement.
fn lower_set<'db>(
    ctx: &mut LowerCtx<'db>,
    set_stmt: &ast::StmtSet<'db>,
) -> Result<(), LowerError> {
    match &set_stmt.target {
        ast::SetTarget::Name(name) => {
            let name_str = name.text(ctx.db).to_string();
            let value_id = lower_expression(ctx, set_stmt.value)?;
            match ctx.lookup_var(&name_str) {
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
                        Err(LowerError::VariableNotMutable(name_str))
                    }
                }
                _ => Err(LowerError::VariableNotMutable(name_str)),
            }
        }
        ast::SetTarget::Proj(proj) => {
            // Walk the projection chain to find root and collect field path.
            let (root_name, field_path) = collect_field_path(ctx, proj)?;
            let root_name_str = root_name.text(ctx.db).to_string();

            // Lower the value expression.
            let value_id = lower_expression(ctx, set_stmt.value)?;

            // Look up the root slot.
            match ctx.lookup_var(&root_name_str) {
                Some(Operand::Slot(slot)) => {
                    // Emit SetField instruction.
                    ctx.emit(Instruction::SetField {
                        slot: SlotDest::Local(slot),
                        field_path,
                        value: Operand::Value(value_id),
                    });
                    Ok(())
                }
                Some(Operand::Param(_)) => {
                    // TODO: Support SetField on mut params.
                    Err(LowerError::NotImplemented(
                        "field projection set on mutable parameters".to_string()
                    ))
                }
                _ => Err(LowerError::VariableNotMutable(root_name_str)),
            }
        }
    }
}

/// Collect the field path from a projection target, resolving named fields to indices.
///
/// Returns (root_name, field_indices) where field_indices is the chain of
/// field indices from root to target.
pub(super) fn collect_field_path<'db>(
    ctx: &LowerCtx<'db>,
    proj: &ast::SetTargetProj<'db>,
) -> Result<(InternedText<'db>, Vec<u32>), LowerError> {
    // First, collect all the field selectors from root to leaf.
    let (root_name, selectors) = collect_selectors(ctx.db, proj);

    // Look up the root variable's type.
    let root_name_str = root_name.text(ctx.db).to_string();
    let root_type = ctx.slot_type_by_name(&root_name_str)
        .ok_or_else(|| LowerError::VariableNotFound(root_name_str.clone()))?;

    // Now resolve each selector to a field index by walking through the types.
    let mut path = Vec::new();
    let mut current_type = root_type.clone();

    for selector in selectors {
        match selector {
            ast::FieldSelector::Index(idx) => {
                // Verify the index is valid and get the field type.
                match &current_type {
                    IrType::Tuple(fields) => {
                        if (idx as usize) >= fields.len() {
                            return Err(LowerError::InvalidLiteral(
                                format!("tuple index {} out of bounds (tuple has {} fields)", idx, fields.len())
                            ));
                        }
                        current_type = fields[idx as usize].clone();
                        path.push(idx);
                    }
                    _ => {
                        return Err(LowerError::InvalidLiteral(
                            format!("cannot index into non-tuple type: {:?}", current_type)
                        ));
                    }
                }
            }
            ast::FieldSelector::Name(name) => {
                let name_str = name.text(ctx.db);
                match &current_type {
                    IrType::Struct(fields) => {
                        // Find the field by name.
                        let field_idx = fields.iter()
                            .position(|(n, _)| n == name_str)
                            .ok_or_else(|| LowerError::InvalidLiteral(
                                format!("field '{}' not found in struct", name_str)
                            ))?;
                        current_type = fields[field_idx].1.clone();
                        path.push(field_idx as u32);
                    }
                    _ => {
                        return Err(LowerError::InvalidLiteral(
                            format!("cannot access field '{}' on non-struct type: {:?}", name_str, current_type)
                        ));
                    }
                }
            }
        }
    }

    Ok((root_name, path))
}

/// Helper to collect all field selectors from a projection chain.
fn collect_selectors<'db>(
    db: &'db dyn salsa::Database,
    proj: &ast::SetTargetProj<'db>,
) -> (InternedText<'db>, Vec<ast::FieldSelector<'db>>) {
    let (root_name, mut selectors) = match proj.base.as_ref() {
        ast::SetTarget::Name(name) => (*name, Vec::new()),
        ast::SetTarget::Proj(base_proj) => collect_selectors(db, base_proj),
    };
    selectors.push(proj.field.clone());
    (root_name, selectors)
}
