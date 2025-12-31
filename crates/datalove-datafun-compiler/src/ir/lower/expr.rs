//! Expression lowering.
//!
//! Transforms AST expressions into IR instructions.

use crate::ast::{self, ExprFun, ExprFunKind};
use super::super::{
    IrType, Operand, ValueId, BinOp, UnaryOp, Instruction, ConstValue, Terminator, TypeRef,
};
use super::context::LowerCtx;
use super::literal::{parse_int_const, parse_hex_const};
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
                        let dest = ctx.fresh_value(slot_type);
                        ctx.emit(Instruction::SlotLoad { dest, slot: s });
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
            let text = lit.value(ctx.db).text(ctx.db);
            let const_value = parse_int_const(text, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::Hex(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value(ctx.db).text(ctx.db);
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
            let func_name = call.name(ctx.db).text(ctx.db).to_string();
            let args: Result<Vec<_>, _> = call.args(ctx.db)
                .iter()
                .map(|arg| lower_expression(ctx, *arg).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            // Resolve function reference.
            let func_ref = ctx.lookup_func(&func_name)
                .ok_or_else(|| LowerError::FunctionNotFound(func_name))?;

            ctx.emit(Instruction::Call {
                dest,
                func: func_ref,
                args: args?,
            });
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, some_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapSome {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ok_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapOk {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, er_expr.payload(ctx.db))?;
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
        ExprFunKind::Tuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
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
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
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
            let elements: Result<Vec<_>, _> = list.elements(ctx.db)
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
            let elements: Result<Vec<_>, _> = set.elements(ctx.db)
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
            let entries: Result<Vec<_>, _> = map.entries(ctx.db)
                .iter()
                .map(|e| {
                    let k = lower_expression(ctx, e.key(ctx.db))?;
                    let v = lower_expression(ctx, e.value(ctx.db))?;
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
        ExprFunKind::Err(err_expr) => {
            let inner_id = lower_expression(ctx, err_expr.value(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::ErrorFrom {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Data(data_expr) => {
            let inner_id = lower_expression(ctx, data_expr.value(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::DataFrom {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Float(_) => {
            Err(LowerError::NotImplemented("Float".to_string()))
        }
        ExprFunKind::String(string_expr) => {
            let raw = string_expr.value(ctx.db).as_str(ctx.db);
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
        ExprFunKind::AnonStruct(_) => {
            Err(LowerError::NotImplemented("AnonStruct".to_string()))
        }
        ExprFunKind::AnonEnum(_) => {
            Err(LowerError::NotImplemented("AnonEnum".to_string()))
        }
        ExprFunKind::Tensor(_) => {
            Err(LowerError::NotImplemented("Tensor".to_string()))
        }
        ExprFunKind::ParseError(_) => {
            Err(LowerError::NotImplemented("ParseError".to_string()))
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
    let lhs = lower_operand(ctx, binop.lhs(ctx.db))?;
    let rhs = lower_operand(ctx, binop.rhs(ctx.db))?;

    let ast_op = binop.op(ctx.db);
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
    // Use lower_operand for borrowing semantics.
    let operand = lower_operand(ctx, unary.operand(ctx.db))?;
    let result_type = ctx.expr_type(expr);
    let dest = ctx.fresh_value(result_type);

    match unary.op(ctx.db) {
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
        else_block: continue_block,
    });

    // Early return block: wrap None and return.
    ctx.start_block(early_return_block);
    let return_type = ctx.return_type.clone()
        .expect("optional arithmetic requires return type");
    let none_value = ctx.fresh_value(return_type);
    ctx.emit(Instruction::WrapNone { dest: none_value });
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(none_value),
        });
    } else {
        ctx.finish_block(Terminator::TryReturn {
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
        else_block: continue_block,
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

    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(wrapped_err),
        });
    } else {
        ctx.finish_block(Terminator::TryReturn {
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
        else_block: continue_block,
    });

    // Early return block: wrap None and return.
    ctx.start_block(early_return_block);
    let return_type = ctx.return_type.clone()
        .expect("optional arithmetic requires return type");
    let none_value = ctx.fresh_value(return_type);
    ctx.emit(Instruction::WrapNone { dest: none_value });
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(none_value),
        });
    } else {
        ctx.finish_block(Terminator::TryReturn {
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
        else_block: continue_block,
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

    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(wrapped_err),
        });
    } else {
        ctx.finish_block(Terminator::TryReturn {
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
    let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
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
        else_block: early_return_block,
    });

    // Early return block: wrap None and return.
    ctx.start_block(early_return_block);
    let return_type = ctx.return_type.clone()
        .expect("try operator requires return type");
    let none_value = ctx.fresh_value(return_type);
    ctx.emit(Instruction::WrapNone { dest: none_value });
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(none_value),
        });
    } else {
        ctx.finish_block(Terminator::TryReturn {
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
    let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
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
        else_block: early_return_block,
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
    if ctx.is_script_unit {
        ctx.finish_block(Terminator::UnitEarlyReturn {
            value: Operand::Value(wrapped_err),
        });
    } else {
        ctx.finish_block(Terminator::TryReturn {
            value: Some(Operand::Value(wrapped_err)),
        });
    }

    // Continue block: ok_dest has the unwrapped Ok value.
    ctx.start_block(continue_block);
    Ok(ok_dest)
}
