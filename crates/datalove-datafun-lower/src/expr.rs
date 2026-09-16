//! Expression lowering.
//!
//! Transforms AST expressions into IR values and instructions. Handles literals,
//! operators, function calls, tuples, records, and control flow expressions.

use datalove_datafun_ast::ast::{self, ExprFun, ExprFunKind, ParamMode, ExprKey};
use datalove_datafun_ir::{
    IrType, Operand, ValueId, BinOp, UnaryOp, Instruction, ConstValue, Terminator, TypeRef,
    SlotDest,
};
use super::context::LowerCtx;
use super::literal::{parse_int_const, parse_hex_const, parse_float_const, try_parse_negated_int_const};
use super::LowerError;

/// Look up a variable by name, checking const bindings first.
///
/// For const bindings, emits a Const instruction and returns the new value
/// with `is_fresh_const = true`. For regular variables, returns the
/// Slot/Param/Value operand with `is_fresh_const = false`.
///
/// Callers that need drop tracking should call `record_expr_temp` only
/// when `is_fresh_const` is true, since regular `let` bindings bound to
/// `Operand::Value` are already managed by the drop schedule.
fn lower_var_operand(ctx: &mut LowerCtx, name: &str, consuming: bool) -> Result<(Operand, bool), LowerError> {
    if let Some((const_type, const_value)) = ctx.lookup_const(name) {
        let const_type = const_type.clone();
        let const_value = const_value.clone();
        let dest = ctx.fresh_value(const_type);
        ctx.emit(Instruction::Const {
            dest,
            value: const_value,
        });
        return Ok((Operand::Value(dest), true));
    }
    // A name the typechecker accepted but nothing here defines is a module-level
    // const that has not been evaluated yet. The caller lowers this function
    // again once it has.
    let op = ctx.lookup_var(name)
        .ok_or_else(|| LowerError::BindingNotAvailable(name.to_string()))?;

    // A const written in this body is evaluated after this body is lowered, so
    // its value is not available here the way a module-level const's is. What
    // this can do is give the reference a value of its own, by cloning, and
    // record it under the const's name, so that the pass which replaces a
    // const's definition with its literal replaces this clone too. Where that
    // pass runs, each reference reaches the backend as its own literal; where
    // it does not, the clone stands and is still a value of its own. Either
    // way reading a const does not consume it.
    //
    // A copy type needs none of this: reading one never consumed it.
    if ctx.const_let_names.contains(name) {
        let ty = ctx.operand_type(op.clone())
            .ok_or_else(|| LowerError::BindingNotAvailable(name.to_string()))?
            .clone();
        if !ty.is_copy() {
            let dest = ctx.fresh_value(ty);
            ctx.emit(Instruction::Clone { dest, src: op });
            ctx.body.const_values.push((name.to_string(), dest));
            return Ok((Operand::Value(dest), true));
        }
    }

    // A const parameter is a constant, so a read that would consume it takes a
    // copy instead, the way a read of any other const does. A borrow takes
    // none: it never consumed anything.
    if consuming && ctx.const_param_names.contains(name) {
        let ty = ctx.operand_type(op.clone())
            .ok_or_else(|| LowerError::BindingNotAvailable(name.to_string()))?
            .clone();
        if !ty.is_copy() {
            let dest = ctx.fresh_value(ty);
            ctx.emit(Instruction::Clone { dest, src: op });
            return Ok((Operand::Value(dest), false));
        }
    }

    Ok((op, false))
}

/// Lower an operand for borrowing contexts (binop, unaryop).
///
/// Returns an Operand directly:
/// - For zero-step Places (variables): returns Operand::Slot/Value/Param
/// - For compound expressions: evaluates and returns Operand::Value(result)
pub fn lower_operand<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<Operand, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Place(ref place) if place.steps.is_empty() => {
            let name_str = place.root.text(ctx.db);
            let (operand, is_fresh_const) = lower_var_operand(ctx, name_str, false)?;
            if is_fresh_const {
                if let Operand::Value(vid) = operand {
                    let ty = ctx.body.value_types[vid.0 as usize].clone();
                    ctx.record_expr_temp(vid, ty);
                }
            }
            Ok(operand)
        }
        _ => {
            // Compound expression: lower to a value.
            let expr_type = ctx.expr_type(expr);
            let value_id = lower_expression(ctx, expr)?;
            // Record as temp for dropping after the borrowing operation completes.
            ctx.record_expr_temp(value_id, expr_type);
            Ok(Operand::Value(value_id))
        }
    }
}

/// Lower an argument for a function call, considering the parameter mode.
///
/// For ref/mut/out params with field projection args, emits GetFieldRef instead
/// of GetField to pass a reference to the field without copying.
///
/// For 'out' mode params, the callee expects uninitialized memory, so we emit
/// a Drop for the existing value before passing the reference.
///
/// For 'in' mode params, the function CONSUMES the argument (takes ownership).
/// So we don't record temps - the value is transferred to the callee.
fn lower_call_arg<'db>(
    ctx: &mut LowerCtx<'db>,
    arg: ExprFun<'db>,
    mode: ParamMode,
    arg_type: Option<&IrType>,
) -> Result<Operand, LowerError> {
    // Check if this is a ref context AND the arg is a field projection or index.
    if matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out) {
        // Index expression: a[i]? or a[i]!
        if let Some(operand) = try_lower_arg_index_as_ref(ctx, arg, mode, arg_type)? {
            return Ok(operand);
        }
        // Place with steps: lower as ref.
        if let ExprFunKind::Place(ref place) = arg.expr(ctx.db) {
            if !place.steps.is_empty() {
                let operand = lower_place_as_ref(ctx, place)?;
                if mode == ParamMode::Out {
                    if let Operand::Value(ref_value) = operand {
                        if let Some(ty) = arg_type {
                            if !ty.is_copy() {
                                ctx.emit(Instruction::DropViaRef { ref_value });
                            }
                        }
                    }
                }
                return Ok(ctx.deref_if_ref(operand));
            }
        }
        if let ExprFunKind::FieldProj(proj) = arg.expr(ctx.db) {
            let operand = lower_field_proj_as_ref(ctx, arg, proj)?;
            // For out params, destroy existing field value before call.
            // Use DropViaRef since we have a reference, not the value itself.
            if mode == ParamMode::Out {
                if let Operand::Value(ref_value) = operand {
                    if let Some(ty) = arg_type {
                        if !ty.is_copy() {
                            ctx.emit(Instruction::DropViaRef { ref_value });
                        }
                    }
                }
            }
            // Convert to ValueRef for proper dereferencing at use site.
            return Ok(ctx.deref_if_ref(operand));
        }
        // For ref/mut/out modes, use lower_operand which records temps.
        // The caller retains ownership and must drop after the call.
        let operand = lower_operand(ctx, arg)?;
        // For out params, destroy existing value before call.
        // Use precise Drop for values/temps, DropTracked for tracked bindings (slots, Out params).
        if mode == ParamMode::Out {
            if let Some(ty) = arg_type {
                ctx.emit_drop_for_operand(&operand, ty);
            }
        }
        return Ok(operand);
    }

    // For 'in' mode: function consumes the argument, so don't record temps.
    match arg.expr(ctx.db) {
        ExprFunKind::Place(ref place) if place.steps.is_empty() => {
            let name_str = place.root.text(ctx.db);
            let (operand, _) = lower_var_operand(ctx, name_str, true)?;
            let is_external = matches!(
                operand,
                Operand::ExternalValue { .. } | Operand::ExternalSlot { .. },
            );
            if ctx.is_adapt_site(arg) || is_external {
                // The callee consumes the copy, not the binding.
                return Ok(Operand::Value(emit_owned_copy(ctx, arg, operand)));
            }
            Ok(operand)
        }
        _ => {
            // Compound expression: lower to a value without recording temps.
            // The function consumes this value, so no drop needed by caller.
            let value_id = lower_expression(ctx, arg)?;
            Ok(Operand::Value(value_id))
        }
    }
}

/// Try to lower a function argument as a list index reference.
///
/// Returns `Some(operand)` if the argument is `a[i]?` or `a[i]!`, emitting
/// ListBoundsCheck + ListElementRef. Returns `None` if not an index pattern.
fn try_lower_arg_index_as_ref<'db>(
    ctx: &mut LowerCtx<'db>,
    arg: ExprFun<'db>,
    mode: ParamMode,
    arg_type: Option<&IrType>,
) -> Result<Option<Operand>, LowerError> {
    let operand = match arg.expr(ctx.db) {
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Option)?
            } else {
                return Ok(None);
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)?
            } else {
                return Ok(None);
            }
        }
        ExprFunKind::Place(ref place) => {
            lower_place_as_ref(ctx, place)?
        }
        _ => return Ok(None),
    };

    // For out params, destroy existing element value before call.
    if mode == ParamMode::Out {
        if let Operand::Value(ref_value) = operand {
            if let Some(ty) = arg_type {
                if !ty.is_copy() {
                    ctx.emit(Instruction::DropViaRef { ref_value });
                }
            }
        }
    }

    Ok(Some(ctx.deref_if_ref(operand)))
}

