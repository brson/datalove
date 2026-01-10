//! Literal expression parsing.
//!
//! Handles datalit-style literals within datafun expressions.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;
use super::state::Parser;

impl<'db> Parser<'db> {
    /// Parse a literal expression into new inline variants.
    ///
    /// This handles the `: type / expr` pattern.
    pub(super) fn parse_lit_expr_full(&mut self) -> ast::ExprFun<'db> {
        // Capture span before parsing for diagnostic reporting.
        let ts = self.peek_text_span();

        // Check for `: type / expr` pattern.
        if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            // Expect `/` after type hint.
            if !self.eat_sigil(Sigil::SlashForward) {
                let ts = self.peek_text_span();
                return self.emit_expr_error(ts,
                    "expected '/' after type hint in `: type / expr` pattern",
                    "D021",
                    "expected '/'"
                );
            }
            let (_heap, expr_kind) = self.parse_lit_expr_and_heap(Some(type_hint));
            // The type_hint is already captured in the expr_kind.
            return self.create_expr(expr_kind, ts);
        }

        let (_heap, expr_kind) = self.parse_lit_expr_and_heap(None);
        self.create_expr(expr_kind, ts)
    }

    /// Parse heap sigil and expression.
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

    /// Parse a literal expression (keywords and literals).
    pub(super) fn parse_lit_expr(
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
                                        // Safe: we verified above that peek is a word token.
                                        let decimal_name = self.eat_name().X();
                                        let float_str = format!("-{}.{}", word, decimal_name.as_str(self.db));
                                        let value = InternedText::new(self.db, float_str.S());
                                        return ast::ExprFunKind::Float(ast::ExprFloat::new(self.db, heap, type_hint, value));
                                    }
                                }
                            }
                            // Dot was consumed but no valid decimal follows.
                            let ts = self.peek_text_span();
                            return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                                self.db,
                                ts.text,
                                ts.span.clone(),
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
            let ts = self.peek_text_span();
            return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                self.db,
                ts.text,
                                ts.span.clone(),
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
                return ast::ExprFunKind::Error(ast::ExprError::new(self.db, heap, type_hint, value));
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
                                            // Safe: we verified above that peek is a word token.
                                            let decimal_name = self.eat_name().X();
                                            let float_str = format!("{}.{}", word, decimal_name.as_str(self.db));
                                            let value = InternedText::new(self.db, float_str.S());
                                            return ast::ExprFunKind::Float(ast::ExprFloat::new(self.db, heap, type_hint, value));
                                        }
                                    }
                                }
                                // Dot was consumed but no valid decimal follows - treat as member access.
                                // This is a parse error for datalit, but we need to handle it.
                                // For now, return an error.
                                let ts = self.peek_text_span();
                                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                                    self.db,
                                    ts.text,
                                ts.span.clone(),
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
                            let ts = self.peek_text_span();
                            self.next();
                            return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                                self.db,
                                ts.text,
                                ts.span.clone(),
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
                        let ts = self.peek_text_span();
                        return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                            self.db,
                            ts.text,
                                ts.span.clone(),
                            InternedText::new(self.db, "unexpected token".S()),
                        ));
                    }
                }
            }
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                // Anonymous tuple.
                return self.parse_lit_anon_tuple(heap, type_hint);
            }
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, .. }) => {
                // Anonymous struct.
                return self.parse_lit_anon_struct(heap, type_hint);
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }) => {
                // List.
                return self.parse_lit_list(heap, type_hint);
            }
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db,
                    ts.text,
                                ts.span.clone(),
                    InternedText::new(self.db, "expected expression".S()),
                ));
            }
        }
    }

    /// Helper to check if a string is a numeric literal.
    pub(super) fn is_numeric_literal(s: &str) -> bool {
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

    /// Parse anonymous tuple: (expr, expr, ...)
    fn parse_lit_anon_tuple(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '('".S()),
                ));
            }
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::AnonTuple(ast::ExprAnonTuple::new(self.db, heap, type_hint, elements))
    }

    /// Parse anonymous struct: { name = expr, ... }
    fn parse_lit_anon_struct(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '{'".S()),
                ));
            }
        };

        let fields = self.parse_comma_separated_struct_fields(inner);
        ast::ExprFunKind::AnonStruct(ast::ExprAnonStruct::new(self.db, heap, type_hint, fields))
    }

    /// Parse list: [expr, expr, ...]
    fn parse_lit_list(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '['".S()),
                ));
            }
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::List(ast::ExprList::new(self.db, heap, type_hint, elements))
    }

    /// Parse set: set { expr, expr, ... }
    fn parse_lit_set(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("set");
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '{' after 'set'".S()),
                ));
            }
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::Set(ast::ExprSet::new(self.db, heap, type_hint, elements))
    }

    /// Parse map: map { key = value, ... }
    fn parse_lit_map(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("map");
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '{' after 'map'".S()),
                ));
            }
        };

        let entries = self.parse_comma_separated_map_entries(inner);
        ast::ExprFunKind::Map(ast::ExprMap::new(self.db, heap, type_hint, entries))
    }

    /// Parse anonymous enum: enum Variant or enum Variant(payload)
    fn parse_lit_anon_enum(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("enum");
        let variant_name = match self.eat_name() {
            Some(n) => n,
            None => {
                self.had_error = true;
                let ts = self.peek_text_span();
                let message = "expected variant name after 'enum'";
                DiagnosticBuilder::error(self.db, message)
                    .code("P021")
                    .primary_label(ts.clone(), "expected variant name")
                    .emit_parse();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db,
                    ts.text,
                                ts.span.clone(),
                    InternedText::new(self.db, message.S()),
                ));
            }
        };
        let payload = self.parse_optional_enum_payload();
        ast::ExprFunKind::AnonEnum(ast::ExprAnonEnum::new(
            self.db, heap, type_hint, variant_name, payload
        ))
    }

    /// Parse optional enum payload: (expr)
    fn parse_optional_enum_payload(&mut self) -> Option<ast::ExprFun<'db>> {
        match self.peek() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::ParenOpen, inner, .. }) => inner,
                    _ => return None,
                };
                let mut sub = Parser::from_branch(self.db, inner, self.source_text());
                if sub.peek().is_none() {
                    return None;
                }
                let expr = sub.parse_expr_full();
                sub.error_if_not_exhausted();
                self.had_error |= sub.had_error;
                self.merge_spans_from(&mut sub);
                Some(expr)
            }
            _ => None
        }
    }

    /// Parse tensor: tensor [shape] [data]
    fn parse_lit_tensor(
        &mut self,
        heap: datalit::ast::Heap,
        type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("tensor");

        // Parse shape: [dim1, dim2, ...]
        let shape = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => {
                self.parse_tensor_shape(inner)
            }
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '[' for tensor shape".S()),
                ));
            }
        };

        let rank = shape.len();

        // Parse data: [elements]
        // For rank 1: comma-separated elements.
        // For rank 2+: comma-separated rows, space-separated elements within each row.
        let elements = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => {
                if rank <= 1 {
                    self.parse_comma_separated_exprs(inner)
                } else {
                    let row_size = *shape.last().unwrap_or(&1) as usize;
                    let (elems, has_error) = self.parse_tensor_data_2d_plus(inner, row_size);
                    if has_error {
                        let ts = self.peek_text_span();
                        return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                            self.db, ts.text, ts.span.clone(),
                            InternedText::new(self.db, format!("expected {} elements per row", row_size).S()),
                        ));
                    }
                    elems
                }
            }
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(
                    self.db, ts.text, ts.span.clone(),
                    InternedText::new(self.db, "expected '[' for tensor data".S()),
                ));
            }
        };

        ast::ExprFunKind::Tensor(ast::ExprTensor::new(self.db, heap, type_hint, shape, elements))
    }

    /// Parse tensor data for 2D+ tensors: comma-separated rows, space-separated elements.
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
            let mut sub = Parser::new(self.db, elem_tokens, self.source_text());
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

    /// Split tokens by comma, preserving spaces within groups (for tensor row parsing).
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

    /// Parse tensor shape dimensions.
    fn parse_tensor_shape(&mut self, iter: BracerIter<'db>) -> Vec<u32> {
        let mut sub = Parser::from_branch(self.db, iter, self.source_text());
        let shape = sub.parse_comma_separated(|p| p.parse_shape_dimension());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        shape
    }

    /// Parse a single tensor shape dimension.
    fn parse_shape_dimension(&mut self) -> u32 {
        match self.peek_word() {
            Some(word) => {
                if let Ok(dim) = word.parse::<u32>() {
                    self.next();
                    dim
                } else {
                    self.had_error = true;
                    let ts = self.peek_text_span();
                    DiagnosticBuilder::error(self.db, "expected dimension number in tensor shape")
                        .code("D030")
                        .primary_label(ts, "expected number")
                        .emit_parse();
                    self.next(); // consume the invalid token
                    0
                }
            }
            None => {
                self.had_error = true;
                let ts = self.peek_text_span();
                DiagnosticBuilder::error(self.db, "expected dimension in tensor shape")
                    .code("D030")
                    .primary_label(ts, "expected dimension")
                    .emit_parse();
                0
            }
        }
    }

    /// Helper to parse comma-separated expressions from a branch.
    pub(super) fn parse_comma_separated_exprs(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprFun<'db>> {
        let mut sub = Parser::from_branch(self.db, iter, self.source_text());
        let elements = sub.parse_comma_separated(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        self.merge_spans_from(&mut sub);
        elements
    }

    /// Helper to parse comma-separated struct fields.
    fn parse_comma_separated_struct_fields(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprStructField<'db>> {
        let mut sub = Parser::from_branch(self.db, iter, self.source_text());
        let fields = sub.parse_comma_separated(|p| p.parse_struct_field());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        self.merge_spans_from(&mut sub);
        fields
    }

    /// Parse a single struct field: `name = value`.
    fn parse_struct_field(&mut self) -> ast::ExprStructField<'db> {
        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                let ts = self.peek_text_span();
                let error_expr = self.emit_expr_error(ts,
                    "expected field name in struct",
                    "D021",
                    "expected field name"
                );
                let error_name = InternedText::new(self.db, "<error>".S());
                return ast::ExprStructField::new(self.db, error_name, error_expr);
            }
        };

        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            let error_expr = self.emit_expr_error(ts,
                "expected '=' after field name in struct",
                "D022",
                "expected '='"
            );
            return ast::ExprStructField::new(self.db, name, error_expr);
        }

        let value = self.parse_expr_full();
        ast::ExprStructField::new(self.db, name, value)
    }

    /// Helper to parse comma-separated map entries.
    fn parse_comma_separated_map_entries(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprMapEntry<'db>> {
        let mut sub = Parser::from_branch(self.db, iter, self.source_text());
        let entries = sub.parse_comma_separated(|p| p.parse_map_entry());
        sub.error_if_not_exhausted();
        self.had_error |= sub.had_error;
        self.merge_spans_from(&mut sub);
        entries
    }

    /// Parse a single map entry: `key = value`.
    fn parse_map_entry(&mut self) -> ast::ExprMapEntry<'db> {
        let key = self.parse_expr_full();

        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            let error_value = self.emit_expr_error(ts,
                "expected '=' between map key and value",
                "D023",
                "expected '='"
            );
            return ast::ExprMapEntry::new(self.db, key, error_value);
        }

        let value = self.parse_expr_full();
        ast::ExprMapEntry::new(self.db, key, value)
    }
}
