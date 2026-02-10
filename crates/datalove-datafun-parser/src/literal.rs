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
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
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
            let type_hint = self.parse_type_hint();
            // Expect `/` after type hint.
            if !self.eat_sigil(Sigil::SlashForward) {
                let ts = self.peek_text_span();
                return self.emit_expr_error(ts,
                    "expected '/' after type hint in `: type / expr` pattern",
                    "D021",
                    "expected '/'"
                );
            }
            let expr_kind = self.parse_lit_expr(Some(type_hint));
            // The type_hint is already captured in the expr_kind.
            return self.create_expr(expr_kind, ts);
        }

        let expr_kind = self.parse_lit_expr(None);
        self.create_expr(expr_kind, ts)
    }

    /// Parse a literal expression (keywords and literals).
    pub(super) fn parse_lit_expr(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
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
                                        return ast::ExprFunKind::Float(ast::ExprFloat { type_hint, value });
                                    }
                                }
                            }
                            // Dot was consumed but no valid decimal follows.
                            let ts = self.peek_text_span();
                            return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                                text: ts.text,
                                span: ts.span.C(),
                                message: InternedText::new(self.db, "expected decimal digits after '.'".S()),
                            });
                        }
                        if is_hex {
                            let hex_str = format!("-{}", word);
                            let value = InternedText::new(self.db, hex_str.S());
                            return ast::ExprFunKind::Hex(ast::ExprHex { type_hint, value });
                        } else {
                            let int_str = format!("-{}", word);
                            let value = InternedText::new(self.db, int_str.S());
                            return ast::ExprFunKind::Int(ast::ExprInt { type_hint, value });
                        }
                    }
                }
            }
            // Not a negative number - error.
            let ts = self.peek_text_span();
            return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                text: ts.text,
                span: ts.span.C(),
                message: InternedText::new(self.db, "unexpected minus sign".S()),
            });
        }

        // Check for keywords.
        match self.peek_word() {
            Some("true") => {
                self.eat_word("true");
                return ast::ExprFunKind::True(ast::ExprLit { type_hint });
            }
            Some("false") => {
                self.eat_word("false");
                return ast::ExprFunKind::False(ast::ExprLit { type_hint });
            }
            Some("none") => {
                self.eat_word("none");
                return ast::ExprFunKind::None(ast::ExprLit { type_hint });
            }
            Some("some") => {
                self.eat_word("some");
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Some(ast::ExprSome { type_hint, payload });
            }
            Some("ok") => {
                self.eat_word("ok");
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Ok(ast::ExprOk { type_hint, payload });
            }
            Some("er") => {
                self.eat_word("er");
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Er(ast::ExprEr { type_hint, payload });
            }
            Some("data") => {
                self.eat_word("data");
                // Parse any datafun expression (superset of datalit).
                let value = self.parse_expr_primary();
                return ast::ExprFunKind::Data(ast::ExprData { type_hint, value });
            }
            Some("error") => {
                self.eat_word("error");
                // Parse any datafun expression (superset of datalit).
                let value = self.parse_expr_primary();
                return ast::ExprFunKind::Error(ast::ExprError { type_hint, value });
            }
            Some("map") => {
                return self.parse_lit_map(type_hint);
            }
            Some("set") => {
                return self.parse_lit_set(type_hint);
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
                                            return ast::ExprFunKind::Float(ast::ExprFloat { type_hint, value });
                                        }
                                    }
                                }
                                // Dot was consumed but no valid decimal follows - treat as member access.
                                // This is a parse error for datalit, but we need to handle it.
                                // For now, return an error.
                                let ts = self.peek_text_span();
                                return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                                    text: ts.text,
                                    span: ts.span.C(),
                                    message: InternedText::new(self.db, "expected decimal digits after '.'".S()),
                                });
                            }
                            let value = InternedText::new(self.db, word.S());
                            if is_hex {
                                return ast::ExprFunKind::Hex(ast::ExprHex { type_hint, value });
                            } else {
                                return ast::ExprFunKind::Int(ast::ExprInt { type_hint, value });
                            }
                        } else {
                            // Unexpected identifier.
                            let ts = self.peek_text_span();
                            self.next();
                            return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                                text: ts.text,
                                span: ts.span.C(),
                                message: InternedText::new(self.db, format!("unexpected identifier '{}'", word).S()),
                            });
                        }
                    }
                    TokenKind::String => {
                        // Get the text before consuming the token.
                        let text_str = token.text(self.db).as_str(self.db).S();
                        self.next();
                        let value = InternedText::new(self.db, text_str);
                        return ast::ExprFunKind::String(ast::ExprString { type_hint, value });
                    }
                    _ => {
                        let ts = self.peek_text_span();
                        return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                            text: ts.text,
                            span: ts.span.C(),
                            message: InternedText::new(self.db, "unexpected token".S()),
                        });
                    }
                }
            }
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                // Anonymous tuple.
                return self.parse_lit_anon_tuple(type_hint);
            }
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, .. }) => {
                // Anonymous struct.
                return self.parse_lit_anon_struct(type_hint);
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }) => {
                // List.
                return self.parse_lit_list(type_hint);
            }
            Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, .. }) => {
                // Table.
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, inner, .. }) => inner,
                    _ => unreachable!(),
                };
                return self.parse_lit_table(type_hint, inner);
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, .. }) => {
                // Tensor: [| data |] with multi-comma separators.
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, inner, .. }) => inner,
                    _ => unreachable!(),
                };
                return self.parse_lit_tensor_multicomma(type_hint, inner);
            }
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                    text: ts.text,
                    span: ts.span.C(),
                    message: InternedText::new(self.db, "expected expression".S()),
                });
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
    ///
    /// Caller must have already peeked and confirmed a `ParenOpen` branch.
    fn parse_lit_anon_tuple(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, inner, .. }) => inner,
            _ => unreachable!("caller must peek for ParenOpen before calling"),
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::AnonTuple(ast::ExprAnonTuple { type_hint, elements })
    }

    /// Parse anonymous struct: { name = expr, ... }
    ///
    /// Caller must have already peeked and confirmed a `BraceOpen` branch.
    fn parse_lit_anon_struct(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => unreachable!("caller must peek for BraceOpen before calling"),
        };

        let fields = self.parse_comma_separated_struct_fields(inner);
        ast::ExprFunKind::AnonStruct(ast::ExprAnonStruct { type_hint, fields })
    }

    /// Parse list: [expr, expr, ...]
    ///
    /// Caller must have already peeked and confirmed a `BracketOpen` branch.
    fn parse_lit_list(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => inner,
            _ => unreachable!("caller must peek for BracketOpen before calling"),
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::List(ast::ExprList { type_hint, elements })
    }

    /// Parse set: set { expr, expr, ... }
    fn parse_lit_set(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("set");
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                    text: ts.text, span: ts.span.C(),
                    message: InternedText::new(self.db, "expected '{' after 'set'".S()),
                });
            }
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::Set(ast::ExprSet { type_hint, elements })
    }

    /// Parse map: map { key = value, ... }
    fn parse_lit_map(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        self.eat_word("map");
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => {
                let ts = self.peek_text_span();
                return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                    text: ts.text, span: ts.span.C(),
                    message: InternedText::new(self.db, "expected '{' after 'map'".S()),
                });
            }
        };

        let entries = self.parse_comma_separated_map_entries(inner);
        ast::ExprFunKind::Map(ast::ExprMap { type_hint, entries })
    }


    /// Parse tensor with multi-comma syntax: [| data |]
    fn parse_lit_tensor_multicomma(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
        iter: BracerIter<'db>,
    ) -> ast::ExprFunKind<'db> {
        let all_tokens: Vec<_> = iter.collect();

        // Filter to non-whitespace tokens for comma-level scanning.
        let tokens_no_ws: Vec<_> = all_tokens.iter()
            .filter_map(|t| t.C().without_space(self.db))
            .collect();

        if tokens_no_ws.is_empty() {
            return ast::ExprFunKind::Tensor(ast::ExprTensor {
                type_hint, shape: vec![0], elements: vec![],
            });
        }

        // Scan for max consecutive comma count to determine rank.
        let max_comma_level = self.scan_max_comma_level(&tokens_no_ws);
        let rank = max_comma_level + 1;

        // Recursively split by comma levels and parse.
        let (shape, elements) = self.parse_tensor_multicomma_inner(&tokens_no_ws, rank as u32);

        ast::ExprFunKind::Tensor(ast::ExprTensor { type_hint, shape, elements })
    }

    /// Scan tokens to find the maximum consecutive comma count.
    fn scan_max_comma_level(&self, tokens: &[TreeToken<'db>]) -> usize {
        let mut max_level = 0usize;
        let mut current_commas = 0usize;

        for token in tokens {
            if let TreeToken::Token(tok) = token {
                if tok.kind(self.db) == TokenKind::Sigil(Sigil::Comma) {
                    current_commas += 1;
                    max_level = max_level.max(current_commas);
                } else {
                    current_commas = 0;
                }
            } else {
                current_commas = 0;
            }
        }

        max_level
    }

    /// Parse tensor data from tokens with multi-comma structure.
    fn parse_tensor_multicomma_inner(
        &mut self,
        tokens: &[TreeToken<'db>],
        rank: u32,
    ) -> (Vec<u32>, Vec<ast::ExprFun<'db>>) {
        if rank == 1 {
            // Innermost level: space-separated elements.
            let mut sub = Parser::new(self.db, tokens.to_vec(), self.source_text(), self.module_id());
            let mut elements = Vec::new();
            while sub.peek().is_some() {
                elements.push(sub.parse_expr_full());
            }
            self.merge_from_sub(&mut sub);
            let shape = vec![elements.len() as u32];
            return (shape, elements);
        }

        // Split by (rank-1) consecutive commas.
        let split_level = rank - 1;
        let groups = self.split_by_comma_level(tokens, split_level as usize);

        if groups.is_empty() {
            return (vec![0; rank as usize], vec![]);
        }

        let mut all_elements = Vec::new();
        let mut inner_shape: Option<Vec<u32>> = None;

        for (group_idx, group) in groups.iter().enumerate() {
            let (sub_shape, sub_elements) = self.parse_tensor_multicomma_inner(group, rank - 1);
            match &inner_shape {
                None => inner_shape = Some(sub_shape),
                Some(expected) => {
                    if *expected != sub_shape {
                        let ts = self.peek_text_span();
                        DiagnosticBuilder::error(self.db,
                            &format!("inconsistent tensor shape at group {}: expected {:?} but got {:?}",
                                group_idx, expected, sub_shape))
                            .code("D023")
                            .primary_label(ts, "shape mismatch")
                            .emit_parse();
                    }
                }
            }
            all_elements.extend(sub_elements);
        }

        let mut shape = vec![groups.len() as u32];
        if let Some(inner) = inner_shape {
            shape.extend(inner);
        }

        (shape, all_elements)
    }

    /// Split tokens by N consecutive commas.
    fn split_by_comma_level(&self, tokens: &[TreeToken<'db>], level: usize) -> Vec<Vec<TreeToken<'db>>> {
        let mut groups: Vec<Vec<TreeToken<'db>>> = Vec::new();
        let mut current_group: Vec<TreeToken<'db>> = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            let mut comma_count = 0;
            let mut j = i;
            while j < tokens.len() {
                if let TreeToken::Token(tok) = &tokens[j] {
                    if tok.kind(self.db) == TokenKind::Sigil(Sigil::Comma) {
                        comma_count += 1;
                        j += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if comma_count >= level {
                if !current_group.is_empty() {
                    groups.push(std::mem::take(&mut current_group));
                }
                i = j;
            } else if comma_count > 0 {
                for k in i..j {
                    current_group.push(tokens[k].clone());
                }
                i = j;
            } else {
                current_group.push(tokens[i].clone());
                i += 1;
            }
        }

        if !current_group.is_empty() {
            groups.push(current_group);
        }

        groups
    }

    /// Parse table: {| header; row1; row2 |}
    fn parse_lit_table(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
        iter: BracerIter<'db>,
    ) -> ast::ExprFunKind<'db> {
        // Collect all tokens including whitespace.
        let all_tokens: Vec<_> = iter.collect();

        // Split by row delimiters (newline in whitespace, or semicolon).
        let rows = self.split_tokens_by_row(&all_tokens);

        if rows.is_empty() {
            // Empty table: {||}.
            return ast::ExprFunKind::Table(ast::ExprTable {
                type_hint,
                header: vec![],
                rows: vec![],
            });
        }

        // First row is header (column names).
        let header = self.parse_table_header(&rows[0]);
        let num_columns = header.len();

        // Remaining rows are data.
        let mut data_rows = Vec::new();
        for (row_idx, row_tokens) in rows.iter().skip(1).enumerate() {
            let elements = self.parse_table_data_row(row_tokens);
            if elements.len() != num_columns && !elements.is_empty() {
                let ts = self.peek_text_span();
                DiagnosticBuilder::error(self.db,
                    &format!("row {} has {} columns but header has {}",
                        row_idx + 1, elements.len(), num_columns))
                    .code("D030")
                    .primary_label(ts, "column count mismatch")
                    .emit_parse();
            }
            data_rows.push(ast::ExprTableRow { elements });
        }

        ast::ExprFunKind::Table(ast::ExprTable { type_hint, header, rows: data_rows })
    }

    /// Split tokens by row delimiters (newline or semicolon).
    fn split_tokens_by_row(&self, tokens: &[TreeToken<'db>]) -> Vec<Vec<TreeToken<'db>>> {
        let mut rows = Vec::new();
        let mut current_row = Vec::new();

        for token in tokens {
            match token {
                // Semicolon is explicit row delimiter.
                TreeToken::Token(tok) if tok.kind(self.db) == TokenKind::Sigil(Sigil::Semicolon) => {
                    if !current_row.is_empty() {
                        rows.push(std::mem::take(&mut current_row));
                    }
                }
                // Whitespace containing newline is implicit row delimiter.
                TreeToken::Token(tok) if tok.kind(self.db) == TokenKind::Whitespace => {
                    let text = tok.text(self.db).as_str(self.db);
                    if text.contains('\n') {
                        if !current_row.is_empty() {
                            rows.push(std::mem::take(&mut current_row));
                        }
                    } else {
                        current_row.push(token.C());
                    }
                }
                _ => {
                    current_row.push(token.C());
                }
            }
        }

        if !current_row.is_empty() {
            rows.push(current_row);
        }

        rows
    }

    /// Parse table header row (column names).
    fn parse_table_header(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<InternedText<'db>> {
        // Filter out whitespace and split by comma.
        let tokens_no_ws: Vec<_> = row_tokens.iter()
            .filter_map(|t| t.C().without_space(self.db))
            .collect();

        // Split by comma and extract names.
        let parts = self.split_tokens_by_comma_for_table(&tokens_no_ws);
        let mut names = Vec::new();

        for part in parts {
            // Each part should be a single name token.
            if let Some(TreeToken::Token(tok)) = part.first() {
                if let Some(name) = tok.word_str(self.db) {
                    names.push(InternedText::new(self.db, name.S()));
                    continue;
                }
            }
            // Error: expected column name.
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "expected column name in table header")
                .code("D031")
                .primary_label(ts, "expected name")
                .emit_parse();
            names.push(InternedText::new(self.db, "<error>".S()));
        }

        names
    }

    /// Parse table data row (comma-separated expressions).
    fn parse_table_data_row(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<ast::ExprFun<'db>> {
        // Filter out whitespace.
        let tokens_no_ws: Vec<_> = row_tokens.iter()
            .filter_map(|t| t.C().without_space(self.db))
            .collect();

        // Split by comma and parse each element.
        let parts = self.split_tokens_by_comma_for_table(&tokens_no_ws);
        let mut elements = Vec::new();

        for part in parts {
            let mut sub = Parser::new(self.db, part, self.source_text(), self.module_id());
            let expr = sub.parse_expr_full();
            sub.error_if_not_exhausted();
            self.merge_from_sub(&mut sub);
            elements.push(expr);
        }

        elements
    }

    /// Split tokens by comma (for table parsing).
    fn split_tokens_by_comma_for_table(&self, tokens: &[TreeToken<'db>]) -> Vec<Vec<TreeToken<'db>>> {
        let mut groups = Vec::new();
        let mut current = Vec::new();

        for token in tokens {
            match token {
                TreeToken::Token(tok) if tok.kind(self.db) == TokenKind::Sigil(Sigil::Comma) => {
                    if !current.is_empty() {
                        groups.push(std::mem::take(&mut current));
                    }
                }
                _ => {
                    current.push(token.C());
                }
            }
        }

        if !current.is_empty() {
            groups.push(current);
        }

        groups
    }

    /// Helper to parse comma-separated expressions from a branch.
    pub(super) fn parse_comma_separated_exprs(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprFun<'db>> {
        let mut sub = self.sub_parser(iter, None);
        let elements = sub.parse_comma_separated(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
        elements
    }

    /// Helper to parse comma-separated struct fields.
    fn parse_comma_separated_struct_fields(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprStructField<'db>> {
        let mut sub = self.sub_parser(iter, None);
        let fields = sub.parse_comma_separated(|p| p.parse_struct_field());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
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
                return ast::ExprStructField { name: error_name, value: error_expr };
            }
        };

        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            let error_expr = self.emit_expr_error(ts,
                "expected '=' after field name in struct",
                "D022",
                "expected '='"
            );
            return ast::ExprStructField { name, value: error_expr };
        }

        let value = self.parse_expr_full();
        ast::ExprStructField { name, value }
    }

    /// Helper to parse comma-separated map entries.
    fn parse_comma_separated_map_entries(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprMapEntry<'db>> {
        let mut sub = self.sub_parser(iter, None);
        let entries = sub.parse_comma_separated(|p| p.parse_map_entry());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
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
            return ast::ExprMapEntry { key, value: error_value };
        }

        let value = self.parse_expr_full();
        ast::ExprMapEntry { key, value }
    }
}
