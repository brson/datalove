//! Expression checking (bidirectional typechecking).
//!
//! Provides check_expr for checking expressions against expected types,
//! and helpers for checking collection elements.
//!
//! Organization (parallel to datalit/tycheck/check.rs):
//! 1. Element checking helper - type compatibility for collection elements
//! 2. Main check_expr function - entry point for type checking
//! 3. Collection checking helpers - list, set, map, tensor, tuple, struct, enum

use rmx::prelude::*;
use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::types::*;

pub use crate::{Type, TypeError};

// ============================================================================
// Type Checking
// ============================================================================

/// Check an expression against an expected type.
pub fn check_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    expected: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    // A hint says what the expression is, and it has to be what is expected.
    // Nothing converts on the way from one to the other: a `: u8 / 1` where a
    // `u32` is wanted is a mismatch, as a `u8` variable would be, and not a
    // `u32` with a hint that went unread.
    if let Some(type_hint) = expr_kind.type_hint().cloned() {
        let hinted = ctx.convert_hint(type_hint)?;
        if !types_equivalent(db, &hinted, expected) {
            let expected_str = type_to_string(db, expected);
            let hinted_str = type_to_string(db, &hinted);
            return Err(ctx.error_type_mismatch(expr, &expected_str, &hinted_str, "the type hint disagrees with the expected type"));
        }
    }

    match expr_kind {
        // Handle None literals specially - they can check against any Option type.
        ExprFunKind::None(_lit) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // None checks against any Option<T>.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "None", "None requires Option type"))
                }
            }
        }

        // Handle Some expressions - check against Option type.
        ExprFunKind::Some(some_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                    // Check payload against inner type.
                    let expected_inner = Type::Datalit(*opt.inner_type.clone());
                    check_expr(ctx, some_expr.payload, &expected_inner)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "some", "some requires Option type"))
                }
            }
        }

        // Handle Ok expressions - check against Result type.
        ExprFunKind::Ok(ok_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                    // Check payload against inner type.
                    let expected_inner = Type::Datalit(*res.inner_type.clone());
                    check_expr(ctx, ok_expr.payload, &expected_inner)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "ok", "ok requires Result type"))
                }
            }
        }

        // Handle Er expressions - check against Result type.
        ExprFunKind::Er(er_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                    // Check payload against Error type.
                    let error_ty = Type::Datalit(datalit::tycheck::Type::Error
                    );
                    check_expr(ctx, er_expr.payload, &error_ty)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "er", "er requires Result type"))
                }
            }
        }


        // Handle table expressions - check rows against expected column types.
        ExprFunKind::Table(table_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Table(table_ty)) => {
                    refuse_building_over_a_type_param(ctx, expr, &expected)?;
                    // Check rows against expected column types.
                    check_table_rows(ctx, &table_expr.header, &table_expr.rows, table_ty)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle list expressions - check elements against expected element type.
        ExprFunKind::List(list_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::List(_)) => {
                    // Check elements against expected element type (with coercion).
                    check_list_elements(ctx, &list_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle set expressions - check elements against expected element type.
        ExprFunKind::Set(set_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Set(_)) => {
                    // Check elements against expected element type (with coercion).
                    check_set_elements(ctx, &set_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle map expressions - check entries against expected key/value types.
        ExprFunKind::Map(map_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Map(_)) => {
                    // Check entries against expected key/value types (with coercion).
                    check_map_entries(ctx, &map_expr.entries, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tensor expressions - check elements against expected element type.
        ExprFunKind::Tensor(tensor_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Tensor(_)) => {
                    refuse_building_over_a_type_param(ctx, expr, &expected)?;
                    // Check shape and elements against expected type.
                    check_tensor_shape_and_elements(ctx, tensor_expr.C(), &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tuple expressions (datalit) - check elements against expected field types.
        ExprFunKind::AnonTuple(tuple_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::AnonTuple(_)) => {
                    // Check elements against expected field types.
                    check_tuple_elements(ctx, &tuple_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tuple expressions (datafun) - check elements against expected field types.
        ExprFunKind::Tuple(tuple_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::AnonTuple(_)) => {
                    // Check elements against expected field types.
                    check_tuple_elements(ctx, &tuple_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle struct expressions - check fields against expected field types.
        ExprFunKind::AnonStruct(struct_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::AnonStruct(_)) => {
                    // Check fields against expected field types.
                    check_struct_fields(ctx, &struct_expr.fields, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle integer literals specially - they can coerce to expected integer types.
        ExprFunKind::Int(int_expr) => {
            match expected {
                Type::Datalit(expected_datalit_ty) => {
                    // An integer type takes the literal if the value fits.
                    // A float does not take one at all - there is no implicit
                    // conversion between the two families - so it falls
                    // through to the mismatch below rather than reaching a
                    // range check that has no answer for it.
                    if is_fixed_int_type(expected) || is_bigint_type(expected) {
                        // Validate the literal value fits in the expected type.
                        let value_str = int_expr.value.as_str(db);
                        check_int_fits_type(value_str, expected_datalit_ty)
                            .map_err(|_| ctx.error_literal_out_of_range(expr, false, expected_datalit_ty))?;
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // Otherwise, synthesize and compare.
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    if types_equivalent(db, &synthesized, expected) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = type_to_string(db, &synthesized);
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // A float literal checks against either float type, and nothing else.
        ExprFunKind::Float(_) => {
            if is_float_type(expected) {
                ctx.store_expr_type(expr, &expected);
                return Ok(());
            }
            let synthesized = ctx.synthesize_unhinted(expr)?;
            let expected_str = type_to_string(db, expected);
            let actual_str = type_to_string(db, &synthesized);
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
        }

        // Handle hex literals specially - they can coerce to expected integer types.
        ExprFunKind::Hex(hex_expr) => {
            match expected {
                Type::Datalit(expected_datalit_ty) => {
                    // A hex literal is an unsigned integer, or a float's bits,
                    // as it is in datalit. It does not check against a signed
                    // type, where what it spells would depend on the width.
                    if is_unsigned_int_type(expected) || is_bigint_type(expected) || is_float_type(expected) {
                        let value_str = hex_expr.value.as_str(db);
                        check_hex_fits_type(value_str, expected_datalit_ty)
                            .map_err(|_| ctx.error_literal_out_of_range(expr, true, expected_datalit_ty))?;
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // Otherwise, synthesize and compare.
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    if types_equivalent(db, &synthesized, expected) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = type_to_string(db, &synthesized);
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle unary negation against a numeric type.
        //
        // A negated literal is still a literal: the sign says nothing about
        // the width, so the expected type reaches through the negation to the
        // operand, the same as it would without one.
        ExprFunKind::UnaryOp(unary) if unary.op == UnaryOp::Neg => {
            // An integer literal is checked with its sign attached, since
            // that is what decides the range: -2147483648 is an i32 even
            // though 2147483648 is not, and -5 is no u32 even though 5 is.
            // This is how datalit reads a signed literal, which carries its
            // sign in the token rather than under an operator.
            if let (Type::Datalit(expected_datalit_ty), ExprFunKind::Int(int_expr)) =
                (expected, unary.operand.expr(db))
            {
                if is_fixed_int_type(expected) {
                    let negated = format!("-{}", int_expr.value.as_str(db));
                    check_int_fits_type(&negated, expected_datalit_ty)
                        .map_err(|_| ctx.error_literal_out_of_range(expr, false, expected_datalit_ty))?;
                    ctx.store_expr_type(unary.operand, expected);
                    ctx.store_expr_type(expr, expected);
                    return Ok(());
                }
            }
            // A float negates to its own type, so the expected width reaches
            // the operand.
            if is_float_type(expected) {
                check_expr(ctx, unary.operand, expected)?;
                ctx.store_expr_type(expr, expected);
                return Ok(());
            }
            // Anything else is negated as it is synthesized, which takes an
            // int and refuses a fixed-width integer: bare `-` is not defined
            // there, only `-?` and `-!`.
            let synthesized = ctx.synthesize_unhinted(expr)?;
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }
            let expected_str = type_to_string(db, expected);
            let actual_str = type_to_string(db, &synthesized);
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
        }

        // Handle clone/coerce operator (@) - explicit lossless conversion.
        ExprFunKind::CloneCoerce(ref cc_expr) => {
            // @ borrows its operand, so set ref_context to allow non-copy projections.
            let old_ref_context = ctx.ref_context;
            ctx.ref_context = true;
            let operand_ty = ctx.synthesize_expr(cc_expr.operand);
            ctx.ref_context = old_ref_context;
            let operand_ty = operand_ty?;

            // Check if the conversion is valid using can_clone_coerce_to.
            if can_clone_coerce_to(&operand_ty, expected, db) {
                ctx.store_expr_type(expr, expected);
                return Ok(());
            }

            // Conversion not valid - report error.
            let expected_str = type_to_string(db, expected);
            let actual_str = type_to_string(db, &operand_ty);
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "@ operator cannot convert between these types"))
        }

        // Handle binary operations - bidirectional type propagation for arithmetic.
        ExprFunKind::BinOp(ref binop) => {
            let op = binop.op;
            let is_checked = matches!(op,
                BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked);
            let is_optional = matches!(op,
                BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional);

            // For checked/optional ops, verify the function's return type is compatible.
            // Checked ops require Result return type, optional ops require Option return type.
            // If this check fails, fall through to synthesis which will produce the proper error.
            let return_type_ok = if is_checked {
                matches!(
                    ctx.expected_return_type,
                    Some(Type::Datalit(datalit::tycheck::Type::Result(_)))
                )
            } else if is_optional {
                matches!(
                    ctx.expected_return_type,
                    Some(Type::Datalit(datalit::tycheck::Type::Option(_)))
                )
            } else {
                true
            };

            // Propagate the expected type into both operands, for every
            // operator whose operands have the same type as its result. This is
            // what lets `@` appear in an operand: it needs a target to know
            // whether it clones or which type it widens to, and it has no other
            // source for one. Comparisons are absent because their result is
            // bool, which says nothing about the operands; those pair the
            // operands off against each other in synthesis instead.
            //
            // This has to admit exactly what synthesis admits, because it
            // returns without consulting it. Propagating to a type the operator
            // does not have would accept the operator for that type: checking
            // `a@ + b@` against u32 leaves both operands u32, where `@` is a
            // no-op and bare + is not allowed, and checking `a@ / b@` against
            // int leaves a bare bigint division the backends do not implement.
            // Against int, + - and * widen their operands and do exist, which
            // is the idiom the spec gives for arithmetic on fixed ints.
            let propagates = if is_checked || is_optional {
                return_type_ok
                    && (is_fixed_int_type(expected)
                        || (matches!(op, BinOp::DivChecked | BinOp::DivOptional)
                            && is_bigint_type(expected)))
            } else {
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul => {
                        is_float_type(expected) || is_bigint_type(expected)
                    }
                    BinOp::Div => is_float_type(expected),
                    BinOp::And | BinOp::Or | BinOp::Xor => is_bool_type(expected),
                    _ => false,
                }
            };

            // On failure fall through to synthesis, which reports against the
            // operands' own types. What this attempt had to say about them is
            // dropped, since it describes a path not taken.
            if propagates {
                let propagated = ctx.try_check(|ctx| {
                    let lhs_ok = check_expr(ctx, binop.lhs, expected).is_ok();
                    let rhs_ok = check_expr(ctx, binop.rhs, expected).is_ok();
                    (lhs_ok && rhs_ok).then_some(())
                });
                if propagated.is_some() {
                    ctx.store_expr_type(expr, expected);
                    return Ok(());
                }
            }

            // Fall through to default synthesis behavior.
            // Note: Bare arithmetic on fixed ints is a type error - they must use
            // @ to widen first, or use checked/optional operators (+!, +?, etc.).
            let synthesized = ctx.synthesize_unhinted(expr)?;
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }
            // No match or coercion possible - try auto-adapt or report error.
            ctx.check_type_mismatch_or_adapt(expr, expected, &synthesized, "type mismatch")
        }

        // Handle atom expressions - check against expected Atom or Enum type.
        ExprFunKind::Atom(ref atom_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Atom(expected_atom)) => {
                    if atom_expr.name == expected_atom.name {
                        ctx.store_expr_type(expr, expected);
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = format!("atom {}", atom_expr.name.as_str(db));
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "atom name mismatch"))
                    }
                }
                Type::Datalit(datalit::tycheck::Type::Enum(enum_ty)) => {
                    // Check that atom name exists as a variant with no payload.
                    let found = enum_ty.variants.iter().any(|v| {
                        v.name == atom_expr.name && v.payload.is_none()
                    });
                    if found {
                        ctx.store_expr_type(expr, expected);
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = format!("atom {}", atom_expr.name.as_str(db));
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "atom not a variant of this enum"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle term expressions - check against expected Term or Enum type.
        ExprFunKind::Term(ref term_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Term(expected_term)) => {
                    if term_expr.name == expected_term.name {
                        // Check payload against expected payload type.
                        let expected_payload = Type::Datalit(*expected_term.payload.clone());
                        check_expr(ctx, term_expr.payload, &expected_payload)?;
                        ctx.store_expr_type(expr, expected);
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = format!("term {}", term_expr.name.as_str(db));
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "term name mismatch"))
                    }
                }
                Type::Datalit(datalit::tycheck::Type::Enum(enum_ty)) => {
                    // Find variant by name with payload.
                    let variant = enum_ty.variants.iter().find(|v| v.name == term_expr.name);
                    match variant {
                        Some(v) => {
                            match &v.payload {
                                Some(payload_ty) => {
                                    let expected_payload = Type::Datalit(*payload_ty.clone());
                                    check_expr(ctx, term_expr.payload, &expected_payload)?;
                                    ctx.store_expr_type(expr, expected);
                                    Ok(())
                                }
                                None => {
                                    let expected_str = type_to_string(db, expected);
                                    let actual_str = format!("term {}", term_expr.name.as_str(db));
                                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "variant has no payload but term expression has one"))
                                }
                            }
                        }
                        None => {
                            let expected_str = type_to_string(db, expected);
                            let actual_str = format!("term {}", term_expr.name.as_str(db));
                            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "term not a variant of this enum"))
                        }
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_unhinted(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle enum literal expressions - check inner variant against expected enum.
        // A hinted expression checks its inner against the outer expectation.
        // The hint is what `synthesize` returns for it, and reaching here
        // means something outside already said what the type must be, so
        // agreement between the two is checked where the two meet.
        ExprFunKind::Hinted(ref hinted) => {
            check_expr(ctx, hinted.inner, expected)?;
            ctx.store_expr_type(expr, expected);
            Ok(())
        }

        ExprFunKind::EnumLiteral(ref enum_lit) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Enum(_)) => {
                    // Check the inner variant expression against the expected enum type.
                    check_expr(ctx, enum_lit.variant, expected)?;
                    ctx.store_expr_type(expr, expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "enum literal", "enum literal requires Enum type"))
                }
            }
        }

        // For other non-datalit expressions, use synthesis + comparison.
        // No implicit widening - numeric conversions require @.
        // A call is checked rather than only synthesized, so that what the call
        // site expects can fix a type parameter no argument reached.
        ExprFunKind::FunctionCall(call) => {
            let synthesized = crate::synthesize::synthesize_function_call_expecting(
                ctx, expr, call, Some(expected),
            )?;
            // `ctx.synthesize_expr` is what usually records this, and going
            // straight to the call skips it. Lowering reads the table and
            // panics rather than guessing.
            ctx.store_expr_type(expr, &synthesized);
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }
            ctx.check_type_mismatch_or_adapt(expr, expected, &synthesized, "type mismatch")
        }

        _ => {
            let synthesized = ctx.synthesize_unhinted(expr)?;

            // Check for exact type match first.
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }

            // No match or coercion possible - try auto-adapt or report error.
            ctx.check_type_mismatch_or_adapt(expr, expected, &synthesized, "type mismatch")
        }
    }
}

// ============================================================================
// Collection Checking Helpers
// ============================================================================

/// Check list elements against expected type.
pub fn check_list_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual list type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the list type.
    // Callers ensure expected_ty is a List type before calling.
    let expected_elem_ty = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::List(list_ty)) => {
            Type::Datalit((*list_ty.element_type).clone())
        }
        _ => unreachable!("check_list_elements called with non-list type"),
    };
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check set elements against expected type.
pub fn check_set_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual set type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the set type.
    // Callers ensure expected_ty is a Set type before calling.
    let expected_elem_ty = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::Set(set_ty)) => {
            Type::Datalit((*set_ty.element_type).clone())
        }
        _ => unreachable!("check_set_elements called with non-set type"),
    };
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check map entries against expected type.
pub fn check_map_entries<'db>(
    ctx: &mut TypeContext<'db>,
    entries: &[ExprMapEntry<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual map type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the key and value types from the map type.
    // Callers ensure expected_ty is a Map type before calling.
    let (expected_key_ty, expected_value_ty) = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::Map(map_ty)) => {
            (
                Type::Datalit((*map_ty.key_type).clone()),
                Type::Datalit((*map_ty.value_type).clone()),
            )
        }
        _ => unreachable!("check_map_entries called with non-map type"),
    };
    for entry in entries {
        check_expr(ctx, entry.key, &expected_key_ty)?;
        check_expr(ctx, entry.value, &expected_value_ty)?;
    }

    Ok(())
}

/// Check tensor shape and elements against expected type.
pub fn check_tensor_shape_and_elements<'db>(
    ctx: &mut TypeContext<'db>,
    tensor_expr: ExprTensor<'db>,
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let elements = &tensor_expr.elements;
    let shape = &tensor_expr.shape;

    // Unwrap Option/Result wrappers to get the actual tensor type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract tensor type info.
    // Callers ensure expected_ty is a Tensor type before calling.
    let (elem_type, expected_rank) = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::Tensor(tensor_ty)) => {
            (tensor_ty.element_type.clone(), tensor_ty.rank)
        }
        _ => unreachable!("check_tensor_shape_and_elements called with non-tensor type"),
    };

    // Check rank matches type hint.
    let actual_rank = shape.len() as u32;
    if actual_rank != expected_rank {
        return Err(TypeError::ArityMismatch {
            expected: expected_rank as usize,
            actual: actual_rank as usize,
        });
    }

    // Check element count matches shape product.
    datalit::tycheck::check_tensor_element_count(shape, elements.len())?;

    // Check each element against expected element type.
    let expected_elem = Type::Datalit(*elem_type);
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem)?;
    }

    Ok(())
}

