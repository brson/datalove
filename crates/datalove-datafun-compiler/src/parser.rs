use rmx::prelude::*;

use rmx::core::iter::Peekable;
use bct::{
    input::Source,
    lexer::{
        Token,
        TokenKind,
        Sigil
    },
    bracer::{
        Bracer,
        BracerIter,
        TreeToken,
    },
    text::InternedText,
    source_map,
    lexer,
    bracer,
};
use salsa::Accumulator;

use crate::ast;
use crate::datalit;
use crate::script;
use datalove_diagnostic::DiagnosticBuilder;

/// Parse a specific unit from a Script.
/// Returns the parsed statements for that unit.
/// Salsa will memoize this per unit, so unchanged units don't need re-parsing.
#[salsa::tracked]
pub fn parse_script_unit<'db>(
    db: &'db dyn crate::Db,
    script: script::Script,
    unit_index: usize,
) -> ast::Script<'db> {
    let units = &script.units(db);
    let unit = units[unit_index];
    let source = unit.source(db);
    parse(db, source).script(db)
}

/// Parse a Source into a datafun script with span information.
#[salsa::tracked]
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ParseResult<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer(db, bracer)
}

/// Parse a Source as a single expression.
#[salsa::tracked]
pub fn parse_expr<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFun<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer_expr(db, bracer)
}

fn parse_bracer_expr<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
) -> ast::ExprFun<'db> {
    // Collect all tokens, filtering spaces.
    let tokens: Vec<TreeToken<'db>> = bracer.iter(db)
        .filter_map(|token| token.without_space(db))
        .collect();

    let mut parser = Parser {
        db,
        tokens,
        pos: 0,
        had_error: false,
    };

    let expr = parser.parse_expr_full();
    parser.error_if_not_exhausted();
    expr
}

fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
) -> ast::ParseResult<'db> {
    // Get line iterator - newlines inside balanced braces don't count as line breaks.
    // First split on newlines, then filter spaces from each line.
    let lines: Vec<Vec<TreeToken<'db>>> = bracer.iter(db)
        .batching(|iter| {
            let mut line = vec![];
            let mut found_newline = false;

            while let Some(token) = iter.next() {
                match token {
                    TreeToken::Token(t) if is_line_separator(db, t) => {
                        found_newline = true;
                        break;
                    }
                    _ => {
                        // Filter spaces here, after newline check
                        if let Some(t) = token.without_space(db) {
                            line.push(t);
                        }
                    }
                }
            }

            if !line.is_empty() || found_newline {
                Some(line)
            } else {
                None
            }
        })
        .collect();

    let statements = parse_statements(db, lines);
    let script = ast::Script::new(db, statements);
    ast::ParseResult::new(db, script)
}

/// Check if a token acts as a line separator.
/// Line separators are newlines or semicolons.
fn is_line_separator<'db>(db: &'db dyn crate::Db, token: Token<'db>) -> bool {
    match token.kind(db) {
        TokenKind::Whitespace => token.text(db).as_str(db).contains("\n"),
        TokenKind::Sigil(Sigil::Semicolon) => true,
        _ => false,
    }
}

/// Parse statements from lines, creating a Parser for each line.
fn parse_statements<'db>(
    db: &'db dyn crate::Db,
    lines: Vec<Vec<TreeToken<'db>>>,
) -> Vec<ast::Statement<'db>> {
    let mut statements = vec![];
    let mut line_iter = lines.into_iter().enumerate().peekable();

    while let Some((_line_num, line)) = line_iter.next() {
        if line.is_empty() {
            continue;
        }

        let mut parser = Parser {
            db,
            tokens: line,
            pos: 0,
            had_error: false,
        };
        let statement = parser.parse_statement(&mut line_iter);
        statements.push(statement);
    }

    statements
}

struct Parser<'db> {
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    pos: usize,
    had_error: bool,
}

