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

        // Parse the value expression using datalit parser
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
                let mut line_tokens = line.into_iter().peekable();
                let stmt = match self.peek_word(&mut line_tokens) {
                    Some("ret") => self.parse_ret(&mut line_tokens),
                    _ => {
                        let message = InternedText::new(self.db, "unexpected statement in fun body".S());
                        ast::Statement::ParseError(ast::StmtParseError::new(self.db, message))
                    }
                };
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

        let kind = match self.peek_word(tokens) {
            Some("module") => {
                self.eat_word(tokens, "module");
                ast::RequireKind::Module
            }
            Some("data") => {
                self.eat_word(tokens, "data");
                ast::RequireKind::Data
            }
            _ => {
                let message = InternedText::new(self.db, "expected 'module' or 'data' after 'require'".S());
                return ast::Statement::ParseError(ast::StmtParseError::new(self.db, message));
            }
        };

        let name = self.need_name(tokens);

        // Optional type hint: `: type`
        let type_hint = if self.peek_sigil(tokens, Sigil::Colon) {
            self.eat_sigil(tokens, Sigil::Colon);
            Some(self.parse_type_hint_and_heap(tokens))
        } else {
            None
        };

        ast::Statement::Require(ast::StmtRequire::new(
            self.db,
            kind,
            name,
            type_hint,
        ))
    }

    // Delegate to datalit parser for type hints and expressions
    fn parse_type_hint_and_heap(
        &mut self,
        tokens: &mut Peekable<impl Iterator<Item = TreeToken<'db>>>,
    ) -> datalit::ast::TypeHintAndHeap<'db> {
        // For now, do simple type parsing inline without delegating
        // This avoids the issue of the datalit parser consuming too many tokens
        // fixme

        // Heap sigils: @ for local, # for global.
        let heap = if self.peek_sigil(tokens, Sigil::At) {
            self.eat_sigil(tokens, Sigil::At);
            datalit::ast::Heap::Local
        } else if self.peek_sigil(tokens, Sigil::Hash) {
            self.eat_sigil(tokens, Sigil::Hash);
            datalit::ast::Heap::Global
        } else if self.peek_sigil(tokens, Sigil::Exclamation) || self.peek_sigil(tokens, Sigil::Question) {
            // Result/option types can omit heap
            datalit::ast::Heap::Omitted
        } else {
            let message = InternedText::new(self.db, "expected heap sigil @ or # before type".S());
            let error_node = datalit::ast::TypeHint::ParseError(
                datalit::ast::TypeHintParseError::new(self.db, message)
            );
            return datalit::ast::TypeHintAndHeap::new(self.db, datalit::ast::Heap::Omitted, error_node);
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
        } else {
            // Parse base type keyword
            match self.peek_word(tokens) {
                Some("bool") => { self.eat_word(tokens, "bool"); datalit::ast::TypeHint::Bool }
                Some("u32") => { self.eat_word(tokens, "u32"); datalit::ast::TypeHint::U32 }
                Some("u64") => { self.eat_word(tokens, "u64"); datalit::ast::TypeHint::U32 } // TODO: add U64 to datalit
                Some("u8") => { self.eat_word(tokens, "u8"); datalit::ast::TypeHint::U32 } // TODO: add U8 to datalit
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
        // Check if it starts with a heap sigil (@ or #) - if so, it's a datalit expression
        if self.peek_sigil(tokens, Sigil::At) || self.peek_sigil(tokens, Sigil::Hash) {
            // Parse as datalit expression
            let remaining: Vec<TreeToken<'db>> = tokens.collect();
            let mut datalit_parser = DatalitParser {
                db: self.db,
                tokens: remaining,
                pos: 0,
            };
            let datalit_expr = datalit_parser.parse_expr_full();
            return ast::ExprFun::new(
                self.db,
                ast::ExprFunKind::Datalit(datalit_expr)
            );
        }

        // Check if it's a bare name/identifier
        match tokens.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        if let Some(word) = token.word_str(self.db) {
                            // Check if it's a number literal - parse as datalit with omitted heap
                            if word.chars().all(|c| c.is_ascii_digit()) {
                                let remaining: Vec<TreeToken<'db>> = tokens.collect();
                                let mut datalit_parser = DatalitParser {
                                    db: self.db,
                                    tokens: remaining,
                                    pos: 0,
                                };
                                let datalit_expr = datalit_parser.parse_expr_full();
                                ast::ExprFun::new(
                                    self.db,
                                    ast::ExprFunKind::Datalit(datalit_expr)
                                )
                            } else {
                                // It's a bare name/identifier
                                tokens.next(); // consume the token
                                let name = InternedText::new(self.db, word.S());
                                ast::ExprFun::new(
                                    self.db,
                                    ast::ExprFunKind::Name(name)
                                )
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
                        tokens.next();
                        let message = InternedText::new(self.db, "unexpected token in expression".S());
                        ast::ExprFun::new(
                            self.db,
                            ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(self.db, message))
                        )
                    }
                }
            }
            Some(TreeToken::Branch(..)) => {
                // Could be tuple, struct, etc - parse as datalit
                let remaining: Vec<TreeToken<'db>> = tokens.collect();
                let mut datalit_parser = DatalitParser {
                    db: self.db,
                    tokens: remaining,
                    pos: 0,
                };
                let datalit_expr = datalit_parser.parse_expr_full();
                ast::ExprFun::new(
                    self.db,
                    ast::ExprFunKind::Datalit(datalit_expr)
                )
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

// Helper struct to reuse datalit parsers
struct DatalitParser<'db> {
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    pos: usize,
}