/// Check tuple elements against expected type.
pub fn check_tuple_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual tuple type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the tuple type.
    // Callers ensure expected_ty is a Tuple type before calling.
    let expected_fields = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::AnonTuple(tuple_ty)) => {
            tuple_ty.fields.C()
        }
        _ => unreachable!("check_tuple_elements called with non-tuple type"),
    };

    // Check arity.
    if elements.len() != expected_fields.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_fields.len(),
            actual: elements.len(),
        });
    }

    // Check each element against expected field type using bidirectional checking.
    for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
        let expected_elem_ty = Type::Datalit(expected_field.clone(),
        );
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check struct fields against expected type.
pub fn check_struct_fields<'db>(
    ctx: &mut TypeContext<'db>,
    fields: &[ExprStructField<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual struct type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the struct type.
    // Callers ensure expected_ty is a Struct type before calling.
    let expected_fields = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::AnonStruct(struct_ty)) => {
            struct_ty.fields.C()
        }
        _ => unreachable!("check_struct_fields called with non-struct type"),
    };

    // Check arity.
    if fields.len() != expected_fields.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_fields.len(),
            actual: fields.len(),
        });
    }

    // Check each field against expected field type.
    for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
        // Check field name matches.
        if field.name != expected_field.name {
            return Err(TypeError::FieldOrderMismatch);
        }

        let expected_field_ty = Type::Datalit(*expected_field.ty.clone());
        check_expr(ctx, field.value, &expected_field_ty)?;
    }

    Ok(())
}


