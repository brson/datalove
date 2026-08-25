use crate::ast::*;

/// Resolution result for an expression.
///
/// With named types removed from the language, this is now just a container
/// for the expression and source, used to pass context through typechecking.
#[salsa::tracked]
pub struct ResolvedExpr<'db> {
    /// Original expression.
    #[returns(copy)]
    pub expr: ExprFull<'db>,

    /// Source for on-demand span lookup.
    #[returns(copy)]
    pub source: bct::input::Source,
}

/// Main entry point: resolve all names in an expression.
///
/// With named types removed, this now simply wraps the expression with source.
#[salsa::tracked(returns(copy))]
pub fn resolve_names<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    expr: ExprFull<'db>,
) -> ResolvedExpr<'db> {
    ResolvedExpr::new(db, expr, source)
}

#[cfg(test)]
mod tests {
}
