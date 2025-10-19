use rmx::prelude::*;

use rmx::core::iter::Peekable;
use bct::{
    input::Source,
    chunk::Chunk,
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
    lines,
};

use crate::ast;
use crate::datalit;
use crate::script;

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
    parse(db, source)
}

#[salsa::tracked]
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::Script<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer(db, bracer)
}

#[salsa::tracked]
fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
) -> ast::Script<'db> {
    let mut parser = Parser {
        db,
    };

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

    let statements = parser.parse_statements(lines);
    ast::Script::new(db, statements)
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

struct Parser<'db> {
    db: &'db dyn crate::Db,
}

impl<'db> Parser<'db> {
    fn parse_statements(&mut self, lines: Vec<Vec<TreeToken<'db>>>) -> Vec<ast::Statement<'db>> {
        let mut statements = vec![];
        let mut line_iter = lines.into_iter().enumerate().peekable();

        while let Some((line_num, line)) = line_iter.next() {
            if line.is_empty() {
                continue;
            }

            let statement = self.parse_statement(line, &mut line_iter);
            statements.push(statement);
        }

        statements
    }

    fn parse_statement(
        &mut self,
        line: Vec<TreeToken<'db>>,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        let mut tokens = line.into_iter().peekable();

        match self.peek_word(&mut tokens) {
            Some("let") => self.parse_let(&mut tokens),
            Some("fun") => self.parse_fun(&mut tokens, remaining_lines),
            Some("ret") => self.parse_ret(&mut tokens),
            Some("require") => self.parse_require(&mut tokens),
            Some("import") => self.parse_import(&mut tokens),
            Some("if") => self.parse_if(&mut tokens, remaining_lines),
            _ => {
                let message = InternedText::new(self.db, "unexpected statement".S());
                ast::Statement::ParseError(ast::StmtParseError::new(self.db, message))
            }
        }
    }

    fn parse_let(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::Statement<'db> {
        self.eat_word(tokens, "let");

        let name = self.need_name(tokens);

        // Check for type hint: `: type`
        let type_hint = if self.peek_sigil(tokens, Sigil::Colon) {
            self.eat_sigil(tokens, Sigil::Colon);
            Some(self.parse_type_hint_and_heap(tokens))
        } else {
            None
        };

        // Need `=` sigil
        self.need_sigil(tokens, Sigil::Equals);

        // Parse the value expression.
        let value = self.parse_expr_full(tokens);

        ast::Statement::Let(ast::StmtLet::new(
            self.db,
            name,
            type_hint,
            value,
        ))
    }

    fn parse_fun(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word(tokens, "fun");

        let name = self.need_name(tokens);

        // Parse parameters in parentheses
        let params = match tokens.next() {
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                self.parse_fun_params(iter)
            }
            _ => {
                let message = InternedText::new(self.db, "expected parameter list".S());
                return ast::Statement::ParseError(ast::StmtParseError::new(self.db, message));
            }
        };

        // Check for return type: `: type`
        let return_type = if self.peek_sigil(tokens, Sigil::Colon) {
            self.eat_sigil(tokens, Sigil::Colon);
            Some(self.parse_type_hint_and_heap(tokens))
        } else {
            None
        };

        // Parse body until we hit "end fun"
        let mut body = vec![];
        while let Some((_, line)) = remaining_lines.peek() {
            if line.len() >= 2 {
                if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                    if let (Some("end"), Some("fun")) = (t1.word_str(self.db), t2.word_str(self.db)) {
                        remaining_lines.next(); // consume "end fun" line
                        break;
                    }
                }
            }

            let (_, line) = remaining_lines.next().X();
            if !line.is_empty() {
                // Parse statement recursively to handle if/ret/etc in function body.
                let stmt = self.parse_statement(line, remaining_lines);
                body.push(stmt);
            }
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
        &mut self,
        iter: BracerIter<'db>,
    ) -> Vec<ast::FunParam<'db>> {
        let tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if tokens.is_empty() {
            return vec![];
        }

        let mut params = vec![];
        let mut tokens = tokens.into_iter().peekable();

