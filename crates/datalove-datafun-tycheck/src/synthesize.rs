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
use datalit::tycheck::OpenLiteral;
use crate::context::{TypeContext, Wrapper};
use crate::check::check_expr;
use crate::types::*;

pub use crate::{Type, TypeError, is_copy_type};
use datalove_datafun_common::generics::{
    substitute_type_params, TypeParamBindings,
};

// ============================================================================
// Operator Display Helpers
// ============================================================================

/// Convert BinOp to its source syntax.
pub(crate) fn binop_to_str(op: BinOp) -> &'static str {
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
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    // A module-level const is outside any function, so an early-return operator
    // has nowhere to return to.
    let Some(expected_return) = ctx.expected_return_type.clone() else {
        return Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            Wrapper::Option,
            None,
        ));
    };
    match expected_return {
        Type::Datalit(datalit::tycheck::Type::Option(_)) => Ok(expected_return),
        _ => Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            Wrapper::Option,
            Some(&type_to_string(db, &expected_return)),
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
    // A module-level const is outside any function, so an early-return operator
    // has nowhere to return to.
    let Some(expected_return) = ctx.expected_return_type.clone() else {
        return Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            Wrapper::Result,
            None,
        ));
    };
    match expected_return {
        Type::Datalit(datalit::tycheck::Type::Result(_)) => Ok(expected_return),
        _ => Err(ctx.error_try_return_type_mismatch(
            expr,
            op_str,
            Wrapper::Result,
            Some(&type_to_string(db, &expected_return)),
        )),
    }
}

// ============================================================================
// Type Synthesis
// ============================================================================

/// Synthesize a type for an expression.
///
/// An expression with a hint has the hint's type, and is checked against it
/// rather than taking its word: the literal under `: u32 / true` is still a
/// `bool`.
pub fn synthesize_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
) -> Result<Type<'db>, TypeError> {
    if ctx.ref_context && builds_from_parts(expr.kind()) {
        ctx.ref_context = false;
        let result = synthesize_expr(ctx, expr);
        ctx.ref_context = true;
        return result;
    }
    if let Some(type_hint) = expr.kind().type_hint().cloned() {
        let expected = ctx.convert_hint(type_hint)?;
        check_expr(ctx, expr, &expected)?;
        ctx.store_expr_type(expr, &expected);
        return Ok(expected);
    }
    // A literal, or a value built of parts, has the type its parts are
    // inferred to have together, and is checked against it.
    if is_construction(expr.kind()) {
        let ty = inferred_type(ctx, expr)?;
        check_expr(ctx, expr, &ty)?;
        return Ok(ty);
    }
    synthesize_unhinted(ctx, expr)
}

/// Whether an expression is a literal or builds a value from parts, and so
/// takes its type from what inference finds for its parts together.
fn is_construction(kind: &ExprFunKind<'_>) -> bool {
    matches!(
        kind,
        ExprFunKind::True(_)
            | ExprFunKind::False(_)
            | ExprFunKind::None(_)
            | ExprFunKind::Int(_)
            | ExprFunKind::Float(_)
            | ExprFunKind::Hex(_)
            | ExprFunKind::String(_)
            | ExprFunKind::List(_)
            | ExprFunKind::Set(_)
            | ExprFunKind::Map(_)
            | ExprFunKind::Tensor(_)
            | ExprFunKind::Table(_)
            | ExprFunKind::Tuple(_)
            | ExprFunKind::AnonTuple(_)
            | ExprFunKind::AnonStruct(_)
            | ExprFunKind::Some(_)
            | ExprFunKind::Ok(_)
            | ExprFunKind::Er(_)
            | ExprFunKind::Atom(_)
            | ExprFunKind::Term(_)
            | ExprFunKind::EnumLiteral(_)
    )
}

/// The type inference found for an expression.
///
/// An expression that leaves a type open with nothing to decide it is
/// reported. Where inference could not read something, synthesis of the
/// expression is what reports it.
pub(crate) fn inferred_type<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
) -> Result<Type<'db>, TypeError> {
    match crate::infer::resolve(ctx, expr) {
        Ok(ty) => Ok(ty),
        Err(crate::infer::Undecided::Open(open)) => Err(report_open(ctx, open)),
        Err(crate::infer::Undecided::Unknown) => synthesize_unhinted(ctx, expr),
    }
}

/// Report an expression that leaves a type open with nothing to decide it.
fn report_open<'db>(ctx: &mut TypeContext<'db>, open: ExprFun<'db>) -> TypeError {
    let open_literal = match open.kind() {
        ExprFunKind::None(_) => OpenLiteral::None,
        ExprFunKind::Er(_) => OpenLiteral::Er,
        ExprFunKind::EnumLiteral(_) => OpenLiteral::Enum,
        ExprFunKind::List(_) => OpenLiteral::List,
        ExprFunKind::Set(_) => OpenLiteral::Set,
        ExprFunKind::Map(_) => OpenLiteral::Map,
        ExprFunKind::Tensor(_) => OpenLiteral::Tensor,
        ExprFunKind::Table(_) => OpenLiteral::Table,
        _ => unreachable!("no other expression leaves a type open"),
    };
    ctx.error_cannot_synthesize(open, open_literal.message())
}

/// Whether an expression builds a value out of the expressions inside it.
///
/// The parts are moved into what is built, whatever the context the whole is
/// in: `debuglog (p.name,)` borrows the tuple, and the tuple takes `p.name`.
/// So a borrowing context ends at one of these, and a non-copy field or
/// element inside it has to be cloned out with `@` like anywhere else it is
/// consumed. It used to carry on into the parts, which let the field be
/// taken without a clone, and dropping the tuple then freed what `p` still
/// held.
pub(crate) fn builds_from_parts(kind: &ExprFunKind<'_>) -> bool {
    matches!(
        kind,
        ExprFunKind::Tuple(_)
            | ExprFunKind::AnonTuple(_)
            | ExprFunKind::AnonStruct(_)
            | ExprFunKind::List(_)
            | ExprFunKind::Set(_)
            | ExprFunKind::Map(_)
            | ExprFunKind::Tensor(_)
            | ExprFunKind::Table(_)
            | ExprFunKind::Some(_)
            | ExprFunKind::Ok(_)
            | ExprFunKind::Er(_)
            | ExprFunKind::Data(_)
            | ExprFunKind::Error(_)
            | ExprFunKind::Term(_)
    )
}

