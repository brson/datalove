//! Statement parsing.

use rmx::prelude::*;
use rmx::core::iter::Peekable;

use bct::{
    lexer::Sigil,
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
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
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
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
        let mut sub = Parser::new(self.db, line_tokens, self.source_text(), self.module_id());
        let stmt = sub.parse_statement(remaining_lines);
        self.had_error |= sub.had_error;
        self.merge_spans_from(&mut sub);
        stmt
    }

    fn parse_let(&mut self) -> ast::Statement<'db> {
        self.eat_word("let");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
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
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after let binding",
                "D023",
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Let(ast::StmtLet {
            name,
            type_hint,
            value,
        })
    }

    fn parse_var(&mut self) -> ast::Statement<'db> {
        self.eat_word("var");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
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
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after var binding",
                "D024",
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Var(ast::StmtVar {
            name,
            type_hint,
            value,
        })
    }

    fn parse_set(&mut self) -> ast::Statement<'db> {
        let ts = self.peek_text_span();
        self.eat_word("set");
        let local_index = self.record_set_span(ts);

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'set'",
                    "P009",
                    "expected name",
                );
            }
        };

        // Parse optional field projections: set a.x.y = value
        let target = self.parse_set_target_projections(ast::SetTarget::Name(name));

        // Need `=` sigil.
        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected '=' after set target",
                "D025",
                "expected '='"
            );
        }

        // Parse the value expression.
        let value = self.parse_expr_full();

        ast::Statement::Set(ast::StmtSet {
            target,
            value,
            local_index,
        })
    }

    /// Parse optional field projections for set targets (e.g., `.x.0.y`).
    fn parse_set_target_projections(&mut self, mut target: ast::SetTarget<'db>) -> ast::SetTarget<'db> {
        loop {
            if self.peek_sigil(Sigil::Dot) {
                self.next(); // consume .
                let field = self.parse_set_field_selector();
                target = ast::SetTarget::Proj(ast::SetTargetProj {
                    base: Box::new(target),
                    field,
                });
            } else {
                break;
            }
        }
        target
    }

    /// Parse a field selector for set target (name or index).
    fn parse_set_field_selector(&mut self) -> ast::FieldSelector<'db> {
        match self.peek_word() {
            Some(word) => {
                self.next(); // consume word
                // Check if all digits (tuple index).
                if word.chars().all(|c| c.is_ascii_digit()) && !word.is_empty() {
                    match word.parse::<u32>() {
                        Ok(idx) => ast::FieldSelector::Index(idx),
                        Err(_) => {
                            // Too large for u32, treat as name.
                            let name = InternedText::new(self.db, word.S());
                            ast::FieldSelector::Name(name)
                        }
                    }
                } else {
                    let name = InternedText::new(self.db, word.S());
                    ast::FieldSelector::Name(name)
                }
            }
            None => {
                // No valid field selector - create error name.
                let name = InternedText::new(self.db, "<error>".S());
                ast::FieldSelector::Name(name)
            }
        }
    }

    fn parse_fun(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        // Record function span starting at 'fun' keyword.
        let fun_span = self.peek_text_span();
        let local_index = self.record_fun_span(fun_span);
        self.eat_word("fun");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected function name after 'fun'",
                    "P010",
                    "expected function name",
                );
            }
        };

        // Parse parameters in parentheses.
        let params = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span(self.db));
                self.parse_fun_params(inner, Some((open_span, "in this parameter list")))
            }
            _ => {
                let ts = self.error_span();
                return self.emit_stmt_error(ts,
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

        // Enter function context for expression identity tracking.
        self.enter_function(name);

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
            self.exit_function();
            let ts = TextSpan::new(self.source_text(), 0..0);
            return self.emit_stmt_error(
                ts,
                "unterminated function body",
                "P010",
                "expected 'end fun' before end of input"
            );
        }

        // Exit function context.
        self.exit_function();

        ast::Statement::Fun(ast::StmtFun::new(
            self.db,
            self.module_id(),
            name,
            params,
            return_type,
            body,
            local_index,
        ))
    }

    fn parse_fun_params(
        &mut self,
        iter: BracerIter<'db>,
        context: Option<(TextSpan<'db>, &'static str)>,
    ) -> Vec<ast::FunParam<'db>> {
        let mut sub = Parser::from_branch_with_context(self.db, iter, self.source_text(), context, self.module_id());
        let params = sub.parse_comma_separated(|p| p.parse_fun_param());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        params
    }

    /// Parse a single function parameter: `[mode] name: type`.
    fn parse_fun_param(&mut self) -> ast::FunParam<'db> {
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

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                self.had_error = true;
                let ts = self.peek_text_span();
                DiagnosticBuilder::error(self.db, "expected parameter name")
                    .code("P011")
                    .primary_label(ts, "expected parameter name")
                    .emit_parse();
                InternedText::new(self.db, "<error>".S())
            }
        };

        // Need colon.
        if !self.eat_sigil(Sigil::Colon) {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "expected ':' after parameter name")
                .code("P012")
                .primary_label(ts, "expected ':'")
                .emit_parse();
        }

        let type_hint = self.parse_type_hint_and_heap();

        ast::FunParam { name, mode, type_hint }
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
        self.eat_word("require");

        match self.peek_word() {
            Some("module") => {
                self.eat_word("module");

                // Parse 3-part path: lib/pkg/module
                let import_space = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        return self.emit_stmt_error(ts,
                            "expected import space name after 'require module'",
                            "P013",
                            "expected import space name",
                        );
                    }
                };

                // Need forward slash.
                if !self.peek_sigil(Sigil::SlashForward) {
                    let ts = self.error_span();
                    return self.emit_stmt_error(ts,
                        "expected '/' after import space",
                        "P003",
                        "expected '/' after import space"
                    );
                }
                self.eat_sigil(Sigil::SlashForward);

                let package_alias = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.error_span();
                        return self.emit_stmt_error(ts,
                            "expected package name after '/'",
                            "P014",
                            "expected package name",
                        );
                    }
                };

                // Need forward slash.
                if !self.peek_sigil(Sigil::SlashForward) {
                    let ts = self.error_span();
                    return self.emit_stmt_error(ts,
                        "expected '/' after package alias",
                        "P004",
                        "expected '/' after package alias"
                    );
                }
                self.eat_sigil(Sigil::SlashForward);

                let module_alias = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.error_span();
                        return self.emit_stmt_error(ts,
                            "expected module name after '/'",
                            "P015",
                            "expected module name",
                        );
                    }
                };

                ast::Statement::Require(ast::StmtRequire::Module(
                    ast::StmtRequireModule {
                        import_space,
                        package_alias,
                        module_alias,
                    }
                ))
            }
            Some("data") => {
                self.eat_word("data");

                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        return self.emit_stmt_error(ts,
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
                    ast::StmtRequireData {
                        name,
                        type_hint,
                    }
                ))
            }
            _ => {
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
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
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected module name after 'import'",
                    "P017",
                    "expected module name",
                );
            }
        };

        // Need dot sigil.
        if !self.peek_sigil(Sigil::Dot) {
            let ts = self.error_span();
            return self.emit_stmt_error(ts,
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
                let ts = self.error_span();
                return self.emit_stmt_error(ts,
                    "expected item name after '.'",
                    "P018",
                    "expected item name",
                );
            }
        };

        ast::Statement::Import(
            ast::StmtImport {
                module_name,
                item_name,
            }
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
                    let ts = self.peek_text_span();
                    DiagnosticBuilder::error(self.db, "expected binding name after '|'")
                        .code("P019")
                        .primary_label(ts, "expected binding name")
                        .emit_parse();
                    InternedText::new(self.db, "<error>".S())
                }
            };
            if !self.eat_sigil(Sigil::Pipe) {
                self.had_error = true;
                // Use position after binding name, not peek position (which may be 0 at end of line).
                let pos = self.last_byte_end();
                let ts = TextSpan::new(self.source_text(), pos..pos);
                DiagnosticBuilder::error(self.db, "expected '|' after binding name")
                    .code("P020")
                    .primary_label(ts, "expected '|'")
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
            let mut else_sub = Parser::new(self.db, else_tokens, self.source_text(), self.module_id());
            else_sub.eat_word("else");

            // Parse optional else binding: |identifier|
            let else_binding = if else_sub.peek_sigil(Sigil::Pipe) {
                else_sub.eat_sigil(Sigil::Pipe);
                let binding = match else_sub.eat_name() {
                    Some(n) => n,
                    None => {
                        else_sub.had_error = true;
                        let ts = else_sub.peek_text_span();
                        DiagnosticBuilder::error(else_sub.db, "expected binding name after '|'")
                            .code("P019")
                            .primary_label(ts, "expected binding name")
                            .emit_parse();
                        InternedText::new(else_sub.db, "<error>".S())
                    }
                };
                if !else_sub.eat_sigil(Sigil::Pipe) {
                    else_sub.had_error = true;
                    // Use position after binding name, not peek position (which may be 0 at end of line).
                    let pos = else_sub.last_byte_end();
                    let ts = TextSpan::new(else_sub.source_text(), pos..pos);
                    DiagnosticBuilder::error(else_sub.db, "expected '|' after binding name")
                        .code("P020")
                        .primary_label(ts, "expected '|'")
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

        ast::Statement::If(ast::StmtIf {
            condition,
            then_binding,
            then_body,
            else_binding,
            else_body,
        })
    }

    fn parse_loop(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word("loop");

        // Check for optional `while condition` clause.
        let condition = if self.peek_word() == Some("while") {
            self.eat_word("while");
            Some(self.parse_expr_full())
        } else {
            None
        };

        // Parse body until we hit "end loop".
        let mut body = vec![];
        while let Some((_, line)) = remaining_lines.peek() {
            // Check for "end loop".
            if line.len() >= 2 {
                if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                    if let (Some("end"), Some("loop")) = (t1.word_str(self.db), t2.word_str(self.db)) {
                        // Consume "end loop" line.
                        remaining_lines.next();
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

        ast::Statement::Loop(ast::StmtLoop { condition, body })
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
            collected.push(token.C());
            self.next();
        }

        let collected_len = collected.len();

        // Delegate to datalit parser.
        let (type_hint, consumed) = datalit::parser::parse_type_hint_and_heap_from_tokens(
            self.db,
            collected,
            self.source_text(),
        );

        // Check for unconsumed tokens - this indicates a parse error in the type hint.
        // But only emit a new error if the type hint isn't already a ParseError
        // (to avoid duplicate errors).
        if consumed < collected_len {
            if !matches!(type_hint.type_hint(self.db), datalit::ast::TypeHint::ParseError(_)) {
                use bct::text::{InternedText, TextSpan};
                use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;

                // Get text/span info for the error.
                let text = self.source_text();
                let message = InternedText::new(self.db, "unexpected tokens in type hint".S());

                DiagnosticBuilder::error(self.db, "unexpected tokens in type hint")
                    .code("D021")
                    .primary_label(TextSpan::new(text, 0..1), "unexpected tokens")
                    .emit_parse();

                let error = datalit::ast::TypeHintParseError {
                    text,
                    span: (0..1).into(),
                    message,
                };
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
