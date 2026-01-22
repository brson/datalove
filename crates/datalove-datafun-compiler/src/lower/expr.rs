//! Expression lowering.
//!
//! Transforms AST expressions into IR instructions.

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
            if let Some(operand) = ctx.lookup_var(name_str) {
                // Return the operand directly - no load needed for borrowing.
                Ok(operand)
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
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
/// For 'in' mode params, the function CONSUMES the argument (takes ownership).
/// So we don't record temps - the value is transferred to the callee.
fn lower_call_arg<'db>(
    ctx: &mut LowerCtx<'db>,
    arg: ExprFun<'db>,
    mode: ParamMode,
) -> Result<Operand, LowerError> {
    // Check if this is a ref context AND the arg is a field projection.
    if matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out) {
        if let ExprFunKind::FieldProj(proj) = arg.expr(ctx.db) {
            return lower_field_proj_as_ref(ctx, arg, proj);
        }
        // For ref/mut/out modes, use lower_operand which records temps.
        // The caller retains ownership and must drop after the call.
        return lower_operand(ctx, arg);
    }

    // For 'in' mode: function consumes the argument, so don't record temps.
    match arg.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            // Return the operand directly (Value, Slot, or Param).
            let name_str = name.text(ctx.db);
            if let Some(operand) = ctx.lookup_var(name_str) {
                Ok(operand)
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
        }
        _ => {
            // Compound expression: lower to a value without recording temps.
            // The function consumes this value, so no drop needed by caller.
            let value_id = lower_expression(ctx, arg)?;
            Ok(Operand::Value(value_id))
        }
    }
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
            if let Some(operand) = ctx.lookup_var(name_str) {
                match operand {
                    Operand::Slot(_) | Operand::Param(_) => {
                        // Use the slot/param directly - no copy needed.
                        operand
                    }
                    Operand::Value(v) => {
                        // SSA value - use as-is.
                        Operand::Value(v)
                    }
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                        // External references - use as-is.
                        operand
                    }
                }
            } else {
                return Err(LowerError::VariableNotFound(name_str.to_string()));
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
        ExprFunKind::Name(name) => {
            // For named values/params, return the operand directly to borrow.
            // This avoids the Move that lower_expression would emit for Params,
            // which would transfer ownership and leave the Param empty.
            let name_str = name.text(ctx.db);
            if let Some(operand) = ctx.lookup_var(name_str) {
                Ok(operand)
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
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
            if let Some(operand) = ctx.lookup_var(name_str) {
                match operand {
                    Operand::Value(v) => {
                        // For SSA values, just return the existing ValueId.
                        Ok(v)
                    }
                    Operand::Slot(s) => {
                        // For slots, emit a load.
                        let slot_type = ctx.expr_type(expr);
                        let is_copy = slot_type.is_copy();
                        let dest = ctx.fresh_value(slot_type);
                        ctx.emit(Instruction::SlotLoad { dest, slot: s, is_copy });
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
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
        }
        ExprFunKind::True(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(true),
            });
            Ok(dest)
        }
        ExprFunKind::False(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(false),
            });
            Ok(dest)
        }
        ExprFunKind::None(_) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapNone { dest });
            Ok(dest)
        }
        ExprFunKind::Int(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value.text(ctx.db);
            let const_value = parse_int_const(text, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::Hex(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value.text(ctx.db);
            let hex_str = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
            let const_value = parse_hex_const(hex_str, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::BinOp(binop) => {
            lower_binop(ctx, expr, binop)
        }
        ExprFunKind::UnaryOp(unary) => {
            lower_unaryop(ctx, expr, unary)
        }
        ExprFunKind::FunctionCall(call) => {
            // Resolve function reference using typechecker's resolved call target.
            let func_ref = ctx.resolve_call(call)?;

            // Get param modes from the resolved call target.
            let id = call.as_id().index() as usize;
            let target = ctx.call_targets.get(id).and_then(|t| t.as_ref());
            let param_modes: Vec<ParamMode> = target
                .map(|t| t.func(ctx.db).params(ctx.db).iter().map(|p| p.mode).collect())
                .unwrap_or_default();

            // Lower each arg, using GetFieldRef for field projections in ref context.
            let call_args = call.args(ctx.db);
            let mut args = Vec::with_capacity(call_args.len());
            for (i, arg) in call_args.iter().enumerate() {
                let mode = param_modes.get(i).copied().unwrap_or(ParamMode::In);
                let operand = lower_call_arg(ctx, *arg, mode)?;
                args.push(operand);
            }

            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            ctx.emit(Instruction::Call {
                dest,
                func: func_ref,
                args,
            });
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, some_expr.payload)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapSome {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ok_expr.payload)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapOk {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, er_expr.payload)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapErr {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::TryOption(try_expr) => {
            lower_try_option(ctx, expr, try_expr)
        }
        ExprFunKind::TryResult(try_expr) => {
            lower_try_result(ctx, expr, try_expr)
        }
        ExprFunKind::FieldProj(proj) => {
            lower_field_proj(ctx, expr, proj)
        }
        ExprFunKind::Tuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let fields = elements?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::Tuple(fields.len() as u32),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::AnonTuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let fields = elements?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::Tuple(fields.len() as u32),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::List(list) => {
            let elements: Result<Vec<_>, _> = list.elements
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::ListNew {
                dest,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Set(set) => {
            let elements: Result<Vec<_>, _> = set.elements
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::SetNew {
                dest,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Map(map) => {
            let entries: Result<Vec<_>, _> = map.entries
                .iter()
                .map(|e| {
                    let k = lower_expression(ctx, e.key)?;
                    let v = lower_expression(ctx, e.value)?;
                    Ok((Operand::Value(k), Operand::Value(v)))
                })
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::MapNew {
                dest,
                entries: entries?,
            });
            Ok(dest)
        }
        ExprFunKind::Error(err_expr) => {
            let inner_id = lower_expression(ctx, err_expr.value)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::ErrorFrom {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Data(data_expr) => {
            let inner_id = lower_expression(ctx, data_expr.value)?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::DataFrom {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Float(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value.text(ctx.db);
            let const_value = parse_float_const(text, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
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
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::String(content.to_string()),
            });
            Ok(dest)
        }
        ExprFunKind::AnonStruct(struct_expr) => {
            // Get the result type - this is IrType::Struct with sorted fields.
            let result_type = ctx.expr_type(expr);
            let sorted_field_names: Vec<String> = match &result_type {
                IrType::Struct(fields) => fields.iter().map(|(n, _)| n.clone()).collect(),
                _ => return Err(LowerError::NotImplemented(
                    format!("AnonStruct with non-struct type: {:?}", result_type)
                )),
            };

            // Lower all field expressions and collect by name.
            let mut field_values: std::collections::HashMap<String, ValueId> =
                std::collections::HashMap::new();
            for field in struct_expr.fields.iter() {
                let name = field.name.text(ctx.db).to_string();
                let value = lower_expression(ctx, field.value)?;
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
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::AnonStruct(0),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::AnonEnum(enum_expr) => {
            // Get the result type - this is IrType::Enum with sorted variants.
            let result_type = ctx.expr_type(expr);
            let variant_name = enum_expr.variant_name.text(ctx.db).to_string();

            // Find the variant index in the sorted list.
            let variant_index = match &result_type {
                IrType::Enum(variants) => {
                    variants.iter()
                        .position(|(n, _)| n == &variant_name)
                        .ok_or_else(|| LowerError::NotImplemented(
                            format!("enum variant {} not found", variant_name)
                        ))?
                }
                _ => return Err(LowerError::NotImplemented(
                    format!("AnonEnum with non-enum type: {:?}", result_type)
                )),
            };

            // Lower payload if present.
            let payload = enum_expr.payload
                .map(|p| lower_expression(ctx, p))
                .transpose()?
                .map(Operand::Value);

            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::EnumVariant {
                dest,
                variant_index: variant_index as u32,
                payload,
            });
            Ok(dest)
        }
        ExprFunKind::Tensor(tensor) => {
            let shape = tensor.shape.clone();
            let elements: Result<Vec<_>, _> = tensor.elements
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::TensorNew {
                dest,
                shape,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Table(table) => {
            // Get the table type to determine column types.
            let result_type = ctx.expr_type(expr);
            let column_types = match &result_type {
                IrType::Table(cols) => cols.clone(),
                _ => return Err(LowerError::NotImplemented("table type mismatch".to_string())),
            };

            // Each row becomes a tuple operand.
            let rows: Result<Vec<_>, _> = table.rows.iter().map(|row| {
                // Lower row elements.
                let elem_values: Result<Vec<_>, _> = row.elements.iter()
                    .map(|e| lower_expression(ctx, *e))
                    .collect();
                let elem_values = elem_values?;

                // Convert to operands.
                let elem_operands: Vec<_> = elem_values.iter()
                    .map(|&v| Operand::Value(v))
                    .collect();

                // Build tuple type for row from column types.
                let tuple_fields: Vec<_> = column_types.iter()
                    .map(|(_, ty)| (**ty).clone())
                    .collect();
                let row_type = IrType::Tuple(tuple_fields);

                // Create tuple from elements.
                let tuple_dest = ctx.fresh_value(row_type);
                ctx.emit(Instruction::Pack {
                    dest: tuple_dest,
                    ty: TypeRef::Tuple(elem_operands.len() as u32),
                    fields: elem_operands,
                });
                Ok(Operand::Value(tuple_dest))
            }).collect();

            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::TableNew {
                dest,
                rows: rows?,
            });
            Ok(dest)
        }
        ExprFunKind::ParseError(_) => {
            Err(LowerError::NotImplemented("ParseError".to_string()))
        }
        ExprFunKind::IntrinsicCall(icall) => {
            // Look up the intrinsic by name.
            let name_str = icall.name.as_str(ctx.db);
            let (intrinsic_id, _def) = datalove_datafun_intrinsics::lookup_intrinsic(name_str)
                .ok_or_else(|| LowerError::NotImplemented(format!("unknown intrinsic: {}", name_str)))?;

            // Lower arguments.
            let args: Result<Vec<_>, _> = icall.args
                .iter()
                .map(|arg| lower_operand(ctx, *arg))
                .collect();
            let args = args?;

            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            ctx.emit(Instruction::Intrinsic {
                dest,
                intrinsic: intrinsic_id,
                args,
            });

            // Drop expression temporaries after the intrinsic completes.
            ctx.emit_expr_temp_drops();

            Ok(dest)
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
    let mut lhs = lower_operand(ctx, binop.lhs)?;
    let mut rhs = lower_operand(ctx, binop.rhs)?;

    let ast_op = binop.op;
    let result_type = ctx.expr_type(expr);

    // Check if we need to widen fixed-width operands to Int.
    // This happens for bare arithmetic (+, -, *) on fixed-width ints.
    let lhs_type = ctx.expr_type(binop.lhs);
    let rhs_type = ctx.expr_type(binop.rhs);
    let needs_widening = matches!(result_type, IrType::Int)
        && is_fixed_width_int(&lhs_type)
        && is_fixed_width_int(&rhs_type);

    if needs_widening {
        // Emit Widen instructions for both operands.
        let lhs_widened = ctx.fresh_value(IrType::Int);
        ctx.emit(Instruction::Widen { dest: lhs_widened, src: lhs });
        ctx.record_expr_temp(lhs_widened, IrType::Int);
        lhs = Operand::Value(lhs_widened);

        let rhs_widened = ctx.fresh_value(IrType::Int);
        ctx.emit(Instruction::Widen { dest: rhs_widened, src: rhs });
        ctx.record_expr_temp(rhs_widened, IrType::Int);
        rhs = Operand::Value(rhs_widened);
    }

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
                ctx.emit(Instruction::Const { dest, value: const_value });
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
    ctx.emit(Instruction::WrapNone { dest: none_value });
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
    ctx.emit(Instruction::Const {
        dest: err_msg,
        value: ConstValue::String("arithmetic overflow".to_string()),
    });

    // Create Error from string.
    let err_value = ctx.fresh_value(IrType::Error);
    ctx.emit(Instruction::ErrorFrom {
        dest: err_value,
        inner: Operand::Value(err_msg),
    });

    // Wrap in Err.
    let return_type = ctx.return_type.clone()
        .expect("checked result arithmetic requires return type");
    let wrapped_err = ctx.fresh_value(return_type);
    ctx.emit(Instruction::WrapErr {
        dest: wrapped_err,
        inner: Operand::Value(err_value),
    });
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
    ctx.emit(Instruction::WrapNone { dest: none_value });
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
    ctx.emit(Instruction::Const {
        dest: err_msg,
        value: ConstValue::String("negation overflow".to_string()),
    });

    // Create Error from string.
    let err_value = ctx.fresh_value(IrType::Error);
    ctx.emit(Instruction::ErrorFrom {
        dest: err_value,
        inner: Operand::Value(err_msg),
    });

    // Wrap in Err.
    let return_type = ctx.return_type.clone()
        .expect("checked result arithmetic requires return type");
    let wrapped_err = ctx.fresh_value(return_type);
    ctx.emit(Instruction::WrapErr {
        dest: wrapped_err,
        inner: Operand::Value(err_value),
    });
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
    ctx.emit(Instruction::WrapNone { dest: none_value });
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
    ctx.emit(Instruction::WrapErr {
        dest: wrapped_err,
        inner: Operand::Value(err_dest),
    });
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

    ctx.emit(Instruction::GetField {
        dest,
        src: Operand::Value(base_id),
        field_index,
    });
    Ok(dest)
}

/// Resolve a field selector to a field index.
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
            let name_str = name.text(db);
            match base_type {
                IrType::Struct(fields) => {
                    for (i, (field_name, _)) in fields.iter().enumerate() {
                        if field_name == name_str {
                            return Ok(i as u32);
                        }
                    }
                    Err(LowerError::InvalidLiteral(format!(
                        "field '{}' not found in struct",
                        name_str
                    )))
                }
                _ => Err(LowerError::InvalidLiteral(format!(
                    "named field projection on non-struct type: {:?}",
                    base_type
                ))),
            }
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