/// Synthesize a type for an expression from what it is, leaving aside any
/// hint written on it.
///
/// Checking reaches for this when it has to know what an expression is by
/// itself, having already weighed the hint against what was expected.
pub fn synthesize_unhinted<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
) -> Result<Type<'db>, TypeError> {
    let expr_kind = expr.kind();

    match expr_kind {
        ExprFunKind::DataFile(file) => {
            crate::datafile::type_data_file(ctx, expr, file, None)
        }

        ExprFunKind::BinOp(binop) => {
            synthesize_binop(ctx, expr, binop)
        }

        ExprFunKind::UnaryOp(unaryop) => {
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
                let elem_ty = ctx.synthesize_expr(ctx.expr(*elem))?;

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

        ExprFunKind::TryOption(try_op) => {
            synthesize_try_option(ctx, expr, try_op)
        }

        ExprFunKind::TryResult(try_op) => {
            synthesize_try_result(ctx, expr, try_op)
        }

        ExprFunKind::CloneCoerce(cc_expr) => {
            // With nothing to coerce to, `@` clones, and a clone is the type
            // it was taken from. Coercion is the half that needs a target;
            // cloning does not, and `can_clone_coerce_to` says as much by
            // holding for a type and itself.
            //
            // Requiring a target either way left no way to write a clone
            // except under an annotation, so `let c = s@` was an error, and so
            // was `match h@` -- which is what the ownership analysis tells you
            // to write when a match moves what it must not.
            //
            // `@` borrows its operand, so a non-copy projection is allowed
            // under one.
            let old_ref_context = ctx.ref_context;
            ctx.ref_context = true;
            let operand_ty = ctx.synthesize_expr(ctx.expr(cc_expr.operand));
            ctx.ref_context = old_ref_context;
            let operand_ty = operand_ty?;
            ctx.store_expr_type(expr, &operand_ty);
            Ok(operand_ty)
        }

        ExprFunKind::FieldProj(proj) => {
            synthesize_field_proj(ctx, expr, proj)
        }

        ExprFunKind::ParseError(_) => {
            Err(ctx.error_cannot_synthesize(expr, "type inference blocked by syntax error"))
        }

        // New inline variants - simple literals.
        // All these check for type hints first.
        ExprFunKind::True(_) => {
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(ty)
        }
        ExprFunKind::False(_) => {
            let ty = Type::Datalit(datalit::tycheck::Type::Bool);
            Ok(ty)
        }
        ExprFunKind::None(_) => {
            Err(ctx.error_cannot_synthesize(expr, OpenLiteral::None.message()))
        }
        ExprFunKind::Int(_) => {
            // Integer literals synthesize to bigint for REPL ergonomics.
            // Use type hints for fixed-width integers.
            let ty = Type::Datalit(datalit::tycheck::Type::Int);
            Ok(ty)
        }
        ExprFunKind::Float(_) => {
            // A literal with nothing to infer from is an f64, the same way a
            // bare integer literal is an int: a default that has to be chosen
            // without knowing what it is for should be the one that keeps the
            // most of what was written. An f32 is asked for by saying so.
            let ty = Type::Datalit(datalit::tycheck::Type::F64);
            Ok(ty)
        }
        ExprFunKind::Hex(_) => {
            // Hex literals synthesize to bigint for REPL ergonomics.
            // Use type hints for fixed-width integers.
            let ty = Type::Datalit(datalit::tycheck::Type::Int);
            Ok(ty)
        }
        ExprFunKind::String(_) => {
            let ty = Type::Datalit(datalit::tycheck::Type::String);
            Ok(ty)
        }

        // Collection types.
        ExprFunKind::List(list_expr) => {
            synthesize_inline_list(ctx, expr, list_expr)
        }
        ExprFunKind::Set(set_expr) => {
            synthesize_inline_set(ctx, expr, set_expr)
        }
        ExprFunKind::Map(map_expr) => {
            synthesize_inline_map(ctx, expr, map_expr)
        }
        ExprFunKind::Tensor(tensor_expr) => {
            synthesize_inline_tensor(ctx, expr, tensor_expr)
        }

        // Aggregate types.
        ExprFunKind::AnonTuple(tuple_expr) => {
            synthesize_inline_anon_tuple(ctx, expr, tuple_expr)
        }
        ExprFunKind::AnonStruct(struct_expr) => {
            synthesize_inline_anon_struct(ctx, expr, struct_expr)
        }

        // Wrapper types.
        ExprFunKind::Some(some_expr) => {
            // Synthesize inner type and wrap in Option.
            // Synthesized expression types are always Datalit (Function types only appear in signatures).
            let payload = some_expr.payload;
            let inner_ty = ctx.synthesize_expr(ctx.expr(payload))?;
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
            // Synthesize inner type and wrap in Result.
            // Synthesized expression types are always Datalit (Function types only appear in signatures).
            let payload = ok_expr.payload;
            let inner_ty = ctx.synthesize_expr(ctx.expr(payload))?;
            let inner_datalit_ty = match inner_ty {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
            };
            let result_ty = datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult { inner_type: Box::new(inner_datalit_ty) }
            );
            Ok(Type::Datalit(result_ty))
        }
        ExprFunKind::Er(_) => {
            // `er` needs a hint to say what the result holds.
            Err(ctx.error_cannot_synthesize(expr, OpenLiteral::Er.message()))
        }
        ExprFunKind::Data(data_expr) => {
            synthesize_inline_data(ctx, expr, data_expr)
        }
        ExprFunKind::Error(err_expr) => {
            synthesize_inline_err(ctx, expr, err_expr)
        }

        // Table expression.
        ExprFunKind::Table(table_expr) => {
            synthesize_inline_table(ctx, expr, table_expr)
        }

        // Atom expression: synthesize standalone Atom type.
        ExprFunKind::Atom(atom_expr) => {
            let ty = Type::Datalit(datalit::tycheck::Type::Atom(
                datalit::tycheck::TypeAtom { name: atom_expr.name }
            ));
            ctx.store_expr_type(expr, &ty);
            Ok(ty)
        }

        // Term expression: synthesize standalone Term type.
        ExprFunKind::Term(term_expr) => {
            let payload_ty = ctx.synthesize_expr(ctx.expr(term_expr.payload))?;
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

        // What is under the hint, the hint being left aside.
        ExprFunKind::Hinted(hinted) => ctx.synthesize_expr(ctx.expr(hinted.inner)),

        // An enum literal has no type of its own.
        ExprFunKind::EnumLiteral(_) => {
            Err(ctx.error_cannot_synthesize(expr, "enum literal requires type context"))
        }

        // Intrinsic call expression.
        ExprFunKind::IntrinsicCall(icall) => {
            synthesize_intrinsic_call(ctx, expr, icall)
        }

        // Bare index expression is a type error — must be wrapped in ? or !.
        ExprFunKind::Index(_) => {
            Err(ctx.error_cannot_synthesize(expr, "bare index `a[i]` requires `?` or `!` suffix"))
        }

        // Place expression with index steps (e.g., `a[i]?`, `a[i]?.field`).
        ExprFunKind::Place(place) => {
            synthesize_place(ctx, expr, place)
        }
    }
}

