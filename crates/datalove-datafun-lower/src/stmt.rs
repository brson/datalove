//! Statement lowering.
//!
//! Lowers statements (let, var, set, return) and control flow (if, loop, break,
//! continue). Called from both script and function body lowering.

use bct::text::InternedText;
use datalove_datafun_ast::ast::{self, Statement, ExprFun, ExprFunKind};
use datalove_datafun_ir::{IrType, Operand, ValueId, SlotId, Instruction, Terminator, SlotDest, ParamMode};
use datalove_datafun_sema::Destructure;
use super::context::LowerCtx;
use super::expr::{lower_expression, lower_expression_for_ref, lower_operand, operand_type, field_type_from_base};
use super::LowerError;

/// Check if a set statement is a self-assignment (set v0 = v0).
///
/// Self-assignment is a no-op and must be detected early because the lowering
/// sequence (SlotLoad -> Drop -> SlotStore) would incorrectly destroy the value
/// before copying it.
fn is_self_assignment<'db>(
    ctx: &LowerCtx<'db>,
    target_name: &str,
    value_expr: ExprFun<'db>,
) -> bool {
    // Check if the value expression is just a name reference.
    if let ExprFunKind::Place(ref place) = value_expr.expr(ctx.db) {
        if !place.steps.is_empty() {
            return false;
        }
        let value_name = place.root.text(ctx.db);
        // Check if it's the same name as the target.
        if value_name == target_name {
            // Check if target is bound to a slot (mutable variable).
            if let Some(Operand::Slot(_)) = ctx.lookup_var(target_name) {
                return true;
            }
        }
    }
    false
}

/// Take apart the value a `let` or `var` binds.
///
/// Returns each name with the value bound to it and its type, in the order the
/// pattern names them, which is the order ownership analysis allocated them in.
pub(super) fn lower_destructure<'db>(
    ctx: &mut LowerCtx<'db>,
    binding: &ast::Binding<'db>,
    init_expr: ExprFun<'db>,
    value_id: ValueId,
) -> Vec<(String, ValueId, IrType)> {
    let ty = ctx.expr_type(init_expr);
    let db = ctx.db;
    match Destructure::of(db, binding, &ty) {
        Destructure::Whole(name) => vec![(name.text(db).to_string(), value_id, ty)],
        Destructure::Fields { names, field_count } => {
            // A pattern names every field, so every field has a name to go to.
            let mut dests = vec![None; field_count];
            for (_, index, field_ty) in &names {
                dests[*index as usize] = Some(ctx.fresh_value(field_ty.clone()));
            }
            let dests: Vec<ValueId> = dests.into_iter()
                .map(|d| d.expect("a destructure names every field"))
                .collect();
            ctx.emit_unpack(dests.clone(), Operand::Value(value_id));
            names.into_iter()
                .map(|(name, index, field_ty)| (name.text(db).to_string(), dests[index as usize], field_ty))
                .collect()
        }
        Destructure::Payload(name, payload_ty) => {
            // A term is laid out as its payload, so taking it out is a move.
            let dest = ctx.fresh_value(payload_ty.clone());
            ctx.emit(Instruction::Move { dest, src: Operand::Value(value_id) });
            vec![(name.text(db).to_string(), dest, payload_ty)]
        }
        Destructure::Nothing => vec![],
    }
}

