//! Expression type synthesis.
//!
//! Provides synthesize_expr for inferring types from expressions.
//!
//! Organization (parallel to datalit/tycheck/synthesize.rs):
//! 1. Return type checking helpers - for try operators
//! 2. Main synthesize_expr function - entry point for type synthesis
//! 3. Operator synthesis helpers - binop, unaryop
//! 4. Call synthesis - function calls
//! 5. Try operator synthesis - ?, !
//! 6. Field projection synthesis
//! 7. Inline literal synthesis - list, set, map, tensor, tuple, struct, data, error

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::check::{check_expr, check_list_elements, check_set_elements, check_map_entries, check_tensor_shape_and_elements, check_tuple_elements, check_struct_fields, check_enum_variant, check_table_rows};
use crate::types::*;

pub use crate::{Type, TypeAndHeap, TypeError, is_copy_type};

// ============================================================================
// Operator Display Helpers
// ============================================================================

/// Convert BinOp to its source syntax.
fn binop_to_str(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        Add => "+",
        Sub => "-",
        Mul => "*",
        Div => "/",
        AddChecked => "+!",
        SubChecked => "-!",
        MulChecked => "*!",
        DivChecked => "/!",
        AddOptional => "+?",
        SubOptional => "-?",
        MulOptional => "*?",
        DivOptional => "/?",
        Lt => ".<",
        Gt => ".>",
        Le => "<=",
        Ge => ">=",
        Eq => "==",
        Ne => "!=",
        And => "and",
        Or => "or",
        Xor => "xor",
    }
}

/// Convert UnaryOp to its source syntax.
fn unaryop_to_str(op: UnaryOp) -> &'static str {
    use UnaryOp::*;
    match op {
        Neg => "-",
        NegOptional => "-?",
        NegResult => "-!",
        Not => "not",
    }
}

// ============================================================================
// Return Type Checking Helpers
// ============================================================================

/// Require that the current function returns an Option type.
///
/// Used for operators like `?`, `+?`, `-?`, `*?`, `/?`, `-?` that early-return None.
fn require_option_return_type<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    op_str: &str,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    // Scripts always have an expected return type, so this is always Some.
    let expected_return = ctx.expected_return_type
        .expect("try operator used outside function context");
    match expected_return.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(_)) => Ok(expected_return),
        _ => Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            "Option",
            &type_to_string(db, expected_return.ty(db))
        )),
    }
}

/// Require that the current function returns a Result type.
///
/// Used for operators like `!`, `+!`, `-!`, `*!`, `/!`, `-!` that early-return Err.
fn require_result_return_type<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    op_str: &str,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    // Scripts always have an expected return type, so this is always Some.
    let expected_return = ctx.expected_return_type
        .expect("try operator used outside function context");
    match expected_return.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Result(_)) => Ok(expected_return),
        _ => Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            "Result",
            &type_to_string(db, expected_return.ty(db))
        )),
    }
}

// ============================================================================
// Type Synthesis
// ============================================================================

