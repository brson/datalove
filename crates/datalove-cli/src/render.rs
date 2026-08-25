//! Diagnostic rendering using ariadne.

use rmx::prelude::*;
use rmx::std::path::Path;
use rmx::std::collections::HashMap;

use ariadne::{Color, ColorGenerator, Label, Report, ReportKind, Source, Cache};
use bct::diagnostic::{Diagnostic, LabelStyle, Severity};

/// Render parse diagnostics to stderr using ariadne.
pub fn render_parse_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    diagnostics: &[&datalove_diagnostic::ParseDiagnostic],
    file_path: &Path,
    cwd: &Path,
) {
    let mut colors = ColorGenerator::new();

    for diag_wrapper in diagnostics {
        let diag = diag_wrapper.to_diagnostic(db);
        render_single_diagnostic(db, &diag, file_path, cwd, &mut colors);
    }
}

/// Render type diagnostics to stderr using ariadne.
pub fn render_type_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    diagnostics: &[&datalove_diagnostic::TypeDiagnostic],
    file_path: &Path,
    cwd: &Path,
) {
    let mut colors = ColorGenerator::new();

    for diag_wrapper in diagnostics {
        let diag = diag_wrapper.to_diagnostic(db);
        render_single_diagnostic(db, &diag, file_path, cwd, &mut colors);
    }
}

/// Render module type diagnostics to stderr using ariadne.
///
/// This handles cross-module diagnostics where labels may come from different sources.
/// Each unique source text gets its own virtual file name based on the worldfile path.
pub fn render_module_type_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    diagnostics: &[&datalove_diagnostic::TypeDiagnostic],
    worldfile_path: &Path,
    cwd: &Path,
) {
    let mut colors = ColorGenerator::new();

    for diag_wrapper in diagnostics {
        let diag = diag_wrapper.to_diagnostic(db);
        render_multi_source_diagnostic(db, &diag, worldfile_path, cwd, &mut colors);
    }
}

/// Render a single diagnostic (single source).
fn render_single_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    diag: &Diagnostic<'db>,
    file_path: &Path,
    cwd: &Path,
    colors: &mut ColorGenerator,
) {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
        Severity::Help => ReportKind::Advice,
    };

    // Get relative path for display, stripping cwd prefix if present.
    let display_path = file_path.strip_prefix(cwd).unwrap_or(file_path);
    let file_name = display_path.display().S();

    // Find the primary label's span for the report location.
    let offset = diag.labels.first()
        .map(|l| l.span.start)
        .unwrap_or(0);

    // Start building the report.
    let mut builder = Report::build(kind, &file_name, offset);

    // Set the main message.
    let message = diag.message.as_str(db);
    builder = builder.with_message(message);

    // Add error code if present.
    if let Some(code) = &diag.code {
        builder = builder.with_code(code.as_str(db));
    }

    // Add labels.
    for label in &diag.labels {
        let label_color = match label.style {
            LabelStyle::Primary => colors.next(),
            LabelStyle::Secondary => Color::Cyan,
        };

        let span = label.span.C();
        let mut ariadne_label = Label::new((&file_name, span))
            .with_color(label_color);

        if let Some(msg) = &label.message {
            ariadne_label = ariadne_label.with_message(msg.as_str(db));
        }

        builder = builder.with_label(ariadne_label);
    }

    // Add notes.
    for note in &diag.notes {
        builder = builder.with_note(note.as_str(db));
    }

    let report = builder.finish();

    // Get the source text from the first label.
    // All labels should reference the same source for now.
    let source_text = diag.labels.first()
        .map(|l| l.text.as_str(db))
        .unwrap_or("");

    // Write to stderr.
    let _ = report.eprint((&file_name, Source::from(source_text)));
}