/// Lower a `var`, giving each name it binds a slot of its own.
///
/// Returns each name with its slot, in the order the pattern names them.
/// Script lowering does not keep a wrapped container's shape, and passes
/// `record_shape` false.
pub(super) fn lower_var<'db>(
    ctx: &mut LowerCtx<'db>,
    var_stmt: &ast::StmtVar<'db>,
    record_shape: bool,
) -> Result<Vec<(String, SlotId)>, LowerError> {
    let Some(init_expr) = var_stmt.value else {
        // Uninitialized var - get type from type hint.
        let name = var_stmt.binding.as_name()
            .expect("the parser requires a plain name without a value")
            .text(ctx.db).to_string();
        let type_hint = var_stmt.type_hint.as_ref()
            .expect("uninitialized var must have type hint");
        let slot = ctx.fresh_slot(IrType::from_type_hint(ctx.db, type_hint));
        let operand = Operand::Slot(slot);
        ctx.bind_var(&name, operand);
        // The slot is uninitialized and tracked.
        ctx.record_binding_operand(operand);
        return Ok(vec![(name, slot)]);
    };

    let value_id = lower_expression(ctx, init_expr)?;
    let mut slots = vec![];
    for (name, value_id, slot_type) in lower_destructure(ctx, &var_stmt.binding, init_expr, value_id) {
        let is_copy = slot_type.is_copy();
        let slot = ctx.fresh_slot(slot_type);
        let operand = Operand::Slot(slot);
        ctx.bind_var(&name, operand);
        if record_shape && var_stmt.binding.as_name().is_some() {
            if let Some(shape) = super::expr::container_shape(ctx, init_expr) {
                ctx.record_wrapped_shape(operand, shape);
            }
        }
        // Record binding operand for drop schedule. This has to happen
        // before the store, which asks whether the slot is tracked.
        ctx.record_binding_operand(operand);
        if is_copy {
            ctx.emit_slot_store_copy(SlotDest::Local(slot), Operand::Value(value_id));
        } else {
            ctx.emit_slot_store_move(SlotDest::Local(slot), Operand::Value(value_id));
        }
        slots.push((name, slot));
    }
    Ok(slots)
}

/// Lower a statement.
pub fn lower_statement<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    lower_statement_impl(ctx, stmt)
}

/// Lower a statement, allocating a globally-unique statement ID.
fn lower_statement_impl<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    // Allocate a globally-unique statement ID that matches ownership analysis.
    let stmt_idx = ctx.alloc_stmt_id(stmt);

    // And say that this is the statement being lowered, for as long as it is.
    //
    // The drop schedule is keyed by this id, and everything that reads it --
    // an early return out of the middle of a statement, chiefly -- asks for
    // the current one. The body loops set it to the statement's position among
    // the top-level ones, which is the same number only while nothing nests:
    // an `if` with two statements in it puts the two numberings out of step,
    // and a `?` or a checked overflow inside that branch then looked up the
    // enclosing statement's drops and found none, and leaked what was live.
    //
    // Put back rather than cleared, because what encloses this statement is
    // the statement to go back to.
    let enclosing_stmt_idx = ctx.body.current_stmt_idx.replace(stmt_idx);
    let result = lower_statement_with_id(ctx, stmt, stmt_idx);
    ctx.body.current_stmt_idx = enclosing_stmt_idx;
    result
}