/// Synthesize a type for an expression.
pub fn synthesize_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    match expr_kind {
        ExprFunKind::Name(name) => {
            // F001: Undefined variable.
            ctx.lookup_variable(name)
                .ok_or_else(|| ctx.error_undefined_variable(expr, name))
        }

        ExprFunKind::BinOp(ref binop) => {
            synthesize_binop(ctx, expr, binop)
        }

        ExprFunKind::UnaryOp(ref unaryop) => {
            synthesize_unaryop(ctx, expr, unaryop)
        }

        ExprFunKind::FunctionCall(call) => {
            synthesize_function_call(ctx, expr, call)
        }

        ExprFunKind::Tuple(tuple) => {
            // Synthesize type for each element.
            let elements = &tuple.elements;
            let mut datalit_element_types = Vec::new();

            for elem in elements {
                let elem_ty = ctx.synthesize_expr(*elem)?;

                // Extract datalit TypeAndHeap from datafun TypeAndHeap.
                // Tuple elements must be datalit types. Function types cannot
                // appear in expressions, so this match is exhaustive in practice.
                match elem_ty.ty(db) {
                    Type::Datalit(datalit_ty) => {
                        let datalit_elem_ty = datalit::tycheck::TypeAndHeap::new(
                            db,
                            elem_ty.heap(db),
                            datalit_ty.clone(),
                        );
                        datalit_element_types.push(datalit_elem_ty);
                    }
                    Type::Function(_) => {
                        unreachable!("function types cannot appear in tuple elements");
                    }
                }
            }

            // Create datalit tuple type.
            let datalit_tuple_ty = datalit::tycheck::Type::AnonTuple(
                datalit::tycheck::TypeAnonTuple { fields: datalit_element_types }
            );

            // Wrap in datafun type.
            // Use Heap::Omitted since tuple heap is determined by element heaps.
            let ty = Type::Datalit(datalit_tuple_ty);
            Ok(TypeAndHeap::new(db, datalit::ast::Heap::Omitted, ty))
        }

        ExprFunKind::TryOption(ref try_op) => {
            synthesize_try_option(ctx, expr, try_op)
        }

        ExprFunKind::TryResult(ref try_op) => {
            synthesize_try_result(ctx, expr, try_op)
        }

        ExprFunKind::FieldProj(ref proj) => {
            synthesize_field_proj(ctx, expr, proj)
        }

        ExprFunKind::ParseError(_) => {
            Err(ctx.error_cannot_synthesize(expr, "type inference blocked by syntax error"))
        }

        // New inline variants - simple literals.
        // All these check for type hints first.
        ExprFunKind::True(lit) => {
            if let Some(type_hint) = lit.type_hint {
                return convert_type_hint(db, type_hint);
            }
            let heap = lit.heap;
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(TypeAndHeap::new(db, heap, ty))
        }
        ExprFunKind::False(lit) => {
            if let Some(type_hint) = lit.type_hint {
                return convert_type_hint(db, type_hint);
            }
            let heap = lit.heap;
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(TypeAndHeap::new(db, heap, ty))
        }
        ExprFunKind::None(lit) => {
            // None requires type hint to determine the inner type.
            if let Some(type_hint) = lit.type_hint {
                return convert_type_hint(db, type_hint);
            }
            Err(ctx.error_cannot_synthesize(expr, "cannot infer type for None value"))
        }
        ExprFunKind::Int(int_expr) => {
            // If type hint present, use it and validate the value fits.
            if let Some(type_hint) = int_expr.type_hint {
                let result_ty = convert_type_hint(db, type_hint)?;
                // Validate integer value fits within the type.
                let value_str = int_expr.value.as_str(db);
                if let Type::Datalit(datalit_ty) = result_ty.ty(db) {
                    check_int_fits_wrapped_type(value_str, datalit_ty, db)?;
                }
                // Validate heap compatibility between type hint and expression.
                let expected_heap = unwrap_wrapper_heap(db, result_ty);
                let actual_heap = int_expr.heap;
                if !heaps_compatible(expected_heap, actual_heap) {
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(expected_heap),
                        actual_heap: heap_to_string(actual_heap),
                    });
                }
                return Ok(result_ty);
            }
            let heap = int_expr.heap;
            let value_str = int_expr.value.as_str(db);
            // Parse as u32 by default.
            if value_str.parse::<u32>().is_ok() {
                let ty = Type::Datalit(datalit::tycheck::Type::U32);
                Ok(TypeAndHeap::new(db, heap, ty))
            } else if value_str.parse::<i32>().is_ok() {
                let ty = Type::Datalit(datalit::tycheck::Type::I32);
                Ok(TypeAndHeap::new(db, heap, ty))
            } else {
                Err(ctx.error_cannot_synthesize(expr, "integer literal out of range"))
            }
        }
        ExprFunKind::Float(float_expr) => {
            if let Some(type_hint) = float_expr.type_hint {
                return convert_type_hint(db, type_hint);
            }
            let heap = float_expr.heap;
            let ty = Type::Datalit(datalit::tycheck::Type::F32);
            Ok(TypeAndHeap::new(db, heap, ty))
        }
        ExprFunKind::Hex(hex_expr) => {
            // If type hint present, use it and validate the value fits.
            if let Some(type_hint) = hex_expr.type_hint {
                let result_ty = convert_type_hint(db, type_hint)?;
                // Validate hex value fits within the type.
                let value_str = hex_expr.value.as_str(db);
                if let Type::Datalit(datalit_ty) = result_ty.ty(db) {
                    check_hex_fits_wrapped_type(value_str, datalit_ty, db)?;
                }
                // Validate heap compatibility between type hint and expression.
                let expected_heap = unwrap_wrapper_heap(db, result_ty);
                let actual_heap = hex_expr.heap;
                if !heaps_compatible(expected_heap, actual_heap) {
                    return Err(TypeError::HeapMismatch {
                        expected_heap: heap_to_string(expected_heap),
                        actual_heap: heap_to_string(actual_heap),
                    });
                }
                return Ok(result_ty);
            }
            let heap = hex_expr.heap;
            let value_str = hex_expr.value.as_str(db);
            let hex_part = value_str.trim_start_matches('-').trim_start_matches("0x").trim_start_matches("0X");
            if u32::from_str_radix(hex_part, 16).is_ok() && !value_str.starts_with('-') {
                let ty = Type::Datalit(datalit::tycheck::Type::U32);
                Ok(TypeAndHeap::new(db, heap, ty))
            } else {
                Err(ctx.error_cannot_synthesize(expr, "hex literal out of range"))
            }
        }
        ExprFunKind::String(str_expr) => {
            if let Some(type_hint) = str_expr.type_hint {
                return convert_type_hint(db, type_hint);
            }
            let heap = str_expr.heap;
            let ty = Type::Datalit(datalit::tycheck::Type::String);
            Ok(TypeAndHeap::new(db, heap, ty))
        }

        // Collection types.
        ExprFunKind::List(ref list_expr) => {
            if let Some(type_hint) = list_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches heap mismatches).
                check_list_elements(ctx, &list_expr.elements, expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_list(ctx, expr, list_expr)
        }
        ExprFunKind::Set(ref set_expr) => {
            if let Some(type_hint) = set_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches heap mismatches).
                check_set_elements(ctx, &set_expr.elements, expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_set(ctx, expr, set_expr)
        }
        ExprFunKind::Map(ref map_expr) => {
            if let Some(type_hint) = map_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check entries against expected type (catches heap mismatches).
                check_map_entries(ctx, &map_expr.entries, expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_map(ctx, expr, map_expr)
        }
        ExprFunKind::Tensor(ref tensor_expr) => {
            if let Some(type_hint) = tensor_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check rank and elements against expected type.
                check_tensor_shape_and_elements(ctx, tensor_expr.clone(), expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_tensor(ctx, expr, tensor_expr)
        }

        // Aggregate types.
        ExprFunKind::AnonTuple(ref tuple_expr) => {
            if let Some(type_hint) = tuple_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches arity mismatches).
                check_tuple_elements(ctx, &tuple_expr.elements, expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_anon_tuple(ctx, expr, tuple_expr)
        }
        ExprFunKind::AnonStruct(ref struct_expr) => {
            if let Some(type_hint) = struct_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check fields against expected type (catches arity mismatches).
                check_struct_fields(ctx, &struct_expr.fields, expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_anon_struct(ctx, expr, struct_expr)
        }
        ExprFunKind::AnonEnum(enum_expr) => {
            if let Some(type_hint) = enum_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                check_enum_variant(ctx, enum_expr.variant_name, enum_expr.payload, &expected_ty)?;
                return Ok(expected_ty);
            }
            Err(ctx.error_cannot_synthesize(expr, "anonymous enum requires type hint"))
        }

        // Wrapper types.
        ExprFunKind::Some(some_expr) => {
            if let Some(type_hint) = some_expr.type_hint {
                return convert_type_hint(db, type_hint);
            }
            // Synthesize inner type and wrap in Option.
            let payload = some_expr.payload;
            let inner_ty = ctx.synthesize_expr(payload)?;
            let heap = some_expr.heap;
            let inner_datalit = to_datalit_type_and_heap(db, inner_ty)?;
            let option_ty = datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption { inner_type: inner_datalit }
            );
            Ok(TypeAndHeap::new(db, heap, Type::Datalit(option_ty)))
        }
        ExprFunKind::Ok(ok_expr) => {
            if let Some(type_hint) = ok_expr.type_hint {
                return convert_type_hint(db, type_hint);
            }
            // Synthesize inner type and wrap in Result.
            let payload = ok_expr.payload;
            let inner_ty = ctx.synthesize_expr(payload)?;
            let heap = ok_expr.heap;
            let inner_datalit = to_datalit_type_and_heap(db, inner_ty)?;
            let result_ty = datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult { inner_type: inner_datalit }
            );
            Ok(TypeAndHeap::new(db, heap, Type::Datalit(result_ty)))
        }
        ExprFunKind::Er(er_expr) => {
            // Er requires type hint to determine the Ok type of the Result.
            if let Some(type_hint) = er_expr.type_hint {
                // Check payload against Error type.
                let error_ty = TypeAndHeap::new(
                    db,
                    datalit::ast::Heap::Omitted,
                    Type::Datalit(datalit::tycheck::Type::Error)
                );
                check_expr(ctx, er_expr.payload, error_ty)?;
                let result = convert_type_hint(db, type_hint)?;
                ctx.store_expr_type(expr, result);
                return Ok(result);
            }
            Err(ctx.error_cannot_synthesize(expr, "cannot infer type for Er value"))
        }
        ExprFunKind::Data(ref data_expr) => {
            if let Some(type_hint) = data_expr.type_hint {
                // Synthesize inner value type (Data can wrap any type).
                ctx.synthesize_expr(data_expr.value)?;
                let result = convert_type_hint(db, type_hint)?;
                ctx.store_expr_type(expr, result);
                return Ok(result);
            }
            synthesize_inline_data(ctx, expr, data_expr)
        }
        ExprFunKind::Error(ref err_expr) => {
            if let Some(type_hint) = err_expr.type_hint {
                // Synthesize inner value type (Error can wrap any type).
                ctx.synthesize_expr(err_expr.value)?;
                let result = convert_type_hint(db, type_hint)?;
                ctx.store_expr_type(expr, result);
                return Ok(result);
            }
            synthesize_inline_err(ctx, expr, err_expr)
        }

        // Table expression.
        ExprFunKind::Table(ref table_expr) => {
            if let Some(type_hint) = table_expr.type_hint {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check rows against expected table type.
                if let Type::Datalit(datalit::tycheck::Type::Table(table_ty)) = expected_ty.ty(db) {
                    check_table_rows(ctx, &table_expr.header, &table_expr.rows, table_ty)?;
                }
                return Ok(expected_ty);
            }
            // Table requires type hint - cannot infer schema.
            Err(ctx.error_cannot_synthesize(expr, "table requires type hint"))
        }
    }
}