// ============================================================================
// Operator Synthesis
// ============================================================================




/// The part of a type that keeps `==` from comparing it, if any.
///
/// Equality is structural: an option, tuple, struct, term or enum compares when
/// everything it holds does. A float compares by IEEE 754, so a NaN anywhere
/// inside makes two values unequal. Collections, results, `data` and `error`
/// are left out until what equality means for them is settled -- a set's
/// elements are told apart by total order, which an IEEE comparison of them
/// would contradict.
///
/// A type parameter inside an aggregate is refused too. The aggregate is then
/// carried as `data`, and the runtime only compares a `data` that is a number.
fn equality_blocker<'db>(ty: &Type<'db>) -> Option<Type<'db>> {
    use datalit::tycheck::Type as T;
    let Type::Datalit(dt) = ty else {
        return Some(ty.C());
    };
    let blocker = |inner: &T<'db>| equality_blocker(&Type::Datalit(inner.C()));
    match dt {
        T::Bool | T::U8 | T::I8 | T::U16 | T::I16 | T::U32 | T::I32 | T::U64 | T::I64
        | T::Index | T::Offset | T::F32 | T::F64 | T::Int | T::String | T::Atom(_) => None,
        T::AnonTuple(tuple) => tuple.fields.iter().find_map(blocker),
        T::AnonStruct(st) => st.fields.iter().find_map(|f| blocker(&f.ty)),
        T::Option(opt) => blocker(&opt.inner_type),
        T::Term(term) => blocker(&term.payload),
        T::Enum(en) => en.variants.iter().find_map(|v| v.payload.as_deref().and_then(blocker)),
        T::List(_) | T::Map(_) | T::Set(_) | T::Result(_) | T::Tensor(_) | T::Table(_)
        | T::Data | T::Error | T::Var(_) => Some(ty.C()),
    }
}

/// Whether arithmetic operator `op` applies to operands of type `ty`.
///
/// The one statement of which arithmetic each type has, for a binary
/// expression and a compound assignment alike. `bound` is the bound of `ty`
/// when it is a type parameter.
///
/// Floats have the bare operators, and `int` all but bare division. A
/// fixed-width integer has no bare arithmetic: it overflows, so it has the
/// checked and optional forms, which say what happens when it does. `int` has
/// those for division only, which fails on a zero divisor.
pub(crate) fn arithmetic_admits(
    op: BinOp,
    ty: &Type<'_>,
    bound: Option<datalove_datafun_ast::ast::TypeBound>,
) -> bool {
    use datalove_datafun_ast::ast::TypeBound;
    let float = bound == Some(TypeBound::Float) || is_float_type(ty);
    let fixed = bound == Some(TypeBound::FixedInt) || is_fixed_int_type(ty);
    let bigint = is_bigint_type(ty);
    use BinOp::*;
    match op {
        Add | Sub | Mul => float || bigint,
        Div => float,
        AddChecked | SubChecked | MulChecked | AddOptional | SubOptional | MulOptional => fixed,
        DivChecked | DivOptional => fixed || bigint,
        Lt | Gt | Le | Ge | Eq | Ne | And | Or | Xor => panic!("{op:?} is not arithmetic"),
    }
}

/// How an arithmetic operator leaves the function when it fails, if it can.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum EarlyReturn {
    /// With an error, for the checked operators: the function returns a result.
    Error,
    /// With none, for the optional operators: the function returns an option.
    None,
}

pub(crate) fn arithmetic_early_return(op: BinOp) -> Option<EarlyReturn> {
    use BinOp::*;
    match op {
        AddChecked | SubChecked | MulChecked | DivChecked => Some(EarlyReturn::Error),
        AddOptional | SubOptional | MulOptional | DivOptional => Some(EarlyReturn::None),
        _ => Option::None,
    }
}

