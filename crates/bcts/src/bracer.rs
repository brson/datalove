use rmx::prelude::*;

use rmx::core::ops::Range;

use crate::lexer::{Lexed, Token, TokenKind, Sigil};

#[cfg(test)]
use rmx::std::io::Write;

/// A source's brackets matched, with recovery: where each token stands among
/// them, and what was wrong.
#[salsa::tracked]
pub struct Bracer<'db> {
    #[returns(copy)]
    pub lexed: Lexed<'db>,
    /// One entry per token; see [`Cursor`].
    #[returns(ref)]
    pub layout: Vec<u32>,
    #[returns(ref)]
    pub errors: Vec<(Range<usize>, Sigil)>,
}

/// A token its level passes over: the close of a bracket, or a close that
/// recovery removed.
const HIDDEN: u32 = u32::MAX;
/// A token its level hands out as it is.
const PLAIN: u32 = u32::MAX - 1;

impl<'db> Bracer<'db> {
    /// Where the level around token `index` resumes, if the token opens a
    /// bracket: just past its close, or where recovery cut it off.
    pub fn resume(&self, db: &'db dyn crate::Db, index: usize) -> Option<usize> {
        match self.layout(db)[index] {
            HIDDEN | PLAIN => None,
            resume => Some(resume as usize),
        }
    }

    /// The source's tokens at the top level.
    pub fn iter(&self, db: &'db dyn crate::Db) -> Cursor<'db> {
        let tokens = self.lexed(db).tokens(db);
        Cursor {
            tokens,
            layout: self.layout(db),
            pos: 0,
            end: u32::try_from(tokens.len()).X(),
        }
    }
}

/// The tokens of one level of bracket nesting, in place in the lexer's array.
///
/// A level is a range of token indices, and the bracer's `layout` says, for
/// each token, whether the level hands it out (`PLAIN`), passes over it
/// (`HIDDEN`), or opens a bracket whose contents run to the index given, after
/// which the level carries on. A bracket comes out as one
/// [`TreeToken::Branch`] holding a cursor over its contents, so nothing is
/// copied to say what a line or a bracket holds: a cursor is a few words, and
/// `Copy`.
#[derive(Clone, Copy)]
pub struct Cursor<'db> {
    tokens: &'db [Token],
    layout: &'db [u32],
    /// The next index to look at, which may be one the level passes over.
    pos: u32,
    end: u32,
}

impl<'db> Cursor<'db> {
    /// Whether there is nothing left to hand out.
    pub fn is_empty(&self) -> bool {
        self.clone().next_index().is_none()
    }

    /// The token at the cursor, without moving it.
    pub fn first(&self) -> Option<TreeToken<'db>> {
        self.clone().next()
    }

    /// The `n`th token from the cursor, without moving it.
    pub fn peek_nth(&self, n: usize) -> Option<TreeToken<'db>> {
        self.clone().nth(n)
    }

    /// The index of the next token handed out, and the cursor moved past it.
    pub fn next_index(&mut self) -> Option<u32> {
        while self.pos < self.end {
            let index = self.pos;
            let entry = self.layout[index as usize];
            match entry {
                HIDDEN => self.pos = index + 1,
                PLAIN => {
                    self.pos = index + 1;
                    return Some(index);
                }
                resume => {
                    self.pos = resume;
                    return Some(index);
                }
            }
        }
        None
    }

    /// The token or bracket at `index`, which this level hands out.
    pub fn token_at(&self, index: u32) -> TreeToken<'db> {
        let token = &self.tokens[index as usize];
        match self.layout[index as usize] {
            PLAIN => TreeToken::Token(token.clone()),
            HIDDEN => bug!(),
            resume => {
                let TokenKind::Sigil(sigil) = token.kind else { bug!() };
                // The contents run to `resume`, the close included where
                // there is one, so the last token says whether there is.
                let last = &self.tokens[resume as usize - 1];
                let (close, end_byte) = if last.kind == TokenKind::Sigil(sigil.close_sigil()) {
                    (Some(last.clone()), last.span().end)
                } else {
                    (None, last.span().end)
                };
                TreeToken::Branch {
                    sigil,
                    open: token.clone(),
                    close,
                    end_byte,
                    inner: Cursor { tokens: self.tokens, layout: self.layout, pos: index + 1, end: resume },
                }
            }
        }
    }

    /// The raw token at `index`, whatever the level makes of it.
    pub fn raw(&self, index: u32) -> &'db Token {
        &self.tokens[index as usize]
    }

    /// Whether the level hands out the token at `index` as a plain token.
    pub fn is_plain(&self, index: u32) -> bool {
        self.layout[index as usize] == PLAIN
    }

    /// Where the cursor is: the next index it will look at.
    pub fn position(&self) -> u32 {
        self.pos
    }

    /// Where the level ends.
    pub fn end(&self) -> u32 {
        self.end
    }

    /// The same level from `from` to `to`, which have to be boundaries of it:
    /// indices this cursor stopped at, or its end.
    pub fn between(&self, from: u32, to: u32) -> Cursor<'db> {
        debug_assert!(from <= to && to <= self.end);
        Cursor { tokens: self.tokens, layout: self.layout, pos: from, end: to }
    }
}

