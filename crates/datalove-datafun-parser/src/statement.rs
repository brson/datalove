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
use datalove_datalit::parser_util::NameKind;

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
            Some("call") => self.parse_call(),
            _ => {
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
                    "unexpected statement",
                    "P001",
                    "expected 'let', 'var', 'const', 'set', 'call', 'fun', 'ret', 'require', 'import', 'if', 'match', 'loop', 'break', 'continue', 'debuglog', 'type', or 'native'"
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
        let mut sub = self.new_sub(line);
        let stmt = sub.parse_statement(remaining_lines);
        self.merge_identity_from(&mut sub);
        stmt
    }

    /// Consume an `end <keyword>` line, complaining about anything after it.
    ///
    /// A block's terminator is a whole line, so a word left over on it was
    /// meant to do something and does not. Dropping it is how `end fun please`
    /// came to mean `end fun`.
    fn eat_end_line(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
        keyword: &str,
    ) {
        let (_, line) = remaining_lines.next().X();
        let Some(extra) = line.get(2) else {
            return;
        };
        self.had_error = true;
        DiagnosticBuilder::error(self.db, &fmt!("unexpected token after `end {keyword}`"))
            .code("P033")
            .primary_label(self.extract_text_span(extra), "`end` takes the keyword and nothing else")
            .emit_parse();
    }

    /// Report a block that ran out of input before its `end` line.
    fn unterminated_block(&mut self, keyword: &str) -> ast::Statement<'db> {
        let end = self.last_byte_end();
        self.emit_stmt_error(
            TextSpan::new(self.source_text(), end..end),
            &fmt!("unterminated {keyword} body"),
            "P034",
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
                    .code("P070")
                    .primary_label(paren_ts, "write `(a,)` for a one-element tuple, or drop the parentheses")
                    .emit_parse();
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
                let fresh_field = seen_fields.take(p.db, field, field_ts.clone(), "field");
                if !fresh_field {
                    p.had_error = true;
                }
                let binding = if p.eat_sigil(Sigil::Equals) {
                    p.parse_pattern_name(&mut seen)
                } else {
                    // A field named twice has been reported once already.
                    if fresh_field {
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
        ts: TextSpan<'db>,
        seen: &mut datalit::parser_util::SeenNames<'db>,
    ) {
        if !seen.take(self.db, name, ts, "binding") {
            self.had_error = true;
        }
    }

    /// Report a pattern missing a name, and stand in for it.
    fn pattern_name_error(&mut self) -> InternedText<'db> {
        self.had_error = true;
        let ts = self.error_span();
        DiagnosticBuilder::error(self.db, "expected a name in this pattern")
            .code("P069")
            .primary_label(ts, "expected a name")
            .emit_parse();
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
            // No initializer - require a plain name and a type hint.
            if binding.as_name().is_none() {
                let ts = self.peek_text_span();
                return self.emit_stmt_error(ts,
                    "a destructuring var needs a value to take apart",
                    "P068",
                    "expected '='"
                );
            }
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
                    "P009",
                    "expected name",
                );
            }
        };

        // Parse optional field projections and index steps.
        let steps = self.parse_place_steps();
        let target = ast::Place { root: name, steps };
        let local_index = self.record_set_span(target_ts.with_end(self.last_byte_end()));

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

        let name = match self.eat_declared_name(NameKind::Function) {
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
        let (type_params, type_bounds) = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::AngleOpen, .. }) => {
                match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => self.parse_type_params(*inner),
                    _ => unreachable!("peeked an angle branch"),
                }
            }
            _ => (Vec::new(), Vec::new()),
        };

        // Parse parameters in parentheses.
        let params = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span());
                self.parse_fun_params(*inner, Some((open_span, "in this parameter list")))
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

        // Bounds, if the signature ends with `with { T is float, }`.
        let type_bounds = self.parse_with_clause(&type_params, type_bounds);

        // Enter function context for expression identity tracking.
        self.enter_function(name);

        // Parse body until we hit "end fun".
        let mut body = vec![];
        let mut found_end_fun = false;
        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "fun") {
                self.eat_end_line(remaining_lines, "fun");
                found_end_fun = true;
                break;
            }

            let (_, line) = remaining_lines.next().X();
            // Parse statement recursively to handle if/ret/etc in function body.
            let stmt = self.parse_line_statement(line, remaining_lines);
            body.push(stmt);
        }

        if !found_end_fun {
            self.exit_function();
            return self.unterminated_block("fun");
        }

        // Exit function context.
        self.exit_function();

        ast::Statement::Fun(ast::StmtFun::new(
            self.db,
            self.module_id(),
            name,
            ast::FunSignature { type_params, type_bounds, params, return_type },
            body,
            local_index,
        ))
    }

    /// Parse the type parameter names inside `<...>` on a function signature.
    ///
    /// The bounds come separately, from the `with` clause at the end of the
    /// signature, so this gives an empty bound for each name and the clause
    /// fills them in.
    fn parse_type_params(
        &mut self,
        inner: BracerIter<'db>,
    ) -> (Vec<InternedText<'db>>, Vec<Option<ast::TypeBound>>) {
        let mut sub = Parser::from_branch_with_context(
            self.db, inner, self.source_text(), None, self.module_id());
        let names = sub.parse_comma_separated(|p| p.eat_declared_name(NameKind::Type));
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        let names: Vec<_> = names.into_iter().flatten().collect();
        let bounds = vec![None; names.len()];
        (names, bounds)
    }

    /// Parse `with { T is float, }` after a signature, if it is there.
    ///
    /// A bound says which types its parameter may be, and in exchange the body
    /// may do what all of them have in common. Written apart from the name so
    /// that a signature reads as a signature and the constraints sit together
    /// underneath it.
    fn parse_with_clause(
        &mut self,
        type_params: &[InternedText<'db>],
        mut bounds: Vec<Option<ast::TypeBound>>,
    ) -> Vec<Option<ast::TypeBound>> {
        if self.peek_word() != Some("with") {
            return bounds;
        }
        self.eat_word("with");
        let Some(inner) = self.eat_branch(Sigil::BraceOpen) else {
            self.had_error = true;
            DiagnosticBuilder::error(self.db, "expected `{` after `with`")
                .code("P061")
                .primary_label(self.peek_text_span(), "the bounds go in braces")
                .emit_parse();
            return bounds;
        };

        let mut sub = Parser::from_branch_with_context(
            self.db, inner, self.source_text(), None, self.module_id());
        let clauses = sub.parse_comma_separated(|p| p.parse_one_bound());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;

        for (name, bound, ts) in clauses.into_iter().flatten() {
            match type_params.iter().position(|p| *p == name) {
                Some(i) => bounds[i] = Some(bound),
                None => {
                    self.had_error = true;
                    DiagnosticBuilder::error(self.db, &format!(
                        "`{}` is not a type parameter of this function",
                        name.text(self.db)))
                        .code("P062")
                        .primary_label(ts, "bounded here but never declared")
                        .emit_parse();
                }
            }
        }
        bounds
    }

    /// One line of a `with` clause: `T is float`.
    fn parse_one_bound(
        &mut self,
    ) -> Option<(InternedText<'db>, ast::TypeBound, TextSpan<'db>)> {
        let name_span = self.peek_text_span();
        let name = self.eat_name()?;
        if !self.eat_word("is") {
            self.had_error = true;
            DiagnosticBuilder::error(self.db, "expected `is` after the type parameter")
                .code("P063")
                .primary_label(self.peek_text_span(), "a bound reads `T is float`")
                .emit_parse();
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
                    .code("P060")
                    .primary_label(ts, "not a bound this language has")
                    .emit_parse();
                None
            }
        }
    }

    fn parse_fun_params(
        &mut self,
        iter: BracerIter<'db>,
        context: Option<(TextSpan<'db>, &'static str)>,
    ) -> Vec<ast::FunParam<'db>> {
        let mut sub = Parser::from_branch_with_context(self.db, iter, self.source_text(), context, self.module_id());
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
                .code("P013")
                .primary_label(ts, "invalid combination")
                .emit_parse();
        }

        let name = match self.eat_declared_name(NameKind::Parameter) {
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

                let module_alias = match self.eat_declared_name(NameKind::Module) {
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

                let name = match self.eat_declared_name(NameKind::Module) {
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
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word("if");
        match self.parse_if_arm(remaining_lines) {
            Some(stmt) => {
                self.eat_end_line(remaining_lines, "if");
                ast::Statement::If(stmt)
            }
            None => self.unterminated_block("if"),
        }
    }

    /// Parse an `if` after its keyword, stopping before the `end if` line.
    ///
    /// An `else if` line is parsed as a nested arm that becomes the whole
    /// else body, so a chain shares the single `end if` of its first `if`.
    /// Returns `None` if the input ran out before `end if`.
    fn parse_if_arm(
        &mut self,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> Option<ast::StmtIf<'db>> {
        let condition = self.parse_expr_full();
        let then_binding = self.parse_pipe_binding();

        let mut then_body = vec![];
        let mut found_else = false;

        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "if") {
                break;
            }

            if let Some(TreeToken::Token(t1)) = line.get(0) {
                if let Some("else") = t1.word_str(self.db) {
                    found_else = true;
                    break;
                }
            }

            let (_, line) = remaining_lines.next().X();
            let stmt = self.parse_line_statement(line, remaining_lines);
            then_body.push(stmt);
        }

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
                        .code("P066")
                        .primary_label(ts, "nest this `if` inside the else body instead")
                        .emit_parse();
                }
                else_sub.eat_word("if");
                let nested = else_sub.parse_if_arm(remaining_lines);
                else_sub.error_if_not_exhausted();
                self.merge_identity_from(&mut else_sub);
                (else_binding, Some(vec![ast::Statement::If(nested?)]))
            } else {
                else_sub.error_if_not_exhausted();
                self.merge_identity_from(&mut else_sub);

                let mut body = vec![];
                while let Some((_, line)) = remaining_lines.peek() {
                    if self.line_is_end_keyword(line, "if") {
                        break;
                    }

                    let (_, line) = remaining_lines.next().X();
                    let stmt = self.parse_line_statement(line, remaining_lines);
                    body.push(stmt);
                }
                (else_binding, Some(body))
            }
        } else {
            (None, None)
        };

        remaining_lines.peek()?;

        Some(ast::StmtIf {
            condition,
            then_binding,
            then_body,
            else_binding,
            else_body,
        })
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
        let mut found_end_loop = false;
        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "loop") {
                self.eat_end_line(remaining_lines, "loop");
                found_end_loop = true;
                break;
            }

            let (_, line) = remaining_lines.next().X();
            let stmt = self.parse_line_statement(line, remaining_lines);
            body.push(stmt);
        }

        if !found_end_loop {
            return self.unterminated_block("loop");
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

    /// Parse a `call` statement, whose expression must be a function call.
    fn parse_call(&mut self) -> ast::Statement<'db> {
        self.eat_word("call");
        let expr = self.parse_expr_full();
        // Verify the expression is a function call.
        match expr.expr(self.db) {
            ast::ExprFunKind::FunctionCall(_) => {
                ast::Statement::ExprStatement(ast::StmtExprStatement { expr })
            }
            _ => {
                let ts = self.peek_text_span();
                self.emit_stmt_error(ts,
                    "`call` takes a function call",
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

        let name = match self.eat_declared_name(NameKind::Type) {
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
        // Recorded in the same table a `fun` uses, so a diagnostic about the
        // declaration has somewhere to point.
        let native_span = self.peek_text_span();
        let local_index = self.record_fun_span(native_span);
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

        let name = match self.eat_declared_name(NameKind::Function) {
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
        let (type_params, type_bounds) = match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::AngleOpen, .. }) => {
                match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => self.parse_type_params(*inner),
                    _ => unreachable!("peeked an angle branch"),
                }
            }
            _ => (Vec::new(), Vec::new()),
        };

        // Parse parameters in parentheses.
        let params = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span());
                self.parse_fun_params(*inner, Some((open_span, "in this parameter list")))
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

        let type_bounds = self.parse_with_clause(&type_params, type_bounds);

        ast::Statement::NativeFun(ast::StmtNativeFun {
            name,
            type_params,
            type_bounds,
            params,
            return_type,
            local_index,
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
        let mut found_end_match = false;

        while let Some((_, line)) = remaining_lines.peek() {
            if self.line_is_end_keyword(line, "match") {
                self.eat_end_line(remaining_lines, "match");
                found_end_match = true;
                break;
            }

            // Check for "case" line.
            if let Some(TreeToken::Token(t1)) = line.get(0) {
                if let Some("case") = t1.word_str(self.db) {
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
                            let binding = match case_sub.eat_declared_name(NameKind::Value) {
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
                            let default_span = case_sub.peek_text_span();
                            case_sub.next(); // consume "default"
                            case_sub.error_if_not_exhausted();
                            self.had_error |= case_sub.had_error;

                            // Parsed either way, so that a second default's body
                            // is checked like any other rather than skipped.
                            let body = self.parse_match_arm_body(remaining_lines);
                            if default_body.is_some() {
                                self.had_error = true;
                                DiagnosticBuilder::error(self.db, "duplicate `case default` in match")
                                    .code("P049")
                                    .primary_label(default_span, "a match takes only one default arm")
                                    .emit_parse();
                            } else {
                                default_body = Some(body);
                            }
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

            // A line inside a match that is not a `case` belongs to no arm.
            let (_, line) = remaining_lines.next().X();
            self.had_error = true;
            let ts = self.extract_text_span(&line[0]);
            DiagnosticBuilder::error(self.db, "this line is in no case of the match")
                .code("P048")
                .primary_label(ts, "every statement here belongs to a `case`")
                .emit_parse();
        }

        if !found_end_match {
            return self.unterminated_block("match");
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
            let stmt = self.parse_line_statement(line, remaining_lines);
            body.push(stmt);
        }
        body
    }

    /// Read a type hint, which datalit knows the shape of and this does not.
    ///
    /// It reads straight out of this parser, so it consumes the type and
    /// nothing else: the `=` of a `let`, the `,` between two parameters and
    /// the `with` opening a bounds clause are all still here afterwards, for
    /// whichever caller wanted one to say so itself.
    pub(super) fn parse_type_hint(&mut self) -> datalit::ast::TypeHint<'db> {
        datalit::parser::parse_type_hint(self)
    }
}
