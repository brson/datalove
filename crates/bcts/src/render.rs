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

use ariadne::{Cache, CharSet, Color, ColorGenerator, Config, Label, Report, ReportKind, Source};

use crate::diagnostic::{Diagnostic, LabelStyle, Severity};
use crate::text::InternedText;

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
        render_one(db, diag, file_path, cwd, &mut self.colors, Sink::Stderr);
    }

    /// The same diagnostic as text, for a caller that is not a terminal.
    ///
    /// Plain and ASCII: no escape sequences and no box-drawing, because what
    /// asks for a string is a test asserting on it, a log, or a window drawing
    /// the text itself with a font that need not have `\u{256d}` in it. What
    /// wants colour has a terminal, and has [`Renderer::diagnostic`].
    pub fn to_string<'db>(
        &mut self,
        db: &'db dyn crate::Db,
        diag: &Diagnostic<'db>,
        file_path: &Path,
        cwd: &Path,
    ) -> String {
        let mut out = String::new();
        render_one(db, diag, file_path, cwd, &mut self.colors, Sink::Text(&mut out));
        out
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

/// The same run as one string, for a caller that is not a terminal.
///
/// See [`Renderer::to_string`] for why it is plain.
pub fn render_diagnostics_to_string<'db>(
    db: &'db dyn crate::Db,
    diagnostics: impl IntoIterator<Item = Diagnostic<'db>>,
    file_path: &Path,
    cwd: &Path,
) -> String {
    let mut renderer = Renderer::new();
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push_str(&renderer.to_string(db, &diagnostic, file_path, cwd));
    }
    out
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

/// Several lines as one, or nothing where there are none.
///
/// Every line after the first is marked with [`CONTINUATION`], for
/// [`reframe`] to put the frame back in front of.
fn joined<'db>(db: &'db dyn crate::Db, lines: &[InternedText<'db>]) -> Option<String> {
    match lines.is_empty() {
        true => None,
        false => {
            let lines: Vec<_> = lines.iter().flat_map(|l| l.as_str(db).lines()).collect();
            Some(lines.join(&fmt!("\n{CONTINUATION}")))
        }
    }
}

/// The mark on a line of a note or help that is not its first.
const CONTINUATION: char = '\u{1}';

/// The width of the `Help: ` and `Note: ` ariadne writes before the text.
const NOTE_LABEL_WIDTH: usize = 6;

/// A rendered report with its notes' and helps' later lines framed.
///
/// ariadne writes a note or help as one row, so the lines of a multi-line
/// one after the first come out at column zero, outside the frame. Each such
/// line is given the margin of the row it continues, drawn in the same
/// characters and colours, and indented to line up under the text.
fn reframe(report: &str) -> String {
    let mut out = String::with_capacity(report.len());
    let mut row = "";
    for line in report.split_inclusive('\n') {
        match line.strip_prefix(CONTINUATION) {
            Some(rest) => {
                out.push_str(&fmt!("{}{:NOTE_LABEL_WIDTH$}", &row[..label_start(row)], ""));
                out.push_str(rest);
            }
            None => {
                row = line;
                out.push_str(line);
            }
        }
    }
    out
}

/// Where the `Help` or `Note` begins in the row ariadne wrote it on.
///
/// That is the first letter, the margin being bars and spaces, or the
/// colour escape directly in front of it.
fn label_start(row: &str) -> usize {
    let bytes = row.as_bytes();
    let mut escapes_start = None;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\x1b' => {
                escapes_start.get_or_insert(i);
                let end = row[i..].find('m').X();
                i = i.checked_add(end).X().checked_add(1).X();
            }
            b if b.is_ascii_alphabetic() => return escapes_start.unwrap_or(i),
            _ => {
                escapes_start = None;
                i = i.checked_add(1).X();
            }
        }
    }
    panic!("a note or help row with no label: {row:?}");
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

/// Where a rendered diagnostic goes.
///
/// A terminal gets colour and the box-drawing frame; a string gets neither,
/// since the things that want a string are tests, logs and a window that draws
/// the text itself.
enum Sink<'a> {
    Stderr,
    Text(&'a mut String),
}

impl Sink<'_> {
    fn config(&self) -> Config {
        match self {
            Sink::Stderr => Config::default(),
            Sink::Text(_) => Config::default().with_color(false).with_char_set(CharSet::Ascii),
        }
    }

    fn emit<S: ariadne::Span, C: Cache<S::SourceId>>(self, report: Report<'_, S>, cache: C) {
        let mut bytes = Vec::new();
        let _ = report.write(cache, &mut bytes);
        let text = reframe(&String::from_utf8_lossy(&bytes));
        match self {
            Sink::Stderr => eprint!("{text}"),
            Sink::Text(out) => out.push_str(&text),
        }
    }
}