// ============================================================================
// Operator Synthesis
// ============================================================================

/// Synthesize type for binary operation.
fn synthesize_binop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    binop: &ExprBinOp<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let op = binop.op;
    let lhs = binop.lhs;
    let rhs = binop.rhs;

    // Synthesize types for operands in ref context.
    // All binops treat their operands as ref (they don't move).
    let old_ref_context = ctx.ref_context;
    ctx.ref_context = true;
    let lhs_ty = ctx.synthesize_expr(lhs)?;
    let rhs_ty = ctx.synthesize_expr(rhs)?;
    ctx.ref_context = old_ref_context;

    // Check that operands have the same type.
    if !types_equivalent(db, lhs_ty.ty(db), rhs_ty.ty(db)) {
        return Err(ctx.error_type_mismatch(
            expr,
            &type_to_string(db, lhs_ty.ty(db)),
            &type_to_string(db, rhs_ty.ty(db)),
            "operands must have the same type"
        ));
    }

    let operand_ty = lhs_ty.ty(db);

    // Boolean logic operators require bool operands.
    if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
        if !is_bool_type(operand_ty) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                binop_to_str(op),
                &type_to_string(db, operand_ty)
            ));
        }
    } else {
        // All other operators require numeric types.
        if !is_numeric_type(operand_ty) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                binop_to_str(op),
                &type_to_string(db, operand_ty)
            ));
        }
    }

    // Determine result type based on operator.
    use BinOp::*;
    let result_ty = match op {
        // Basic arithmetic: floats, bigints, and fixed ints (which widen to int).
        Add | Sub | Mul => {
            if is_float_type(operand_ty) {
                // Floats return float.
                lhs_ty
            } else if is_bigint_type(operand_ty) {
                // Bigints return bigint.
                lhs_ty
            } else if is_fixed_int_type(operand_ty) {
                // Fixed ints widen to int.
                let int_ty = Type::Datalit(datalit::tycheck::Type::Int);
                TypeAndHeap::new(db, datalit::ast::Heap::Omitted, int_ty)
            } else {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, operand_ty)
                ));
            }
        }

        // Bare division: only floats (bigints must use /! or /?).
        Div => {
            if !is_float_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, operand_ty)
                ));
            }
            lhs_ty
        }

        // Checked arithmetic: only fixed ints, plus division for bigints.
        // Checked operators yield element type directly (not wrapped in Result).
        // On overflow, the function early-returns with an error.
        AddChecked | SubChecked | MulChecked => {
            if !is_fixed_int_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, operand_ty)
                ));
            }
            require_result_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        DivChecked => {
            if !is_fixed_int_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, operand_ty)
                ));
            }
            require_result_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        // Optional arithmetic: only fixed ints, early-returns None on overflow.
        AddOptional | SubOptional | MulOptional => {
            if !is_fixed_int_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, operand_ty)
                ));
            }
            require_option_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        DivOptional => {
            if !is_fixed_int_type(operand_ty) && !is_bigint_type(operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, operand_ty)
                ));
            }
            require_option_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        // Comparison: bool.
        Lt | Gt | Le | Ge | Eq | Ne => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, bool_ty)
        }

        // Boolean logic operators: bool -> bool.
        And | Or | Xor => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, bool_ty)
        }
    };

    Ok(result_ty)
}

