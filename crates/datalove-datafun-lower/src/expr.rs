//! Expression lowering.
//!
//! Transforms AST expressions into IR values and instructions. Handles literals,
//! operators, function calls, tuples, records, and control flow expressions.

use datalove_datafun_ast::ast::{self, ExprFun, ExprFunKind, ParamMode};
use datalove_datafun_ir::{
    IrType, Operand, ValueId, BinOp, UnaryOp, Instruction, ConstValue, Terminator, TypeRef,
};
use datalove_datafun_intrinsics::IntrinsicId;
use super::context::LowerCtx;
use super::literal::{parse_int_const, parse_hex_const, parse_float_const, try_parse_negated_int_const};
use super::LowerError;

/// Look up a variable by name, checking const bindings first.
///
/// A const evaluated before this body, a module's or a script's, has no
/// binding here. One of a copy type is written in as a `Const`, returned with
/// `is_fresh_const = true`. One of any other type is borrowed from the one
/// copy the backend builds, through a `StaticRef`, and returned as the
/// reference, which nothing drops. Other names return their
/// Slot/Param/Value operand with `is_fresh_const = false`.
///
/// Callers that need drop tracking should call `record_expr_temp` only
/// when `is_fresh_const` is true, since regular `let` bindings bound to
/// `Operand::Value` are already managed by the drop schedule.
fn lower_var_operand(ctx: &mut LowerCtx, name: &str) -> Result<(Operand, bool), LowerError> {
    if let Some((const_type, const_value)) = ctx.lookup_const(name) {
        let const_type = const_type.clone();
        let const_value = std::sync::Arc::clone(const_value);
        if !const_type.is_copy() {
            let dest = ctx.fresh_value(IrType::Ref(Box::new(const_type)));
            ctx.emit(Instruction::StaticRef { dest, value: datalove_datafun_ir::SharedConst(const_value) });
            return Ok((Operand::ValueRef(dest), false));
        }
        let dest = ctx.fresh_value(const_type);
        ctx.emit(Instruction::Const {
            dest,
            value: (*const_value).clone(),
        });
        return Ok((Operand::Value(dest), true));
    }
    // A name the typechecker accepted but nothing here defines is a module-level
    // const that has not been evaluated yet. The caller lowers this function
    // again once it has.
    let op = ctx.lookup_var(name)
        .ok_or_else(|| LowerError::BindingNotAvailable(name.to_string()))?;

    Ok((op, false))
}

/// Record a const `lower_var_operand` built as a temporary to drop.
fn record_fresh_const_temp(ctx: &mut LowerCtx, operand: &Operand) {
    let Operand::Value(vid) = *operand else {
        unreachable!("lower_var_operand builds a const as a value");
    };
    let ty = ctx.body.value_types[vid.0 as usize].clone();
    ctx.record_expr_temp(vid, ty);
}

