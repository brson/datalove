//! Splitting a run of tokens on the delimiters between its parts.
//!
//! A parser that drops a delimiter it was not expecting accepts text that
//! means nothing, and says nothing about it. So splitting keeps every group,
//! the empty ones included, and records what closed each one: a caller can
//! then tell a blank line, which separates nothing and is no one's mistake,
//! from a `;` or a `,` that was written to separate two things and was given
//! only one of them.

use rmx::prelude::*;

use rmx::core::ops::Range;

use crate::bracer::{Cursor, TreeToken};
use crate::diagnostic::DiagnosticBuilder;
use crate::lexer::{Sigil, TokenKind};
use crate::input::Source;
use crate::text::TextSpan;

/// A run of tokens between two delimiters.
#[derive(Clone)]
pub struct TokenGroup<'db> {
    pub tokens: Cursor<'db>,
    /// What closed the group, absent where the input ran out instead.
    pub end: Option<Delimiter>,
}

impl<'db> TokenGroup<'db> {
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// What closed a group.
#[derive(Clone)]
pub enum Delimiter {
    /// A newline, which falls between whatever happens to be on either side.
    Newline,
    /// A `;`, or the run of `,` a tensor separates an axis with: written to
    /// separate two particular things.
    Written(Written),
}

/// A delimiter someone wrote, which is one sigil or a run of them.
#[derive(Clone)]
pub struct Written {
    pub sigil: Sigil,
    pub count: usize,
    pub span: Range<usize>,
}

impl Written {
    /// The delimiter as it was written, `,,` for a run of two.
    pub fn as_string(&self) -> String {
        self.sigil.as_str().repeat(self.count)
    }
}

/// Split on line delimiters: a newline outside a comment, or `;`.
///
/// A newline is kept on the token after it, so one at the end of the tokens
/// is not seen, and closes nothing that the end would not.
pub fn split_lines<'db>(tokens: Cursor<'db>) -> Vec<TokenGroup<'db>> {
    Lines::new(tokens).collect()
}

/// The groups [`split_lines`] makes, one at a time.
///
/// Each is a cursor over its part of the level, so a source's lines are
/// found rather than collected: what a line holds stays where the lexer put
/// it.
#[derive(Clone)]
pub struct Lines<'db> {
    rest: Cursor<'db>,
    /// Where the group being read began.
    start: u32,
    /// A group a `;` closed straight after a newline closed the one before.
    pending: Option<TokenGroup<'db>>,
    finished: bool,
}

impl<'db> Lines<'db> {
    pub fn new(tokens: Cursor<'db>) -> Self {
        Lines { rest: tokens, start: tokens.position(), pending: None, finished: false }
    }
}

impl<'db> Iterator for Lines<'db> {
    type Item = TokenGroup<'db>;

    fn next(&mut self) -> Option<TokenGroup<'db>> {
        if let Some(group) = self.pending.take() {
            return Some(group);
        }
        if self.finished {
            return None;
        }
        while let Some(index) = self.rest.next_index() {
            let token = self.rest.raw(index);
            let mut closed = None;
            if token.newline_before {
                closed = Some(TokenGroup {
                    tokens: self.rest.between(self.start, index),
                    end: Some(Delimiter::Newline),
                });
                self.start = index;
            }
            if self.rest.is_plain(index) && token.kind == TokenKind::Sigil(Sigil::Semicolon) {
                let group = TokenGroup {
                    tokens: self.rest.between(self.start, index),
                    end: Some(Delimiter::Written(Written {
                        sigil: Sigil::Semicolon,
                        count: 1,
                        span: token.span(),
                    })),
                };
                self.start = index.checked_add(1).X();
                match closed {
                    Some(_) => self.pending = Some(group),
                    None => closed = Some(group),
                }
            }
            if closed.is_some() {
                return closed;
            }
        }
        self.finished = true;
        Some(TokenGroup { tokens: self.rest.between(self.start, self.rest.end()), end: None })
    }
}

/// Whether the level hands out a comma at `index`.
fn is_comma(tokens: &Cursor<'_>, index: u32) -> bool {
    tokens.is_plain(index) && tokens.raw(index).kind == TokenKind::Sigil(Sigil::Comma)
}