/// Synthesize type for unary operation.
fn synthesize_unaryop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    unaryop: &ExprUnaryOp<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let op = unaryop.op;
    let operand = unaryop.operand;

    // Synthesize type for operand in ref context.
    // Unary operators treat their operands as ref (they don't move).
    let old_ref_context = ctx.ref_context;
    ctx.ref_context = true;
    let operand_ty = ctx.synthesize_expr(operand)?;
    ctx.ref_context = old_ref_context;
    let operand_type = operand_ty.ty(db);

    // Boolean not requires bool operand.
    if matches!(op, UnaryOp::Not) {
        if !is_bool_type(operand_type) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                unaryop_to_str(op),
                &type_to_string(db, operand_type)
            ));
        }
    } else {
        // All other unary operators require numeric types.
        if !is_numeric_type(operand_type) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                unaryop_to_str(op),
                &type_to_string(db, operand_type)
            ));
        }
    }

    // Determine result type based on operator.
    let result_ty = match op {
        // Bare negation: only floats and bigints.
        UnaryOp::Neg => {
            if !is_float_type(operand_type) && !is_bigint_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, operand_type)
                ));
            }
            operand_ty
        }

        // Optional negation: only signed fixed ints.
        // Returns element type directly; on overflow, early-returns None.
        UnaryOp::NegOptional => {
            if !is_fixed_int_type(operand_type) || is_unsigned_int_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, operand_type)
                ));
            }
            require_option_return_type(ctx, expr, unaryop_to_str(op))?;
            operand_ty
        }

        // Result negation: only fixed ints.
        // Returns element type directly; on overflow, early-returns Err.
        UnaryOp::NegResult => {
            if !is_fixed_int_type(operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, operand_type)
                ));
            }
            require_result_return_type(ctx, expr, unaryop_to_str(op))?;
            operand_ty
        }

        // Boolean not: bool -> bool.
        UnaryOp::Not => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            TypeAndHeap::new(db, datalit::ast::Heap::Omitted, bool_ty)
        }
    };

    Ok(result_ty)
}