impl<'db> Parser<'db> {
    /// Emit both a diagnostic and create a StmtParseError node in one call.
    fn emit_stmt_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Statement<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::Statement::ParseError(ast::StmtParseError::new(self.db, text, span, message_text))
    }

    /// Emit both a diagnostic and create an ExprFun with ParseError kind in one call.
    fn emit_expr_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprFun<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::ExprFun::new(
            self.db,
            ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(self.db, text, span, message_text))
        )
    }

    // Token navigation methods (Vec + pos pattern).

    fn peek(&self) -> Option<&TreeToken<'db>> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn peek_sigil(&self, sigil: Sigil) -> bool {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db), TokenKind::Sigil(s) if s == sigil)
            }
            Some(TreeToken::Branch(s, _)) => *s == sigil,
            None => false,
        }
    }

    fn eat_sigil(&mut self, sigil: Sigil) -> bool {
        if self.peek_sigil(sigil) {
            self.next();
            true
        } else {
            false
        }
    }

    fn need_sigil(&mut self, sigil: Sigil) {
        if !self.eat_sigil(sigil) {
            panic!("expected sigil {}", sigil.as_str());
        }
    }

    fn peek_word(&self) -> Option<&'db str> {
        match self.peek() {
            Some(TreeToken::Token(token)) => token.word_str(self.db),
            _ => None,
        }
    }

    fn eat_word(&mut self, word: &str) {
        match self.next() {
            Some(TreeToken::Token(token)) => {
                if token.word_str(self.db) != Some(word) {
                    panic!("expected word '{}'", word);
                }
            }
            _ => panic!("expected word '{}'", word),
        }
    }

    fn eat_name(&mut self) -> Option<InternedText<'db>> {
        if self.peek_word().is_some() {
            match self.next() {
                Some(TreeToken::Token(token)) => {
                    match token.word_str(self.db) {
                        Some(word) => Some(InternedText::new(self.db, word.S())),
                        None => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    fn need_name(&mut self) -> InternedText<'db> {
        match self.eat_name() {
            Some(name) => name,
            None => {
                match self.peek() {
                    Some(TreeToken::Token(token)) => {
                        let text = token.text(self.db).as_str(self.db);
                        panic!("expected name, got token: {}", text)
                    }
                    Some(TreeToken::Branch(..)) => panic!("expected name, got branch"),
                    None => panic!("expected name, got end of input"),
                }
            }
        }
    }

    fn peek_colon_type_hint(&self) -> bool {
        self.peek_sigil(Sigil::Colon)
    }

    /// Create a sub-parser for processing branch content.
    fn sub_parser(&self, tokens: Vec<TreeToken<'db>>) -> Parser<'db> {
        Parser {
            db: self.db,
            tokens,
            pos: 0,
            had_error: false,
        }
    }

    /// Get source Text for error reporting from the first token.
    fn source_text(&self) -> bct::text::Text<'db> {
        if let Some(token) = self.tokens.first() {
            match token {
                TreeToken::Token(tok) => tok.text(self.db).text(self.db),
                TreeToken::Branch(_, iter) => {
                    // Try to find a token inside the branch.
                    for inner in iter.clone() {
                        if let Some(TreeToken::Token(tok)) = inner.without_space(self.db) {
                            return tok.text(self.db).text(self.db);
                        }
                    }
                    bct::text::Text::new(self.db, String::new())
                }
            }
        } else {
            bct::text::Text::new(self.db, String::new())
        }
    }

    fn extract_text_span(&self, token: &TreeToken<'db>) -> (bct::text::Text<'db>, datalove_diagnostic::ByteSpan) {
        match token {
            TreeToken::Token(tok) => {
                let subtext = tok.text(self.db);
                (subtext.text(self.db), subtext.range(self.db))
            }
            TreeToken::Branch(_, _) => {
                (self.source_text(), 0..0)
            }
        }
    }

    fn peek_text_span(&self) -> (bct::text::Text<'db>, datalove_diagnostic::ByteSpan) {
        if let Some(token) = self.peek() {
            self.extract_text_span(token)
        } else {
            (self.source_text(), 0..0)
        }
    }

    /// Create an expression and emit its span as accumulator.
    fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, text: bct::text::Text<'db>, span: datalove_diagnostic::ByteSpan) -> ast::ExprFun<'db> {
        use salsa::plumbing::AsId;
        let expr = ast::ExprFun::new(self.db, kind);
        crate::spans::DatafunSpanAccumulator {
            expr_id: expr.as_id(),
            text_id: text.as_id(),
            span,
        }.accumulate(self.db);
        expr
    }

    /// Emit error if tokens remain unconsumed after a successful parse.
    fn error_if_not_exhausted(&mut self) {
        if self.pos < self.tokens.len() && !self.had_error {
            self.had_error = true;
            let (text, span) = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("P021")
                .primary_label(text, span, "unexpected token")
                .emit_parse();
        }
    }

    fn parse_statement(
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
            _ => {
                let (text, span) = self.peek_text_span();
                self.emit_stmt_error(
                    text,
                    span,
                    "unexpected statement",
                    "P001",
                    "expected 'let', 'var', 'set', 'fun', 'ret', 'require', 'import', 'if', 'loop', 'break', or 'continue'"
                )
            }
        };

        // Check for unconsumed tokens on this line.
        self.error_if_not_exhausted();

        stmt
    }

    /// Parse a statement from a line of tokens.
    /// Creates a sub-parser for the line and parses the statement.
    fn parse_line_statement(
        &mut self,
        line: Vec<TreeToken<'db>>,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        let line_tokens: Vec<_> = line.into_iter().filter_map(|t| t.without_space(self.db)).collect();
        let mut sub = self.sub_parser(line_tokens);
        let stmt = sub.parse_statement(remaining_lines);
        self.had_error |= sub.had_error;
        stmt
    }

    fn parse_let(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("let");

        let name = self.need_name();

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

    fn parse_var(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("var");

        let name = self.need_name();

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

    fn parse_set(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("set");

        let name = self.need_name();

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

        let name = self.need_name();

        // Parse parameters in parentheses
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

        // Parse body until we hit "end fun"
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

    fn parse_fun_params(
        &self,
        iter: BracerIter<'db>,
    ) -> Vec<ast::FunParam<'db>> {
        let tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if tokens.is_empty() {
            return vec![];
        }

        let mut sub = self.sub_parser(tokens);
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

            let name = sub.need_name();

            // Need colon.
            sub.need_sigil(Sigil::Colon);

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

    fn parse_function_call_args(
        &self,
        iter: BracerIter<'db>,
    ) -> Vec<ast::ExprFun<'db>> {
        let tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if tokens.is_empty() {
            return vec![];
        }

        // Split tokens by comma to get individual argument token groups.
        let mut arg_token_groups: Vec<Vec<TreeToken<'db>>> = vec![];
        let mut current_group: Vec<TreeToken<'db>> = vec![];

        for token in tokens {
            match token {
                TreeToken::Token(t) if matches!(t.kind(self.db), TokenKind::Sigil(Sigil::Comma)) => {
                    if !current_group.is_empty() {
                        arg_token_groups.push(current_group);
                        current_group = vec![];
                    }
                }
                _ => {
                    current_group.push(token);
                }
            }
        }

        // Don't forget the last group.
        if !current_group.is_empty() {
            arg_token_groups.push(current_group);
        }

        // Parse each argument group with a sub-parser.
        let mut args = vec![];
        for group in arg_token_groups {
            let mut sub = self.sub_parser(group);
            let arg = sub.parse_expr_full();
            sub.error_if_not_exhausted();
            args.push(arg);
        }

        args
    }

    fn parse_ret(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("ret");

        // Bare `ret` for void functions has no expression.
        let value = if self.peek().is_some() {
            Some(self.parse_expr_full())
        } else {
            None
        };

        ast::Statement::Ret(ast::StmtRet::new(self.db, value))
    }

    fn parse_require(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("require");

        match self.peek_word() {
            Some("module") => {
                self.eat_word("module");

                // Parse 3-part path: lib/pkg/module
                let import_space = self.need_name();

                // Need forward slash
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

                let package_alias = self.need_name();

                // Need forward slash
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

                let module_alias = self.need_name();

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

                let name = self.need_name();

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

    fn parse_import(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("import");

        // Parse module name.
        let module_name = self.need_name();

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
        let item_name = self.need_name();

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
            let binding = self.need_name();
            self.need_sigil(Sigil::Pipe);
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
            let mut else_sub = self.sub_parser(else_tokens);
            else_sub.eat_word("else");

            // Parse optional else binding: |identifier|
            let else_binding = if else_sub.peek_sigil(Sigil::Pipe) {
                else_sub.eat_sigil(Sigil::Pipe);
                let binding = else_sub.need_name();
                else_sub.need_sigil(Sigil::Pipe);
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

    fn parse_break(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("break");
        ast::Statement::Break(ast::StmtBreak::new(self.db, ()))
    }

    fn parse_continue(
        &mut self,
    ) -> ast::Statement<'db> {
        self.eat_word("continue");
        ast::Statement::Continue(ast::StmtContinue::new(self.db, ()))
    }

    // Delegate to datalit parser for type hints.
    fn parse_type_hint_and_heap(
        &mut self,
    ) -> datalit::ast::TypeHintAndHeap<'db> {
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

    fn parse_expr_full(
        &mut self,
    ) -> ast::ExprFun<'db> {
        // Note: span recording now happens in create_expr for each expression.
        self.parse_expr_binop(0)
    }

    // Parse binary operations with precedence climbing algorithm.
    fn parse_expr_binop(
        &mut self,
        min_precedence: u8,
    ) -> ast::ExprFun<'db> {
        let mut lhs = self.parse_expr_primary();

        // Check for postfix try operators (? and !)
        // These have highest precedence and are parsed before binary operators.
        lhs = self.parse_postfix_try_operators(lhs);

        loop {
            // Check for binary operator
            let op = match self.peek_binop() {
                Some(op) => op,
                None => break,
            };

            let precedence = Self::binop_precedence(op);
            if precedence < min_precedence {
                break;
            }

            // Consume the operator
            self.eat_binop(op);

            // Parse right-hand side with higher precedence
            let rhs = self.parse_expr_binop(precedence + 1);

            lhs = ast::ExprFun::new(
                self.db,
                ast::ExprFunKind::BinOp(ast::ExprBinOp::new(self.db, op, lhs, rhs))
            );
        }

        lhs
    }

    // Parse postfix try operators (? and !).
    // These are postfix operators that unwrap Option/Result with early return.
    fn parse_postfix_try_operators(
        &mut self,
        mut expr: ast::ExprFun<'db>,
    ) -> ast::ExprFun<'db> {
        loop {
            match self.peek() {
                Some(TreeToken::Token(token)) => {
                    match token.kind(self.db) {
                        TokenKind::Sigil(Sigil::Question) => {
                            self.next(); // consume ?
                            expr = ast::ExprFun::new(
                                self.db,
                                ast::ExprFunKind::TryOption(ast::ExprTryOption::new(self.db, expr))
                            );
                        }
                        TokenKind::Sigil(Sigil::Exclamation) => {
                            self.next(); // consume !
                            expr = ast::ExprFun::new(
                                self.db,
                                ast::ExprFunKind::TryResult(ast::ExprTryResult::new(self.db, expr))
                            );
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        expr
    }

    // Get operator precedence (higher number = higher precedence).
    fn binop_precedence(op: ast::BinOp) -> u8 {
        match op {
            // Comparison operators (lowest precedence)
            ast::BinOp::Eq | ast::BinOp::Ne |
            ast::BinOp::Lt | ast::BinOp::Gt |
            ast::BinOp::Le | ast::BinOp::Ge => 1,

            // Addition and subtraction (all variants)
            ast::BinOp::Add | ast::BinOp::Sub |
            ast::BinOp::AddChecked | ast::BinOp::SubChecked |
            ast::BinOp::AddOptional | ast::BinOp::SubOptional => 2,

            // Multiplication and division (highest precedence)
            ast::BinOp::Mul | ast::BinOp::Div |
            ast::BinOp::MulChecked | ast::BinOp::DivChecked |
            ast::BinOp::MulOptional | ast::BinOp::DivOptional => 3,
        }
    }

    // Peek at the next token(s) and return the binary operator if present.
    fn peek_binop(
        &self,
    ) -> Option<ast::BinOp> {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    // Two-character operators
                    TokenKind::Sigil(Sigil::PlusExclamation) => Some(ast::BinOp::AddChecked),
                    TokenKind::Sigil(Sigil::MinusExclamation) => Some(ast::BinOp::SubChecked),
                    TokenKind::Sigil(Sigil::StarExclamation) => Some(ast::BinOp::MulChecked),
                    TokenKind::Sigil(Sigil::SlashExclamation) => Some(ast::BinOp::DivChecked),

                    TokenKind::Sigil(Sigil::PlusQuestion) => Some(ast::BinOp::AddOptional),
                    TokenKind::Sigil(Sigil::MinusQuestion) => Some(ast::BinOp::SubOptional),
                    TokenKind::Sigil(Sigil::StarQuestion) => Some(ast::BinOp::MulOptional),
                    TokenKind::Sigil(Sigil::SlashQuestion) => Some(ast::BinOp::DivOptional),

                    TokenKind::Sigil(Sigil::EqualsEquals) => Some(ast::BinOp::Eq),
                    TokenKind::Sigil(Sigil::ExclamationEquals) => Some(ast::BinOp::Ne),
                    TokenKind::Sigil(Sigil::DotLess) => Some(ast::BinOp::Lt),
                    TokenKind::Sigil(Sigil::DotGreater) => Some(ast::BinOp::Gt),
                    TokenKind::Sigil(Sigil::LessEquals) => Some(ast::BinOp::Le),
                    TokenKind::Sigil(Sigil::GreaterEquals) => Some(ast::BinOp::Ge),

                    // Single-character operators (basic arithmetic)
                    TokenKind::Sigil(Sigil::Plus) => Some(ast::BinOp::Add),
                    TokenKind::Sigil(Sigil::Minus) => Some(ast::BinOp::Sub),
                    TokenKind::Sigil(Sigil::Star) => Some(ast::BinOp::Mul),
                    TokenKind::Sigil(Sigil::SlashForward) => Some(ast::BinOp::Div),

                    _ => None,
                }
            }
            _ => None,
        }
    }

    // Consume the operator token(s).
    fn eat_binop(
        &mut self,
        expected_op: ast::BinOp,
    ) {
        // Peek to verify we're consuming the right operator
        if let Some(op) = self.peek_binop() {
            if op == expected_op {
                self.next(); // consume the operator token
                return;
            }
        }
        panic!("expected binary operator {:?}", expected_op);
    }

    // Parse primary expression (literals, names, parenthesized expressions)
    fn parse_expr_primary(
        &mut self,
    ) -> ast::ExprFun<'db> {
        // Check for unary operators (-, -?, or -!).
        if let Some(TreeToken::Token(token)) = self.peek() {
            let unary_op = match token.kind(self.db) {
                TokenKind::Sigil(Sigil::Minus) => Some(ast::UnaryOp::Neg),
                TokenKind::Sigil(Sigil::MinusQuestion) => Some(ast::UnaryOp::NegOptional),
                TokenKind::Sigil(Sigil::MinusExclamation) => Some(ast::UnaryOp::NegResult),
                _ => None,
            };

            if let Some(op) = unary_op {
                self.next(); // Consume the operator.
                let operand = self.parse_expr_primary();
                return ast::ExprFun::new(
                    self.db,
                    ast::ExprFunKind::UnaryOp(ast::ExprUnaryOp::new(self.db, op, operand))
                );
            }
        }

        // Check if it starts with a heap sigil (@ or #) or type hint (`:`) - use new inline variants.
        // Note: `: type / expr` syntax starts without a heap sigil.
        if self.peek_sigil(Sigil::At)
            || self.peek_sigil(Sigil::Hash)
            || self.peek_colon_type_hint()
        {
            return self.parse_lit_expr_full();
        }

        // Peek the next token to determine how to parse this expression.
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                // If it's a word token, check if it's a datalit keyword or a datafun name.
                match token.kind(self.db) {
                    TokenKind::Word => {
                        if let Some(word) = token.word_str(self.db) {
                            // Check against datalit keywords - use new inline variants.
                            match word {
                                "true" | "false" | "tuple" | "struct" | "enum" |
                                "option" | "result" | "error" | "map" | "set" | "none" | "data" |
                                "tensor" => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let (text, start_span) = self.peek_text_span();
                                    let expr_kind = self.parse_lit_expr(datalit::ast::Heap::Omitted, None);
                                    self.create_expr(expr_kind, text, start_span)
                                }
                                // some/ok/er are always keywords - they require a payload expression.
                                "some" | "ok" | "er" => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let (text, start_span) = self.peek_text_span();
                                    self.next(); // consume the keyword
                                    let payload = self.parse_expr_primary();
                                    let heap = datalit::ast::Heap::Omitted;
                                    let expr_kind = match word {
                                        "some" => ast::ExprFunKind::Some(ast::ExprSome::new(self.db, heap, None, payload)),
                                        "ok" => ast::ExprFunKind::Ok(ast::ExprOk::new(self.db, heap, None, payload)),
                                        "er" => ast::ExprFunKind::Er(ast::ExprEr::new(self.db, heap, None, payload)),
                                        _ => unreachable!(),
                                    };
                                    self.create_expr(expr_kind, text, start_span)
                                }
                                num if num.chars().all(|c| char::is_ascii_digit(&c)) => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let (text, start_span) = self.peek_text_span();
                                    let expr_kind = self.parse_lit_expr(datalit::ast::Heap::Omitted, None);
                                    self.create_expr(expr_kind, text, start_span)
                                }
                                _ => {
                                    // It's a datafun name or function call.
                                    // Capture span before consuming token.
                                    let (text, start_span) = self.peek_text_span();
                                    self.next(); // consume the token
                                    let name = InternedText::new(self.db, word.S());

                                    // Check if followed by parentheses (function call).
                                    if let Some(TreeToken::Branch(Sigil::ParenOpen, _)) = self.peek() {
                                        // It's a function call.
                                        let args_iter = match self.next() {
                                            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => iter,
                                            _ => unreachable!(),
                                        };
                                        let args = self.parse_function_call_args(args_iter);
                                        // For function calls, span should include the parens, but for now just use the name span.
                                        self.create_expr(
                                            ast::ExprFunKind::FunctionCall(
                                                ast::ExprFunctionCall::new(self.db, name, args)
                                            ),
                                            text,
                                            start_span
                                        )
                                    } else {
                                        // It's just a variable name.
                                        self.create_expr(
                                            ast::ExprFunKind::Name(name),
                                            text,
                                            start_span
                                        )
                                    }
                                }
                            }
                        } else {
                            let (text, span) = self.peek_text_span();
                            self.next();
                            self.emit_expr_error(
                                text,
                                span,
                                "unexpected token in expression",
                                "P007",
                                "unexpected token"
                            )
                        }
                    }
                    TokenKind::String => {
                        // String literal - use new inline variant.
                        let text_str = token.text(self.db).as_str(self.db).S();
                        self.next();
                        let value = InternedText::new(self.db, text_str);
                        ast::ExprFun::new(
                            self.db,
                            ast::ExprFunKind::String(ast::ExprString::new(
                                self.db,
                                datalit::ast::Heap::Omitted,
                                None,
                                value
                            ))
                        )
                    }
                    _ => {
                        let (text, span) = self.peek_text_span();
                        self.emit_expr_error(
                            text,
                            span,
                            "unexpected token in expression",
                            "P010",
                            "unexpected token"
                        )
                    }
                }
            }
            Some(TreeToken::Branch(sigil, _)) => {
                // Check if it's a tuple (ParenOpen) - parse as datafun tuple.
                // Other branches like {}, [] are literal expressions.
                if matches!(sigil, Sigil::ParenOpen) {
                    self.parse_datafun_tuple()
                } else {
                    // Capture span before parsing for diagnostic reporting.
                    let (text, start_span) = self.peek_text_span();
                    let expr_kind = self.parse_lit_expr(datalit::ast::Heap::Omitted, None);
                    self.create_expr(expr_kind, text, start_span)
                }
            }
            None => {
                let (text, span) = self.peek_text_span();
                self.emit_expr_error(
                    text,
                    span,
                    "expected expression",
                    "P008",
                    "expected expression"
                )
            }
        }
    }

    // Parse a literal expression into new inline variants.
    // This handles the `: type / expr` pattern.
    fn parse_lit_expr_full(
        &mut self,
    ) -> ast::ExprFun<'db> {
        // Capture span before parsing for diagnostic reporting.
        let (text, start_span) = self.peek_text_span();

        // Check for `: type / expr` pattern.
        if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            // Expect `/` after type hint.
            if !self.eat_sigil(Sigil::SlashForward) {
                let (text, span) = self.peek_text_span();
                return self.emit_expr_error(
                    text,
                    span,
                    "expected '/' after type hint in `: type / expr` pattern",
                    "D021",
                    "expected '/'"
                );
            }
            let (_heap, expr_kind) = self.parse_lit_expr_and_heap(Some(type_hint));
            // The type_hint is already captured in the expr_kind.
            return self.create_expr(expr_kind, text, start_span);
        }

        let (_heap, expr_kind) = self.parse_lit_expr_and_heap(None);
        self.create_expr(expr_kind, text, start_span)
    }

    // Parse heap sigil and expression.
    fn parse_lit_expr_and_heap(
        &mut self,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> (datalit::ast::Heap, ast::ExprFunKind<'db>) {
        // Heap sigils: @ for local, # for global.
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            datalit::ast::Heap::Local
        } else if self.peek_sigil(Sigil::Hash) {
            self.eat_sigil(Sigil::Hash);
            datalit::ast::Heap::Global
        } else {
            datalit::ast::Heap::Omitted
        };

        let expr_kind = self.parse_lit_expr(heap, type_hint);
        (heap, expr_kind)
    }

    // Parse a literal expression (keywords and literals).
    fn parse_lit_expr(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        // Check for negative number.
        if self.peek_sigil(Sigil::Minus) {
            self.eat_sigil(Sigil::Minus);
            if let Some(TreeToken::Token(token)) = self.peek() {
                if let Some(word) = token.word_str(self.db) {
                    if Self::is_numeric_literal(word) {
                        self.next();
                        let is_hex = word.starts_with("0x") || word.starts_with("0X");
                        // Check for float: consume dot, then check for decimal digits.
                        if !is_hex && self.peek_sigil(Sigil::Dot) {
                            self.eat_sigil(Sigil::Dot);
                            if let Some(TreeToken::Token(next)) = self.peek() {
                                if let Some(decimal) = next.word_str(self.db) {
                                    if decimal.chars().all(|c| c.is_ascii_digit()) {
                                        let decimal_name = self.need_name();
                                        let float_str = format!("-{}.{}", word, decimal_name.as_str(self.db));
                                        let value = InternedText::new(self.db, float_str.S());
                                        return ast::ExprFunKind::Float(ast::ExprFloat::new(self.db, heap, type_hint, value));
                                    }
                                }
                            }
                            // Dot was consumed but no valid decimal follows.
                            let (text, span) = self.peek_text_span();
                            return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                                self.db,
                                text,
                                span,
                                InternedText::new(self.db, "expected decimal digits after '.'".S()),
                            ));
                        }
                        if is_hex {
                            let hex_str = format!("-{}", word);
                            let value = InternedText::new(self.db, hex_str.S());
                            return ast::ExprFunKind::Hex(ast::ExprHex::new(self.db, heap, type_hint, value));
                        } else {
                            let int_str = format!("-{}", word);
                            let value = InternedText::new(self.db, int_str.S());
                            return ast::ExprFunKind::Int(ast::ExprInt::new(self.db, heap, type_hint, value));
                        }
                    }
                }
            }
            // Not a negative number - error.
            let (text, span) = self.peek_text_span();
            return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                self.db,
                text,
                span,
                InternedText::new(self.db, "unexpected minus sign".S()),
            ));
        }

        // Check for keywords.
        match self.peek_word() {
            Some("true") => {
                self.eat_word("true");
                return ast::ExprFunKind::True(ast::ExprLit::new(self.db, heap, type_hint));
            }
            Some("false") => {
                self.eat_word("false");
                return ast::ExprFunKind::False(ast::ExprLit::new(self.db, heap, type_hint));
            }
            Some("none") => {
                self.eat_word("none");
                return ast::ExprFunKind::None(ast::ExprLit::new(self.db, heap, type_hint));
            }
            Some("some") => {
                self.eat_word("some");
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Some(ast::ExprSome::new(self.db, heap, type_hint, payload));
            }
            Some("ok") => {
                self.eat_word("ok");
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Ok(ast::ExprOk::new(self.db, heap, type_hint, payload));
            }
            Some("er") => {
                self.eat_word("er");
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Er(ast::ExprEr::new(self.db, heap, type_hint, payload));
            }
            Some("data") => {
                self.eat_word("data");
                // Parse any datafun expression (superset of datalit).
                let value = self.parse_expr_primary();
                return ast::ExprFunKind::Data(ast::ExprData::new(self.db, heap, type_hint, value));
            }
            Some("error") => {
                self.eat_word("error");
                // Parse any datafun expression (superset of datalit).
                let value = self.parse_expr_primary();
                return ast::ExprFunKind::Err(ast::ExprErr::new(self.db, heap, type_hint, value));
            }
            Some("tensor") => {
                return self.parse_lit_tensor(heap, type_hint);
            }
            Some("enum") => {
                return self.parse_lit_anon_enum(heap, type_hint);
            }
            Some("map") => {
                return self.parse_lit_map(heap, type_hint);
            }
            Some("set") => {
                return self.parse_lit_set(heap, type_hint);
            }
            _ => {}
        }

        // Check for numbers, strings, or branches.
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).X();
                        if Self::is_numeric_literal(word) {
                            self.next();
                            let is_hex = word.starts_with("0x") || word.starts_with("0X");
                            // Check for float: consume dot, then check for decimal digits.
                            if !is_hex && self.peek_sigil(Sigil::Dot) {
                                self.eat_sigil(Sigil::Dot);
                                if let Some(TreeToken::Token(next)) = self.peek() {
                                    if let Some(decimal) = next.word_str(self.db) {
                                        if decimal.chars().all(|c| c.is_ascii_digit()) {
                                            let decimal_name = self.need_name();
                                            let float_str = format!("{}.{}", word, decimal_name.as_str(self.db));
                                            let value = InternedText::new(self.db, float_str.S());
                                            return ast::ExprFunKind::Float(ast::ExprFloat::new(self.db, heap, type_hint, value));
                                        }
                                    }
                                }
                                // Dot was consumed but no valid decimal follows - treat as member access.
                                // This is a parse error for datalit, but we need to handle it.
                                // For now, return an error.
                                let (text, span) = self.peek_text_span();
                                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                                    self.db,
                                    text,
                                    span,
                                    InternedText::new(self.db, "expected decimal digits after '.'".S()),
                                ));
                            }
                            let value = InternedText::new(self.db, word.S());
                            if is_hex {
                                return ast::ExprFunKind::Hex(ast::ExprHex::new(self.db, heap, type_hint, value));
                            } else {
                                return ast::ExprFunKind::Int(ast::ExprInt::new(self.db, heap, type_hint, value));
                            }
                        } else {
                            // Unexpected identifier.
                            let (text, span) = self.peek_text_span();
                            self.next();
                            return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                                self.db,
                                text,
                                span,
                                InternedText::new(self.db, format!("unexpected identifier '{}'", word).S()),
                            ));
                        }
                    }
                    TokenKind::String => {
                        // Get the text before consuming the token.
                        let text_str = token.text(self.db).as_str(self.db).S();
                        self.next();
                        let value = InternedText::new(self.db, text_str);
                        return ast::ExprFunKind::String(ast::ExprString::new(self.db, heap, type_hint, value));
                    }
                    _ => {
                        let (text, span) = self.peek_text_span();
                        return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                            self.db,
                            text,
                            span,
                            InternedText::new(self.db, "unexpected token".S()),
                        ));
                    }
                }
            }
            Some(TreeToken::Branch(Sigil::ParenOpen, _)) => {
                // Anonymous tuple.
                return self.parse_lit_anon_tuple(heap, type_hint);
            }
            Some(TreeToken::Branch(Sigil::BraceOpen, _)) => {
                // Anonymous struct.
                return self.parse_lit_anon_struct(heap, type_hint);
            }
            Some(TreeToken::Branch(Sigil::BracketOpen, _)) => {
                // List.
                return self.parse_lit_list(heap, type_hint);
            }
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db,
                    text,
                    span,
                    InternedText::new(self.db, "expected expression".S()),
                ));
            }
        }
    }

    // Helper to check if a string is a numeric literal.
    fn is_numeric_literal(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        if s.starts_with("0x") || s.starts_with("0X") {
            // Hex literal - allow hex digits after prefix.
            let s = &s[2..];
            !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit() || c == '_')
        } else {
            // Decimal literal - only allow decimal digits.
            s.chars().all(|c| c.is_ascii_digit() || c == '_')
        }
    }

    // Parse anonymous tuple: (expr, expr, ...)
    fn parse_lit_anon_tuple(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => iter,
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '('".S()),
                ));
            }
        };

        let elements = self.parse_comma_separated_exprs(iter);
        ast::ExprFunKind::AnonTuple(ast::ExprAnonTuple::new(self.db, heap, type_hint, elements))
    }

    // Parse anonymous struct: { name = expr, ... }
    fn parse_lit_anon_struct(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => iter,
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '{'".S()),
                ));
            }
        };

        let fields = self.parse_comma_separated_struct_fields(iter);
        ast::ExprFunKind::AnonStruct(ast::ExprAnonStruct::new(self.db, heap, type_hint, fields))
    }

    // Parse list: [expr, expr, ...]
    fn parse_lit_list(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => iter,
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '['".S()),
                ));
            }
        };

        let elements = self.parse_comma_separated_exprs(iter);
        ast::ExprFunKind::List(ast::ExprList::new(self.db, heap, type_hint, elements))
    }

    // Parse set: set { expr, expr, ... }
    fn parse_lit_set(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("set");
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => iter,
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '{' after 'set'".S()),
                ));
            }
        };

        let elements = self.parse_comma_separated_exprs(iter);
        ast::ExprFunKind::Set(ast::ExprSet::new(self.db, heap, type_hint, elements))
    }

    // Parse map: map { key = value, ... }
    fn parse_lit_map(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("map");
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => iter,
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '{' after 'map'".S()),
                ));
            }
        };

        let entries = self.parse_comma_separated_map_entries(iter);
        ast::ExprFunKind::Map(ast::ExprMap::new(self.db, heap, type_hint, entries))
    }

    // Parse anonymous enum: enum Variant or enum Variant(payload)
    fn parse_lit_anon_enum(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("enum");
        let variant_name = self.need_name();
        let payload = self.parse_optional_enum_payload();
        ast::ExprFunKind::AnonEnum(ast::ExprAnonEnum::new(
            self.db, heap, type_hint, variant_name, payload
        ))
    }

    // Parse optional enum payload: (expr)
    fn parse_optional_enum_payload(
        &mut self,
    ) -> Option<ast::ExprFun<'db>> {
        match self.peek() {
            Some(TreeToken::Branch(Sigil::ParenOpen, _)) => {
                let iter = match self.next() {
                    Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => iter,
                    _ => return None,
                };
                let all_tokens: Vec<_> = iter.filter_map(|t| t.without_space(self.db)).collect();
                if all_tokens.is_empty() {
                    return None;
                }
                let mut sub = self.sub_parser(all_tokens);
                let expr = sub.parse_expr_full();
                sub.error_if_not_exhausted();
                self.had_error |= sub.had_error;
                Some(expr)
            }
            _ => None
        }
    }

    // Parse tensor: tensor [shape] [data]
    fn parse_lit_tensor(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("tensor");

        // Parse shape: [dim1, dim2, ...]
        let shape = match self.next() {
            Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => {
                let all_tokens: Vec<_> = iter.filter_map(|t| t.without_space(self.db)).collect();
                self.parse_tensor_shape(all_tokens)
            }
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '[' for tensor shape".S()),
                ));
            }
        };

        let rank = shape.len();

        // Parse data: [elements]
        // For rank 1: comma-separated elements.
        // For rank 2+: comma-separated rows, space-separated elements within each row.
        let elements = match self.next() {
            Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => {
                if rank <= 1 {
                    self.parse_comma_separated_exprs(iter)
                } else {
                    let row_size = *shape.last().unwrap_or(&1) as usize;
                    let (elems, has_error) = self.parse_tensor_data_2d_plus(iter, row_size);
                    if has_error {
                        let (text, span) = self.peek_text_span();
                        return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                            self.db, text, span,
                            InternedText::new(self.db, format!("expected {} elements per row", row_size).S()),
                        ));
                    }
                    elems
                }
            }
            _ => {
                let (text, span) = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, text, span,
                    InternedText::new(self.db, "expected '[' for tensor data".S()),
                ));
            }
        };

        ast::ExprFunKind::Tensor(ast::ExprTensor::new(self.db, heap, type_hint, shape, elements))
    }

    // Parse tensor data for 2D+ tensors: comma-separated rows, space-separated elements.
    fn parse_tensor_data_2d_plus(&mut self, iter: BracerIter<'db>, row_size: usize) -> (Vec<ast::ExprFun<'db>>, bool) {
        let all_tokens: Vec<_> = iter.collect();
        if all_tokens.is_empty() {
            return (vec![], false);
        }

        // Split by comma to get rows.
        let rows = self.split_tokens_by_comma_with_spaces(&all_tokens);
        let mut elements = Vec::new();
        let mut has_error = false;

        for row_tokens in rows {
            // Each row contains space-separated elements.
            // Filter spaces to get element tokens, then parse greedily.
            let elem_tokens: Vec<_> = row_tokens.into_iter()
                .filter_map(|t| t.without_space(self.db))
                .collect();

            // Parse all elements in the row using a sub-parser.
            let mut sub = self.sub_parser(elem_tokens);
            let mut row_elements = Vec::new();
            while sub.peek().is_some() {
                row_elements.push(sub.parse_expr_full());
            }
            self.had_error |= sub.had_error;

            // Validate row size matches the last dimension.
            if row_elements.len() != row_size {
                has_error = true;
            }

            elements.extend(row_elements);
        }

        (elements, has_error)
    }

    // Split tokens by comma, preserving spaces within groups (for tensor row parsing).
    fn split_tokens_by_comma_with_spaces(&self, tokens: &[TreeToken<'db>]) -> Vec<Vec<TreeToken<'db>>> {
        let mut groups = Vec::new();
        let mut current = Vec::new();

        for token in tokens {
            if let TreeToken::Token(t) = token {
                if matches!(t.kind(self.db), TokenKind::Sigil(Sigil::Comma)) {
                    if !current.is_empty() {
                        groups.push(std::mem::take(&mut current));
                    }
                    continue;
                }
            }
            current.push(token.clone());
        }

        if !current.is_empty() {
            groups.push(current);
        }

        groups
    }

    // Parse tensor shape dimensions.
    // Uses incremental parsing like datalit: parse dimension, look for comma, repeat.
    fn parse_tensor_shape(&mut self, tokens: Vec<TreeToken<'db>>) -> Vec<u32> {
        let filtered: Vec<_> = tokens.into_iter()
            .filter_map(|t| t.without_space(self.db))
            .collect();

        if filtered.is_empty() {
            return vec![];
        }

        let mut iter = filtered.into_iter().peekable();
        let mut shape = Vec::new();

        loop {
            // Try to parse a dimension number.
            if let Some(TreeToken::Token(t)) = iter.peek() {
                if let Some(word) = t.word_str(self.db) {
                    if let Ok(dim) = word.parse::<u32>() {
                        iter.next(); // consume the token
                        shape.push(dim);
                    } else {
                        // Not a valid number, stop parsing.
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }

            // Look for comma to continue, otherwise stop.
            if let Some(TreeToken::Token(t)) = iter.peek() {
                if matches!(t.kind(self.db), TokenKind::Sigil(Sigil::Comma)) {
                    iter.next(); // consume comma
                    // Handle trailing comma.
                    if iter.peek().is_none() {
                        break;
                    }
                } else {
                    // No comma, stop parsing.
                    break;
                }
            } else {
                break;
            }
        }

        // Check for unconsumed tokens.
        if iter.peek().is_some() {
            self.had_error = true;
            // Emit a diagnostic for unconsumed tokens.
            let (text, span) = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected tokens in tensor shape")
                .code("D030")
                .primary_label(text, span, "unexpected")
                .emit_parse();
        }
        shape
    }

    // Helper to parse comma-separated expressions from a branch.
    // Uses incremental parsing like datalit: parse expr, look for comma, repeat.
    fn parse_comma_separated_exprs(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprFun<'db>> {
        let all_tokens: Vec<_> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if all_tokens.is_empty() {
            return vec![];
        }

        let mut sub = self.sub_parser(all_tokens);
        let mut elements = Vec::new();

        loop {
            if sub.peek().is_none() {
                break;
            }
            elements.push(sub.parse_expr_full());

            // Look for comma to continue, otherwise stop.
            if !sub.eat_sigil(Sigil::Comma) {
                break;
            }

            // Handle trailing comma: if we're at the end after comma, stop parsing.
            if sub.peek().is_none() {
                break;
            }
        }

        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        elements
    }

    // Helper to parse comma-separated struct fields.
    // Uses incremental parsing like datalit: parse field, look for comma, repeat.
    fn parse_comma_separated_struct_fields(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprStructField<'db>> {
        let all_tokens: Vec<_> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if all_tokens.is_empty() {
            return vec![];
        }

        let mut sub = self.sub_parser(all_tokens);
        let mut fields = Vec::new();

        loop {
            // Try to get name.
            let name = match sub.eat_name() {
                Some(n) => n,
                None => {
                    if sub.peek().is_none() {
                        // End of tokens - we're done.
                        break;
                    }
                    // Missing name - emit error and use placeholder.
                    let (text, span) = sub.peek_text_span();
                    let error_expr = sub.emit_expr_error(
                        text,
                        span,
                        "expected field name in struct",
                        "D021",
                        "expected field name"
                    );
                    let error_name = InternedText::new(sub.db, "<error>".S());
                    fields.push(ast::ExprStructField::new(sub.db, error_name, error_expr));
                    break;
                }
            };

            // Try to get `=`.
            if !sub.eat_sigil(Sigil::Equals) {
                // Missing equals - emit error and use placeholder value.
                let (text, span) = sub.peek_text_span();
                let error_expr = sub.emit_expr_error(
                    text,
                    span,
                    "expected '=' after field name in struct",
                    "D022",
                    "expected '='"
                );
                fields.push(ast::ExprStructField::new(sub.db, name, error_expr));
                break;
            }

            let value = sub.parse_expr_full();
            fields.push(ast::ExprStructField::new(sub.db, name, value));

            // Look for comma to continue, otherwise stop.
            if !sub.eat_sigil(Sigil::Comma) {
                break;
            }

            // Handle trailing comma: if we're at the end after comma, stop parsing.
            if sub.peek().is_none() {
                break;
            }
        }

        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        fields
    }

    // Helper to parse comma-separated map entries.
    // Uses incremental parsing like datalit: parse entry (key = value), look for comma, repeat.
    fn parse_comma_separated_map_entries(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprMapEntry<'db>> {
        let all_tokens: Vec<_> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if all_tokens.is_empty() {
            return vec![];
        }

        let mut sub = self.sub_parser(all_tokens);
        let mut entries = Vec::new();

        loop {
            if sub.peek().is_none() {
                break;
            }

            // Parse key.
            let key = sub.parse_expr_full();

            // Expect `=`.
            if !sub.eat_sigil(Sigil::Equals) {
                // Missing equals - emit error.
                let (text, span) = sub.peek_text_span();
                let error_value = sub.emit_expr_error(
                    text,
                    span,
                    "expected '=' between map key and value",
                    "D023",
                    "expected '='"
                );
                entries.push(ast::ExprMapEntry::new(sub.db, key, error_value));
                break;
            }

            // Parse value.
            let value = sub.parse_expr_full();
            entries.push(ast::ExprMapEntry::new(sub.db, key, value));

            // Look for comma to continue, otherwise stop.
            if !sub.eat_sigil(Sigil::Comma) {
                break;
            }

            // Handle trailing comma.
            if sub.peek().is_none() {
                break;
            }
        }

        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        entries
    }

    // Split tokens by comma.
    fn _split_by_comma(&self, tokens: Vec<TreeToken<'db>>) -> Vec<Vec<TreeToken<'db>>> {
        let mut groups = Vec::new();
        let mut current = Vec::new();
        for token in tokens {
            match token {
                TreeToken::Token(t) if matches!(t.kind(self.db), TokenKind::Sigil(Sigil::Comma)) => {
                    if !current.is_empty() {
                        groups.push(current);
                        current = Vec::new();
                    }
                }
                _ => current.push(token),
            }
        }
        if !current.is_empty() {
            groups.push(current);
        }
        groups
    }

    // Split tokens by equals sign (for map entries).
    fn _split_by_equals(&self, tokens: Vec<TreeToken<'db>>) -> (Vec<TreeToken<'db>>, Vec<TreeToken<'db>>) {
        let mut key = Vec::new();
        let mut value = Vec::new();
        let mut found_equals = false;
        for token in tokens {
            if !found_equals {
                if matches!(&token, TreeToken::Token(t) if matches!(t.kind(self.db), TokenKind::Sigil(Sigil::Equals))) {
                    found_equals = true;
                } else {
                    key.push(token);
                }
            } else {
                value.push(token);
            }
        }
        (key, value)
    }

    // Parse a datafun tuple: (expr1, expr2, ...).
    // Uses incremental parsing like datalit: parse element, look for comma, repeat.
    fn parse_datafun_tuple(
        &mut self,
    ) -> ast::ExprFun<'db> {
        // Consume the ParenOpen branch and get its contents.
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => iter,
            _ => {
                let (text, span) = self.peek_text_span();
                return self.emit_expr_error(
                    text,
                    span,
                    "expected tuple",
                    "P009",
                    "expected '(' to start tuple"
                );
            }
        };

        // Collect all tokens and filter out spaces.
        let all_tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if all_tokens.is_empty() {
            // Empty tuple.
            return ast::ExprFun::new(
                self.db,
                ast::ExprFunKind::Tuple(ast::ExprTuple::new(self.db, vec![]))
            );
        }

        // Use incremental parsing like datalit.
        let mut sub = self.sub_parser(all_tokens);
        let mut elements = vec![];

        loop {
            if sub.peek().is_none() {
                break;
            }
            elements.push(sub.parse_expr_full());

            // Look for comma to continue, otherwise stop.
            if !sub.eat_sigil(Sigil::Comma) {
                break;
            }

            // Handle trailing comma.
            if sub.peek().is_none() {
                break;
            }
        }

        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;

        // If there's exactly one element and no trailing comma, treat as grouping (not tuple).
        if elements.len() == 1 {
            elements.into_iter().next().unwrap()
        } else {
            ast::ExprFun::new(
                self.db,
                ast::ExprFunKind::Tuple(ast::ExprTuple::new(self.db, elements))
            )
        }
    }

}

/// Tracked wrapper for parser tests that only need the Script.
#[salsa::tracked]
pub(crate) fn parse_for_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::Script<'db> {
    parse(db, source).script(db)
}

/// Public tracked wrapper for integration tests that returns just the Script.
/// Integration tests are compiled as separate binaries and need pub access.
#[salsa::tracked]
pub fn parse_integration_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::Script<'db> {
    parse(db, source).script(db)
}

