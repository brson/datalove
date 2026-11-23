//! Re-export parser utilities from bcts.

use rmx::prelude::*;
use datalove_diagnostic_codes::DCode;
pub use bct::parser_util::{
    eat_number, is_decimal_run, is_sigil, is_hex_word, is_identifier, is_numeric_literal, is_number_word,
    strip_separators,
    Number, Radix, TokenStream, TokenStreamExt,
};

// Re-export TextSpan from bct for convenience.
pub use bct::text::TextSpan;

/// What to say about a number written with letters on the end.
///
/// datalove has no numeric suffixes: a type is written in a hint before the
/// value, not against its digits. Said here so that both languages say it the
/// same way.
pub fn suffix_complaint(suffix: &str) -> (String, String) {
    (
        rmx::std::format!("numeric suffixes are not supported"),
        rmx::std::format!("`{suffix}` is not part of the number; write a type hint, as in `: u8 / 1`"),
    )
}

/// What a string literal holds: the text between its quotes, escapes read.
///
/// Both parsers call this on every string they read and report what it
/// refuses, so that a value built later can take the literal as read.
pub fn string_literal_value(raw: &str) -> Result<String, bct::escapes::EscapeError> {
    let inner = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
        .expect("the lexer gives a string token its quotes");
    bct::escapes::process_escape_sequences(inner)
}

/// What to say about an escape a string literal cannot hold.
pub fn escape_complaint(error: &bct::escapes::EscapeError) -> (String, String) {
    (
        "invalid escape in string literal".S(),
        rmx::std::format!("{error}; the escapes are \\\" \\\\ \\n \\r \\t \\0 and \\u{{...}}"),
    )
}

/// Where a parser's diagnostics go: filed as they are made, or kept.
///
/// A chunk keeps them, and hands them back to the parse that asked for it to
/// file at the place in the collection they were made, which keeps them in
/// the order a parse of the whole collection would have filed them in. Shared
/// with the chunk's sub-parsers, for the same reason.
pub type Sink = Option<std::rc::Rc<std::cell::RefCell<Vec<bct::diagnostic::StoredDiagnostic>>>>;

/// Filing a diagnostic where a parser's diagnostics go.
pub trait EmitTo {
    fn emit_to(self, sink: &Sink);
}

impl EmitTo for bct::diagnostic::DiagnosticBuilder<'_> {
    fn emit_to(self, sink: &Sink) {
        match sink {
            Some(kept) => kept.borrow_mut().push(self.build_stored()),
            None => datalove_diagnostic::DiagnosticBuilderExt::emit_parse(self),
        }
    }
}

/// File a diagnostic built earlier, as [`EmitTo`] files one being built.
pub fn emit_stored(db: &dyn salsa::Database, sink: &Sink, diagnostic: bct::diagnostic::StoredDiagnostic) {
    match sink {
        Some(kept) => kept.borrow_mut().push(diagnostic),
        None => datalove_diagnostic::ParseDiagnostic::emit_stored(db, diagnostic),
    }
}

/// The names a list has given so far, for refusing one given twice.
///
/// Struct fields, table columns and enum variants are each named once. Both
/// languages read their names through this, so they say the same thing about
/// a repeated one.
#[derive(Default)]
pub struct SeenNames<'db>(bct::lookup::LookupSet<bct::text::InternedText<'db>>);

impl<'db> SeenNames<'db> {
    /// Take a name, reporting it if it was taken already. False if it was.
    pub fn take(
        &mut self,
        db: &'db dyn salsa::Database,
        sink: &Sink,
        name: bct::text::InternedText<'db>,
        ts: TextSpan,
        what: &str,
    ) -> bool {
        match self.take_reported(db, name, ts, what) {
            None => true,
            Some(twice) => {
                twice.emit_to(sink);
                false
            }
        }
    }