/// Lower `a[i]?` / `m[k]?` as a reference (pointer to element/value).
///
/// Emits validity check + early-return scaffolding, then ListElementRef or MapValueRef.
/// Returns a reference operand suitable for passing to ref/mut/out params.
fn lower_index_as_ref<'db>(
    ctx: &mut LowerCtx<'db>,
    index_expr: &ast::ExprIndex<'db>,
    error_mode: ast::IndexErrorMode,
) -> Result<Operand, LowerError> {
    let base_op = lower_operand(ctx, index_expr.base)?;
    let key_op = lower_operand(ctx, index_expr.index)?;
    let base_type = ctx.expr_type(index_expr.base);
    super::stmt::emit_fallible_index_check(ctx, base_op, &base_type, key_op, error_mode)?;
    let dest = super::stmt::emit_collection_element_ref(ctx, base_op, &base_type, key_op);
    Ok(Operand::Value(dest))
}

/// Lower a field projection as a reference (pointer to field).
///
/// Emits GetFieldRef instead of GetField, returning a pointer to the field
/// without copying. Used when passing field projections to ref/mut/out params
/// and for debuglog expressions.
pub fn lower_field_proj_as_ref<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    proj: ast::ExprFieldProj<'db>,
) -> Result<Operand, LowerError> {
    let base_type = ctx.expr_type(proj.base);

    // When the base is a Place (with or without steps), build an extended
    // Place with the field step appended and delegate to lower_place_as_ref.
    if let ExprFunKind::Place(ref place) = proj.base.expr(ctx.db) {
        let mut extended = place.clone();
        extended.steps.push(ast::PlaceStep::Field(proj.field));
        return lower_place_as_ref(ctx, &extended);
    }

    // When the base is a nested FieldProj, recurse to get a ref, then
    // apply GetFieldRef on top.
    let src = match proj.base.expr(ctx.db) {
        ExprFunKind::FieldProj(inner_proj) => {
            let base_ref = lower_field_proj_as_ref(ctx, proj.base, inner_proj)?;
            operand_as_value_ref(base_ref)
        }
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                operand_as_value_ref(lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Option)?)
            } else {
                Operand::Value(lower_expression(ctx, proj.base)?)
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                operand_as_value_ref(lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)?)
            } else {
                Operand::Value(lower_expression(ctx, proj.base)?)
            }
        }
        _ => {
            Operand::Value(lower_expression(ctx, proj.base)?)
        }
    };

    let field_index = resolve_field_index(&proj.field, &base_type, ctx.db)?;
    let field_type = ctx.expr_type(expr);
    let ref_type = IrType::Ref(Box::new(field_type));
    let dest = ctx.fresh_value(ref_type);

    ctx.emit(Instruction::GetFieldRef {
        dest,
        src,
        field_index,
    });
    Ok(Operand::Value(dest))
}

/// Convert a Value operand to a ValueRef operand.
///
/// Ref-returning functions return `Operand::Value(ref_value_id)` where the
/// value has Ref type. To chain GetFieldRef, we need `Operand::ValueRef`.
fn operand_as_value_ref(op: Operand) -> Operand {
    match op {
        Operand::Value(v) => Operand::ValueRef(v),
        other => other,
    }
}

/// Lower an expression for reference context (borrowing).
///
/// For field projections, emits GetFieldRef to borrow the field without copying.
/// This avoids the shallow-copy problem with move types containing pointers.
/// For names (variables/params), returns the operand directly to borrow without moving.
/// For other expressions, uses standard lower_expression.
pub fn lower_expression_for_ref<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<Operand, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::FieldProj(proj) => {
            // Field projections use GetFieldRef to borrow without copying.
            lower_field_proj_as_ref(ctx, expr, proj)
        }
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                // a[i]? — borrow list element by reference.
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Option)
            } else {
                let expr_type = ctx.expr_type(expr);
                let value_id = lower_expression(ctx, expr)?;
                ctx.record_expr_temp(value_id, expr_type);
                Ok(Operand::Value(value_id))
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                // a[i]! — borrow list element by reference.
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)
            } else {
                let expr_type = ctx.expr_type(expr);
                let value_id = lower_expression(ctx, expr)?;
                ctx.record_expr_temp(value_id, expr_type);
                Ok(Operand::Value(value_id))
            }
        }
        ExprFunKind::Place(ref place) if place.steps.is_empty() => {
            let name_str = place.root.text(ctx.db);
            let (operand, is_fresh_const) = lower_var_operand(ctx, name_str, false)?;
            if is_fresh_const {
                if let Operand::Value(vid) = operand {
                    let ty = ctx.body.value_types[vid.0 as usize].clone();
                    ctx.record_expr_temp(vid, ty);
                }
            }
            Ok(operand)
        }
        ExprFunKind::Place(ref place) => {
            // Place expression with steps — borrow by reference.
            lower_place_as_ref(ctx, place)
        }
        _ => {
            // All other expressions: use standard lowering.
            let expr_type = ctx.expr_type(expr);
            let value_id = lower_expression(ctx, expr)?;
            ctx.record_expr_temp(value_id, expr_type);
            Ok(Operand::Value(value_id))
        }
    }
}

/// Produce an owned value from an operand without consuming it.
///
/// Emits what an explicit `@` on the expression would: a widening between
/// fixed ints, a clone for linear types. Used where auto-adapt supplies the
/// `@`, and for every consuming use of a binding an earlier unit owns.
fn emit_owned_copy<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    operand: Operand,
) -> ValueId {
    let dest_type = ctx.expr_type(expr);
    let src_type = ctx.operand_type(operand).unwrap_or_else(|| dest_type.clone());
    let dest = ctx.fresh_value(dest_type.clone());

    if src_type == dest_type {
        if src_type.is_copy() {
            ctx.emit(Instruction::Copy { dest, src: operand });
        } else {
            ctx.emit(Instruction::Clone { dest, src: operand });
        }
    } else if is_fixed_width_int(&src_type) && is_fixed_width_int(&dest_type) {
        ctx.emit(Instruction::WidenFixed { dest, src: operand });
    } else if widens_to_int(&src_type) && matches!(dest_type, IrType::Int) {
        ctx.emit(Instruction::Widen { dest, src: operand });
    } else {
        ctx.emit(Instruction::Clone { dest, src: operand });
    }

    dest
}

