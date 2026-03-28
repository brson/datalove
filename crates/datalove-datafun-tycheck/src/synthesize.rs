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

use rmx::prelude::*;
use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::check::{check_expr, check_list_elements, check_set_elements, check_map_entries, check_tensor_shape_and_elements, check_tuple_elements, check_struct_fields, check_table_rows};
use crate::types::*;

pub use crate::{Type, TypeError, is_copy_type};
use crate::types::ComptimeCallSite;
use salsa::plumbing::AsId;

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
        Lt => "<",
        Gt => ">",
        Le => "≤",
        Ge => "≥",
        Eq => "≡",
        Ne => "≢",
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
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    // Scripts always have an expected return type, so this is always Some.
    let expected_return = ctx.expected_return_type.clone()
        .expect("try operator used outside function context");
    match expected_return {
        Type::Datalit(datalit::tycheck::Type::Option(_)) => Ok(expected_return),
        _ => Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            "Option",
            &type_to_string(db, &expected_return)
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
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    // Scripts always have an expected return type, so this is always Some.
    let expected_return = ctx.expected_return_type.clone()
        .expect("try operator used outside function context");
    match expected_return {
        Type::Datalit(datalit::tycheck::Type::Result(_)) => Ok(expected_return),
        _ => Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            "Result",
            &type_to_string(db, &expected_return)
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
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    match expr_kind {
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

                // Extract datalit type from datafun Type.
                // Tuple elements must be datalit types. Function types cannot
                // appear in expressions, so this match is exhaustive in practice.
                match elem_ty {
                    Type::Datalit(datalit_ty) => {
                        datalit_element_types.push(datalit_ty.clone());
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
            let ty = Type::Datalit(datalit_tuple_ty);
            Ok(ty)
        }

        ExprFunKind::TryOption(ref try_op) => {
            synthesize_try_option(ctx, expr, try_op)
        }

        ExprFunKind::TryResult(ref try_op) => {
            synthesize_try_result(ctx, expr, try_op)
        }

        ExprFunKind::CloneCoerce(_) => {
            // The @ operator requires type context - it cannot synthesize a type.
            Err(ctx.error_cannot_synthesize(expr, "@ operator requires type context for coercion"))
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
            if let Some(type_hint) = lit.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(ty)
        }
        ExprFunKind::False(lit) => {
            if let Some(type_hint) = lit.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(ty)
        }
        ExprFunKind::None(lit) => {
            // None requires type hint to determine the inner type.
            if let Some(type_hint) = lit.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            Err(ctx.error_cannot_synthesize(expr, "cannot infer type for None value"))
        }
        ExprFunKind::Int(int_expr) => {
            // If type hint present, use it and validate the value fits.
            if let Some(type_hint) = int_expr.type_hint.clone() {
                let result_ty = convert_type_hint(db, type_hint)?;
                // Validate integer value fits within the type.
                let value_str = int_expr.value.as_str(db);
                if let Type::Datalit(ref datalit_ty) = result_ty {
                    check_int_fits_wrapped_type(value_str, datalit_ty, db)?;
                }
                return Ok(result_ty);
            }
            // Integer literals synthesize to bigint for REPL ergonomics.
            // Use type hints for fixed-width integers.
            let ty = Type::Datalit(datalit::tycheck::Type::Int);
            Ok(ty)
        }
        ExprFunKind::Float(float_expr) => {
            if let Some(type_hint) = float_expr.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            let ty = Type::Datalit(datalit::tycheck::Type::F32);
            Ok(ty)
        }
        ExprFunKind::Hex(hex_expr) => {
            // If type hint present, use it and validate the value fits.
            if let Some(type_hint) = hex_expr.type_hint.clone() {
                let result_ty = convert_type_hint(db, type_hint)?;
                // Validate hex value fits within the type.
                let value_str = hex_expr.value.as_str(db);
                if let Type::Datalit(ref datalit_ty) = result_ty {
                    check_hex_fits_wrapped_type(value_str, datalit_ty, db)?;
                }
                return Ok(result_ty);
            }
            // Hex literals synthesize to bigint for REPL ergonomics.
            // Use type hints for fixed-width integers.
            let ty = Type::Datalit(datalit::tycheck::Type::Int);
            Ok(ty)
        }
        ExprFunKind::String(str_expr) => {
            if let Some(type_hint) = str_expr.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            let ty = Type::Datalit(datalit::tycheck::Type::String);
            Ok(ty)
        }

        // Collection types.
        ExprFunKind::List(ref list_expr) => {
            if let Some(type_hint) = list_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type.
                check_list_elements(ctx, &list_expr.elements, &expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_list(ctx, expr, list_expr)
        }
        ExprFunKind::Set(ref set_expr) => {
            if let Some(type_hint) = set_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type.
                check_set_elements(ctx, &set_expr.elements, &expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_set(ctx, expr, set_expr)
        }
        ExprFunKind::Map(ref map_expr) => {
            if let Some(type_hint) = map_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check entries against expected type.
                check_map_entries(ctx, &map_expr.entries, &expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_map(ctx, expr, map_expr)
        }
        ExprFunKind::Tensor(ref tensor_expr) => {
            if let Some(type_hint) = tensor_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check rank and elements against expected type.
                check_tensor_shape_and_elements(ctx, tensor_expr.C(), &expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_tensor(ctx, expr, tensor_expr)
        }

        // Aggregate types.
        ExprFunKind::AnonTuple(ref tuple_expr) => {
            if let Some(type_hint) = tuple_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check elements against expected type (catches arity mismatches).
                check_tuple_elements(ctx, &tuple_expr.elements, &expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_anon_tuple(ctx, expr, tuple_expr)
        }
        ExprFunKind::AnonStruct(ref struct_expr) => {
            if let Some(type_hint) = struct_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check fields against expected type (catches arity mismatches).
                check_struct_fields(ctx, &struct_expr.fields, &expected_ty)?;
                return Ok(expected_ty);
            }
            synthesize_inline_anon_struct(ctx, expr, struct_expr)
        }

        // Wrapper types.
        ExprFunKind::Some(some_expr) => {
            if let Some(type_hint) = some_expr.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            // Synthesize inner type and wrap in Option.
            // Synthesized expression types are always Datalit (Function types only appear in signatures).
            let payload = some_expr.payload;
            let inner_ty = ctx.synthesize_expr(payload)?;
            let inner_datalit_ty = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
            };
            let option_ty = datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption { inner_type: Box::new(inner_datalit_ty) }
            );
            Ok(Type::Datalit(option_ty))
        }
        ExprFunKind::Ok(ok_expr) => {
            if let Some(type_hint) = ok_expr.type_hint.clone() {
                return convert_type_hint(db, type_hint);
            }
            // Synthesize inner type and wrap in Result.
            // Synthesized expression types are always Datalit (Function types only appear in signatures).
            let payload = ok_expr.payload;
            let inner_ty = ctx.synthesize_expr(payload)?;
            let inner_datalit_ty = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
            };
            let result_ty = datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult { inner_type: Box::new(inner_datalit_ty) }
            );
            Ok(Type::Datalit(result_ty))
        }
        ExprFunKind::Er(er_expr) => {
            // Er requires type hint to determine the Ok type of the Result.
            if let Some(type_hint) = er_expr.type_hint.clone() {
                // Check payload against Error type.
                let error_ty = Type::Datalit(datalit::tycheck::Type::Error
                );
                check_expr(ctx, er_expr.payload, &error_ty)?;
                let result = convert_type_hint(db, type_hint)?;
                ctx.store_expr_type(expr, &result);
                return Ok(result);
            }
            Err(ctx.error_cannot_synthesize(expr, "cannot infer type for Er value"))
        }
        ExprFunKind::Data(ref data_expr) => {
            if let Some(type_hint) = data_expr.type_hint.clone() {
                // Synthesize inner value type (Data can wrap any type).
                ctx.synthesize_expr(data_expr.value)?;
                let result = convert_type_hint(db, type_hint)?;
                ctx.store_expr_type(expr, &result);
                return Ok(result);
            }
            synthesize_inline_data(ctx, expr, data_expr)
        }
        ExprFunKind::Error(ref err_expr) => {
            if let Some(type_hint) = err_expr.type_hint.clone() {
                // Synthesize inner value type (Error can wrap any type).
                ctx.synthesize_expr(err_expr.value)?;
                let result = convert_type_hint(db, type_hint)?;
                ctx.store_expr_type(expr, &result);
                return Ok(result);
            }
            synthesize_inline_err(ctx, expr, err_expr)
        }

        // Table expression.
        ExprFunKind::Table(ref table_expr) => {
            if let Some(type_hint) = table_expr.type_hint.clone() {
                let expected_ty = convert_type_hint(db, type_hint)?;
                // Check rows against expected table type.
                if let Type::Datalit(datalit::tycheck::Type::Table(ref table_ty)) = expected_ty {
                    check_table_rows(ctx, &table_expr.header, &table_expr.rows, table_ty)?;
                }
                return Ok(expected_ty);
            }
            // Table requires type hint - cannot infer schema.
            Err(ctx.error_cannot_synthesize(expr, "table requires type hint"))
        }

        // Atom expression: synthesize standalone Atom type.
        ExprFunKind::Atom(ref atom_expr) => {
            let ty = Type::Datalit(datalit::tycheck::Type::Atom(
                datalit::tycheck::TypeAtom { name: atom_expr.name }
            ));
            ctx.store_expr_type(expr, &ty);
            Ok(ty)
        }

        // Term expression: synthesize standalone Term type.
        ExprFunKind::Term(ref term_expr) => {
            let payload_ty = ctx.synthesize_expr(term_expr.payload)?;
            let payload_datalit = match payload_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
            };
            let ty = Type::Datalit(datalit::tycheck::Type::Term(
                datalit::tycheck::TypeTerm {
                    name: term_expr.name,
                    payload: Box::new(payload_datalit),
                }
            ));
            ctx.store_expr_type(expr, &ty);
            Ok(ty)
        }

        // Enum literal: cannot synthesize (requires type context).
        ExprFunKind::EnumLiteral(_) => {
            Err(ctx.error_cannot_synthesize(expr, "enum literal requires type context"))
        }

        // Intrinsic call expression.
        ExprFunKind::IntrinsicCall(ref icall) => {
            synthesize_intrinsic_call(ctx, expr, icall)
        }

        // Bare index expression is a type error — must be wrapped in ? or !.
        ExprFunKind::Index(_) => {
            Err(ctx.error_cannot_synthesize(expr, "bare index `a[i]` requires `?` or `!` suffix"))
        }

        // Place expression with index steps (e.g., `a[i]?`, `a[i]?.field`).
        ExprFunKind::Place(ref place) => {
            synthesize_place(ctx, expr, place)
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
) -> Result<Type<'db>, TypeError> {
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
    if !types_equivalent(db, &lhs_ty, &rhs_ty) {
        return Err(ctx.error_type_mismatch(
            expr,
            &type_to_string(db, &lhs_ty),
            &type_to_string(db, &rhs_ty),
            "operands must have the same type"
        ));
    }

    let operand_ty = lhs_ty.clone();

    // Boolean logic operators require bool operands.
    if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
        if !is_bool_type(&operand_ty) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                binop_to_str(op),
                &type_to_string(db, &operand_ty)
            ));
        }
    } else {
        // All other operators require numeric types.
        if !is_numeric_type(&operand_ty) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                binop_to_str(op),
                &type_to_string(db, &operand_ty)
            ));
        }
    }

    // Determine result type based on operator.
    use BinOp::*;
    let result_ty = match op {
        // Basic arithmetic: floats and bigints only.
        // Fixed ints must use @ to widen, or use checked/optional operators.
        Add | Sub | Mul => {
            if is_float_type(&operand_ty) {
                // Floats return float.
                lhs_ty
            } else if is_bigint_type(&operand_ty) {
                // Bigints return bigint.
                lhs_ty
            } else {
                // Fixed ints and other types are not allowed.
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
        }

        // Bare division: only floats (bigints must use /! or /?).
        Div => {
            if !is_float_type(&operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
            lhs_ty
        }

        // Checked arithmetic: only fixed ints, plus division for bigints.
        // Checked operators yield element type directly (not wrapped in Result).
        // On overflow, the function early-returns with an error.
        AddChecked | SubChecked | MulChecked => {
            if !is_fixed_int_type(&operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
            require_result_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        DivChecked => {
            if !is_fixed_int_type(&operand_ty) && !is_bigint_type(&operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
            require_result_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        // Optional arithmetic: only fixed ints, early-returns None on overflow.
        AddOptional | SubOptional | MulOptional => {
            if !is_fixed_int_type(&operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
            require_option_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        DivOptional => {
            if !is_fixed_int_type(&operand_ty) && !is_bigint_type(&operand_ty) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
            require_option_return_type(ctx, expr, binop_to_str(op))?;
            lhs_ty
        }

        // Comparison: bool.
        Lt | Gt | Le | Ge | Eq | Ne => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            bool_ty
        }

        // Boolean logic operators: bool -> bool.
        And | Or | Xor => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            bool_ty
        }
    };

    Ok(result_ty)
}

/// Synthesize type for unary operation.
fn synthesize_unaryop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    unaryop: &ExprUnaryOp<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let op = unaryop.op;
    let operand = unaryop.operand;

    // Synthesize type for operand in ref context.
    // Unary operators treat their operands as ref (they don't move).
    let old_ref_context = ctx.ref_context;
    ctx.ref_context = true;
    let operand_ty = ctx.synthesize_expr(operand)?;
    ctx.ref_context = old_ref_context;
    let operand_type = operand_ty.clone();

    // Boolean not requires bool operand.
    if matches!(op, UnaryOp::Not) {
        if !is_bool_type(&operand_type) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                unaryop_to_str(op),
                &type_to_string(db, &operand_type)
            ));
        }
    } else {
        // All other unary operators require numeric types.
        if !is_numeric_type(&operand_type) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                unaryop_to_str(op),
                &type_to_string(db, &operand_type)
            ));
        }
    }

    // Determine result type based on operator.
    let result_ty = match op {
        // Bare negation: only floats and bigints.
        UnaryOp::Neg => {
            if !is_float_type(&operand_type) && !is_bigint_type(&operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, &operand_type)
                ));
            }
            operand_ty
        }

        // Optional negation: only signed fixed ints.
        // Returns element type directly; on overflow, early-returns None.
        UnaryOp::NegOptional => {
            if !is_fixed_int_type(&operand_type) || is_unsigned_int_type(&operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, &operand_type)
                ));
            }
            require_option_return_type(ctx, expr, unaryop_to_str(op))?;
            operand_ty
        }

        // Result negation: only fixed ints.
        // Returns element type directly; on overflow, early-returns Err.
        UnaryOp::NegResult => {
            if !is_fixed_int_type(&operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, &operand_type)
                ));
            }
            require_result_return_type(ctx, expr, unaryop_to_str(op))?;
            operand_ty
        }

        // Boolean not: bool -> bool.
        UnaryOp::Not => {
            let bool_ty = Type::Datalit(datalit::tycheck::Type::Bool);
            bool_ty
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
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let name = call.name(db);
    let args = call.args(db);

    // F002: Undefined function.
    let func_type = ctx.lookup_function(name)
        .ok_or_else(|| ctx.error_undefined_function(expr, name))?;

    let param_types = func_type.param_types(db);
    let param_modes = func_type.param_modes(db);
    let param_comptime = func_type.param_comptime(db);
    let return_type = func_type.return_type(db);

    // F045: Function arity mismatch.
    if args.len() != param_types.len() {
        return Err(ctx.error_arity_mismatch(expr, name, param_types.len(), args.len()));
    }

    // Collect comptime parameter indices.
    let comptime_indices: Vec<usize> = param_comptime.iter()
        .enumerate()
        .filter_map(|(i, &is_ct)| if is_ct { Some(i) } else { None })
        .collect();

    // If function has comptime params, validate and record.
    let mut comptime_arg_names = Vec::new();
    if !comptime_indices.is_empty() {
        // Register that this function has comptime params.
        ctx.comptime_registry_mut().register_comptime_func(name, comptime_indices.clone());

        // Validate each comptime argument is a const binding name.
        for &i in &comptime_indices {
            let arg = args[i];
            match validate_comptime_arg(ctx, arg, i)? {
                Some(arg_name) => comptime_arg_names.push(arg_name),
                None => {
                    // Error already recorded by validate_comptime_arg
                }
            }
        }

        // Record this call site if we successfully validated all comptime args.
        if comptime_arg_names.len() == comptime_indices.len() {
            ctx.comptime_registry_mut().record_call_site(ComptimeCallSite {
                call_expr_id: call.as_id(),
                func_name: name,
                comptime_param_indices: comptime_indices.clone(),
                comptime_arg_names,
            });
        }
    }

    // Check each argument type, setting ref/mut context for ref/mut/out params.
    for ((arg, expected_param_ty), mode) in args.iter().zip(param_types.iter()).zip(param_modes.iter()) {
        let old_ref_context = ctx.ref_context;
        let old_mut_context = ctx.mut_context;
        ctx.ref_context = matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out);
        ctx.mut_context = matches!(mode, ParamMode::Mut | ParamMode::Out);
        let result = check_expr(ctx, *arg, expected_param_ty);
        ctx.ref_context = old_ref_context;
        ctx.mut_context = old_mut_context;
        result?;
    }

    // Store resolved call target for interpreter.
    if let Some((func_ast, module_id)) = ctx.lookup_function_ast(name) {
        ctx.store_call_target(call, func_ast, module_id);
    }

    // Return the function's return type.
    Ok(return_type)
}