fn lower_statement_with_id<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let init_expr = let_stmt.value;
            let value_id = lower_expression(ctx, init_expr)?;
            for (name, value_id, _) in lower_destructure(ctx, &let_stmt.binding, init_expr, value_id) {
                let operand = Operand::Value(value_id);
                ctx.bind_var(&name, operand);
                // A wrapped container keeps the shape it was written as; see
                // `LowerBody::wrapped_shapes`.
                if let_stmt.binding.as_name().is_some() {
                    if let Some(shape) = super::expr::container_shape(ctx, init_expr) {
                        ctx.record_wrapped_shape(operand, shape);
                    }
                }
                // Record binding operand for drop schedule.
                ctx.record_binding_operand(operand);
            }
            Ok(())
        }
        Statement::Var(var_stmt) => {
            lower_var(ctx, var_stmt, true)?;
            Ok(())
        }
        Statement::Set(set_stmt) => {
            lower_set(ctx, set_stmt)
        }
        Statement::Ret(ret_stmt) => {
            let value = if let Some(expr) = ret_stmt.value {
                let value_id = lower_expression(ctx, expr)?;
                // A function whose return type is unit returns nothing, even
                // where the source wrote something for it to return. `()` is
                // that type, so `ret ()` names a value of no size and a
                // signature with nowhere to put it -- the expression is
                // lowered for whatever it does, and then left behind.
                if matches!(ctx.return_type, Some(IrType::Unit) | None) {
                    None
                } else {
                    Some(Operand::Value(value_id))
                }
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
            // Typechecker validates break is inside a loop (F050).
            let loop_ctx = ctx.body.loop_stack.last()
                .unwrap_or_else(|| panic!("break outside loop - typechecker should catch this"));
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
            // Typechecker validates continue is inside a loop (F051).
            let loop_ctx = ctx.body.loop_stack.last()
                .unwrap_or_else(|| panic!("continue outside loop - typechecker should catch this"));
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
            panic!("nested function definitions not allowed in datalove syntax");
        }
        Statement::Require(_) | Statement::Import(_) => {
            // These are module-level, not in function bodies.
            Ok(())
        }
        Statement::DebugLog(stmt) => {
            // Use lower_expression_for_ref to handle field projections with GetFieldRef.
            // This borrows the value instead of copying, avoiding shallow-copy issues.
            let operand = lower_expression_for_ref(ctx, stmt.value)?;
            // If the operand is a ref, dereference it for reading.
            let operand = ctx.deref_if_ref(operand);
            ctx.emit(Instruction::DebugLog { operand });
            // Drop any expression temporaries (e.g., string literals created for debuglog).
            ctx.emit_expr_temp_drops();
            Ok(())
        }
        Statement::TypeAlias(_) => {
            // Type aliases are resolved at typecheck time; nothing to lower.
            Ok(())
        }
        Statement::Const(const_stmt) => {
            // Const bindings in function bodies are lowered as let bindings.
            // The const inlining pass runs later to replace with literal values.
            let name = const_stmt.name.text(ctx.db).to_string();
            let init_expr = const_stmt.value;
            let value_id = lower_expression(ctx, init_expr)?;
            let operand = Operand::Value(value_id);
            ctx.const_let_names.insert(name.clone());
            ctx.bind_var(&name, operand);
            ctx.record_binding_operand(operand);
            ctx.body.const_values.push((name, value_id));
            Ok(())
        }
        Statement::Match(match_stmt) => {
            lower_match(ctx, match_stmt, stmt_idx)
        }
        Statement::NativeFun(_) => {
            // Native function declarations have no body to lower.
            Ok(())
        }
        Statement::ExprStatement(stmt) => {
            // Lower the function call expression and discard the unit result.
            lower_expression(ctx, stmt.expr)?;
            ctx.emit_expr_temp_drops();
            Ok(())
        }
        Statement::ParseError(_) => {
            panic!("parse error node reached lowering - callers should check for parse errors before lowering")
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
                .unwrap_or_else(|| panic!("Result if-binding without else binding - typechecker F046 should catch this"));
            lower_if_result(ctx, if_stmt, condition, ok_type, binding_name, err_binding, stmt_idx)
        }

        // Boolean condition (no binding).
        (IrType::Bool, None) => {
            lower_if_bool(ctx, if_stmt, condition, stmt_idx)
        }

        // Invalid combinations - typechecker should catch these.
        (_, Some(_)) => {
            panic!("if-binding requires Option or Result type, got {:?} - typechecker should catch this", cond_type);
        }
        (_, None) => {
            panic!("if condition must be Bool without binding, got {:?} - typechecker should catch this", cond_type);
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
    ctx.enter_var_scope();
    for stmt in if_stmt.then_body.iter() {
        lower_statement(ctx, stmt)?;
    }
    ctx.exit_var_scope();
    // Only emit Goto if branch didn't terminate early.
    let then_terminated = ctx.is_unreachable();
    if !then_terminated {
        ctx.emit_then_branch_drops(stmt_idx);
        ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
    }

    // Lower else branch.
    ctx.start_block(else_block);
    ctx.enter_var_scope();
    if let Some(else_body) = &if_stmt.else_body {
        for stmt in else_body.iter() {
            lower_statement(ctx, stmt)?;
        }
    }
    ctx.exit_var_scope();
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
    ctx.enter_var_scope();
    ctx.bind_var(binding_str, Operand::Value(inner_dest));

    // Register binding with drop schedule system.
    ctx.record_binding_operand(Operand::Value(inner_dest));

    for stmt in &if_stmt.then_body {
        lower_statement(ctx, stmt)?;
    }

    ctx.exit_var_scope();

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

    ctx.enter_var_scope();
    if let Some(else_body) = &if_stmt.else_body {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }
    ctx.exit_var_scope();

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
    ctx.enter_var_scope();
    ctx.bind_var(ok_binding_str, Operand::Value(ok_dest));

    // Register binding with drop schedule system.
    ctx.record_binding_operand(Operand::Value(ok_dest));

    for stmt in &if_stmt.then_body {
        lower_statement(ctx, stmt)?;
    }

    ctx.exit_var_scope();

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
    ctx.enter_var_scope();
    ctx.bind_var(err_binding_str, Operand::Value(err_dest));

    // Register binding with drop schedule system.
    ctx.record_binding_operand(Operand::Value(err_dest));

    if let Some(else_body) = &if_stmt.else_body {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }

    ctx.exit_var_scope();

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
    let _body_block = if let Some(cond_expr) = condition {
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
    ctx.body.loop_stack.push(LoopLowerContext {
        header: loop_header,
        exit: loop_exit,
    });

    // Lower loop body.
    ctx.enter_var_scope();
    for stmt in loop_stmt.body.iter() {
        lower_statement(ctx, stmt)?;
    }
    ctx.exit_var_scope();

    // Only emit loop-back if the body didn't terminate early (via break/return).
    // If the body terminated, the current block is unreachable and we shouldn't
    // emit drops that reference values defined only in the loop body.
    if !ctx.is_unreachable() {
        // Emit drops before looping back.
        ctx.emit_loop_body_end_drops(stmt_idx);

        // Loop back to header.
        ctx.finish_block(Terminator::Goto {
            target: loop_header,
            args: Vec::new(),
        });
    }

    // Pop loop context.
    ctx.body.loop_stack.pop();

    // Start loop exit block.
    ctx.start_block(loop_exit);

    Ok(())
}

/// Lower a set statement.
fn lower_set<'db>(
    ctx: &mut LowerCtx<'db>,
    set_stmt: &ast::StmtSet<'db>,
) -> Result<(), LowerError> {
    let place = &set_stmt.target;

    if place.steps.is_empty() {
        lower_set_simple(ctx, place, set_stmt.value)
    } else if place_contains_index(place) {
        lower_set_indexed(ctx, place, set_stmt.value)
    } else {
        lower_set_field_path(ctx, place, set_stmt.value)
    }
}

/// Lower simple variable assignment: `set x = v`.
fn lower_set_simple<'db>(
    ctx: &mut LowerCtx<'db>,
    place: &ast::Place<'db>,
    value_expr: ExprFun<'db>,
) -> Result<(), LowerError> {
    let root_name_str = place.root.text(ctx.db).to_string();

    if is_self_assignment(ctx, &root_name_str, value_expr) {
        return Ok(());
    }
    let value_id = lower_expression(ctx, value_expr)?;
    match ctx.lookup_var(&root_name_str) {
        Some(Operand::Slot(slot)) => {
            let is_copy = ctx.slot_type(slot).expect("slot must have a type").is_copy();
            if !is_copy {
                let operand = Operand::Slot(slot);
                if ctx.is_operand_tracked(operand) {
                    ctx.emit(Instruction::DropTracked { operand });
                } else {
                    ctx.emit(Instruction::Drop { operand });
                }
            }
            if is_copy {
                ctx.emit_slot_store_copy(SlotDest::Local(slot), Operand::Value(value_id));
            } else {
                ctx.emit_slot_store_move(SlotDest::Local(slot), Operand::Value(value_id));
            }
            Ok(())
        }
        Some(Operand::Param(param)) => {
            let mode = ctx.param_mode(param);
            if mode == Some(ParamMode::Mut) || mode == Some(ParamMode::Out) {
                ctx.emit_param_store(param, Operand::Value(value_id));
                Ok(())
            } else {
                panic!("assignment to immutable param '{}' - typechecker should catch this", root_name_str)
            }
        }
        _ => panic!("assignment to immutable variable '{}' - typechecker should catch this", root_name_str),
    }
}