/// Public tracked wrapper for integration code to enable diagnostic accumulation.
/// This function should be called before parse() to accumulate diagnostics,
/// then parse() can be called separately to get the full ParseResult.
/// Returns just the Script to satisfy Salsa's type requirements.
#[salsa::tracked]
pub fn parse_for_diagnostics<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::Script<'db> {
    parse(db, source).script(db)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_let_simple() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @42"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                assert!(stmt.type_hint(db).is_none());
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_let_with_type() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x: @u32 = @42"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                assert!(stmt.type_hint(db).is_some());
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_fun_simple() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("fun foo()\nend fun"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Fun(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "foo");
                assert_eq!(stmt.params(db).len(), 0);
                assert!(stmt.return_type(db).is_none());
                assert_eq!(stmt.body(db).len(), 0);
            }
            _ => panic!("expected fun statement"),
        }
    }

    #[test]
    fn test_parse_fun_with_params() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("fun increment(accum: @u64, amount: @u8): !@u64\nend fun"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Fun(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "increment");
                assert_eq!(stmt.params(db).len(), 2);
                assert_eq!(stmt.params(db)[0].name(db).as_str(db), "accum");
                assert_eq!(stmt.params(db)[1].name(db).as_str(db), "amount");
                assert!(stmt.return_type(db).is_some());
            }
            _ => panic!("expected fun statement"),
        }
    }

    #[test]
    fn test_parse_fun_with_list_param() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("fun identity(a: @[@u32]): @[@u32]\n  ret a\nend fun"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Fun(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "identity");
                assert_eq!(stmt.params(db).len(), 1);
                assert_eq!(stmt.params(db)[0].name(db).as_str(db), "a");
                // Check that return type is present
                assert!(stmt.return_type(db).is_some());

                // Check if it's a ParseError
                let param_type = stmt.params(db)[0].type_hint(db);
                match param_type.type_hint(db) {
                    datalit::ast::TypeHint::ParseError(_) => {
                        panic!("Parameter type hint is a ParseError!");
                    }
                    datalit::ast::TypeHint::List(_) => {
                        // Good!
                    }
                    _ => panic!("Expected List type hint"),
                }
            }
            _ => panic!("expected fun statement"),
        }
    }

    #[test]
    fn test_parse_fun_multiline_params() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("fun increment(\n  accum: @u64, amount: @u8,\n): !@u64\n  ret @0\nend fun"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Fun(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "increment");
                assert_eq!(stmt.params(db).len(), 2);
                assert_eq!(stmt.body(db).len(), 1);
                // Check the body has a ret statement
                match &stmt.body(db)[0] {
                    ast::Statement::Ret(_) => {}
                    _ => panic!("expected ret statement in body"),
                }
            }
            _ => panic!("expected fun statement"),
        }
    }

    #[test]
    fn test_parse_require() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("require module sys/std/bool"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Require(ast::StmtRequire::Module(stmt)) => {
                assert_eq!(stmt.import_space(db).as_str(db), "sys");
                assert_eq!(stmt.package_alias(db).as_str(db), "std");
                assert_eq!(stmt.module_alias(db).as_str(db), "bool");
            }
            _ => panic!("expected require module statement"),
        }
    }

    #[test]
    fn test_parse_import() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("import u32.negate"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Import(stmt) => {
                assert_eq!(stmt.module_name(db).as_str(db), "u32");
                assert_eq!(stmt.item_name(db).as_str(db), "negate");
            }
            _ => panic!("expected import statement"),
        }
    }

    #[test]
    fn test_parse_import_with_require() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("require module sys/std/u32\nimport u32.negate"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 2);
        match &statements[0] {
            ast::Statement::Require(ast::StmtRequire::Module(stmt)) => {
                assert_eq!(stmt.module_alias(db).as_str(db), "u32");
            }
            _ => panic!("expected require module statement"),
        }
        match &statements[1] {
            ast::Statement::Import(stmt) => {
                assert_eq!(stmt.module_name(db).as_str(db), "u32");
                assert_eq!(stmt.item_name(db).as_str(db), "negate");
            }
            _ => panic!("expected import statement"),
        }
    }

    #[test]
    fn test_parse_expr_bare_name() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = accum"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Name(name) => {
                        assert_eq!(name.as_str(db), "accum");
                    }
                    _ => panic!("expected name expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_datalit() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @42"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Int(int_expr) => {
                        // Successfully parsed as inline int.
                        assert_eq!(int_expr.value(db).as_str(db), "42");
                    }
                    other => panic!("expected Int expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_binop_checked() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a +! b"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::BinOp(binop) => {
                        assert_eq!(binop.op(db), ast::BinOp::AddChecked);
                    }
                    _ => panic!("expected binop expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_binop_optional() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a +? b"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::BinOp(binop) => {
                        assert_eq!(binop.op(db), ast::BinOp::AddOptional);
                    }
                    _ => panic!("expected binop expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_binop_basic() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a + b"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::BinOp(binop) => {
                        assert_eq!(binop.op(db), ast::BinOp::Add);
                    }
                    _ => panic!("expected binop expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_binop_comparison() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a .< b"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::BinOp(binop) => {
                        assert_eq!(binop.op(db), ast::BinOp::Lt);
                    }
                    _ => panic!("expected binop expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_binop_precedence() {
        // Test that multiplication has higher precedence than addition
        // "a + b * c" should parse as "a + (b * c)"
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a + b * c"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::BinOp(binop) => {
                        // Top level should be addition
                        assert_eq!(binop.op(db), ast::BinOp::Add);
                        // RHS should be multiplication
                        match binop.rhs(db).expr(db) {
                            ast::ExprFunKind::BinOp(rhs_binop) => {
                                assert_eq!(rhs_binop.op(db), ast::BinOp::Mul);
                            }
                            _ => panic!("expected binop for rhs"),
                        }
                    }
                    _ => panic!("expected binop expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_fun_with_binop_in_ret() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("fun increment(accum: @u64, amount: @u8): !@u64\n  ret accum +! amount\nend fun"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Fun(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "increment");
                assert_eq!(stmt.body(db).len(), 1);
                // Check the body has a ret statement with binop
                match &stmt.body(db)[0] {
                    ast::Statement::Ret(ret) => {
                        let value = ret.value(db).expect("expected ret with value");
                        match value.expr(db) {
                            ast::ExprFunKind::BinOp(binop) => {
                                assert_eq!(binop.op(db), ast::BinOp::AddChecked);
                            }
                            _ => panic!("expected binop in ret"),
                        }
                    }
                    _ => panic!("expected ret statement in body"),
                }
            }
            _ => panic!("expected fun statement"),
        }
    }

    #[test]
    fn test_parse_multiple_statements_with_semicolon() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @1; let y = @2"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 2);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
            }
            _ => panic!("expected let statement"),
        }
        match &statements[1] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "y");
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_semicolon_with_newline_mix() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @1; let y = @2\nlet z = @3"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 3);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
            }
            _ => panic!("expected let statement"),
        }
        match &statements[1] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "y");
            }
            _ => panic!("expected let statement"),
        }
        match &statements[2] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "z");
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_require_with_semicolon() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("require module sys/std/bool; let x = @42"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 2);
        match &statements[0] {
            ast::Statement::Require(ast::StmtRequire::Module(stmt)) => {
                assert_eq!(stmt.import_space(db).as_str(db), "sys");
                assert_eq!(stmt.package_alias(db).as_str(db), "std");
                assert_eq!(stmt.module_alias(db).as_str(db), "bool");
            }
            _ => panic!("expected require module statement"),
        }
        match &statements[1] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
            }
            _ => panic!("expected let statement"),
        }
    }

    // Tests for complex datalit expressions enabled by direct token parsing

    #[test]
    fn test_parse_datalit_tuple() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @(1, 2, 3)"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::AnonTuple(tuple) => {
                        assert_eq!(tuple.elements(db).len(), 3);
                    }
                    other => panic!("expected AnonTuple expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_list() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @[1, 2, 3]"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::List(list) => {
                        assert_eq!(list.elements(db).len(), 3);
                    }
                    other => panic!("expected List expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_map() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @map { @1 = @10, @2 = @20 }"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Map(map) => {
                        assert_eq!(map.entries(db).len(), 2);
                    }
                    other => panic!("expected Map expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_nested_tuple_in_list() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @[(1, 2), (3, 4)]"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::List(list) => {
                        // List with nested tuples.
                        assert_eq!(list.elements(db).len(), 2);
                    }
                    other => panic!("expected List expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_nested_list_in_tuple() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @(@[@1, @2, @3], @100)"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::AnonTuple(tuple) => {
                        // Tuple with nested list.
                        assert_eq!(tuple.elements(db).len(), 2);
                    }
                    other => panic!("expected AnonTuple expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_set() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @set { @1, @2, @3 }"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Set(set) => {
                        assert_eq!(set.elements(db).len(), 3);
                    }
                    other => panic!("expected Set expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_deeply_nested() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
        let script = parse_for_test(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::AnonTuple(tuple) => {
                        // Deeply nested tuple.
                        assert_eq!(tuple.elements(db).len(), 2);
                    }
                    other => panic!("expected AnonTuple expression, got {:?}", std::mem::discriminant(&other)),
                }
            }
            _ => panic!("expected let statement"),
        }
    }
}