/// Lower an expression, returning the ValueId holding the result.
pub fn lower_expression<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ValueId, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Place(ref place) if place.steps.is_empty() => {
            let name_str = place.root.text(ctx.db);
            let (operand, _) = lower_var_operand(ctx, name_str, true)?;

            // Auto-adapt supplies the `@` here, so clone rather than consume.
            if ctx.is_adapt_site(expr) {
                return Ok(emit_owned_copy(ctx, expr, operand));
            }

            match operand {
                Operand::Value(v) | Operand::ValueRef(v) => {
                    // For SSA values, just return the existing ValueId.
                    Ok(v)
                }
                Operand::Slot(s) => {
                    // For slots, emit a load.
                    let slot_type = ctx.expr_type(expr);
                    let is_copy = slot_type.is_copy();
                    let dest = ctx.fresh_value(slot_type);
                    if is_copy {
                        ctx.emit_slot_load_copy(dest, s);
                    } else if ctx.is_operand_tracked(operand) {
                        ctx.emit(Instruction::SlotLoadMoveTracked { dest, slot: s });
                    } else {
                        ctx.emit(Instruction::SlotLoadMove { dest, slot: s });
                    }
                    Ok(dest)
                }
                Operand::Param(_) => {
                    // For params, emit Copy or Move from the param.
                    let param_type = ctx.expr_type(expr);
                    let dest = ctx.fresh_value(param_type.clone());
                    if param_type.is_copy() {
                        ctx.emit(Instruction::Copy { dest, src: operand });
                    } else {
                        ctx.emit(Instruction::Move { dest, src: operand });
                    }
                    Ok(dest)
                }
                Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                    // A binding an earlier unit owns is copied out of, never
                    // taken: the name has to keep working at the next prompt.
                    let ext_type = ctx.expr_type(expr);
                    let dest = ctx.fresh_value(ext_type.clone());
                    if ext_type.is_copy() {
                        ctx.emit(Instruction::Copy { dest, src: operand });
                    } else {
                        ctx.emit(Instruction::Clone { dest, src: operand });
                    }
                    Ok(dest)
                }
            }
        }
        ExprFunKind::True(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit_const(dest, ConstValue::Bool(true));
            Ok(dest)
        }
        ExprFunKind::False(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit_const(dest, ConstValue::Bool(false));
            Ok(dest)
        }
        ExprFunKind::None(_) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_wrap_none(dest);
            Ok(dest)
        }
        ExprFunKind::Int(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value.text(ctx.db);
            let const_value = parse_int_const(text, &result_type)
                .unwrap_or_else(|_| panic!("invalid integer literal '{}' for type {:?} - typechecker should catch this", text, result_type));
            ctx.emit_const(dest, const_value);
            Ok(dest)
        }
        ExprFunKind::Hex(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value.text(ctx.db);
            let hex_str = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
            let const_value = parse_hex_const(hex_str, &result_type)
                .unwrap_or_else(|_| panic!("invalid hex literal '{}' for type {:?} - typechecker should catch this", text, result_type));
            ctx.emit_const(dest, const_value);
            Ok(dest)
        }
        ExprFunKind::BinOp(binop) => {
            lower_binop(ctx, expr, binop)
        }
        ExprFunKind::UnaryOp(unary) => {
            lower_unaryop(ctx, expr, unary)
        }
        ExprFunKind::FunctionCall(call) => {
            // Push a new scope for this call's pending intermediates.
            ctx.push_pending_scope();
            let expr_temp_mark = ctx.expr_temps_mark();

            // Resolve function reference using typechecker's resolved call target.
            let func_ref = ctx.resolve_call(call);

            // Get param modes and types from the resolved call target.
            let target = ctx.call_targets.and_then(|t| t.get(&ExprKey::of_call(ctx.db, call)));
            let param_modes: Vec<ParamMode> = target
                .map(|t| t.func(ctx.db).params(ctx.db).iter().map(|p| p.mode).collect())
                .unwrap_or_default();
            // A parameter whose type mentions one of the callee's type
            // parameters was erased to `data` at that position when the callee
            // was lowered, so the argument has to be converted on the way in.
            // The parameter need not be the whole type: `[T]` was lowered as
            // `[data]`, and the conversion walks into the list. Everything else
            // keeps its own type.
            let type_params: Vec<bct::text::InternedText<'db>> = target
                .map(|t| t.func(ctx.db).type_params(ctx.db).clone())
                .unwrap_or_default();
            let names_a_type_param = |hint: &datalove_datafun_ast::datalit::ast::TypeHint<'db>| {
                datalove_datafun_ir::type_hint_mentions_param(hint, &type_params)
            };
            // A borrowed parameter is not converted: the callee gets the value
            // as it stands, and a descriptor saying what it is, and never drops
            // it. Only a parameter the callee takes ownership of is erased,
            // because that one needs a slot and `data` is the shape that fits.
            let param_is_erased: Vec<bool> = target
                .map(|t| t.func(ctx.db).params(ctx.db).iter()
                    .map(|p| {
                        let borrowed = matches!(p.mode, ast::ParamMode::Ref | ast::ParamMode::Mut);
                        !borrowed && names_a_type_param(&p.type_hint)
                    })
                    .collect())
                .unwrap_or_default();
            let param_types: Vec<IrType> = target
                .map(|t| t.func(ctx.db).params(ctx.db).iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let borrowed = matches!(p.mode, ast::ParamMode::Ref | ast::ParamMode::Mut);
                        if param_is_erased.get(i).copied().unwrap_or(false) {
                            datalove_datafun_ir::erased_param_type(
                                ctx.db, &p.type_hint, &type_params, borrowed)
                        } else {
                            IrType::from_type_hint(ctx.db, &p.type_hint)
                        }
                    })
                    .collect())
                .unwrap_or_default();
            // The erased return shape, by the same rule the callee shaped it
            // with: `data` for a bare `T`, `?data` for a `?T`, and `data` for
            // a container, which is wrapped whole rather than walked.
            let erased_return_type = target
                .and_then(|t| t.func(ctx.db).return_type(ctx.db))
                .filter(|hint| names_a_type_param(hint))
                .map(|hint| datalove_datafun_ir::erased_return_type(ctx.db, &hint, &type_params));

            // Lower each arg, tracking in-mode args as pending intermediates.
            let call_args = call.args(ctx.db);
            let mut args = Vec::with_capacity(call_args.len());
            // Out parameters the callee writes in the erased shape, paired with
            // where the caller wants the value once it is moved back out.
            let mut erased_out_params: Vec<(Operand, ValueId)> = Vec::new();
            for (i, arg) in call_args.iter().enumerate() {
                let mode = param_modes.get(i).copied().unwrap_or(ParamMode::In);
                // An erased out parameter drops what was already there by
                // erasing it into the value the callee is given, which the call
                // then destroys as it does for any out parameter. So no
                // separate drop is wanted, and passing no type asks for none.
                let erased_out = mode == ParamMode::Out
                    && param_is_erased.get(i).copied().unwrap_or(false);
                let arg_type = if erased_out { None } else { param_types.get(i) };
                let mut operand = lower_call_arg(ctx, *arg, mode, arg_type)?;
                // Convert into the shape the erased callee was compiled for,
                // which is the parameter's own type with `data` at each type
                // parameter rather than `data` outright.
                //
                // An out parameter also comes back. What was there is erased
                // into the value the callee is given, so that the call destroys
                // it exactly once as it does for any out parameter, and what
                // the callee writes there is moved back out into the caller's
                // type below. That is the same thing the return value does, in
                // the other direction.
                // An argument already in the erased shape is one the caller
                // holds under a type parameter of its own, which is what a
                // generic function passing its `T` to another has. Erasing it
                // again would box the box, and the callee would find a `data`
                // where the value it was told to expect should be.
                let param_erased = param_is_erased.get(i).copied().unwrap_or(false);
                if param_erased && param_types.get(i) != Some(&operand_type(ctx, &operand)) {
                    let erased_shape = param_types[i].clone();
                    let erased = ctx.fresh_value(erased_shape);
                    if mode == ParamMode::Out {
                        erased_out_params.push((operand.clone(), erased));
                        // The destination may be holding nothing: a `var`
                        // declared without a value is what an out parameter is
                        // usually given, and there is nothing there to move
                        // across. Reading it anyway read the frame's own
                        // poison, which the interpreter refused outright and
                        // cranelift boxed and then freed.
                        ctx.emit(Instruction::EraseTracked { dest: erased, src: operand.clone() });
                    } else {
                        ctx.emit(Instruction::Erase { dest: erased, src: operand.clone() });
                    }
                    operand = Operand::Value(erased);
                }
                // Track in-mode args as pending intermediate.
                // Ref/mut/out args are tracked via lower_operand's expr_temps.
                if mode == ParamMode::In {
                    if let Operand::Value(v) = operand {
                        if let Some(arg_ty) = param_types.get(i) {
                            ctx.push_pending_intermediate(v, arg_ty);
                        }
                    }
                }
                args.push(operand);
            }

            let result_type = ctx.expr_type(expr);
            // An erased return arrives in the erased shape, and the value is
            // moved back out of it below into the type this call site expects.
            let call_result_type = erased_return_type.clone().unwrap_or_else(|| result_type.clone());
            let dest = ctx.fresh_value(call_result_type);

            // Check if this is a call to a function with comptime params.
            // If so, emit ComptimeCall with discriminant=0 as placeholder.
            // The specialization pass will compute the correct discriminant.
            let comptime_param_indices: Vec<usize> = target
                .map(|t| {
                    t.func(ctx.db).params(ctx.db)
                        .iter()
                        .enumerate()
                        .filter_map(|(i, p)| if p.is_comptime { Some(i) } else { None })
                        .collect()
                })
                .unwrap_or_default();

            if !comptime_param_indices.is_empty() {
                // Emit ComptimeCall with discriminant=0 (placeholder).
                // Specialization pass will fill in correct discriminant from const values.
                ctx.emit_comptime_call(dest, func_ref, args, 0, comptime_param_indices);
            } else {
                // What this site bound each of the callee's type parameters to,
                // in the callee's order and written over this function's own.
                // Which of them the callee wants a descriptor for is settled
                // afterwards, once every function's shapes are known.
                let type_args: Vec<datalove_datafun_ir::DescriptorShape> = target
                    .map(|t| t.type_args(ctx.db).clone())
                    .unwrap_or_default()
                    .iter()
                    .map(|ty| ctx.shape_of(ty).unwrap_or_else(|| {
                        datalove_datafun_ir::DescriptorShape::Concrete(
                            IrType::from_datalit(ctx.db, ty))
                    }))
                    .collect();
                ctx.emit_call_with_type_args(dest, func_ref, args, type_args);
            }
            // Move what the callee wrote to an erased out parameter back out
            // into the type the caller keeps there.
            for (dest_operand, written) in erased_out_params {
                let real_ty = ctx.operand_type(dest_operand.clone())
                    .ok_or_else(|| LowerError::NotImplemented(
                        "out parameter of a generic function has no type here".to_string()
                    ))?
                    .clone();
                // A reference names the type it points at, and that is what the
                // value moved back out of the erased shape has to be.
                let real_ty = match real_ty {
                    IrType::Ref(inner) => (*inner).clone(),
                    other => other,
                };
                let reified = ctx.fresh_value(real_ty.clone());
                ctx.emit(Instruction::Reify { dest: reified, src: Operand::Value(written) });
                match dest_operand {
                    Operand::Slot(slot) => {
                        // A copy type was not dropped before the call, so its
                        // slot still holds something and takes a copying store.
                        if real_ty.is_copy() {
                            ctx.emit_slot_store_copy(SlotDest::Local(slot), Operand::Value(reified));
                        } else {
                            ctx.emit_slot_store_move(SlotDest::Local(slot), Operand::Value(reified));
                        }
                    }
                    // Anything else is a reference to where the caller keeps
                    // the value: a field, or an element. Storing back through
                    // one loses the value and leaks what it replaced, so it is
                    // refused rather than written wrong. Only a whole binding
                    // takes an out parameter of a generic function for now.
                    // Anything else is a reference to where the caller keeps
                    // the value: a field, or an element. What was there was
                    // dropped before the call, so this is a write into a place
                    // holding nothing, which is what the tracked store is for.
                    // The plain one would destroy the old value a second time.
                    reference => {
                        ctx.emit(Instruction::RefStoreTracked {
                            dest: reference,
                            value: Operand::Value(reified),
                        });
                    }
                }
            }

            // Args consumed by Call/ComptimeCall; pop scope.
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            ctx.emit_expr_temp_drops_since(expr_temp_mark);

            // Move the value back out of the shape the erased callee returned,
            // unless the caller keeps it in that shape too, which is the case
            // when a generic function returns what another gave it under a
            // type parameter of its own.
            if erased_return_type.is_some() && erased_return_type != Some(result_type.clone()) {
                let unwrapped = ctx.fresh_value(result_type);
                ctx.emit(Instruction::Reify { dest: unwrapped, src: Operand::Value(dest) });
                return Ok(unwrapped);
            }
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, some_expr.payload)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_wrap_some(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ok_expr.payload)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_wrap_ok(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, er_expr.payload)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_wrap_err(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::TryOption(try_expr) => {
            lower_try_option(ctx, expr, try_expr)
        }
        ExprFunKind::TryResult(try_expr) => {
            lower_try_result(ctx, expr, try_expr)
        }
        ExprFunKind::CloneCoerce(cc_expr) => {
            lower_clone_coerce(ctx, expr, cc_expr)
        }
        ExprFunKind::FieldProj(proj) => {
            lower_field_proj(ctx, expr, proj)
        }
        ExprFunKind::Tuple(tuple) => {
            lower_tuple_elements(ctx, expr, &tuple.elements)
        }
        ExprFunKind::AnonTuple(tuple) => {
            lower_tuple_elements(ctx, expr, &tuple.elements)
        }
        ExprFunKind::List(list) => {
            // Push a new scope for this list's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            // A list built over a type parameter erases to `data`, so its own
            // type says nothing about its elements and a descriptor has to come
            // from the call site. The elements are erased too, which is what a
            // `T` is anywhere.
            let source = ctx.expr_source_type(expr);
            let source_dt = match &source {
                Some(datalove_datafun_common::Type::Datalit(dt)) => Some(dt.clone()),
                _ => None,
            };
            let built = source_dt.as_ref().and_then(|dt| ctx.build_shape(dt));
            let elem_type = match (&result_type, &source_dt, &built) {
                (IrType::List(t), _, _) => (**t).clone(),
                (_, Some(datalove_datalit::tycheck::Type::List(t)), Some(_)) => {
                    ctx.erased_element(&t.element_type)
                }
                _ => panic!("expected List type from typechecker"),
            };
            // A built collection's elements are converted on the way in, and
            // the only conversion there is takes a whole `data`. An element
            // that erases to something composite -- `(A, B)` becoming a tuple
            // of two `data`, `?T` becoming an option of one -- has no such
            // conversion, and pushing it as though it were a `data` writes the
            // wrong number of bytes. Refused rather than written wrongly.
            //
            // A collection of one of those is broken the same way when it
            // merely arrives, so this is a hole in the collections rather than
            // in building them.
            if built.is_some() && elem_type != IrType::Data {
                return Err(crate::LowerError::NotImplemented(format!(
                    "a list whose elements are {:?} cannot be built here: an element \
                     of a collection built over a type parameter has to be carried as \
                     a single `data`, and this one is not", elem_type)));
            }
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in list.elements.iter() {
                let value = lower_expression(ctx, *e)?;
                ctx.push_pending_intermediate(value, &elem_type);
                elements.push(Operand::Value(value));
            }
            let dest = ctx.fresh_value(result_type);
            match built {
                Some(shape) => ctx.emit_list_new_erased(dest, elements, shape),
                None => ctx.emit_list_new(dest, elements),
            }
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Set(set) => {
            // Push a new scope for this set's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            // A set built over a type parameter erases to `data`, so its own
            // type says nothing about its elements and a descriptor has to come
            // from the call site. See the list arm above.
            let source = ctx.expr_source_type(expr);
            let source_dt = match &source {
                Some(datalove_datafun_common::Type::Datalit(dt)) => Some(dt.clone()),
                _ => None,
            };
            let built = source_dt.as_ref().and_then(|dt| ctx.build_shape(dt));
            let elem_type = match (&result_type, &source_dt, &built) {
                (IrType::Set(t), _, _) => (**t).clone(),
                (_, Some(datalove_datalit::tycheck::Type::Set(t)), Some(_)) => {
                    ctx.erased_element(&t.element_type)
                }
                _ => panic!("expected Set type from typechecker"),
            };
            // See the list arm.
            if built.is_some() && elem_type != IrType::Data {
                return Err(crate::LowerError::NotImplemented(format!(
                    "a set whose elements are {:?} cannot be built here: an element of \
                     a collection built over a type parameter has to be carried as a \
                     single `data`, and this one is not", elem_type)));
            }
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in set.elements.iter() {
                let value = lower_expression(ctx, *e)?;
                ctx.push_pending_intermediate(value, &elem_type);
                elements.push(Operand::Value(value));
            }
            let dest = ctx.fresh_value(result_type);
            match built {
                Some(shape) => ctx.emit_set_new_erased(dest, elements, shape),
                None => ctx.emit_set_new(dest, elements),
            }
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Map(map) => {
            // Push a new scope for this map's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            // A map built over a type parameter erases the same way, and both
            // its key and its value become `data`.
            let source = ctx.expr_source_type(expr);
            let source_dt = match &source {
                Some(datalove_datafun_common::Type::Datalit(dt)) => Some(dt.clone()),
                _ => None,
            };
            let built = source_dt.as_ref().and_then(|dt| ctx.build_shape(dt));
            let (key_type, val_type) = match (&result_type, &source_dt, &built) {
                (IrType::Map(k, v), _, _) => ((**k).clone(), (**v).clone()),
                (_, Some(datalove_datalit::tycheck::Type::Map(t)), Some(_)) => (
                    ctx.erased_element(&t.key_type),
                    ctx.erased_element(&t.value_type),
                ),
                _ => panic!("expected Map type from typechecker"),
            };
            // See the list arm.
            if built.is_some() && (key_type != IrType::Data || val_type != IrType::Data) {
                return Err(crate::LowerError::NotImplemented(format!(
                    "a map from {:?} to {:?} cannot be built here: a key or value of a \
                     collection built over a type parameter has to be carried as a \
                     single `data`, and this one is not", key_type, val_type)));
            }
            // Lower each entry and track keys/values as pending intermediates.
            let mut entries = Vec::new();
            for e in map.entries.iter() {
                let k = lower_expression(ctx, e.key)?;
                ctx.push_pending_intermediate(k, &key_type);
                let v = lower_expression(ctx, e.value)?;
                ctx.push_pending_intermediate(v, &val_type);
                entries.push((Operand::Value(k), Operand::Value(v)));
            }
            let dest = ctx.fresh_value(result_type);
            match built {
                Some(shape) => ctx.emit_map_new_erased(dest, entries, shape),
                None => ctx.emit_map_new(dest, entries),
            }
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Error(err_expr) => {
            let inner_id = lower_expression(ctx, err_expr.value)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_error_from(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Data(data_expr) => {
            let inner_id = lower_expression(ctx, data_expr.value)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_data_from(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Float(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value.text(ctx.db);
            let const_value = parse_float_const(text, &result_type)
                .unwrap_or_else(|_| panic!("invalid float literal '{}' for type {:?} - typechecker should catch this", text, result_type));
            ctx.emit_const(dest, const_value);
            Ok(dest)
        }
        ExprFunKind::String(string_expr) => {
            let raw = string_expr.value.as_str(ctx.db);
            let content = super::literal::string_literal_content(raw);
            let dest = ctx.fresh_value(IrType::String);
            ctx.emit_const(dest, ConstValue::String(content.to_string()));
            Ok(dest)
        }
        ExprFunKind::AnonStruct(struct_expr) => {
            // Push a new scope for this struct's pending intermediates.
            ctx.push_pending_scope();

            // Get the result type - this is IrType::Struct with sorted fields.
            // Typechecker ensures struct literals have struct types.
            let result_type = ctx.expr_type(expr);
            let IrType::Struct(fields) = &result_type else {
                panic!("AnonStruct with non-struct type {:?} - typechecker should catch this", result_type);
            };
            let field_types: std::collections::HashMap<String, IrType> =
                fields.iter().map(|(n, t)| (n.clone(), t.clone())).collect();
            let sorted_field_names: Vec<String> = match &result_type {
                IrType::Struct(fields) => fields.iter().map(|(n, _)| n.clone()).collect(),
                _ => unreachable!(),
            };

            // Lower all field expressions and collect by name.
            // Track each non-Copy field value as pending intermediate.
            let mut field_values: std::collections::HashMap<String, ValueId> =
                std::collections::HashMap::new();
            for field in struct_expr.fields.iter() {
                let name = field.name.text(ctx.db).to_string();
                let value = lower_expression(ctx, field.value)?;
                // Track as pending intermediate so it's dropped on early return.
                if let Some(field_ty) = field_types.get(&name) {
                    ctx.push_pending_intermediate(value, field_ty);
                }
                field_values.insert(name, value);
            }

            // Build operands in sorted field order.
            let fields: Vec<Operand> = sorted_field_names.iter()
                .map(|name| {
                    field_values.get(name)
                        .map(|v| Operand::Value(*v))
                        .expect("struct field should exist")
                })
                .collect();

            let dest = ctx.fresh_value(result_type);
            ctx.emit_pack(dest, TypeRef::AnonStruct(0), fields);
            // Clear pending - field values are now consumed by Pack.
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }

        ExprFunKind::Tensor(tensor) => {
            // Push a new scope for this tensor's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            let elem_type = match &result_type {
                IrType::Tensor(t, _) => (**t).clone(),
                _ => panic!("expected Tensor type from typechecker"),
            };
            let shape = tensor.shape.clone();
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in tensor.elements.iter() {
                let value = lower_expression(ctx, *e)?;
                ctx.push_pending_intermediate(value, &elem_type);
                elements.push(Operand::Value(value));
            }
            let dest = ctx.fresh_value(result_type);
            ctx.emit_tensor_new(dest, shape, elements);
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Table(table) => {
            // Push a new scope for this table's pending intermediates.
            ctx.push_pending_scope();

            // Get the table type to determine column types.
            // Typechecker validates table literals have table types.
            let result_type = ctx.expr_type(expr);
            let IrType::Table(column_types) = &result_type else {
                panic!("table literal with non-table type {:?} - typechecker should catch this", result_type);
            };
            let column_types = column_types.clone();

            // Build tuple type for row from column types.
            let tuple_fields: Vec<_> = column_types.iter()
                .map(|(_, ty)| (**ty).clone())
                .collect();
            let row_type = IrType::Tuple(tuple_fields.clone());

            // Each row becomes a tuple operand.
            let mut rows = Vec::new();
            for row in table.rows.iter() {
                // Lower row elements, tracking each as pending intermediate.
                let mut elem_operands = Vec::new();
                for (i, e) in row.elements.iter().enumerate() {
                    let value = lower_expression(ctx, *e)?;
                    if let Some(elem_ty) = tuple_fields.get(i) {
                        ctx.push_pending_intermediate(value, elem_ty);
                    }
                    elem_operands.push(Operand::Value(value));
                }

                // Create tuple from elements.
                let tuple_dest = ctx.fresh_value(row_type.clone());
                ctx.emit_pack(tuple_dest, TypeRef::Tuple(elem_operands.len() as u32), elem_operands);
                // Elements consumed by Pack, but tuple itself is now pending.
                ctx.clear_pending_intermediates();
                ctx.push_pending_intermediate(tuple_dest, &row_type);
                rows.push(Operand::Value(tuple_dest));
            }

            let dest = ctx.fresh_value(result_type);
            ctx.emit_table_new(dest, rows);
            // All row tuples consumed by TableNew.
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Place(ref place) => {
            lower_place_expression(ctx, expr, place)
        }
        ExprFunKind::Index(_) => {
            panic!("bare index expression reached lowering - typechecker should reject bare a[i] without ? or !")
        }
        ExprFunKind::ParseError(_) => {
            panic!("parse error node reached lowering - callers should check for parse errors before lowering")
        }
        ExprFunKind::IntrinsicCall(icall) => {
            // Look up the intrinsic by name.
            // Typechecker validates intrinsic names.
            let name_str = icall.name.as_str(ctx.db);
            let (intrinsic_id, _def) = datalove_datafun_intrinsics::lookup_intrinsic(name_str)
                .unwrap_or_else(|| panic!("unknown intrinsic '{}' - typechecker should catch this", name_str));

            // Lower arguments.
            let args: Result<Vec<_>, _> = icall.args
                .iter()
                .map(|arg| lower_operand(ctx, *arg))
                .collect();
            let args = args?;

            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            ctx.emit_intrinsic(dest, intrinsic_id, args);

            // Drop expression temporaries after the intrinsic completes.
            ctx.emit_expr_temp_drops();

            Ok(dest)
        }

        ExprFunKind::Atom(atom) => {
            let result_type = ctx.expr_type(expr);
            match &result_type {
                IrType::Atom(name) => {
                    // Atom is zero-sized. Emit a const to define the value.
                    let variant_name = name.clone();
                    let dest = ctx.fresh_value(result_type);
                    ctx.emit(Instruction::Const {
                        dest,
                        value: ConstValue::Enum {
                            variant: variant_name,
                            payload: None,
                        },
                    });
                    Ok(dest)
                }
                IrType::Enum(_) => {
                    // Atom used in enum context - emit EnumVariant.
                    let name_str = atom.name.as_str(ctx.db);
                    let variants = result_type.enum_variants().unwrap();
                    let variant_index = variants.iter()
                        .position(|(n, _)| n == name_str)
                        .unwrap_or_else(|| panic!("atom variant '{}' not found in type", name_str))
                        as u32;
                    let dest = ctx.fresh_value(result_type);
                    ctx.emit(Instruction::EnumVariant {
                        dest,
                        variant_index,
                        payload: None,
                    });
                    Ok(dest)
                }
                _ => panic!("atom expr has unexpected IrType: {:?}", result_type),
            }
        }
        ExprFunKind::Term(term) => {
            let result_type = ctx.expr_type(expr);
            match &result_type {
                IrType::Term(_, _) => {
                    // Term has same layout as payload. Lower payload then move.
                    let payload_id = lower_expression(ctx, term.payload)?;
                    let dest = ctx.fresh_value(result_type);
                    ctx.emit(Instruction::Move {
                        dest,
                        src: Operand::Value(payload_id),
                    });
                    Ok(dest)
                }
                IrType::Enum(_) => {
                    // Term used in enum context - emit EnumVariant with payload.
                    let name_str = term.name.as_str(ctx.db);
                    let variants = result_type.enum_variants().unwrap();
                    let variant_index = variants.iter()
                        .position(|(n, _)| n == name_str)
                        .unwrap_or_else(|| panic!("term variant '{}' not found in type", name_str))
                        as u32;
                    let payload_id = lower_expression(ctx, term.payload)?;
                    let dest = ctx.fresh_value(result_type);
                    ctx.emit(Instruction::EnumVariant {
                        dest,
                        variant_index,
                        payload: Some(Operand::Value(payload_id)),
                    });
                    Ok(dest)
                }
                _ => panic!("term expr has unexpected IrType: {:?}", result_type),
            }
        }
        ExprFunKind::EnumLiteral(enum_lit) => {
            // EnumLiteral wraps an atom or term expr checked against target enum type.
            // Just lower the inner variant expression.
            lower_expression(ctx, enum_lit.variant)
        }
        ExprFunKind::Hinted(hinted) => {
            // The hint said what to check against and has done its work by
            // now; what runs is the expression under it.
            lower_expression(ctx, hinted.inner)
        }
    }
}

/// Lower a binary operation expression.
fn lower_binop<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    binop: ast::ExprBinOp<'db>,
) -> Result<ValueId, LowerError> {
    // Only the temps these operands create are ours to drop. An enclosing
    // expression that has already lowered something still needs it.
    let expr_temp_mark = ctx.expr_temps_mark();

    // Use lower_operand for borrowing semantics - operands are read by
    // reference, not consumed.
    let lhs = lower_operand(ctx, binop.lhs)?;
    let rhs = lower_operand(ctx, binop.rhs)?;

    let ast_op = binop.op;
    let result_type = ctx.expr_type(expr);

    let dest = ctx.fresh_value(result_type);

    let op = match ast_op {
        ast::BinOp::Add => BinOp::Add,
        ast::BinOp::Sub => BinOp::Sub,
        ast::BinOp::Mul => BinOp::Mul,
        ast::BinOp::Div => BinOp::Div,
        ast::BinOp::Eq => BinOp::Eq,
        ast::BinOp::Ne => BinOp::Ne,
        ast::BinOp::Lt => BinOp::Lt,
        ast::BinOp::Le => BinOp::Le,
        ast::BinOp::Gt => BinOp::Gt,
        ast::BinOp::Ge => BinOp::Ge,
        // Boolean logic operators.
        ast::BinOp::And => BinOp::LogicAnd,
        ast::BinOp::Or => BinOp::LogicOr,
        ast::BinOp::Xor => BinOp::LogicXor,
        // Checked result ops - emit BinOpChecked with early return on overflow.
        ast::BinOp::AddChecked => {
            return lower_checked_result_binop(ctx, BinOp::Add, lhs, rhs, dest);
        }
        ast::BinOp::SubChecked => {
            return lower_checked_result_binop(ctx, BinOp::Sub, lhs, rhs, dest);
        }
        ast::BinOp::MulChecked => {
            return lower_checked_result_binop(ctx, BinOp::Mul, lhs, rhs, dest);
        }
        ast::BinOp::DivChecked => {
            return lower_checked_result_binop(ctx, BinOp::Div, lhs, rhs, dest);
        }
        // Optional ops - emit BinOpChecked with early return on overflow.
        ast::BinOp::AddOptional => {
            return lower_optional_binop(ctx, BinOp::Add, lhs, rhs, dest);
        }
        ast::BinOp::SubOptional => {
            return lower_optional_binop(ctx, BinOp::Sub, lhs, rhs, dest);
        }
        ast::BinOp::MulOptional => {
            return lower_optional_binop(ctx, BinOp::Mul, lhs, rhs, dest);
        }
        ast::BinOp::DivOptional => {
            return lower_optional_binop(ctx, BinOp::Div, lhs, rhs, dest);
        }
    };

    ctx.emit(Instruction::BinOp {
        dest,
        op,
        lhs,
        rhs,
    });
    // Drop expression temporaries after borrowing operation completes.
    ctx.emit_expr_temp_drops_since(expr_temp_mark);
    Ok(dest)
}

/// Lower a unary operation expression.
fn lower_unaryop<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    unary: ast::ExprUnaryOp<'db>,
) -> Result<ValueId, LowerError> {
    let result_type = ctx.expr_type(expr);

    // Special case: negation of integer literal for signed fixed int types.
    // This handles cases like `let x: i32 = -2147483648` where the literal
    // value (2147483648) doesn't fit in the target type but the negated value does.
    if unary.op == ast::UnaryOp::Neg {
        if let ExprFunKind::Int(lit) = unary.operand.expr(ctx.db) {
            let text = lit.value.text(ctx.db);
            if let Some(const_value) = try_parse_negated_int_const(text, &result_type) {
                let dest = ctx.fresh_value(result_type);
                ctx.emit_const(dest, const_value);
                return Ok(dest);
            }
        }
    }

    // Use lower_operand for borrowing semantics.
    let operand = lower_operand(ctx, unary.operand)?;
    let dest = ctx.fresh_value(result_type);

    match unary.op {
        ast::UnaryOp::Neg => {
            ctx.emit(Instruction::UnaryOp {
                dest,
                op: UnaryOp::Neg,
                operand,
            });
            // Drop expression temporaries after borrowing operation completes.
            ctx.emit_expr_temp_drops();
            Ok(dest)
        }
        ast::UnaryOp::NegOptional => {
            lower_optional_unaryop(ctx, UnaryOp::Neg, operand, dest)
        }
        ast::UnaryOp::NegResult => {
            lower_checked_result_unaryop(ctx, UnaryOp::Neg, operand, dest)
        }
        ast::UnaryOp::Not => {
            ctx.emit(Instruction::UnaryOp {
                dest,
                op: UnaryOp::LogicNot,
                operand,
            });
            ctx.emit_expr_temp_drops();
            Ok(dest)
        }
    }
}

/// Lower an optional binary operation with early return on overflow.
///
/// For operators like `+?`, `-?`, `*?`, `/?`:
/// - Performs checked arithmetic
/// - On success: returns the result value
/// - On overflow/error: early returns with None
/// Drop the result a checked operator wrote, on the path that throws it away.
///
/// The operator writes its answer whether or not it fit, so that the overflow
/// branch has something whole to leave behind. For every fixed-width integer
/// that is free -- the answer is a scalar the frame owns nothing of. Inside a
/// generic it is a `data`, which for `index` and `offset` is on the heap,
/// because those are the two the erased shape cannot pack into its own words.
/// So the early return has to let it go.
fn drop_discarded_checked_result<'db>(ctx: &mut LowerCtx<'db>, dest: ValueId) {
    if ctx.body.value_types[dest.0 as usize] != IrType::Data {
        return;
    }
    ctx.emit(Instruction::Drop { operand: Operand::Value(dest) });
}

fn lower_optional_binop<'db>(
    ctx: &mut LowerCtx<'db>,
    op: BinOp,
    lhs: Operand,
    rhs: Operand,
    dest: ValueId,
) -> Result<ValueId, LowerError> {
    // Emit checked operation.
    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::BinOpChecked {
        dest,
        overflow,
        op,
        lhs,
        rhs,
    });
    ctx.emit_expr_temp_drops();

    // Create early return and continue blocks.
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();

    // Branch: if overflow, early return; else continue.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(overflow),
        then_block: early_return_block,
        then_args: Vec::new(),
        else_block: continue_block,
        else_args: Vec::new(),
    });

    // Early return block: wrap None and return.
    ctx.start_block(early_return_block);
    drop_discarded_checked_result(ctx, dest);
    ctx.emit_early_return_none(false);

    // Continue block: dest already has the computed value.
    ctx.start_block(continue_block);
    Ok(dest)
}

