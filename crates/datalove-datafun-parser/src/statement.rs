//! Statement parsing.

use datalove_diagnostic_codes::PCode;
use rmx::prelude::*;
use rmx::core::iter::Peekable;

use bct::{
    lexer::Sigil,
    bracer::{Cursor, TreeToken},
    text::InternedText,
};
use bct::diagnostic::DiagnosticBuilder;
use datalove_datalit::parser_util::EmitTo;

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use super::state::Parser;
use datalove_datalit::parser_util::NameKind;

/// The keywords of the blocks a line opens, which an `end` line names.
const BLOCK_KEYWORDS: &[&str] = &["fun", "if", "loop", "for", "match"];

/// What a line is to the reading of blocks.
enum LineKind<'db> {
    /// An `end` line, and the word after its `end`.
    End(Option<&'db str>),
    /// A line opening a block that is not a declaration.
    Opener(&'static str),
    /// A declaration, which is only read at the top level.
    Declaration { opens_fun: bool },
    /// Anything else, and its first word.
    Other(Option<&'db str>),
}

/// What the next line does to the block being read.
enum BlockLine {
    /// Its `end` line.
    Close,
    /// An `end` line for it written wrong.
    WrongEnd,
    /// Not part of it: the input ran out, an enclosing block's `end` came, or
    /// a declaration outside it did. The block is missing its `end`.
    Unterminated,
    /// A line of its body.
    Take,
}

/// How a block's body ended.
enum BodyEnd {
    /// At its `end` line, left for the caller.
    Close,
    /// At its `end` line written wrong, consumed and reported.
    Closed,
    /// At one of the stops the caller gave.
    Stop,
    /// Without its `end`.
    Unterminated,
    /// Without its `end`, which a bracket, string or comment never closed
    /// took to the end of the source: what the block was meant to hold is not
    /// there to read, and the unclosed thing's error is the one reported.
    Swallowed,
}

/// Whether a line leaves a bracket, a string or a block comment open,
/// running to the end of the source, `text`.
pub(super) fn leaves_open(line: &Cursor<'_>, text: &str) -> bool {
    let mut tokens = *line;
    rmx::std::iter::from_fn(|| tokens.next()).any(|t| match t {
        TreeToken::Branch { close: None, .. } => true,
        TreeToken::Token(t) => t.kind == bct::lexer::TokenKind::Error && t.span().end == text.len(),
        TreeToken::Branch { .. } => false,
    })
}

/// Close the innermost block an `end` line names, and those inside it; one
/// naming no open block closes the innermost.
fn close(open: &mut Vec<&'static str>, word: Option<&str>) {
    match open.iter().rposition(|k| Some(*k) == word) {
        Some(at) => open.truncate(at),
        None => {
            open.pop();
        }
    }
}

/// The words a statement begins with, every one `parse_statement` reads.
pub(super) const STATEMENT_WORDS: &[&str] = &[
    "let", "var", "const", "set", "fun", "ret", "require", "import", "if", "loop", "for", "break",
    "continue", "debuglog", "type", "native", "intrinsic", "match", "call", "inline",
];

impl<'db> Parser<'db> {
    pub(super) fn parse_statement(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        let stmt = match self.peek_word() {
            Some("let") => self.parse_let(),
            Some("var") => self.parse_var(),
            Some("const") => self.parse_const(),
            Some("set") => self.parse_set(),
            Some("fun") => self.parse_fun(remaining_lines),
            Some("ret") => self.parse_ret(),
            Some("require") => self.parse_require(),
            Some("import") => self.parse_import(),
            Some("if") => self.parse_if(remaining_lines),
            Some("loop") => self.parse_loop(remaining_lines),
            Some("for") => self.parse_for(remaining_lines),
            Some("break") => self.parse_break(),
            Some("continue") => self.parse_continue(),
            Some("debuglog") => self.parse_debuglog(),
            Some("type") => self.parse_type_alias(),
            Some("native" | "intrinsic") => self.parse_native_fun(),
            Some("match") => self.parse_match(remaining_lines),
            Some("call") => self.parse_call(),
            Some("inline") => {
                let ts = self.peek_text_span();
                self.eat_word("inline");
                self.emit_stmt_error(ts,
                    "`inline` is only allowed as the first statement of a function",
                    PCode::MisplacedInline,
                    "not the first statement of a function",
                )
            }
            word => {
                let ts = self.peek_text_span();
                let error = self.emit_stmt_error(ts,
                    "unexpected statement",
                    PCode::UnknownStatement,
                    "expected 'let', 'var', 'const', 'set', 'call', 'fun', 'ret', 'require', 'import', 'if', 'match', 'loop', 'for', 'break', 'continue', 'debuglog', 'type', or 'native'"
                );
                // A block header written wrong still has its body and `end`
                // after it, which are taken with it rather than read alone.
                if !matches!(word, Some("end" | "else" | "case"))
                    && let Some(keyword) = self.orphan_block_ahead(remaining_lines.clone())
                {
                    self.skip_block(remaining_lines, keyword);
                }
                error
            }
        };

        // Check for unconsumed tokens on this line.
        self.error_if_not_exhausted();

        stmt
    }

    /// Parse a statement from a line of tokens.
    ///
    /// Creates a sub-parser for the line and parses the statement.
    ///
    /// Every line parsed this way is inside a block -- the top level is parsed
    /// line by line in `parse_statements` -- so this is where a declaration
    /// written inside one is refused (P074). A `require`, `import`, `fun`,
    /// `native fun` or `type` is only collected from the top level. One
    /// anywhere else was silently nothing, leaving the calls a `require` was
    /// meant to serve to report that nothing was required, a `type` to be an
    /// unknown name, and a `fun` to bring lowering down.
    pub(super) fn parse_line_statement(
        &mut self,
        line: Cursor<'db>,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        if let Some(token @ TreeToken::Token(first)) = &line.first() {
            if let Some(word @ ("require" | "import" | "fun" | "native" | "intrinsic" | "type")) = first.word_str(self.text()) {
                self.had_error = true;
                let ts = self.extract_text_span(token);
                DiagnosticBuilder::error(self.db, &fmt!("`{word}` inside a block"))
                    .code(PCode::DeclarationInBlock.as_str())
                    .primary_label(ts, &fmt!("`{word}` is only read at the top level"))
                    .help(&fmt!("move this `{word}` to the top level of the file"))
                    .emit_to(&self.sink);
            }
        }
        let mut sub = self.new_sub(line);
        let stmt = sub.parse_statement(remaining_lines);
        self.merge_identity_from(&mut sub);
        stmt
    }

    /// Refuse a statement a module's top level has no use for (P077).
    ///
    /// A module defines things for others to use; only a script runs
    /// statements from top to bottom. Anything but a declaration at the top
    /// of a module was never run, so a `debuglog` there printed nothing and
    /// a `let` bound nothing anyone could name. The statement is still parsed,
    /// so what is wrong inside it is reported too.
    pub(super) fn refuse_at_module_top_level(&mut self) {
        let Some(word) = self.peek_word() else {
            return;
        };
        // A line that begins no statement is that line's error (P001) alone.
        if !STATEMENT_WORDS.contains(&word)
            || matches!(word, "require" | "import" | "fun" | "native" | "intrinsic" | "type" | "const")
        {
            return;
        }
        self.had_error = true;
        let ts = self.peek_text_span();
        DiagnosticBuilder::error(self.db, &fmt!("`{word}` at the top level of a module"))
            .code(PCode::StatementInModule.as_str())
            .primary_label(ts, "a module's top level is not run")
            .help("a module holds `fun`, `const`, `type`, `require` and `import`; \
statements that run belong in a function or a script")
            .emit_to(&self.sink);
    }

    /// Refuse anything in a rider interface but a `native fun` or a `type`
    /// (P084).
    ///
    /// An interface says what a rider's crate implements and nothing runs in
    /// it, so a function with a body, a binding or a `require` there has no
    /// meaning. The statement is still parsed, so what is wrong inside it is
    /// reported too.
    pub(super) fn refuse_at_interface_top_level(&mut self) {
        let Some(word) = self.peek_word() else {
            return;
        };
        // A line that begins no statement is that line's error (P001) alone.
        if !STATEMENT_WORDS.contains(&word) || matches!(word, "native" | "intrinsic" | "type") {
            return;
        }
        self.had_error = true;
        let ts = self.peek_text_span();
        DiagnosticBuilder::error(self.db, &fmt!("`{word}` in a rider interface"))
            .code(PCode::NotInRiderInterface.as_str())
            .primary_label(ts, "an interface declares natives and types")
            .help("a rider interface holds `native fun` declarations and `type` aliases only")
            .emit_to(&self.sink);
    }

    /// Consume an `end <keyword>` line, complaining about anything after it.
    ///
    /// A block's terminator is a whole line, so a word left over on it was
    /// meant to do something and does not. Dropping it is how `end fun please`
    /// came to mean `end fun`.
    fn eat_end_line(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        keyword: &str,
    ) {
        let (_, line) = remaining_lines.next().X();
        let Some(extra) = line.peek_nth(2) else {
            return;
        };
        self.had_error = true;
        DiagnosticBuilder::error(self.db, &fmt!("unexpected token after `end {keyword}`"))
            .code(PCode::EndLineExtraTokens.as_str())
            .primary_label(self.extract_text_span(&extra), "`end` takes the keyword and nothing else")
            .emit_to(&self.sink);
    }

    /// What a line is to the reading of blocks, by its first words.
    fn line_kind(&self, line: &Cursor<'db>) -> LineKind<'db> {
        let mut words = *line;
        let first = match words.next() {
            Some(TreeToken::Token(t)) => t.word_str(self.text()),
            _ => None,
        };
        let second = match words.next() {
            Some(TreeToken::Token(t)) => t.word_str(self.text()),
            _ => None,
        };
        match first {
            Some("end") => LineKind::End(second),
            Some(word @ ("if" | "loop" | "for" | "match")) => LineKind::Opener(BLOCK_KEYWORDS.iter().find(|k| **k == word).X()),
            Some("fun") => LineKind::Declaration { opens_fun: true },
            Some("require" | "import" | "native" | "intrinsic" | "type") => LineKind::Declaration { opens_fun: false },
            Some(word) => LineKind::Other(Some(word)),
            None => LineKind::Other(None),
        }
    }

    /// Whether the innermost block open around this parser is closed by an
    /// `end` line in `lines`.
    ///
    /// A declaration inside a block (P074) is told this way from the block
    /// missing its `end` above it (P034): datafun has no nested functions, so
    /// a `fun` line inside a block is one or the other, and which is decided by
    /// whether the block's own `end` comes at all, each `fun` on the way taking
    /// its own. What the line about to be read opens, if it opens a `fun`, is
    /// counted too.
    fn block_closes_ahead(&self, lines: impl Iterator<Item = (usize, Cursor<'db>)>, opens_fun: bool) -> bool {
        let base = self.open_blocks.len();
        let mut open = self.open_blocks.C();
        if opens_fun {
            open.push("fun");
        }
        for (_, line) in lines {
            match self.line_kind(&line) {
                LineKind::Declaration { opens_fun } => {
                    if opens_fun {
                        open.push("fun");
                    }
                }
                LineKind::Opener(keyword) => open.push(keyword),
                LineKind::End(word) => {
                    close(&mut open, word);
                    if open.len() < base {
                        return true;
                    }
                }
                LineKind::Other(_) => {}
            }
        }
        false
    }

    /// The keyword of the block a line that begins no statement was meant to
    /// open, if there is one.
    ///
    /// A block header written wrong, `fnu f()` for `fun f()`, still has its
    /// body and its `end` after it, which read as statements of their own
    /// would each be an error. So the line is taken for the header of the
    /// block that makes the `end` lines in `lines` match: up to the next
    /// declaration outside every block, they do not as they are, and with a
    /// block of that kind opened here they do, each closing the innermost
    /// block open.
    fn orphan_block_ahead(&self, lines: impl Iterator<Item = (usize, Cursor<'db>)> + Clone) -> Option<&'static str> {
        let balanced = |extra: Option<&'static str>| {
            let mut open = self.open_blocks.C();
            open.extend(extra);
            for (_, line) in lines.clone() {
                match self.line_kind(&line) {
                    LineKind::Declaration { opens_fun } => {
                        if open.is_empty() {
                            break;
                        }
                        if opens_fun {
                            open.push("fun");
                        }
                    }
                    LineKind::Opener(keyword) => open.push(keyword),
                    LineKind::End(word) => {
                        if open.last().is_none_or(|top| Some(*top) != word) {
                            return false;
                        }
                        open.pop();
                    }
                    LineKind::Other(_) => {}
                }
            }
            open.is_empty()
        };
        if balanced(None) {
            return None;
        }
        BLOCK_KEYWORDS.iter().copied().find(|keyword| balanced(Some(keyword)))
    }

    /// What the next line does to the block being read, whose keyword is
    /// `keyword` and which is the innermost of `open_blocks`.
    fn block_line(
        &self,
        remaining_lines: &Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        keyword: &str,
    ) -> BlockLine {
        let mut rest = remaining_lines.clone();
        let Some((_, line)) = rest.next() else {
            return BlockLine::Unterminated;
        };
        match self.line_kind(&line) {
            LineKind::End(Some(word)) if word == keyword => BlockLine::Close,
            LineKind::End(word) => {
                let enclosing = &self.open_blocks[..self.open_blocks.len().saturating_sub(1)];
                if word.is_some_and(|w| enclosing.contains(&w)) {
                    BlockLine::Unterminated
                } else {
                    BlockLine::WrongEnd
                }
            }
            LineKind::Declaration { opens_fun } => {
                if self.block_closes_ahead(rest, opens_fun) { BlockLine::Take } else { BlockLine::Unterminated }
            }
            // An `else` or `case` line belongs to the innermost `if` or
            // `match` around this block, which has lost its `end` if there is
            // one: this block reads none.
            LineKind::Other(Some(word @ ("else" | "case"))) => {
                let owner = if word == "else" { "if" } else { "match" };
                let enclosing = &self.open_blocks[..self.open_blocks.len().saturating_sub(1)];
                if enclosing.contains(&owner) { BlockLine::Unterminated } else { BlockLine::Take }
            }
            LineKind::Opener(_) | LineKind::Other(_) => BlockLine::Take,
        }
    }

    /// Consume an `end` line that ends the block being read with the wrong
    /// keyword, or none (P093).
    ///
    /// It is the block's end written wrong: no block open around this one has
    /// that keyword, so reading it as the end of one of those would leave this
    /// block unterminated and the rest of the source in it.
    fn eat_wrong_end(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        keyword: &str,
    ) {
        let (_, line) = remaining_lines.next().X();
        let first = line.first().X();
        let mut ts = self.extract_text_span(&first);
        if let Some(second) = line.peek_nth(1) {
            ts = ts.with_end(self.extract_text_span(&second).span.end);
        }
        self.had_error = true;
        DiagnosticBuilder::error(self.db, &fmt!("this `end` line does not end the `{keyword}` it closes"))
            .code(PCode::WrongEnd.as_str())
            .primary_label(ts, &fmt!("expected `end {keyword}`"))
            .emit_to(&self.sink);
    }

    /// Read the lines of a block's body into `body`, up to the line that ends
    /// it or one of `stops`, which the caller reads.
    ///
    /// The block's keyword is the innermost of `open_blocks`. A correct `end`
    /// line is left for the caller to consume; a wrong one is consumed and
    /// reported here.
    ///
    /// A block without its `end` is not reported again (P034) where an error
    /// already accounts for the `end`: a line in its body, or in a block
    /// inside it, that begins no statement (P001) or ends a block wrong
    /// (P093) is taken for an `end` written wrong, which a block inside it may
    /// have taken this one's `end` to make up for; and a source that ends
    /// inside a bracket, string or comment never closed has taken the `end`
    /// into it.
    fn parse_block_body(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        keyword: &str,
        stops: &[&str],
        body: &mut Vec<ast::Statement<'db>>,
    ) -> BodyEnd {
        let filed_before = self.filed_count();
        loop {
            if let Some((_, line)) = remaining_lines.peek()
                && let LineKind::Other(Some(word)) = self.line_kind(line)
                && stops.contains(&word)
            {
                return BodyEnd::Stop;
            }
            match self.block_line(remaining_lines, keyword) {
                BlockLine::Close => return BodyEnd::Close,
                BlockLine::WrongEnd => {
                    self.eat_wrong_end(remaining_lines, keyword);
                    return BodyEnd::Closed;
                }
                BlockLine::Unterminated => return self.missing_end(remaining_lines, filed_before),
                BlockLine::Take => {
                    let (_, line) = remaining_lines.next().X();
                    let stmt = self.parse_line_statement(line, remaining_lines);
                    body.push(stmt);
                }
            }
        }
    }

    /// How a block that has run out without its `end` ended: swallowed, by a
    /// source ending inside a bracket, string or comment never closed;
    /// closed, where an error filed since `filed_before`, when its body began,
    /// accounts for the `end` already, a line that begins no statement (P001),
    /// ends a block wrong (P093) or is in no case of a match (P048), each an
    /// `end` or a header written wrong; or unterminated, to be reported.
    fn missing_end(
        &self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        filed_before: usize,
    ) -> BodyEnd {
        if self.ends_open && remaining_lines.peek().is_none() {
            BodyEnd::Swallowed
        } else if self.filed_since(filed_before, &[PCode::UnknownStatement, PCode::WrongEnd, PCode::LineOutsideCase]) {
            BodyEnd::Closed
        } else {
            BodyEnd::Unterminated
        }
    }

    /// How many diagnostics the parse has filed so far.
    fn filed_count(&self) -> usize {
        self.sink.as_ref().X().borrow().len()
    }

    /// Whether a diagnostic with one of `codes` was filed after the first `from`.
    fn filed_since(&self, from: usize, codes: &[PCode]) -> bool {
        let filed = self.sink.as_ref().X().borrow();
        filed[from..].iter().any(|d| codes.iter().any(|c| d.code.as_deref() == Some(c.as_str())))
    }

    /// Read a block's body and its end, after a header that is the block's
    /// error, so that the body is not read as statements of their own.
    fn skip_block(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        keyword: &'static str,
    ) {
        // The lines a block of this kind divides its body with, `else` and
        // `case`, are passed over as the block would take them.
        let dividers: &[&str] = match keyword {
            "if" => &["else"],
            "match" => &["case"],
            _ => &[],
        };
        // A function's `inline` line is its own, as `parse_fun` reads it.
        if keyword == "fun"
            && let Some((_, line)) = remaining_lines.peek()
            && let LineKind::Other(Some("inline")) = self.line_kind(line)
            && line.peek_nth(1).is_none()
        {
            remaining_lines.next();
        }
        self.open_blocks.push(keyword);
        let mut body = vec![];
        let end = loop {
            match self.parse_block_body(remaining_lines, keyword, dividers, &mut body) {
                BodyEnd::Stop => {
                    remaining_lines.next();
                }
                end => break end,
            }
        };
        self.open_blocks.pop();
        match end {
            BodyEnd::Close => self.eat_end_line(remaining_lines, keyword),
            BodyEnd::Closed | BodyEnd::Unterminated | BodyEnd::Swallowed => {}
            BodyEnd::Stop => unreachable!("a stop is passed over"),
        }
    }

    /// Stand for a block whose `end` was swallowed, reporting nothing more.
    fn swallowed_block(&mut self) -> ast::Statement<'db> {
        self.had_error = true;
        let end = self.last_byte_end();
        let message = InternedText::new(self.db, "the block's `end` is inside a bracket, string or comment never closed".S());
        ast::Statement::ParseError(ast::StmtParseError { source: self.source_text(), span: end..end, message })
    }

    /// Report a block that ran out of input before its `end` line.
    fn unterminated_block(&mut self, keyword: &str) -> ast::Statement<'db> {
        let end = self.last_byte_end();
        self.emit_stmt_error(
            TextSpan::new(self.source_text(), end..end),
            &fmt!("unterminated {keyword} body"),
            PCode::MissingEnd,
            &fmt!("expected `end {keyword}` before the end of the input"),
        )
    }

    fn parse_let(&mut self) -> ast::Statement<'db> {
        self.eat_word("let");

        let binding = match self.parse_binding() {
            Some(b) => b,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'let'",
                    PCode::LetWithoutName,
                    "expected name",
                );
            }
        };

        // Check for type hint: `: type`
        let type_hint = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint())
        } else {
            None
        };

        // Need `=` sigil. If missing, emit error and return parse error.
        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after let binding",
                PCode::LetWithoutEquals,
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Let(ast::StmtLet {
            binding,
            type_hint,
            value,
        })
    }

    /// Parse what a `let` or `var` binds: a name or a destructuring pattern.
    ///
    /// None if what follows is neither, having consumed nothing. A pattern that
    /// starts well and then goes wrong is reported here, and returned with
    /// what could be read of it.
    fn parse_binding(&mut self) -> Option<ast::Binding<'db>> {
        let mut seen = datalit::parser_util::SeenNames::default();
        if self.peek_sigil(Sigil::ParenOpen) {
            let paren_ts = self.peek_text_span();
            let inner = self.eat_branch(Sigil::ParenOpen).X();
            let mut sub = self.sub_parser(inner, None);
            let (names, had_comma) = sub.parse_comma_separated_with_trailing(|p| {
                p.parse_pattern_name(&mut seen)
            });
            sub.error_if_not_exhausted();
            self.merge_from_sub(&mut sub);
            // In an expression `(x)` groups, but a pattern has nothing to
            // group, so `(a)` is neither a name nor a tuple.
            if names.len() == 1 && !had_comma {
                self.had_error = true;
                DiagnosticBuilder::error(self.db, "a parenthesized name is not a pattern")
                    .code(PCode::ParenthesizedName.as_str())
                    .primary_label(paren_ts, "write `(a,)` for a one-element tuple, or drop the parentheses")
                    .emit_to(&self.sink);
            }
            return Some(ast::Binding::Tuple(names));
        }
        if let Some(inner) = self.eat_branch(Sigil::BraceOpen) {
            let mut sub = self.sub_parser(inner, None);
            let mut seen_fields = datalit::parser_util::SeenNames::default();
            let fields = sub.parse_comma_separated(|p| {
                let field_ts = p.peek_text_span();
                let field = match p.eat_name() {
                    Some(field) => field,
                    None => {
                        let error = p.pattern_name_error();
                        return ast::StructBinding { field: error, binding: error };
                    }
                };
                let fresh_field = seen_fields.take(p.db, &p.sink, field, field_ts.C(), "field");
                if !fresh_field {
                    p.had_error = true;
                }
                let binding = if p.eat_sigil(Sigil::Equals) {
                    p.parse_pattern_name(&mut seen)
                } else {
                    // A field named twice has been reported once already.
                    if fresh_field {
                        // The shorthand binds the field's name, so that name
                        // is declared, and is held to what a value's name is.
                        p.report_reserved_name(field, field_ts.C(), NameKind::Value);
                        p.declare_pattern_name(field, field_ts, &mut seen);
                    }
                    field
                };
                ast::StructBinding { field, binding }
            });
            sub.error_if_not_exhausted();
            self.merge_from_sub(&mut sub);
            return Some(ast::Binding::Struct(fields));
        }
        match self.peek_word() {
            Some("atom") => {
                self.next();
                let name = self.eat_name().unwrap_or_else(|| self.pattern_name_error());
                Some(ast::Binding::Atom(name))
            }
            Some("term") => {
                self.next();
                let name = self.eat_name().unwrap_or_else(|| self.pattern_name_error());
                let binding = self.parse_pattern_name(&mut seen);
                Some(ast::Binding::Term { name, binding })
            }
            _ => self.eat_declared_name(NameKind::Value).map(ast::Binding::Name),
        }
    }

    /// Parse a name a pattern binds, reporting it if it is bound twice.
    fn parse_pattern_name(
        &mut self,
        seen: &mut datalit::parser_util::SeenNames<'db>,
    ) -> InternedText<'db> {
        let ts = self.peek_text_span();
        match self.eat_declared_name(NameKind::Value) {
            Some(name) => {
                self.declare_pattern_name(name, ts, seen);
                name
            }
            None => self.pattern_name_error(),
        }
    }

    fn declare_pattern_name(
        &mut self,
        name: InternedText<'db>,
        ts: TextSpan,
        seen: &mut datalit::parser_util::SeenNames<'db>,
    ) {
        if !seen.take(self.db, &self.sink, name, ts, "binding") {
            self.had_error = true;
        }
    }

    /// Report a pattern missing a name, and stand in for it.
    fn pattern_name_error(&mut self) -> InternedText<'db> {
        self.had_error = true;
        let ts = self.error_span();
        DiagnosticBuilder::error(self.db, "expected a name in this pattern")
            .code(PCode::NestedPattern.as_str())
            .primary_label(ts, "expected a name")
            .emit_to(&self.sink);
        // Skip the rest of this element, so one mistake is one report.
        while self.peek().is_some() && !self.peek_sigil(Sigil::Comma) {
            self.next();
        }
        InternedText::new(self.db, "<error>".S())
    }

    fn parse_var(&mut self) -> ast::Statement<'db> {
        self.eat_word("var");

        let binding = match self.parse_binding() {
            Some(b) => b,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'var'",
                    PCode::VarWithoutName,
                    "expected name",
                );
            }
        };

        // Check for type hint: `: type`
        let type_hint = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint())
        } else {
            None
        };

        // Check for `=` sigil. If present, parse initializer.
        // If absent, this is an uninitialized var (requires type hint).
        let value = if self.eat_sigil(Sigil::Equals) {
            Some(self.parse_expr_full())
        } else {
            // No initializer - require a plain name and a type hint.
            if binding.as_name().is_none() {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "a destructuring var needs a value to take apart",
                    PCode::DestructuringVarWithoutValue,
                    "expected '='"
                );
            }
            if type_hint.is_none() {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "uninitialized var requires type hint",
                    PCode::VarWithoutTypeOrValue,
                    "add ': Type' or '= value'"
                );
            }
            None
        };

        ast::Statement::Var(ast::StmtVar {
            binding,
            type_hint,
            value,
        })
    }

    fn parse_const(&mut self) -> ast::Statement<'db> {
        self.eat_word("const");

        let name = match self.eat_declared_name(NameKind::Value) {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'const'",
                    PCode::ConstWithoutName,
                    "expected name",
                );
            }
        };

        // Check for type hint: `: type`
        let type_hint = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint())
        } else {
            None
        };

        // Need `=` sigil.
        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after const binding",
                PCode::ConstWithoutEquals,
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Const(ast::StmtConst {
            name,
            type_hint,
            value,
        })
    }

    fn parse_set(&mut self) -> ast::Statement<'db> {
        self.eat_word("set");

        // The statement's span is its target, which is what every error about
        // a `set` is about: `xs[i]!` rather than the keyword in front of it.
        let target_ts = self.peek_text_span();
        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'set'",
                    PCode::SetWithoutName,
                    "expected name",
                );
            }
        };

        // Parse optional field projections and index steps.
        let steps = self.parse_place_steps();
        let target = ast::Place { root: name, steps, hinted: None };
        let local_index = self.record_set_span(target_ts.with_end(self.last_byte_end()));

        // Need `=`, or a compound assignment's operator and `=`.
        let op = if self.eat_sigil(Sigil::Equals) {
            None
        } else if let Some(op) = self.eat_compound_assignment() {
            Some(op)
        } else {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after set target",
                PCode::SetWithoutAssignOp,
                "expected '='"
            );
        };

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Set(ast::StmtSet {
            target,
            op,
            value,
            local_index,
        })
    }

    /// Consume a compound assignment sigil, giving the operator it applies.
    fn eat_compound_assignment(&mut self) -> Option<ast::BinOp> {
        const COMPOUND: [(Sigil, ast::BinOp); 12] = [
            (Sigil::PlusEquals, ast::BinOp::Add),
            (Sigil::MinusEquals, ast::BinOp::Sub),
            (Sigil::StarEquals, ast::BinOp::Mul),
            (Sigil::SlashEquals, ast::BinOp::Div),
            (Sigil::PlusExclamationEquals, ast::BinOp::AddChecked),
            (Sigil::MinusExclamationEquals, ast::BinOp::SubChecked),
            (Sigil::StarExclamationEquals, ast::BinOp::MulChecked),
            (Sigil::SlashExclamationEquals, ast::BinOp::DivChecked),
            (Sigil::PlusQuestionEquals, ast::BinOp::AddOptional),
            (Sigil::MinusQuestionEquals, ast::BinOp::SubOptional),
            (Sigil::StarQuestionEquals, ast::BinOp::MulOptional),
            (Sigil::SlashQuestionEquals, ast::BinOp::DivOptional),
        ];
        COMPOUND.iter().find(|(sigil, _)| self.eat_sigil(*sigil)).map(|(_, op)| *op)
    }

    /// Parse place steps for set targets (e.g., `.x.0.y`, `[i]?`).
    ///
    /// Each step is written against what it steps from, as the same postfix
    /// operators are in an expression. One written apart is reported and read
    /// anyway, so that the rest of the statement is still checked.
    fn parse_place_steps(&mut self) -> Vec<ast::PlaceStep<'db>> {
        let mut steps = Vec::new();
        loop {
            let is_step = self.peek_sigil(Sigil::Dot)
                || matches!(self.peek(), Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }));
            if is_step && !self.glued_left() {
                self.report_spaced_postfix();
            }
            if self.peek_sigil(Sigil::Dot) {
                self.next(); // consume .
                let field = self.parse_field_selector();
                steps.push(ast::PlaceStep::Field(field));
            } else if let Some(inner) = self.eat_branch(Sigil::BracketOpen) {
                // Index access: [expr]? or [expr]!
                let mut sub = self.sub_parser(inner, None);
                let index = sub.parse_expr_full();
                sub.error_if_not_exhausted();
                self.merge_from_sub(&mut sub);
                // Optional ? or ! suffix. Bare index means upsert for maps.
                if (self.peek_sigil(Sigil::Question) || self.peek_sigil(Sigil::Exclamation))
                    && !self.glued_left()
                {
                    self.report_spaced_postfix();
                }
                let error_mode = if self.eat_sigil(Sigil::Question) {
                    Some(ast::IndexErrorMode::Option)
                } else if self.eat_sigil(Sigil::Exclamation) {
                    Some(ast::IndexErrorMode::Result)
                } else {
                    None
                };
                steps.push(ast::PlaceStep::Index(ast::PlaceIndex {
                    index,
                    error_mode,
                }));
            } else {
                break;
            }
        }
        steps
    }



    fn parse_fun(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        // Record function span starting at 'fun' keyword.
        let fun_span = self.peek_text_span();
        let local_index = self.record_fun_span(fun_span);
        self.eat_word("fun");

        // A header that does not parse still opens its block, so that its
        // body and its `end fun` are not read as statements of their own; the
        // header's error is the function's one error, and what stands for it.
        let mut broken = None;
        let name = match self.eat_declared_name(NameKind::Function) {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                broken = Some(self.emit_stmt_error(ts,
                    "expected function name after 'fun'",
                    PCode::FunWithoutName,
                    "expected function name",
                ));
                InternedText::new(self.db, "<error>".S())
            }
        };

        // Type parameters, if the name is followed by `<...>`. Angle brackets
        // are bracers, so this arrives as a single branch.
        let (type_params, type_bounds) = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::AngleOpen, .. }) => {
                match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => self.parse_type_params(inner),
                    _ => unreachable!("peeked an angle branch"),
                }
            }
            _ => (Vec::new(), Vec::new()),
        };

        // Parse parameters in parentheses. What is there instead is the
        // error, so it is looked at before it is taken.
        let params = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                let Some(TreeToken::Branch { open, inner, .. }) = self.next() else { unreachable!("peeked a branch") };
                let open_span = TextSpan::new(self.source_text(), open.span());
                self.parse_fun_params(inner, Some((open_span, "in this parameter list")))
            }
            _ => {
                if broken.is_none() {
                    let ts = self.error_span();
                    broken = Some(self.emit_stmt_error(ts,
                        "expected parameter list",
                        PCode::FunWithoutParams,
                        "expected '(' to start parameter list"
                    ));
                }
                vec![]
            }
        };

        // Check for return type: `: type`
        let return_type = if broken.is_none() && self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint())
        } else {
            None
        };

        // Bounds, if the signature ends with `where { T is float, }`.
        let type_bounds = if broken.is_none() { self.parse_where_clause(&type_params, type_bounds) } else { type_bounds };

        // Enter function context for expression identity tracking.
        let enclosing = self.enter_function(name);

        // `inline`, alone on the body's first line, marks the function for
        // inlining. Anywhere else it is an error, which `parse_statement` gives.
        let inline = remaining_lines.peek().is_some_and(|(_, line)| {
            let mut line = *line;
            matches!((line.next(), line.next()), (Some(TreeToken::Token(t)), None)
                if t.word_str(self.text()) == Some("inline"))
        });
        if inline {
            remaining_lines.next();
        }

        // What is left on the header line is its error, reported before the
        // body's errors can stand in for it.
        self.error_if_not_exhausted();

        // Parse body until we hit "end fun".
        let mut body = vec![];
        self.open_blocks.push("fun");
        let end = self.parse_block_body(remaining_lines, "fun", &[], &mut body);
        self.open_blocks.pop();

        let arena = self.exit_function(local_index, enclosing);
        match end {
            BodyEnd::Close => self.eat_end_line(remaining_lines, "fun"),
            BodyEnd::Closed => {}
            BodyEnd::Unterminated => return broken.unwrap_or_else(|| self.unterminated_block("fun")),
            BodyEnd::Swallowed => return broken.unwrap_or_else(|| self.swallowed_block()),
            BodyEnd::Stop => unreachable!("no stops were given"),
        }
        if let Some(error) = broken {
            return error;
        }

        ast::Statement::Fun(ast::StmtFun {
            module_id: self.module_id(),
            name,
            signature: std::sync::Arc::new(ast::FunSignature { type_params, type_bounds, params, return_type, inline }),
            body: ast::Body { statements: body, arena },
            local_index,
        })
    }

    /// Parse the type parameter names inside `<...>` on a function signature.
    ///
    /// The bounds come separately, from the `where` clause at the end of the
    /// signature, so this gives an empty bound for each name and the clause
    /// fills them in.
    fn parse_type_params(
        &mut self,
        inner: Cursor<'db>,
    ) -> (Vec<InternedText<'db>>, Vec<Option<ast::TypeBound>>) {
        let mut sub = Parser::from_branch_with_context(
            self.db, inner, self.source_text(), None, self.module_id(), self.sink.C());
        let names = sub.parse_comma_separated(|p| {
            let ts = p.peek_text_span();
            let name = p.eat_declared_name(NameKind::Type);
            if name.is_none() {
                p.had_error = true;
                DiagnosticBuilder::error(p.db, "expected a type parameter's name")
                    .code(PCode::TypeParamWithoutName.as_str())
                    .primary_label(ts, "a type parameter is a name")
                    .emit_to(&self.sink);
            }
            name
        });
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        let names: Vec<_> = names.into_iter().flatten().collect();
        let bounds = vec![None; names.len()];
        (names, bounds)
    }

    /// Parse `where { T is float, }` after a signature, if it is there.
    ///
    /// A bound says which types its parameter may be, and in exchange the body
    /// may do what all of them have in common. Written apart from the name so
    /// that a signature reads as a signature and the constraints sit together
    /// underneath it.
    fn parse_where_clause(
        &mut self,
        type_params: &[InternedText<'db>],
        mut bounds: Vec<Option<ast::TypeBound>>,
    ) -> Vec<Option<ast::TypeBound>> {
        if self.peek_word() != Some("where") {
            return bounds;
        }
        self.eat_word("where");
        let Some(inner) = self.eat_branch(Sigil::BraceOpen) else {
            self.had_error = true;
            DiagnosticBuilder::error(self.db, "expected `{` after `where`")
                .code(PCode::WhereWithoutBrace.as_str())
                .primary_label(self.peek_text_span(), "the bounds go in braces")
                .emit_to(&self.sink);
            return bounds;
        };

        let mut sub = Parser::from_branch_with_context(
            self.db, inner, self.source_text(), None, self.module_id(), self.sink.C());
        let clauses = sub.parse_comma_separated(|p| p.parse_one_bound());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;

        for (name, bound, ts) in clauses.into_iter().flatten() {
            match type_params.iter().position(|p| *p == name) {
                Some(i) if bounds[i].is_some() => {
                    self.had_error = true;
                    DiagnosticBuilder::error(self.db, &format!(
                        "`{}` is bounded twice", name.text(self.db)))
                        .code(PCode::TypeParamBoundedTwice.as_str())
                        .primary_label(ts, "a type parameter takes one bound")
                        .emit_to(&self.sink);
                }
                Some(i) => bounds[i] = Some(bound),
                None => {
                    self.had_error = true;
                    DiagnosticBuilder::error(self.db, &format!(
                        "`{}` is not a type parameter of this function",
                        name.text(self.db)))
                        .code(PCode::BoundOnUndeclaredParam.as_str())
                        .primary_label(ts, "bounded here but never declared")
                        .emit_to(&self.sink);
                }
            }
        }
        bounds
    }

    /// One line of a `where` clause: `T is float`.
    fn parse_one_bound(
        &mut self,
    ) -> Option<(InternedText<'db>, ast::TypeBound, TextSpan)> {
        let name_span = self.peek_text_span();
        let name = self.eat_name()?;
        if !self.eat_word("is") {
            self.had_error = true;
            DiagnosticBuilder::error(self.db, "expected `is` after the type parameter")
                .code(PCode::BoundWithoutIs.as_str())
                .primary_label(self.peek_text_span(), "a bound reads `T is float`")
                .emit_to(&self.sink);
            return None;
        }
        let ts = self.peek_text_span();
        let bound_name = self.eat_name()?;
        match ast::TypeBound::from_name(bound_name.text(self.db)) {
            Some(bound) => Some((name, bound, name_span)),
            None => {
                self.had_error = true;
                DiagnosticBuilder::error(self.db, &format!(
                    "unknown bound `{}`", bound_name.text(self.db)))
                    .code(PCode::UnknownBound.as_str())
                    .primary_label(ts, "not a bound this language has")
                    .emit_to(&self.sink);
                None
            }
        }
    }

    fn parse_fun_params(
        &mut self,
        iter: Cursor<'db>,
        context: Option<(TextSpan, &'static str)>,
    ) -> Vec<ast::FunParam<'db>> {
        let mut sub = Parser::from_branch_with_context(self.db, iter, self.source_text(), context, self.module_id(), self.sink.C());
        // A parameter list holds types and nothing else, so the alias
        // numbering is the only thing it has to carry on with and hand back.
        sub.lend_alias_numbering(self);
        let params = sub.parse_comma_separated(|p| p.parse_fun_param());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        self.take_alias_numbering(&mut sub);
        params
    }

    /// Parse a single function parameter: `[const] [mode] name: type`.
    fn parse_fun_param(&mut self) -> ast::FunParam<'db> {
        // Check for `const` modifier (comptime parameter).
        let is_comptime = if self.peek_word() == Some("const") {
            self.eat_word("const");
            true
        } else {
            false
        };

        // Check for parameter mode keywords.
        let mode = match self.peek_word() {
            Some("out") => {
                self.eat_word("out");
                ast::ParamMode::Out
            }
            Some("ref") => {
                self.eat_word("ref");
                ast::ParamMode::Ref
            }
            Some("mut") => {
                self.eat_word("mut");
                ast::ParamMode::Mut
            }
            _ => ast::ParamMode::In,
        };

        // A const parameter has no passing mode, because nothing is passed:
        // specialization removes the parameter and writes the value into the
        // body, so `ref` has no borrow to describe and `out` and `mut` have
        // nothing to write back to.
        if is_comptime && !matches!(mode, ast::ParamMode::In) {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "const parameter cannot be 'ref', 'out' or 'mut'")
                .code(PCode::ConstParamWithMode.as_str())
                .primary_label(ts, "invalid combination")
                .emit_to(&self.sink);
        }

        let name = match self.eat_declared_name(NameKind::Parameter) {
            Some(n) => n,
            None => {
                self.had_error = true;
                let ts = self.peek_text_span();
                DiagnosticBuilder::error(self.db, "expected parameter name")
                    .code(PCode::ParamWithoutName.as_str())
                    .primary_label(ts, "expected parameter name")
                    .emit_to(&self.sink);
                InternedText::new(self.db, "<error>".S())
            }
        };

        // Need colon.
        if !self.eat_sigil(Sigil::Colon) {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "expected ':' after parameter name")
                .code(PCode::ParamWithoutColon.as_str())
                .primary_label(ts, "expected ':'")
                .emit_to(&self.sink);
        }

        let type_hint = self.parse_type_hint();

        ast::FunParam { name, mode, is_comptime, type_hint }
    }

    fn parse_ret(&mut self) -> ast::Statement<'db> {
        let ts = self.peek_text_span();
        self.eat_word("ret");
        let local_index = self.record_ret_span(ts);

        // Bare `ret` for void functions has no expression.
        let value = if self.peek().is_some() {
            Some(self.parse_expr_full())
        } else {
            None
        };

        ast::Statement::Ret(ast::StmtRet { value, local_index })
    }

    fn parse_require(&mut self) -> ast::Statement<'db> {
        // The whole statement is what a diagnostic about this require points
        // at, so the span is taken before any of it is consumed.
        let ts = self.peek_text_span();
        self.eat_word("require");

        match self.peek_word() {
            Some("module") => {
                self.eat_word("module");
                let (import_space, package_alias, module_alias) =
                    match self.parse_require_path("module", PCode::RequireModuleWithoutSpace, NameKind::Module) {
                        Ok(path) => path,
                        Err(error) => return error,
                    };
                let local_index = self.record_require_span(ts.with_end(self.last_byte_end()));
                ast::Statement::Require(ast::StmtRequire::Module(
                    ast::StmtRequireModule {
                        import_space,
                        package_alias,
                        module_alias,
                        local_index,
                    }
                ))
            }
            Some("data") => {
                self.eat_word("data");
                let (import_space, package_alias, data_alias) =
                    match self.parse_require_path("data", PCode::RequireDataWithoutSpace, NameKind::Value) {
                        Ok(path) => path,
                        Err(error) => return error,
                    };

                // Optional type hint: `: type`
                let type_hint = if self.peek_sigil(Sigil::Colon) {
                    self.eat_sigil(Sigil::Colon);
                    Some(self.parse_type_hint())
                } else {
                    None
                };

                // The const the data is bound to. Its value is the file, and
                // what the file says is checked against the hint the way any
                // const's value is.
                let file = self.create_expr(
                    ast::ExprFunKind::DataFile(ast::ExprDataFile { import_space, package_alias, data_alias }),
                    ts.with_end(self.last_byte_end()),
                );
                ast::Statement::Const(ast::StmtConst { name: data_alias, type_hint, value: file })
            }
            Some("rider") => {
                self.eat_word("rider");

                let name = match self.eat_declared_name(NameKind::Module) {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        return self.emit_stmt_error(ts,
                            "expected rider name after 'require rider'",
                            PCode::RequireRiderWithoutName,
                            "expected rider name",
                        );
                    }
                };

                let local_index = self.record_require_span(ts.with_end(self.last_byte_end()));
                ast::Statement::Require(ast::StmtRequire::Rider(
                    ast::StmtRequireRider { name, local_index }
                ))
            }
            _ => {
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
                    "expected 'module', 'data', or 'rider' after 'require'",
                    PCode::BadRequireKind,
                    "expected 'module', 'data', or 'rider'"
                )
            }
        }
    }

    /// The `library/package/name` path a `require module` or `require data`
    /// names, or the error statement for a malformed one.
    ///
    /// Both resolve the same way, so they are written the same way. The last
    /// segment is the name the require binds, which is a module alias for a
    /// module and a value for data.
    fn parse_require_path(
        &mut self,
        kind: &str,
        first_code: PCode,
        alias_kind: NameKind,
    ) -> Result<(InternedText<'db>, InternedText<'db>, InternedText<'db>), ast::Statement<'db>> {
        let import_space = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return Err(self.emit_stmt_error(ts,
                    &fmt!("expected import space name after 'require {kind}'"),
                    first_code,
                    "expected import space name",
                ));
            }
        };

        if !self.peek_sigil(Sigil::SlashForward) {
            let ts = self.error_span();
            return Err(self.emit_stmt_error(ts,
                "expected '/' after import space",
                PCode::RequireWithoutSpaceSlash,
                "expected '/' after import space"
            ));
        }
        self.eat_sigil(Sigil::SlashForward);

        let package_alias = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.error_span();
                return Err(self.emit_stmt_error(ts,
                    "expected package name after '/'",
                    PCode::RequireWithoutPackage,
                    "expected package name",
                ));
            }
        };

        if !self.peek_sigil(Sigil::SlashForward) {
            let ts = self.error_span();
            return Err(self.emit_stmt_error(ts,
                "expected '/' after package alias",
                PCode::RequireWithoutPackageSlash,
                "expected '/' after package alias"
            ));
        }
        self.eat_sigil(Sigil::SlashForward);

        let alias = match self.eat_declared_name(alias_kind) {
            Some(n) => n,
            None => {
                let ts = self.error_span();
                return Err(self.emit_stmt_error(ts,
                    &fmt!("expected {kind} name after '/'"),
                    PCode::RequireWithoutName,
                    &fmt!("expected {kind} name"),
                ));
            }
        };

        Ok((import_space, package_alias, alias))
    }

    fn parse_import(&mut self) -> ast::Statement<'db> {
        // The whole statement is what a diagnostic about this import points
        // at, so the span is taken before any of it is consumed.
        let ts = self.peek_text_span();
        self.eat_word("import");

        // Parse module name.
        let module_name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected module name after 'import'",
                    PCode::ImportWithoutModule,
                    "expected module name",
                );
            }
        };

        // Need dot sigil.
        if !self.peek_sigil(Sigil::Dot) {
            let ts = self.error_span();
            return self.emit_stmt_error(ts,
                "expected '.' after module name",
                PCode::ImportWithoutDot,
                "expected '.' after module name"
            );
        }
        self.eat_sigil(Sigil::Dot);

        // Parse item name.
        let item_name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.error_span();
                return self.emit_stmt_error(ts,
                    "expected item name after '.'",
                    PCode::ImportWithoutItem,
                    "expected item name",
                );
            }
        };

        let local_index = self.record_import_span(ts.with_end(self.last_byte_end()));
        ast::Statement::Import(
            ast::StmtImport {
                module_name,
                item_name,
                local_index,
            }
        )
    }

    fn parse_if(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        self.eat_word("if");
        self.open_blocks.push("if");
        let (stmt, end) = self.parse_if_arm(remaining_lines, false);
        self.open_blocks.pop();
        match end {
            BodyEnd::Close => self.eat_end_line(remaining_lines, "if"),
            BodyEnd::Closed => {}
            BodyEnd::Unterminated => return self.unterminated_block("if"),
            BodyEnd::Swallowed => return self.swallowed_block(),
            BodyEnd::Stop => unreachable!("an arm reads its `else`"),
        }
        ast::Statement::If(stmt)
    }

    /// Parse an `if` after its keyword, stopping before the `end if` line.
    ///
    /// An `else if` line is parsed as a nested arm that becomes the whole
    /// else body, so a chain shares the single `end if` of its first `if`.
    /// `chained` says this arm is one of those, after an `else`.
    /// Returns the arm and how the chain ended, at its `end if` or without it.
    ///
    /// A destructuring `if` takes no part in a chain, on either side of an
    /// `else if` (P073): which branch's bindings are in scope where, and what
    /// an `else |e|` at the end of the chain would bind, are not obvious.
    fn parse_if_arm(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
        chained: bool,
    ) -> (ast::StmtIf<'db>, BodyEnd) {
        let condition = self.parse_expr_full();
        let binding_ts = self.peek_text_span();
        let then_binding = self.parse_pipe_binding();
        if chained && then_binding.is_some() {
            self.had_error = true;
            DiagnosticBuilder::error(self.db, "`else if` cannot destructure")
                .code(PCode::DestructuringElseIf.as_str())
                .primary_label(binding_ts.with_end(self.last_byte_end()), "destructuring binding in an `else if`")
                .help("nest this `if` inside an `else` body instead")
                .emit_to(&self.sink);
        }

        self.error_if_not_exhausted();
        let mut then_body = vec![];
        let then_end = self.parse_block_body(remaining_lines, "if", &["else"], &mut then_body);
        let found_else = matches!(then_end, BodyEnd::Stop);
        let mut end = then_end;

        let (else_binding, else_body) = if found_else {
            let (_, else_line) = remaining_lines.next().X();
            let mut else_sub = self.new_sub(else_line);
            else_sub.eat_word("else");
            let else_binding = else_sub.parse_pipe_binding();

            if else_sub.peek_word() == Some("if") {
                if else_binding.is_some() {
                    else_sub.had_error = true;
                    let ts = else_sub.peek_text_span();
                    DiagnosticBuilder::error(else_sub.db, "an else binding cannot be followed by `if`")
                        .code(PCode::ElseBindingThenIf.as_str())
                        .primary_label(ts, "nest this `if` inside the else body instead")
                        .emit_to(&self.sink);
                } else if then_binding.is_some() {
                    else_sub.had_error = true;
                    let ts = else_sub.peek_text_span();
                    DiagnosticBuilder::error(else_sub.db, "a destructuring `if` cannot be followed by `else if`")
                        .code(PCode::DestructuringElseIf.as_str())
                        .primary_label(ts, "`else if` after a destructuring `if`")
                        .help("nest this `if` inside an `else` body instead")
                        .emit_to(&self.sink);
                }
                else_sub.eat_word("if");
                let (nested, nested_end) = else_sub.parse_if_arm(remaining_lines, true);
                else_sub.error_if_not_exhausted();
                self.merge_identity_from(&mut else_sub);
                end = nested_end;
                (else_binding, Some(vec![ast::Statement::If(nested)]))
            } else {
                else_sub.error_if_not_exhausted();
                let broken = else_sub.had_error;
                self.merge_identity_from(&mut else_sub);

                let mut body = vec![];
                if broken {
                    // An `else` line that did not parse, an `else if` written
                    // wrong, leaves the chain's later `else` lines its own.
                    end = self.parse_block_body(remaining_lines, "if", &["else"], &mut body);
                    while matches!(end, BodyEnd::Stop) {
                        remaining_lines.next();
                        end = self.parse_block_body(remaining_lines, "if", &["else"], &mut body);
                    }
                } else {
                    // A second `else` is a line of the body, and its error.
                    end = self.parse_block_body(remaining_lines, "if", &[], &mut body);
                }
                (else_binding, Some(body))
            }
        } else {
            (None, None)
        };

        let stmt = ast::StmtIf {
            condition,
            then_binding,
            then_body,
            else_binding,
            else_body,
        };
        (stmt, end)
    }

    /// Parse an optional `|name|` branch binding.
    fn parse_pipe_binding(&mut self) -> Option<InternedText<'db>> {
        if !self.peek_sigil(Sigil::Pipe) {
            return None;
        }
        self.eat_sigil(Sigil::Pipe);
        let binding = match self.eat_declared_name(NameKind::Value) {
            Some(n) => n,
            None => {
                self.had_error = true;
                let ts = self.peek_text_span();
                DiagnosticBuilder::error(self.db, "expected binding name after '|'")
                    .code(PCode::PipeBindingWithoutName.as_str())
                    .primary_label(ts, "expected binding name")
                    .emit_to(&self.sink);
                InternedText::new(self.db, "<error>".S())
            }
        };
        if !self.eat_sigil(Sigil::Pipe) {
            self.had_error = true;
            // Use position after binding name, not peek position (which may be 0 at end of line).
            let pos = self.last_byte_end();
            let ts = TextSpan::new(self.source_text(), pos..pos);
            DiagnosticBuilder::error(self.db, "expected '|' after binding name")
                .code(PCode::PipeBindingUnclosed.as_str())
                .primary_label(ts, "expected '|'")
                .emit_to(&self.sink);
        }
        Some(binding)
    }

    fn parse_loop(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        self.eat_word("loop");

        // Check for optional `while condition` clause.
        let condition = if self.peek_word() == Some("while") {
            self.eat_word("while");
            Some(self.parse_expr_full())
        } else {
            None
        };

        self.error_if_not_exhausted();

        // Parse body until we hit "end loop".
        let mut body = vec![];
        self.open_blocks.push("loop");
        let end = self.parse_block_body(remaining_lines, "loop", &[], &mut body);
        self.open_blocks.pop();
        match end {
            BodyEnd::Close => self.eat_end_line(remaining_lines, "loop"),
            BodyEnd::Closed => {}
            BodyEnd::Unterminated => return self.unterminated_block("loop"),
            BodyEnd::Swallowed => return self.swallowed_block(),
            BodyEnd::Stop => unreachable!("no stops were given"),
        }

        ast::Statement::Loop(ast::StmtLoop { condition, body })
    }

    /// `for item in ref coll`, or with two names, `for i, item` or
    /// `for key, value`, then a body up to `end for`.
    ///
    /// A header that does not parse still takes its body with it, so that the
    /// body and its `end for` are not read as statements of their own.
    fn parse_for(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        self.eat_word("for");
        let header = self.parse_for_header();
        self.error_if_not_exhausted();

        let mut body = vec![];
        self.open_blocks.push("for");
        let end = self.parse_block_body(remaining_lines, "for", &[], &mut body);
        self.open_blocks.pop();
        match end {
            BodyEnd::Close => self.eat_end_line(remaining_lines, "for"),
            BodyEnd::Closed => {}
            BodyEnd::Unterminated => return self.unterminated_block("for"),
            BodyEnd::Swallowed => return self.swallowed_block(),
            BodyEnd::Stop => unreachable!("no stops were given"),
        }

        match header {
            Ok((bindings, iteratee)) => ast::Statement::For(ast::StmtFor { bindings, iteratee, body }),
            Err(error) => error,
        }
    }

    /// The names a `for` binds and what it walks, after the `for`.
    fn parse_for_header(&mut self) -> Result<(Vec<InternedText<'db>>, ast::ExprId), ast::Statement<'db>> {
        let mut bindings = vec![];
        let mut seen = datalit::parser_util::SeenNames::default();
        loop {
            let ts = self.peek_text_span();
            match self.eat_declared_name(NameKind::Value) {
                Some(name) => {
                    self.declare_pattern_name(name, ts, &mut seen);
                    bindings.push(name);
                }
                None => {
                    let ts = self.peek_text_span();
                    return Err(self.emit_stmt_error(ts,
                        "expected a name for `for` to bind",
                        PCode::ForNames,
                        "a `for` binds a name to each element"));
                }
            }
            if !self.eat_sigil(Sigil::Comma) {
                break;
            }
        }
        if bindings.len() > 2 {
            let ts = self.peek_text_span();
            return Err(self.emit_stmt_error(ts,
                "a `for` binds one or two names",
                PCode::ForNames,
                "an element, an index and an element, or a key and a value"));
        }

        if !self.eat_word("in") {
            let ts = self.peek_text_span();
            return Err(self.emit_stmt_error(ts,
                "expected `in` after the names a `for` binds",
                PCode::ForWithoutIn,
                "expected `in`"));
        }
        if !self.eat_word("ref") {
            let ts = self.peek_text_span();
            return Err(self.emit_stmt_error(ts,
                "a `for` walks its collection by reference",
                PCode::ForWithoutRef,
                "write `in ref` before the collection"));
        }
        Ok((bindings, self.parse_expr_full()))
    }

    fn parse_break(&mut self) -> ast::Statement<'db> {
        let ts = self.peek_text_span();
        self.eat_word("break");
        let local_index = self.record_break_span(ts);
        ast::Statement::Break(ast::StmtBreak { local_index })
    }

    fn parse_continue(&mut self) -> ast::Statement<'db> {
        let ts = self.peek_text_span();
        self.eat_word("continue");
        let local_index = self.record_continue_span(ts);
        ast::Statement::Continue(ast::StmtContinue { local_index })
    }

    fn parse_debuglog(&mut self) -> ast::Statement<'db> {
        self.eat_word("debuglog");
        let value = self.parse_expr_full();
        ast::Statement::DebugLog(ast::StmtDebugLog { value })
    }

    /// Parse a `call` statement, whose expression must be a function call.
    ///
    /// The call may be followed by `?` and `!`, so a function returning `?()`
    /// or `!()` can be called for its effect.
    fn parse_call(&mut self) -> ast::Statement<'db> {
        self.eat_word("call");
        let expr = self.parse_expr_full();
        let mut inner = expr;
        loop {
            match self.expr_kind(inner) {
                ast::ExprFunKind::TryOption(t) => inner = t.operand,
                ast::ExprFunKind::TryResult(t) => inner = t.operand,
                _ => break,
            }
        }
        match self.expr_kind(inner) {
            ast::ExprFunKind::FunctionCall(_) => {
                ast::Statement::ExprStatement(ast::StmtExprStatement { expr })
            }
            _ => {
                let ts = TextSpan::new(self.source_text(),
                    self.expr_span_start(expr)..self.last_byte_end());
                self.emit_stmt_error(ts,
                    "`call` takes a function call",
                    PCode::CallOfNonCall,
                    "not a function call"
                )
            }
        }
    }

    fn parse_type_alias(&mut self) -> ast::Statement<'db> {
        let ts = self.peek_text_span();
        self.eat_word("type");
        let local_index = self.record_type_alias_span(ts);

        let name = match self.eat_declared_name(NameKind::Type) {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'type'",
                    PCode::TypeAliasWithoutName,
                    "expected name",
                );
            }
        };

        // An alias defines its name, so it takes `=`; `:` would say the
        // name is a value of the type.
        if !self.eat_sigil(Sigil::Equals) {
            let label = if self.peek_sigil(Sigil::Colon) {
                "a type alias is defined with '=', not ':'"
            } else {
                "expected '='"
            };
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after type alias name",
                PCode::TypeAliasWithoutEquals,
                label
            );
        }

        // Parse the type hint.
        let type_hint = self.parse_type_hint();

        ast::Statement::TypeAlias(ast::StmtTypeAlias {
            name,
            type_hint,
            local_index,
        })
    }

    /// A `native fun`, or an `intrinsic native fun`: a native the compiler
    /// knows the meaning of, by its name (P085 if it knows no such one).
    fn parse_native_fun(&mut self) -> ast::Statement<'db> {
        // Recorded in the same table a `fun` uses, so a diagnostic about the
        // declaration has somewhere to point.
        let native_span = self.peek_text_span();
        let local_index = self.record_fun_span(native_span.C());
        let intrinsic = self.peek_word() == Some("intrinsic");
        if intrinsic {
            self.eat_word("intrinsic");
            if self.peek_word() != Some("native") {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected 'native' after 'intrinsic'",
                    PCode::NativeFunHeader,
                    "expected 'native'",
                );
            }
        }
        self.eat_word("native");

        if self.peek_word() != Some("fun") {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected 'fun' after 'native'",
                PCode::NativeFunHeader,
                "expected 'fun'",
            );
        }
        self.eat_word("fun");

        let name = match self.eat_declared_name(NameKind::Function) {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected function name after 'native fun'",
                    PCode::NativeFunWithoutName,
                    "expected function name",
                );
            }
        };

        // Type parameters, spelled as they are on a regular function.
        let (type_params, type_bounds) = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::AngleOpen, .. }) => {
                match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => self.parse_type_params(inner),
                    _ => unreachable!("peeked an angle branch"),
                }
            }
            _ => (Vec::new(), Vec::new()),
        };

        // Parse parameters in parentheses. What is there instead is the
        // error, so it is looked at before it is taken.
        let params = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                let Some(TreeToken::Branch { open, inner, .. }) = self.next() else { unreachable!("peeked a branch") };
                let open_span = TextSpan::new(self.source_text(), open.span());
                self.parse_fun_params(inner, Some((open_span, "in this parameter list")))
            }
            _ => {
                let ts = self.error_span();
                return self.emit_stmt_error(ts,
                    "expected parameter list for native fun",
                    PCode::NativeFunWithoutParams,
                    "expected '(' to start parameter list",
                );
            }
        };

        // Check for return type: `: type`.
        let return_type = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint())
        } else {
            None
        };

        let type_bounds = self.parse_where_clause(&type_params, type_bounds);

        if intrinsic && datalove_datafun_intrinsics::NativeIntrinsic::lookup(name.as_str(self.db)).is_none() {
            self.had_error = true;
            DiagnosticBuilder::error(self.db, &fmt!("no intrinsic native `{}`", name.as_str(self.db)))
                .code(PCode::UnknownIntrinsic.as_str())
                .primary_label(native_span, "the compiler knows no native by this name")
                .help("an intrinsic native is one of `list_push`, `list_pop`, `mem_swap` and `mem_replace`")
                .emit_to(&self.sink);
        }

        ast::Statement::NativeFun(ast::StmtNativeFun {
            name,
            type_params,
            type_bounds,
            params,
            return_type,
            intrinsic,
            local_index,
        })
    }

    fn parse_match(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> ast::Statement<'db> {
        self.eat_word("match");

        // Parse input expression.
        let input = self.parse_expr_full();
        self.error_if_not_exhausted();

        // Parse case arms until "end match".
        let mut cases = vec![];
        let mut default_body = None;
        self.open_blocks.push("match");
        let filed_before = self.filed_count();
        let mut end;

        loop {
            // Its own `case` lines are this match's to read.
            let own_case = remaining_lines
                .peek()
                .is_some_and(|(_, line)| matches!(self.line_kind(line), LineKind::Other(Some("case"))));
            let action = if own_case { BlockLine::Take } else { self.block_line(remaining_lines, "match") };
            match action {
                BlockLine::Close => {
                    end = BodyEnd::Close;
                    break;
                }
                BlockLine::WrongEnd => {
                    self.eat_wrong_end(remaining_lines, "match");
                    end = BodyEnd::Closed;
                    break;
                }
                BlockLine::Unterminated => {
                    end = BodyEnd::Unterminated;
                    break;
                }
                BlockLine::Take => {}
            }
            let (_, line) = remaining_lines.peek().X();

            // Check for "case" line.
            if let Some(TreeToken::Token(t1)) = line.first() {
                if let Some("case") = t1.word_str(self.text()) {
                    let (_, case_line) = remaining_lines.next().X();
                    let mut case_sub = self.new_sub(case_line);
                    case_sub.eat_word("case");

                    match case_sub.peek_word() {
                        Some("atom") => {
                            case_sub.next(); // consume "atom"
                            let name = match case_sub.eat_name() {
                                Some(n) => n,
                                None => {
                                    self.had_error = true;
                                    let ts = case_sub.peek_text_span();
                                    DiagnosticBuilder::error(self.db, "expected name after 'case atom'")
                                        .code(PCode::CaseAtomWithoutName.as_str())
                                        .primary_label(ts, "expected atom name")
                                        .emit_to(&self.sink);
                                    InternedText::new(self.db, "<error>".S())
                                }
                            };
                            case_sub.error_if_not_exhausted();
                            self.merge_identity_from(&mut case_sub);

                            // Parse body until next case/default/end match.
                            let (body, arm_end) = self.parse_match_arm_body(remaining_lines);
                            cases.push(ast::MatchCase {
                                kind: ast::MatchCaseKind::Atom { name },
                                body,
                            });
                            end = arm_end;
                        }
                        Some("term") => {
                            case_sub.next(); // consume "term"
                            let name = match case_sub.eat_name() {
                                Some(n) => n,
                                None => {
                                    self.had_error = true;
                                    let ts = case_sub.peek_text_span();
                                    DiagnosticBuilder::error(self.db, "expected name after 'case term'")
                                        .code(PCode::CaseTermWithoutName.as_str())
                                        .primary_label(ts, "expected term name")
                                        .emit_to(&self.sink);
                                    InternedText::new(self.db, "<error>".S())
                                }
                            };
                            let binding = match case_sub.eat_declared_name(NameKind::Value) {
                                Some(n) => n,
                                None => {
                                    self.had_error = true;
                                    let ts = case_sub.peek_text_span();
                                    DiagnosticBuilder::error(self.db, "expected binding name after term name")
                                        .code(PCode::CaseTermWithoutBinding.as_str())
                                        .primary_label(ts, "expected binding name")
                                        .emit_to(&self.sink);
                                    InternedText::new(self.db, "<error>".S())
                                }
                            };
                            case_sub.error_if_not_exhausted();
                            self.merge_identity_from(&mut case_sub);

                            // Parse body until next case/default/end match.
                            let (body, arm_end) = self.parse_match_arm_body(remaining_lines);
                            cases.push(ast::MatchCase {
                                kind: ast::MatchCaseKind::Term { name, binding },
                                body,
                            });
                            end = arm_end;
                        }
                        Some("default") => {
                            let default_span = case_sub.peek_text_span();
                            case_sub.next(); // consume "default"
                            case_sub.error_if_not_exhausted();
                            self.merge_identity_from(&mut case_sub);

                            // Parsed either way, so that a second default's body
                            // is checked like any other rather than skipped.
                            let (body, arm_end) = self.parse_match_arm_body(remaining_lines);
                            end = arm_end;
                            if default_body.is_some() {
                                self.had_error = true;
                                DiagnosticBuilder::error(self.db, "duplicate `case default` in match")
                                    .code(PCode::DuplicateDefaultCase.as_str())
                                    .primary_label(default_span, "a match takes only one default arm")
                                    .emit_to(&self.sink);
                            } else {
                                default_body = Some(body);
                            }
                        }
                        _ => {
                            self.had_error = true;
                            let ts = case_sub.peek_text_span();
                            DiagnosticBuilder::error(self.db, "expected 'atom', 'term', or 'default' after 'case'")
                                .code(PCode::BadCaseKind.as_str())
                                .primary_label(ts, "expected 'atom', 'term', or 'default'")
                                .emit_to(&self.sink);
                            self.merge_identity_from(&mut case_sub);
                            // Skip to next case/end match.
                            let (_, arm_end) = self.parse_match_arm_body(remaining_lines);
                            end = arm_end;
                        }
                    }
                    if matches!(end, BodyEnd::Closed | BodyEnd::Unterminated | BodyEnd::Swallowed) {
                        break;
                    }
                    continue;
                }
            }

            // A line inside a match that is not a `case` belongs to no arm.
            // It is taken for a `case` line written wrong, the one error, and
            // the lines after it for its arm, read as any other.
            let (_, line) = remaining_lines.next().X();
            self.had_error = true;
            let ts = self.extract_text_span(&line.first().X());
            DiagnosticBuilder::error(self.db, "this line is in no case of the match")
                .code(PCode::LineOutsideCase.as_str())
                .primary_label(ts, "every statement here belongs to a `case`")
                .emit_to(&self.sink);
            let (_, arm_end) = self.parse_match_arm_body(remaining_lines);
            end = arm_end;
            if matches!(end, BodyEnd::Closed | BodyEnd::Unterminated | BodyEnd::Swallowed) {
                break;
            }
        }

        self.open_blocks.pop();
        if matches!(end, BodyEnd::Unterminated) {
            end = self.missing_end(remaining_lines, filed_before);
        }
        match end {
            BodyEnd::Close => self.eat_end_line(remaining_lines, "match"),
            BodyEnd::Closed => {}
            BodyEnd::Unterminated => return self.unterminated_block("match"),
            BodyEnd::Swallowed => return self.swallowed_block(),
            BodyEnd::Stop => unreachable!("an arm stops at a `case`, which the loop reads"),
        }

        ast::Statement::Match(ast::StmtMatch {
            input,
            cases,
            default_body,
        })
    }

    /// Parse match arm body lines until the next case/default/end match.
    fn parse_match_arm_body(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Cursor<'db>)> + Clone>,
    ) -> (Vec<ast::Statement<'db>>, BodyEnd) {
        let mut body = vec![];
        let end = self.parse_block_body(remaining_lines, "match", &["case"], &mut body);
        (body, end)
    }

    /// Read a type hint, which datalit knows the shape of and this does not.
    ///
    /// It reads straight out of this parser, so it consumes the type and
    /// nothing else: the `=` of a `let`, the `,` between two parameters and
    /// the `where` opening a bounds clause are all still here afterwards, for
    /// whichever caller wanted one to say so itself.
    pub(super) fn parse_type_hint(&mut self) -> datalit::ast::TypeHint<'db> {
        datalit::parser::parse_type_hint(self)
    }
}