    /// Take a name, or say what to report if it was taken already.
    ///
    /// The report is handed back rather than filed, so that a parser that
    /// keeps its diagnostics (a chunk's; see `crate::chunk_plan`) keeps this one too.
    #[must_use]
    pub fn take_reported(
        &mut self,
        db: &'db dyn salsa::Database,
        name: bct::text::InternedText<'db>,
        ts: TextSpan,
        what: &str,
    ) -> Option<bct::diagnostic::DiagnosticBuilder<'db>> {
        if self.0.insert(name) {
            return None;
        }
        let name = name.as_str(db);
        Some(bct::diagnostic::DiagnosticBuilder::error(db, &rmx::std::format!("`{name}` is named twice"))
            .code(DCode::DuplicateName.as_str())
            .primary_label(ts, &rmx::std::format!("a {what} is named once")))
    }
}

/// The kind of thing a declaration names, which decides what it may not be
/// called.
///
/// Datalove has no words reserved everywhere. A word is reserved for a kind
/// of name only where it can stand where that name is read and mean something
/// else there; see `botdocs/botspec.md` §2.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// A `let`, `var` or `const`, or a binding in `if`, `else` or `case`.
    Value,
    /// A function's parameter.
    Parameter,
    /// A function.
    Function,
    /// A type alias or type parameter.
    Type,
    /// A module or rider, under the alias `require` gives it.
    Module,
}

/// Words that start an expression, and so cannot be read as a value's name.
pub const EXPRESSION_WORDS: &[&str] = &[
    "true", "false", "none", "some", "ok", "er", "data", "error",
    "atom", "term", "enum", "not", "icall",
];

/// Words written before a parameter's name to say how it is passed.
pub const PARAMETER_MODE_WORDS: &[&str] = &["mut", "out", "ref", "const"];

/// The names of the primitive types.
pub const PRIMITIVE_TYPE_NAMES: &[&str] = &[
    "bool", "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64",
    "index", "offset", "f32", "f64", "int", "string", "data", "error",
];

/// Words that are a type, or start one, where a type is read.
pub fn is_type_word(word: &str) -> bool {
    PRIMITIVE_TYPE_NAMES.contains(&word) || matches!(word, "atom" | "term" | "enum" | "rec")
}

/// Why a word cannot name a thing of this kind, if it cannot.
pub fn reserved_for(word: &str, kind: NameKind) -> Option<&'static str> {
    let expression = EXPRESSION_WORDS.contains(&word);
    match kind {
        NameKind::Value if expression => Some("it starts an expression"),
        NameKind::Parameter if expression => Some("it starts an expression"),
        NameKind::Parameter if PARAMETER_MODE_WORDS.contains(&word) => {
            Some("it says how a parameter is passed")
        }
        NameKind::Function if expression => Some("it starts an expression"),
        NameKind::Module if expression => Some("it starts an expression"),
        NameKind::Type if is_type_word(word) => Some("it is a type, or starts one"),
        _ => None,
    }
}

/// What to say about a declaration named with a reserved word.
pub fn reserved_complaint(word: &str, reason: &str) -> (String, String) {
    (
        rmx::std::format!("`{word}` cannot be declared here"),
        rmx::std::format!("`{word}` is reserved, since {reason}"),
    )
}

/// What a type name that names nothing may have been meant as.
///
/// A primitive written in another case, or the name of a collection, whose
/// type is written with a sigil. Asked only once no alias or parameter of the
/// name was found, since both are free to use these names.
pub fn type_name_suggestion(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    if PRIMITIVE_TYPE_NAMES.contains(&lower.as_str()) {
        return Some(rmx::std::format!("did you mean `{lower}`?"));
    }
    let spelled = match lower.as_str() {
        "tuple" => "(T, ...)",
        "list" => "[T]",
        "map" => "%{K -> V}",
        "set" => "#{T}",
        "table" => "{| name: T |}",
        "tensor" => "[|T, N|]",
        _ => return None,
    };
    Some(rmx::std::format!("a {lower} type is written `{spelled}`"))
}

/// A tensor literal's shape header, read off the front of its tokens.
pub struct TensorHeader {
    pub extents: Vec<u32>,
    /// The header and the `|` that ends it.
    pub span: rmx::core::ops::Range<usize>,
}

/// What is wrong with a tensor's shape header.
pub struct TensorHeaderError {
    pub span: rmx::core::ops::Range<usize>,
    pub message: String,
    pub label: String,
}

