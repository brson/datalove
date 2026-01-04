//! Statement parsing.

use rmx::prelude::*;
use rmx::core::iter::Peekable;

use bct::{
    lexer::Sigil,
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};
use datalove_diagnostic::DiagnosticBuilder;

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{TokenStream, TokenStreamExt};
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_statement(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        let stmt = match self.peek_word() {
            Some("let") => self.parse_let(),
            Some("var") => self.parse_var(),
            Some("set") => self.parse_set(),
            Some("fun") => self.parse_fun(remaining_lines),
            Some("ret") => self.parse_ret(),
            Some("require") => self.parse_require(),
            Some("import") => self.parse_import(),
            Some("if") => self.parse_if(remaining_lines),
            Some("loop") => self.parse_loop(remaining_lines),
            Some("break") => self.parse_break(),
            Some("continue") => self.parse_continue(),
            Some("debuglog") => self.parse_debuglog(),
            _ => {
                let (text, span) = self.peek_text_span();
                self.emit_stmt_error(
                    text,
                    span,
                    "unexpected statement",
                    "P001",
                    "expected 'let', 'var', 'set', 'fun', 'ret', 'require', 'import', 'if', 'loop', 'break', 'continue', or 'debuglog'"
                )
            }
        };

        // Check for unconsumed tokens on this line.
        self.error_if_not_exhausted();

        stmt
    }

    /// Parse a statement from a line of tokens.
    ///
    /// Creates a sub-parser for the line and parses the statement.
    pub(super) fn parse_line_statement(
        &mut self,
        line: Vec<TreeToken<'db>>,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        let line_tokens: Vec<_> = line.into_iter().filter_map(|t| t.without_space(self.db)).collect();
        let mut sub = Parser::new(self.db, line_tokens);
        let stmt = sub.parse_statement(remaining_lines);
        self.had_error |= sub.had_error;
        stmt
    }

    fn parse_let(&mut self) -> ast::Statement<'db> {
        self.eat_word("let");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected name after 'let'",
                    "P007",
                    "expected name",
                );
            }
        };

        // Check for type hint: `: type`
        let type_hint = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint_and_heap())
        } else {
            None
        };

        // Need `=` sigil. If missing, emit error and return parse error.
        if !self.eat_sigil(Sigil::Equals) {
            let (text, span) = self.peek_text_span();
            return self.emit_stmt_error(
                text,
                span,
                "expected '=' after let binding",
                "D023",
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Let(ast::StmtLet::new(
            self.db,
            name,
            type_hint,
            value,
        ))
    }

    fn parse_var(&mut self) -> ast::Statement<'db> {
        self.eat_word("var");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected name after 'var'",
                    "P008",
                    "expected name",
                );
            }
        };

        // Check for type hint: `: type`
        let type_hint = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint_and_heap())
        } else {
            None
        };

        // Need `=` sigil.
        if !self.eat_sigil(Sigil::Equals) {
            let (text, span) = self.peek_text_span();
            return self.emit_stmt_error(
                text,
                span,
                "expected '=' after var binding",
                "D024",
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Var(ast::StmtVar::new(
            self.db,
            name,
            type_hint,
            value,
        ))
    }

    fn parse_set(&mut self) -> ast::Statement<'db> {
        self.eat_word("set");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected name after 'set'",
                    "P009",
                    "expected name",
                );
            }
        };

        // Need `=` sigil.
        if !self.eat_sigil(Sigil::Equals) {
            let (text, span) = self.peek_text_span();
            return self.emit_stmt_error(
                text,
                span,
                "expected '=' after set target",
                "D025",
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Set(ast::StmtSet::new(
            self.db,
            name,
            value,
        ))
    }

    fn parse_fun(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word("fun");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected function name after 'fun'",
                    "P010",
                    "expected function name",
                );
            }
        };

        // Parse parameters in parentheses.
        let params = match self.next() {
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                self.parse_fun_params(iter)
            }
            _ => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected parameter list",
                    "P002",
                    "expected '(' to start parameter list"
                );
            }
        };

        // Check for return type: `: type`
        let return_type = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint_and_heap())
        } else {
            None
        };

        // Parse body until we hit "end fun".
        let mut body = vec![];
        let mut found_end_fun = false;
        while let Some((_, line)) = remaining_lines.peek() {
            if line.len() >= 2 {
                if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                    if let (Some("end"), Some("fun")) = (t1.word_str(self.db), t2.word_str(self.db)) {
                        remaining_lines.next(); // consume "end fun" line
                        found_end_fun = true;
                        break;
                    }
                }
            }

            let (_, line) = remaining_lines.next().X();
            if !line.is_empty() {
                // Parse statement recursively to handle if/ret/etc in function body.
                let stmt = self.parse_line_statement(line, remaining_lines);
                body.push(stmt);
            }
        }

        if !found_end_fun {
            let text = self.source_text();
            return self.emit_stmt_error(
                text,
                0..0,
                "unterminated function body",
                "P010",
                "expected 'end fun' before end of input"
            );
        }

        ast::Statement::Fun(ast::StmtFun::new(
            self.db,
            name,
            params,
            return_type,
            body,
        ))
    }

    fn parse_fun_params(&self, iter: BracerIter<'db>) -> Vec<ast::FunParam<'db>> {
        let tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if tokens.is_empty() {
            return vec![];
        }

        let mut sub = Parser::new(self.db, tokens);
        let mut params = vec![];

        loop {
            // Check if we've reached the end (handles trailing comma case).
            if sub.peek().is_none() {
                break;
            }

            // Check for parameter mode keywords.
            let mode = match sub.peek_word() {
                Some("out") => {
                    sub.eat_word("out");
                    ast::ParamMode::Out
                }
                Some("ref") => {
                    sub.eat_word("ref");
                    ast::ParamMode::Ref
                }
                Some("mut") => {
                    sub.eat_word("mut");
                    ast::ParamMode::Mut
                }
                _ => ast::ParamMode::In,
            };

            let name = match sub.eat_name() {
                Some(n) => n,
                None => {
                    sub.had_error = true;
                    let (text, span) = sub.peek_text_span();
                    DiagnosticBuilder::error(sub.db, "expected parameter name")
                        .code("P011")
                        .primary_label(text, span, "expected parameter name")
                        .emit_parse();
                    InternedText::new(sub.db, "<error>".S())
                }
            };

            // Need colon.
            if !sub.eat_sigil(Sigil::Colon) {
                sub.had_error = true;
                let (text, span) = sub.peek_text_span();
                DiagnosticBuilder::error(sub.db, "expected ':' after parameter name")
                    .code("P012")
                    .primary_label(text, span, "expected ':'")
                    .emit_parse();
            }

            let type_hint = sub.parse_type_hint_and_heap();

            params.push(ast::FunParam::new(self.db, name, mode, type_hint));

            // Check for comma (more params) or end.
            if sub.peek_sigil(Sigil::Comma) {
                sub.eat_sigil(Sigil::Comma);
            } else {
                break;
            }
        }

        sub.error_if_not_exhausted();
        params
    }

    fn parse_ret(&mut self) -> ast::Statement<'db> {
        self.eat_word("ret");

        // Bare `ret` for void functions has no expression.
        let value = if self.peek().is_some() {
            Some(self.parse_expr_full())
        } else {
            None
        };

        ast::Statement::Ret(ast::StmtRet::new(self.db, value))
    }

    fn parse_require(&mut self) -> ast::Statement<'db> {
        self.eat_word("require");

        match self.peek_word() {
            Some("module") => {
                self.eat_word("module");

                // Parse 3-part path: lib/pkg/module
                let import_space = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let (text, span) = self.peek_text_span();
                        return self.emit_stmt_error(
                            text,
                            span,
                            "expected import space name after 'require module'",
                            "P013",
                            "expected import space name",
                        );
                    }
                };

                // Need forward slash.
                if !self.peek_sigil(Sigil::SlashForward) {
                    let (text, span) = self.peek_text_span();
                    return self.emit_stmt_error(
                        text,
                        span,
                        "expected '/' after import space",
                        "P003",
                        "expected '/' after import space"
                    );
                }
                self.eat_sigil(Sigil::SlashForward);

                let package_alias = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let (text, span) = self.peek_text_span();
                        return self.emit_stmt_error(
                            text,
                            span,
                            "expected package name after '/'",
                            "P014",
                            "expected package name",
                        );
                    }
                };

                // Need forward slash.
                if !self.peek_sigil(Sigil::SlashForward) {
                    let (text, span) = self.peek_text_span();
                    return self.emit_stmt_error(
                        text,
                        span,
                        "expected '/' after package alias",
                        "P004",
                        "expected '/' after package alias"
                    );
                }
                self.eat_sigil(Sigil::SlashForward);

                let module_alias = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let (text, span) = self.peek_text_span();
                        return self.emit_stmt_error(
                            text,
                            span,
                            "expected module name after '/'",
                            "P015",
                            "expected module name",
                        );
                    }
                };

                ast::Statement::Require(ast::StmtRequire::Module(
                    ast::StmtRequireModule::new(
                        self.db,
                        import_space,
                        package_alias,
                        module_alias,
                    )
                ))
            }
            Some("data") => {
                self.eat_word("data");

                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let (text, span) = self.peek_text_span();
                        return self.emit_stmt_error(
                            text,
                            span,
                            "expected data name after 'require data'",
                            "P016",
                            "expected data name",
                        );
                    }
                };

                // Optional type hint: `: type`
                let type_hint = if self.peek_sigil(Sigil::Colon) {
                    self.eat_sigil(Sigil::Colon);
                    Some(self.parse_type_hint_and_heap())
                } else {
                    None
                };

                ast::Statement::Require(ast::StmtRequire::Data(
                    ast::StmtRequireData::new(
                        self.db,
                        name,
                        type_hint,
                    )
                ))
            }
            _ => {
                let (text, span) = self.peek_text_span();
                self.emit_stmt_error(
                    text,
                    span,
                    "expected 'module' or 'data' after 'require'",
                    "P005",
                    "expected 'module' or 'data'"
                )
            }
        }
    }

    fn parse_import(&mut self) -> ast::Statement<'db> {
        self.eat_word("import");

        // Parse module name.
        let module_name = match self.eat_name() {
            Some(n) => n,
            None => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected module name after 'import'",
                    "P017",
                    "expected module name",
                );
            }
        };

        // Need dot sigil.
        if !self.peek_sigil(Sigil::Dot) {
            let (text, span) = self.peek_text_span();
            return self.emit_stmt_error(
                text,
                span,
                "expected '.' after module name",
                "P006",
                "expected '.' after module name"
            );
        }
        self.eat_sigil(Sigil::Dot);

        // Parse item name.
        let item_name = match self.eat_name() {
            Some(n) => n,
            None => {
                let (text, span) = self.peek_text_span();
                return self.emit_stmt_error(
                    text,
                    span,
                    "expected item name after '.'",
                    "P018",
                    "expected item name",
                );
            }
        };

        ast::Statement::Import(
            ast::StmtImport::new(
                self.db,
                module_name,
                item_name,
            )
        )
    }

    fn parse_if(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word("if");

        // Parse condition expression.
        let condition = self.parse_expr_full();

        // Parse optional then binding: |identifier|
        let then_binding = if self.peek_sigil(Sigil::Pipe) {
            self.eat_sigil(Sigil::Pipe);
            let binding = match self.eat_name() {
                Some(n) => n,
                None => {
                    self.had_error = true;
                    let (text, span) = self.peek_text_span();
                    DiagnosticBuilder::error(self.db, "expected binding name after '|'")
                        .code("P019")
                        .primary_label(text, span, "expected binding name")
                        .emit_parse();
                    InternedText::new(self.db, "<error>".S())
                }
            };
            if !self.eat_sigil(Sigil::Pipe) {
                self.had_error = true;
                let (text, span) = self.peek_text_span();
                DiagnosticBuilder::error(self.db, "expected '|' after binding name")
                    .code("P020")
                    .primary_label(text, span, "expected '|'")
                    .emit_parse();
            }
            Some(binding)
        } else {
            None
        };

        // Parse then body until we hit "else" or "end if".
        let mut then_body = vec![];
        let mut found_else = false;

        while let Some((_, line)) = remaining_lines.peek() {
            if line.len() >= 2 {
                if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                    if let (Some("end"), Some("if")) = (t1.word_str(self.db), t2.word_str(self.db)) {
                        remaining_lines.next(); // consume "end if" line
                        break;
                    }
                }
            }

            if line.len() >= 1 {
                if let Some(TreeToken::Token(t1)) = line.get(0) {
                    if let Some("else") = t1.word_str(self.db) {
                        found_else = true;
                        break;
                    }
                }
            }

            let (_, line) = remaining_lines.next().X();
            if !line.is_empty() {
                let stmt = self.parse_line_statement(line, remaining_lines);
                then_body.push(stmt);
            }
        }

        // Parse else binding and body if we found "else".
        let (else_binding, else_body) = if found_else {
            // Consume the "else" line and parse any binding.
            let (_, else_line) = remaining_lines.next().X();
            let else_tokens: Vec<_> = else_line.into_iter().filter_map(|t| t.without_space(self.db)).collect();
            let mut else_sub = Parser::new(self.db, else_tokens);
            else_sub.eat_word("else");

            // Parse optional else binding: |identifier|
            let else_binding = if else_sub.peek_sigil(Sigil::Pipe) {
                else_sub.eat_sigil(Sigil::Pipe);
                let binding = match else_sub.eat_name() {
                    Some(n) => n,
                    None => {
                        else_sub.had_error = true;
                        let (text, span) = else_sub.peek_text_span();
                        DiagnosticBuilder::error(else_sub.db, "expected binding name after '|'")
                            .code("P019")
                            .primary_label(text, span, "expected binding name")
                            .emit_parse();
                        InternedText::new(else_sub.db, "<error>".S())
                    }
                };
                if !else_sub.eat_sigil(Sigil::Pipe) {
                    else_sub.had_error = true;
                    let (text, span) = else_sub.peek_text_span();
                    DiagnosticBuilder::error(else_sub.db, "expected '|' after binding name")
                        .code("P020")
                        .primary_label(text, span, "expected '|'")
                        .emit_parse();
                }
                Some(binding)
            } else {
                None
            };
            else_sub.error_if_not_exhausted();
            self.had_error |= else_sub.had_error;

            let mut body = vec![];

            while let Some((_, line)) = remaining_lines.peek() {
                if line.len() >= 2 {
                    if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                        if let (Some("end"), Some("if")) = (t1.word_str(self.db), t2.word_str(self.db)) {
                            remaining_lines.next(); // consume "end if" line
                            break;
                        }
                    }
                }

                let (_, line) = remaining_lines.next().X();
                if !line.is_empty() {
                    let stmt = self.parse_line_statement(line, remaining_lines);
                    body.push(stmt);
                }
            }

            (else_binding, Some(body))
        } else {
            (None, None)
        };

        ast::Statement::If(ast::StmtIf::new(
            self.db,
            condition,
            then_binding,
            then_body,
            else_binding,
            else_body,
        ))
    }

    fn parse_loop(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word("loop");

        // Parse body until we hit "end loop".
        let mut body = vec![];
        while let Some((_, line)) = remaining_lines.peek() {
            if line.len() >= 2 {
                if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                    if let (Some("end"), Some("loop")) = (t1.word_str(self.db), t2.word_str(self.db)) {
                        remaining_lines.next(); // Consume "end loop" line.
                        break;
                    }
                }
            }

            let (_, line) = remaining_lines.next().X();
            if !line.is_empty() {
                let stmt = self.parse_line_statement(line, remaining_lines);
                body.push(stmt);
            }
        }

        ast::Statement::Loop(ast::StmtLoop::new(self.db, body))
    }

    fn parse_break(&mut self) -> ast::Statement<'db> {
        self.eat_word("break");
        ast::Statement::Break(ast::StmtBreak::new(self.db, ()))
    }

    fn parse_continue(&mut self) -> ast::Statement<'db> {
        self.eat_word("continue");
        ast::Statement::Continue(ast::StmtContinue::new(self.db, ()))
    }

    fn parse_debuglog(&mut self) -> ast::Statement<'db> {
        self.eat_word("debuglog");
        let value = self.parse_expr_full();
        ast::Statement::DebugLog(ast::StmtDebugLog::new(self.db, value))
    }

    /// Delegate to datalit parser for type hints.
    pub(super) fn parse_type_hint_and_heap(&mut self) -> datalit::ast::TypeHintAndHeap<'db> {
        use bct::lexer::TokenKind;

        // Collect tokens for the type hint, stopping at delimiters that mark the end of a type.
        // We stop at `=` (assignment), `,` (parameter separator), or `/` (type/value separator).
        // These delimiters are NOT consumed, so they remain in the iterator.
        // fixme this is so brittle
        let mut collected = Vec::new();

        while let Some(token) = self.peek() {
            let should_stop = match token {
                TreeToken::Token(t) => {
                    matches!(
                        t.kind(self.db),
                        TokenKind::Sigil(Sigil::Equals)
                            | TokenKind::Sigil(Sigil::Comma)
                            | TokenKind::Sigil(Sigil::SlashForward)
                    )
                }
                _ => false,
            };

            if should_stop {
                break;
            }

            // Consume and collect the token.
            collected.push(token.clone());
            self.next();
        }

        let collected_len = collected.len();

        // Delegate to datalit parser.
        let (type_hint, consumed) = datalit::parser::parse_type_hint_and_heap_from_tokens(
            self.db,
            collected,
        );

        // Check for unconsumed tokens - this indicates a parse error in the type hint.
        // But only emit a new error if the type hint isn't already a ParseError
        // (to avoid duplicate errors).
        if consumed < collected_len {
            if !matches!(type_hint.type_hint(self.db), datalit::ast::TypeHint::ParseError(_)) {
                use bct::text::InternedText;
                use datalove_diagnostic::DiagnosticBuilder;

                // Get text/span info for the error.
                let text = self.source_text();
                let message = InternedText::new(self.db, "unexpected tokens in type hint".S());

                DiagnosticBuilder::error(self.db, "unexpected tokens in type hint")
                    .code("D021")
                    .primary_label(text, 0..1, "unexpected tokens")
                    .emit_parse();

                let error = datalit::ast::TypeHintParseError::new(
                    self.db,
                    text,
                    0..1, // placeholder span
                    message,
                );
                return datalit::ast::TypeHintAndHeap::new(
                    self.db,
                    datalit::ast::Heap::Omitted,
                    datalit::ast::TypeHint::ParseError(error),
                );
            }
        }

        type_hint
    }
}