// ============================================================================
// Call Synthesis
// ============================================================================

/// Synthesize type for function call.
fn synthesize_function_call<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    call: ExprFunctionCall<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let name = call.name(db);
    let args = call.args(db);

    // F002: Undefined function.
    let func_type = ctx.lookup_function(name)
        .ok_or_else(|| ctx.error_undefined_function(expr, name))?;

    let param_types = func_type.param_types(db);
    let param_modes = func_type.param_modes(db);
    let return_type = func_type.return_type(db);

    // F045: Function arity mismatch.
    if args.len() != param_types.len() {
        return Err(ctx.error_arity_mismatch(expr, name, param_types.len(), args.len()));
    }

    // Check each argument type, setting ref context for ref/mut/out params.
    for ((arg, expected_param_ty), mode) in args.iter().zip(param_types.iter()).zip(param_modes.iter()) {
        // Set ref context for reference parameter modes.
        let old_ref_context = ctx.ref_context;
        ctx.ref_context = matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out);
        let result = check_expr(ctx, *arg, *expected_param_ty);
        ctx.ref_context = old_ref_context;
        result?;
    }

    // Store resolved call target for interpreter.
    if let Some((func_ast, module_id)) = ctx.lookup_function_ast(name) {
        ctx.store_call_target(call, func_ast, module_id);
    }

    // Return the function's return type.
    Ok(return_type)
}