/// Validate that a comptime argument is a const binding name.
///
/// Returns the const binding name if valid, or None if an error was recorded.
fn validate_comptime_arg<'db>(
    ctx: &mut TypeContext<'db>,
    arg: ExprFun<'db>,
    param_idx: usize,
) -> Result<Option<bct::text::InternedText<'db>>, TypeError> {
    let db = ctx.db;

    // The argument must be a simple Name expression.
    match arg.expr(db) {
        ExprFunKind::Place(ref place) if place.steps.is_empty() => {
            let name = place.root;
            // Must be a const binding, not a let/var/parameter.
            if ctx.is_const_binding(name) {
                Ok(Some(name))
            } else {
                let reason = format!("'{}' is not a const binding", name.as_str(db));
                ctx.add_error(TypeError::ComptimeArgNotConstBinding { param_idx, reason: reason.clone() });
                // Still return Ok(None) so we can continue checking other args
                Ok(None)
            }
        }
        _ => {
            let reason = "comptime argument must be a const binding name".to_string();
            ctx.add_error(TypeError::ComptimeArgNotConstBinding { param_idx, reason });
            Ok(None)
        }
    }
}

// ============================================================================
// Intrinsic Call Synthesis
// ============================================================================

/// Synthesize type for intrinsic call.
fn synthesize_intrinsic_call<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    icall: &ExprIntrinsicCall<'db>,
) -> Result<Type<'db>, TypeError> {
    use datalove_datafun_intrinsics::lookup_intrinsic;

    let db = ctx.db;
    let name_str = icall.name.as_str(db);
    let args = &icall.args;

    // Look up intrinsic by name.
    let (intrinsic_id, intrinsic_def) = lookup_intrinsic(name_str)
        .ok_or_else(|| ctx.error_cannot_synthesize(expr, &format!("unknown intrinsic: {}", name_str)))?;

    // Check arity.
    if args.len() != intrinsic_def.params.len() {
        return Err(ctx.error_cannot_synthesize(
            expr,
            &format!("intrinsic {} expects {} arguments, got {}", name_str, intrinsic_def.params.len(), args.len())
        ));
    }

    // Check argument types.
    for (arg, expected_intrinsic_ty) in args.iter().zip(intrinsic_def.params.iter()) {
        let arg_ty = ctx.synthesize_expr(*arg)?;
        let expected_datafun_ty = intrinsic_type_to_datafun(db, *expected_intrinsic_ty);

        if !types_equivalent(db, &arg_ty, &expected_datafun_ty) {
            return Err(ctx.error_type_mismatch(
                expr,
                &type_to_string(db, &expected_datafun_ty),
                &type_to_string(db, &arg_ty),
                &format!("argument to intrinsic {}", name_str)
            ));
        }
    }

    // Store the resolved intrinsic ID for lowering.
    ctx.store_intrinsic_target(expr, intrinsic_id);

    // Return the intrinsic's return type.
    Ok(intrinsic_type_to_datafun(db, intrinsic_def.ret))
}