/// Lower set with index steps: `set a[i] = v`, `set a[i]? = v`, etc.
fn lower_set_indexed<'db>(
    ctx: &mut LowerCtx<'db>,
    place: &ast::Place<'db>,
    value_expr: ExprFun<'db>,
) -> Result<(), LowerError> {
    let last_step = place.steps.last().unwrap();
    if let ast::PlaceStep::Index(idx) = last_step {
        if idx.error_mode.is_none() {
            // Bare index (upsert) for maps.
            let base_op = lower_place_steps_to_operand(ctx, place, place.steps.len() - 1)?;
            let key_id = lower_expression(ctx, idx.index)?;
            let value_id = lower_expression(ctx, value_expr)?;
            ctx.emit(Instruction::MapUpsert {
                map: base_op,
                key: Operand::Value(key_id),
                value: Operand::Value(value_id),
            });
        } else {
            // Fallible index: set a[i]? = v / set a[i]! = v.
            let error_mode = idx.error_mode.unwrap();
            let base_op = lower_place_steps_to_operand(ctx, place, place.steps.len() - 1)?;
            let base_type = operand_type(ctx, &base_op);
            let key_op = lower_operand(ctx, idx.index)?;
            emit_fallible_index_check(ctx, base_op, &base_type, key_op, error_mode)?;
            let value_id = lower_expression(ctx, value_expr)?;
            match &base_type {
                IrType::Map(_, _) => {
                    ctx.emit(Instruction::MapSetValue {
                        map: base_op,
                        key: key_op,
                        value: Operand::Value(value_id),
                    });
                }
                IrType::Tensor(_, _) => {
                    ctx.emit(Instruction::TensorSet {
                        tensor: base_op,
                        index: key_op,
                        value: Operand::Value(value_id),
                    });
                }
                _ => {
                    ctx.emit(Instruction::ListSet {
                        list: base_op,
                        index: key_op,
                        value: Operand::Value(value_id),
                    });
                }
            }
        }
    } else {
        // Last step is a field, but chain contains index -- use ref-based approach.
        let ref_op = lower_place_steps_to_operand(ctx, place, place.steps.len())?;
        let value_id = lower_expression(ctx, value_expr)?;
        ctx.emit(Instruction::RefStore {
            dest: ref_op,
            value: Operand::Value(value_id),
        });
    }
    Ok(())
}

