//! Diagnostic rendering using ariadne.

use rmx::prelude::*;
use rmx::std::path::Path;
use rmx::std::collections::HashMap;

use ariadne::{Color, ColorGenerator, Label, Report, ReportKind, Source, Cache};
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
    let base_file_name = display_path.display().to_string();

    // Build source cache: map source text to file ID.
    // Use numbered suffixes for different sources in the same worldfile.
    let mut source_to_id: HashMap<String, String> = HashMap::new();
    let mut sources: HashMap<String, Source<String>> = HashMap::new();
    let mut next_id = 0;

    for label in &diag.labels {
        let source_text = label.text.as_str(db).to_string();
        if !source_to_id.contains_key(&source_text) {
            let file_id = if next_id == 0 {
                base_file_name.clone()
            } else {
                format!("{}:{}", base_file_name, next_id)
            };
            source_to_id.insert(source_text.clone(), file_id.clone());
            sources.insert(file_id, Source::from(source_text));
            next_id += 1;
        }
    }

    // If no labels, use the base file name.
    if diag.labels.is_empty() {
        sources.insert(base_file_name.clone(), Source::from(String::new()));
    }

    // Get the primary file ID.
    let primary_file_id = diag.labels.first()
        .map(|l| source_to_id.get(l.text.as_str(db)).unwrap().clone())
        .unwrap_or_else(|| base_file_name.clone());

    let offset = diag.labels.first()
        .map(|l| l.span.start)
        .unwrap_or(0);

    // Build the report.
    let mut builder = Report::build(kind, primary_file_id.clone(), offset);

    let message = diag.message.as_str(db);
    builder = builder.with_message(message);

    if let Some(code) = &diag.code {
        builder = builder.with_code(code.as_str(db));
    }

    // Add labels with their respective file IDs.
    for label in &diag.labels {
        let source_text = label.text.as_str(db);
        let file_id = source_to_id.get(source_text).unwrap().clone();

        let label_color = match label.style {
            LabelStyle::Primary => colors.next(),
            LabelStyle::Secondary => Color::Cyan,
        };

        let span = label.span.clone();
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

impl Cache<String> for MultiSourceCache {
    type Storage = String;

    fn fetch(&mut self, id: &String) -> Result<&Source<String>, Box<dyn std::fmt::Debug + '_>> {
        self.sources.get(id)
            .ok_or_else(|| Box::new(format!("Source not found: {}", id)) as Box<dyn std::fmt::Debug>)
    }

    fn display<'a>(&self, id: &'a String) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(id.clone()))
    }
}
