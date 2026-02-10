use rmx::prelude::*;

use rmx::core::ops::Range;
use rmx::core::iter::Peekable;
use rmx::std::io::Write;

use crate::chunk::Chunk;
use crate::lexer::{ChunkLex, Token, TokenKind, Sigil};

#[salsa::tracked]
pub struct Bracer<'db> {
    pub chunk: ChunkLex<'db>,
    #[returns(ref)]
    pub branches: Vec<Branch>,
    #[returns(ref)]
    pub inserted_closes: Vec<(usize, Sigil)>,
    #[returns(ref)]
    pub removed_closes: Vec<(usize, Sigil)>,
    #[returns(ref)]
    pub errors: Vec<(Range<usize>, Sigil)>,
}

#[derive(Clone, Debug, Hash, salsa::Update)]
pub struct Branch {
    real_token_range: Range<usize>,
    branches: usize,
    inserted_closes: usize,
    removed_closes: usize,
    errors: usize,
    open_sigil: Sigil,
    close_sigil: Sigil,
}

impl<'db> Bracer<'db> {
    pub fn iter(
        &self,
        db: &'db dyn crate::Db,
    ) -> BracerIter<'db> {
        BracerIter {
            db,
            tree: *self,
            real_token_range: 0..self.chunk(db).tokens(db).len(),
            branches: 0..self.branches(db).len(),
            inserted_closes: 0..self.inserted_closes(db).len(),
            removed_closes: 0..self.removed_closes(db).len(),
            next_token_index: 0,
            next_branch_index: 0,
            next_inserted_close_index: 0,
            next_removed_close_index: 0,
        }
    }
}

#[derive(Clone)]
pub struct BracerIter<'db> {
    pub db: &'db dyn crate::Db,
    tree: Bracer<'db>,
    real_token_range: Range<usize>,
    branches: Range<usize>,
    inserted_closes: Range<usize>,
    removed_closes: Range<usize>,
    next_token_index: usize,
    next_branch_index: usize,
    next_inserted_close_index: usize,
    next_removed_close_index: usize,
}

impl<'db> Iterator for BracerIter<'db> {
    type Item = TreeToken<'db>;

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let res = self.next2();
        debug!("next: {:?}", match res.as_ref() {
            None => "none",
            Some(TreeToken::Token(t)) => t.text(self.db).as_str(self.db),
            Some(TreeToken::Branch { .. }) => "branch",
        });
        res
    }
}

