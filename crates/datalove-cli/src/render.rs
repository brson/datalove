//! Diagnostic rendering using ariadne.

use rmx::prelude::*;
use rmx::std::path::Path;

use ariadne::{Color, ColorGenerator, Label, Report, ReportKind, Source};
use datalove_diagnostic::{Diagnostic, LabelStyle, Severity};

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

/// Render a single diagnostic.
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
    let file_name = display_path.display().to_string();

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

        let span = label.span.clone();
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