fn synthesize_binop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    binop: &ExprBinOp,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let op = binop.op;
    let lhs = binop.lhs;
    let rhs = binop.rhs;

    // The operands are typed together, as a list's elements are: a literal
    // takes the type of the operand beside it, so `n == 0` with `n: u32`
    // compares two `u32`s, and with literals on both sides they take their
    // default, so `1 == 2` compares two `int`s. Each operand is then checked
    // against the type they share, which is what reads a literal as that type.
    //
    // Operands are borrowed, not moved, so they are checked in ref context.
    crate::infer::infer(ctx, expr);
    let lhs_ty = inferred_operand_type(ctx, expr.at(lhs))?;
    let rhs_ty = inferred_operand_type(ctx, expr.at(rhs))?;
    if types_equivalent(db, &lhs_ty, &rhs_ty) {
        let old_ref_context = ctx.ref_context;
        ctx.ref_context = true;
        let checked = check_expr(ctx, expr.at(lhs), &lhs_ty).and_then(|()| check_expr(ctx, expr.at(rhs), &rhs_ty));
        ctx.ref_context = old_ref_context;
        checked?;
    }


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

    // A bounded type parameter is whichever of its types the call site picks,
    // and the operators they all have go through. Which one it turns out to be
    // is read off the descriptor at run time, the same way a collection's
    // elements are.
    //
    // `float` is `f32` or `f64`, both of which take the bare arithmetic and the
    // comparisons. `fixedint` is any of the ten fixed widths, none of which has
    // bare arithmetic at all, so its parameter gets the comparisons and the
    // checked and optional forms and no more.
    let bound = operand_bound(ctx, &operand_ty);
    let float_bounded = bound == Some(datalove_datafun_ast::ast::TypeBound::Float);
    let fixedint_bounded = bound == Some(datalove_datafun_ast::ast::TypeBound::FixedInt);

    // Boolean logic operators require bool operands.
    if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
        if !is_bool_type(&operand_ty) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                binop_to_str(op),
                &type_to_string(db, &operand_ty)
            ));
        }
    } else if float_bounded || fixedint_bounded {
        // Nothing more to check: every type either bound admits is numeric.
    } else if matches!(op, BinOp::Eq | BinOp::Ne) {
        if let Some(part) = equality_blocker(&operand_ty) {
            let ty_str = type_to_string(db, &operand_ty);
            let part_str = type_to_string(db, &part);
            let note = if part_str == ty_str {
                None
            } else if matches!(part, Type::Datalit(datalit::tycheck::Type::Var(_))) {
                Some(fmt!("`{ty_str}` holds the type parameter `{part_str}`, which cannot be compared inside another type"))
            } else {
                Some(fmt!("`{ty_str}` holds `{part_str}`, which cannot be compared"))
            };
            return Err(ctx.error_invalid_operand_type_because(
                expr,
                binop_to_str(op),
                &ty_str,
                note.as_deref(),
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
        // Arithmetic yields its operands' type. Checked and optional operators
        // yield it directly too, not wrapped: on failure the function returns
        // early with an error or with none.
        Add | Sub | Mul | Div
        | AddChecked | SubChecked | MulChecked | DivChecked
        | AddOptional | SubOptional | MulOptional | DivOptional => {
            if !arithmetic_admits(op, &operand_ty, bound) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    binop_to_str(op),
                    &type_to_string(db, &operand_ty)
                ));
            }
            match arithmetic_early_return(op) {
                Some(EarlyReturn::Error) => {
                    require_result_return_type(ctx, expr, binop_to_str(op))?;
                }
                Some(EarlyReturn::None) => {
                    require_option_return_type(ctx, expr, binop_to_str(op))?;
                }
                Option::None => {}
            }
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

/// The type inferred for an operand, synthesizing it to report what
/// inference could not read.
fn inferred_operand_type<'db>(ctx: &mut TypeContext<'db>, operand: ExprFun<'db>) -> Result<Type<'db>, TypeError> {
    match crate::infer::resolve(ctx, operand) {
        Ok(ty) => Ok(ty),
        Err(crate::infer::Undecided::Open(open)) => Err(report_open(ctx, open)),
        Err(crate::infer::Undecided::Unknown) => {
            let old_ref_context = ctx.ref_context;
            ctx.ref_context = true;
            let ty = ctx.synthesize_expr(operand);
            ctx.ref_context = old_ref_context;
            ty
        }
    }
}

/// Synthesize type for unary operation.
fn synthesize_unaryop<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    unaryop: &ExprUnaryOp,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let op = unaryop.op;
    let operand = unaryop.operand;

    // Synthesize type for operand in ref context.
    // Unary operators treat their operands as ref (they don't move).
    let old_ref_context = ctx.ref_context;
    ctx.ref_context = true;
    let operand_ty = ctx.synthesize_expr(ctx.expr(operand))?;
    ctx.ref_context = old_ref_context;
    let operand_type = operand_ty.clone();

    let bound = operand_bound(ctx, &operand_type);
    let fixedint_bounded = bound == Some(datalove_datafun_ast::ast::TypeBound::FixedInt);
    let float_bounded = bound == Some(datalove_datafun_ast::ast::TypeBound::Float);

    // Boolean not requires bool operand.
    if matches!(op, UnaryOp::Not) {
        if !is_bool_type(&operand_type) {
            return Err(ctx.error_invalid_operand_type(
                expr,
                unaryop_to_str(op),
                &type_to_string(db, &operand_type)
            ));
        }
    } else if fixedint_bounded || float_bounded {
        // Every type the bound admits is numeric.
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
        // Bare negation: only floats and bigints, and a parameter bounded to
        // the floats, which all have it.
        UnaryOp::Neg => {
            if !float_bounded && !is_float_type(&operand_type) && !is_bigint_type(&operand_type) {
                return Err(ctx.error_invalid_operand_type(
                    expr,
                    unaryop_to_str(op),
                    &type_to_string(db, &operand_type)
                ));
            }
            operand_ty
        }

        // Optional negation: every fixed int, as for the binary optional
        // operators. An unsigned value negates only when it is zero.
        // Returns element type directly; on overflow, early-returns None.
        UnaryOp::NegOptional => {
            if !fixedint_bounded && !is_fixed_int_type(&operand_type) {
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
            if !fixedint_bounded && !is_fixed_int_type(&operand_type) {
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


fn synthesize_function_call<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    call: &'db ExprFunctionCall<'db>,
) -> Result<Type<'db>, TypeError> {
    synthesize_function_call_expecting(ctx, expr, call, None)
}

/// A call, with what the call site expects of its result if it knows.
///
/// The expectation is what fixes a type parameter that no argument reaches,
/// which is the only way to bind one appearing solely in the return type.
pub(crate) fn synthesize_function_call_expecting<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    call: &'db ExprFunctionCall<'db>,
    expected: Option<&Type<'db>>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let name = call.name;
    let args: Vec<ExprFun<'db>> = call.args.iter().map(|&arg| expr.at(arg)).collect();

    // F002: Undefined function. A qualified call is looked for in the module
    // its alias names, and an unqualified one among the functions in scope.
    let (func_type, func_ast) = match call.qualifier {
        Some(alias) => {
            let found = ctx.lookup_qualified(expr, alias, name)?;
            (found.func, Some((found.ast, Some(found.module_id))))
        }
        None => {
            let func_type = ctx.lookup_function(name)
                .ok_or_else(|| ctx.error_undefined_function(expr, name))?;
            (func_type, ctx.lookup_function_ast(name))
        }
    };

    let param_types = func_type.param_types(db);
    let param_modes = func_type.param_modes(db);
    let param_comptime = func_type.param_comptime(db);
    let return_type = func_type.return_type(db);

    // F045: Function arity mismatch.
    if args.len() != param_types.len() {
        return Err(ctx.error_arity_mismatch(expr, name, func_ast, param_types.len(), args.len()));
    }

    // Collect comptime parameter indices.
    let comptime_indices: Vec<usize> = param_comptime.iter()
        .enumerate()
        .filter_map(|(i, &is_ct)| if is_ct { Some(i) } else { None })
        .collect();

    // Each const argument must be the name of a const binding. Nothing is
    // recorded here: specialization reads the values it needs out of the
    // lowered IR, where the argument is an operand a `Const` defines.
    for &i in &comptime_indices {
        validate_comptime_arg(ctx, args[i], i)?;
    }

    // Every argument must carry the parameter's mode, so that mutation and
    // borrowing are visible at the call site rather than only in the callee's
    // signature. `in` is written by omitting the marker.
    let arg_modes = &call.arg_modes;
    for (i, mode) in param_modes.iter().enumerate() {
        let written = arg_modes.get(i).copied().flatten();
        if written == mode_marker(*mode) {
            continue;
        }
        let err = ctx.error_argument_mode_mismatch(
            args[i], i, mode_name(*mode), written.map_or("in", |m| mode_name(m)),
        );
        ctx.add_error(err);
    }

    // The type parameters are what inference made of them: each is a variable
    // unified with the arguments in its positions and with what the call site
    // expects of the result, so it does not matter which argument says what
    // a parameter is, and an argument that says nothing, such as `none`, takes
    // the type the others give it. Arguments are unified first, so where they
    // and the expected result disagree the arguments are what is checked.
    if let Some(expected) = expected {
        crate::infer::expect(ctx, expr, expected);
    }
    let mut bindings: TypeParamBindings<'db> = TypeParamBindings::new();
    for (param, bound_to) in crate::infer::call_type_args(ctx, expr) {
        match bound_to {
            Ok(bound_to) => {
                bindings.insert(param, bound_to);
            }
            // An argument such as `[]` that leaves the parameter open is
            // what needs a type, and is where to say so.
            Err(crate::infer::Undecided::Open(open)) => return Err(report_open(ctx, open)),
            // Nothing mentions the parameter, which is reported below.
            Err(crate::infer::Undecided::Unknown) => {}
        }
    }

    // Check each argument type, setting ref/mut context for ref/mut/out params.
    for ((arg, expected_param_ty), mode) in args.iter().zip(param_types.iter()).zip(param_modes.iter()) {
        let expected_param_ty = substitute_type_vars(expected_param_ty, &bindings);
        let old_ref_context = ctx.ref_context;
        let old_mut_context = ctx.mut_context;
        ctx.ref_context = matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out);
        ctx.mut_context = matches!(mode, ParamMode::Mut | ParamMode::Out);
        let result = check_expr(ctx, *arg, &expected_param_ty);
        ctx.ref_context = old_ref_context;
        ctx.mut_context = old_mut_context;
        result?;
    }

    let return_type = substitute_type_vars(&return_type, &bindings);

    // Store resolved call target for interpreter.
    // A bound says which types its parameter may be, and this is where that is
    // settled: the call site is what picks one.
    if let Some((func_ast, _)) = &func_ast {
        let bounds = func_ast.type_bounds();
        for (i, param) in func_ast.type_params().iter().enumerate() {
            let Some(Some(bound)) = bounds.get(i) else { continue };
            let Some(bound_to) = bindings.get(param) else { continue };
            if !bound_admits(ctx, *bound, bound_to) {
                let shown = type_to_string(db, &Type::Datalit(bound_to.clone()));
                return Err(ctx.error_type_mismatch(
                    expr, bound.as_str(), &shown,
                    "this type parameter is bounded, and that is not one of the types \
                     it admits"));
            }
        }
    }

    if let Some((func_ast, module_id)) = func_ast {
        // What the type parameters were bound to, which lowering needs in order
        // to say what descriptors the callee gets.
        //
        // A parameter no argument mentions is bound by what the call site says
        // the answer is, so `let z: T = zero()` fixes it and `self == zero()`
        // does not: an operand is read for what it is, not against what is
        // wanted. One left over is refused here rather than stood in for,
        // because standing in for it would hand the callee a descriptor for
        // `data` and it would build the wrong thing.
        let mut type_args = Vec::new();
        for param in func_ast.type_params().iter() {
            match bindings.get(param) {
                Some(bound_to) => type_args.push(bound_to.clone()),
                None => {
                    return Err(ctx.error_cannot_synthesize(expr, &format!(
                        "nothing here says what `{}` is. No argument mentions it, so \
                         the type has to come from what the answer is bound to: write \
                         `let x: <type> = {}(...)` rather than using the call where \
                         its type is only read",
                        param.text(db), name.text(db),
                    )));
                }
            }
        }
        ctx.store_call_target(expr, call, func_ast, module_id, type_args);
    }

    // Return the function's return type.
    Ok(return_type)
}

/// The bound on a type, if it is a type parameter carrying one.
///
/// Anything that is not a parameter is its own type and has no bound; anything
/// that is a parameter without one admits nothing but moving and dropping, so
/// it is `None` too and every operator refuses it.
pub(crate) fn operand_bound<'db>(
    ctx: &TypeContext<'db>,
    ty: &Type<'db>,
) -> Option<datalove_datafun_ast::ast::TypeBound> {
    match ty {
        Type::Datalit(datalit::tycheck::Type::Var(name)) =>
            ctx.type_param_bounds.get(name).copied(),
        _ => None,
    }
}

/// Whether a bound admits a type.
///
/// `float` is `f32` or `f64` and nothing else. A bare integer literal is `int`
/// here, which is not a float, so `lerp(0, 1, 0.5)` is refused for its first
/// two arguments rather than quietly widening them.
fn bound_admits<'db>(
    ctx: &TypeContext<'db>,
    bound: datalove_datafun_ast::ast::TypeBound,
    ty: &datalit::tycheck::Type<'db>,
) -> bool {
    use datalove_datafun_ast::ast::TypeBound;
    // A bounded generic handing its own parameter to another one satisfies the
    // bound by carrying it: the caller of the outer function already picked a
    // type that admits it, and the inner one gets whatever that was.
    if let datalit::tycheck::Type::Var(name) = ty {
        return ctx.type_param_bounds.get(name) == Some(&bound);
    }
    match bound {
        TypeBound::Float => matches!(ty, datalit::tycheck::Type::F32 | datalit::tycheck::Type::F64),
        TypeBound::FixedInt => datalit::tycheck::is_fixed_int_type(ty),
        // Every type reaching here is one the runtime can order, because a
        // type parameter is bound to a data type and every data type has a
        // total order. A function type is not a data type and cannot be
        // written where this would be asked.
        TypeBound::Ord => true,
    }
}