/// Lower set with pure field projections: `set x.a.b = v`.
fn lower_set_field_path<'db>(
    ctx: &mut LowerCtx<'db>,
    place: &ast::Place<'db>,
    value_expr: ExprFun<'db>,
) -> Result<(), LowerError> {
    let root_name_str = place.root.text(ctx.db).to_string();
    let field_path = collect_field_path_from_place(ctx, place)?;
    let value_id = lower_expression(ctx, value_expr)?;
    match ctx.lookup_var(&root_name_str) {
        Some(Operand::Slot(slot)) => {
            ctx.emit_set_field(SlotDest::Local(slot), field_path, Operand::Value(value_id));
            Ok(())
        }
        Some(Operand::Param(param)) => {
            let mode = ctx.param_mode(param);
            if mode == Some(ParamMode::Mut) || mode == Some(ParamMode::Out) {
                ctx.emit_param_set_field(param, field_path, Operand::Value(value_id));
                Ok(())
            } else {
                panic!("assignment to field of immutable param '{}' - typechecker should catch this", root_name_str)
            }
        }
        _ => panic!("assignment to field of immutable variable '{}' - typechecker should catch this", root_name_str),
    }
}

/// Emit the early return block for an out-of-bounds index.
///
/// Must be called with the early return block already started. Emits the
/// appropriate None or Err value and terminates the block with a return.
/// Emit the validity check and early-return scaffolding for fallible index operations.
///
/// Emits ListBoundsCheck or MapContainsKey based on collection type,
/// branches on the result, emits an early-return block, and starts the continue block.
pub(crate) fn emit_fallible_index_check(
    ctx: &mut LowerCtx,
    collection_op: Operand,
    collection_type: &IrType,
    key_op: Operand,
    error_mode: ast::IndexErrorMode,
) -> Result<(), LowerError> {
    let collection_op = super::expr::open_container(ctx, collection_op, collection_type);
    let is_valid = ctx.fresh_value(IrType::Bool);
    match collection_type {
        IrType::Map(_, _) => {
            ctx.emit(Instruction::MapContainsKey {
                is_valid,
                map: collection_op,
                key: key_op,
            });
        }
        IrType::Tensor(_, _) => {
            ctx.emit(Instruction::TensorBoundsCheck {
                is_valid,
                tensor: collection_op,
                index: key_op,
            });
        }
        _ => {
            ctx.emit(Instruction::ListBoundsCheck {
                is_valid,
                list: collection_op,
                index: key_op,
            });
        }
    }

    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_valid),
        then_block: continue_block,
        then_args: Vec::new(),
        else_block: early_return_block,
        else_args: Vec::new(),
    });

    ctx.start_block(early_return_block);
    match error_mode {
        ast::IndexErrorMode::Option => ctx.emit_early_return_none(true),
        ast::IndexErrorMode::Result => ctx.emit_early_return_err_message("index out of bounds", true),
    }

    ctx.start_block(continue_block);
    Ok(())
}