// ============================================================================
// Try Operator Synthesis
// ============================================================================

/// Synthesize type for try-option operator (?).
fn synthesize_try_option<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    try_op: &ExprTryOption<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;

    // Synthesize operand type first (to report operand errors before context errors).
    let operand_ty = ctx.synthesize_expr(try_op.operand)?;

    // Operand must be Option<T>.
    let inner_ty = match operand_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => opt.inner_type,
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "?",
                "Option",
                &type_to_string(db, operand_ty.ty(db))
            ));
        }
    };

    // Verify function returns Option type.
    require_option_return_type(ctx, expr, "?")?;

    // Return the unwrapped type T.
    let heap = inner_ty.heap(db);
    let ty = Type::Datalit(inner_ty.ty(db).clone());
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for try-result operator (!).
fn synthesize_try_result<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    try_op: &ExprTryResult<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;

    // Synthesize operand type first (to report operand errors before context errors).
    let operand_ty = ctx.synthesize_expr(try_op.operand)?;

    // Operand must be Result<T>.
    let inner_ty = match operand_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Result(res)) => res.inner_type,
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "!",
                "Result",
                &type_to_string(db, operand_ty.ty(db))
            ));
        }
    };

    // Verify function returns Result type.
    require_result_return_type(ctx, expr, "!")?;

    // Return the unwrapped type T.
    let heap = inner_ty.heap(db);
    let ty = Type::Datalit(inner_ty.ty(db).clone());
    Ok(TypeAndHeap::new(db, heap, ty))
}