impl<'db> BracerIter<'db> {
    /// Get source Text and byte span for this branch, including delimiters.
    ///
    /// Returns None for top-level iterators (which have no enclosing braces).
    pub fn text_span(&self) -> Option<crate::text::TextSpan<'db>> {
        let chunk_lex = self.tree.chunk(self.db);
        let tokens = chunk_lex.tokens(self.db);
        // real_token_range starts AFTER the open brace, so go back 1 for open brace.
        let open_idx = self.real_token_range.start.checked_sub(1)?;
        let close_idx = self.real_token_range.end.checked_sub(1)?;
        let open_token = tokens.get(open_idx)?;
        let close_token = tokens.get(close_idx)?;
        let text = chunk_lex.chunk(self.db).text(self.db);
        let span = open_token.span(self.db).start
                 ..close_token.span(self.db).end;
        Some(crate::text::TextSpan::new(text, span))
    }

    fn next2(&mut self) -> Option<TreeToken<'db>> {
        loop {
            debug!("--");
            debug!("real token range {:?}", self.real_token_range.C());
            debug!("next branches {:?}", self.branches.C());
            debug!("inserted closes {:?}", self.inserted_closes.C());
            debug!("removed closes {:?}", self.removed_closes.C());
            debug!("next token index {:?}", self.next_token_index);
            debug!("next branch index {:?}", self.next_branch_index);
            debug!("next inserted close index {:?}", self.next_inserted_close_index);
            debug!("next removed close index {:?}", self.next_removed_close_index);
            debug!("--");

            let tokens = &self.tree.chunk(self.db).tokens(self.db)
                [self.real_token_range.C()];
            let branches = &self.tree.branches(self.db)
                [self.branches.C()];
            let inserted_closes = &self.tree.inserted_closes(self.db)
                [self.inserted_closes.C()];
            let removed_closes = &self.tree.removed_closes(self.db)
                [self.removed_closes.C()];
            let tokens = &self.tree.chunk(self.db).tokens(self.db)
                [0..self.real_token_range.C().end];
            let branches = &self.tree.branches(self.db)
                [0..self.branches.C().end];
            let inserted_closes = &self.tree.inserted_closes(self.db)
                [0..self.inserted_closes.C().end];
            let removed_closes = &self.tree.removed_closes(self.db)
                [0..self.removed_closes.C().end];

            let next_token = tokens.get(self.next_token_index);
            let next_branch = branches.get(self.next_branch_index);
            let next_inserted_close = inserted_closes.get(self.next_inserted_close_index);
            let next_removed_close = removed_closes.get(self.next_removed_close_index);

            return match (
                next_token,
                next_branch,
                next_inserted_close,
                next_removed_close,
            ) {
                (Some(next_token), None, _, None) => {
                    self.next_token_index = self.next_token_index.checked_add(1).X();
                    if !next_token.is_close_sigil(self.db) {
                        Some(TreeToken::Token(*next_token))
                    } else {
                        continue;
                    }
                },
                (Some(next_token), None, _, Some(next_removed_close)) => {
                    match self.next_token_index.cmp(&next_removed_close.0) {
                        Ordering::Less => {
                            self.next_token_index = self.next_token_index.checked_add(1).X();
                            if !next_token.is_close_sigil(self.db) {
                                Some(TreeToken::Token(*next_token))
                            } else {
                                panic!("is this possible?")
                            }
                        }
                        Ordering::Equal => {
                            self.next_token_index = self.next_token_index.checked_add(1).X();
                            self.next_removed_close_index = self.next_removed_close_index.checked_add(1).X();
                            continue;
                        }
                        Ordering::Greater => bug!(),
                    }
                },
                (Some(next_token), Some(next_branch), _, _) => {
                    match self.next_token_index.cmp(&next_branch.real_token_range.start) {
                        Ordering::Less => {
                            self.next_token_index = self.next_token_index.checked_add(1).X();
                            Some(TreeToken::Token(*next_token))
                        },
                        Ordering::Equal => {
                            self.next_token_index = self.next_token_index.checked_add(1).X();
                            self.next_branch_index = self.next_branch_index.checked_add(1).X();

                            // Skip the opening brace of the branch.
                            let branch_token_range_start = next_branch.real_token_range.start.checked_add(1).X();
                            let branch_token_range = branch_token_range_start..next_branch.real_token_range.end;

                            // Get the open token.
                            let open_token = *next_token;

                            // Get the potential close token and check if it's a real close.
                            let all_tokens = self.tree.chunk(self.db).tokens(self.db);
                            let close_idx = next_branch.real_token_range.end.checked_sub(1);
                            let expected_close_sigil = next_branch.open_sigil.close_sigil();
                            let (close_token, end_byte) = match close_idx.and_then(|i| all_tokens.get(i)) {
                                Some(tok) if tok.kind(self.db) == TokenKind::Sigil(expected_close_sigil) => {
                                    // Real close token found.
                                    (Some(*tok), tok.span(self.db).end)
                                }
                                Some(tok) => {
                                    // Last token exists but isn't the close sigil (unclosed/mismatched).
                                    (None, tok.span(self.db).end)
                                }
                                None => {
                                    // No tokens at all after open (empty unclosed branch).
                                    (None, open_token.span(self.db).end)
                                }
                            };

                            let branch = TreeToken::Branch {
                                sigil: next_branch.open_sigil,
                                open: open_token,
                                close: close_token,
                                end_byte,
                                inner: BracerIter {
                                    db: self.db,
                                    tree: self.tree,
                                    real_token_range: branch_token_range,
                                    branches: Range::from_start_len(self.next_branch_index, next_branch.branches).X(),
                                    inserted_closes: Range::from_start_len(self.next_inserted_close_index, next_branch.inserted_closes).X(),
                                    removed_closes: Range::from_start_len(self.next_removed_close_index, next_branch.removed_closes).X(),
                                    next_token_index: self.next_token_index,
                                    next_branch_index: self.next_branch_index,
                                    next_inserted_close_index: self.next_inserted_close_index,
                                    next_removed_close_index: self.next_removed_close_index,
                                },
                            };

                            debug!("sbi {:#?}", Range::from_start_len(self.next_branch_index, next_branch.branches).X());

                            self.next_token_index = next_branch.real_token_range.end;
                            self.next_branch_index = self.next_branch_index
                                .checked_add(next_branch.branches).X();
                            self.next_inserted_close_index = self.next_inserted_close_index
                                .checked_add(next_branch.inserted_closes).X();
                            self.next_removed_close_index = self.next_removed_close_index
                                .checked_add(next_branch.removed_closes).X();

                            // Skip any removed_closes that are now behind our position.
                            // This handles cases where removed_closes exist at positions
                            // we've jumped past after exiting the branch.
                            let removed_closes = &self.tree.removed_closes(self.db)
                                [0..self.removed_closes.C().end];
                            while let Some(rc) = removed_closes.get(self.next_removed_close_index) {
                                if rc.0 < self.next_token_index {
                                    self.next_removed_close_index = self.next_removed_close_index.checked_add(1).X();
                                } else {
                                    break;
                                }
                            }

                            Some(branch)
                        },
                        Ordering::Greater => bug!(),
                    }
                }

                (None, Some(next_branch), _, _) => bug!(),

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

#[derive(Clone)]
pub enum TreeToken<'db> {
    Token(Token<'db>),
    Branch {
        sigil: Sigil,
        open: Token<'db>,
        close: Option<Token<'db>>,
        end_byte: usize,
        inner: BracerIter<'db>,
    },
}

#[salsa::tracked]
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
                    let mut parent_brace_map = stack.last_mut()
                        .map(|(_, _, brace_map)| brace_map)
                        .unwrap_or(&mut top_map);
                    if open_sigil == open_s {
                        parent_brace_map.branches.push(Branch {
                            real_token_range: open_index..index.checked_add(1).X(),
                            branches: brace_map.branches.len(),
                            inserted_closes: brace_map.inserted_closes.len(),
                            removed_closes: brace_map.removed_closes.len(),
                            errors: brace_map.errors.len(),
                            open_sigil: open_s,
                            close_sigil: close_s,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::ParenOpen,
                            close_sigil: Sigil::ParenClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::BraceOpen,
                            close_sigil: Sigil::BraceClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::BracketOpen,
                            close_sigil: Sigil::BracketClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::AngleOpen,
                            close_sigil: Sigil::AngleClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::ParenPipeOpen,
                            close_sigil: Sigil::ParenPipeClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::BracePipeOpen,
                            close_sigil: Sigil::BracePipeClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::BracketPipeOpen,
                            close_sigil: Sigil::BracketPipeClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::AnglePipeOpen,
                            close_sigil: Sigil::AnglePipeClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::PercentBraceOpen,
                            close_sigil: Sigil::BraceClose,
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
                            errors: brace_map.errors.len(),
                            open_sigil: Sigil::HashBraceOpen,
                            close_sigil: Sigil::BraceClose,
                        });
                        parent_brace_map.append(brace_map);
                    } else {
                        bug!()
                    }
                }
            } else {
                let mut parent_brace_map = stack.last_mut()
                    .map(|(_, _, brace_map)| brace_map)
                    .unwrap_or(&mut top_map);
                parent_brace_map.removed_closes.push((index, close_s));
                parent_brace_map.errors.push((index..index.checked_add(1).X(), close_s));
            }
        };

    for (index, token) in tokens {
        match token.kind(db) {
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
        let mut parent_brace_map = stack.last_mut()
            .map(|(_, _, brace_map)| brace_map)
            .unwrap_or(&mut top_map);
        parent_brace_map.branches.push(Branch {
            real_token_range: open_index..num_tokens,
            branches: brace_map.branches.len(),
            inserted_closes: brace_map.inserted_closes.len(),
            removed_closes: brace_map.removed_closes.len(),
            errors: brace_map.errors.len(),
            open_sigil,
            close_sigil: open_sigil.close_sigil(),
        });
        parent_brace_map.errors.push((
            open_index..num_tokens,
            open_sigil,
        ));
        parent_brace_map.append(brace_map);
    }

    debug!("bm {top_map:#?}");

    Bracer::new(
        db,
        chunk,
        top_map.branches,
        top_map.inserted_closes,
        top_map.removed_closes,
        top_map.errors,
    )
}