/// Convert intrinsic type to datafun type.
fn intrinsic_type_to_datafun<'db>(_db: &'db dyn crate::Db, ty: datalove_datafun_intrinsics::IntrinsicType) -> Type<'db> {
    use datalove_datafun_intrinsics::IntrinsicType;

    let datalit_ty = match ty {
        IntrinsicType::U8 => datalit::tycheck::Type::U8,
        IntrinsicType::I8 => datalit::tycheck::Type::I8,
        IntrinsicType::U16 => datalit::tycheck::Type::U16,
        IntrinsicType::I16 => datalit::tycheck::Type::I16,
        IntrinsicType::U32 => datalit::tycheck::Type::U32,
        IntrinsicType::I32 => datalit::tycheck::Type::I32,
        IntrinsicType::U64 => datalit::tycheck::Type::U64,
        IntrinsicType::I64 => datalit::tycheck::Type::I64,
        IntrinsicType::Index => datalit::tycheck::Type::Index,
        IntrinsicType::Offset => datalit::tycheck::Type::Offset,
        IntrinsicType::F32 => datalit::tycheck::Type::F32,
        IntrinsicType::F64 => datalit::tycheck::Type::F64,
        IntrinsicType::Bool => datalit::tycheck::Type::Bool,
    };

    Type::Datalit(datalit_ty)
}