// ============================================================================
// Field Projection Synthesis
// ============================================================================

/// Synthesize type for field projection expression.
fn synthesize_field_proj<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    proj: &ExprFieldProj<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;

    // Synthesize base type.
    let base_ty = ctx.synthesize_expr(proj.base)?;

    // Base must be a datalit type (tuple or struct).
    let base_datalit_ty = match base_ty.ty(db) {
        Type::Datalit(dt) => dt,
        _ => {
            return Err(TypeError::ProjectionOnNonAggregate {
                ty: type_to_string(db, base_ty.ty(db)),
            });
        }
    };

    // Extract field type based on selector.
    match &proj.field {
        FieldSelector::Index(idx) => {
            // Index projection: base must be tuple.
            match base_datalit_ty {
                datalit::tycheck::Type::AnonTuple(tuple) => {
                    let idx_usize = *idx as usize;
                    if idx_usize >= tuple.fields.len() {
                        return Err(TypeError::FieldIndexOutOfBounds {
                            index: *idx,
                            tuple_size: tuple.fields.len(),
                        });
                    }
                    let field_ty = &tuple.fields[idx_usize];

                    // Check that field is a copy type or we're in ref context.
                    // Move-type field projections are allowed in ref context.
                    if !is_copy_type(db, field_ty.ty(db)) && !ctx.ref_context {
                        return Err(TypeError::NonCopyFieldProjection {
                            field_ty: datalit::tycheck::type_to_string(db, field_ty.ty(db)),
                        });
                    }

                    let heap = field_ty.heap(db);
                    let ty = Type::Datalit(field_ty.ty(db).clone());
                    Ok(TypeAndHeap::new(db, heap, ty))
                }
                _ => {
                    Err(TypeError::ProjectionOnNonAggregate {
                        ty: type_to_string(db, base_ty.ty(db)),
                    })
                }
            }
        }
        FieldSelector::Name(name) => {
            // Named projection: base must be struct.
            match base_datalit_ty {
                datalit::tycheck::Type::AnonStruct(struct_ty) => {
                    let name_str = name.text(db);
                    for field in &struct_ty.fields {
                        if field.name.text(db) == name_str {
                            // Check that field is a copy type or we're in ref context.
                            // Move-type field projections are allowed in ref context.
                            if !is_copy_type(db, field.ty.ty(db)) && !ctx.ref_context {
                                return Err(TypeError::NonCopyFieldProjection {
                                    field_ty: datalit::tycheck::type_to_string(db, field.ty.ty(db)),
                                });
                            }

                            let heap = field.ty.heap(db);
                            let ty = Type::Datalit(field.ty.ty(db).clone());
                            return Ok(TypeAndHeap::new(db, heap, ty));
                        }
                    }
                    Err(TypeError::FieldNotFound {
                        field_name: name_str.to_string(),
                        ty: type_to_string(db, base_ty.ty(db)),
                    })
                }
                _ => {
                    Err(TypeError::ProjectionOnNonAggregate {
                        ty: type_to_string(db, base_ty.ty(db)),
                    })
                }
            }
        }
    }
}

// ============================================================================
// Inline Literal Synthesis
// ============================================================================

