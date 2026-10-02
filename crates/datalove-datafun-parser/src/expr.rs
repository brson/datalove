//! Expression parsing.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit::parser_util::{self, TextSpan, TokenStream, TokenStreamExt};
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

        // Nothing is built on top of an operand that did not parse. Reading an
        // operator after one asks for a right-hand side that the same broken
        // text has to supply, and what it says about the second failure is
        // worth less than the first: `$-` at the end of a line reported the
        // `$`, then reported an expression missing after the `-` at the top of
        // the file, there being no token left to point at.
        if matches!(lhs.expr(self.db), ast::ExprFunKind::ParseError(_)) {
            return lhs;
        }

        // Check for postfix try operators (? and !).
        // These have highest precedence and are parsed before binary operators.
        lhs = self.parse_postfix_try_operators(lhs);

        // Whether `lhs` is a comparison built by this loop. One in parentheses
        // came from `parse_expr_primary` and may be compared again.
        let mut lhs_is_comparison = false;

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

            // Comparisons do not chain. `a == b == c` would compare `a == b`,
            // a bool, with `c`, and `a .< b .< c` is refused by the types only
            // as long as bools are not ordered; neither means what it reads as.
            if is_comparison(op) && lhs_is_comparison {
                use datalove_diagnostic::DiagnosticBuilderExt;
                let ts = self.peek_text_span();
                let op_text = ts.text.as_str(self.db)[ts.span.clone()].S();
                self.had_error = true;
                bct::diagnostic::DiagnosticBuilder::error(self.db, &fmt!("`{op_text}` after a comparison"))
                    .code("P067")
                    .primary_label(ts, "comparisons do not chain")
                    .note("parenthesize the comparison whose result is being compared, or join two comparisons with `and`")
                    .emit_parse();
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
            lhs_is_comparison = is_comparison(op);
        }

        lhs
    }

    /// Parse postfix operators (?, !, @, field projections, and index).
    ///
    /// Handles:
    /// - `?` - unwrap Option with early return
    /// - `!` - unwrap Result with early return
    /// - `@` - clone/coerce (widen or clone to fit target type)
    /// - `.field` - struct field projection
    /// - `.0` - tuple index projection
    /// - `[expr]` - index expression (produces fallible place)
    ///
    /// When an index `[expr]` is followed by `?` or `!` and the base expression
    /// can be decomposed into a root name + field steps, produces
    /// `ExprFunKind::Place` instead of `TryOption(Index(...))`.
    pub(super) fn parse_postfix_try_operators(&mut self, mut expr: ast::ExprFun<'db>) -> ast::ExprFun<'db> {
        loop {
            // A postfix operator is written against what it operates on, so
            // one with a space before it is not attached to this expression.
            if !self.glued_left() {
                break;
            }
            match self.peek() {
                Some(TreeToken::Token(token)) => {
                    let TextSpan { text, span: op_span } = self.extract_text_span(&TreeToken::Token(token.clone()));
                    match token.kind {
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
                            // If base is a Place, push Field step directly.
                            if let ast::ExprFunKind::Place(ref place) = expr.expr(self.db) {
                                let root = place.root;
                                let mut steps = place.steps.clone();
                                steps.push(ast::PlaceStep::Field(field));
                                expr = self.create_expr(
                                    ast::ExprFunKind::Place(ast::Place { root, steps }),
                                    TextSpan::new(text, span),
                                );
                            } else {
                                expr = self.create_expr(
                                    ast::ExprFunKind::FieldProj(ast::ExprFieldProj { base: expr, field }),
                                    TextSpan::new(text, span),
                                );
                            }
                        }
                        _ => break,
                    }
                }
                Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }) => {
                    // Index access: expr[index_expr]
                    let text = self.source_text();
                    let start_pos = self.current_byte_pos();
                    let inner = match self.next() {
                        Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => *inner,
                        _ => unreachable!(),
                    };
                    let mut sub = self.sub_parser(inner, None);
                    let index = sub.parse_expr_full();
                    sub.error_if_not_exhausted();
                    self.merge_from_sub(&mut sub);

                    // Check for ? or ! following the index, written against it.
                    let error_mode = if !self.glued_left() {
                        None
                    } else if self.peek_sigil(Sigil::Question) {
                        self.next(); // consume ?
                        Some(ast::IndexErrorMode::Option)
                    } else if self.peek_sigil(Sigil::Exclamation) {
                        self.next(); // consume !
                        Some(ast::IndexErrorMode::Result)
                    } else {
                        None
                    };

                    // If base is a Place, push Index step directly.
                    if let ast::ExprFunKind::Place(ref place) = expr.expr(self.db) {
                        let root = place.root;
                        let mut steps = place.steps.clone();
                        steps.push(ast::PlaceStep::Index(ast::PlaceIndex {
                            index,
                            error_mode,
                        }));
                        let end_pos = self.last_byte_end();
                        let span = start_pos..end_pos;
                        expr = self.create_expr(
                            ast::ExprFunKind::Place(ast::Place { root, steps }),
                            TextSpan::new(text, span),
                        );
                    } else if let Some(error_mode) = error_mode {
                        // Non-place base with ? or ! — fall back to TryOption/TryResult wrapping Index.
                        let end_pos = self.last_byte_end();
                        let span = start_pos..end_pos;
                        let index_expr = self.create_expr(
                            ast::ExprFunKind::Index(ast::ExprIndex { base: expr, index }),
                            TextSpan::new(text, span.clone()),
                        );
                        expr = self.create_expr(
                            match error_mode {
                                ast::IndexErrorMode::Option =>
                                    ast::ExprFunKind::TryOption(ast::ExprTryOption { operand: index_expr }),
                                ast::IndexErrorMode::Result =>
                                    ast::ExprFunKind::TryResult(ast::ExprTryResult { operand: index_expr }),
                            },
                            TextSpan::new(text, span),
                        );
                    } else {
                        // Non-place base, bare index — produce Index node.
                        let end_pos = self.last_byte_end();
                        let span = start_pos..end_pos;
                        expr = self.create_expr(
                            ast::ExprFunKind::Index(ast::ExprIndex { base: expr, index }),
                            TextSpan::new(text, span),
                        );
                    }
                }
                _ => break,
            }
        }
        expr
    }

    /// Parse a field selector (name or index) after a dot.
    pub(super) fn parse_field_selector(&mut self) -> ast::FieldSelector<'db> {
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
    ///
    /// An operator written against one of its neighbours and not the other is
    /// a prefix or a postfix operator rather than this one, so the expression
    /// ends before it: `a -1` is `a` and then `-1`. A word operator is
    /// delimited by being a word, which is why only the sigils are asked
    /// about their spacing.
    fn peek_binop(&self) -> Option<ast::BinOp> {
        let op = self.peek_binop_ignoring_spacing()?;
        if self.peek_is_sigil() && !self.is_infix_spacing() {
            return None;
        }
        Some(op)
    }

    /// Whether the token at the cursor is a sigil.
    fn peek_is_sigil(&self) -> bool {
        matches!(
            self.peek(),
            Some(TreeToken::Token(token)) if matches!(token.kind, TokenKind::Sigil(_))
        )
    }

    /// What to say about an operator written against one side only.
    ///
    /// An expression that ended at one of these ended because of how it was
    /// spaced, which is worth saying rather than reporting the operator as a
    /// token nobody expected.
    pub(super) fn lopsided_operator(&self) -> Option<(String, String)> {
        let Some(TreeToken::Token(token)) = self.peek() else {
            return None;
        };
        let TokenKind::Sigil(sigil) = token.kind else {
            return None;
        };
        let text = sigil.as_str();

        if self.peek_binop_ignoring_spacing().is_some() && !self.is_infix_spacing() {
            let fixity = if self.glued_right() { "prefix" } else { "postfix" };
            return Some((
                fmt!("this `{text}` is spaced as a {fixity} operator"),
                S("an operator between two things is written against both of them, or against neither"),
            ));
        }

        if Self::is_postfix_sigil(sigil) && !self.glued_left() {
            return Some((
                fmt!("this `{text}` is written apart from what it applies to"),
                S("a postfix operator is written against the expression before it"),
            ));
        }

        None
    }

    /// Whether a sigil is one of the operators written after its operand.
    fn is_postfix_sigil(sigil: Sigil) -> bool {
        matches!(
            sigil,
            Sigil::Question | Sigil::Exclamation | Sigil::At | Sigil::Dot,
        )
    }

    /// The binary operator at the cursor, whatever its spacing says.
    fn peek_binop_ignoring_spacing(&self) -> Option<ast::BinOp> {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind {
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
    /// Parse the payload of `some`, `ok`, `er`, `data`, `error` or `term`.
    ///
    /// A primary expression and the postfix operators on it, so `some x@` is
    /// `some (x@)`. Every payload is read here, whether or not a hint is
    /// written over the constructor, so that the two read the same.
    ///
    /// A binary operator after a payload is refused. It could only mean the
    /// operator applies to the constructed value, which none of these have,
    /// or that the payload reaches further than it does; either way what was
    /// meant needs parentheses to say.
    pub(super) fn parse_payload(&mut self) -> ast::ExprFun<'db> {
        let payload = self.parse_expr_primary();
        let payload = self.parse_postfix_try_operators(payload);
        if self.peek_binop().is_some() {
            use datalove_diagnostic::DiagnosticBuilderExt;
            let ts = self.peek_text_span();
            let op = ts.text.as_str(self.db)[ts.span.clone()].S();
            self.had_error = true;
            bct::diagnostic::DiagnosticBuilder::error(self.db, &fmt!("`{op}` after a constructor's payload"))
                .code("P065")
                .primary_label(ts, "the payload ends before this")
                .note(&fmt!("a payload is one expression. Parenthesize `a {op} b` to put the \
operation in the payload, or the whole constructor to apply it to what is built"))
                .emit_parse();
        }
        payload
    }

    pub(super) fn parse_expr_primary(&mut self) -> ast::ExprFun<'db> {
        // Check for unary operators (-, -?, -!, not).
        if let Some(TreeToken::Token(token)) = self.peek() {
            let unary_op = match token.kind {
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
                let start = self.current_byte_pos();
                let text = self.source_text();
                self.next(); // Consume the operator.
                let operand = self.parse_expr_primary();
                let span = start..self.last_byte_end();
                return self.create_expr(
                    ast::ExprFunKind::UnaryOp(ast::ExprUnaryOp { op, operand }),
                    TextSpan::new(text, span),
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
                match token.kind {
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
                                    let payload = self.parse_payload();
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
                                    let value = self.parse_payload();
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
                                        ast::ExprFunKind::Atom(ast::ExprAtom { type_hint: None, name }),
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
                                    let payload = self.parse_payload();
                                    self.create_expr(
                                        ast::ExprFunKind::Term(ast::ExprTerm { type_hint: None, name, payload }),
                                        ts
                                    )
                                }
                                // Enum literal expression: `enum { atom Foo }`.
                                "enum" if self.peek_second_sigil(Sigil::BraceOpen) => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume "enum"
                                    // Parse the inner variant expression in braces.
                                    let iter = match self.next() {
                                        Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => *inner,
                                        _ => unreachable!(),
                                    };
                                    let mut sub = self.sub_parser(iter, None);
                                    let variant = sub.parse_expr_full();
                                    // A trailing comma closes the one item, as it may anywhere.
                                    sub.eat_sigil(Sigil::Comma);
                                    sub.error_if_not_exhausted();
                                    self.merge_from_sub(&mut sub);
                                    self.create_expr(
                                        ast::ExprFunKind::EnumLiteral(ast::ExprEnumLiteral { type_hint: None, variant }),
                                        ts
                                    )
                                }
                                // Intrinsic call: icall name(args)
                                "icall" => {
                                    let ts = self.peek_text_span();
                                    self.next(); // consume "icall"

                                    // Parse intrinsic name.
                                    let intrinsic_name = match self.peek() {
                                        Some(TreeToken::Token(tok)) if tok.kind == TokenKind::Word => {
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
                                    let (args, intrinsic_arg_modes) = match self.peek() {
                                        Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                                            let (args_iter, open_span) = match self.next() {
                                                Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                                                    let open_span = TextSpan::new(self.source_text(), open.span());
                                                    (inner, open_span)
                                                }
                                                _ => unreachable!(),
                                            };
                                            // Intrinsics have no parameter modes; a marker
                                            // here is rejected during typechecking.
                                            self.parse_function_call_args(*args_iter, Some((open_span, "in this argument list")))
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
                                            arg_modes: intrinsic_arg_modes,
                                        }),
                                        ts
                                    )
                                }
                                num if Self::is_number_word(num) => {
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
                                                let open_span = TextSpan::new(self.source_text(), open.span());
                                                (inner, open_span)
                                            }
                                            _ => unreachable!(),
                                        };
                                        let (args, arg_modes) = self.parse_function_call_args(*args_iter, Some((open_span, "in this argument list")));
                                        // For function calls, span should include the parens, but for now just use the name span.
                                        let call = ast::ExprFunctionCall::new(
                                            self.db,
                                            self.module_id(),
                                            self.current_fn_name(),
                                            self.next_call_index(),
                                            name,
                                            args,
                                            arg_modes,
                                        );
                                        self.create_expr(
                                            ast::ExprFunKind::FunctionCall(call),
                                            ts
                                        )
                                    } else {
                                        // It's just a variable name.
                                        self.create_expr(
                                            ast::ExprFunKind::Place(ast::Place { root: name, steps: vec![] }),
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
                        let raw = token.text.as_str(self.db);
                        let ts = self.peek_text_span();
                        self.next();
                        if let Err(error) = parser_util::string_literal_value(raw) {
                            let (message, label) = parser_util::escape_complaint(&error);
                            return self.emit_expr_error(ts, &message, "D039", &label);
                        }
                        let value = InternedText::new(self.db, raw.S());
                        self.create_expr(
                            ast::ExprFunKind::String(ast::ExprString {
                                type_hint: None,
                                value
                            }),
                            ts,
                        )
                    }
                    _ => {
                        let ts = self.peek_text_span();
                        // Consumed, because a caller that reads expressions
                        // until the tokens run out has nothing else to move it
                        // along: a tensor's innermost axis is separated by
                        // spaces, so the loop over its elements ends only when
                        // the parser has eaten them all.
                        self.next();
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
    ) -> (Vec<ast::ExprFun<'db>>, Vec<Option<ast::ParamMode>>) {
        let mut sub = self.sub_parser(iter, context);
        let args = sub.parse_comma_separated(|p| {
            let mode = p.parse_arg_mode();
            (p.parse_expr_full(), mode)
        });
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
        args.into_iter().unzip()
    }

    /// Parse a `ref`, `mut` or `out` marker before a call argument.
    ///
    /// Returns `None` when the argument carries no marker, which denotes `in`.
    fn parse_arg_mode(&mut self) -> Option<ast::ParamMode> {
        use bct::lexer::TokenKind;

        let TreeToken::Token(t) = self.peek()? else {
            return None;
        };
        if t.kind != TokenKind::Word {
            return None;
        }
        let mode = match t.word_str(self.db)? {
            "ref" => ast::ParamMode::Ref,
            "mut" => ast::ParamMode::Mut,
            "out" => ast::ParamMode::Out,
            _ => return None,
        };
        self.next();
        Some(mode)
    }

    /// Parse a datafun tuple: (expr1, expr2, ...).
    ///
    /// Caller must have already peeked and confirmed a `ParenOpen` branch.
    pub(super) fn parse_datafun_tuple(&mut self) -> ast::ExprFun<'db> {
        // Consume the ParenOpen branch and get its contents.
        let (iter, open_span) = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, open, inner, .. }) => {
                let open_span = TextSpan::new(self.source_text(), open.span());
                (inner, open_span)
            }
            _ => unreachable!("caller must peek for ParenOpen before calling"),
        };
        let tuple_ts = TextSpan::new(open_span.text, open_span.span.start..self.last_byte_end());

        let mut sub = self.sub_parser(*iter, Some((open_span, "in this tuple")));
        let (elements, had_comma) = sub.parse_comma_separated_with_trailing(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);

        // Single element without comma is grouping parens, not a 1-tuple.
        if elements.len() == 1 && !had_comma {
            elements.into_iter().next().unwrap()
        } else {
            self.create_expr(ast::ExprFunKind::Tuple(ast::ExprTuple { elements }), tuple_ts)
        }
    }
}

fn is_comparison(op: ast::BinOp) -> bool {
    use ast::BinOp::*;
    matches!(op, Eq | Ne | Lt | Gt | Le | Ge)
}
