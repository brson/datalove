//! Turning datalove's errors into diagnostics, which `bct::render` prints.

use rmx::prelude::*;
use rmx::std::ops::Range;
use rmx::std::path::Path;

use bct::diagnostic::{Diagnostic, DiagnosticBuilder};
use bct::render::{self, Renderer};
use bct::text::{Text, TextSpan};

use datalove_datafun_ast::ast::ExprKey;
use datalove_datafun_ast::spans::DatafunSpans;
use datalove_datafun_sema::{AnalysisError, OwnershipRecoveryHint};

/// Render parse diagnostics to stderr.
pub fn render_parse_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    diagnostics: &[&datalove_diagnostic::ParseDiagnostic],
    file_path: &Path,
    cwd: &Path,
) {
    let diagnostics = diagnostics.iter().map(|d| d.to_diagnostic(db));
    render::render_diagnostics(db, diagnostics, file_path, cwd);
}

/// Render type diagnostics to stderr.
pub fn render_type_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    diagnostics: &[&datalove_diagnostic::TypeDiagnostic],
    file_path: &Path,
    cwd: &Path,
) {
    let diagnostics = diagnostics.iter().map(|d| d.to_diagnostic(db));
    render::render_diagnostics(db, diagnostics, file_path, cwd);
}

/// Render module type diagnostics to stderr.
///
/// A worldfile's modules are separate sources, and a diagnostic about one can
/// point at another, so these are rendered against all of them at once.
pub fn render_module_type_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    diagnostics: &[&datalove_diagnostic::TypeDiagnostic],
    worldfile_path: &Path,
    cwd: &Path,
) {
    let diagnostics = diagnostics.iter().map(|d| d.to_diagnostic(db));
    render::render_multi_source_diagnostics(db, diagnostics, worldfile_path, cwd);
}

/// Render ownership errors directly from structured AnalysisError and spans.
///
/// This bypasses salsa accumulators since ownership diagnostics are emitted
/// outside of tracked functions. Takes the raw error list and span lookup.
pub fn render_ownership_errors_direct<'db>(
    db: &'db dyn salsa::Database,
    errors: &[AnalysisError<'db>],
    spans: &DatafunSpans<'db>,
    file_path: &Path,
    cwd: &Path,
) {
    let mut renderer = Renderer::new();

    for error in errors {
        match ownership_diagnostic(db, error, spans) {
            Some(diagnostic) => renderer.diagnostic(db, &diagnostic, file_path, cwd),
            // An error with no expression to point at has only its message,
            // so it is written out rather than framed around nothing.
            None => match error {
                AnalysisError::OutParamNotInitialized { name, .. } => {
                    eprintln!("error[D006]: out parameter not initialized: `{name}`");
                }
                AnalysisError::InconsistentBranchMove { name, moved_in, .. } => {
                    eprintln!("error[D008]: `{name}` moved in {moved_in} branch but not the other");
                }
                _ => {}
            },
        }
    }
}

/// Look up span for an expression by local_index.
fn lookup_expr_span<'db>(
    db: &'db dyn salsa::Database,
    spans: &DatafunSpans<'db>,
    expr_key: ExprKey<'db>,
) -> Option<(Text<'db>, Range<usize>)> {
    spans.lookup_key(expr_key).map(|entry| entry.to_text_and_span(db))
}

/// The help for an error a `@` would fix, showing the line as it would read.
fn adapt_help<'db>(
    recovery_hint: &OwnershipRecoveryHint,
    source: &str,
    span: &Range<usize>,
    name: &str,
) -> Option<String> {
    let OwnershipRecoveryHint::InsertAdapt { .. } = recovery_hint else {
        return None;
    };
    Some(match render::insertion_suggestion(source, span, "@") {
        Some(suggestion) => fmt!("insert `@` to clone:\n{suggestion}"),
        None => fmt!("insert `@` to clone: `{name}@`"),
    })
}