/// Split a tensor's tokens at the `|` that ends its shape header, if there
/// is one, and read the header's extents.
///
/// What comes before the first `|` is the header: whole numbers, one per axis, of which
/// there has to be at least one, since a tensor has at least one axis.
pub fn split_tensor_header<'db>(
    text: &str,
    tokens: bct::bracer::Cursor<'db>,
) -> (Option<Result<TensorHeader, TensorHeaderError>>, bct::bracer::Cursor<'db>) {
    let mut rest = tokens;
    let pipe = std::iter::from_fn(|| rest.next_index()).find(|&index| {
        tokens.is_plain(index) && tokens.raw(index).kind == bct::lexer::TokenKind::Sigil(bct::lexer::Sigil::Pipe)
    });
    let Some(pipe) = pipe else {
        return (None, tokens);
    };
    let header = tokens.between(tokens.position(), pipe);
    let body = tokens.between(pipe.checked_add(1).X(), tokens.end());
    let pipe_span = tokens.raw(pipe).span();

    let start = header.first().map_or(pipe_span.start, |token| token.span().start);
    let span = start..pipe_span.end;
    if header.is_empty() {
        return (Some(Err(TensorHeaderError {
            span,
            message: S("a tensor's shape has no extents"),
            label: S("a tensor has at least one axis, and the shape gives each its extent"),
        })), body);
    }

    let mut extents = vec![];
    for token in header {
        let extent = match &token {
            bct::bracer::TreeToken::Token(t) => t.word_str(text)
                .filter(|w| is_decimal_run(w))
                .and_then(|w| strip_separators(w).parse::<u32>().ok()),
            bct::bracer::TreeToken::Branch { .. } => None,
        };
        match extent {
            Some(extent) => extents.push(extent),
            None => return (Some(Err(TensorHeaderError {
                span: token.span(),
                message: S("a tensor's shape is written as whole numbers"),
                label: S("expected the extent of an axis"),
            })), body),
        }
    }
    (Some(Ok(TensorHeader { extents, span })), body)
}

/// What is wrong with a tensor body written under a shape header, if
/// anything.
///
/// A body under a header is written flat, with no commas, and filled in
/// row-major order, or shaped exactly as the header says. Both parsers ask
/// here so that they refuse the same bodies in the same words.
pub fn tensor_body_complaint(
    extents: &[u32],
    body_rank: usize,
    body_shape: &[u32],
    element_count: usize,
) -> Option<(String, String)> {
    let spelled = |shape: &[u32]| {
        shape.iter().map(|e| e.S()).collect::<Vec<_>>().join(" ")
    };
    if body_rank == 1 {
        // Extents are each a u32, so four of them can overflow a u64.
        let holds = extents.iter().try_fold(1u64, |n, &e| n.checked_mul(e as u64));
        if holds == Some(element_count as u64) {
            return None;
        }
        let holds = match holds {
            Some(n) => n.S(),
            None => "more than 2^64".S(),
        };
        return Some((
            rmx::std::format!("a tensor of shape `{}` holds {holds} elements, and {element_count} are written", spelled(extents)),
            "the shape".S(),
        ));
    }
    if body_shape == extents {
        return None;
    }
    Some((
        rmx::std::format!(
            "the shape says `{}`, and the body is shaped `{}`",
            spelled(extents), spelled(body_shape),
        ),
        "a body under a shape is written flat, or shaped as it says".S(),
    ))
}

/// Whether a tensor of this shape can only be written with a shape header.
///
/// Without one, the rank is read off the widest comma run between the
/// body's parts, and every extent off how many parts there are. So a zero
/// extent above the innermost axis cannot be written, and nor can a leading
/// extent of one, whose axis never shows a separator.
pub fn tensor_needs_header(shape: &[u32]) -> bool {
    shape.len() > 1 && (shape[0] == 1 || shape.contains(&0))
}