// ============================================================================
// Try Operator Synthesis
// ============================================================================

/// Synthesize type for try-option operator (?).
fn synthesize_try_option<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    try_op: &ExprTryOption<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Special case: a[i]? — list index with option early-return.
    if let ExprFunKind::Index(ref index_expr) = try_op.operand.expr(db) {
        return synthesize_index_try_option(ctx, expr, index_expr);
    }

    // Synthesize operand type first (to report operand errors before context errors).
    let operand_ty = ctx.synthesize_expr(try_op.operand)?;

    // Operand must be Option<T>.
    let inner_ty = match operand_ty {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => opt.inner_type.clone(),
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "?",
                "Option",
                &type_to_string(db, &operand_ty)
            ));
        }
    };

    // Verify function returns Option type.
    require_option_return_type(ctx, expr, "?")?;

    // Return the unwrapped type T.
    let ty = Type::Datalit(*inner_ty);
    Ok(ty)
}

/// Synthesize type for try-result operator (!).
fn synthesize_try_result<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    try_op: &ExprTryResult<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Special case: a[i]! — list index with result early-return.
    if let ExprFunKind::Index(ref index_expr) = try_op.operand.expr(db) {
        return synthesize_index_try_result(ctx, expr, index_expr);
    }

    // Synthesize operand type first (to report operand errors before context errors).
    let operand_ty = ctx.synthesize_expr(try_op.operand)?;

    // Operand must be Result<T>.
    let inner_ty = match operand_ty {
        Type::Datalit(datalit::tycheck::Type::Result(res)) => res.inner_type.clone(),
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "!",
                "Result",
                &type_to_string(db, &operand_ty)
            ));
        }
    };

    // Verify function returns Result type.
    require_result_return_type(ctx, expr, "!")?;

    // Return the unwrapped type T.
    let ty = Type::Datalit(*inner_ty);
    Ok(ty)
}

