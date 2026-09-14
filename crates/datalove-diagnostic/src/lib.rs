//! Diagnostic system for Datalove compiler.
//!
//! Names the accumulators datalove's phases file their complaints in. The
//! plumbing behind them is [`bct::diagnostics`], since which phases a
//! compiler has is the only part of this that belongs to a language.

bct::diagnostics! {
    ParseDiagnostic => emit_parse,
    TypeDiagnostic => emit_type,
    ResolutionDiagnostic => emit_resolution,
    LintDiagnostic => emit_lint,
    OwnershipDiagnostic => emit_ownership,
}