/// Emit a reference to a collection element (list element or map value).
///
/// Returns the ValueId of the reference.
pub(crate) fn emit_collection_element_ref(
    ctx: &mut LowerCtx,
    collection_op: Operand,
    collection_type: &IrType,
    key_op: Operand,
) -> ValueId {
    let collection_op = super::expr::open_container(ctx, collection_op, collection_type);
    match collection_type {
        IrType::Map(_, v) => {
            let value_type = v.as_ref().clone();
            let dest = ctx.fresh_value(IrType::Ref(Box::new(value_type)));
            ctx.emit(Instruction::MapValueRef {
                dest,
                map: collection_op,
                key: key_op,
            });
            dest
        }
        IrType::Tensor(elem, rank) => {
            let inner_type = if *rank > 1 {
                IrType::Tensor(elem.clone(), rank - 1)
            } else {
                elem.as_ref().clone()
            };
            let dest = ctx.fresh_value(IrType::Ref(Box::new(inner_type)));
            ctx.emit(Instruction::TensorIndexRef {
                dest,
                tensor: collection_op,
                index: key_op,
            });
            dest
        }
        _ => {
            let elem_type = match collection_type {
                IrType::List(e) => e.as_ref().clone(),
                _ => panic!("index on non-list/map/tensor type {:?}", collection_type),
            };
            let dest = ctx.fresh_value(IrType::Ref(Box::new(elem_type)));
            ctx.emit(Instruction::ListElementRef {
                dest,
                list: collection_op,
                index: key_op,
            });
            dest
        }
    }
}

/// Lower place steps to an operand referencing a location within the place.
///
/// Processes `step_count` steps from the place, returning the operand for
/// the resulting location. For the root, returns the slot/param operand.
/// For field/index steps, emits GetFieldRef/ListElementRef and returns ValueRef.
fn lower_place_steps_to_operand<'db>(
    ctx: &mut LowerCtx<'db>,
    place: &ast::Place<'db>,
    step_count: usize,
) -> Result<Operand, LowerError> {
    let root_name_str = place.root.text(ctx.db).to_string();
    let mut current_op = ctx.lookup_var(&root_name_str)
        .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", root_name_str));

    for step in &place.steps[..step_count] {
        match step {
            ast::PlaceStep::Field(field) => {
                let base_type = operand_type(ctx, &current_op);
                let field_index = match field {
                    ast::FieldSelector::Index(idx) => *idx,
                    ast::FieldSelector::Name(name) => {
                        let name_str = name.text(ctx.db);
                        let IrType::Struct(fields) = &base_type else {
                            panic!("named field on non-struct type {:?} - typechecker should catch this", base_type);
                        };
                        fields.iter().position(|(n, _)| n == name_str)
                            .unwrap_or_else(|| panic!("field '{}' not found in struct - typechecker should catch this", name_str)) as u32
                    }
                };
                let field_type = field_type_from_base(&base_type, field_index);
                let dest = ctx.fresh_value(IrType::Ref(Box::new(field_type)));
                ctx.emit(Instruction::GetFieldRef {
                    dest,
                    src: current_op,
                    field_index,
                });
                current_op = Operand::ValueRef(dest);
            }
            ast::PlaceStep::Index(idx) => {
                let error_mode = idx.error_mode
                    .expect("bare index (upsert) cannot appear in intermediate set target position");
                let base_type = operand_type(ctx, &current_op);
                let key_op = lower_operand(ctx, idx.index)?;
                emit_fallible_index_check(ctx, current_op, &base_type, key_op, error_mode)?;
                let dest = emit_collection_element_ref(ctx, current_op, &base_type, key_op);
                current_op = Operand::ValueRef(dest);
            }
        }
    }

    Ok(current_op)
}