impl<'db> Iterator for Cursor<'db> {
    type Item = TreeToken<'db>;

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let index = self.next_index()?;
        Some(self.token_at(index))
    }
}

#[derive(Clone)]
pub enum TreeToken<'db> {
    Token(Token),
    Branch {
        sigil: Sigil,
        open: Token,
        close: Option<Token>,
        end_byte: usize,
        inner: Cursor<'db>,
    },
}

/// Match a source's brackets, recovering from the ones that do not match.
///
/// A close finds the nearest open of its kind still waiting, and any opens
/// inside that one are cut off where it closes, each reported unclosed (P051).
/// A close that finds no open is reported (P050) and passed over. Whatever is
/// still open at the end runs to the end and is reported unclosed. A `}`
/// closes `{`, `%{` or `#{`, whichever is nearest.
///
/// The layout comes out of the same pass: an open bracket's entry is where
/// its level resumes, just past its close or where it was cut off, and every
/// close is passed over, matched or not. See [`Cursor`].
#[salsa::tracked(returns(copy))]
pub fn bracer<'db>(
    db: &'db dyn crate::Db,
    lexed: Lexed<'db>
) -> Bracer<'db> {
    let tokens = lexed.tokens(db);
    let count = tokens.len();
    let mut layout = vec![PLAIN; count];

    /// A bracket waiting for its close, and the errors found inside it, which
    /// go to the one around it once it closes.
    struct Open {
        index: usize,
        sigil: Sigil,
        errors: Vec<(Range<usize>, Sigil)>,
    }
    let mut stack: Vec<Open> = vec![];
    let mut top_errors = vec![];

    fn around<'a>(
        stack: &'a mut [Open],
        top: &'a mut Vec<(Range<usize>, Sigil)>,
    ) -> &'a mut Vec<(Range<usize>, Sigil)> {
        match stack.last_mut() {
            Some(open) => &mut open.errors,
            None => top,
        }
    }

    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::Sigil(sigil) = token.kind else { continue };
        let open_sigil = match sigil {
            Sigil::ParenOpen | Sigil::BraceOpen | Sigil::BracketOpen | Sigil::AngleOpen
            | Sigil::ParenPipeOpen | Sigil::BracePipeOpen | Sigil::BracketPipeOpen
            | Sigil::AnglePipeOpen | Sigil::PercentBraceOpen | Sigil::HashBraceOpen => {
                stack.push(Open { index, sigil, errors: vec![] });
                continue;
            }
            Sigil::BraceClose => stack.iter().rev()
                .map(|open| open.sigil)
                .find(|s| matches!(s, Sigil::BraceOpen | Sigil::PercentBraceOpen | Sigil::HashBraceOpen))
                .unwrap_or(Sigil::BraceOpen),
            Sigil::ParenClose | Sigil::BracketClose | Sigil::AngleClose | Sigil::ParenPipeClose
            | Sigil::BracePipeClose | Sigil::BracketPipeClose | Sigil::AnglePipeClose => sigil.open_sigil(),
            _ => continue,
        };

        layout[index] = HIDDEN;
        if !stack.iter().any(|open| open.sigil == open_sigil) {
            around(&mut stack, &mut top_errors).push((index..index.checked_add(1).X(), sigil));
            continue;
        }
        loop {
            let mut open = stack.pop().X();
            if open.sigil == open_sigil {
                layout[open.index] = u32::try_from(index.checked_add(1).X()).X();
                around(&mut stack, &mut top_errors).append(&mut open.errors);
                break;
            }
            // Cut off by the close of a bracket around it.
            open.errors.push((open.index..index, open.sigil));
            layout[open.index] = u32::try_from(index).X();
            around(&mut stack, &mut top_errors).append(&mut open.errors);
        }
    }

    while let Some(mut open) = stack.pop() {
        layout[open.index] = u32::try_from(count).X();
        let errors = around(&mut stack, &mut top_errors);
        errors.push((open.index..count, open.sigil));
        errors.append(&mut open.errors);
    }

    Bracer::new(db, lexed, layout, top_errors)
}

