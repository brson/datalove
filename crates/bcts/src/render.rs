//! Printing diagnostics, with the source they point into.
//!
//! A [`crate::diagnostic::Diagnostic`] says what is wrong and where; this
//! turns one into the framed, underlined form a reader sees. Kept here rather
//! than at each driver so that every language built on this toolkit complains
//! in the same shape.

use rmx::prelude::*;

use rmx::std::collections::HashMap;
use rmx::std::ops::Range;
use rmx::std::path::Path;

use ariadne::{Cache, Color, ColorGenerator, Label, Report, ReportKind, Source};

use crate::diagnostic::{Diagnostic, LabelStyle, Severity};

/// A run of diagnostics printed together.
///
/// The colours run across the whole run rather than restarting at each
/// diagnostic, so two labels printed together are told apart by theirs. A
/// caller that has other things to print between them holds one of these;
/// one that does not can use the functions below.
pub struct Renderer {
    colors: ColorGenerator,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer { colors: ColorGenerator::new() }
    }
}

impl Renderer {
    pub fn new() -> Self {
        Renderer::default()
    }

    /// Print one diagnostic, quoting the one source it points into.
    pub fn diagnostic<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        diag: &Diagnostic<'db>,
        file_path: &Path,
        cwd: &Path,
    ) {
        render_one(db, diag, file_path, cwd, &mut self.colors);
    }

    /// Print one diagnostic whose labels may point into several sources.
    ///
    /// A worldfile holds several modules, and an error about one of them can
    /// point at another, so each source gets an id of its own and the frame
    /// names which it is quoting.
    pub fn multi_source_diagnostic<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        diag: &Diagnostic<'db>,
        file_path: &Path,
        cwd: &Path,
    ) {
        render_one_multi_source(db, diag, file_path, cwd, &mut self.colors);
    }
}

/// Print diagnostics to stderr, quoting the one source they point into.
pub fn render_diagnostics<'db>(
    db: &'db dyn crate::Db,
    diagnostics: impl IntoIterator<Item = Diagnostic<'db>>,
    file_path: &Path,
    cwd: &Path,
) {
    let mut renderer = Renderer::new();
    for diagnostic in diagnostics {
        renderer.diagnostic(db, &diagnostic, file_path, cwd);
    }
}

/// Print diagnostics whose labels may point into more than one source.
pub fn render_multi_source_diagnostics<'db>(
    db: &'db dyn crate::Db,
    diagnostics: impl IntoIterator<Item = Diagnostic<'db>>,
    file_path: &Path,
    cwd: &Path,
) {
    let mut renderer = Renderer::new();
    for diagnostic in diagnostics {
        renderer.multi_source_diagnostic(db, &diagnostic, file_path, cwd);
    }
}

/// The path as a reader knows it, which is where they are standing.
fn display_path(file_path: &Path, cwd: &Path) -> String {
    file_path.strip_prefix(cwd).unwrap_or(file_path).display().S()
}

fn report_kind(severity: Severity) -> ReportKind<'static> {
    match severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
        Severity::Help => ReportKind::Advice,
    }
}

/// The colour a label is drawn in.
///
/// A primary one takes the next colour of the batch, so that two of them are
/// told apart; a secondary is always the same, being the one pointed back at
/// rather than the thing complained about.
fn label_color(style: LabelStyle, colors: &mut ColorGenerator) -> Color {
    match style {
        LabelStyle::Primary => colors.next(),
        LabelStyle::Secondary => Color::Cyan,
    }
}

fn render_one<'db>(
    db: &'db dyn crate::Db,
    diag: &Diagnostic<'db>,
    file_path: &Path,
    cwd: &Path,
    colors: &mut ColorGenerator,
) {
    let file_name = display_path(file_path, cwd);

    // The frame is placed at the first label, and at the top where a
    // diagnostic carries none.
    let offset = diag.labels.first().map(|l| l.span.start).unwrap_or(0);

    let mut builder = Report::build(report_kind(diag.severity), &file_name, offset)
        .with_message(diag.message.as_str(db));

    if let Some(code) = &diag.code {
        builder = builder.with_code(code.as_str(db));
    }

    for label in &diag.labels {
        let mut ariadne_label = Label::new((&file_name, label.span.C()))
            .with_color(label_color(label.style, colors));
        if let Some(message) = &label.message {
            ariadne_label = ariadne_label.with_message(message.as_str(db));
        }
        builder = builder.with_label(ariadne_label);
    }

    for note in &diag.notes {
        builder = builder.with_note(note.as_str(db));
    }
    for help in &diag.helps {
        builder = builder.with_help(help.as_str(db));
    }

    // Every label is in the one source here, so the first of them says which.
    let source_text = diag.labels.first().map(|l| l.text.as_str(db)).unwrap_or("");

    let _ = builder.finish().eprint((&file_name, Source::from(source_text)));
}

