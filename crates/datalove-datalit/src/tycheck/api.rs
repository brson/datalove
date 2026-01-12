//! Public API for datalit typechecking.
//!
//! Provides the main entry points for type checking datalit expressions.

use crate::ast::ExprFull;
use crate::resolve::ResolvedExpr;
use super::context::TypeContext;
use super::check::check;
use super::synthesize::synthesize;
use super::types::{TypeAndHeap, TypeError};

/// Type error entry with location info.
#[salsa::tracked]
pub struct TypeErrorEntry<'db> {
    pub error: TypeError,
}

/// Result of typechecking.
#[salsa::tracked]
pub struct TypecheckResult<'db> {
    /// The root expression.
    pub root_expr: ExprFull<'db>,

    /// The root expression type (if successfully synthesized).
    pub root_type: Option<TypeAndHeap<'db>>,

    /// Type errors encountered.
    pub errors: Vec<TypeErrorEntry<'db>>,

    /// The resolved expression context.
    ///
    /// Preserved for instantiation of nested types like Data/Error.
    pub resolved: ResolvedExpr<'db>,
}

/// Main entry point: typecheck an expression.
#[salsa::tracked]
pub fn type_check<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
    resolved: ResolvedExpr<'db>,
) -> TypecheckResult<'db> {
    type_check_with_expected(db, expr, resolved, None)
}

/// Type check an expression with an optional expected type.
///
/// When expected type is provided, uses checking mode (bidirectional typing).
/// Otherwise uses synthesis mode.
pub fn type_check_with_expected<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
    resolved: ResolvedExpr<'db>,
    expected: Option<TypeAndHeap<'db>>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, resolved);

    let root_type = if let Some(expected_ty) = expected {
        // Use checking mode when expected type is provided.
        match check(&mut ctx, expr, expected_ty) {
            Ok(()) => Some(expected_ty),
            Err(e) => {
                ctx.add_error(e);
                None
            }
        }
    } else {
        // Use synthesis mode when no expected type.
        match synthesize(&mut ctx, expr) {
            Ok(ty) => Some(ty),
            Err(e) => {
                ctx.add_error(e);
                None
            }
        }
    };

    let errors = ctx
        .errors
        .into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();

    TypecheckResult::new(db, expr, root_type, errors, resolved)
}