impl<'db> TreeToken<'db> {
    /// The byte range this token or branch covers.
    ///
    /// A branch runs from its open sigil to wherever it ended, which is its
    /// close where it has one and where the recovery put it where it does not.
    pub fn span(&self) -> Range<usize> {
        match self {
            TreeToken::Token(tok) => tok.span(),
            TreeToken::Branch { open, end_byte, .. } => open.span().start..*end_byte,
        }
    }

    /// The span of this token or branch in its source.
    pub fn text_span(&self, source_text: crate::input::Source) -> Option<crate::text::TextSpan> {
        Some(crate::text::TextSpan::new(source_text, self.span()))
    }

    /// Whether a newline was written between this and what came before it.
    pub fn newline_before(&self) -> bool {
        match self {
            TreeToken::Token(token) => token.newline_before,
            TreeToken::Branch { open, .. } => open.newline_before,
        }
    }
}

#[cfg(test)]
#[extension_trait]
impl<'db> VecTreeTokenExt<'db> for Vec<TreeToken<'db>> {
    fn debug_str(&self, text: &str) -> String {
        let mut buf = Vec::<u8>::new();
        Bracer::debug_write(self.iter().cloned(), &mut buf, text).X();
        String::from_utf8(buf).X()
    }
}

#[cfg(test)]
#[extension_trait]
pub impl<'db, I> IteratorOfTreeTokenExt<'db> for I
where I: Iterator<Item = TreeToken<'db>>
{
    fn debug_str(self, text: &str) -> String {
        let mut buf = Vec::<u8>::new();
        Bracer::debug_write(self, &mut buf, text).X();
        String::from_utf8(buf).X()
    }
}

#[cfg(test)]
impl<'db> Bracer<'db> {
    fn debug_str(&self, db: &'db dyn crate::Db) -> String {
        let mut buf = Vec::<u8>::new();
        let iter = self.iter(db);
        let text = self.lexed(db).source(db).as_str(db);
        Self::debug_write(iter, &mut buf, text).X();
        String::from_utf8(buf).X()
    }