// ============================================================================
// Index Synthesis (for list indexing via ? and !)
// ============================================================================

/// Synthesize type for `a[i]?` — list index with option early-return.
fn synthesize_index_try_option<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    index_expr: &ExprIndex<'db>,
) -> Result<Type<'db>, TypeError> {
    let element_ty = synthesize_index_common(ctx, expr, index_expr)?;

    // Verify function returns Option type.
    require_option_return_type(ctx, expr, "?")?;

    Ok(element_ty)
}

/// Synthesize type for `a[i]!` — list index with result early-return.
fn synthesize_index_try_result<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    index_expr: &ExprIndex<'db>,
) -> Result<Type<'db>, TypeError> {
    let element_ty = synthesize_index_common(ctx, expr, index_expr)?;

    // Verify function returns Result type.
    require_result_return_type(ctx, expr, "!")?;

    Ok(element_ty)
}

/// Common index validation: base must be List<T> or Map<K,V>.
///
/// For lists, index must be `index` type, returns element type T.
/// For maps, index must match key type K, returns value type V.
fn synthesize_index_common<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    index_expr: &ExprIndex<'db>,
) -> Result<Type<'db>, TypeError> {
    let base_ty = ctx.synthesize_expr(index_expr.base)?;
    let (element_ty, index_type) = resolve_index_types(ctx.db, &base_ty)
        .map_err(|msg| ctx.error_cannot_synthesize(expr, &msg))?;

    if let Err(e) = check_expr(ctx, index_expr.index, &index_type) {
        ctx.add_error(e);
    }

    // Reject view-producing index in mut/out context.
    if ctx.mut_context && is_view_producing_index(&base_ty) {
        return Err(TypeError::ViewTypeMutBinding {
            view_ty: type_to_string(ctx.db, &element_ty),
        });
    }

    Ok(element_ty)
}