/// Lower checked result binary operators (+!, -!, *!, /!).
///
/// For operators like `+!`, `-!`, `*!`, `/!`:
/// - Performs checked arithmetic
/// - On success: returns the result value
/// - On overflow/error: early returns with Err(overflow error)
fn lower_checked_result_binop<'db>(
    ctx: &mut LowerCtx<'db>,
    op: BinOp,
    lhs: Operand,
    rhs: Operand,
    dest: ValueId,
) -> Result<ValueId, LowerError> {
    // Emit checked operation.
    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::BinOpChecked {
        dest,
        overflow,
        op,
        lhs,
        rhs,
    });
    ctx.emit_expr_temp_drops();

    // Create early return and continue blocks.
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();

    // Branch: if overflow, early return; else continue.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(overflow),
        then_block: early_return_block,
        then_args: Vec::new(),
        else_block: continue_block,
        else_args: Vec::new(),
    });

    // Early return block: create error and return Err.
    ctx.start_block(early_return_block);
    drop_discarded_checked_result(ctx, dest);
    ctx.emit_early_return_err_message("arithmetic overflow", false);

    // Continue block: dest already has the computed value.
    ctx.start_block(continue_block);
    Ok(dest)
}

/// Lower an optional unary operation with early return on overflow.
///
/// For operators like `-?`:
/// - Performs checked negation
/// - On success: returns the result value
/// - On overflow: early returns with None
fn lower_optional_unaryop<'db>(
    ctx: &mut LowerCtx<'db>,
    op: UnaryOp,
    operand: Operand,
    dest: ValueId,
) -> Result<ValueId, LowerError> {
    // Emit checked operation.
    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::UnaryOpChecked {
        dest,
        overflow,
        op,
        operand,
    });
    ctx.emit_expr_temp_drops();

    // Create early return and continue blocks.
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();

    // Branch: if overflow, early return; else continue.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(overflow),
        then_block: early_return_block,
        then_args: Vec::new(),
        else_block: continue_block,
        else_args: Vec::new(),
    });

    // Early return block: wrap None and return.
    ctx.start_block(early_return_block);
    drop_discarded_checked_result(ctx, dest);
    ctx.emit_early_return_none(false);

    // Continue block: dest already has the computed value.
    ctx.start_block(continue_block);
    Ok(dest)
}