impl<'db> TreeToken<'db> {
    /// Get source Text and byte span for this token or branch.
    pub fn text_span(&self, db: &'db dyn crate::Db, source_text: crate::text::Text<'db>) -> Option<crate::text::TextSpan<'db>> {
        match self {
            TreeToken::Token(tok) => {
                Some(crate::text::TextSpan::new(source_text, tok.span(db)))
            }
            TreeToken::Branch { open, end_byte, .. } => {
                Some(crate::text::TextSpan::new(source_text, open.span(db).start..*end_byte))
            }
        }
    }

    /// Get the byte span of the opening delimiter, if this is a branch.
    pub fn open_span(&self, db: &'db dyn crate::Db) -> Option<Range<usize>> {
        match self {
            TreeToken::Token(_) => None,
            TreeToken::Branch { open, .. } => Some(open.span(db)),
        }
    }

    /// Get the byte span of the closing delimiter, if this is a branch with a real close.
    pub fn close_span(&self, db: &'db dyn crate::Db) -> Option<Range<usize>> {
        match self {
            TreeToken::Token(_) => None,
            TreeToken::Branch { close, .. } => close.map(|c| c.span(db)),
        }
    }

    pub fn without_space(self, db: &'db dyn crate::Db) -> Option<Self> {
        match self {
            TreeToken::Token(token) => {
                token.without_space(db)
                    .map(TreeToken::Token)
            }
            token @ TreeToken::Branch { .. } => Some(token),
        }
    }
}