// ============================================================================
// Place Expression Synthesis
// ============================================================================

/// Synthesize type for a place expression (root + steps with index operations).
fn synthesize_place<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    place: &Place<'db>,
) -> Result<Type<'db>, TypeError> {
    // Look up root variable type.
    let mut current_ty = ctx.lookup_variable(place.root)
        .ok_or_else(|| ctx.error_undefined_variable(expr, place.root))?;

    let old_ref_context = ctx.ref_context;
    let old_mut_context = ctx.mut_context;
    let step_count = place.steps.len();

    // Walk each step. Intermediate steps (all but last) are in ref context
    // because they only navigate to a location; only the final step's result
    // is consumed/borrowed per the destination context.
    for (i, step) in place.steps.iter().enumerate() {
        let is_final = i == step_count - 1;
        if !is_final {
            ctx.ref_context = true;
            ctx.mut_context = false;
        } else {
            ctx.ref_context = old_ref_context;
            ctx.mut_context = old_mut_context;
        }

        match step {
            PlaceStep::Field(field) => {
                current_ty = synthesize_place_field_step(ctx, expr, &current_ty, field)?;
            }
            PlaceStep::Index(idx) => {
                let error_mode = match idx.error_mode {
                    Some(mode) => mode,
                    None => {
                        ctx.ref_context = old_ref_context;
                        ctx.mut_context = old_mut_context;
                        return Err(ctx.error_cannot_synthesize(
                            expr,
                            "bare index `a[i]` requires `?` or `!` suffix",
                        ));
                    }
                };
                current_ty = synthesize_place_index_step(ctx, expr, &current_ty, idx)?;
                // Verify return type matches error mode.
                match error_mode {
                    IndexErrorMode::Option => {
                        require_option_return_type(ctx, expr, "?")?;
                    }
                    IndexErrorMode::Result => {
                        require_result_return_type(ctx, expr, "!")?;
                    }
                }
            }
        }
    }

    ctx.ref_context = old_ref_context;
    ctx.mut_context = old_mut_context;
    Ok(current_ty)
}

/// Resolve the type of a field selector on a base type.
///
/// Shared logic for place field steps and field projections.
pub(crate) fn resolve_field_type<'db>(
    db: &'db dyn crate::Db,
    base_ty: &Type<'db>,
    selector: &FieldSelector<'db>,
) -> Result<Type<'db>, TypeError> {
    let base_datalit_ty = match base_ty {
        Type::Datalit(dt) => dt,
        _ => {
            return Err(TypeError::ProjectionOnNonAggregate {
                ty: type_to_string(db, base_ty),
            });
        }
    };

    match selector {
        FieldSelector::Index(idx) => {
            match base_datalit_ty {
                datalit::tycheck::Type::AnonTuple(tuple) => {
                    let idx_usize = *idx as usize;
                    if idx_usize >= tuple.fields.len() {
                        return Err(TypeError::FieldIndexOutOfBounds {
                            index: *idx,
                            tuple_size: tuple.fields.len(),
                        });
                    }
                    Ok(Type::Datalit(tuple.fields[idx_usize].clone()))
                }
                _ => Err(TypeError::ProjectionOnNonAggregate {
                    ty: type_to_string(db, base_ty),
                }),
            }
        }
        FieldSelector::Name(name) => {
            match base_datalit_ty {
                datalit::tycheck::Type::AnonStruct(struct_ty) => {
                    let name_str = name.text(db);
                    for f in &struct_ty.fields {
                        if f.name.text(db) == name_str {
                            return Ok(Type::Datalit((*f.ty).clone()));
                        }
                    }
                    Err(TypeError::FieldNotFound {
                        field_name: name_str.S(),
                        ty: type_to_string(db, base_ty),
                    })
                }
                _ => Err(TypeError::ProjectionOnNonAggregate {
                    ty: type_to_string(db, base_ty),
                }),
            }
        }
    }
}

