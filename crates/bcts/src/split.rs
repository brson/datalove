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
use rmx::std::mem;

use crate::bracer::TreeToken;
use crate::diagnostic::DiagnosticBuilder;
use crate::lexer::{Sigil, TokenKind};
use crate::text::{Text, TextSpan};

/// A run of tokens between two delimiters, with the whitespace taken out.
#[derive(Clone)]
pub struct TokenGroup<'db> {
    pub tokens: Vec<TreeToken<'db>>,
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

/// Split on line delimiters: a newline inside whitespace, or `;`.
pub fn split_lines<'db>(
    db: &'db dyn crate::Db,
    tokens: impl IntoIterator<Item = TreeToken<'db>>,
) -> Vec<TokenGroup<'db>> {
    let mut groups = vec![];
    let mut current = vec![];

    for token in tokens {
        let delimiter = match &token {
            TreeToken::Token(t) => match t.kind {
                TokenKind::Whitespace if t.text.as_str(db).contains('\n') => {
                    Some(Delimiter::Newline)
                }
                TokenKind::Sigil(sigil @ Sigil::Semicolon) => Some(Delimiter::Written(Written {
                    sigil,
                    count: 1,
                    span: t.span(),
                })),
                _ => None,
            },
            TreeToken::Branch { .. } => None,
        };

        match delimiter {
            Some(end) => groups.push(TokenGroup {
                tokens: mem::take(&mut current),
                end: Some(end),
            }),
            None => {
                if let Some(token) = token.without_space() {
                    current.push(token);
                }
            }
        }
    }

    groups.push(TokenGroup { tokens: current, end: None });
    groups
}

/// Split on separators of `level` commas.
///
/// A shorter run stays inside the group it fell in, which is what gives a
/// tensor its axes: the `,` between two rows belongs to the slab that `,,`
/// separates, and is split from it one level down. A longer run is that many
/// separators rather than one, so the group between two of them is empty and
/// the caller hears about it.
pub fn split_commas<'db>(
    tokens: impl IntoIterator<Item = TreeToken<'db>>,
    level: usize,
) -> Vec<TokenGroup<'db>> {
    assert!(level > 0, "a separator is at least one comma");
    let tokens: Vec<_> = tokens.into_iter().filter_map(|t| t.without_space()).collect();

    let mut groups = vec![];
    let mut current = vec![];
    let mut i = 0;

    while i < tokens.len() {
        let run = comma_run(&tokens[i..]);
        if run == 0 {
            current.push(tokens[i].C());
            i = i.checked_add(1).X();
            continue;
        }
        if run >= level {
            let end = i.checked_add(level).X();
            groups.push(TokenGroup {
                tokens: mem::take(&mut current),
                end: Some(Delimiter::Written(Written {
                    sigil: Sigil::Comma,
                    count: level,
                    span: token_span(&tokens[i]).start..token_span(&tokens[end.checked_sub(1).X()]).end,
                })),
            });
            i = end;
        } else {
            // Below this level the run is not a separator, so it travels on
            // to whichever level it does separate.
            current.extend(tokens[i..i.checked_add(run).X()].iter().cloned());
            i = i.checked_add(run).X();
        }
    }

    groups.push(TokenGroup { tokens: current, end: None });
    groups
}

/// The longest run of commas anywhere in the tokens, which must already have
/// had their whitespace taken out.
///
/// A tensor's rank is one more than this: the widest separator written is the
/// outermost axis, and the elements inside the innermost one are separated by
/// spaces rather than by anything.
pub fn max_comma_run<'db>(tokens: &[TreeToken<'db>]) -> usize {
    (0..tokens.len()).map(|i| comma_run(&tokens[i..])).max().unwrap_or(0)
}

/// How many commas a slice of tokens starts with.
fn comma_run<'db>(tokens: &[TreeToken<'db>]) -> usize {
    tokens
        .iter()
        .take_while(|t| {
            matches!(t, TreeToken::Token(t) if t.kind == TokenKind::Sigil(Sigil::Comma))
        })
        .count()
}

/// A comma's span, the only kind of token [`split_commas`] asks for one from.
fn token_span<'db>(token: &TreeToken<'db>) -> Range<usize> {
    let TreeToken::Token(token) = token else { bug!() };
    token.span()
}