/// Check table rows against expected table type.
pub fn check_table_rows<'db>(
    ctx: &mut TypeContext<'db>,
    header: &[bct::text::InternedText<'db>],
    rows: &[ExprTableRow<'db>],
    table_ty: &datalit::tycheck::TypeTable<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expected_columns = &table_ty.columns;

    // Validate column count matches.
    if header.len() != expected_columns.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_columns.len(),
            actual: header.len(),
        });
    }

    // Validate column names match (in order).
    for (h, c) in header.iter().zip(expected_columns.iter()) {
        if h.as_str(db) != c.name.as_str(db) {
            return Err(TypeError::FieldOrderMismatch);
        }
    }

    // Check each row's elements against column types.
    for row in rows {
        if row.elements.len() != expected_columns.len() {
            return Err(TypeError::ArityMismatch {
                expected: expected_columns.len(),
                actual: row.elements.len(),
            });
        }

        for (elem, col) in row.elements.iter().zip(expected_columns.iter()) {
            let expected_col_ty = Type::Datalit(*col.ty.clone());
            check_expr(ctx, *elem, &expected_col_ty)?;
        }
    }

    Ok(())
}

/// Refuse to build a collection whose element type is a type parameter.
///
/// One can be taken, stored, passed on and returned, because it arrives with a
/// descriptor saying what its elements are. Making one from nothing has no
/// such descriptor to work from: an empty `[T]` inside a generic would have to
/// say what a `T` is, and nothing at hand does.
fn refuse_building_over_a_type_param<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    expected: &Type<'db>,
) -> Result<(), TypeError> {
    let Type::Datalit(dt) = expected else { return Ok(()) };
    if !datalove_datafun_common::generics::contains_type_param(dt) {
        return Ok(());
    }
    let shown = type_to_string(ctx.db, expected);
    Err(ctx.error_cannot_synthesize(expr, &format!(
        "cannot build a {shown} here: it is written over a type parameter, and \
         nothing at hand says what one is. A collection of a type parameter can \
         be taken, stored, passed on and returned, because it arrives with a \
         descriptor saying what it holds; one made here would have none",
    )))
}