/// Resolve element type and expected index type for a collection.
///
/// Returns `(element_type, expected_index_type)`.
pub(crate) fn resolve_index_types<'db>(
    db: &'db dyn crate::Db,
    base_ty: &Type<'db>,
) -> Result<(Type<'db>, Type<'db>), String> {
    match base_ty {
        Type::Datalit(datalit::tycheck::Type::List(list)) => {
            Ok((
                Type::Datalit(*list.element_type.clone()),
                Type::Datalit(datalit::tycheck::Type::Index),
            ))
        }
        Type::Datalit(datalit::tycheck::Type::Map(map)) => {
            Ok((
                Type::Datalit(*map.value_type.clone()),
                Type::Datalit(*map.key_type.clone()),
            ))
        }
        Type::Datalit(datalit::tycheck::Type::Tensor(tensor)) => {
            let element_ty = if tensor.rank > 1 {
                Type::Datalit(datalit::tycheck::Type::Tensor(datalit::tycheck::TypeTensor {
                    element_type: tensor.element_type.clone(),
                    rank: tensor.rank - 1,
                }))
            } else {
                Type::Datalit(*tensor.element_type.clone())
            };
            Ok((
                element_ty,
                Type::Datalit(datalit::tycheck::Type::Index),
            ))
        }
        _ => Err(format!("indexing requires list, map, or tensor type, got {}", type_to_string(db, base_ty))),
    }
}

/// Returns true when indexing this type produces a view rather than
/// a direct reference. Today: only tensor with rank > 1.
pub(crate) fn is_view_producing_index(base_ty: &Type) -> bool {
    matches!(base_ty, Type::Datalit(datalit::tycheck::Type::Tensor(t)) if t.rank > 1)
}

/// Synthesize type through a field step in a place expression.
fn synthesize_place_field_step<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    base_ty: &Type<'db>,
    field: &FieldSelector<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let field_ty = resolve_field_type(db, base_ty, field)?;

    // Check that field is a copy type or we're in ref context.
    // Move-type field projections are allowed in ref context.
    if let Type::Datalit(ref dt) = field_ty {
        if !is_copy_type(db, dt) && !ctx.ref_context {
            return Err(TypeError::NonCopyFieldProjection {
                field_ty: datalit::tycheck::type_to_string(db, dt),
            });
        }
    }

    Ok(field_ty)
}

/// Synthesize type through an index step in a place expression.
fn synthesize_place_index_step<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    base_ty: &Type<'db>,
    idx: &PlaceIndex<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let (element_ty, index_type) = resolve_index_types(db, base_ty)
        .map_err(|msg| ctx.error_cannot_synthesize(expr, &msg))?;

    if let Err(e) = check_expr(ctx, idx.index, &index_type) {
        ctx.add_error(e);
    }

    // Reject view-producing index in mut/out context.
    if ctx.mut_context && is_view_producing_index(base_ty) {
        return Err(TypeError::ViewTypeMutBinding {
            view_ty: type_to_string(ctx.db, &element_ty),
        });
    }

    // Non-copy guard: indexing into a collection with non-copy element type
    // requires ref context (e.g. via @, ref param, or intermediate step).
    if let Type::Datalit(ref dt) = element_ty {
        if !is_copy_type(db, dt) && !ctx.ref_context {
            return Err(TypeError::NonCopyIndexProjection {
                elem_ty: datalit::tycheck::type_to_string(db, dt),
            });
        }
    }

    Ok(element_ty)
}

// ============================================================================
// Field Projection Synthesis
// ============================================================================

/// Synthesize type for field projection expression.
fn synthesize_field_proj<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    proj: &ExprFieldProj<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Synthesize base type.
    let base_ty = ctx.synthesize_expr(proj.base)?;

    let field_ty = resolve_field_type(db, &base_ty, &proj.field)?;

    // Check that field is a copy type or we're in ref context.
    // Move-type field projections are allowed in ref context.
    if let Type::Datalit(ref dt) = field_ty {
        if !is_copy_type(db, dt) && !ctx.ref_context {
            return Err(TypeError::NonCopyFieldProjection {
                field_ty: datalit::tycheck::type_to_string(db, dt),
            });
        }
    }

    Ok(field_ty)
}

// ============================================================================
// Inline Literal Synthesis
// ============================================================================