/// Synthesize type for inline list expression.
fn synthesize_inline_list<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    list_expr: &ExprList<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = list_expr.heap;
    let elements = &list_expr.elements;

    if elements.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_list_type(db, heap).ty(db).clone());
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    // Synthesize type of first element.
    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = to_datalit_type_and_heap(db, first_ty)?;

    // Check remaining elements for type and heap compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_compatible(db, first_ty, elem_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::List(
        datalit::tycheck::TypeList { element_type: first_datalit }
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline set expression.
fn synthesize_inline_set<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    set_expr: &ExprSet<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = set_expr.heap;
    let elements = &set_expr.elements;

    if elements.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_set_type(db, heap).ty(db).clone());
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = to_datalit_type_and_heap(db, first_ty)?;

    // Check remaining elements for type and heap compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_compatible(db, first_ty, elem_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Set(
        datalit::tycheck::TypeSet { element_type: first_datalit }
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline map expression.
fn synthesize_inline_map<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    map_expr: &ExprMap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = map_expr.heap;
    let entries = &map_expr.entries;

    if entries.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_map_type(db, heap).ty(db).clone());
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    let first_key_ty = ctx.synthesize_expr(entries[0].key)?;
    let first_key_datalit = to_datalit_type_and_heap(db, first_key_ty)?;
    let first_value_ty = ctx.synthesize_expr(entries[0].value)?;
    let first_value_datalit = to_datalit_type_and_heap(db, first_value_ty)?;

    // Check remaining entries for type and heap compatibility.
    for entry in &entries[1..] {
        let key_ty = ctx.synthesize_expr(entry.key)?;
        let value_ty = ctx.synthesize_expr(entry.value)?;
        check_element_compatible(db, first_key_ty, key_ty)?;
        check_element_compatible(db, first_value_ty, value_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Map(
        datalit::tycheck::TypeMap { key_type: first_key_datalit, value_type: first_value_datalit }
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline tensor expression.
fn synthesize_inline_tensor<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    tensor_expr: &ExprTensor<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = tensor_expr.heap;
    let shape = &tensor_expr.shape;
    let elements = &tensor_expr.elements;

    // Rank is the number of dimensions in the shape.
    let rank = shape.len() as u32;

    if elements.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_tensor_type(db, heap, rank).ty(db).clone());
        return Ok(TypeAndHeap::new(db, heap, ty));
    }

    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = to_datalit_type_and_heap(db, first_ty)?;

    // Check remaining elements for type and heap compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_compatible(db, first_ty, elem_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Tensor(
        datalit::tycheck::TypeTensor { element_type: first_datalit, rank }
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline anonymous tuple.
fn synthesize_inline_anon_tuple<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    tuple_expr: &ExprAnonTuple<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = tuple_expr.heap;
    let elements = &tuple_expr.elements;

    let mut elem_types = Vec::new();
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        let elem_datalit = to_datalit_type_and_heap(db, elem_ty)?;
        elem_types.push(elem_datalit);
    }

    let ty = Type::Datalit(datalit::tycheck::Type::AnonTuple(
        datalit::tycheck::TypeAnonTuple { fields: elem_types }
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline anonymous struct.
fn synthesize_inline_anon_struct<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    struct_expr: &ExprAnonStruct<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = struct_expr.heap;
    let fields = &struct_expr.fields;

    let mut field_types = Vec::new();
    for field in fields {
        let field_ty = ctx.synthesize_expr(field.value)?;
        let field_datalit = to_datalit_type_and_heap(db, field_ty)?;
        field_types.push(datalit::tycheck::TypeNamedField { name: field.name, ty: field_datalit });
    }

    let ty = Type::Datalit(datalit::tycheck::Type::AnonStruct(
        datalit::tycheck::TypeAnonStruct { fields: field_types }
    ));
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline data expression.
fn synthesize_inline_data<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    data_expr: &ExprData<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = data_expr.heap;
    let value = data_expr.value;

    // Type-check the inner value.
    let _ = ctx.synthesize_expr(value)?;

    // Data synthesizes to Type::Data (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Data);
    Ok(TypeAndHeap::new(db, heap, ty))
}

/// Synthesize type for inline err expression.
fn synthesize_inline_err<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    err_expr: &ExprError<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;
    let heap = err_expr.heap;
    let value = err_expr.value;

    // Type-check the inner value.
    let _ = ctx.synthesize_expr(value)?;

    // Error synthesizes to Type::Error (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Error);
    Ok(TypeAndHeap::new(db, heap, ty))
}
