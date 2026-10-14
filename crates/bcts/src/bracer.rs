use rmx::prelude::*;

use rmx::core::ops::Range;

use crate::lexer::{ChunkLex, Token, TokenKind, Sigil};

#[cfg(test)]
use rmx::std::io::Write;

/// A chunk's brackets matched, with recovery: where each token stands among
/// them, and what was wrong.
#[salsa::tracked]
pub struct Bracer<'db> {
    #[returns(copy)]
    pub chunk: ChunkLex<'db>,
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
    /// The chunk's tokens at the top level.
    pub fn iter(&self, db: &'db dyn crate::Db) -> Cursor<'db> {
        let tokens = self.chunk(db).tokens(db);
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

/// The bracer's branches, as the recovery found them: a level's contents, and
/// how many of each list belong to the levels inside it.
#[derive(Clone, Debug)]
struct Branch {
    real_token_range: Range<usize>,
    branches: usize,
    inserted_closes: usize,
    removed_closes: usize,
}

/// What [`Walk`] meets at one level.
enum Step {
    /// A token the level hands out, by index.
    Token(usize),
    /// A bracket opening at an index, its contents, and where the level
    /// resumes after it.
    Branch(usize, Walk, usize),
}

/// One level of the branches, walked in order to fill the layout.
///
/// This is how the bracer's output was read before there was a layout, kept
/// as it was so that the levels hand out exactly what they did, down to the
/// odd cases of recovery: a removed close before a later bracket at the same
/// level comes out as a token, for one.
struct Walk {
    real_token_range: Range<usize>,
    branches: Range<usize>,
    inserted_closes: Range<usize>,
    removed_closes: Range<usize>,
    next_token_index: usize,
    next_branch_index: usize,
    next_inserted_close_index: usize,
    next_removed_close_index: usize,
}

struct WalkLists<'a> {
    tokens: &'a [Token],
    branches: &'a [Branch],
    inserted_closes: &'a [(usize, Sigil)],
    removed_closes: &'a [(usize, Sigil)],
}

impl Walk {
    fn next(&mut self, lists: &WalkLists<'_>) -> Option<Step> {
        loop {
            let tokens = &lists.tokens[0..self.real_token_range.end];
            let branches = &lists.branches[0..self.branches.end];
            let inserted_closes = &lists.inserted_closes[0..self.inserted_closes.end];
            let removed_closes = &lists.removed_closes[0..self.removed_closes.end];

            let index = self.next_token_index;
            let next_token = tokens.get(index);
            let next_branch = branches.get(self.next_branch_index);
            let next_inserted_close = inserted_closes.get(self.next_inserted_close_index);
            let next_removed_close = removed_closes.get(self.next_removed_close_index);

            return match (next_token, next_branch, next_inserted_close, next_removed_close) {
                (Some(next_token), None, _, None) => {
                    self.next_token_index = index.checked_add(1).X();
                    if !next_token.is_close_sigil() {
                        Some(Step::Token(index))
                    } else {
                        continue;
                    }
                }
                (Some(next_token), None, _, Some(next_removed_close)) => {
                    match index.cmp(&next_removed_close.0) {
                        Ordering::Less => {
                            self.next_token_index = index.checked_add(1).X();
                            if !next_token.is_close_sigil() {
                                Some(Step::Token(index))
                            } else {
                                panic!("is this possible?")
                            }
                        }
                        Ordering::Equal => {
                            self.next_token_index = index.checked_add(1).X();
                            self.next_removed_close_index = self.next_removed_close_index.checked_add(1).X();
                            continue;
                        }
                        Ordering::Greater => bug!(),
                    }
                }
                (Some(_), Some(next_branch), _, _) => {
                    match index.cmp(&next_branch.real_token_range.start) {
                        Ordering::Less => {
                            self.next_token_index = index.checked_add(1).X();
                            Some(Step::Token(index))
                        }
                        Ordering::Equal => {
                            self.next_token_index = index.checked_add(1).X();
                            self.next_branch_index = self.next_branch_index.checked_add(1).X();

                            // Skip the opening brace of the branch.
                            let branch_token_range_start = next_branch.real_token_range.start.checked_add(1).X();
                            let inner = Walk {
                                real_token_range: branch_token_range_start..next_branch.real_token_range.end,
                                branches: Range::from_start_len(self.next_branch_index, next_branch.branches).X(),
                                inserted_closes: Range::from_start_len(self.next_inserted_close_index, next_branch.inserted_closes).X(),
                                removed_closes: Range::from_start_len(self.next_removed_close_index, next_branch.removed_closes).X(),
                                next_token_index: self.next_token_index,
                                next_branch_index: self.next_branch_index,
                                next_inserted_close_index: self.next_inserted_close_index,
                                next_removed_close_index: self.next_removed_close_index,
                            };
                            let resume = next_branch.real_token_range.end;

                            self.next_token_index = resume;
                            self.next_branch_index = self.next_branch_index
                                .checked_add(next_branch.branches).X();
                            self.next_inserted_close_index = self.next_inserted_close_index
                                .checked_add(next_branch.inserted_closes).X();
                            self.next_removed_close_index = self.next_removed_close_index
                                .checked_add(next_branch.removed_closes).X();

                            // Skip any removed_closes that are now behind our position.
                            // This handles cases where removed_closes exist at positions
                            // we've jumped past after exiting the branch.
                            let removed_closes = &lists.removed_closes[0..self.removed_closes.end];
                            while let Some(rc) = removed_closes.get(self.next_removed_close_index) {
                                if rc.0 < self.next_token_index {
                                    self.next_removed_close_index = self.next_removed_close_index.checked_add(1).X();
                                } else {
                                    break;
                                }
                            }

                            Some(Step::Branch(index, inner, resume))
                        }
                        Ordering::Greater => bug!(),
                    }
                }

                (None, Some(_), _, _) => bug!(),

                (None, None, Some(next_inserted_close), _) => {
                    assert_eq!(next_inserted_close.0, self.next_token_index);
                    self.next_inserted_close_index = self.next_inserted_close_index.checked_add(1).X();
                    continue;
                }
                (None, None, _, Some(_next_removed_close)) => bug!(),
                (None, None, None, None) => None,
            }
        }
    }
}

/// Every level walked, each token's entry written: see [`Cursor`].
fn layout(lists: &WalkLists<'_>) -> Vec<u32> {
    let mut layout = vec![HIDDEN; lists.tokens.len()];
    let top = Walk {
        real_token_range: 0..lists.tokens.len(),
        branches: 0..lists.branches.len(),
        inserted_closes: 0..lists.inserted_closes.len(),
        removed_closes: 0..lists.removed_closes.len(),
        next_token_index: 0,
        next_branch_index: 0,
        next_inserted_close_index: 0,
        next_removed_close_index: 0,
    };
    // A stack rather than recursion, since how deep brackets nest is up to
    // whoever wrote the source.
    let mut levels = vec![top];
    while let Some(level) = levels.last_mut() {
        match level.next(lists) {
            Some(Step::Token(index)) => layout[index] = PLAIN,
            Some(Step::Branch(index, inner, resume)) => {
                layout[index] = u32::try_from(resume).X();
                levels.push(inner);
            }
            None => {
                levels.pop();
            }
        }
    }
    layout
}

#[salsa::tracked(returns(copy))]
pub fn bracer<'db>(
    db: &'db dyn crate::Db,
    chunk: ChunkLex<'db>
) -> Bracer<'db> {
    let tokens = chunk.tokens(db).iter().enumerate();

    #[derive(Default, Debug)]
    pub struct BraceMap {
        branches: Vec<Branch>,
        inserted_closes: Vec<(usize, Sigil)>,
        removed_closes: Vec<(usize, Sigil)>,
        errors: Vec<(Range<usize>, Sigil)>,
    }

    impl BraceMap {
        fn append(&mut self, other: BraceMap) {
            self.branches.extend(other.branches);
            self.inserted_closes.extend(other.inserted_closes);
            self.removed_closes.extend(other.removed_closes);
            self.errors.extend(other.errors);
        }
    }

    let mut top_map = BraceMap::default();
    let mut stack: Vec<(usize, Sigil, BraceMap)> = vec![];

    let mut close_brace =
        |
    stack: &mut Vec<(usize, Sigil, BraceMap)>,
    index: usize,
    open_s: Sigil,
    close_s: Sigil
        | {
            let seen_open = stack.iter().any(|(_, sigil, _)| *sigil == open_s);
            if seen_open {
                loop {
                    let (open_index, open_sigil, mut brace_map) = stack.pop().X();
                    let parent_brace_map = stack.last_mut()
                        .map(|(_, _, brace_map)| brace_map)
                        .unwrap_or(&mut top_map);
                    if open_sigil == open_s {
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index.checked_add(1).X(),
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                        break;
                    } else if open_sigil == Sigil::ParenOpen {
                        brace_map.inserted_closes.push((index, Sigil::ParenClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::ParenOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::BraceOpen {
                        brace_map.inserted_closes.push((index, Sigil::BraceClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::BraceOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::BracketOpen {
                        brace_map.inserted_closes.push((index, Sigil::BracketClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::BracketOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::AngleOpen {
                        brace_map.inserted_closes.push((index, Sigil::AngleClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::AngleOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::ParenPipeOpen {
                        brace_map.inserted_closes.push((index, Sigil::ParenPipeClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::ParenPipeOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::BracePipeOpen {
                        brace_map.inserted_closes.push((index, Sigil::BracePipeClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::BracePipeOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::BracketPipeOpen {
                        brace_map.inserted_closes.push((index, Sigil::BracketPipeClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::BracketPipeOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::AnglePipeOpen {
                        brace_map.inserted_closes.push((index, Sigil::AnglePipeClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::AnglePipeOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::PercentBraceOpen {
                        brace_map.inserted_closes.push((index, Sigil::BraceClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::PercentBraceOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else if open_sigil == Sigil::HashBraceOpen {
                        brace_map.inserted_closes.push((index, Sigil::BraceClose));
                        brace_map.errors.push((
                            open_index..index,
                            Sigil::HashBraceOpen,
                        ));
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index,
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                        });
                        parent_brace_map.append(brace_map);
                    } else {
                        bug!()
                    }
                }
            } else {
                let parent_brace_map = stack.last_mut()
                    .map(|(_, _, brace_map)| brace_map)
                    .unwrap_or(&mut top_map);
                parent_brace_map.removed_closes.push((index, close_s));
                parent_brace_map.errors.push((index..index.checked_add(1).X(), close_s));
            }
        };

    for (index, token) in tokens {
        match token.kind {
            TokenKind::Sigil(Sigil::ParenOpen) => {
                stack.push((index, Sigil::ParenOpen, default()));
            }
            TokenKind::Sigil(Sigil::BraceOpen) => {
                stack.push((index, Sigil::BraceOpen, default()));
            }
            TokenKind::Sigil(Sigil::BracketOpen) => {
                stack.push((index, Sigil::BracketOpen, default()));
            }
            TokenKind::Sigil(Sigil::AngleOpen) => {
                stack.push((index, Sigil::AngleOpen, default()));
            }
            TokenKind::Sigil(Sigil::ParenPipeOpen) => {
                stack.push((index, Sigil::ParenPipeOpen, default()));
            }
            TokenKind::Sigil(Sigil::BracePipeOpen) => {
                stack.push((index, Sigil::BracePipeOpen, default()));
            }
            TokenKind::Sigil(Sigil::BracketPipeOpen) => {
                stack.push((index, Sigil::BracketPipeOpen, default()));
            }
            TokenKind::Sigil(Sigil::AnglePipeOpen) => {
                stack.push((index, Sigil::AnglePipeOpen, default()));
            }
            TokenKind::Sigil(Sigil::PercentBraceOpen) => {
                stack.push((index, Sigil::PercentBraceOpen, default()));
            }
            TokenKind::Sigil(Sigil::HashBraceOpen) => {
                stack.push((index, Sigil::HashBraceOpen, default()));
            }
            TokenKind::Sigil(Sigil::ParenClose) => {
                close_brace(&mut stack, index, Sigil::ParenOpen, Sigil::ParenClose);
            }
            TokenKind::Sigil(Sigil::BraceClose) => {
                let open_s = stack.iter().rev()
                    .find_map(|(_, s, _)| match s {
                        Sigil::BraceOpen | Sigil::PercentBraceOpen | Sigil::HashBraceOpen => Some(*s),
                        _ => None,
                    })
                    .unwrap_or(Sigil::BraceOpen);
                close_brace(&mut stack, index, open_s, Sigil::BraceClose);
            }
            TokenKind::Sigil(Sigil::BracketClose) => {
                close_brace(&mut stack, index, Sigil::BracketOpen, Sigil::BracketClose);
            }
            TokenKind::Sigil(Sigil::AngleClose) => {
                close_brace(&mut stack, index, Sigil::AngleOpen, Sigil::AngleClose);
            }
            TokenKind::Sigil(Sigil::ParenPipeClose) => {
                close_brace(&mut stack, index, Sigil::ParenPipeOpen, Sigil::ParenPipeClose);
            }
            TokenKind::Sigil(Sigil::BracePipeClose) => {
                close_brace(&mut stack, index, Sigil::BracePipeOpen, Sigil::BracePipeClose);
            }
            TokenKind::Sigil(Sigil::BracketPipeClose) => {
                close_brace(&mut stack, index, Sigil::BracketPipeOpen, Sigil::BracketPipeClose);
            }
            TokenKind::Sigil(Sigil::AnglePipeClose) => {
                close_brace(&mut stack, index, Sigil::AnglePipeOpen, Sigil::AnglePipeClose);
            }
            _ => {},
        }
    }

    let num_tokens = chunk.tokens(db).len();

    while let Some((open_index, open_sigil, brace_map)) = stack.pop() {
        let parent_brace_map = stack.last_mut()
            .map(|(_, _, brace_map)| brace_map)
            .unwrap_or(&mut top_map);
        parent_brace_map.branches.push(Branch {
            real_token_range: open_index..num_tokens,
            branches: brace_map.branches.len(),
            inserted_closes: brace_map.inserted_closes.len(),
            removed_closes: brace_map.removed_closes.len(),
        });
        parent_brace_map.errors.push((
            open_index..num_tokens,
            open_sigil,
        ));
        parent_brace_map.append(brace_map);
    }

    let layout = layout(&WalkLists {
        tokens: chunk.tokens(db),
        branches: &top_map.branches,
        inserted_closes: &top_map.inserted_closes,
        removed_closes: &top_map.removed_closes,
    });
    Bracer::new(db, chunk, layout, top_map.errors)
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

    /// Get source Text and byte span for this token or branch.
    pub fn text_span(&self, source_text: crate::text::Text<'db>) -> Option<crate::text::TextSpan<'db>> {
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
        let text = self.chunk(db).chunk(db).text(db).as_str(db);
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
    let chunk = crate::source_map::basic_source_map(db, source);
    let chunk_lex = crate::lexer::lex_chunk(db, chunk);
    let bracer = bracer(db, chunk_lex);
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
        let chunk = crate::source_map::basic_source_map(db, source);
        let chunk_lex = crate::lexer::lex_chunk(db, chunk);
        let bracer = bracer(db, chunk_lex);
        let source_text = chunk_lex.chunk(db).text(db);
        // Find the first branch.
        for token in bracer.iter(db) {
            if let TreeToken::Branch { .. } = &token {
                let ts = token.text_span(source_text)?;
                let spanned = &ts.text.as_str(db)[ts.span.C()];
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