/// Check if a place contains an index step.
fn place_contains_index(place: &ast::Place) -> bool {
    place.steps.iter().any(|s| matches!(s, ast::PlaceStep::Index(_)))
}

/// Collect the field path from a place with only field steps.
///
/// Returns the field indices from root to target.
pub(super) fn collect_field_path_from_place<'db>(
    ctx: &LowerCtx<'db>,
    place: &ast::Place<'db>,
) -> Result<Vec<u32>, LowerError> {
    let root_name_str = place.root.text(ctx.db).to_string();
    let root_type = ctx.var_type_by_name(&root_name_str)
        .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", root_name_str));

    let mut path = Vec::new();
    let mut current_type = root_type.clone();

    for step in &place.steps {
        let ast::PlaceStep::Field(selector) = step else {
            panic!("collect_field_path_from_place called on place with index step");
        };
        match selector {
            ast::FieldSelector::Index(idx) => {
                let IrType::Tuple(fields) = &current_type else {
                    panic!("tuple index on non-tuple type {:?} - typechecker should catch this", current_type);
                };
                if (*idx as usize) >= fields.len() {
                    panic!("tuple index {} out of bounds (tuple has {} fields) - typechecker should catch this", idx, fields.len());
                }
                current_type = fields[*idx as usize].clone();
                path.push(*idx);
            }
            ast::FieldSelector::Name(name) => {
                let name_str = name.text(ctx.db);
                let IrType::Struct(fields) = &current_type else {
                    panic!("field access on non-struct type {:?} - typechecker should catch this", current_type);
                };
                let field_idx = fields.iter()
                    .position(|(n, _)| n == name_str)
                    .unwrap_or_else(|| panic!("field '{}' not found in struct - typechecker should catch this", name_str));
                current_type = fields[field_idx].1.clone();
                path.push(field_idx as u32);
            }
        }
    }

    Ok(path)
}