        loop {
            // Check if we've reached the end (handles trailing comma case)
            if tokens.peek().is_none() {
                break;
            }

            // Check for parameter mode keywords
            let mode = match self.peek_word(&mut tokens) {
                Some("out") => {
                    self.eat_word(&mut tokens, "out");
                    ast::ParamMode::Out
                }
                Some("ref") => {
                    self.eat_word(&mut tokens, "ref");
                    ast::ParamMode::Ref
                }
                Some("mut") => {
                    self.eat_word(&mut tokens, "mut");
                    ast::ParamMode::Mut
                }
                _ => ast::ParamMode::In, // default
            };

            let name = self.need_name(&mut tokens);

            // Need colon
            self.need_sigil(&mut tokens, Sigil::Colon);

            let type_hint = self.parse_type_hint_and_heap(&mut tokens);

            params.push(ast::FunParam::new(self.db, name, mode, type_hint));

            // Check for comma (more params) or end
            if self.peek_sigil(&mut tokens, Sigil::Comma) {
                self.eat_sigil(&mut tokens, Sigil::Comma);
                // Continue loop to check for more params (or trailing comma)
            } else {
                break;
            }
        }

        params
    }

    fn parse_function_call_args(
        &mut self,
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

        // Parse each argument group.
        let mut args = vec![];
        for group in arg_token_groups {
            let mut group_iter = group.into_iter().peekable();
            let arg = self.parse_expr_full(&mut group_iter);
            args.push(arg);
        }

        args
    }

    fn parse_ret(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::Statement<'db> {
        self.eat_word(tokens, "ret");

        let value = self.parse_expr_full(tokens);

        ast::Statement::Ret(ast::StmtRet::new(self.db, value))
    }

    fn parse_require(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::Statement<'db> {
        self.eat_word(tokens, "require");

        match self.peek_word(tokens) {
            Some("module") => {
                self.eat_word(tokens, "module");

                // Parse 3-part path: lib/pkg/module
                let import_space = self.need_name(tokens);

                // Need forward slash
                if !self.peek_sigil(tokens, Sigil::SlashForward) {
                    let message = InternedText::new(self.db, "expected '/' after import space".S());
                    return ast::Statement::ParseError(ast::StmtParseError::new(self.db, message));
                }
                self.eat_sigil(tokens, Sigil::SlashForward);

                let package_alias = self.need_name(tokens);

                // Need forward slash
                if !self.peek_sigil(tokens, Sigil::SlashForward) {
                    let message = InternedText::new(self.db, "expected '/' after package alias".S());
                    return ast::Statement::ParseError(ast::StmtParseError::new(self.db, message));
                }
                self.eat_sigil(tokens, Sigil::SlashForward);

                let module_alias = self.need_name(tokens);

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
                self.eat_word(tokens, "data");

                let name = self.need_name(tokens);

                // Optional type hint: `: type`
                let type_hint = if self.peek_sigil(tokens, Sigil::Colon) {
                    self.eat_sigil(tokens, Sigil::Colon);
                    Some(self.parse_type_hint_and_heap(tokens))
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
                let message = InternedText::new(self.db, "expected 'module' or 'data' after 'require'".S());
                ast::Statement::ParseError(ast::StmtParseError::new(self.db, message))
            }
        }
    }

    fn parse_import(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::Statement<'db> {
        self.eat_word(tokens, "import");

        // Parse module name.
        let module_name = self.need_name(tokens);

        // Need dot sigil.
        if !self.peek_sigil(tokens, Sigil::Dot) {
            let message = InternedText::new(self.db, "expected '.' after module name".S());
            return ast::Statement::ParseError(ast::StmtParseError::new(self.db, message));
        }
        self.eat_sigil(tokens, Sigil::Dot);

        // Parse item name.
        let item_name = self.need_name(tokens);

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
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        remaining_lines: &mut Peekable<impl Iterator<Item = (usize, Vec<TreeToken<'db>>)>>,
    ) -> ast::Statement<'db> {
        self.eat_word(tokens, "if");

        // Parse condition expression.
        let condition = self.parse_expr_full(tokens);

        // Parse optional then binding: |identifier|
        let then_binding = if self.peek_sigil(tokens, Sigil::Pipe) {
            self.eat_sigil(tokens, Sigil::Pipe);
            let binding = self.need_name(tokens);
            self.need_sigil(tokens, Sigil::Pipe);
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
                let stmt = self.parse_statement(line, remaining_lines);
                then_body.push(stmt);
            }
        }

        // Parse else binding and body if we found "else".
        let (else_binding, else_body) = if found_else {
            // Consume the "else" line and parse any binding.
            let (_, else_line) = remaining_lines.next().X();
            let mut else_tokens = else_line.into_iter().peekable();
            self.eat_word(&mut else_tokens, "else");

            // Parse optional else binding: |identifier|
            let else_binding = if self.peek_sigil(&mut else_tokens, Sigil::Pipe) {
                self.eat_sigil(&mut else_tokens, Sigil::Pipe);
                let binding = self.need_name(&mut else_tokens);
                self.need_sigil(&mut else_tokens, Sigil::Pipe);
                Some(binding)
            } else {
                None
            };

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
                    let stmt = self.parse_statement(line, remaining_lines);
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

    // Delegate to datalit parser for type hints and expressions
    fn parse_type_hint_and_heap(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> datalit::ast::TypeHintAndHeap<'db> {
        // For now, do simple type parsing inline without delegating
        // This avoids the issue of the datalit parser consuming too many tokens

        // Heap sigils: @ for local, # for global.
        // If omitted, defaults to Heap::Omitted (inferred).
        let heap = if self.peek_sigil(tokens, Sigil::At) {
            self.eat_sigil(tokens, Sigil::At);
            datalit::ast::Heap::Local
        } else if self.peek_sigil(tokens, Sigil::Hash) {
            self.eat_sigil(tokens, Sigil::Hash);
            datalit::ast::Heap::Global
        } else {
            // No heap sigil - default to Omitted (inferred).
            datalit::ast::Heap::Omitted
        };

        // Check for ? or ! prefix for Option/Result types.
        let type_hint = if self.peek_sigil(tokens, Sigil::Question) {
            self.eat_sigil(tokens, Sigil::Question);
            let inner_type = self.parse_type_hint_and_heap(tokens);
            datalit::ast::TypeHint::Option(datalit::ast::TypeHintOption::new(self.db, inner_type))
        } else if self.peek_sigil(tokens, Sigil::Exclamation) {
            self.eat_sigil(tokens, Sigil::Exclamation);
            let inner_type = self.parse_type_hint_and_heap(tokens);
            datalit::ast::TypeHint::Result(datalit::ast::TypeHintResult::new(self.db, inner_type))
        } else if matches!(tokens.peek(), Some(TreeToken::Branch(Sigil::BracketOpen, _))) {
            // List type: [element_type]
            // Delegate to a helper function to avoid monomorphization issues.
            match tokens.next() {
                Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => {
                    self.parse_list_type_hint(iter)
                }
                _ => {
                    let message = InternedText::new(self.db, "expected list type".S());
                    datalit::ast::TypeHint::ParseError(datalit::ast::TypeHintParseError::new(self.db, message))
                }
            }
        } else {
            // Parse base type keyword
            match self.peek_word(tokens) {
                Some("bool") => { self.eat_word(tokens, "bool"); datalit::ast::TypeHint::Bool }
                Some("u8") => { self.eat_word(tokens, "u8"); datalit::ast::TypeHint::U8 }
                Some("i8") => { self.eat_word(tokens, "i8"); datalit::ast::TypeHint::I8 }
                Some("u16") => { self.eat_word(tokens, "u16"); datalit::ast::TypeHint::U16 }
                Some("i16") => { self.eat_word(tokens, "i16"); datalit::ast::TypeHint::I16 }
                Some("u32") => { self.eat_word(tokens, "u32"); datalit::ast::TypeHint::U32 }
                Some("i32") => { self.eat_word(tokens, "i32"); datalit::ast::TypeHint::I32 }
                Some("u64") => { self.eat_word(tokens, "u64"); datalit::ast::TypeHint::U64 }
                Some("i64") => { self.eat_word(tokens, "i64"); datalit::ast::TypeHint::I64 }
                Some("f32") => { self.eat_word(tokens, "f32"); datalit::ast::TypeHint::F32 }
                Some("int") => { self.eat_word(tokens, "int"); datalit::ast::TypeHint::Int }
                Some("string") => { self.eat_word(tokens, "string"); datalit::ast::TypeHint::String }
                Some("data") => { self.eat_word(tokens, "data"); datalit::ast::TypeHint::Data }
                Some("error") => { self.eat_word(tokens, "error"); datalit::ast::TypeHint::Error }
                _ => {
                    let message = InternedText::new(self.db, "unknown type hint".S());
                    datalit::ast::TypeHint::ParseError(datalit::ast::TypeHintParseError::new(self.db, message))
                }
            }
        };

        datalit::ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_list_type_hint(
        &mut self,
        iter: BracerIter<'db>,
    ) -> datalit::ast::TypeHint<'db> {
        let tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        if tokens.is_empty() {
            let message = InternedText::new(self.db, "list type must have element type".S());
            return datalit::ast::TypeHint::ParseError(datalit::ast::TypeHintParseError::new(self.db, message));
        }
        let mut tokens = tokens.into_iter().peekable();
        let element_type = self.parse_type_hint_and_heap(&mut tokens);
        datalit::ast::TypeHint::List(datalit::ast::TypeHintList::new(self.db, element_type))
    }

    fn parse_expr_full(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::ExprFun<'db> {
        self.parse_expr_binop(tokens, 0)
    }

    // Parse binary operations with precedence climbing algorithm.
    fn parse_expr_binop(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        min_precedence: u8,
    ) -> ast::ExprFun<'db> {
        let mut lhs = self.parse_expr_primary(tokens);

        loop {
            // Check for binary operator
            let op = match self.peek_binop(tokens) {
                Some(op) => op,
                None => break,
            };

            let precedence = Self::binop_precedence(op);
            if precedence < min_precedence {
                break;
            }

            // Consume the operator
            self.eat_binop(tokens, op);

            // Parse right-hand side with higher precedence
            let rhs = self.parse_expr_binop(tokens, precedence + 1);

            lhs = ast::ExprFun::new(
                self.db,
                ast::ExprFunKind::BinOp(ast::ExprBinOp::new(self.db, op, lhs, rhs))
            );
        }

        lhs
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
            ast::BinOp::AddOptional | ast::BinOp::SubOptional |
            ast::BinOp::AddSaturating | ast::BinOp::SubSaturating => 2,

            // Multiplication and division (highest precedence)
            ast::BinOp::Mul | ast::BinOp::Div |
            ast::BinOp::MulChecked | ast::BinOp::DivChecked |
            ast::BinOp::MulOptional | ast::BinOp::DivOptional |
            ast::BinOp::MulSaturating | ast::BinOp::DivSaturating => 3,
        }
    }

    // Peek at the next token(s) and return the binary operator if present.
    fn peek_binop(
        &self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> Option<ast::BinOp> {
        match tokens.peek() {
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

                    TokenKind::Sigil(Sigil::PlusBar) => Some(ast::BinOp::AddSaturating),
                    TokenKind::Sigil(Sigil::MinusBar) => Some(ast::BinOp::SubSaturating),
                    TokenKind::Sigil(Sigil::StarBar) => Some(ast::BinOp::MulSaturating),
                    TokenKind::Sigil(Sigil::SlashBar) => Some(ast::BinOp::DivSaturating),

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
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        expected_op: ast::BinOp,
    ) {
        // Peek to verify we're consuming the right operator
        if let Some(op) = self.peek_binop(tokens) {
            if op == expected_op {
                tokens.next(); // consume the operator token
                return;
            }
        }
        panic!("expected binary operator {:?}", expected_op);
    }

    // Parse primary expression (literals, names, parenthesized expressions)
    fn parse_expr_primary(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::ExprFun<'db> {
        // Check if it starts with a heap sigil (@ or #) - if so, it's definitely a datalit expression.
        if self.peek_sigil(tokens, Sigil::At) || self.peek_sigil(tokens, Sigil::Hash) {
            return self.parse_datalit_expr(tokens);
        }

        // Peek the next token to determine how to parse this expression.
        match tokens.peek() {
            Some(TreeToken::Token(token)) => {
                // If it's a word token, check if it's a datalit keyword or a datafun name.
                match token.kind(self.db) {
                    TokenKind::Word => {
                        if let Some(word) = token.word_str(self.db) {
                            // Check against datalit keywords.
                            match word {
                                "true" | "false" | "tuple" | "struct" | "enum" |
                                "option" | "result" | "error" | "map" | "set" | "none" | "data" => {
                                    self.parse_datalit_expr(tokens)
                                }
                                num if num.chars().all(|c| char::is_ascii_digit(&c)) => {
                                    self.parse_datalit_expr(tokens)
                                }
                                _ => {
                                    // It's a datafun name or function call.
                                    tokens.next(); // consume the token
                                    let name = InternedText::new(self.db, word.S());

                                    // Check if followed by parentheses (function call).
                                    if let Some(TreeToken::Branch(Sigil::ParenOpen, args_iter)) = tokens.peek() {
                                        // It's a function call.
                                        let args_iter = match tokens.next() {
                                            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => iter,
                                            _ => unreachable!(),
                                        };
                                        let args = self.parse_function_call_args(args_iter);
                                        ast::ExprFun::new(
                                            self.db,
                                            ast::ExprFunKind::FunctionCall(
                                                ast::ExprFunctionCall::new(self.db, name, args)
                                            )
                                        )
                                    } else {
                                        // It's just a variable name.
                                        ast::ExprFun::new(
                                            self.db,
                                            ast::ExprFunKind::Name(name)
                                        )
                                    }
                                }
                            }
                        } else {
                            tokens.next();
                            let message = InternedText::new(self.db, "unexpected token in expression".S());
                            ast::ExprFun::new(
                                self.db,
                                ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(self.db, message))
                            )
                        }
                    }
                    _ => {
                        // Not a word, parse as datalit (might be a number literal, string, etc.).
                        self.parse_datalit_expr(tokens)
                    }
                }
            }
            Some(TreeToken::Branch(..)) => {
                // Branches like (), {}, [] are datalit expressions.
                self.parse_datalit_expr(tokens)
            }
            None => {
                let message = InternedText::new(self.db, "expected expression".S());
                ast::ExprFun::new(
                    self.db,
                    ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(self.db, message))
                )
            }
        }
    }

    // Helper to parse a datalit expression by delegating to the datalit parser.
    fn parse_datalit_expr(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> ast::ExprFun<'db> {
        // Collect tokens until we hit a datafun operator or end of tokens.
        // Datalit expressions don't contain binary operators, so we stop at +, -, *, /, etc.
        // Exception: - can appear as part of a negative literal right after @ or # sigil.
        let mut datalit_tokens = Vec::new();
        let mut just_saw_heap_sigil = false;

        // Collect tokens for the datalit expression, stopping at datafun operators.
        while let Some(token) = tokens.peek() {
            // Track if we just consumed a heap sigil.
            let is_heap_sigil = matches!(
                token,
                TreeToken::Token(t) if matches!(
                    t.kind(self.db),
                    TokenKind::Sigil(Sigil::At) | TokenKind::Sigil(Sigil::Hash)
                )
            );

            // Check if this is a datafun binary operator.
            // Special case: - right after @ or # is part of a negative literal, not a binop.
            let is_binop = match token {
                TreeToken::Token(t) => {
                    let sigil = t.kind(self.db);
                    // Minus after heap sigil is part of negative literal.
                    if just_saw_heap_sigil && matches!(sigil, TokenKind::Sigil(Sigil::Minus)) {
                        false
                    } else {
                        matches!(
                            sigil,
                            TokenKind::Sigil(Sigil::Plus) |
                            TokenKind::Sigil(Sigil::Minus) |
                            TokenKind::Sigil(Sigil::Star) |
                            TokenKind::Sigil(Sigil::SlashForward) |
                            TokenKind::Sigil(Sigil::PlusExclamation) |
                            TokenKind::Sigil(Sigil::MinusExclamation) |
                            TokenKind::Sigil(Sigil::StarExclamation) |
                            TokenKind::Sigil(Sigil::SlashExclamation) |
                            TokenKind::Sigil(Sigil::PlusQuestion) |
                            TokenKind::Sigil(Sigil::MinusQuestion) |
                            TokenKind::Sigil(Sigil::StarQuestion) |
                            TokenKind::Sigil(Sigil::SlashQuestion) |
                            TokenKind::Sigil(Sigil::PlusBar) |
                            TokenKind::Sigil(Sigil::MinusBar) |
                            TokenKind::Sigil(Sigil::StarBar) |
                            TokenKind::Sigil(Sigil::SlashBar) |
                            TokenKind::Sigil(Sigil::EqualsEquals) |
                            TokenKind::Sigil(Sigil::ExclamationEquals) |
                            TokenKind::Sigil(Sigil::DotLess) |
                            TokenKind::Sigil(Sigil::DotGreater) |
                            TokenKind::Sigil(Sigil::LessEquals) |
                            TokenKind::Sigil(Sigil::GreaterEquals)
                        )
                    }
                }
                _ => false,
            };

            if is_binop {
                break;
            }

            // Not a binop, so consume this token for the datalit expression.
            datalit_tokens.push(tokens.next().unwrap());
            just_saw_heap_sigil = is_heap_sigil;
        }

        // Parse the collected tokens as a datalit expression.
        let datalit_expr = datalit::parser::parse_from_tokens(self.db, datalit_tokens);

        ast::ExprFun::new(
            self.db,
            ast::ExprFunKind::Datalit(datalit_expr)
        )
    }

    // Token manipulation helpers
    fn peek_word(
        &self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> Option<&'db str> {
        match tokens.peek() {
            Some(TreeToken::Token(token)) => token.word_str(self.db),
            _ => None,
        }
    }

    fn eat_word(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        word: &str,
    ) {
        match tokens.next() {
            Some(TreeToken::Token(token)) => {
                if token.word_str(self.db) != Some(word) {
                    panic!("expected word '{}'", word);
                }
            }
            _ => panic!("expected word '{}'", word),
        }
    }

    fn need_name(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> InternedText<'db> {
        match tokens.next() {
            Some(TreeToken::Token(token)) => {
                match token.word_str(self.db) {
                    Some(word) => InternedText::new(self.db, word.S()),
                    None => {
                        let text = token.text(self.db).as_str(self.db);
                        panic!("expected name, got token: {}", text)
                    }
                }
            }
            Some(TreeToken::Branch(..)) => panic!("expected name, got branch"),
            None => panic!("expected name, got end of input"),
        }
    }

    fn peek_sigil(
        &self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        sigil: Sigil,
    ) -> bool {
        match tokens.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db), TokenKind::Sigil(s) if s == sigil)
            }
            _ => false,
        }
    }

    fn eat_sigil(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        sigil: Sigil,
    ) {
        match tokens.next() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Sigil(s) if s == sigil => return,
                    _ => {}
                }
            }
            _ => {}
        }
        panic!("expected sigil {}", sigil.as_str());
    }

    fn need_sigil(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
        sigil: Sigil,
    ) {
        self.eat_sigil(tokens, sigil)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_let_simple() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @42"));
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed as datalit
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_expr_binop_checked() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a +! b"));
        let script = parse(db, source);
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
        let script = parse(db, source);
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
    fn test_parse_expr_binop_saturating() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = a +| b"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::BinOp(binop) => {
                        assert_eq!(binop.op(db), ast::BinOp::AddSaturating);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Fun(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "increment");
                assert_eq!(stmt.body(db).len(), 1);
                // Check the body has a ret statement with binop
                match &stmt.body(db)[0] {
                    ast::Statement::Ret(ret) => {
                        match ret.value(db).expr(db) {
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
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
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed tuple as datalit
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_list() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @[1, 2, 3]"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed list as datalit
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_map() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @map { @1 = @10, @2 = @20 }"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed map as datalit
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_nested_tuple_in_list() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @[(1, 2), (3, 4)]"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed nested structure
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_nested_list_in_tuple() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @(@[@1, @2, @3], @100)"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed nested list in tuple
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_set() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @set { @1, @2, @3 }"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed set as datalit
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_parse_datalit_deeply_nested() {
        let ref db = crate::Database::default();
        let source = Source::new(db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
                match stmt.value(db).expr(db) {
                    ast::ExprFunKind::Datalit(_) => {
                        // Successfully parsed deeply nested structure
                    }
                    _ => panic!("expected datalit expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }
}