/// Synthesize type for inline list expression.
fn synthesize_inline_list<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    list_expr: &ExprList<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let elements = &list_expr.elements;

    if elements.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_list_type());
        return Ok(ty);
    }

    // Synthesize type of first element.
    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = match first_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining elements for type compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_compatible(db, &first_ty, &elem_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::List(
        datalit::tycheck::TypeList { element_type: Box::new(first_datalit) }
    ));
    Ok(ty)
}

/// Synthesize type for inline set expression.
fn synthesize_inline_set<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    set_expr: &ExprSet<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let elements = &set_expr.elements;

    if elements.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_set_type());
        return Ok(ty);
    }

    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = match first_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining elements for type compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_compatible(db, &first_ty, &elem_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Set(
        datalit::tycheck::TypeSet { element_type: Box::new(first_datalit) }
    ));
    Ok(ty)
}

/// Synthesize type for inline map expression.
fn synthesize_inline_map<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    map_expr: &ExprMap<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let entries = &map_expr.entries;

    if entries.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_map_type());
        return Ok(ty);
    }

    let first_key_ty = ctx.synthesize_expr(entries[0].key)?;
    let first_key_datalit = match first_key_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };
    let first_value_ty = ctx.synthesize_expr(entries[0].value)?;
    let first_value_datalit = match first_value_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining entries for type compatibility.
    for entry in &entries[1..] {
        let key_ty = ctx.synthesize_expr(entry.key)?;
        let value_ty = ctx.synthesize_expr(entry.value)?;
        check_element_compatible(db, &first_key_ty, &key_ty)?;
        check_element_compatible(db, &first_value_ty, &value_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Map(
        datalit::tycheck::TypeMap { key_type: Box::new(first_key_datalit), value_type: Box::new(first_value_datalit) }
    ));
    Ok(ty)
}

/// Synthesize type for inline tensor expression.
fn synthesize_inline_tensor<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    tensor_expr: &ExprTensor<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let shape = &tensor_expr.shape;
    let elements = &tensor_expr.elements;

    // Rank is the number of dimensions in the shape.
    let rank = shape.len() as u32;

    if elements.is_empty() {
        let ty = Type::Datalit(datalit::tycheck::empty_tensor_type(rank));
        return Ok(ty);
    }

    let first_ty = ctx.synthesize_expr(elements[0])?;
    let first_datalit = match first_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining elements for type compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_compatible(db, &first_ty, &elem_ty)?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Tensor(
        datalit::tycheck::TypeTensor { element_type: Box::new(first_datalit), rank }
    ));
    Ok(ty)
}

/// Synthesize type for inline anonymous tuple.
fn synthesize_inline_anon_tuple<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    tuple_expr: &ExprAnonTuple<'db>,
) -> Result<Type<'db>, TypeError> {
    let _db = ctx.db;
    let elements = &tuple_expr.elements;

    let mut elem_types = Vec::new();
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        let elem_datalit = match elem_ty {
            Type::Datalit(dt) => dt.clone(),
            Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
        };
        elem_types.push(elem_datalit);
    }

    let ty = Type::Datalit(datalit::tycheck::Type::AnonTuple(
        datalit::tycheck::TypeAnonTuple { fields: elem_types }
    ));
    Ok(ty)
}

/// Synthesize type for inline anonymous struct.
fn synthesize_inline_anon_struct<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    struct_expr: &ExprAnonStruct<'db>,
) -> Result<Type<'db>, TypeError> {
    let _db = ctx.db;
    let fields = &struct_expr.fields;

    let mut field_types = Vec::new();
    for field in fields {
        let field_ty = ctx.synthesize_expr(field.value)?;
        let field_datalit = match field_ty {
            Type::Datalit(dt) => dt.clone(),
            Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
        };
        field_types.push(datalit::tycheck::TypeNamedField { name: field.name, ty: Box::new(field_datalit) });
    }

    let ty = Type::Datalit(datalit::tycheck::Type::AnonStruct(
        datalit::tycheck::TypeAnonStruct { fields: field_types }
    ));
    Ok(ty)
}

/// Synthesize type for inline data expression.
fn synthesize_inline_data<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    data_expr: &ExprData<'db>,
) -> Result<Type<'db>, TypeError> {
    let value = data_expr.value;

    // Type-check the inner value.
    let _ = ctx.synthesize_expr(value)?;

    // Data synthesizes to Type::Data (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Data);
    Ok(ty)
}

/// Synthesize type for inline err expression.
fn synthesize_inline_err<'db>(
    ctx: &mut TypeContext<'db>,
    _expr: ExprFun<'db>,
    err_expr: &ExprError<'db>,
) -> Result<Type<'db>, TypeError> {
    let value = err_expr.value;

    // Type-check the inner value.
    let _ = ctx.synthesize_expr(value)?;

    // Error synthesizes to Type::Error (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Error);
    Ok(ty)
}