/// Render a diagnostic with potentially multiple sources (cross-module).
fn render_multi_source_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    diag: &Diagnostic<'db>,
    worldfile_path: &Path,
    cwd: &Path,
    colors: &mut ColorGenerator,
) {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
        Severity::Help => ReportKind::Advice,
    };

    // Get relative path for display.
    let display_path = worldfile_path.strip_prefix(cwd).unwrap_or(worldfile_path);
    let base_file_name = display_path.display().S();

    // Build source cache: map source text to file ID.
    // Use numbered suffixes for different sources in the same worldfile.
    let mut source_to_id: HashMap<String, String> = HashMap::new();
    let mut sources: HashMap<String, Source<String>> = HashMap::new();
    let mut next_id = 0;

    for label in &diag.labels {
        let source_text = label.text.as_str(db).S();
        if !source_to_id.contains_key(&source_text) {
            let file_id = if next_id == 0 {
                base_file_name.C()
            } else {
                format!("{}:{}", base_file_name, next_id)
            };
            source_to_id.insert(source_text.C(), file_id.C());
            sources.insert(file_id, Source::from(source_text));
            next_id += 1;
        }
    }

    // If no labels, use the base file name.
    if diag.labels.is_empty() {
        sources.insert(base_file_name.C(), Source::from(String::new()));
    }

    // Get the primary file ID.
    let primary_file_id = diag.labels.first()
        .map(|l| source_to_id.get(l.text.as_str(db)).unwrap().C())
        .unwrap_or_else(|| base_file_name.C());

    let offset = diag.labels.first()
        .map(|l| l.span.start)
        .unwrap_or(0);

    // Build the report.
    let mut builder = Report::build(kind, primary_file_id.C(), offset);

    let message = diag.message.as_str(db);
    builder = builder.with_message(message);

    if let Some(code) = &diag.code {
        builder = builder.with_code(code.as_str(db));
    }

    // Add labels with their respective file IDs.
    for label in &diag.labels {
        let source_text = label.text.as_str(db);
        let file_id = source_to_id.get(source_text).unwrap().C();

        let label_color = match label.style {
            LabelStyle::Primary => colors.next(),
            LabelStyle::Secondary => Color::Cyan,
        };

        let span = label.span.C();
        let mut ariadne_label = Label::new((file_id, span))
            .with_color(label_color);

        if let Some(msg) = &label.message {
            ariadne_label = ariadne_label.with_message(msg.as_str(db));
        }

        builder = builder.with_label(ariadne_label);
    }

    for note in &diag.notes {
        builder = builder.with_note(note.as_str(db));
    }

    let report = builder.finish();

    // Create cache from sources.
    let cache = MultiSourceCache { sources };
    let _ = report.eprint(cache);
}

/// Simple cache for multiple in-memory sources.
struct MultiSourceCache {
    sources: HashMap<String, Source<String>>,
}

/// Render ownership errors directly from structured AnalysisError and spans.
///
/// This bypasses salsa accumulators since ownership diagnostics are emitted
/// outside of tracked functions. Takes the raw error list and span lookup.
pub fn render_ownership_errors_direct<'db>(
    db: &'db dyn salsa::Database,
    errors: &[datalove_datafun_sema::AnalysisError],
    spans: &datalove_datafun_ast::spans::DatafunSpans,
    file_path: &Path,
    cwd: &Path,
) {
    let mut colors = ColorGenerator::new();
    let display_path = file_path.strip_prefix(cwd).unwrap_or(file_path);
    let file_name = display_path.display().S();

    for error in errors {
        render_ownership_error(db, error, spans, &file_name, &mut colors);
    }
}

/// Look up span for an expression by local_index.
fn lookup_expr_span<'db>(
    db: &'db dyn salsa::Database,
    spans: &datalove_datafun_ast::spans::DatafunSpans<'db>,
    expr_key: datalove_datafun_ast::ast::ExprKey<'db>,
) -> Option<(bct::text::Text<'db>, std::ops::Range<usize>)> {
    spans.lookup_key(expr_key).map(|entry| entry.to_text_and_span(db))
}