    fn debug_write(
        iter: impl Iterator<Item = TreeToken<'db>>,
        w: &mut dyn Write,
        text: &str,
    ) -> AnyResult<()> {
        let mut iter = iter.peekable();
        while let Some(token) = iter.next() {
            match token {
                TreeToken::Token(token) => {
                    write!(w, "{}", token.debug_str(text))?;
                }
                TreeToken::Branch { sigil, inner, .. } => {
                    write!(w, "{} ", sigil.as_str())?;
                    rmx::extras::recurse(|| {
                        Self::debug_write(inner, w, text)
                    })?;
                    if !inner.is_empty() {
                        write!(w, " ")?;
                    }
                    write!(w, "{}", sigil.close_sigil().as_str())?;
                }
            }
            if iter.peek().is_some() {
                write!(w, " ")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
fn dbglex(s: &str) -> String {
    debug!("dbglex {s}");
    let ref db = crate::Database::default();
    let source = crate::input::Source::new(db, S(s));
    let bracer = bracer(db, crate::lexer::lex(db, source));
    bracer.debug_str(db)
}

#[test]
fn test_bracer() {
    assert_eq!(
        dbglex(" "),
        "",
    );
    assert_eq!(
        dbglex("a b"),
        "a b",
    );
    assert_eq!(
        dbglex("a\nb"),
        "a b",
    );
    assert_eq!(
        dbglex("()"),
        "( )",
    );
    assert_eq!(
        dbglex("{}"),
        "{ }",
    );
    assert_eq!(
        dbglex("())"),
        "( )",
    );
    assert_eq!(
        dbglex("(})"),
        "( )",
    );
    assert_eq!(
        dbglex("(()"),
        "( ( ) )",
    );
    assert_eq!(
        dbglex("({)"),
        "( { } )",
    );
    assert_eq!(
        dbglex(")"),
        "",
    );
    assert_eq!(
        dbglex("))})"),
        "",
    );
    assert_eq!(
        dbglex("(({("),
        "( ( { ( ) } ) )",
    );
    assert_eq!(
        dbglex("a(b)c"),
        "a ( b ) c",
    );
    assert_eq!(
        dbglex("a(b(c"),
        "a ( b ( c ) )",
    );
    assert_eq!(
        dbglex("a)b)c"),
        "a b c",
    );
    assert_eq!(
        dbglex("(a}b}c)"),
        "( a b c )",
    );
    assert_eq!(
        dbglex("[]"),
        "[ ]",
    );
    assert_eq!(
        dbglex("<>"),
        "< >",
    );
    assert_eq!(
        dbglex("a[b]c"),
        "a [ b ] c",
    );
    assert_eq!(
        dbglex("a<b>c"),
        "a < b > c",
    );
    assert_eq!(
        dbglex("([{<>}])"),
        "( [ { < > } ] )",
    );
    // Mismatch: paren inside brace closed by brace.
    assert_eq!(
        dbglex("{(}"),
        "{ ( ) }",
    );
    // Mismatch: bracket inside brace closed by brace.
    assert_eq!(
        dbglex("{[}"),
        "{ [ ] }",
    );
    // Mismatch: angle inside brace closed by brace.
    assert_eq!(
        dbglex("{<}"),
        "{ < > }",
    );
    // Mismatch: bracket inside paren closed by paren.
    assert_eq!(
        dbglex("([)"),
        "( [ ] )",
    );
    // Mismatch: angle inside paren closed by paren.
    assert_eq!(
        dbglex("(<)"),
        "( < > )",
    );
    // Mismatch: angle inside bracket closed by bracket.
    assert_eq!(
        dbglex("[<]"),
        "[ < > ]",
    );
}

#[test]
fn test_text_span() {
    let ref db = crate::Database::default();

    // Helper to get the span string from input.
    let get_span = |s: &str| -> Option<(usize, usize, String)> {
        let source = crate::input::Source::new(db, S(s));
        let bracer = bracer(db, crate::lexer::lex(db, source));
        let source_text = source;
        // Find the first branch.
        for token in bracer.iter(db) {
            if let TreeToken::Branch { .. } = &token {
                let ts = token.text_span(source_text)?;
                let spanned = &ts.source.as_str(db)[ts.span.C()];
                return Some((ts.start(), ts.end(), spanned.S()));
            }
        }
        None
    };

    // Simple branch - span covers entire (a).
    let (start, end, spanned) = get_span("(a)").X();
    assert_eq!(spanned, "(a)");
    assert_eq!(start, 0);
    assert_eq!(end, 3);

    // Empty branch - span covers ().
    let (start, end, spanned) = get_span("()").X();
    assert_eq!(spanned, "()");
    assert_eq!(start, 0);
    assert_eq!(end, 2);

    // Branch with leading content.
    let (start, end, spanned) = get_span("x(a)").X();
    assert_eq!(spanned, "(a)");
    assert_eq!(start, 1);
    assert_eq!(end, 4);

    // Unclosed branch.
    let (start, end, spanned) = get_span("(a").X();
    assert_eq!(spanned, "(a");
    assert_eq!(start, 0);
    assert_eq!(end, 2);

    // Nested branches - outer.
    let (start, end, spanned) = get_span("((a))").X();
    assert_eq!(spanned, "((a))");
    assert_eq!(start, 0);
    assert_eq!(end, 5);

    // Different bracket types.
    let (start, end, spanned) = get_span("[x]").X();
    assert_eq!(spanned, "[x]");
    assert_eq!(start, 0);
    assert_eq!(end, 3);

    let (start, end, spanned) = get_span("{y}").X();
    assert_eq!(spanned, "{y}");
    assert_eq!(start, 0);
    assert_eq!(end, 3);

    let (start, end, spanned) = get_span("<z>").X();
    assert_eq!(spanned, "<z>");
    assert_eq!(start, 0);
    assert_eq!(end, 3);
}

#[test]
fn test_removed_closes() {
    // Stray closes get removed.
    assert_eq!(dbglex("a)b"), "a b");
    assert_eq!(dbglex("a}b"), "a b");
    assert_eq!(dbglex("a]b"), "a b");
    assert_eq!(dbglex("a>b"), "a b");
    // Multiple stray closes.
    assert_eq!(dbglex("a)}]>b"), "a b");
    // Stray close inside matched braces.
    assert_eq!(dbglex("(a}b)"), "( a b )");
    assert_eq!(dbglex("(a}b}c)"), "( a b c )");
    // Stray closes before a later bracket at the same level.
    assert_eq!(dbglex("a)b(c)"), "a b ( c )");
    assert_eq!(dbglex("(a)b)c(d)"), "( a ) b c ( d )");
    // Stray closes after matched braces.
    assert_eq!(dbglex("(a))"), "( a )");
    assert_eq!(dbglex("(a)})"), "( a )");
    // Complex nesting with stray closes.
    assert_eq!(dbglex("((a)})"), "( ( a ) )");
}

#[test]
fn test_earmuff_braces() {
    // Basic earmuff brace matching.
    assert_eq!(dbglex("(|a|)"), "(| a |)");
    assert_eq!(dbglex("{|a|}"), "{| a |}");
    assert_eq!(dbglex("[|a|]"), "[| a |]");
    assert_eq!(dbglex("<|a|>"), "<| a |>");

    // Empty earmuff braces.
    assert_eq!(dbglex("(||)"), "(| |)");
    assert_eq!(dbglex("{||}"), "{| |}");
    assert_eq!(dbglex("[||]"), "[| |]");
    assert_eq!(dbglex("<||>"), "<| |>");

    // Nesting earmuff braces.
    assert_eq!(dbglex("(|[|a|]|)"), "(| [| a |] |)");
    assert_eq!(dbglex("{|<|a|>|}"), "{| <| a |> |}");

    // Mixing earmuff and regular braces.
    assert_eq!(dbglex("(|(a)|)"), "(| ( a ) |)");
    assert_eq!(dbglex("([|a|])"), "( [| a |] )");

    // Unclosed earmuff braces.
    assert_eq!(dbglex("(|a"), "(| a |)");
    assert_eq!(dbglex("{|a"), "{| a |}");
    assert_eq!(dbglex("[|a"), "[| a |]");
    assert_eq!(dbglex("<|a"), "<| a |>");

    // Mismatched earmuff braces.
    assert_eq!(dbglex("(|a)"), "(| a |)");
    assert_eq!(dbglex("{|a}"), "{| a |}");
    assert_eq!(dbglex("[|a]"), "[| a |]");
    assert_eq!(dbglex("<|a>"), "<| a |>");

    // Stray earmuff close braces.
    assert_eq!(dbglex("a|)b"), "a b");
    assert_eq!(dbglex("a|}b"), "a b");
    assert_eq!(dbglex("a|]b"), "a b");
    assert_eq!(dbglex("a|>b"), "a b");

    // Sigil-brace opens: %{ and #{.
    assert_eq!(dbglex("%{a}"), "%{ a }");
    assert_eq!(dbglex("#{a}"), "#{ a }");
    assert_eq!(dbglex("%{}"), "%{ }");
    assert_eq!(dbglex("#{}"), "#{ }");
    assert_eq!(dbglex("%{#{a}}"), "%{ #{ a } }");
    assert_eq!(dbglex("#{%{a}}"), "#{ %{ a } }");

    // Unclosed sigil-brace opens.
    assert_eq!(dbglex("%{a"), "%{ a }");
    assert_eq!(dbglex("#{a"), "#{ a }");

    // Nested with regular braces.
    assert_eq!(dbglex("%{{a}}"), "%{ { a } }");
    assert_eq!(dbglex("{%{a}}"), "{ %{ a } }");
}