impl<'db> DatalitParser<'db> {
    fn parse_type_hint_and_heap(&mut self) -> datalit::ast::TypeHintAndHeap<'db> {
        // Heap sigils: @ for local, # for global.
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            datalit::ast::Heap::Local
        } else if self.peek_sigil(Sigil::Hash) {
            self.eat_sigil(Sigil::Hash);
            datalit::ast::Heap::Global
        } else {
            // For result/option types, we might see ! or ? first
            if self.peek_sigil(Sigil::Exclamation) || self.peek_sigil(Sigil::Question) {
                datalit::ast::Heap::Omitted
            } else {
                let message = InternedText::new(self.db, "expected heap sigil @ or # before type".S());
                let error_node = datalit::ast::TypeHint::ParseError(
                    datalit::ast::TypeHintParseError::new(self.db, message)
                );
                return datalit::ast::TypeHintAndHeap::new(self.db, datalit::ast::Heap::Omitted, error_node);
            }
        };
        let type_hint = self.parse_type_hint();
        datalit::ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_type_hint(&mut self) -> datalit::ast::TypeHint<'db> {
        // Check for ? or ! prefix for Option/Result types.
        if self.peek_sigil(Sigil::Question) {
            self.eat_sigil(Sigil::Question);
            let inner_type = self.parse_type_hint_and_heap();
            return datalit::ast::TypeHint::Option(datalit::ast::TypeHintOption::new(self.db, inner_type));
        } else if self.peek_sigil(Sigil::Exclamation) {
            self.eat_sigil(Sigil::Exclamation);
            let inner_type = self.parse_type_hint_and_heap();
            return datalit::ast::TypeHint::Result(datalit::ast::TypeHintResult::new(self.db, inner_type));
        }

        // Parse base type.
        match self.peek_word() {
            Some("bool") => { self.eat_word("bool"); datalit::ast::TypeHint::Bool }
            Some("u32") => { self.eat_word("u32"); datalit::ast::TypeHint::U32 }
            Some("u64") => { self.eat_word("u64"); datalit::ast::TypeHint::U32 } // TODO: add U64
            Some("u8") => { self.eat_word("u8"); datalit::ast::TypeHint::U32 } // TODO: add U8
            Some("f32") => { self.eat_word("f32"); datalit::ast::TypeHint::F32 }
            Some("int") => { self.eat_word("int"); datalit::ast::TypeHint::Int }
            Some("string") => { self.eat_word("string"); datalit::ast::TypeHint::String }
            Some("data") => { self.eat_word("data"); datalit::ast::TypeHint::Data }
            Some("error") => { self.eat_word("error"); datalit::ast::TypeHint::Error }
            _ => {
                let message = InternedText::new(self.db, "unknown type hint".S());
                datalit::ast::TypeHint::ParseError(datalit::ast::TypeHintParseError::new(self.db, message))
            }
        }
    }

    fn parse_expr_full(&mut self) -> datalit::ast::ExprFull<'db> {
        // Check for `: type / expr` pattern.
        if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            self.need_sigil(Sigil::SlashForward);
            let expr = self.parse_expr_and_heap();
            datalit::ast::ExprFull::new(self.db, Some(type_hint), expr)
        } else {
            // No type hint, just parse expression.
            let expr = self.parse_expr_and_heap();
            datalit::ast::ExprFull::new(self.db, None, expr)
        }
    }

    fn parse_expr_and_heap(&mut self) -> datalit::ast::ExprAndHeap<'db> {
        // For now, just parse simple expressions
        // In reality, we'd delegate to the full datalit parser
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            datalit::ast::Heap::Local
        } else if self.peek_sigil(Sigil::Hash) {
            self.eat_sigil(Sigil::Hash);
            datalit::ast::Heap::Global
        } else {
            datalit::ast::Heap::Omitted
        };

        // Parse a simple literal or identifier
        let expr = match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        self.next();
                        let word = token.word_str(self.db).X();
                        if word.chars().all(|c| c.is_ascii_digit()) {
                            let value = InternedText::new(self.db, word.S());
                            datalit::ast::Expr::Int(datalit::ast::ExprInt::new(self.db, value))
                        } else {
                            let message = InternedText::new(self.db, "unexpected identifier in expression".S());
                            datalit::ast::Expr::ParseError(datalit::ast::ExprParseError::new(self.db, message))
                        }
                    }
                    _ => {
                        let message = InternedText::new(self.db, "unexpected token in expression".S());
                        datalit::ast::Expr::ParseError(datalit::ast::ExprParseError::new(self.db, message))
                    }
                }
            }
            _ => {
                let message = InternedText::new(self.db, "unexpected token in expression".S());
                datalit::ast::Expr::ParseError(datalit::ast::ExprParseError::new(self.db, message))
            }
        };

        datalit::ast::ExprAndHeap::new(self.db, heap, expr)
    }

    // Token manipulation helpers
    fn peek(&self) -> Option<TreeToken<'db>> {
        self.tokens.get(self.pos).cloned()
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
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
                match token.word_str(self.db) {
                    Some(w) if w == word => return,
                    _ => {}
                }
            }
            _ => {}
        }
        panic!("expected word '{}'", word);
    }

    fn peek_sigil(&self, sigil: Sigil) -> bool {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db), TokenKind::Sigil(s) if s == sigil)
            }
            _ => false,
        }
    }

    fn eat_sigil(&mut self, sigil: Sigil) {
        match self.next() {
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

    fn need_sigil(&mut self, sigil: Sigil) {
        self.eat_sigil(sigil)
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
        let source = Source::new(db, S("require module std"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 1);
        match &statements[0] {
            ast::Statement::Require(stmt) => {
                assert_eq!(stmt.kind(db), ast::RequireKind::Module);
                assert_eq!(stmt.name(db).as_str(db), "std");
                assert!(stmt.type_hint(db).is_none());
            }
            _ => panic!("expected require statement"),
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
        let source = Source::new(db, S("require module std; let x = @42"));
        let script = parse(db, source);
        let statements = script.statements(db);
        assert_eq!(statements.len(), 2);
        match &statements[0] {
            ast::Statement::Require(stmt) => {
                assert_eq!(stmt.kind(db), ast::RequireKind::Module);
                assert_eq!(stmt.name(db).as_str(db), "std");
            }
            _ => panic!("expected require statement"),
        }
        match &statements[1] {
            ast::Statement::Let(stmt) => {
                assert_eq!(stmt.name(db).as_str(db), "x");
            }
            _ => panic!("expected let statement"),
        }
    }
}