fn render_one<'db>(
    db: &'db dyn crate::Db,
    diag: &Diagnostic<'db>,
    file_path: &Path,
    cwd: &Path,
    colors: &mut ColorGenerator,
    sink: Sink<'_>,
) {
    let file_name = display_path(file_path, cwd);

    // The frame is placed at the first label, and at the top where a
    // diagnostic carries none.
    let offset = diag.labels.first().map(|l| l.span.start).unwrap_or(0);

    let mut builder = Report::build(report_kind(diag.severity), &file_name, offset)
        .with_config(sink.config())
        .with_message(diag.message.as_str(db));

    if let Some(code) = &diag.code {
        builder = builder.with_code(code.as_str(db));
    }

    for label in &diag.labels {
        // Always a message, even where there is none to give. ariadne draws a
        // label without one *not at all* -- no underline, nothing -- so a
        // caller that asked for a span to be marked and had nothing to add
        // about it got silence. An empty message leaves the connector running
        // out to nothing, which is ugly and is much the lesser fault.
        //
        // It is also a nudge in the right direction: a label reads better with
        // a few words on it than without, which is why rustc's carry them.
        let message = label.message.map(|m| m.as_str(db)).unwrap_or("");
        builder = builder.with_label(
            Label::new((&file_name, label.span.C()))
                .with_color(label_color(label.style, colors))
                .with_message(message),
        );
    }

    // One call each, not one per line. ariadne's report holds a single note
    // and a single help, so `with_note` in a loop keeps the last and drops
    // every one before it -- a diagnostic that carefully explained itself in
    // three notes printed one. Joined, they all arrive.
    if let Some(notes) = joined(db, &diag.notes) {
        builder = builder.with_note(notes);
    }
    if let Some(helps) = joined(db, &diag.helps) {
        builder = builder.with_help(helps);
    }

    // Every label is in the one source here, so the first of them says which.
    let source_text = diag.labels.first().map(|l| l.text.as_str(db)).unwrap_or("");

    sink.emit(builder.finish(), (&file_name, Source::from(source_text)));
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

    // One call each, not one per line. ariadne's report holds a single note
    // and a single help, so `with_note` in a loop keeps the last and drops
    // every one before it -- a diagnostic that carefully explained itself in
    // three notes printed one. Joined, they all arrive.
    if let Some(notes) = joined(db, &diag.notes) {
        builder = builder.with_note(notes);
    }
    if let Some(helps) = joined(db, &diag.helps) {
        builder = builder.with_help(helps);
    }

    Sink::Stderr.emit(builder.finish(), MultiSourceCache { sources });
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

/// A line of source with text inserted into it, over `+` markers.
///
/// What a reader has to do to the line is easier to see written out than
/// described, so the suggestion shows the line as it would be. It carries no
/// line number or gutter of its own: it is meant for a help, which the
/// renderer frames, about a span a label already points at.
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

    let original_line = &source[line_start..line_end];
    let col = insert_pos.checked_sub(line_start).X();

    let mut modified_line = original_line.S();
    modified_line.insert_str(col, insertion);

    let indent = "    ";
    let plus_markers = "+".repeat(insertion.len());
    Some(fmt!("{indent}{modified_line}\n{indent}{:col$}{plus_markers}", ""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::DiagnosticBuilder;
    use crate::input::Source as Input;
    use crate::text::TextSpan;

    /// Render a diagnostic over `src`, with a primary label and a secondary.
    fn rendered(src: &str, primary: Range<usize>, secondary: Option<Range<usize>>) -> String {
        let ref db = crate::Database::default();
        let source = Input::new(db, src.S());
        let text = crate::source_map::basic_source_map(db, source).text(db);

        let mut b = DiagnosticBuilder::warning(db, "`w` is a length, and this adds an angle to one")
            .primary_label(TextSpan::new(text, primary), "an angle")
            .note("a length and an angle are not the same quantity")
            .did_you_mean("widt", ["width", "height"]);
        if let Some(s) = secondary {
            b = b.secondary_label(TextSpan::new(text, s), "a length");
        }
        let mut r = Renderer::new();
        r.to_string(db, &b.build(), Path::new("panel.fui"), Path::new(""))
    }

    #[test]
    fn a_rendered_string_is_plain_ascii() {
        let src = "stack { w = 10px + 5deg }\n";
        assert_eq!(&src[19..23], "5deg");
        assert_eq!(&src[12..16], "10px");
        let out = rendered(src, 19..23, Some(12..16));

        // No escape sequences: what asks for a string is a test, a log, or a
        // window drawing the glyphs itself.
        assert!(!out.contains('\u{1b}'), "{out:?}");
        // And no box-drawing, for the same reason.
        assert!(out.is_ascii(), "{out:?}");

        // The parts a caller depends on.
        assert!(out.contains("panel.fui"), "{out}");
        assert!(out.contains("is a length, and this adds an angle to one"), "{out}");
        assert!(out.contains("an angle"), "{out}");
        assert!(out.contains("a length"), "{out}");
        assert!(out.contains("a length and an angle are not the same quantity"), "{out}");
        assert!(out.contains("did you mean `width`?"), "{out}");
    }

    #[test]
    fn the_frame_names_the_line_and_column_of_the_primary_label() {
        let src = "stack {\n    w = 10px + 5deg\n}\n";
        // `5deg` begins at byte 23, which is line 2, column 16.
        assert_eq!(&src[23..27], "5deg");
        let out = rendered(src, 23..27, None);
        assert!(out.contains("panel.fui:2:16"), "{out}");
    }

    #[test]
    fn a_run_renders_every_diagnostic_it_was_given() {
        let ref db = crate::Database::default();
        let src = "a\nb\nc\n";
        let source = Input::new(db, src.S());
        let text = crate::source_map::basic_source_map(db, source).text(db);
        let diags: Vec<_> = (0..3)
            .map(|i| {
                DiagnosticBuilder::error(db, &format!("problem {i}"))
                    .primary_label(TextSpan::new(text, i * 2..i * 2 + 1), "here")
                    .build()
            })
            .collect();

        let out = render_diagnostics_to_string(db, diags, Path::new("t.fui"), Path::new(""));
        for i in 0..3 {
            assert!(out.contains(&format!("problem {i}")), "{out}");
        }
    }

    /// A span with nothing to say about it is still underlined.
    ///
    /// ariadne draws a label with no message not at all. `primary_span` and
    /// `secondary_span` were therefore silent no-ops, which nothing noticed
    /// because nothing in this repository calls them.
    #[test]
    fn a_label_without_a_message_is_still_underlined() {
        let ref db = crate::Database::default();
        let src = "radius = 15\n";
        let source = Input::new(db, src.S());
        let text = crate::source_map::basic_source_map(db, source).text(db);
        let diag = DiagnosticBuilder::error(db, "not a length")
            .primary_span(TextSpan::new(text, 9..11))
            .build();

        let out = Renderer::new().to_string(db, &diag, Path::new("t.fui"), Path::new(""));
        assert!(out.contains('^'), "no underline was drawn:\n{out}");
        assert!(out.contains("radius = 15"), "{out}");
    }

    /// And one with a message says it, which is the shape to prefer.
    #[test]
    fn a_label_with_a_message_says_it() {
        let ref db = crate::Database::default();
        let src = "w = 10px + 5deg\n";
        let source = Input::new(db, src.S());
        let text = crate::source_map::basic_source_map(db, source).text(db);
        let diag = DiagnosticBuilder::warning(db, "a length and an angle")
            .primary_label(TextSpan::new(text, 11..15), "an angle")
            .secondary_label(TextSpan::new(text, 4..8), "a length")
            .build();

        let out = Renderer::new().to_string(db, &diag, Path::new("t.fui"), Path::new(""));
        assert!(out.contains("an angle"), "{out}");
        assert!(out.contains("a length"), "{out}");
        assert!(!out.contains('\u{1b}'), "a string should carry no colour:\n{out}");
    }

    /// A help of several lines keeps the frame on each of them.
    ///
    /// ariadne writes a help as one row, so its later lines came out at
    /// column zero, under the frame rather than in it.
    #[test]
    fn a_multi_line_help_is_framed() {
        let ref db = crate::Database::default();
        let src = "let b = a\nlet c = a\n";
        let source = Input::new(db, src.S());
        let text = crate::source_map::basic_source_map(db, source).text(db);
        let suggestion = insertion_suggestion(src, &(8..9), "@").X();
        let diag = DiagnosticBuilder::error(db, "use of moved value")
            .primary_label(TextSpan::new(text, 18..19), "used after move")
            .help(&fmt!("insert `@` to clone:\n{suggestion}"))
            .build();

        let out = Renderer::new().to_string(db, &diag, Path::new("t.dfs"), Path::new(""));
        let help: Vec<_> = out.lines().skip_while(|l| !l.contains("Help:")).take(3).collect();
        assert_eq!(help, [
            "   | Help: insert `@` to clone:",
            "   |           let b = a@",
            "   |                    +",
        ], "{out}");
    }

    /// Every note is printed, not just the last one.
    ///
    /// ariadne's report holds one note and one help, so `with_note` called
    /// twice keeps the second. A diagnostic that explained itself in three
    /// notes -- what the colour is, what will be drawn instead, and which
    /// field it should have gone in -- printed the third.
    #[test]
    fn all_of_the_notes_and_helps_are_printed() {
        let ref db = crate::Database::default();
        let src = "albedo = #ff8a65\n";
        let source = Input::new(db, src.S());
        let text = crate::source_map::basic_source_map(db, source).text(db);
        let diag = DiagnosticBuilder::warning(db, "brighter than any real surface")
            .primary_label(TextSpan::new(text, 9..16), "too bright")
            .note("the lightest hex that passes is #f38360")
            .note("drawing it as #f38360 so the rest of the scene still shows")
            .note("`albedo` is reflectance; a light this bright goes in `emission`")
            .help("write #f38360 or darker")
            .help("or move it to `emission`, where there is no ceiling")
            .build();

        let out = Renderer::new().to_string(db, &diag, Path::new("t.fui"), Path::new(""));
        for line in [
            "the lightest hex that passes is #f38360",
            "drawing it as #f38360",
            "`albedo` is reflectance",
            "write #f38360 or darker",
            "or move it to `emission`",
        ] {
            assert!(out.contains(line), "`{line}` was dropped:\n{out}");
        }
    }
}
