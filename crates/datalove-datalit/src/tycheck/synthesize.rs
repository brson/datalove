//! Expression type synthesis.
//!
//! An expression with no expected type is typed in two steps. Inference finds
//! the type, typing the parts that must agree together (see [`super::infer`]),
//! and then the expression is checked against it. Checking is what reports
//! anything wrong with the parts, so synthesis and checking report the same
//! faults the same way.

use crate::ast::*;
use super::context::TypeContext;
use super::check::check;
use super::infer::{infer, infer_unhinted, report_unresolved, Unifier};
use super::types::*;

/// Synthesize a type for an expression.
pub fn synthesize<'db>(
    ctx: &mut TypeContext<'db>,
    expr: &ExprFull<'db>,
) -> Result<Type<'db>, TypeError> {
    // Rule: Syn-TypedExpr - a hint is the type, and the expression is checked
    // against it.
    if let Some(type_hint) = expr.type_hint() {
        let expected_type = ctx.convert_hint(expr, &type_hint)?;
        check(ctx, expr, &expected_type)?;
        return Ok(expected_type);
    }

    let mut uni = Unifier::new(ctx.db);
    let inferred = infer(ctx, &mut uni, expr)?;
    let ty = uni.resolve(&inferred).map_err(|unresolved| {
        report_unresolved(ctx, &unresolved);
        TypeError::CannotSynthesize
    })?;
    check(ctx, expr, &ty)?;
    Ok(ty)
}

/// What an expression would synthesize, leaving aside its hint, as far as it
/// can be told: for the message of a mismatch it is checked into.
///
/// `_` stands for whatever nothing decides, so `[]` is `[_]`.
pub fn describe_unhinted<'db>(
    ctx: &TypeContext<'db>,
    expr: &ExprFull<'db>,
) -> Result<String, TypeError> {
    let mut uni = Unifier::new(ctx.db);
    let inferred = infer_unhinted(ctx, &mut uni, expr)?;
    Ok(uni.describe(ctx.db, &inferred))
}