/// The written delimiters that closed a group with nothing in it.
///
/// A newline is not one of them however many of them there are, since it
/// separates whatever it happens to fall between and a blank line falls
/// between nothing. A trailing `;` or `,` is not one either: it closes the
/// group before it, and the empty group after it is closed by the end of the
/// input rather than by another delimiter.
pub fn stray_delimiters<'db>(groups: &[TokenGroup<'db>]) -> Vec<Written> {
    groups
        .iter()
        .filter(|group| group.is_empty())
        .filter_map(|group| match &group.end {
            Some(Delimiter::Written(written)) => Some(written.C()),
            Some(Delimiter::Newline) | None => None,
        })
        .collect()
}

/// The diagnostic for a delimiter written with nothing before it.
///
/// Built here rather than at each parser so that the two of them say the same
/// thing: a reader moving between the languages is reading one literal syntax
/// and should not have to learn its complaints twice. `what` names the pair
/// the delimiter was meant to go between - rows, columns, statements.
pub fn stray_delimiter_error<'db>(
    db: &'db dyn crate::Db,
    text: Text<'db>,
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
    text: Text<'db>,
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
/// The tokens must already have had their whitespace taken out. What comes
/// before the first `|` is the header: whole numbers, one per axis, of which
/// there has to be at least one, since a tensor has at least one axis.
pub fn split_tensor_header<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
) -> (Option<Result<TensorHeader, TensorHeaderError>>, Vec<TreeToken<'db>>) {
    let is_pipe = |t: &TreeToken<'db>| {
        matches!(t, TreeToken::Token(t) if t.kind == TokenKind::Sigil(Sigil::Pipe))
    };
    let Some(pipe) = tokens.iter().position(is_pipe) else {
        return (None, tokens);
    };
    let mut tokens = tokens;
    let body = tokens.split_off(pipe.checked_add(1).X());
    let pipe_token = tokens.pop().X();
    let header = tokens;

    let start = header.first().unwrap_or(&pipe_token).span().start;
    let span = start..pipe_token.span().end;
    if header.is_empty() {
        return (Some(Err(TensorHeaderError {
            span,
            message: S("a tensor's shape has no extents"),
            label: S("a tensor has at least one axis, and the shape gives each its extent"),
        })), body);
    }

    let mut extents = vec![];
    for token in &header {
        let extent = match token {
            TreeToken::Token(t) => t.word_str(db)
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
pub fn nonempty_groups<'db>(groups: Vec<TokenGroup<'db>>) -> Vec<Vec<TreeToken<'db>>> {
    groups
        .into_iter()
        .map(|group| group.tokens)
        .filter(|tokens| !tokens.is_empty())
        .collect()
}

#[cfg(test)]
fn tree_tokens<'db>(db: &'db crate::Database, text: &str) -> Vec<TreeToken<'db>> {
    let source = crate::input::Source::new(db, text.S());
    let chunk = crate::source_map::basic_source_map(db, source);
    let chunk_lex = crate::lexer::lex_chunk(db, chunk);
    crate::bracer::bracer(db, chunk_lex).iter(db).collect()
}

#[cfg(test)]
fn shape<'db>(groups: &[TokenGroup<'db>]) -> (Vec<usize>, Vec<String>) {
    (
        groups.iter().map(|g| g.tokens.len()).collect(),
        stray_delimiters(groups).iter().map(|w| w.as_string()).collect(),
    )
}

#[cfg(test)]
fn line_shape(db: &crate::Database, text: &str) -> (Vec<usize>, Vec<String>) {
    shape(&split_lines(db, tree_tokens(db, text)))
}

#[test]
fn test_split_lines() {
    let ref db = crate::Database::default();
    let none: Vec<String> = vec![];
    // A blank line separates nothing and is nobody's mistake.
    assert_eq!(line_shape(db, "a\nb"), (vec![1, 1], none.C()));
    // A run of them is one whitespace token, so it is one delimiter.
    assert_eq!(line_shape(db, "\n\na\n\nb\n\n"), (vec![0, 1, 1, 0], none.C()));
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
        let tokens: Vec<_> = tree_tokens(db, text)
            .into_iter()
            .filter_map(|t| t.without_space())
            .collect();
        max_comma_run(&tokens)
    };
    assert_eq!(run("a b"), 0);
    assert_eq!(run("a, b"), 1);
    assert_eq!(run("a, b,, c"), 2);
    assert_eq!(run("a,,, b, c"), 3);
    // Whitespace is out by then, so a space does not break a run.
    assert_eq!(run("a, , b"), 2);
}