/// The diagnostic for one ownership error, absent where it has no span.
fn ownership_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    error: &AnalysisError<'db>,
    spans: &DatafunSpans<'db>,
) -> Option<Diagnostic<'db>> {
    match error {
        AnalysisError::UseAfterMoveInEarlierUnit { expr_key, name, recovery_hint } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            let mut builder =
                DiagnosticBuilder::error(db, &fmt!("`{name}` was given away by an earlier input"))
                    .code("D013")
                    .primary_label(TextSpan::new(text, span), "value used after move");
            if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                builder = builder.help(&fmt!("use `@` to {description}"));
            }
            Some(builder.build())
        }
        AnalysisError::UseAfterMove { expr_key, moved_at, name, recovery_hint } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            let mut builder = DiagnosticBuilder::error(db, &fmt!("use of moved value: `{name}`"))
                .code("D001")
                .primary_label(TextSpan::new(text, span.C()), "value used after move");

            // The move is labelled too where it is somewhere else, and is
            // where the `@` goes.
            let move_span = match moved_at == expr_key {
                true => None,
                false => lookup_expr_span(db, spans, *moved_at).map(|(_, span)| span),
            };
            if let Some(move_span) = &move_span {
                builder = builder
                    .secondary_label(TextSpan::new(text, move_span.C()), "value moved here");
            }

            let source = text.as_str(db);
            let suggestion_span = move_span.unwrap_or(span);
            if let Some(help) = adapt_help(recovery_hint, source, &suggestion_span, name) {
                builder = builder.help(&help);
            }
            Some(builder.build())
        }
        AnalysisError::DoubleMove { expr_key, moved_at, name, recovery_hint } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            let mut builder = DiagnosticBuilder::error(db, &fmt!("value moved twice: `{name}`"))
                .code("D002")
                .primary_label(TextSpan::new(text, span.C()), "second move here");

            let move_span = match moved_at == expr_key {
                true => None,
                false => lookup_expr_span(db, spans, *moved_at).map(|(_, span)| span),
            };
            if let Some(move_span) = &move_span {
                builder = builder
                    .secondary_label(TextSpan::new(text, move_span.C()), "first move here");
            }

            let source = text.as_str(db);
            let suggestion_span = move_span.unwrap_or(span);
            if let Some(help) = adapt_help(recovery_hint, source, &suggestion_span, name) {
                builder = builder.help(&help);
            }
            Some(builder.build())
        }
        AnalysisError::CannotMoveBorrowed { expr_key, name } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(db, &fmt!("cannot move borrowed value: `{name}`"))
                    .code("D003")
                    .primary_label(TextSpan::new(text, span), "cannot move borrowed value")
                    .note("borrowed parameters (ref, mut, out) cannot be moved")
                    .build(),
            )
        }
        AnalysisError::CannotMutFromRef { expr_key, name } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(
                    db,
                    &fmt!("cannot get mutable reference from immutable: `{name}`"),
                )
                .code("D004")
                .primary_label(TextSpan::new(text, span), "ref parameter cannot be passed as mut")
                .build(),
            )
        }
        AnalysisError::AliasedMutableArgument { expr_key, name } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(db, &fmt!("aliased mutable argument: `{name}`"))
                    .code("D010")
                    .primary_label(TextSpan::new(text, span), "passed again in the same call")
                    .note("an argument passed as `mut` or `out` cannot also be passed to another parameter")
                    .build(),
            )
        }
        AnalysisError::CannotMutateImmutable { expr_key, name } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(
                    db,
                    &fmt!("cannot pass immutable binding as mutable: `{name}`"),
                )
                .code("D011")
                .primary_label(TextSpan::new(text, span), "passed to a `mut` or `out` parameter")
                .note(&fmt!("declare `{name}` with `var` to allow mutation"))
                .build(),
            )
        }
        AnalysisError::CannotMutateTemporary { expr_key } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(db, "cannot pass a temporary as mutable")
                    .code("D012")
                    .primary_label(TextSpan::new(text, span), "passed to a `mut` or `out` parameter")
                    .note("bind the value to a `var` first, so the mutation is observable")
                    .build(),
            )
        }
        AnalysisError::ReadUninitialized { expr_key, name } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(db, &fmt!("read of uninitialized binding: `{name}`"))
                    .code("D005")
                    .primary_label(TextSpan::new(text, span), "used before initialization")
                    .build(),
            )
        }
        AnalysisError::MoveInLoop { expr_key, name, recovery_hint } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            let mut builder = DiagnosticBuilder::error(db, &fmt!("cannot move `{name}` in loop"))
                .code("D007")
                .primary_label(TextSpan::new(text, span.C()), "value moved inside loop");
            if let Some(help) = adapt_help(recovery_hint, text.as_str(db), &span, name) {
                builder = builder.help(&help);
            }
            Some(builder.build())
        }
        AnalysisError::OutParamPartialWrite { expr_key, name } => {
            let (text, span) = lookup_expr_span(db, spans, *expr_key)?;
            Some(
                DiagnosticBuilder::error(
                    db,
                    &fmt!("cannot partially write to out parameter: `{name}`"),
                )
                .code("D009")
                .primary_label(TextSpan::new(text, span), "partial write to out parameter")
                .note("out parameters must be written as a whole value")
                .build(),
            )
        }
        // These carry no expression to point at.
        AnalysisError::OutParamNotInitialized { .. }
        | AnalysisError::InconsistentBranchMove { .. } => None,
    }
}
