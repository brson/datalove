use bct::text::InternedText;

use crate::ast::*;
use datalove_diagnostic::ByteSpan;

/// Unique identifier for a binding site.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct BindingId(pub u32);

/// Resolved reference pointing to a specific type hint definition.
#[salsa::tracked]
pub struct Resolution<'db> {
    /// The binding this reference resolves to.
    pub binding_id: BindingId,

    /// Scope depth where binding was found (0 = outermost).
    pub scope_depth: u32,

    /// Original type hint definition site.
    pub definition: TypeHintAndHeap<'db>,
}

/// Resolution error for a single name.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
pub enum ResolutionError {
    /// Name was not found in any scope.
    UnboundName,
}

/// A single resolution entry.
#[salsa::tracked]
pub struct ResolutionEntry<'db> {
    pub name: InternedText<'db>,
    pub resolution: Resolution<'db>,
}

/// A single resolution error entry.
#[salsa::tracked]
pub struct ResolutionErrorEntry<'db> {
    pub name: InternedText<'db>,
    pub error: ResolutionError,
}

/// Stored span entry (no lifetimes for Salsa storage).
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct StoredSpan {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

/// Resolution result for an expression.
#[salsa::tracked]
pub struct ResolvedExpr<'db> {
    /// Original expression.
    pub expr: ExprFull<'db>,

    /// List of successful resolutions.
    pub resolutions: Vec<ResolutionEntry<'db>>,

    /// List of resolution errors.
    pub errors: Vec<ResolutionErrorEntry<'db>>,

    /// Source for on-demand span lookup.
    pub source: bct::input::Source,
}

/// Main entry point: resolve all names in an expression.
///
/// With named types removed, this now simply returns an empty resolution.
/// The infrastructure is kept for API compatibility.
#[salsa::tracked]
pub fn resolve_names<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    expr: ExprFull<'db>,
) -> ResolvedExpr<'db> {
    // No named types means no resolutions needed.
    ResolvedExpr::new(db, expr, Vec::new(), Vec::new(), source)
}

#[cfg(test)]
mod tests {
}