/// Lower checked result unary operators (-!).
///
/// For operators like `-!`:
/// - Performs checked negation
/// - On success: returns the result value
/// - On overflow: early returns with Err(overflow error)
fn lower_checked_result_unaryop<'db>(
    ctx: &mut LowerCtx<'db>,
    op: UnaryOp,
    operand: Operand,
    dest: ValueId,
) -> Result<ValueId, LowerError> {
    // Emit checked operation.
    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::UnaryOpChecked {
        dest,
        overflow,
        op,
        operand,
    });
    ctx.emit_expr_temp_drops();

    // Create early return and continue blocks.
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();

    // Branch: if overflow, early return; else continue.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(overflow),
        then_block: early_return_block,
        then_args: Vec::new(),
        else_block: continue_block,
        else_args: Vec::new(),
    });

    // Early return block: create error and return Err.
    ctx.start_block(early_return_block);
    drop_discarded_checked_result(ctx, dest);
    ctx.emit_early_return_err_message("negation overflow", false);

    // Continue block: dest already has the computed value.
    ctx.start_block(continue_block);
    Ok(dest)
}

/// Lower try-option operator (`?` on Option).
fn lower_try_option<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    try_expr: ast::ExprTryOption<'db>,
) -> Result<ValueId, LowerError> {
    // Special case: a[i]? / m[k]? — index with option early-return.
    if let ExprFunKind::Index(ref index_expr) = try_expr.operand.expr(ctx.db) {
        return lower_collection_index_value(ctx, expr, index_expr, ast::IndexErrorMode::Option);
    }

    let src_id = lower_expression(ctx, try_expr.operand)?;
    let result_type = ctx.expr_type(expr);
    let dest = ctx.fresh_value(result_type.clone());
    let is_some = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::UnwrapOption {
        dest,
        is_some,
        src: Operand::Value(src_id),
    });

    // Create early return and continue blocks.
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();

    // Branch: if is_some, continue; else early return.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_some),
        then_block: continue_block,
        then_args: Vec::new(),
        else_block: early_return_block,
        else_args: Vec::new(),
    });

    // Early return block: wrap None and return.
    ctx.start_block(early_return_block);
    ctx.emit_early_return_none(false);

    // Continue block: dest already has the unwrapped value.
    ctx.start_block(continue_block);
    Ok(dest)
}

