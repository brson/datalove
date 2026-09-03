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
            Some("const") => self.parse_const(),
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
            Some("type") => self.parse_type_alias(),
            Some("native") => self.parse_native_fun(),
            Some("match") => self.parse_match(remaining_lines),
            _ => {
                // Try function call statement: parse as expression, then
                // verify it's a function call. The expression parser handles
                // keyword priority (some/ok/er/etc.) so no blocklist needed.
                if self.peek_word().is_some() && self.peek_next_is_paren() {
                    self.parse_expr_statement()
                } else {
                    let ts = self.peek_text_span();
                    self.emit_stmt_error(ts,
                        "unexpected statement",
                        "P001",
                        "expected 'let', 'var', 'const', 'set', 'fun', 'ret', 'require', 'import', 'if', 'loop', 'break', 'continue', 'debuglog', 'type', or 'native'"
                    )
                }
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
        let line_tokens: Vec<_> = line.into_iter().filter_map(|t| t.without_space()).collect();
        let mut sub = self.new_sub(line_tokens);
        let stmt = sub.parse_statement(remaining_lines);
        self.merge_identity_from(&mut sub);
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
            Some(self.parse_type_hint())
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
            Some(self.parse_type_hint())
        } else {
            None
        };

        // Check for `=` sigil. If present, parse initializer.
        // If absent, this is an uninitialized var (requires type hint).
        let value = if self.eat_sigil(Sigil::Equals) {
            Some(self.parse_expr_full())
        } else {
            // No initializer - require type hint.
            if type_hint.is_none() {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "uninitialized var requires type hint",
                    "P026",
                    "add ': Type' or '= value'"
                );
            }
            None
        };

        ast::Statement::Var(ast::StmtVar {
            name,
            type_hint,
            value,
        })
    }

    fn parse_const(&mut self) -> ast::Statement<'db> {
        self.eat_word("const");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'const'",
                    "P024",
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
                "P025",
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

        // Parse optional field projections and index steps.
        let steps = self.parse_place_steps();
        let target = ast::Place { root: name, steps };

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

    /// Parse place steps for set targets (e.g., `.x.0.y`, `[i]?`).
    fn parse_place_steps(&mut self) -> Vec<ast::PlaceStep<'db>> {
        let mut steps = Vec::new();
        loop {
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

        // Type parameters, if the name is followed by `<...>`. Angle brackets
        // are bracers, so this arrives as a single branch.
        let type_params = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::AngleOpen, .. }) => {
                match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => self.parse_type_params(inner),
                    _ => unreachable!("peeked an angle branch"),
                }
            }
            _ => Vec::new(),
        };

        // Parse parameters in parentheses.
        let params = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span());
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
            Some(self.parse_type_hint())
        } else {
            None
        };

        // Enter function context for expression identity tracking.
        self.enter_function(name);

        // Parse body until we hit "end fun".
        let mut body = vec![];
        let mut found_end_fun = false;
        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "fun") {
                remaining_lines.next(); // consume "end fun" line
                found_end_fun = true;
                break;
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
            type_params,
            params,
            return_type,
            body,
            local_index,
        ))
    }

    /// Parse the names inside `<...>` on a function signature.
    fn parse_type_params(&mut self, inner: BracerIter<'db>) -> Vec<InternedText<'db>> {
        let mut sub = Parser::from_branch_with_context(
            self.db, inner, self.source_text(), None, self.module_id());
        let names = sub.parse_comma_separated(|p| p.eat_name());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        names.into_iter().flatten().collect()
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

        // Validate: const cannot combine with out or mut
        if is_comptime && matches!(mode, ast::ParamMode::Out | ast::ParamMode::Mut) {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "const parameter cannot be 'out' or 'mut'")
                .code("P013")
                .primary_label(ts, "invalid combination")
                .emit_parse();
        }

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
                    Some(self.parse_type_hint())
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
            Some("rider") => {
                self.eat_word("rider");

                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        return self.emit_stmt_error(ts,
                            "expected rider name after 'require rider'",
                            "P027",
                            "expected rider name",
                        );
                    }
                };

                ast::Statement::Require(ast::StmtRequire::Rider(
                    ast::StmtRequireRider { name }
                ))
            }
            _ => {
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
                    "expected 'module', 'data', or 'rider' after 'require'",
                    "P005",
                    "expected 'module', 'data', or 'rider'"
                )
            }
        }
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

        let local_index = self.record_import_span(ts);
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
            if self.line_is_end_keyword(line, "if") {
                remaining_lines.next(); // consume "end if" line
                break;
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
            let else_tokens: Vec<_> = else_line.into_iter().filter_map(|t| t.without_space()).collect();
            let mut else_sub = self.new_sub(else_tokens);
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
                if self.line_is_end_keyword(line, "if") {
                    remaining_lines.next(); // consume "end if" line
                    break;
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
            if self.line_is_end_keyword(line, "loop") {
                remaining_lines.next();
                break;
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

    /// Check if the next token after the current one is an open paren.
    fn peek_next_is_paren(&self) -> bool {
        matches!(self.peek_next(), Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }))
    }

    /// Parse an expression and verify it's a function call, then wrap as statement.
    ///
    /// The expression parser handles keyword priority (some/ok/er/etc. are
    /// parsed as their own expression kinds, not as function calls), so no
    /// keyword blocklist is needed here.
    fn parse_expr_statement(&mut self) -> ast::Statement<'db> {
        let expr = self.parse_expr_full();
        // Verify the expression is a function call.
        match expr.expr(self.db) {
            ast::ExprFunKind::FunctionCall(_) => {
                ast::Statement::ExprStatement(ast::StmtExprStatement { expr })
            }
            _ => {
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
                    "only function calls can be used as statements",
                    "P031",
                    "not a function call"
                )
            }
        }
    }

    fn parse_type_alias(&mut self) -> ast::Statement<'db> {
        let ts = self.peek_text_span();
        self.eat_word("type");
        let local_index = self.record_type_alias_span(ts);

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected name after 'type'",
                    "P022",
                    "expected name",
                );
            }
        };

        // Need `:` sigil.
        if !self.eat_sigil(Sigil::Colon) {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected ':' after type alias name",
                "P023",
                "expected ':'"
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

    fn parse_native_fun(&mut self) -> ast::Statement<'db> {
        self.eat_word("native");

        if self.peek_word() != Some("fun") {
            let ts = self.peek_text_span();
            return self.emit_stmt_error(ts,
                "expected 'fun' after 'native'",
                "P028",
                "expected 'fun'",
            );
        }
        self.eat_word("fun");

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "expected function name after 'native fun'",
                    "P029",
                    "expected function name",
                );
            }
        };

        // Type parameters, spelled as they are on a regular function.
        let type_params = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::AngleOpen, .. }) => {
                match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => self.parse_type_params(inner),
                    _ => unreachable!("peeked an angle branch"),
                }
            }
            _ => Vec::new(),
        };

        // Parse parameters in parentheses.
        let params = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span());
                self.parse_fun_params(inner, Some((open_span, "in this parameter list")))
            }
            _ => {
                let ts = self.error_span();
                return self.emit_stmt_error(ts,
                    "expected parameter list for native fun",
                    "P030",
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

        ast::Statement::NativeFun(ast::StmtNativeFun {
            name,
            type_params,
            params,
            return_type,
        })
    }

    fn parse_match(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word("match");

        // Parse input expression.
        let input = self.parse_expr_full();

        // Parse case arms until "end match".
        let mut cases = vec![];
        let mut default_body = None;

        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "match") {
                remaining_lines.next(); // consume "end match"
                break;
            }

            // Check for "case" line.
            if let Some(TreeToken::Token(t1)) = line.get(0) {
                if let Some("case") = t1.word_str(self.db) {
                    let (_, case_line) = remaining_lines.next().X();
                    let case_tokens: Vec<_> = case_line.into_iter().filter_map(|t| t.without_space()).collect();
                    let mut case_sub = self.new_sub(case_tokens);
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
                                        .code("P044")
                                        .primary_label(ts, "expected atom name")
                                        .emit_parse();
                                    InternedText::new(self.db, "<error>".S())
                                }
                            };
                            case_sub.error_if_not_exhausted();
                            self.had_error |= case_sub.had_error;

                            // Parse body until next case/default/end match.
                            let body = self.parse_match_arm_body(remaining_lines);
                            cases.push(ast::MatchCase {
                                kind: ast::MatchCaseKind::Atom { name },
                                body,
                            });
                        }
                        Some("term") => {
                            case_sub.next(); // consume "term"
                            let name = match case_sub.eat_name() {
                                Some(n) => n,
                                None => {
                                    self.had_error = true;
                                    let ts = case_sub.peek_text_span();
                                    DiagnosticBuilder::error(self.db, "expected name after 'case term'")
                                        .code("P045")
                                        .primary_label(ts, "expected term name")
                                        .emit_parse();
                                    InternedText::new(self.db, "<error>".S())
                                }
                            };
                            let binding = match case_sub.eat_name() {
                                Some(n) => n,
                                None => {
                                    self.had_error = true;
                                    let ts = case_sub.peek_text_span();
                                    DiagnosticBuilder::error(self.db, "expected binding name after term name")
                                        .code("P046")
                                        .primary_label(ts, "expected binding name")
                                        .emit_parse();
                                    InternedText::new(self.db, "<error>".S())
                                }
                            };
                            case_sub.error_if_not_exhausted();
                            self.had_error |= case_sub.had_error;

                            // Parse body until next case/default/end match.
                            let body = self.parse_match_arm_body(remaining_lines);
                            cases.push(ast::MatchCase {
                                kind: ast::MatchCaseKind::Term { name, binding },
                                body,
                            });
                        }
                        Some("default") => {
                            case_sub.next(); // consume "default"
                            case_sub.error_if_not_exhausted();
                            self.had_error |= case_sub.had_error;

                            let body = self.parse_match_arm_body(remaining_lines);
                            default_body = Some(body);
                        }
                        _ => {
                            self.had_error = true;
                            let ts = case_sub.peek_text_span();
                            DiagnosticBuilder::error(self.db, "expected 'atom', 'term', or 'default' after 'case'")
                                .code("P047")
                                .primary_label(ts, "expected 'atom', 'term', or 'default'")
                                .emit_parse();
                            self.had_error |= case_sub.had_error;
                            // Skip to next case/end match.
                            let body = self.parse_match_arm_body(remaining_lines);
                            let _ = body;
                        }
                    }
                    continue;
                }
            }

            // Unexpected line inside match - skip it.
            let (_, _line) = remaining_lines.next().X();
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
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> Vec<ast::Statement<'db>> {
        let mut body = vec![];
        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "match") {
                break;
            }
            // Check for next "case".
            if let Some(TreeToken::Token(t1)) = line.get(0) {
                if let Some("case") = t1.word_str(self.db) {
                    break;
                }
            }

            let (_, line) = remaining_lines.next().X();
            if !line.is_empty() {
                let stmt = self.parse_line_statement(line, remaining_lines);
                body.push(stmt);
            }
        }
        body
    }

    /// Delegate to datalit parser for type hints.
    pub(super) fn parse_type_hint(&mut self) -> datalit::ast::TypeHint<'db> {
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
                        t.kind,
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
        let (type_hint, consumed) = datalit::parser::parse_type_hint_from_tokens(
            self.db,
            collected,
            self.source_text(),
        );

        // Check for unconsumed tokens - this indicates a parse error in the type hint.
        // But only emit a new error if the type hint isn't already a ParseError
        // (to avoid duplicate errors).
        if consumed < collected_len {
            if !matches!(type_hint, datalit::ast::TypeHint::ParseError(_)) {
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
                return datalit::ast::TypeHint::ParseError(error);
            }
        }

        type_hint
    }
}
