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

/// Render the diagnostics that stopped modules compiling.
///
/// Each is framed against the file `locate` says its text came from, falling
/// back to `file_path` -- the script, which is what the reader asked to run --
/// for one it does not know.
pub fn render_module_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    parse: &[&datalove_diagnostic::ParseDiagnostic],
    types: &[&datalove_diagnostic::TypeDiagnostic],
    locate: &dyn Fn(Text<'db>) -> Option<rmx::std::path::PathBuf>,
    file_path: &Path,
    cwd: &Path,
) {
    let diagnostics = parse.iter().map(|d| d.to_diagnostic(db))
        .chain(types.iter().map(|d| d.to_diagnostic(db)));
    render::render_located_diagnostics(db, diagnostics, file_path, locate, cwd);
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
                AnalysisError::InconsistentLoopExit { name, .. } => {
                    eprintln!("error[D014]: `{name}` is given away on one way out of this loop and not another");
                    eprintln!("  note: the move cannot happen twice, so it is not the problem. \
                               Whether `{name}` is still held after the loop depends on which \
                               way out ran, and a drop has to be placed without knowing.");
                    eprintln!("  help: give it away on every way out, or clone it with `@` at the move");
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
        AnalysisError::InconsistentBranchMove {
            at, name, gave_away, fix_in, changed_at, moved_before, ..
        } => {
            let message = datalove_datafun_sema::inconsistent_branch_message(error)
                .expect("is D008");
            let mut builder = DiagnosticBuilder::error(db, &message).code("D008");

            // The branch's own move or `set` is what to look at; the condition
            // only says which `if` or `match` it was.
            builder = match changed_at.and_then(|key| lookup_expr_span(db, spans, key)) {
                Some((text, span)) => {
                    let label = match gave_away {
                        true => "given away here",
                        false => "given a new value here",
                    };
                    builder.primary_label(TextSpan::new(text, span), label)
                }
                None => {
                    let (text, span) = lookup_expr_span(db, spans, *at)?;
                    builder.primary_label(TextSpan::new(text, span), "the branches of this disagree")
                }
            };
            if let Some((text, span)) = moved_before.and_then(|key| lookup_expr_span(db, spans, key)) {
                builder = builder.secondary_label(
                    TextSpan::new(text, span),
                    &fmt!("`{name}` given away here, before the branches"),
                );
            }

            let help = match gave_away {
                true => fmt!("give `{name}` away {fix_in} too, or clone it with `@` where it is given away"),
                false => fmt!("give `{name}` a value {fix_in} too"),
            };
            Some(
                builder
                    .note(&fmt!(
                        "whether `{name}` is held after the branches depends on which one ran, \
                         and it has to be dropped without knowing"
                    ))
                    .help(&help)
                    .build(),
            )
        }
        // These carry no expression to point at.
        AnalysisError::OutParamNotInitialized { .. }
        | AnalysisError::InconsistentLoopExit { .. } => None,
    }
}