/// Replace a bound type parameter with what the call site bound it to.
///
/// A parameter may sit at any depth, so this rebuilds the type around it. A
/// function type has no type parameters inside it to replace.
fn substitute_type_vars<'db>(
    ty: &Type<'db>,
    bindings: &TypeParamBindings<'db>,
) -> Type<'db> {
    match ty {
        Type::Datalit(dt) => Type::Datalit(substitute_type_params(dt, bindings)),
        Type::Function(_) => ty.clone(),
    }
}

/// The marker a parameter mode requires at a call site.
///
/// `in` is written by omitting the marker, so it maps to `None`.
fn mode_marker(mode: ParamMode) -> Option<ParamMode> {
    match mode {
        ParamMode::In => None,
        other => Some(other),
    }
}

/// The keyword naming a parameter mode.
fn mode_name(mode: ParamMode) -> &'static str {
    match mode {
        ParamMode::In => "in",
        ParamMode::Ref => "ref",
        ParamMode::Mut => "mut",
        ParamMode::Out => "out",
    }
}

/// Report an argument to a const parameter that is not a const binding. F076.
fn report_comptime_arg<'db>(ctx: &mut TypeContext<'db>, arg: ExprFun<'db>, message: String) {
    let site = crate::ErrorSite::Expr(arg.key());
    ctx.push_coded(
        site,
        "F076",
        message,
        S("a const parameter takes a const binding"),
        Some(S("bind the value with `const` and pass its name, so the compiler holds it when it \
specializes the function")),
    );
}