/// Lower try-result operator (`!` on Result).
fn lower_try_result<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    try_expr: ast::ExprTryResult<'db>,
) -> Result<ValueId, LowerError> {
    // Special case: a[i]! / m[k]! — index with result early-return.
    if let ExprFunKind::Index(ref index_expr) = try_expr.operand.expr(ctx.db) {
        return lower_collection_index_value(ctx, expr, index_expr, ast::IndexErrorMode::Result);
    }

    let src_id = lower_expression(ctx, try_expr.operand)?;
    let result_type = ctx.expr_type(expr);
    let ok_dest = ctx.fresh_value(result_type.clone());
    let err_dest = ctx.fresh_value(IrType::Error);
    let is_ok = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::UnwrapResult {
        ok_dest,
        err_dest,
        is_ok,
        src: Operand::Value(src_id),
    });

    // Create early return and continue blocks.
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();

    // Branch: if is_ok, continue; else early return.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(is_ok),
        then_block: continue_block,
        then_args: Vec::new(),
        else_block: early_return_block,
        else_args: Vec::new(),
    });

    // Early return block: wrap error and return.
    ctx.start_block(early_return_block);
    ctx.emit_early_return_err(Operand::Value(err_dest), false);

    // Continue block: ok_dest has the unwrapped Ok value.
    ctx.start_block(continue_block);
    Ok(ok_dest)
}

