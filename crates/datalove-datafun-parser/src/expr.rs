//! Expression parsing.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_expr_full(&mut self) -> ast::ExprFun<'db> {
        // Note: span recording now happens in create_expr for each expression.
        self.parse_expr_binop(0)
    }

    /// Parse binary operations with precedence climbing algorithm.
    fn parse_expr_binop(&mut self, min_precedence: u8) -> ast::ExprFun<'db> {
        // Track start position for span.
        let start_pos = self.current_byte_pos();
        let text = self.source_text();

        let mut lhs = self.parse_expr_primary();

        // Check for postfix try operators (? and !).
        // These have highest precedence and are parsed before binary operators.
        lhs = self.parse_postfix_try_operators(lhs);

        loop {
            // Check for binary operator.
            let op = match self.peek_binop() {
                Some(op) => op,
                None => break,
            };

            let precedence = Self::binop_precedence(op);
            if precedence < min_precedence {
                break;
            }

            // Consume the operator.
            self.eat_binop(op);

            // Parse right-hand side with higher precedence.
            let rhs = self.parse_expr_binop(precedence + 1);

            // Create BinOp with span covering entire expression.
            let end_pos = self.last_byte_end();
            let span = start_pos..end_pos;
            lhs = self.create_expr(
                ast::ExprFunKind::BinOp(ast::ExprBinOp::new(self.db, op, lhs, rhs)),
                TextSpan::new(text, span),
            );
        }

        lhs
    }

    /// Parse postfix try operators (? and !).
    ///
    /// These are postfix operators that unwrap Option/Result with early return.
    /// Takes the start position of the inner expression for span tracking.
    fn parse_postfix_try_operators(&mut self, mut expr: ast::ExprFun<'db>) -> ast::ExprFun<'db> {
        loop {
            match self.peek() {
                Some(TreeToken::Token(token)) => {
                    let TextSpan { text, span: op_span } = self.extract_text_span(&TreeToken::Token(*token));
                    match token.kind(self.db) {
                        TokenKind::Sigil(Sigil::Question) => {
                            self.next(); // consume ?
                            let end_pos = self.last_byte_end();
                            let span = op_span.start..end_pos;
                            expr = self.create_expr(
                                ast::ExprFunKind::TryOption(ast::ExprTryOption::new(self.db, expr)),
                                TextSpan::new(text, span),
                            );
                        }
                        TokenKind::Sigil(Sigil::Exclamation) => {
                            self.next(); // consume !
                            let end_pos = self.last_byte_end();
                            let span = op_span.start..end_pos;
                            expr = self.create_expr(
                                ast::ExprFunKind::TryResult(ast::ExprTryResult::new(self.db, expr)),
                                TextSpan::new(text, span),
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

    /// Get operator precedence (higher number = higher precedence).
    fn binop_precedence(op: ast::BinOp) -> u8 {
        match op {
            // Comparison operators (lowest precedence).
            ast::BinOp::Eq | ast::BinOp::Ne |
            ast::BinOp::Lt | ast::BinOp::Gt |
            ast::BinOp::Le | ast::BinOp::Ge => 1,

            // Addition and subtraction (all variants).
            ast::BinOp::Add | ast::BinOp::Sub |
            ast::BinOp::AddChecked | ast::BinOp::SubChecked |
            ast::BinOp::AddOptional | ast::BinOp::SubOptional => 2,

            // Multiplication and division (highest precedence).
            ast::BinOp::Mul | ast::BinOp::Div |
            ast::BinOp::MulChecked | ast::BinOp::DivChecked |
            ast::BinOp::MulOptional | ast::BinOp::DivOptional => 3,
        }
    }

    /// Peek at the next token(s) and return the binary operator if present.
    fn peek_binop(&self) -> Option<ast::BinOp> {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    // Two-character operators.
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

                    // Single-character operators (basic arithmetic).
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

    /// Consume the operator token(s).
    fn eat_binop(&mut self, expected_op: ast::BinOp) {
        // Peek to verify we're consuming the right operator.
        if let Some(op) = self.peek_binop() {
            if op == expected_op {
                self.next(); // consume the operator token
                return;
            }
        }
        panic!("expected binary operator {:?}", expected_op);
    }

    /// Parse primary expression (literals, names, parenthesized expressions).
    pub(super) fn parse_expr_primary(&mut self) -> ast::ExprFun<'db> {
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
                                    let ts = self.peek_text_span();
                                    let expr_kind = self.parse_lit_expr(datalit::ast::Heap::Omitted, None);
                                    self.create_expr(expr_kind, ts)
                                }
                                // some/ok/er are always keywords - they require a payload expression.
                                "some" | "ok" | "er" => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let ts = self.peek_text_span();
                                    self.next(); // consume the keyword
                                    let payload = self.parse_expr_primary();
                                    let heap = datalit::ast::Heap::Omitted;
                                    let expr_kind = match word {
                                        "some" => ast::ExprFunKind::Some(ast::ExprSome::new(self.db, heap, None, payload)),
                                        "ok" => ast::ExprFunKind::Ok(ast::ExprOk::new(self.db, heap, None, payload)),
                                        "er" => ast::ExprFunKind::Er(ast::ExprEr::new(self.db, heap, None, payload)),
                                        _ => unreachable!(),
                                    };
                                    self.create_expr(expr_kind, ts)
                                }
                                num if num.chars().all(|c| char::is_ascii_digit(&c)) => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let ts = self.peek_text_span();
                                    let expr_kind = self.parse_lit_expr(datalit::ast::Heap::Omitted, None);
                                    self.create_expr(expr_kind, ts)
                                }
                                _ => {
                                    // It's a datafun name or function call.
                                    // Capture span before consuming token.
                                    let ts = self.peek_text_span();
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
                                            ts
                                        )
                                    } else {
                                        // It's just a variable name.
                                        self.create_expr(
                                            ast::ExprFunKind::Name(name),
                                            ts
                                        )
                                    }
                                }
                            }
                        } else {
                            let ts = self.peek_text_span();
                            self.next();
                            self.emit_expr_error(ts,
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
                        let ts = self.peek_text_span();
                        self.emit_expr_error(ts,
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
                    let ts = self.peek_text_span();
                    let expr_kind = self.parse_lit_expr(datalit::ast::Heap::Omitted, None);
                    self.create_expr(expr_kind, ts)
                }
            }
            None => {
                let ts = self.peek_text_span();
                self.emit_expr_error(ts,
                    "expected expression",
                    "P008",
                    "expected expression"
                )
            }
        }
    }

    pub(super) fn parse_function_call_args(&self, iter: BracerIter<'db>) -> Vec<ast::ExprFun<'db>> {
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
            let mut sub = Parser::new(self.db, group);
            let arg = sub.parse_expr_full();
            sub.error_if_not_exhausted();
            args.push(arg);
        }

        args
    }

    /// Parse a datafun tuple: (expr1, expr2, ...).
    pub(super) fn parse_datafun_tuple(&mut self) -> ast::ExprFun<'db> {
        // Consume the ParenOpen branch and get its contents.
        let iter = match self.next() {
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => iter,
            _ => {
                let ts = self.peek_text_span();
                return self.emit_expr_error(ts,
                    "expected tuple",
                    "P009",
                    "expected '(' to start tuple"
                );
            }
        };

        let all_tokens: Vec<TreeToken<'db>> = iter.filter_map(|t| t.without_space(self.db)).collect();
        let mut sub = Parser::new(self.db, all_tokens);
        let elements = sub.parse_comma_separated(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;

        // If there's exactly one element, treat as grouping (not tuple).
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