/// Lower an operand for borrowing contexts (binop, unaryop, `@`).
///
/// A field projection borrows part of an aggregate; it does not produce a value
/// anyone owns. Lowering one with `lower_expression` moves the field out and
/// records it as a temporary to drop, which frees a field the aggregate still
/// holds -- the shape `lower_expression_for_ref` was written to avoid, and
/// which `@` on a linear field reached because nothing sent it there.
///
/// Everything else keeps the old route, including a place with steps: what
/// `lower_place_as_ref` gives for `a[i]?.0` is a borrow of a list element,
/// which is not the same thing here as it is for a `ref` argument, and the
/// index fixtures say so.
pub fn lower_operand<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<Operand, LowerError> {
    match expr.kind() {
        ExprFunKind::Place(place) if place.steps.is_empty() => {
            ctx.note_place_lowered(expr);
            let name_str = place.root.text(ctx.db);
            let (operand, is_fresh_const) = lower_var_operand(ctx, name_str)?;
            if is_fresh_const {
                record_fresh_const_temp(ctx, &operand);
            }
            Ok(operand)
        }
        // A projection borrows part of what it reads from and reads through
        // the borrow. Reading a linear field as a value moves it out of an
        // aggregate that still holds it, and the temporary that records is
        // dropped, freeing it twice. An index has no such fault -- `ListGet`
        // and its neighbours clone the element -- but it clones it a second
        // time when `@` follows, which borrowing avoids.
        //
        // A copy element keeps the direct read: there is nothing to move out
        // and nothing to free, so a borrow would buy nothing.
        ExprFunKind::Place(place)
            if !place.steps.is_empty()
                && !ctx.expr_type(expr).is_copy() =>
        {
            let field_ref = lower_place_as_ref(ctx, place)?;
            Ok(operand_as_value_ref(field_ref))
        }
        ExprFunKind::FieldProj(proj) if !ctx.expr_type(expr).is_copy() => {
            let field_ref = lower_field_proj_as_ref(ctx, expr, proj)?;
            Ok(operand_as_value_ref(field_ref))
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
        if let ExprFunKind::Place(place) = arg.kind() {
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
        if let ExprFunKind::FieldProj(proj) = arg.kind() {
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
    match arg.kind() {
        ExprFunKind::Place(place) if place.steps.is_empty() => {
            ctx.note_place_lowered(arg);
            let name_str = place.root.text(ctx.db);
            let (operand, is_fresh_const) = lower_var_operand(ctx, name_str)?;
            let is_external = matches!(
                operand,
                Operand::ExternalValue { .. } | Operand::ExternalSlot { .. },
            );
            if is_external {
                // The callee consumes the copy, not the binding, and a const
                // built for the occasion is dropped once copied.
                if is_fresh_const {
                    record_fresh_const_temp(ctx, &operand);
                }
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
    let operand = match arg.kind() {
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = ctx.expr(try_op.operand).kind() {
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Option)?
            } else {
                return Ok(None);
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = ctx.expr(try_op.operand).kind() {
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)?
            } else {
                return Ok(None);
            }
        }
        ExprFunKind::Place(place) => {
            lower_place_as_ref(ctx, place)?
        }
        _ => return Ok(None),
    };

    // For an out param, destroy what the destination holds before the call:
    // through the reference for an element or field, and as the binding is
    // tracked for a whole binding, which may hold nothing.
    if mode == ParamMode::Out
        && let Some(ty) = arg_type
        && !ty.is_copy()
    {
        match operand {
            Operand::Value(ref_value) => ctx.emit(Instruction::DropViaRef { ref_value }),
            ref place => ctx.emit_drop_for_operand(place, ty),
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
    index_expr: &ast::ExprIndex,
    error_mode: ast::IndexErrorMode,
) -> Result<Operand, LowerError> {
    let base_op = lower_operand(ctx, ctx.expr(index_expr.base))?;
    let key_op = lower_operand(ctx, ctx.expr(index_expr.index))?;
    let base_type = ctx.expr_type(ctx.expr(index_expr.base));
    super::stmt::emit_fallible_index_check(ctx, base_op, &base_type, key_op, error_mode, false)?;
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
    proj: &ast::ExprFieldProj<'db>,
) -> Result<Operand, LowerError> {
    let base_type = ctx.expr_type(ctx.expr(proj.base));

    // When the base is a Place (with or without steps), build an extended
    // Place with the field step appended and delegate to lower_place_as_ref.
    if let ExprFunKind::Place(place) = ctx.expr(proj.base).kind() {
        let mut extended = place.clone();
        extended.steps.push(ast::PlaceStep::Field(proj.field.clone()));
        return lower_place_as_ref(ctx, &extended);
    }

    // When the base is a nested FieldProj, recurse to get a ref, then
    // apply GetFieldRef on top.
    let src = match ctx.expr(proj.base).kind() {
        ExprFunKind::FieldProj(inner_proj) => {
            let base_ref = lower_field_proj_as_ref(ctx, ctx.expr(proj.base), inner_proj)?;
            operand_as_value_ref(base_ref)
        }
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = ctx.expr(try_op.operand).kind() {
                operand_as_value_ref(lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Option)?)
            } else {
                Operand::Value(lower_expression(ctx, ctx.expr(proj.base))?)
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = ctx.expr(try_op.operand).kind() {
                operand_as_value_ref(lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)?)
            } else {
                Operand::Value(lower_expression(ctx, ctx.expr(proj.base))?)
            }
        }
        _ => {
            Operand::Value(lower_expression(ctx, ctx.expr(proj.base))?)
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
    match expr.kind() {
        ExprFunKind::FieldProj(proj) => {
            // Field projections use GetFieldRef to borrow without copying.
            lower_field_proj_as_ref(ctx, expr, proj)
        }
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = ctx.expr(try_op.operand).kind() {
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
            if let ExprFunKind::Index(index_expr) = ctx.expr(try_op.operand).kind() {
                // a[i]! — borrow list element by reference.
                lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)
            } else {
                let expr_type = ctx.expr_type(expr);
                let value_id = lower_expression(ctx, expr)?;
                ctx.record_expr_temp(value_id, expr_type);
                Ok(Operand::Value(value_id))
            }
        }
        ExprFunKind::Place(place) if place.steps.is_empty() => {
            ctx.note_place_lowered(expr);
            let name_str = place.root.text(ctx.db);
            let (operand, is_fresh_const) = lower_var_operand(ctx, name_str)?;
            if is_fresh_const {
                record_fresh_const_temp(ctx, &operand);
            }
            Ok(operand)
        }
        ExprFunKind::Place(place) => {
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
/// Used for every consuming use of a binding an earlier unit owns.
fn emit_owned_copy<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    operand: Operand,
) -> ValueId {
    let ty = ctx.expr_type(expr);
    let dest = ctx.fresh_value(ty.clone());
    if ty.is_copy() {
        ctx.emit(Instruction::Copy { dest, src: operand });
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
    match expr.kind() {
        ExprFunKind::Place(place) if place.steps.is_empty() => {
            ctx.note_place_lowered(expr);
            let name_str = place.root.text(ctx.db);
            let (operand, _) = lower_var_operand(ctx, name_str)?;

            match operand {
                Operand::Value(v) => {
                    // For SSA values, just return the existing ValueId.
                    Ok(v)
                }
                // A name bound to a reference, as a `for` binds its element:
                // a copy is read through it, where returning the reference
                // would hand over the pointer for the value.
                Operand::ValueRef(r) => {
                    let ty = ctx.expr_type(expr);
                    if !ty.is_copy() {
                        return Ok(r);
                    }
                    let dest = ctx.fresh_value(ty);
                    ctx.emit(Instruction::Copy { dest, src: operand });
                    Ok(dest)
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
                    Ok(emit_owned_copy(ctx, expr, operand))
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
            let const_value = parse_hex_const(text, &result_type)
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
            let func_ref = ctx.resolve_call(expr, call);

            // Get param modes and types from the resolved call target.
            let target = ctx.call_targets.and_then(|t| t.get_call(expr, call));
            // A const argument is passed by reference, as the callee's const
            // parameter is lowered: see `lower_function_body`.
            let param_modes: Vec<ParamMode> = target
                .map(|t| t.func().params().iter()
                    .map(|p| if p.is_comptime { ParamMode::Ref } else { p.mode })
                    .collect())
                .unwrap_or_default();
            // The callee is handed a descriptor for each of its type
            // parameters, and every argument is passed as it stands. What this
            // site binds them to is `bindings`, written over the caller's own
            // type parameters, and a parameter's type here is the callee's
            // written type with those substituted in.
            let type_params: Vec<bct::text::InternedText<'db>> = target
                .map(|t| t.func().type_params().clone())
                .unwrap_or_default();
            let bindings: Vec<IrType> = target
                .map(|t| t.type_args().clone())
                .unwrap_or_default()
                .iter()
                .map(|ty| ctx.ir_type(ty))
                .collect();
            let param_types: Vec<IrType> = target
                .map(|t| t.func().params().iter()
                    .map(|p| IrType::from_type_hint_generic(ctx.db, &p.type_hint, &type_params)
                        .substitute(&bindings))
                    .collect())
                .unwrap_or_default();

            // Lower each arg, tracking in-mode args as pending intermediates.
            let call_args = &call.args;
            let mut args = Vec::with_capacity(call_args.len());
            for (i, arg) in call_args.iter().enumerate() {
                let mode = param_modes.get(i).copied().unwrap_or(ParamMode::In);
                let operand = lower_call_arg(ctx, expr.at(*arg), mode, param_types.get(i))?;
                // Track in-mode args as pending intermediate, a binding handed
                // over as well as a value: a later argument's `?` returns
                // before the call takes either.
                // Ref/mut/out args are tracked via lower_operand's expr_temps.
                if mode == ParamMode::In {
                    if let Some(arg_ty) = param_types.get(i) {
                        match operand {
                            Operand::Value(v) => ctx.push_pending_intermediate(v, arg_ty),
                            Operand::Param(_) | Operand::Slot(_) => {
                                ctx.push_pending_moved_binding(operand, arg_ty);
                            }
                            _ if arg_ty.is_copy() => {}
                            _ => unreachable!("an owned argument lowers to a value, param or slot: {operand:?}"),
                        }
                    }
                }
                args.push(operand);
            }

            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            // A call to a function with const parameters is a `ComptimeCall`,
            // which specialization points at the copy for its arguments. A
            // callee's const parameters and its type parameters are
            // independent, so a comptime callee may be generic, and the copy
            // built for an instantiation takes the same descriptors.
            let comptime_param_indices: Vec<usize> = target
                .map(|t| {
                    t.func().params()
                        .iter()
                        .enumerate()
                        .filter_map(|(i, p)| if p.is_comptime { Some(i) } else { None })
                        .collect()
                })
                .unwrap_or_default();

            if !comptime_param_indices.is_empty() {
                ctx.emit_comptime_call(dest, func_ref, args, comptime_param_indices, bindings);
            } else {
                ctx.emit_call_with_bindings(dest, func_ref, args, bindings);
            }

            // Args consumed by Call/ComptimeCall; pop scope.
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            ctx.emit_expr_temp_drops_since(expr_temp_mark);
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, ctx.expr(some_expr.payload))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_wrap_some(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ctx.expr(ok_expr.payload))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_wrap_ok(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, ctx.expr(er_expr.payload))?;
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
            let IrType::List(elem_type) = &result_type else {
                panic!("expected List type from typechecker");
            };
            let elem_type = (**elem_type).clone();
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in list.elements.iter() {
                let value = lower_expression(ctx, ctx.expr(*e))?;
                ctx.push_pending_intermediate(value, &elem_type);
                elements.push(Operand::Value(value));
            }
            let dest = ctx.fresh_value(result_type);
            ctx.emit_list_new(dest, elements);
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Set(set) => {
            // Push a new scope for this set's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            let IrType::Set(elem_type) = &result_type else {
                panic!("expected Set type from typechecker");
            };
            let elem_type = (**elem_type).clone();
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in set.elements.iter() {
                let value = lower_expression(ctx, ctx.expr(*e))?;
                ctx.push_pending_intermediate(value, &elem_type);
                elements.push(Operand::Value(value));
            }
            let dest = ctx.fresh_value(result_type);
            ctx.emit_set_new(dest, elements);
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Map(map) => {
            // Push a new scope for this map's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            let IrType::Map(key_type, val_type) = &result_type else {
                panic!("expected Map type from typechecker");
            };
            let (key_type, val_type) = ((**key_type).clone(), (**val_type).clone());
            // Lower each entry and track keys/values as pending intermediates.
            let mut entries = Vec::new();
            for e in map.entries.iter() {
                let k = lower_expression(ctx, ctx.expr(e.key))?;
                ctx.push_pending_intermediate(k, &key_type);
                let v = lower_expression(ctx, ctx.expr(e.value))?;
                ctx.push_pending_intermediate(v, &val_type);
                entries.push((Operand::Value(k), Operand::Value(v)));
            }
            let dest = ctx.fresh_value(result_type);
            ctx.emit_map_new(dest, entries);
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
            Ok(dest)
        }
        ExprFunKind::Error(err_expr) => {
            let inner_id = lower_expression(ctx, ctx.expr(err_expr.value))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit_error_from(dest, Operand::Value(inner_id));
            Ok(dest)
        }
        ExprFunKind::Data(data_expr) => {
            let inner_id = lower_expression(ctx, ctx.expr(data_expr.value))?;
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
        ExprFunKind::DataFile(file) => {
            let ty = ctx.expr_type(expr);
            let value = ctx.data_file_value(&file, &ty);
            let dest = ctx.fresh_value(ty);
            ctx.emit_const(dest, (*value).clone());
            Ok(dest)
        }
        ExprFunKind::String(string_expr) => {
            let raw = string_expr.value.as_str(ctx.db);
            let content = super::literal::string_literal_value(raw);
            let dest = ctx.fresh_value(IrType::String);
            ctx.emit_const(dest, ConstValue::String(content));
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
                let value = lower_expression(ctx, ctx.expr(field.value))?;
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
                let value = lower_expression(ctx, ctx.expr(*e))?;
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
                    let value = lower_expression(ctx, ctx.expr(*e))?;
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
        ExprFunKind::Place(place) => {
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
            let expr_temp_mark = ctx.expr_temps_mark();
            let args: Result<Vec<_>, _> = icall.args
                .iter()
                .map(|arg| lower_operand(ctx, ctx.expr(*arg)))
                .collect();
            let args = args?;

            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            ctx.emit_intrinsic(dest, intrinsic_id, args);

            // Drop expression temporaries after the intrinsic completes.
            ctx.emit_expr_temp_drops_since(expr_temp_mark);

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
                    let payload_id = lower_expression(ctx, ctx.expr(term.payload))?;
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
                    let payload_id = lower_expression(ctx, ctx.expr(term.payload))?;
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
            lower_expression(ctx, ctx.expr(enum_lit.variant))
        }
        ExprFunKind::Hinted(hinted) => {
            // The hint said what to check against and has done its work by
            // now; what runs is the expression under it.
            lower_expression(ctx, ctx.expr(hinted.inner))
        }
    }
}

/// Lower a binary operation expression.
fn lower_binop<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    binop: &ast::ExprBinOp,
) -> Result<ValueId, LowerError> {
    // Only the temps these operands create are ours to drop. An enclosing
    // expression that has already lowered something still needs it.
    let expr_temp_mark = ctx.expr_temps_mark();

    // Use lower_operand for borrowing semantics - operands are read by
    // reference, not consumed.
    let lhs = lower_operand(ctx, ctx.expr(binop.lhs))?;
    let rhs = lower_operand(ctx, ctx.expr(binop.rhs))?;

    let result_type = ctx.expr_type(expr);
    lower_binop_operands(ctx, binop.op, lhs, rhs, result_type, expr_temp_mark)
}

/// Lower binary operator `ast_op` on operands already lowered.
///
/// A checked or optional operator branches to an early return when it fails.
/// The temporaries recorded since `expr_temp_mark` are the operands', and are
/// dropped once the operator has read them.
pub(crate) fn lower_binop_operands<'db>(
    ctx: &mut LowerCtx<'db>,
    ast_op: ast::BinOp,
    lhs: Operand,
    rhs: Operand,
    result_type: IrType,
    expr_temp_mark: usize,
) -> Result<ValueId, LowerError> {
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
            return lower_checked_result_binop(ctx, BinOp::Add, lhs, rhs, dest, expr_temp_mark);
        }
        ast::BinOp::SubChecked => {
            return lower_checked_result_binop(ctx, BinOp::Sub, lhs, rhs, dest, expr_temp_mark);
        }
        ast::BinOp::MulChecked => {
            return lower_checked_result_binop(ctx, BinOp::Mul, lhs, rhs, dest, expr_temp_mark);
        }
        ast::BinOp::DivChecked => {
            return lower_checked_result_binop(ctx, BinOp::Div, lhs, rhs, dest, expr_temp_mark);
        }
        // Optional ops - emit BinOpChecked with early return on overflow.
        ast::BinOp::AddOptional => {
            return lower_optional_binop(ctx, BinOp::Add, lhs, rhs, dest, expr_temp_mark);
        }
        ast::BinOp::SubOptional => {
            return lower_optional_binop(ctx, BinOp::Sub, lhs, rhs, dest, expr_temp_mark);
        }
        ast::BinOp::MulOptional => {
            return lower_optional_binop(ctx, BinOp::Mul, lhs, rhs, dest, expr_temp_mark);
        }
        ast::BinOp::DivOptional => {
            return lower_optional_binop(ctx, BinOp::Div, lhs, rhs, dest, expr_temp_mark);
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

/// How a checked or optional operator leaves the function when it fails.
enum Failure {
    Error,
    None,
}

/// Lower an update of `place` with `ast_op` and `rhs`, already lowered, as
/// one instruction: what `set p op= v` is, for a place of a concrete type.
///
/// A checked or optional operator branches on its failure to an early return,
/// as its binary form does, leaving the place as it was. The temporaries
/// recorded since `expr_temp_mark` are the operand's, dropped once it's read.
pub(crate) fn lower_op_assign<'db>(
    ctx: &mut LowerCtx<'db>,
    ast_op: ast::BinOp,
    place: Operand,
    rhs: Operand,
    ty: &IrType,
    expr_temp_mark: usize,
) -> Result<(), LowerError> {
    let (op, failure) = match ast_op {
        ast::BinOp::Add => (BinOp::Add, None),
        ast::BinOp::Sub => (BinOp::Sub, None),
        ast::BinOp::Mul => (BinOp::Mul, None),
        ast::BinOp::Div => (BinOp::Div, None),
        ast::BinOp::AddChecked => (BinOp::Add, Some(Failure::Error)),
        ast::BinOp::SubChecked => (BinOp::Sub, Some(Failure::Error)),
        ast::BinOp::MulChecked => (BinOp::Mul, Some(Failure::Error)),
        ast::BinOp::DivChecked => (BinOp::Div, Some(Failure::Error)),
        ast::BinOp::AddOptional => (BinOp::Add, Some(Failure::None)),
        ast::BinOp::SubOptional => (BinOp::Sub, Some(Failure::None)),
        ast::BinOp::MulOptional => (BinOp::Mul, Some(Failure::None)),
        ast::BinOp::DivOptional => (BinOp::Div, Some(Failure::None)),
        other => panic!("{other:?} in a compound assignment, which typecheck refuses"),
    };
    let Some(failure) = failure else {
        ctx.emit(Instruction::OpAssign { place, op, rhs });
        ctx.emit_expr_temp_drops_since(expr_temp_mark);
        return Ok(());
    };

    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::OpAssignChecked { overflow, place, op, rhs });
    let early_return_block = ctx.fresh_block();
    let continue_block = ctx.fresh_block();
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(overflow),
        then_block: early_return_block,
        then_args: Vec::new(),
        else_block: continue_block,
        else_args: Vec::new(),
    });

    ctx.start_block(early_return_block);
    match failure {
        Failure::Error => ctx.emit_early_return_err_message(checked_failure_message(op, ty), false),
        Failure::None => ctx.emit_early_return_none(false),
    }

    ctx.start_block(continue_block);
    ctx.emit_expr_temp_drops_since(expr_temp_mark);
    Ok(())
}

/// Lower a unary operation expression.
fn lower_unaryop<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    unary: &ast::ExprUnaryOp,
) -> Result<ValueId, LowerError> {
    let result_type = ctx.expr_type(expr);

    // Special case: negation of integer literal for signed fixed int types.
    // This handles cases like `let x: i32 = -2147483648` where the literal
    // value (2147483648) doesn't fit in the target type but the negated value does.
    if unary.op == ast::UnaryOp::Neg {
        if let ExprFunKind::Int(lit) = ctx.expr(unary.operand).kind() {
            let text = lit.value.text(ctx.db);
            if let Some(const_value) = try_parse_negated_int_const(text, &result_type) {
                let dest = ctx.fresh_value(result_type);
                ctx.emit_const(dest, const_value);
                return Ok(dest);
            }
        }
    }

    // Use lower_operand for borrowing semantics. Only the temps the operand
    // creates are ours to drop.
    let expr_temp_mark = ctx.expr_temps_mark();
    let operand = lower_operand(ctx, ctx.expr(unary.operand))?;
    let dest = ctx.fresh_value(result_type);

    match unary.op {
        ast::UnaryOp::Neg => {
            ctx.emit(Instruction::UnaryOp {
                dest,
                op: UnaryOp::Neg,
                operand,
            });
            // Drop expression temporaries after borrowing operation completes.
            ctx.emit_expr_temp_drops_since(expr_temp_mark);
            Ok(dest)
        }
        ast::UnaryOp::NegOptional => {
            lower_optional_unaryop(ctx, UnaryOp::Neg, operand, dest, expr_temp_mark)
        }
        ast::UnaryOp::NegResult => {
            lower_checked_result_unaryop(ctx, UnaryOp::Neg, operand, dest, expr_temp_mark)
        }
        ast::UnaryOp::Not => {
            ctx.emit(Instruction::UnaryOp {
                dest,
                op: UnaryOp::LogicNot,
                operand,
            });
            ctx.emit_expr_temp_drops_since(expr_temp_mark);
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
fn lower_optional_binop<'db>(
    ctx: &mut LowerCtx<'db>,
    op: BinOp,
    lhs: Operand,
    rhs: Operand,
    dest: ValueId,
    expr_temp_mark: usize,
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
    ctx.emit_early_return_none(false);

    // Continue block: dest already has the computed value. The early return
    // let go of every temporary; this way out lets go of the operands' own.
    ctx.start_block(continue_block);
    ctx.emit_expr_temp_drops_since(expr_temp_mark);
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
    expr_temp_mark: usize,
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
    let message = checked_failure_message(op, &operand_type(ctx, &lhs));
    ctx.start_block(early_return_block);
    ctx.emit_early_return_err_message(message, false);

    // Continue block: dest already has the computed value. The early return
    // let go of every temporary; this way out lets go of the operands' own.
    ctx.start_block(continue_block);
    ctx.emit_expr_temp_drops_since(expr_temp_mark);
    Ok(dest)
}

/// The error a checked `op` on `ty` returns when it fails.
///
/// The check reports only that the operation failed, not why, so this says
/// every way it can. Division fails on a zero divisor for every integer type,
/// and overflows only for a signed one, at its minimum divided by -1.
fn checked_failure_message(op: BinOp, ty: &IrType) -> &'static str {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul => "arithmetic overflow",
        BinOp::Div => match ty {
            // A `fixedint` type parameter may turn out to be signed.
            IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 | IrType::Offset
            | IrType::Param(_) => {
                "division by zero or overflow"
            }
            IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64 | IrType::Index | IrType::Int => {
                "division by zero"
            }
            other => panic!("checked division of {:?}, which typecheck refuses", other),
        },
        other => panic!("{:?} as a checked operation; only Add, Sub, Mul and Div can fail", other),
    }
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
    expr_temp_mark: usize,
) -> Result<ValueId, LowerError> {
    // Emit checked operation.
    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::UnaryOpChecked {
        dest,
        overflow,
        op,
        operand,
    });

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
    ctx.emit_early_return_none(false);

    // Continue block: dest already has the computed value. The early return
    // let go of every temporary; this way out lets go of the operands' own.
    ctx.start_block(continue_block);
    ctx.emit_expr_temp_drops_since(expr_temp_mark);
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
    expr_temp_mark: usize,
) -> Result<ValueId, LowerError> {
    // Emit checked operation.
    let overflow = ctx.fresh_value(IrType::Bool);
    ctx.emit(Instruction::UnaryOpChecked {
        dest,
        overflow,
        op,
        operand,
    });

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
    ctx.emit_early_return_err_message("negation overflow", false);

    // Continue block: dest already has the computed value. The early return
    // let go of every temporary; this way out lets go of the operands' own.
    ctx.start_block(continue_block);
    ctx.emit_expr_temp_drops_since(expr_temp_mark);
    Ok(dest)
}

/// Lower try-option operator (`?` on Option).
fn lower_try_option<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    try_expr: &ast::ExprTryOption,
) -> Result<ValueId, LowerError> {
    // Special case: a[i]? / m[k]? — index with option early-return.
    if let ExprFunKind::Index(index_expr) = ctx.expr(try_expr.operand).kind() {
        return lower_collection_index_value(ctx, expr, index_expr, ast::IndexErrorMode::Option);
    }

    let src_id = lower_expression(ctx, ctx.expr(try_expr.operand))?;
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
    try_expr: &ast::ExprTryResult,
) -> Result<ValueId, LowerError> {
    // Special case: a[i]! / m[k]! — index with result early-return.
    if let ExprFunKind::Index(index_expr) = ctx.expr(try_expr.operand).kind() {
        return lower_collection_index_value(ctx, expr, index_expr, ast::IndexErrorMode::Result);
    }

    let src_id = lower_expression(ctx, ctx.expr(try_expr.operand))?;
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
    index_expr: &ast::ExprIndex,
    error_mode: ast::IndexErrorMode,
) -> Result<ValueId, LowerError> {
    // A get borrows what it is given, so anything made to hand it is this
    // expression's to let go of, on whichever way out it takes. A map keyed by
    // a string leaked the key it was looked up with.
    let temps_mark = ctx.expr_temps_mark();
    let base_op = lower_operand(ctx, ctx.expr(index_expr.base))?;
    let key_op = lower_operand(ctx, ctx.expr(index_expr.index))?;
    let base_type = ctx.expr_type(ctx.expr(index_expr.base));
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

    // The early return lets go of every temporary, these and any an enclosing
    // expression holds; the other way out lets go of these.
    ctx.start_block(early_return_block);
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
    // A const evaluated before this body has no binding here; see
    // `lower_var_operand`.
    let (mut current_op, is_fresh_const) = lower_var_operand(ctx, root_name_str)?;
    if is_fresh_const {
        record_fresh_const_temp(ctx, &current_op);
    }

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
                } else {
                    // Intermediate field step -- navigate by reference rather
                    // than copying the whole intermediate out to read one field
                    // of it.
                    let field_type = field_type_from_base(&base_type, field_index);
                    let dest = ctx.fresh_value(IrType::Ref(Box::new(field_type)));
                    ctx.emit(Instruction::GetFieldRef {
                        dest,
                        src: current_op,
                        field_index,
                    });
                    current_op = Operand::ValueRef(dest);
                }
            }
            ast::PlaceStep::Index(idx) => {
                let error_mode = idx.error_mode
                    .expect("Place expression index steps always have error mode");
                let base_type = operand_type(ctx, &current_op);
                // As in `lower_collection_index_value`: a get borrows its key,
                // so anything made to hand it over is dropped on both ways out.
                let temps_mark = ctx.expr_temps_mark();
                let key_op = lower_operand(ctx, ctx.expr(idx.index))?;
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

                    ctx.start_block(early_return_block);
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
                    super::stmt::emit_fallible_index_check(ctx, current_op, &base_type, key_op, error_mode, false)?;
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
    // A module-level or script-level const borrowed from a function has no
    // binding here; its value is materialized as a temporary to borrow.
    let (mut current_op, is_fresh_const) = lower_var_operand(ctx, root_name_str)?;
    if is_fresh_const {
        record_fresh_const_temp(ctx, &current_op);
    }

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
                let key_op = lower_operand(ctx, ctx.expr(idx.index))?;
                super::stmt::emit_fallible_index_check(ctx, current_op, &base_type, key_op, error_mode, false)?;
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
        Operand::ExternalSlot { .. } | Operand::ExternalValue { .. } => {
            // An earlier unit's binding has its type recorded under its name,
            // which is the name it's bound to here.
            let name = ctx.body.variables.iter()
                .find_map(|(name, bound)| (bound == op).then_some(name))
                .expect("an external operand comes from a variable bound to it");
            let ty = match op {
                Operand::ExternalSlot { .. } => ctx.external_slot_type(name),
                _ => ctx.external_value_type(name),
            };
            ty.expect("an external binding has a recorded type").clone()
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
/// - For f32 widening to f64: emit the F32ToF64 intrinsic
/// - For linear types (clone): emit Clone
fn lower_clone_coerce<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    cc_expr: &ast::ExprCloneCoerce,
) -> Result<ValueId, LowerError> {
    let src_type = ctx.expr_type(ctx.expr(cc_expr.operand));
    let dest_type = ctx.expr_type(expr);

    // Only the temps this operand creates are ours to drop. An enclosing
    // expression that has already lowered a sibling still needs that sibling,
    // which for `a@ op b@` is the clone the left side left behind.
    let expr_temp_mark = ctx.expr_temps_mark();

    // Use lower_operand to borrow the operand (@ doesn't consume).
    let src = lower_operand(ctx, ctx.expr(cc_expr.operand))?;

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
    // 3. f32 to f64: the F32ToF64 intrinsic
    // 4. Fixed int to Int (bigint): Widen
    // 5. Linear types: Clone
    if src_type.is_copy() && dest_type.is_copy() {
        if src_type == dest_type {
            // Same type - just copy.
            ctx.emit(Instruction::Copy { dest, src });
        } else if is_fixed_width_int(&src_type) && is_fixed_width_int(&dest_type) {
            // Fixed-width to fixed-width widening.
            ctx.emit(Instruction::WidenFixed { dest, src });
        } else if matches!((&src_type, &dest_type), (IrType::F32, IrType::F64)) {
            ctx.emit_intrinsic(dest, IntrinsicId::F32ToF64, vec![src]);
        } else {
            panic!("@ from {:?} to {:?} passed the typechecker but has no lowering", src_type, dest_type);
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
    proj: &ast::ExprFieldProj<'db>,
) -> Result<ValueId, LowerError> {
    // Lower the base expression.
    let temps_mark = ctx.expr_temps_mark();
    let base_id = lower_expression(ctx, ctx.expr(proj.base))?;
    let base_type = ctx.expr_type(ctx.expr(proj.base));

    // A base that is not a place is a value made here, and reading one field
    // out of it leaves the rest to be let go of: `f().0` takes the number out
    // of a tuple the call made and drops the string beside it. A place base
    // does not reach this -- `v.a` parses as a place with a field step, which
    // the place walker lowers -- but the guard says so rather than relying on
    // it, because dropping something a binding owns would be worse.
    if !matches!(ctx.expr(proj.base).kind(), ExprFunKind::Place(_)) {
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
    elements: &[ast::ExprId],
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
        let value = lower_expression(ctx, expr.at(*e))?;
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