/// Lower `a[i]?` / `a[i]!` / `m[k]?` / `m[k]!` — collection index with early-return.
///
/// Emits ListGet or MapGet, branches on validity, early-returns on failure.
fn lower_collection_index_value<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    index_expr: &ast::ExprIndex<'db>,
    error_mode: ast::IndexErrorMode,
) -> Result<ValueId, LowerError> {
    // A get borrows what it is given, so anything made to hand it is this
    // expression's to let go of, on whichever way out it takes. A map keyed by
    // a string leaked the key it was looked up with.
    let temps_mark = ctx.expr_temps_mark();
    let base_op = lower_operand(ctx, index_expr.base)?;
    let key_op = lower_operand(ctx, index_expr.index)?;
    let base_type = ctx.expr_type(index_expr.base);
    let is_map = matches!(&base_type, IrType::Map(_, _));
    let result_type = ctx.expr_type(expr);
    let dest = ctx.fresh_value(result_type);
    let is_valid = ctx.fresh_value(IrType::Bool);

    if is_map {
        ctx.emit(Instruction::MapGet {
            dest,
            is_valid,
            map: base_op,
            key: key_op,
        });
    } else if matches!(&base_type, IrType::Tensor(_, _)) {
        ctx.emit(Instruction::TensorGet {
            dest,
            is_valid,
            tensor: base_op,
            index: key_op,
        });
    } else {
        ctx.emit(Instruction::ListGet {
            dest,
            is_valid,
            list: base_op,
            index: key_op,
        });
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

    // Read rather than taken, because both ways out let go of the same ones.
    let temps = ctx.expr_temps_since(temps_mark);

    ctx.start_block(early_return_block);
    for (value, ty) in &temps {
        ctx.emit_drop_for_operand(&Operand::Value(*value), ty);
    }
    match error_mode {
        ast::IndexErrorMode::Option => ctx.emit_early_return_none(false),
        ast::IndexErrorMode::Result => {
            let msg = if is_map { "key not found" } else { "index out of bounds" };
            ctx.emit_early_return_err_message(msg, false);
        }
    }

    ctx.start_block(continue_block);
    ctx.emit_expr_temp_drops_since(temps_mark);
    Ok(dest)
}

// ============================================================================
// Place Expression Lowering
// ============================================================================

/// Lower a Place expression to a value.
///
/// Walks the place steps, emitting index checks and field accesses.
/// The last index step produces the final value via ListGet/MapGet.
fn lower_place_expression<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    place: &ast::Place<'db>,
) -> Result<ValueId, LowerError> {
    let root_name_str = place.root.text(ctx.db);
    let mut current_op = ctx.lookup_var(root_name_str)
        .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", root_name_str));

    // Check if there are any index steps to determine lowering strategy.
    let has_index_steps = place.steps.iter().any(|s| matches!(s, ast::PlaceStep::Index(_)));

    // Process all steps. For intermediate steps, we get refs. For the final
    // index step, we use ListGet/MapGet to get a value.
    let last_idx = place.steps.len() - 1;
    for (i, step) in place.steps.iter().enumerate() {
        match step {
            ast::PlaceStep::Field(field) => {
                let base_type = operand_type(ctx, &current_op);
                let field_index = resolve_field_index(field, &base_type, ctx.db)?;
                if i == last_idx {
                    // Final field step — produce value via GetField.
                    // Works whether current_op is Value (no index steps)
                    // or ValueRef (after index steps that produced a ref).
                    let result_type = ctx.expr_type(expr);
                    let dest = ctx.fresh_value(result_type);
                    ctx.emit_get_field(dest, current_op, field_index);
                    return Ok(dest);
                } else if has_index_steps {
                    // Intermediate field step with index steps — use GetFieldRef
                    // to keep navigating via refs.
                    let field_type = field_type_from_base(&base_type, field_index);
                    let dest = ctx.fresh_value(IrType::Ref(Box::new(field_type)));
                    ctx.emit(Instruction::GetFieldRef {
                        dest,
                        src: current_op,
                        field_index,
                    });
                    current_op = Operand::ValueRef(dest);
                } else {
                    // Intermediate field step, no index steps — use GetField to walk.
                    let field_type = field_type_from_base(&base_type, field_index);
                    let dest = ctx.fresh_value(field_type);
                    ctx.emit_get_field(dest, current_op, field_index);
                    current_op = Operand::Value(dest);
                }
            }
            ast::PlaceStep::Index(idx) => {
                let error_mode = idx.error_mode
                    .expect("Place expression index steps always have error mode");
                let base_type = operand_type(ctx, &current_op);
                // As in `lower_collection_index_value`: a get borrows its key,
                // so anything made to hand it over is dropped on both ways out.
                let temps_mark = ctx.expr_temps_mark();
                let key_op = lower_operand(ctx, idx.index)?;
                let is_map = matches!(&base_type, IrType::Map(_, _));
                let is_tensor = matches!(&base_type, IrType::Tensor(_, _));

                if i == last_idx {
                    // Final step — produce a value via ListGet/MapGet/TensorGet.
                    let result_type = ctx.expr_type(expr);
                    let dest = ctx.fresh_value(result_type);
                    let is_valid = ctx.fresh_value(IrType::Bool);

                    if is_map {
                        ctx.emit(Instruction::MapGet {
                            dest,
                            is_valid,
                            map: current_op,
                            key: key_op,
                        });
                    } else if is_tensor {
                        ctx.emit(Instruction::TensorGet {
                            dest,
                            is_valid,
                            tensor: current_op,
                            index: key_op,
                        });
                    } else {
                        ctx.emit(Instruction::ListGet {
                            dest,
                            is_valid,
                            list: current_op,
                            index: key_op,
                        });
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

                    let temps = ctx.expr_temps_since(temps_mark);

                    ctx.start_block(early_return_block);
                    for (value, ty) in &temps {
                        ctx.emit_drop_for_operand(&Operand::Value(*value), ty);
                    }
                    match error_mode {
                        ast::IndexErrorMode::Option => ctx.emit_early_return_none(false),
                        ast::IndexErrorMode::Result => {
                            let msg = if is_map { "key not found" } else { "index out of bounds" };
                            ctx.emit_early_return_err_message(msg, false);
                        }
                    }

                    ctx.start_block(continue_block);
                    ctx.emit_expr_temp_drops_since(temps_mark);
                    return Ok(dest);
                } else {
                    // Intermediate index — get ref to element.
                    super::stmt::emit_fallible_index_check(ctx, current_op, &base_type, key_op, error_mode)?;
                    let dest = super::stmt::emit_collection_element_ref(ctx, current_op, &base_type, key_op);
                    current_op = Operand::ValueRef(dest);
                }
            }
        }
    }

    // Field-only Places return early from the loop above.
    // Zero-step Places are handled by the caller before reaching here.
    panic!("Place expression ended without returning — missing index or field steps");
}

/// Lower a Place expression to a reference (for ref/mut/out params and debuglog).
///
/// Returns a ref operand pointing to the final element.
fn lower_place_as_ref<'db>(
    ctx: &mut LowerCtx<'db>,
    place: &ast::Place<'db>,
) -> Result<Operand, LowerError> {
    let root_name_str = place.root.text(ctx.db);
    let mut current_op = ctx.lookup_var(root_name_str)
        .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", root_name_str));

    for step in &place.steps {
        match step {
            ast::PlaceStep::Field(field) => {
                let base_type = operand_type(ctx, &current_op);
                let field_index = resolve_field_index(field, &base_type, ctx.db)?;
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
                    .expect("Place expression index steps always have error mode");
                let base_type = operand_type(ctx, &current_op);
                let key_op = lower_operand(ctx, idx.index)?;
                super::stmt::emit_fallible_index_check(ctx, current_op, &base_type, key_op, error_mode)?;
                let dest = super::stmt::emit_collection_element_ref(ctx, current_op, &base_type, key_op);
                current_op = Operand::ValueRef(dest);
            }
        }
    }

    match current_op {
        Operand::ValueRef(v) => Ok(Operand::Value(v)),
        // Zero-step Place or field-only steps that end on a slot/param — pass through.
        other => Ok(other),
    }
}