/// Validate that a comptime argument is the name of a const binding.
///
/// Not a literal, and not an expression. A const parameter's value has to be
/// something the compiler already holds, and a `const` binding is the one form
/// that says so on its face: it names a value CTFE has evaluated. Everything
/// else would need the compiler to decide, case by case, which shapes it can
/// see through -- which is how a rule stops being one.
///
/// A const parameter counts, since it is a const binding inside the body.
///
/// An error is recorded rather than returned, so that the remaining arguments
/// are still checked.
fn validate_comptime_arg<'db>(
    ctx: &mut TypeContext<'db>,
    arg: ExprFun<'db>,
    param_idx: usize,
) -> Result<(), TypeError> {
    let db = ctx.db;

    match arg.kind() {
        ExprFunKind::Place(place) if place.steps.is_empty() => {
            let name = place.root;
            // Must be a const binding, not a let/var/parameter.
            if !ctx.is_const_binding(name) {
                let reason = fmt!("`{}` is not a const binding", name.as_str(db));
                report_comptime_arg(ctx, arg, reason.C());
                ctx.add_error(TypeError::ComptimeArgNotConstBinding { param_idx, reason });
            }
        }
        _ => {
            let reason = S("the argument to a const parameter must name a const");
            report_comptime_arg(ctx, arg, reason.C());
            ctx.add_error(TypeError::ComptimeArgNotConstBinding { param_idx, reason });
        }
    }

    Ok(())
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

    // Intrinsics take every argument by value, so a marker is always wrong.
    for (i, written) in icall.arg_modes.iter().enumerate() {
        if let Some(mode) = written {
            let err = ctx.error_argument_mode_mismatch(ctx.expr(args[i]), i, "in", mode_name(*mode));
            ctx.add_error(err);
        }
    }

    // Look up intrinsic by name.
    let (_, intrinsic_def) = lookup_intrinsic(name_str)
        .ok_or_else(|| ctx.error_cannot_synthesize(expr, &format!("unknown intrinsic: {}", name_str)))?;

    // Check arity.
    if args.len() != intrinsic_def.params.len() {
        return Err(ctx.error_cannot_synthesize(
            expr,
            &format!("intrinsic {} expects {} arguments, got {}", name_str, intrinsic_def.params.len(), args.len())
        ));
    }

    // Each argument is checked against its parameter, as a call's is, so a
    // literal takes the parameter's type.
    for (arg, expected_intrinsic_ty) in args.iter().zip(intrinsic_def.params.iter()) {
        let expected_datafun_ty = intrinsic_type_to_datafun(db, *expected_intrinsic_ty);
        check_expr(ctx, ctx.expr(*arg), &expected_datafun_ty)?;
    }

    // Return the intrinsic's return type.
    Ok(intrinsic_type_to_datafun(db, intrinsic_def.ret))
}