#[cfg(test)]
#[extension_trait]
impl<'db> VecTreeTokenExt<'db> for Vec<TreeToken<'db>> {
    fn debug_str(&self, db: &'db dyn crate::Db) -> String {
        let mut buf = Vec::<u8>::new();
        Bracer::debug_write(self.iter().cloned(), &mut buf, db).X();
        String::from_utf8(buf).X()
    }
}

#[cfg(test)]
#[extension_trait]
pub impl<'db, I> IteratorOfTreeTokenExt<'db> for I
where I: Iterator<Item = TreeToken<'db>>
{
    fn debug_str(self, db: &'db dyn crate::Db) -> String {
        let mut buf = Vec::<u8>::new();
        Bracer::debug_write(self, &mut buf, db).X();
        String::from_utf8(buf).X()
    }
}

#[cfg(test)]
impl<'db> Bracer<'db> {
    fn debug_str(&self, db: &'db dyn crate::Db) -> String {
        let mut buf = Vec::<u8>::new();
        Self::debug_write(self.iter(db), &mut buf, db).X();
        String::from_utf8(buf).X()
    }

    fn debug_write(
        iter: impl Iterator<Item = TreeToken<'db>>,
        w: &mut dyn Write,
        db: &'db dyn crate::Db,
    ) -> AnyResult<()> {
        let mut iter = iter.peekable();
        while let Some(token) = iter.next() {
            match token {
                TreeToken::Token(token) => {
                    write!(w, "{}", token.debug_str(db))?;
                }
                TreeToken::Branch { sigil, mut inner, .. } => {
                    write!(w, "{} ", sigil.as_str())?;
                    rmx::extras::recurse(|| {
                        Self::debug_write(inner.C(), w, db)
                    })?;
                    if inner.next().is_some() {
                        write!(w, " ");
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
        "ws",
    );
    assert_eq!(
        dbglex("a b"),
        "a ws b",
    );
    assert_eq!(
        dbglex("a\nb"),
        "a ws b",
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
                let ts = token.text_span(db, source_text)?;
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
fn test_without_space() {
    let ref db = crate::Database::default();
    let source = crate::input::Source::new(db, S("a b (c)"));
    let chunk = crate::source_map::basic_source_map(db, source);
    let chunk_lex = crate::lexer::lex_chunk(db, chunk);
    let bracer = bracer(db, chunk_lex);

    let tokens: Vec<_> = bracer.iter(db).collect();
    // tokens: "a", ws, "b", ws, branch(c)
    assert_eq!(tokens.len(), 5);

    // Token "a" - not whitespace, returns Some.
    let t0 = tokens[0].C().without_space(db);
    assert!(t0.is_some());

    // Whitespace token - returns None.
    let t1 = tokens[1].C().without_space(db);
    assert!(t1.is_none());

    // Branch - always returns Some.
    let t4 = tokens[4].C().without_space(db);
    assert!(t4.is_some());
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