/// Generate a Rust-style suggestion showing the modified source line with `+` markers.
///
/// Returns a formatted string like:
/// ```text
/// 7 |     let a: int = consume(msg@)
///   |                             +
/// ```
fn format_insertion_suggestion(
    source: &str,
    span: &std::ops::Range<usize>,
    insertion: &str,
) -> Option<String> {
    // Find the line containing the span end (where we insert).
    let insert_pos = span.end;

    // Find line boundaries.
    let line_start = source[..insert_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[insert_pos..].find('\n').map(|i| insert_pos + i).unwrap_or(source.len());

    // Calculate line number (1-indexed).
    let line_num = source[..line_start].matches('\n').count() + 1;

    // Get the original line.
    let original_line = &source[line_start..line_end];

    // Position within the line where we insert.
    let col = insert_pos - line_start;

    // Create the modified line by inserting the text.
    let mut modified_line = original_line.to_string();
    modified_line.insert_str(col, insertion);

    // Calculate the line number column width.
    let line_num_str = line_num.to_string();
    let line_num_width = line_num_str.len();

    // Build the suggestion display.
    // Line with modification: "N |     <code>"
    let code_line = format!("{} |     {}", line_num_str, modified_line);

    // Plus markers line: "  |     <spaces>+"
    // The leading spaces match the line number width.
    let marker_prefix = format!("{:width$} |     ", "", width = line_num_width);
    let plus_markers = "+".repeat(insertion.len());
    let marker_line = format!("{}{:col$}{}", marker_prefix, "", plus_markers, col = col);

    Some(format!("{}\n{}", code_line, marker_line))
}

/// Render a single ownership error.
fn render_ownership_error<'db>(
    db: &'db dyn salsa::Database,
    error: &datalove_datafun_sema::AnalysisError,
    spans: &datalove_datafun_ast::spans::DatafunSpans,
    file_name: &str,
    colors: &mut ColorGenerator,
) {
    use datalove_datafun_sema::{AnalysisError, OwnershipRecoveryHint};

    match error {
        AnalysisError::UseAfterMoveInEarlierUnit { expr_key, name, recovery_hint } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let source_str = text.as_str(db);
                let msg = format!("`{}` was given away by an earlier input", name);
                let mut builder = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D013")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("value used after move")
                    );

                if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                    builder = builder.with_help(format!("use `@` to {}", description));
                }

                let _ = builder.finish().eprint((file_name, Source::from(source_str)));
            }
        }
        AnalysisError::UseAfterMove { expr_key, moved_at, name, recovery_hint } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let source_str = text.as_str(db);
                let msg = format!("use of moved value: `{}`", name);
                let mut builder = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D001")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("value used after move")
                    );

                // Add secondary label at the move location if different.
                let move_span_for_suggestion = if moved_at != expr_key {
                    if let Some((_, move_span)) = lookup_expr_span(db, spans, *moved_at) {
                        builder = builder.with_label(
                            Label::new((file_name, move_span.clone()))
                                .with_color(Color::Cyan)
                                .with_message("value moved here")
                        );
                        Some(move_span)
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Show Rust-style suggestion with modified code and + markers.
                if let OwnershipRecoveryHint::InsertAdapt { .. } = recovery_hint {
                    // Use the move location for the suggestion if available.
                    let suggestion_span = move_span_for_suggestion.as_ref().unwrap_or(&span);
                    if let Some(suggestion) = format_insertion_suggestion(source_str, suggestion_span, "@") {
                        builder = builder.with_help(format!("insert `@` to clone:\n{}", suggestion));
                    } else {
                        builder = builder.with_help(format!("insert `@` to clone: `{}@`", name));
                    }
                }

                let report = builder.finish();
                let _ = report.eprint((file_name, Source::from(source_str)));
            }
        }
        AnalysisError::DoubleMove { expr_key, moved_at, name, recovery_hint } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let source_str = text.as_str(db);
                let msg = format!("value moved twice: `{}`", name);
                let mut builder = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D002")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("second move here")
                    );

                // Add secondary label at the first move location if different.
                let move_span_for_suggestion = if moved_at != expr_key {
                    if let Some((_, move_span)) = lookup_expr_span(db, spans, *moved_at) {
                        builder = builder.with_label(
                            Label::new((file_name, move_span.clone()))
                                .with_color(Color::Cyan)
                                .with_message("first move here")
                        );
                        Some(move_span)
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Show Rust-style suggestion with modified code and + markers.
                if let OwnershipRecoveryHint::InsertAdapt { .. } = recovery_hint {
                    // Use the first move location for the suggestion if available.
                    let suggestion_span = move_span_for_suggestion.as_ref().unwrap_or(&span);
                    if let Some(suggestion) = format_insertion_suggestion(source_str, suggestion_span, "@") {
                        builder = builder.with_help(format!("insert `@` to clone:\n{}", suggestion));
                    } else {
                        builder = builder.with_help(format!("insert `@` to clone: `{}@`", name));
                    }
                }

                let report = builder.finish();
                let _ = report.eprint((file_name, Source::from(source_str)));
            }
        }
        AnalysisError::CannotMoveBorrowed { expr_key, name } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot move borrowed value: `{}`", name);
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D003")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("cannot move borrowed value")
                    )
                    .with_note("borrowed parameters (ref, mut, out) cannot be moved")
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
        AnalysisError::CannotMutFromRef { expr_key, name } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot get mutable reference from immutable: `{}`", name);
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D004")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("ref parameter cannot be passed as mut")
                    )
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
        AnalysisError::AliasedMutableArgument { expr_key, name } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("aliased mutable argument: `{}`", name);
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D010")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("passed again in the same call")
                    )
                    .with_note("an argument passed as `mut` or `out` cannot also be passed to another parameter")
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
        AnalysisError::CannotMutateImmutable { expr_key, name } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot pass immutable binding as mutable: `{}`", name);
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D011")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("passed to a `mut` or `out` parameter")
                    )
                    .with_note(format!("declare `{}` with `var` to allow mutation", name))
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
        AnalysisError::CannotMutateTemporary { expr_key } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D012")
                    .with_message("cannot pass a temporary as mutable")
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("passed to a `mut` or `out` parameter")
                    )
                    .with_note("bind the value to a `var` first, so the mutation is observable")
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
        AnalysisError::ReadUninitialized { expr_key, name } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("read of uninitialized binding: `{}`", name);
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D005")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("used before initialization")
                    )
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
        AnalysisError::OutParamNotInitialized { ret_stmt_idx: _, name } => {
            // No expression span available for this error type.
            let msg = format!("out parameter not initialized: `{}`", name);
            eprintln!("error[D006]: {}", msg);
        }
        AnalysisError::MoveInLoop { expr_key, name, recovery_hint } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let source_str = text.as_str(db);
                let msg = format!("cannot move `{}` in loop", name);
                let mut builder = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D007")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("value moved inside loop")
                    );
                // Show Rust-style suggestion with modified code and + markers.
                if let OwnershipRecoveryHint::InsertAdapt { .. } = recovery_hint {
                    if let Some(suggestion) = format_insertion_suggestion(source_str, &span, "@") {
                        builder = builder.with_help(format!("insert `@` to clone:\n{}", suggestion));
                    } else {
                        builder = builder.with_help(format!("insert `@` to clone: `{}@`", name));
                    }
                }
                let report = builder.finish();
                let _ = report.eprint((file_name, Source::from(source_str)));
            }
        }
        AnalysisError::InconsistentBranchMove { stmt_idx: _, name, moved_in } => {
            // No expression span available for this error type.
            let msg = format!("`{}` moved in {} branch but not the other", name, moved_in);
            eprintln!("error[D008]: {}", msg);
        }
        AnalysisError::OutParamPartialWrite { expr_key, name } => {
            if let Some((text, span)) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot partially write to out parameter: `{}`", name);
                let report = Report::build(ReportKind::Error, file_name, span.start)
                    .with_code("D009")
                    .with_message(&msg)
                    .with_label(
                        Label::new((file_name, span.clone()))
                            .with_color(colors.next())
                            .with_message("partial write to out parameter")
                    )
                    .with_note("out parameters must be written as a whole value")
                    .finish();
                let _ = report.eprint((file_name, Source::from(text.as_str(db))));
            }
        }
    }
}

impl Cache<String> for MultiSourceCache {
    type Storage = String;

    fn fetch(&mut self, id: &String) -> Result<&Source<String>, Box<dyn std::fmt::Debug + '_>> {
        self.sources.get(id)
            .ok_or_else(|| Box::new(format!("Source not found: {}", id)) as Box<dyn std::fmt::Debug>)
    }

    fn display<'a>(&self, id: &'a String) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(id.C()))
    }
}