/// Lower a match statement.
///
/// Compiles enum match to a switch on the discriminant:
/// 1. Lower input expression
/// 2. Extract discriminant
/// 3. Emit a single Switch terminator dispatching to arm blocks
/// 4. In each arm: extract payload (for term cases), lower body, emit drops
pub fn lower_match<'db>(
    ctx: &mut LowerCtx<'db>,
    match_stmt: &ast::StmtMatch<'db>,
    stmt_idx: usize,
) -> Result<(), LowerError> {
    // Lower the input expression.
    let input_id = lower_expression(ctx, match_stmt.input)?;
    let input_type = ctx.expr_type(match_stmt.input);

    let variants = match &input_type {
        IrType::Enum(v) => v,
        _ => panic!("match input must be Enum type, got: {:?}", input_type),
    };

    // Extract discriminant (borrows input, does not consume).
    let disc_id = ctx.fresh_value(IrType::U32);
    ctx.emit(Instruction::EnumDiscriminant {
        dest: disc_id,
        src: Operand::Value(input_id),
    });

    let merge_block = ctx.fresh_block();

    // Build case arm blocks.
    let num_cases = match_stmt.cases.len();
    let has_default = match_stmt.default_body.is_some();

    // Pre-allocate blocks for each arm body.
    let arm_blocks: Vec<_> = (0..num_cases).map(|_| ctx.fresh_block()).collect();
    let default_block = if has_default { Some(ctx.fresh_block()) } else { None };
    let fallthrough = default_block.unwrap_or(merge_block);

    // Build switch cases: (variant_index, arm_block).
    let mut switch_cases = Vec::new();
    for (i, case) in match_stmt.cases.iter().enumerate() {
        let name_str = match &case.kind {
            ast::MatchCaseKind::Atom { name } => name.as_str(ctx.db),
            ast::MatchCaseKind::Term { name, .. } => name.as_str(ctx.db),
        };

        let variant_index = variants.iter()
            .position(|(n, _)| n == name_str)
            .unwrap_or_else(|| panic!("match case '{}' not found in enum", name_str))
            as u32;

        switch_cases.push((variant_index, arm_blocks[i]));
    }

    // Emit a single switch terminator, remembering where it lands so that its
    // default can be aimed somewhere else once the arms are known.
    let switch_block_index = ctx.body.blocks.len();
    ctx.finish_block(Terminator::Switch {
        discriminant: Operand::Value(disc_id),
        cases: switch_cases,
        default: fallthrough,
    });

    // Whether every way out of the match left rather than reaching the end of
    // it, which decides if there is anything after the match to build.
    let mut all_arms_terminated = true;

    // Emit arm body blocks.
    for (i, case) in match_stmt.cases.iter().enumerate() {
        let name_str = match &case.kind {
            ast::MatchCaseKind::Atom { name } => name.as_str(ctx.db),
            ast::MatchCaseKind::Term { name, .. } => name.as_str(ctx.db),
        };

        let variant_index = variants.iter()
            .position(|(n, _)| n == name_str)
            .unwrap_or_else(|| panic!("match case '{}' not found in enum", name_str))
            as u32;

        ctx.start_block(arm_blocks[i]);
        ctx.enter_var_scope();

        // For term cases, extract payload and bind variable.
        if let ast::MatchCaseKind::Term { name: _, binding } = &case.kind {
            let payload_type = variants[variant_index as usize].1.as_ref()
                .unwrap_or_else(|| panic!("term case but variant has no payload type"))
                .clone();
            let payload_dest = ctx.fresh_value(payload_type);
            ctx.emit(Instruction::EnumPayload {
                dest: payload_dest,
                src: Operand::Value(input_id),
                variant_index,
            });
            let binding_str = binding.text(ctx.db);
            ctx.bind_var(binding_str, Operand::Value(payload_dest));
            ctx.record_binding_operand(Operand::Value(payload_dest));
        } else {
            // Atom case: drop the input (no payload to extract).
            ctx.emit(Instruction::Drop {
                operand: Operand::Value(input_id),
            });
        }

        // Lower arm body.
        for stmt in &case.body {
            lower_statement(ctx, stmt)?;
        }

        ctx.exit_var_scope();

        // Emit match arm drops and goto merge.
        let arm_terminated = ctx.is_unreachable();
        all_arms_terminated &= arm_terminated;
        if !arm_terminated {
            ctx.emit_match_arm_drops(stmt_idx, i);
            ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
        }
    }

    // Handle default arm.
    if let Some(default_body) = &match_stmt.default_body {
        ctx.start_block(default_block.unwrap());

        // Drop the input in default arm.
        ctx.emit(Instruction::Drop {
            operand: Operand::Value(input_id),
        });

        ctx.enter_var_scope();
        for stmt in default_body {
            lower_statement(ctx, stmt)?;
        }
        ctx.exit_var_scope();

        let default_terminated = ctx.is_unreachable();
        all_arms_terminated &= default_terminated;
        if !default_terminated {
            ctx.emit_match_arm_drops(stmt_idx, num_cases);
            ctx.finish_block(Terminator::Goto { target: merge_block, args: Vec::new() });
        }
    }

    // Start the merge block. If every arm left, nothing reaches it, the same
    // way an `if` whose branches both return reaches nothing after it -- and
    // then it is never built, so nothing may be left pointing at it.
    //
    // Without a `case default` the switch's own default points there, for a
    // discriminant none of its cases name. The match covers every variant,
    // which the typechecker sees to, so that edge is dead; it is aimed at an
    // arm instead. Left alone it named a block that did not exist, which
    // renumbering turned into the entry block, and cranelift refused to verify
    // a function that branched to its own entry.
    if all_arms_terminated {
        if !has_default {
            if let Some(&first_arm) = arm_blocks.first() {
                let switch = &mut ctx.body.blocks[switch_block_index].terminator;
                if let Terminator::Switch { default, .. } = switch {
                    *default = first_arm;
                }
            }
        }
        ctx.start_unreachable_block(merge_block);
    } else {
        ctx.start_block(merge_block);
    }
    Ok(())
}