/// Split on separators of `level` commas.
///
/// A shorter run stays inside the group it fell in, which is what gives a
/// tensor its axes: the `,` between two rows belongs to the slab that `,,`
/// separates, and is split from it one level down. A longer run is that many
/// separators rather than one, so the group between two of them is empty and
/// the caller hears about it.
pub fn split_commas<'db>(tokens: Cursor<'db>, level: usize) -> Vec<TokenGroup<'db>> {
    assert!(level > 0, "a separator is at least one comma");
    let indices: Vec<u32> = std::iter::from_fn({
        let mut rest = tokens;
        move || rest.next_index()
    }).collect();

    let mut groups = vec![];
    let mut start = tokens.position();
    let mut i = 0;

    while i < indices.len() {
        let run = indices[i..].iter().take_while(|&&index| is_comma(&tokens, index)).count();
        if run >= level {
            let end = i.checked_add(level).X();
            let first = tokens.raw(indices[i]).span();
            let last = tokens.raw(indices[end.checked_sub(1).X()]).span();
            groups.push(TokenGroup {
                tokens: tokens.between(start, indices[i]),
                end: Some(Delimiter::Written(Written {
                    sigil: Sigil::Comma,
                    count: level,
                    span: first.start..last.end,
                })),
            });
            start = indices.get(end).copied().unwrap_or(tokens.end());
            i = end;
        } else {
            // Below this level the run is not a separator, so it travels on
            // to whichever level it does separate.
            i = i.checked_add(run.max(1)).X();
        }
    }

    groups.push(TokenGroup { tokens: tokens.between(start, tokens.end()), end: None });
    groups
}

/// The longest run of commas anywhere in the tokens.
///
/// A tensor's rank is one more than this: the widest separator written is the
/// outermost axis, and the elements inside the innermost one are separated by
/// spaces rather than by anything.
pub fn max_comma_run<'db>(tokens: Cursor<'db>) -> usize {
    let mut rest = tokens;
    let mut longest = 0;
    let mut run = 0;
    while let Some(index) = rest.next_index() {
        if is_comma(&tokens, index) {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

/// The written delimiters that closed a group with nothing in it.
///
/// A newline is not one of them however many of them there are, since it
/// separates whatever it happens to fall between and a blank line falls
/// between nothing. A trailing `;` or `,` is not one either: it closes the
/// group before it, and the empty group after it is closed by the end of the
/// input rather than by another delimiter.
pub fn stray_delimiters<'db>(groups: &[TokenGroup<'db>]) -> Vec<Written> {
    groups.iter().filter_map(stray_delimiter).collect()
}

/// The written delimiter that closed a group with nothing in it, if one did;
/// see [`stray_delimiters`].
pub fn stray_delimiter(group: &TokenGroup<'_>) -> Option<Written> {
    if !group.is_empty() {
        return None;
    }
    match &group.end {
        Some(Delimiter::Written(written)) => Some(written.C()),
        Some(Delimiter::Newline) | None => None,
    }
}

/// The diagnostic for a delimiter written with nothing before it.
///
/// Built here rather than at each parser so that the two of them say the same
/// thing: a reader moving between the languages is reading one literal syntax
/// and should not have to learn its complaints twice. `what` names the pair
/// the delimiter was meant to go between - rows, columns, statements.
pub fn stray_delimiter_error<'db>(
    db: &'db dyn crate::Db,
    text: Source,
    written: &Written,
    what: &str,
) -> DiagnosticBuilder<'db> {
    let sigil = written.as_string();
    DiagnosticBuilder::error(db, &fmt!("nothing before this `{sigil}`"))
        .primary_label(
            TextSpan::new(text, written.span.C()),
            &fmt!("a `{sigil}` goes between two {what}"),
        )
}

/// The delimiter that closed the last group, where nothing came after it.
///
/// A tensor's commas go between its parts and nowhere else, since a comma run
/// there says which axis it separates. One at the end would close nothing and
/// could only be read as saying something about the shape, which the header
/// says instead.
pub fn trailing_delimiter<'db>(groups: &[TokenGroup<'db>]) -> Option<Written> {
    let [.., before, last] = groups else { return None };
    if !last.is_empty() || last.end.is_some() {
        return None;
    }
    match &before.end {
        Some(Delimiter::Written(written)) => Some(written.C()),
        Some(Delimiter::Newline) | None => None,
    }
}

/// The diagnostic for a delimiter written with nothing after it.
pub fn trailing_delimiter_error<'db>(
    db: &'db dyn crate::Db,
    text: Source,
    written: &Written,
    what: &str,
) -> DiagnosticBuilder<'db> {
    let sigil = written.as_string();
    DiagnosticBuilder::error(db, &fmt!("nothing after this `{sigil}`"))
        .primary_label(
            TextSpan::new(text, written.span.C()),
            &fmt!("a `{sigil}` goes between two {what}"),
        )
}

/// A tensor literal's shape header, read off the front of its tokens.
pub struct TensorHeader {
    pub extents: Vec<u32>,
    /// The header and the `|` that ends it.
    pub span: Range<usize>,
}

/// What is wrong with a tensor's shape header.
pub struct TensorHeaderError {
    pub span: Range<usize>,
    pub message: String,
    pub label: String,
}

