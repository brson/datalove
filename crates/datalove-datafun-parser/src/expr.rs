//! Expression parsing.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
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
                ast::ExprFunKind::BinOp(ast::ExprBinOp { op, lhs, rhs }),
                TextSpan::new(text, span),
            );
        }

        lhs
    }

    /// Parse postfix operators (?, !, @, and field projections).
    ///
    /// Handles:
    /// - `?` - unwrap Option with early return
    /// - `!` - unwrap Result with early return
    /// - `@` - clone/coerce (widen or clone to fit target type)
    /// - `.field` - struct field projection
    /// - `.0` - tuple index projection
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
                                ast::ExprFunKind::TryOption(ast::ExprTryOption { operand: expr }),
                                TextSpan::new(text, span),
                            );
                        }
                        TokenKind::Sigil(Sigil::Exclamation) => {
                            self.next(); // consume !
                            let end_pos = self.last_byte_end();
                            let span = op_span.start..end_pos;
                            expr = self.create_expr(
                                ast::ExprFunKind::TryResult(ast::ExprTryResult { operand: expr }),
                                TextSpan::new(text, span),
                            );
                        }
                        TokenKind::Sigil(Sigil::At) => {
                            self.next(); // consume @
                            let end_pos = self.last_byte_end();
                            let span = op_span.start..end_pos;
                            expr = self.create_expr(
                                ast::ExprFunKind::CloneCoerce(ast::ExprCloneCoerce { operand: expr }),
                                TextSpan::new(text, span),
                            );
                        }
                        TokenKind::Sigil(Sigil::Dot) => {
                            // Note: .< and .> are already tokenized as DotLess/DotGreater,
                            // so a bare Dot is always a field projection.
                            self.next(); // consume .
                            let field = self.parse_field_selector();
                            let end_pos = self.last_byte_end();
                            let span = op_span.start..end_pos;
                            expr = self.create_expr(
                                ast::ExprFunKind::FieldProj(ast::ExprFieldProj { base: expr, field }),
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

    /// Parse a field selector (name or index) after a dot.
    fn parse_field_selector(&mut self) -> ast::FieldSelector<'db> {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).unwrap_or("");
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
                    _ => {
                        // No valid field selector - create error name.
                        let name = InternedText::new(self.db, "<error>".S());
                        ast::FieldSelector::Name(name)
                    }
                }
            }
            _ => {
                // No token after dot - create error name.
                let name = InternedText::new(self.db, "<error>".S());
                ast::FieldSelector::Name(name)
            }
        }
    }

    /// Get operator precedence (higher number = higher precedence).
    fn binop_precedence(op: ast::BinOp) -> u8 {
        match op {
            // Logical or/xor (lowest precedence).
            ast::BinOp::Or | ast::BinOp::Xor => 1,

            // Logical and.
            ast::BinOp::And => 2,

            // Comparison operators.
            ast::BinOp::Eq | ast::BinOp::Ne |
            ast::BinOp::Lt | ast::BinOp::Gt |
            ast::BinOp::Le | ast::BinOp::Ge => 3,

            // Addition and subtraction (all variants).
            ast::BinOp::Add | ast::BinOp::Sub |
            ast::BinOp::AddChecked | ast::BinOp::SubChecked |
            ast::BinOp::AddOptional | ast::BinOp::SubOptional => 4,

            // Multiplication and division (highest precedence).
            ast::BinOp::Mul | ast::BinOp::Div |
            ast::BinOp::MulChecked | ast::BinOp::DivChecked |
            ast::BinOp::MulOptional | ast::BinOp::DivOptional => 5,
        }
    }

    /// Peek at the next token(s) and return the binary operator if present.
    fn peek_binop(&self) -> Option<ast::BinOp> {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    // Keyword operators (logical).
                    TokenKind::Word => {
                        match token.word_str(self.db) {
                            Some("and") => Some(ast::BinOp::And),
                            Some("or") => Some(ast::BinOp::Or),
                            Some("xor") => Some(ast::BinOp::Xor),
                            _ => None,
                        }
                    }

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
        // Check for unary operators (-, -?, -!, not).
        if let Some(TreeToken::Token(token)) = self.peek() {
            let unary_op = match token.kind(self.db) {
                TokenKind::Sigil(Sigil::Minus) => Some(ast::UnaryOp::Neg),
                TokenKind::Sigil(Sigil::MinusQuestion) => Some(ast::UnaryOp::NegOptional),
                TokenKind::Sigil(Sigil::MinusExclamation) => Some(ast::UnaryOp::NegResult),
                TokenKind::Word => {
                    match token.word_str(self.db) {
                        Some("not") => Some(ast::UnaryOp::Not),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(op) = unary_op {
                self.next(); // Consume the operator.
                let operand = self.parse_expr_primary();
                return ast::ExprFun::new(
                    self.db,
                    self.module_id(),
                    self.current_fn_name(),
                    self.next_expr_index(),
                    ast::ExprFunKind::UnaryOp(ast::ExprUnaryOp { op, operand })
                );
            }
        }

        // Check if it starts with a type hint (`:`) - use new inline variants.
        if self.peek_colon_type_hint() {
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
                                // Standalone literals - always keywords.
                                "true" | "false" | "none" => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let ts = self.peek_text_span();
                                    let expr_kind = self.parse_lit_expr(None);
                                    self.create_expr(expr_kind, ts)
                                }
                                // some/ok/er are always keywords - they require a payload expression.
                                "some" | "ok" | "er" => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let ts = self.peek_text_span();
                                    self.next(); // consume the keyword
                                    let payload = self.parse_expr_primary();
                                    let expr_kind = match word {
                                        "some" => ast::ExprFunKind::Some(ast::ExprSome { type_hint: None, payload }),
                                        "ok" => ast::ExprFunKind::Ok(ast::ExprOk { type_hint: None, payload }),
                                        "er" => ast::ExprFunKind::Er(ast::ExprEr { type_hint: None, payload }),
                                        _ => unreachable!(),
                                    };
                                    self.create_expr(expr_kind, ts)
                                }
                                // data/error are always keywords - they require a value expression.
                                "data" | "error" => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume the keyword
                                    let value = self.parse_expr_primary();
                                    let expr_kind = match word {
                                        "data" => ast::ExprFunKind::Data(ast::ExprData { type_hint: None, value }),
                                        "error" => ast::ExprFunKind::Error(ast::ExprError { type_hint: None, value }),
                                        _ => unreachable!(),
                                    };
                                    self.create_expr(expr_kind, ts)
                                }
                                // Atom expression: `atom Name`.
                                "atom" => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume "atom"
                                    let name = match self.peek_word() {
                                        Some(w) => {
                                            let n = InternedText::new(self.db, w.S());
                                            self.next();
                                            n
                                        }
                                        None => {
                                            return self.emit_expr_error(ts,
                                                "expected name after 'atom'",
                                                "P042",
                                                "expected atom name"
                                            );
                                        }
                                    };
                                    self.create_expr(
                                        ast::ExprFunKind::Atom(ast::ExprAtom { name }),
                                        ts
                                    )
                                }
                                // Term expression: `term Name payload`.
                                "term" => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume "term"
                                    let name = match self.peek_word() {
                                        Some(w) => {
                                            let n = InternedText::new(self.db, w.S());
                                            self.next();
                                            n
                                        }
                                        None => {
                                            return self.emit_expr_error(ts,
                                                "expected name after 'term'",
                                                "P043",
                                                "expected term name"
                                            );
                                        }
                                    };
                                    let payload = self.parse_expr_primary();
                                    self.create_expr(
                                        ast::ExprFunKind::Term(ast::ExprTerm { name, payload }),
                                        ts
                                    )
                                }
                                // Enum literal expression: `enum { atom Foo }`.
                                "enum" if self.peek_second_sigil(Sigil::BraceOpen) => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume "enum"
                                    // Parse the inner variant expression in braces.
                                    let iter = match self.next() {
                                        Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
                                        _ => unreachable!(),
                                    };
                                    let mut sub = self.sub_parser(iter, None);
                                    let variant = sub.parse_expr_full();
                                    sub.error_if_not_exhausted();
                                    self.merge_from_sub(&mut sub);
                                    self.create_expr(
                                        ast::ExprFunKind::EnumLiteral(ast::ExprEnumLiteral { variant }),
                                        ts
                                    )
                                }
                                // Intrinsic call: icall name(args)
                                "icall" => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume "icall"

                                    // Parse intrinsic name.
                                    let intrinsic_name = match self.peek() {
                                        Some(TreeToken::Token(tok)) if tok.kind(self.db) == TokenKind::Word => {
                                            let name = tok.word_str(self.db).unwrap_or("");
                                            self.next(); // consume the name
                                            InternedText::new(self.db, name.to_string())
                                        }
                                        _ => {
                                            return self.emit_expr_error(ts,
                                                "expected intrinsic name after 'icall'",
                                                "P040",
                                                "expected intrinsic name"
                                            );
                                        }
                                    };

                                    // Parse arguments in parentheses.
                                    let args = match self.peek() {
                                        Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                                            let (args_iter, open_span) = match self.next() {
                                                Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                                                    let open_span = TextSpan::new(self.source_text(), open.span(self.db));
                                                    (inner, open_span)
                                                }
                                                _ => unreachable!(),
                                            };
                                            self.parse_function_call_args(args_iter, Some((open_span, "in this argument list")))
                                        }
                                        _ => {
                                            return self.emit_expr_error(ts,
                                                "expected '(' after intrinsic name",
                                                "P041",
                                                "expected '('"
                                            );
                                        }
                                    };

                                    self.create_expr(
                                        ast::ExprFunKind::IntrinsicCall(ast::ExprIntrinsicCall {
                                            name: intrinsic_name,
                                            args,
                                        }),
                                        ts
                                    )
                                }
                                num if Self::is_numeric_literal(num) => {
                                    // Capture span before parsing for diagnostic reporting.
                                    let ts = self.peek_text_span();
                                    let expr_kind = self.parse_lit_expr(None);
                                    self.create_expr(expr_kind, ts)
                                }
                                _ => {
                                    // It's a datafun name or function call.
                                    // Capture span before consuming token.
                                    let ts = self.peek_text_span();
                                    self.next(); // consume the token
                                    let name = InternedText::new(self.db, word.S());

                                    // Check if followed by parentheses (function call).
                                    if let Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) = self.peek() {
                                        // It's a function call.
                                        let (args_iter, open_span) = match self.next() {
                                            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                                                let open_span = TextSpan::new(self.source_text(), open.span(self.db));
                                                (inner, open_span)
                                            }
                                            _ => unreachable!(),
                                        };
                                        let args = self.parse_function_call_args(args_iter, Some((open_span, "in this argument list")));
                                        // For function calls, span should include the parens, but for now just use the name span.
                                        let call = ast::ExprFunctionCall::new(
                                            self.db,
                                            self.module_id(),
                                            self.current_fn_name(),
                                            self.next_call_index(),
                                            name,
                                            args,
                                        );
                                        self.create_expr(
                                            ast::ExprFunKind::FunctionCall(call),
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
                            self.module_id(),
                            self.current_fn_name(),
                            self.next_expr_index(),
                            ast::ExprFunKind::String(ast::ExprString {
                                type_hint: None,
                                value
                            })
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
            Some(TreeToken::Branch { sigil, .. }) => {
                // Check if it's a tuple (ParenOpen) - parse as datafun tuple.
                // Other branches like {}, [] are literal expressions.
                if matches!(sigil, Sigil::ParenOpen) {
                    self.parse_datafun_tuple()
                } else {
                    // Capture span before parsing for diagnostic reporting.
                    let ts = self.peek_text_span();
                    let expr_kind = self.parse_lit_expr(None);
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

    pub(super) fn parse_function_call_args(
        &mut self,
        iter: BracerIter<'db>,
        context: Option<(TextSpan<'db>, &'static str)>,
    ) -> Vec<ast::ExprFun<'db>> {
        let mut sub = self.sub_parser(iter, context);
        let args = sub.parse_comma_separated(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
        args
    }

    /// Parse a datafun tuple: (expr1, expr2, ...).
    ///
    /// Caller must have already peeked and confirmed a `ParenOpen` branch.
    pub(super) fn parse_datafun_tuple(&mut self) -> ast::ExprFun<'db> {
        // Consume the ParenOpen branch and get its contents.
        let (iter, open_span) = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span(self.db));
                (inner, open_span)
            }
            _ => unreachable!("caller must peek for ParenOpen before calling"),
        };

        let mut sub = self.sub_parser(iter, Some((open_span, "in this tuple")));
        let (elements, had_comma) = sub.parse_comma_separated_with_trailing(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);

        // Single element without comma is grouping parens, not a 1-tuple.
        if elements.len() == 1 && !had_comma {
            elements.into_iter().next().unwrap()
        } else {
            ast::ExprFun::new(
                self.db,
                self.module_id(),
                self.current_fn_name(),
                self.next_expr_index(),
                ast::ExprFunKind::Tuple(ast::ExprTuple { elements })
            )
        }
    }
}