fn render_one_multi_source<'db>(
    db: &'db dyn crate::Db,
    diag: &Diagnostic<'db>,
    file_path: &Path,
    cwd: &Path,
    colors: &mut ColorGenerator,
) {
    let base_file_name = display_path(file_path, cwd);

    // One id per source, numbered after the first, since several of them
    // came out of the same file.
    let mut source_to_id: HashMap<String, String> = HashMap::new();
    let mut sources: HashMap<String, Source<String>> = HashMap::new();

    for label in &diag.labels {
        let source_text = label.text.as_str(db).S();
        if !source_to_id.contains_key(&source_text) {
            let file_id = match source_to_id.len() {
                0 => base_file_name.C(),
                n => fmt!("{base_file_name}:{n}"),
            };
            source_to_id.insert(source_text.C(), file_id.C());
            sources.insert(file_id, Source::from(source_text));
        }
    }

    if diag.labels.is_empty() {
        sources.insert(base_file_name.C(), Source::from(String::new()));
    }

    let primary_file_id = diag.labels.first()
        .map(|l| source_to_id.get(l.text.as_str(db)).X().C())
        .unwrap_or_else(|| base_file_name.C());
    let offset = diag.labels.first().map(|l| l.span.start).unwrap_or(0);

    let mut builder = Report::build(report_kind(diag.severity), primary_file_id.C(), offset)
        .with_message(diag.message.as_str(db));

    if let Some(code) = &diag.code {
        builder = builder.with_code(code.as_str(db));
    }

    for label in &diag.labels {
        let file_id = source_to_id.get(label.text.as_str(db)).X().C();
        let mut ariadne_label = Label::new((file_id, label.span.C()))
            .with_color(label_color(label.style, colors));
        if let Some(message) = &label.message {
            ariadne_label = ariadne_label.with_message(message.as_str(db));
        }
        builder = builder.with_label(ariadne_label);
    }

    for note in &diag.notes {
        builder = builder.with_note(note.as_str(db));
    }
    for help in &diag.helps {
        builder = builder.with_help(help.as_str(db));
    }

    let _ = builder.finish().eprint(MultiSourceCache { sources });
}

/// Several in-memory sources, looked up by the id a label was given.
struct MultiSourceCache {
    sources: HashMap<String, Source<String>>,
}

impl Cache<String> for MultiSourceCache {
    type Storage = String;

    fn fetch(&mut self, id: &String) -> Result<&Source<String>, Box<dyn std::fmt::Debug + '_>> {
        self.sources.get(id)
            .ok_or_else(|| Box::new(fmt!("Source not found: {id}")) as Box<dyn std::fmt::Debug>)
    }

    fn display<'a>(&self, id: &'a String) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(id.C()))
    }
}

/// A line of source with text inserted into it, under `+` markers.
///
/// What a reader has to do to the line is easier to see written out than
/// described, so the suggestion shows the line as it would be.
pub fn insertion_suggestion(
    source: &str,
    span: &Range<usize>,
    insertion: &str,
) -> Option<String> {
    let insert_pos = span.end;

    let line_start = source[..insert_pos].rfind('\n').map(|i| i.checked_add(1).X()).unwrap_or(0);
    let line_end = source[insert_pos..]
        .find('\n')
        .map(|i| insert_pos.checked_add(i).X())
        .unwrap_or(source.len());

    // Line numbers are what a reader counts from one.
    let line_num = source[..line_start].matches('\n').count().checked_add(1).X();
    let original_line = &source[line_start..line_end];
    let col = insert_pos.checked_sub(line_start).X();

    let mut modified_line = original_line.S();
    modified_line.insert_str(col, insertion);

    let line_num_str = line_num.to_string();
    let line_num_width = line_num_str.len();

    let code_line = fmt!("{line_num_str} |     {modified_line}");
    let marker_prefix = fmt!("{:width$} |     ", "", width = line_num_width);
    let plus_markers = "+".repeat(insertion.len());
    let marker_line = fmt!("{marker_prefix}{:col$}{plus_markers}", "", col = col);

    Some(fmt!("{code_line}\n{marker_line}"))
}