/// The errors a parse finds before it reads a statement: the brackets the
/// bracer could not match, a close with no open and an open never closed, and
/// names written outside ASCII.
///
/// Both parsers file these, the brackets and names being syntax they share.
/// The bracer has already recovered, so parsing goes on over its tree either
/// way.
pub fn prepass_errors<'db>(
    db: &'db dyn salsa::Database,
    bracer: bct::bracer::Bracer<'db>,
    source: bct::input::Source,
) -> Vec<bct::diagnostic::StoredDiagnostic> {
    use bct::diagnostic::DiagnosticBuilder;
    let tokens = bracer.lexed(db).tokens(db);
    let mut out = vec![];
    for (token_range, sigil) in bracer.errors(db) {
        // A stray close is reported as its own one-token range. An unclosed
        // open runs to wherever it ended, which is one token too where the
        // open is the last token of the source, so the token says which.
        let Some(token) = tokens.get(token_range.start) else { continue };
        let ts = TextSpan::new(source, token.span());
        let builder = if token.is_close_sigil() {
            DiagnosticBuilder::error(db, &rmx::std::format!("unmatched '{}'", sigil.as_str()))
                .code(DCode::UnmatchedClose.as_str())
                .primary_label(ts, &rmx::std::format!("this '{}' has no matching '{}'",
                    sigil.as_str(), sigil.open_sigil().as_str()))
        } else {
            DiagnosticBuilder::error(db, &rmx::std::format!("unclosed '{}'", sigil.as_str()))
                .code(DCode::UnclosedOpen.as_str())
                .primary_label(ts, &rmx::std::format!("this '{}' is never closed", sigil.as_str()))
        };
        out.push(builder.build_stored());
    }
    for span in bct::parser_util::non_ascii_names(tokens, source.as_str(db)) {
        out.push(bct::parser_util::non_ascii_name_error(db, source, span)
            .code(DCode::NonAsciiName.as_str())
            .build_stored());
    }
    out
}

/// Where each statement of a source begins, as byte offsets in order.
///
/// A statement is a logical line: it begins at a token with a line break
/// before it, outside brackets, which join the lines inside them, as the
/// bracer matched them. A datalit literal is one statement however many lines
/// it spans; its parse files with no statement starts at all.
pub fn statement_starts<'db>(db: &'db dyn salsa::Database, bracer: bct::bracer::Bracer<'db>) -> Vec<usize> {
    // A line's start is its first token, a stray close passed over included.
    bct::split::Lines::new(bracer.iter(db)).map(|line| line.raw(line.position()).span().start).collect()
}

/// File the first error of each statement, and every diagnostic that is not
/// an error.
///
/// A parse reports at most one error for each statement: the first, in the
/// order of the source, the one nearest the fault, with what it led to
/// dropped. A statement's errors are those whose primary label begins in it,
/// `starts` being where each statement begins; with none, the source is one.
pub fn file_first_error_per_statement(
    db: &dyn salsa::Database,
    starts: &[usize],
    diagnostics: Vec<bct::diagnostic::StoredDiagnostic>,
) {
    use bct::diagnostic::{LabelStyle, Severity};
    let at = |d: &bct::diagnostic::StoredDiagnostic| {
        d.labels.iter().find(|l| l.style == LabelStyle::Primary).or(d.labels.first()).map_or(0, |l| l.span.start)
    };
    // What comes before the first statement, a close passed over or a
    // comment, is part of it.
    let statement = |pos: usize| starts.partition_point(|&s| s <= pos).saturating_sub(1);
    let mut first: Vec<Option<(usize, usize)>> = vec![None; starts.len().max(1)];
    for (k, d) in diagnostics.iter().enumerate() {
        if d.severity != Severity::Error {
            continue;
        }
        let pos = at(d);
        let slot = &mut first[statement(pos)];
        if slot.is_none_or(|(best, _)| pos < best) {
            *slot = Some((pos, k));
        }
    }
    for (k, d) in diagnostics.into_iter().enumerate() {
        let keep = d.severity != Severity::Error || first[statement(at(&d))].is_some_and(|(_, best)| best == k);
        if keep {
            datalove_diagnostic::ParseDiagnostic::emit_stored(db, d);
        }
    }
}