/// Convert intrinsic type to datafun type.
pub(crate) fn intrinsic_type_to_datafun<'db>(_db: &'db dyn crate::Db, ty: datalove_datafun_intrinsics::IntrinsicType) -> Type<'db> {
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
    try_op: &ExprTryOption,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Special case: a[i]? — list index with option early-return.
    if let ExprFunKind::Index(index_expr) = expr.at(try_op.operand).kind() {
        return synthesize_index_try_option(ctx, expr, index_expr);
    }

    // Synthesize operand type first (to report operand errors before context errors).
    let operand_ty = ctx.synthesize_expr(ctx.expr(try_op.operand))?;

    // Operand must be Option<T>.
    let inner_ty = match operand_ty {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => opt.inner_type.clone(),
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "?",
                Wrapper::Option,
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
    try_op: &ExprTryResult,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Special case: a[i]! — list index with result early-return.
    if let ExprFunKind::Index(index_expr) = expr.at(try_op.operand).kind() {
        return synthesize_index_try_result(ctx, expr, index_expr);
    }

    // Synthesize operand type first (to report operand errors before context errors).
    let operand_ty = ctx.synthesize_expr(ctx.expr(try_op.operand))?;

    // Operand must be Result<T>.
    let inner_ty = match operand_ty {
        Type::Datalit(datalit::tycheck::Type::Result(res)) => res.inner_type.clone(),
        _ => {
            return Err(ctx.error_try_type_mismatch(
                expr,
                "!",
                Wrapper::Result,
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
    index_expr: &ExprIndex,
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
    index_expr: &ExprIndex,
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
    index_expr: &ExprIndex,
) -> Result<Type<'db>, TypeError> {
    let base_ty = ctx.synthesize_expr(ctx.expr(index_expr.base))?;
    let (element_ty, index_type) = resolve_index_types(ctx.db, &base_ty)
        .map_err(|msg| ctx.error_cannot_synthesize(expr, &msg))?;

    if let Err(e) = check_expr(ctx, ctx.expr(index_expr.index), &index_type) {
        ctx.add_error(e);
    }

    // Reject view-producing index in mut/out context.
    if ctx.mut_context && is_view_producing_index(&base_ty) {
        let site = crate::ErrorSite::Expr(expr.key());
        return Err(ctx.report_place_error(site, TypeError::ViewTypeMutBinding {
            view_ty: type_to_string(ctx.db, &element_ty),
        }));
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

    // Inside a const expression only other consts are in scope. Without this
    // the name typechecks against the binding it shadows and then reaches
    // lowering, which has no value for it and panics.
    if ctx.in_const_expr && !ctx.is_const_binding(place.root) {
        return Err(ctx.error_non_const_in_const_expr(expr, place.root));
    }

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
        _ => Err(format!("`{}` cannot be indexed; a list `[T]`, a map `%{{K = V}}` or a tensor `[|T, N|]` can", type_to_string(db, base_ty))),
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
    expr: ExprFun<'db>,
    base_ty: &Type<'db>,
    field: &FieldSelector<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let site = crate::ErrorSite::Expr(expr.key());
    let field_ty = resolve_field_type(db, base_ty, field)
        .map_err(|e| ctx.report_place_error(site, e))?;

    // Check that field is a copy type or we're in ref context.
    // Move-type field projections are allowed in ref context.
    if let Type::Datalit(ref dt) = field_ty {
        if !is_copy_type(db, dt) && !ctx.ref_context {
            return Err(ctx.report_place_error(site, TypeError::NonCopyFieldProjection {
                field_ty: datalit::tycheck::type_to_string(db, dt),
            }));
        }
    }

    Ok(field_ty)
}

/// Synthesize type through an index step in a place expression.
fn synthesize_place_index_step<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    base_ty: &Type<'db>,
    idx: &PlaceIndex,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let (element_ty, index_type) = resolve_index_types(db, base_ty)
        .map_err(|msg| ctx.error_cannot_synthesize(expr, &msg))?;

    if let Err(e) = check_expr(ctx, ctx.expr(idx.index), &index_type) {
        ctx.add_error(e);
    }

    // Reject view-producing index in mut/out context.
    if ctx.mut_context && is_view_producing_index(base_ty) {
        let site = crate::ErrorSite::Expr(expr.key());
        return Err(ctx.report_place_error(site, TypeError::ViewTypeMutBinding {
            view_ty: type_to_string(ctx.db, &element_ty),
        }));
    }

    // Non-copy guard: indexing into a collection with non-copy element type
    // requires ref context (e.g. via @, ref param, or intermediate step).
    if let Type::Datalit(ref dt) = element_ty {
        if !is_copy_type(db, dt) && !ctx.ref_context {
            let site = crate::ErrorSite::Expr(expr.key());
            return Err(ctx.report_place_error(site, TypeError::NonCopyIndexProjection {
                elem_ty: datalit::tycheck::type_to_string(db, dt),
            }));
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
    expr: ExprFun<'db>,
    proj: &ExprFieldProj<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Synthesize base type.
    let base_ty = ctx.synthesize_expr(ctx.expr(proj.base))?;

    let site = crate::ErrorSite::Expr(expr.key());
    let field_ty = resolve_field_type(db, &base_ty, &proj.field)
        .map_err(|e| ctx.report_place_error(site, e))?;

    // Check that field is a copy type or we're in ref context.
    // Move-type field projections are allowed in ref context.
    if let Type::Datalit(ref dt) = field_ty {
        if !is_copy_type(db, dt) && !ctx.ref_context {
            return Err(ctx.report_place_error(site, TypeError::NonCopyFieldProjection {
                field_ty: datalit::tycheck::type_to_string(db, dt),
            }));
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
    let elements = &list_expr.elements;

    if elements.is_empty() {
        return Err(report_open(ctx, _expr));
    }

    // Synthesize type of first element.
    let first_ty = ctx.synthesize_expr(ctx.expr(elements[0]))?;
    let first_datalit = match first_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining elements for type compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(ctx.expr(*elem))?;
        check_like_first(ctx, ctx.expr(*elem), &first_ty, &elem_ty, "differs from the first element")?;
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
    let elements = &set_expr.elements;

    if elements.is_empty() {
        return Err(report_open(ctx, _expr));
    }

    let first_ty = ctx.synthesize_expr(ctx.expr(elements[0]))?;
    let first_datalit = match first_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining elements for type compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(ctx.expr(*elem))?;
        check_like_first(ctx, ctx.expr(*elem), &first_ty, &elem_ty, "differs from the first element")?;
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
    let entries = &map_expr.entries;

    if entries.is_empty() {
        return Err(report_open(ctx, _expr));
    }

    let first_key_ty = ctx.synthesize_expr(ctx.expr(entries[0].key))?;
    let first_key_datalit = match first_key_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };
    let first_value_ty = ctx.synthesize_expr(ctx.expr(entries[0].value))?;
    let first_value_datalit = match first_value_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining entries for type compatibility.
    for entry in &entries[1..] {
        let key_ty = ctx.synthesize_expr(ctx.expr(entry.key))?;
        let value_ty = ctx.synthesize_expr(ctx.expr(entry.value))?;
        check_like_first(ctx, ctx.expr(entry.key), &first_key_ty, &key_ty, "differs from the first key")?;
        check_like_first(ctx, ctx.expr(entry.value), &first_value_ty, &value_ty, "differs from the first value")?;
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
    let shape = &tensor_expr.shape;
    let elements = &tensor_expr.elements;

    // Rank is the number of dimensions in the shape.
    let rank = shape.len() as u32;

    if elements.is_empty() {
        return Err(report_open(ctx, _expr));
    }

    let first_ty = ctx.synthesize_expr(ctx.expr(elements[0]))?;
    let first_datalit = match first_ty {
        Type::Datalit(ref dt) => dt.clone(),
        Type::Function(_) => unreachable!("synthesized expression type is always Datalit"),
    };

    // Check remaining elements for type compatibility.
    for elem in &elements[1..] {
        let elem_ty = ctx.synthesize_expr(ctx.expr(*elem))?;
        check_like_first(ctx, ctx.expr(*elem), &first_ty, &elem_ty, "differs from the first element")?;
    }

    let ty = Type::Datalit(datalit::tycheck::Type::Tensor(
        datalit::tycheck::TypeTensor { element_type: Box::new(first_datalit), rank }
    ));
    Ok(ty)
}

/// Synthesize the type of a table written without one.
///
/// A column's type is what the first row puts in it, and every row after that
/// is checked against the first, which is how a list and a map are read. Until
/// this was written a table was the one collection that could not say its own
/// type at all, so it could be written nowhere a type was not already known --
/// not inside a `data`, not as a term's payload.
///
/// A table with no rows says only its column names, and leaves their types
/// open, as an empty list leaves its elements' type.
fn synthesize_inline_table<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    table_expr: &ExprTable<'db>,
) -> Result<Type<'db>, TypeError> {
    let header = &table_expr.header;
    let rows = &table_expr.rows;

    let mut column_types = Vec::with_capacity(header.len());
    match rows.first() {
        Some(first) => {
            if first.elements.len() != header.len() {
                return Err(crate::check::shape_error(
                ctx, expr,
                fmt!("a row of {} elements, where the header has {} columns", first.elements.len(), header.len()),
                None,
                TypeError::ArityMismatch { expected: header.len(), actual: first.elements.len() },
            ));
            }
            for element in &first.elements {
                let ty = ctx.synthesize_expr(ctx.expr(*element))?;
                column_types.push(ty);
            }
        }
        None if header.is_empty() => {}
        None => return Err(report_open(ctx, expr)),
    }

    for row in rows.iter().skip(1) {
        if row.elements.len() != header.len() {
            return Err(crate::check::shape_error(
                ctx, expr,
                fmt!("a row of {} elements, where the header has {} columns", row.elements.len(), header.len()),
                None,
                TypeError::ArityMismatch { expected: header.len(), actual: row.elements.len() },
            ));
        }
        for (element, column_ty) in row.elements.iter().zip(column_types.iter()) {
            let element_ty = ctx.synthesize_expr(ctx.expr(*element))?;
            check_like_first(ctx, ctx.expr(*element), column_ty, &element_ty, "differs from the first row")?;
        }
    }

    let columns = header
        .iter()
        .zip(column_types.into_iter())
        .map(|(name, ty)| {
            let datalit_ty = match ty {
                Type::Datalit(dt) => dt,
                Type::Function(_) => {
                    unreachable!("synthesized expression type is always Datalit")
                }
            };
            datalit::tycheck::TypeNamedField { name: *name, ty: Box::new(datalit_ty) }
        })
        .collect();

    Ok(Type::Datalit(datalit::tycheck::Type::Table(
        datalit::tycheck::TypeTable { columns },
    )))
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
        let elem_ty = ctx.synthesize_expr(ctx.expr(*elem))?;
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
        let field_ty = ctx.synthesize_expr(ctx.expr(field.value))?;
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
    let _ = ctx.synthesize_expr(ctx.expr(value))?;

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
    let _ = ctx.synthesize_expr(ctx.expr(value))?;

    // Error synthesizes to Type::Error (unit type).
    let ty = Type::Datalit(datalit::tycheck::Type::Error);
    Ok(ty)
}

/// Check an element of an unhinted collection against the type its first one
/// set, reporting F016 at the element that differs.
fn check_like_first<'db>(
    ctx: &mut TypeContext<'db>,
    elem: ExprFun<'db>,
    first_ty: &Type<'db>,
    elem_ty: &Type<'db>,
    label: &str,
) -> Result<(), TypeError> {
    check_element_compatible(ctx.db, first_ty, elem_ty).map_err(|_| {
        let expected = type_to_string(ctx.db, first_ty);
        let actual = type_to_string(ctx.db, elem_ty);
        ctx.error_type_mismatch(elem, &expected, &actual, label)
    })
}