/// Get type of an operand for place lowering.
pub(crate) fn operand_type(ctx: &LowerCtx, op: &Operand) -> IrType {
    match op {
        Operand::Slot(s) => ctx.body.slot_types[s.0 as usize].clone(),
        Operand::Param(p) => ctx.body.param_types[p.0 as usize].clone(),
        Operand::Value(v) => ctx.body.value_types[v.0 as usize].clone(),
        Operand::ValueRef(v) => {
            let ref_ty = &ctx.body.value_types[v.0 as usize];
            match ref_ty {
                IrType::Ref(inner) => inner.as_ref().clone(),
                _ => panic!("ValueRef has non-Ref type: {:?}", ref_ty),
            }
        }
        Operand::ExternalSlot { .. } => {
            todo!("external slot type in place lowering")
        }
        Operand::ExternalValue { .. } => {
            todo!("external value type in place lowering")
        }
    }
}

/// Get the type of a field from a base type.
pub(crate) fn field_type_from_base(base_type: &IrType, field_index: u32) -> IrType {
    match base_type {
        IrType::Struct(fields) => fields[field_index as usize].1.clone(),
        IrType::Tuple(fields) => fields[field_index as usize].clone(),
        _ => panic!("field access on non-struct/tuple type {:?}", base_type),
    }
}

/// Lower clone/coerce operator (`@`).
///
/// The `@` operator performs lossless clone/coerce operations:
/// - For same type copy types: emit Copy
/// - For fixed-width int widening to larger fixed-width: emit WidenFixed
/// - For fixed-width int widening to Int: emit Widen
/// - For linear types (clone): emit Clone
fn lower_clone_coerce<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    cc_expr: ast::ExprCloneCoerce<'db>,
) -> Result<ValueId, LowerError> {
    let src_type = ctx.expr_type(cc_expr.operand);
    let dest_type = ctx.expr_type(expr);

    // Only the temps this operand creates are ours to drop. An enclosing
    // expression that has already lowered a sibling still needs that sibling,
    // which for `a@ op b@` is the clone the left side left behind.
    let expr_temp_mark = ctx.expr_temps_mark();

    // Use lower_operand to borrow the operand (@ doesn't consume).
    let src = lower_operand(ctx, cc_expr.operand)?;

    let dest = ctx.fresh_value(dest_type.clone());

    // An atom or a term widening into an enum is built rather than copied.
    //
    // An enum keeps a discriminant saying which variant it holds, and neither
    // an atom nor a term has one to copy across: an atom is zero-sized, and a
    // term is laid out as its payload alone. So the copy below wrote nothing
    // where the discriminant goes and left whatever the slot held, which in a
    // fresh frame is zero -- every widened atom came out as the variant that
    // sorts first, whichever one was written.
    //
    // Used directly in enum context, `atom Red` is already lowered this way;
    // it is only through `@`, where the operand has its own narrow type, that
    // it was not.
    if let IrType::Enum(variants) = &dest_type {
        let variant_name = match &src_type {
            IrType::Atom(name) => Some(name.clone()),
            IrType::Term(name, _) => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = variant_name {
            let variant_index = variants
                .iter()
                .position(|(n, _)| *n == name)
                .unwrap_or_else(|| panic!("variant '{}' not found in enum", name))
                as u32;
            // A term's payload is the term itself, which is laid out as that
            // payload. It is cloned rather than handed over, because `@`
            // borrows what it is given and the enum takes ownership of what it
            // is built from -- moving it would free the same thing twice.
            let payload = match &src_type {
                IrType::Term(_, _) => {
                    let cloned = ctx.fresh_value(src_type.clone());
                    ctx.emit(Instruction::Clone { dest: cloned, src });
                    Some(Operand::Value(cloned))
                }
                _ => None,
            };
            ctx.emit(Instruction::EnumVariant { dest, variant_index, payload });
            ctx.emit_expr_temp_drops_since(expr_temp_mark);
            return Ok(dest);
        }
    }

    // Determine the right instruction based on types:
    // 1. Same type copy types: Copy (no-op for same type)
    // 2. Fixed-width int to larger fixed-width int: WidenFixed
    // 3. Fixed int to Int (bigint): Widen
    // 4. Linear types: Clone
    if src_type.is_copy() && dest_type.is_copy() {
        if src_type == dest_type {
            // Same type - just copy.
            ctx.emit(Instruction::Copy { dest, src });
        } else if is_fixed_width_int(&src_type) && is_fixed_width_int(&dest_type) {
            // Fixed-width to fixed-width widening.
            ctx.emit(Instruction::WidenFixed { dest, src });
        } else {
            // Other copy types - just copy (shouldn't happen in practice).
            ctx.emit(Instruction::Copy { dest, src });
        }
    } else if widens_to_int(&src_type) && matches!(dest_type, IrType::Int) {
        // Fixed int, index, or offset to Int (bigint) - widen.
        ctx.emit(Instruction::Widen { dest, src });
    } else {
        // Linear type - clone.
        ctx.emit(Instruction::Clone { dest, src });
    }

    // Drop expression temporaries after borrowing operation completes.
    ctx.emit_expr_temp_drops_since(expr_temp_mark);

    Ok(dest)
}

/// Lower field projection expression.
fn lower_field_proj<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    proj: ast::ExprFieldProj<'db>,
) -> Result<ValueId, LowerError> {
    // Lower the base expression.
    let temps_mark = ctx.expr_temps_mark();
    let base_id = lower_expression(ctx, proj.base)?;
    let base_type = ctx.expr_type(proj.base);

    // A base that is not a place is a value made here, and reading one field
    // out of it leaves the rest to be let go of: `f().0` takes the number out
    // of a tuple the call made and drops the string beside it. A place base
    // does not reach this -- `v.a` parses as a place with a field step, which
    // the place walker lowers -- but the guard says so rather than relying on
    // it, because dropping something a binding owns would be worse.
    if !matches!(proj.base.expr(ctx.db), ExprFunKind::Place(_)) {
        ctx.record_expr_temp(base_id, base_type.clone());
    }

    // Get the field index.
    let field_index = resolve_field_index(&proj.field, &base_type, ctx.db)?;

    // Get the result type.
    let result_type = ctx.expr_type(expr);
    let dest = ctx.fresh_value(result_type);

    ctx.emit_get_field(dest, Operand::Value(base_id), field_index);
    ctx.emit_expr_temp_drops_since(temps_mark);
    Ok(dest)
}

/// Resolve a field selector to a field index.
///
/// Panics if the field is not found or the base type is not a struct,
/// since the typechecker validates all field accesses.
fn resolve_field_index<'db>(
    selector: &ast::FieldSelector<'db>,
    base_type: &IrType,
    db: &'db dyn salsa::Database,
) -> Result<u32, LowerError> {
    match selector {
        ast::FieldSelector::Index(idx) => Ok(*idx),
        ast::FieldSelector::Name(name) => {
            // For struct types, find the field by name.
            // Struct fields are sorted by name.
            // Typechecker validates all field accesses.
            let name_str = name.text(db);
            let IrType::Struct(fields) = base_type else {
                panic!("named field projection on non-struct type {:?} - typechecker should catch this", base_type);
            };
            for (i, (field_name, _)) in fields.iter().enumerate() {
                if field_name == name_str {
                    return Ok(i as u32);
                }
            }
            panic!("field '{}' not found in struct - typechecker should catch this", name_str);
        }
    }
}

/// Check if a type is a fixed-width integer (u8, u16, u32, u64, i8, i16, i32, i64).
fn is_fixed_width_int(ty: &IrType) -> bool {
    matches!(
        ty,
        IrType::U8
            | IrType::U16
            | IrType::U32
            | IrType::U64
            | IrType::I8
            | IrType::I16
            | IrType::I32
            | IrType::I64
    )
}

/// True if `@` widens this type to `int`.
///
/// `index` and `offset` widen to `int` but do not participate in
/// fixed-to-fixed widening, so they are not [`is_fixed_width_int`].
fn widens_to_int(ty: &IrType) -> bool {
    is_fixed_width_int(ty) || matches!(ty, IrType::Index | IrType::Offset)
}

/// Lower tuple elements into a packed tuple value.
///
/// Shared by Tuple and AnonTuple lowering.
fn lower_tuple_elements<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    elements: &[ExprFun<'db>],
) -> Result<ValueId, LowerError> {
    ctx.push_pending_scope();

    let result_type = ctx.expr_type(expr);
    let element_types: Vec<IrType> = match &result_type {
        IrType::Tuple(types) => types.clone(),
        // Single-element parens like `(x)` have the element's type, not a Tuple.
        _ => vec![],
    };
    let mut fields = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        let value = lower_expression(ctx, *e)?;
        if let Some(elem_ty) = element_types.get(i) {
            ctx.push_pending_intermediate(value, elem_ty);
        }
        fields.push(Operand::Value(value));
    }
    let dest = ctx.fresh_value(result_type);
    ctx.emit_pack(dest, TypeRef::Tuple(fields.len() as u32), fields);
    ctx.clear_pending_intermediates();
    ctx.pop_pending_scope();
    Ok(dest)
}
