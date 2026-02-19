//! Expression lowering.
//!
//! Transforms AST expressions into IR values and instructions. Handles literals,
//! operators, function calls, tuples, records, and control flow expressions.

use datalove_datafun_ast::ast::{self, ExprFun, ExprFunKind, ParamMode};
use datalove_datafun_ir::{
    IrType, Operand, ValueId, BinOp, UnaryOp, Instruction, ConstValue, Terminator, TypeRef,
};
use salsa::plumbing::AsId;
use super::context::LowerCtx;
use super::literal::{parse_int_const, parse_hex_const, parse_float_const, try_parse_negated_int_const};
use super::LowerError;

/// Lower an operand for borrowing contexts (binop, unaryop).
///
/// Returns an Operand directly:
/// - For Names bound to slots: returns Operand::Slot (no load, just borrow)
/// - For Names bound to values: returns Operand::Value
/// - For compound expressions: evaluates and returns Operand::Value(result)
pub fn lower_operand<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<Operand, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            // Check for const binding first.
            if let Some((const_type, const_value)) = ctx.lookup_const(name_str) {
                // Clone to release borrow on ctx.
                let const_type = const_type.clone();
                let const_value = const_value.clone();
                // Emit a Const instruction with the evaluated value.
                let dest = ctx.fresh_value(const_type.clone());
                ctx.emit(Instruction::Const {
                    dest,
                    value: const_value,
                });
                ctx.record_expr_temp(dest, const_type);
                return Ok(Operand::Value(dest));
            }
            let operand = ctx.lookup_var(name_str)
                .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", name_str));
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
        ExprFunKind::Name(name) => {
            // Return the operand directly (Value, Slot, or Param).
            let name_str = name.text(ctx.db);
            let operand = ctx.lookup_var(name_str)
                .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", name_str));
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
    let (index_expr, error_mode) = match arg.expr(ctx.db) {
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                (index_expr, ast::IndexErrorMode::Option)
            } else {
                return Ok(None);
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                (index_expr, ast::IndexErrorMode::Result)
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };

    let operand = lower_index_as_ref(ctx, &index_expr, error_mode)?;

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

    // For mut/out params, we need to reference the original slot/param directly,
    // not a copy. Check if base is a simple variable name bound to a slot or param.
    let src = match proj.base.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            let operand = ctx.lookup_var(name_str)
                .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", name_str));
            match operand {
                Operand::Slot(_) | Operand::Param(_) => {
                    // Use the slot/param directly - no copy needed.
                    operand
                }
                Operand::Value(v) => {
                    // SSA value - use as-is.
                    Operand::Value(v)
                }
                Operand::ValueRef(v) => {
                    // Ref value (from GetFieldRef) - use as-is.
                    Operand::ValueRef(v)
                }
                Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                    // External references - use as-is.
                    operand
                }
            }
        }
        ExprFunKind::FieldProj(inner_proj) => {
            // Nested field projection - recursively get ref to base field,
            // then dereference it for the next GetFieldRef.
            let base_ref = lower_field_proj_as_ref(ctx, proj.base, inner_proj)?;
            match base_ref {
                Operand::Value(v) => Operand::ValueRef(v),
                other => other,
            }
        }
        ExprFunKind::TryOption(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                // a[i]?.field — get ref to list element, then field ref from that.
                let ref_op = lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Option)?;
                match ref_op {
                    Operand::Value(v) => Operand::ValueRef(v),
                    other => other,
                }
            } else {
                let base_id = lower_expression(ctx, proj.base)?;
                Operand::Value(base_id)
            }
        }
        ExprFunKind::TryResult(try_op) => {
            if let ExprFunKind::Index(index_expr) = try_op.operand.expr(ctx.db) {
                // a[i]!.field — get ref to list element, then field ref from that.
                let ref_op = lower_index_as_ref(ctx, &index_expr, ast::IndexErrorMode::Result)?;
                match ref_op {
                    Operand::Value(v) => Operand::ValueRef(v),
                    other => other,
                }
            } else {
                let base_id = lower_expression(ctx, proj.base)?;
                Operand::Value(base_id)
            }
        }
        _ => {
            // Compound expression - need to lower it to a value.
            let base_id = lower_expression(ctx, proj.base)?;
            Operand::Value(base_id)
        }
    };

    // Get the field index.
    let field_index = resolve_field_index(&proj.field, &base_type, ctx.db)?;

    // Get the field type and wrap in Ref.
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
        ExprFunKind::Name(name) => {
            // For named values/params, return the operand directly to borrow.
            // This avoids the Move that lower_expression would emit for Params,
            // which would transfer ownership and leave the Param empty.
            let name_str = name.text(ctx.db);
            // Check const bindings first - these are compile-time values.
            if let Some((const_type, const_value)) = ctx.lookup_const(name_str).cloned() {
                let dest = ctx.fresh_value(const_type.clone());
                ctx.emit(Instruction::Const {
                    dest,
                    value: const_value,
                });
                ctx.record_expr_temp(dest, const_type);
                return Ok(Operand::Value(dest));
            }
            let operand = ctx.lookup_var(name_str)
                .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", name_str));
            Ok(operand)
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

/// Lower an expression, returning the ValueId holding the result.
pub fn lower_expression<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ValueId, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            // Check const bindings first - these are compile-time values.
            if let Some((const_type, const_value)) = ctx.lookup_const(name_str).cloned() {
                let dest = ctx.fresh_value(const_type);
                ctx.emit(Instruction::Const {
                    dest,
                    value: const_value,
                });
                return Ok(dest);
            }
            let operand = ctx.lookup_var(name_str)
                .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", name_str));
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
                    // The interpreter will read through the param pointer.
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
                    // External operands from previous script units.
                    // Copy types use Copy, non-copy types use Move.
                    let ext_type = ctx.expr_type(expr);
                    let dest = ctx.fresh_value(ext_type.clone());
                    if ext_type.is_copy() {
                        ctx.emit(Instruction::Copy { dest, src: operand });
                    } else {
                        ctx.emit(Instruction::Move { dest, src: operand });
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

            // Resolve function reference using typechecker's resolved call target.
            let func_ref = ctx.resolve_call(call);

            // Get param modes and types from the resolved call target.
            let id = call.as_id().index() as usize;
            let target = ctx.call_targets.get(id).and_then(|t| t.as_ref());
            let param_modes: Vec<ParamMode> = target
                .map(|t| t.func(ctx.db).params(ctx.db).iter().map(|p| p.mode).collect())
                .unwrap_or_default();
            let param_types: Vec<IrType> = target
                .map(|t| t.func(ctx.db).params(ctx.db).iter()
                    .map(|p| IrType::from_type_hint(ctx.db, &p.type_hint))
                    .collect())
                .unwrap_or_default();

            // Lower each arg, tracking in-mode args as pending intermediates.
            let call_args = call.args(ctx.db);
            let mut args = Vec::with_capacity(call_args.len());
            for (i, arg) in call_args.iter().enumerate() {
                let mode = param_modes.get(i).copied().unwrap_or(ParamMode::In);
                let arg_type = param_types.get(i);
                let operand = lower_call_arg(ctx, *arg, mode, arg_type)?;
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
            let dest = ctx.fresh_value(result_type);

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
                ctx.emit_call(dest, func_ref, args);
            }
            // Args consumed by Call/ComptimeCall; pop scope.
            ctx.clear_pending_intermediates();
            ctx.pop_pending_scope();
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
            // Push a new scope for this tuple's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            let element_types: Vec<IrType> = match &result_type {
                IrType::Tuple(types) => types.clone(),
                _ => vec![],
            };
            // Lower each element and track as pending intermediate.
            let mut fields = Vec::new();
            for (i, e) in tuple.elements.iter().enumerate() {
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
        ExprFunKind::AnonTuple(tuple) => {
            // Push a new scope for this tuple's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            let element_types: Vec<IrType> = match &result_type {
                IrType::Tuple(types) => types.clone(),
                _ => vec![],
            };
            // Lower each element and track as pending intermediate.
            let mut fields = Vec::new();
            for (i, e) in tuple.elements.iter().enumerate() {
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
        ExprFunKind::List(list) => {
            // Push a new scope for this list's pending intermediates.
            ctx.push_pending_scope();

            let result_type = ctx.expr_type(expr);
            let elem_type = match &result_type {
                IrType::List(t) => (**t).clone(),
                _ => IrType::Unit,
            };
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in list.elements.iter() {
                let value = lower_expression(ctx, *e)?;
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
            let elem_type = match &result_type {
                IrType::Set(t) => (**t).clone(),
                _ => IrType::Unit,
            };
            // Lower each element and track as pending intermediate.
            let mut elements = Vec::new();
            for e in set.elements.iter() {
                let value = lower_expression(ctx, *e)?;
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
            let (key_type, val_type) = match &result_type {
                IrType::Map(k, v) => ((**k).clone(), (**v).clone()),
                _ => (IrType::Unit, IrType::Unit),
            };
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
            ctx.emit_map_new(dest, entries);
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
            // Strip quotes if present.
            let content = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                &raw[1..raw.len()-1]
            } else {
                raw
            };
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
                _ => IrType::Unit,
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
    }
}

/// Lower a binary operation expression.
fn lower_binop<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    binop: ast::ExprBinOp<'db>,
) -> Result<ValueId, LowerError> {
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
    ctx.emit_expr_temp_drops();
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
    let return_type = ctx.return_type.clone()
        .expect("optional arithmetic requires return type");
    let none_value = ctx.fresh_value(return_type);
    ctx.emit_wrap_none(none_value);
    ctx.emit_pending_intermediate_drops();
    ctx.emit_before_try_return_drops();
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(none_value),
        });
    } else {
        ctx.finish_block(Terminator::Return {
            value: Some(Operand::Value(none_value)),
        });
    }

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

    // Create error message constant.
    let err_msg = ctx.fresh_value(IrType::String);
    ctx.emit_const(err_msg, ConstValue::String("arithmetic overflow".to_string()));

    // Create Error from string.
    let err_value = ctx.fresh_value(IrType::Error);
    ctx.emit_error_from(err_value, Operand::Value(err_msg));

    // Wrap in Err.
    let return_type = ctx.return_type.clone()
        .expect("checked result arithmetic requires return type");
    let wrapped_err = ctx.fresh_value(return_type);
    ctx.emit_wrap_err(wrapped_err, Operand::Value(err_value));
    ctx.emit_pending_intermediate_drops();
    ctx.emit_before_try_return_drops();
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(wrapped_err),
        });
    } else {
        ctx.finish_block(Terminator::Return {
            value: Some(Operand::Value(wrapped_err)),
        });
    }

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
    let return_type = ctx.return_type.clone()
        .expect("optional arithmetic requires return type");
    let none_value = ctx.fresh_value(return_type);
    ctx.emit_wrap_none(none_value);
    ctx.emit_pending_intermediate_drops();
    ctx.emit_before_try_return_drops();
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(none_value),
        });
    } else {
        ctx.finish_block(Terminator::Return {
            value: Some(Operand::Value(none_value)),
        });
    }

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

    // Create error message constant.
    let err_msg = ctx.fresh_value(IrType::String);
    ctx.emit_const(err_msg, ConstValue::String("negation overflow".to_string()));

    // Create Error from string.
    let err_value = ctx.fresh_value(IrType::Error);
    ctx.emit_error_from(err_value, Operand::Value(err_msg));

    // Wrap in Err.
    let return_type = ctx.return_type.clone()
        .expect("checked result arithmetic requires return type");
    let wrapped_err = ctx.fresh_value(return_type);
    ctx.emit_wrap_err(wrapped_err, Operand::Value(err_value));
    ctx.emit_pending_intermediate_drops();
    ctx.emit_before_try_return_drops();
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(wrapped_err),
        });
    } else {
        ctx.finish_block(Terminator::Return {
            value: Some(Operand::Value(wrapped_err)),
        });
    }

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
    let return_type = ctx.return_type.clone()
        .expect("try operator requires return type");
    let none_value = ctx.fresh_value(return_type);
    ctx.emit_wrap_none(none_value);
    ctx.emit_pending_intermediate_drops();
    ctx.emit_before_try_return_drops();
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(none_value),
        });
    } else {
        ctx.finish_block(Terminator::Return {
            value: Some(Operand::Value(none_value)),
        });
    }

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
    let return_type = ctx.return_type.clone()
        .expect("try operator requires return type");
    let wrapped_err = ctx.fresh_value(return_type);
    ctx.emit_wrap_err(wrapped_err, Operand::Value(err_dest));
    ctx.emit_pending_intermediate_drops();
    ctx.emit_before_try_return_drops();
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(wrapped_err),
        });
    } else {
        ctx.finish_block(Terminator::Return {
            value: Some(Operand::Value(wrapped_err)),
        });
    }

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

    ctx.start_block(early_return_block);
    super::stmt::emit_index_value_early_return(ctx, error_mode, is_map);

    ctx.start_block(continue_block);
    Ok(dest)
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

    // Use lower_operand to borrow the operand (@ doesn't consume).
    let src = lower_operand(ctx, cc_expr.operand)?;

    let dest = ctx.fresh_value(dest_type.clone());

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
    } else if is_fixed_width_int(&src_type) && matches!(dest_type, IrType::Int) {
        // Fixed int to Int (bigint) - widen.
        ctx.emit(Instruction::Widen { dest, src });
    } else {
        // Linear type - clone.
        ctx.emit(Instruction::Clone { dest, src });
    }

    // Drop expression temporaries after borrowing operation completes.
    ctx.emit_expr_temp_drops();

    Ok(dest)
}

/// Lower field projection expression.
fn lower_field_proj<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
    proj: ast::ExprFieldProj<'db>,
) -> Result<ValueId, LowerError> {
    // Lower the base expression.
    let base_id = lower_expression(ctx, proj.base)?;
    let base_type = ctx.expr_type(proj.base);

    // Get the field index.
    let field_index = resolve_field_index(&proj.field, &base_type, ctx.db)?;

    // Get the result type.
    let result_type = ctx.expr_type(expr);
    let dest = ctx.fresh_value(result_type);

    ctx.emit_get_field(dest, Operand::Value(base_id), field_index);
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