/// Split a tensor's tokens at the `|` that ends its shape header, if there
/// is one, and read the header's extents.
///
/// What comes before the first `|` is the header: whole numbers, one per axis, of which
/// there has to be at least one, since a tensor has at least one axis.
pub fn split_tensor_header<'db>(
    chunk_text: &str,
    tokens: Cursor<'db>,
) -> (Option<Result<TensorHeader, TensorHeaderError>>, Cursor<'db>) {
    let mut rest = tokens;
    let pipe = std::iter::from_fn(|| rest.next_index()).find(|&index| {
        tokens.is_plain(index) && tokens.raw(index).kind == TokenKind::Sigil(Sigil::Pipe)
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
            TreeToken::Token(t) => t.word_str(chunk_text)
                .filter(|w| crate::parser_util::is_decimal_run(w))
                .and_then(|w| crate::parser_util::strip_separators(w).parse::<u32>().ok()),
            TreeToken::Branch { .. } => None,
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

/// The groups with tokens in them, which is what a caller parses.
pub fn nonempty_groups<'db>(groups: Vec<TokenGroup<'db>>) -> Vec<Cursor<'db>> {
    groups
        .into_iter()
        .map(|group| group.tokens)
        .filter(|tokens| !tokens.is_empty())
        .collect()
}

#[cfg(test)]
fn tree_tokens<'db>(db: &'db crate::Database, text: &str) -> Cursor<'db> {
    let source = crate::input::Source::new(db, text.S());
    let chunk = crate::source_map::basic_source_map(db, source);
    let chunk_lex = crate::lexer::lex_chunk(db, chunk);
    crate::bracer::bracer(db, chunk_lex).iter(db)
}

#[cfg(test)]
fn shape<'db>(groups: &[TokenGroup<'db>]) -> (Vec<usize>, Vec<String>) {
    (
        groups.iter().map(|g| g.tokens.count()).collect(),
        stray_delimiters(groups).iter().map(|w| w.as_string()).collect(),
    )
}

#[cfg(test)]
fn line_shape(db: &crate::Database, text: &str) -> (Vec<usize>, Vec<String>) {
    shape(&split_lines(tree_tokens(db, text)))
}

#[test]
fn test_split_lines() {
    let ref db = crate::Database::default();
    let none: Vec<String> = vec![];
    // A blank line separates nothing and is nobody's mistake.
    assert_eq!(line_shape(db, "a\nb"), (vec![1, 1], none.C()));
    // A run of them is one delimiter, on the token after it, so the run at
    // the end is not one at all.
    assert_eq!(line_shape(db, "\n\na\n\nb\n\n"), (vec![0, 1, 1], none.C()));
    // A `;` written with nothing before it is.
    assert_eq!(line_shape(db, "a;b"), (vec![1, 1], none.C()));
    assert_eq!(line_shape(db, "a;;b"), (vec![1, 0, 1], vec![S(";")]));
    assert_eq!(line_shape(db, ";a"), (vec![0, 1], vec![S(";")]));
    assert_eq!(line_shape(db, "a\n;\nb"), (vec![1, 0, 0, 1], vec![S(";")]));
    // A trailing one closes the group before it and is not stray.
    assert_eq!(line_shape(db, "a;"), (vec![1, 0], none.C()));
    assert_eq!(line_shape(db, "a;\nb"), (vec![1, 0, 1], none.C()));
    // A newline inside a branch belongs to the branch.
    assert_eq!(line_shape(db, "a(b\nc)d"), (vec![3], none.C()));
}

#[cfg(test)]
fn comma_shape(db: &crate::Database, text: &str, level: usize) -> (Vec<usize>, Vec<String>) {
    shape(&split_commas(tree_tokens(db, text), level))
}

#[test]
fn test_split_commas() {
    let ref db = crate::Database::default();
    let none: Vec<String> = vec![];
    assert_eq!(comma_shape(db, "a b, c d", 1), (vec![2, 2], none.C()));
    assert_eq!(comma_shape(db, "a,,b", 1), (vec![1, 0, 1], vec![S(",")]));
    assert_eq!(comma_shape(db, ",a", 1), (vec![0, 1], vec![S(",")]));
    // A trailing comma is what keeps a one-row tensor's rank.
    assert_eq!(comma_shape(db, "a b,", 1), (vec![2, 0], none.C()));
    // A run shorter than the level travels on to the level it separates.
    assert_eq!(comma_shape(db, "a, b,, c, d", 2), (vec![3, 3], none.C()));
    assert_eq!(comma_shape(db, "a,,,,b", 4), (vec![1, 1], none.C()));
    // A run longer than the level separates once and hands the rest down,
    // where the level that does own them splits on them.
    assert_eq!(comma_shape(db, "a,,,b", 2), (vec![1, 2], none.C()));
    assert_eq!(comma_shape(db, ",b", 1), (vec![0, 1], vec![S(",")]));
}

#[test]
fn test_max_comma_run() {
    let ref db = crate::Database::default();
    let run = |text: &str| {
        max_comma_run(tree_tokens(db, text))
    };
    assert_eq!(run("a b"), 0);
    assert_eq!(run("a, b"), 1);
    assert_eq!(run("a, b,, c"), 2);
    assert_eq!(run("a,,, b, c"), 3);
    // Whitespace is out by then, so a space does not break a run.
    assert_eq!(run("a, , b"), 2);
}
