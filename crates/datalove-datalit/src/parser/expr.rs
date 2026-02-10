//! Expression parsing.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::TreeToken,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{self, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        // Capture span before parsing.
        let ts = self.peek_text_span();

        // Check for `: type / expr` pattern.
        let expr_full = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint();
            if !self.eat_sigil(Sigil::SlashForward) {
                let ts = self.peek_text_span();
                let error_expr = self.emit_expr_error(ts,
                    "expected '/' after type hint",
                    "D012",
                    "expected '/' separator between type hint and expression"
                );
                ast::ExprFull::new(self.db, Some(type_hint), error_expr)
            } else {
                let expr = self.parse_expr();
                ast::ExprFull::new(self.db, Some(type_hint), expr)
            }
        } else {
            // No type hint, just parse expression.
            let expr = self.parse_expr();
            ast::ExprFull::new(self.db, None, expr)
        };

        // Record span for this expression.
        use salsa::plumbing::AsId;
        self.expr_spans.push(ast::ParseSpanEntry::new(
            expr_full.as_id(),
            ts.text.as_id(),
            ts.span.clone(),
        ));

        expr_full
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        // Parse keywords, literals, and structures.
        // Check for negative number literals first (- followed by digits).
        if self.peek_sigil(Sigil::Minus) {
            // Peek ahead to see if this is a negative number.
            self.eat_sigil(Sigil::Minus);
            if let Some(TreeToken::Token(token)) = self.peek() {
                if let Some(word) = token.word_str(self.db) {
                    if parser_util::is_numeric_literal(word) {
                        // It's a negative number! Consume the literal.
                        self.next();
                        // Only check for float pattern on decimal literals (not hex).
                        let is_hex = word.starts_with("0x") || word.starts_with("0X");
                        let is_float = !is_hex && self.peek_sigil(Sigil::Dot) && {
                            if let Some(TreeToken::Token(next_token)) = self.peek_next() {
                                if let Some(decimal_part) = next_token.word_str(self.db) {
                                    decimal_part.chars().all(|c| c.is_ascii_digit())
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };

                        if is_float {
                            // Negative float: -number.number
                            self.eat_sigil(Sigil::Dot);
                            // Safe: is_float checked that next token is a word with all digits.
                            let decimal_word = self.eat_name().X();
                            let float_str = format!("-{}.{}", word, decimal_word.as_str(self.db));
                            let value = InternedText::new(self.db, float_str.S());
                            return ast::Expr::Float(ast::ExprFloat { value });
                        } else if is_hex {
                            // Negative hex literal.
                            let hex_str = format!("-{}", word);
                            let value = InternedText::new(self.db, hex_str.S());
                            return ast::Expr::Hex(ast::ExprHex { value });
                        } else {
                            // Negative decimal int.
                            let int_str = format!("-{}", word);
                            let value = InternedText::new(self.db, int_str.S());
                            return ast::Expr::Int(ast::ExprInt { value });
                        }
                    }
                }
            }
            // Not a negative number - this is an error (unexpected minus).
            let ts = self.peek_text_span();
            return self.emit_expr_error(ts,
                "unexpected minus sign",
                "D013",
                "unexpected '-' not followed by number"
            );
        }

        // Check for keywords.
        match self.peek_word() {
            Some("true") => {
                self.eat_word("true");
                return ast::Expr::True;
            }
            Some("false") => {
                self.eat_word("false");
                return ast::Expr::False;
            }
            Some("none") => {
                self.eat_word("none");
                return ast::Expr::None;
            }
            Some("some") => {
                self.eat_word("some");
                let payload = self.parse_expr_full();
                return ast::Expr::Some(ast::ExprSome { payload });
            }
            Some("ok") => {
                self.eat_word("ok");
                let payload = self.parse_expr_full();
                return ast::Expr::Ok(ast::ExprOk { payload });
            }
            Some("er") => {
                self.eat_word("er");
                let payload = self.parse_expr_full();
                return ast::Expr::Er(ast::ExprEr { payload });
            }
            Some("data") => {
                self.eat_word("data");
                let value = self.parse_expr_full();
                return ast::Expr::Data(ast::ExprData { value });
            }
            Some("error") => {
                self.eat_word("error");
                let value = self.parse_expr_full();
                return ast::Expr::Error(ast::ExprError { value });
            }
            _ => {}
        }

        // Not a keyword, check for numbers, tokens, or branches.
        match self.peek_owned() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).X();
                        if parser_util::is_numeric_literal(word) {
                            self.next();
                            // Only check for float pattern on decimal literals (not hex).
                            let is_hex = word.starts_with("0x") || word.starts_with("0X");
                            if !is_hex && self.peek_sigil(Sigil::Dot) {
                                if let Some(TreeToken::Token(next_token)) = self.peek_next() {
                                    if let Some(decimal_part) = next_token.word_str(self.db) {
                                        if decimal_part.chars().all(|c| c.is_ascii_digit()) {
                                            // It's a float!
                                            self.eat_sigil(Sigil::Dot);
                                            // Safe: verified above that next token is a word with all digits.
                                            let decimal_word = self.eat_name().X();
                                            let float_str = format!("{}.{}", word, decimal_word.as_str(self.db));
                                            let value = InternedText::new(self.db, float_str.S());
                                            return ast::Expr::Float(ast::ExprFloat { value });
                                        }
                                    }
                                }
                            }
                            // Not a float - check if hex or decimal.
                            let value = InternedText::new(self.db, word.S());
                            if is_hex {
                                ast::Expr::Hex(ast::ExprHex { value })
                            } else {
                                ast::Expr::Int(ast::ExprInt { value })
                            }
                        } else {
                            // Not a number, parse error for bare identifiers.
                            let ts = self.peek_text_span();
                            self.next();
                            self.emit_expr_error(ts,
                                &format!("unexpected identifier '{}'", word),
                                "D019",
                                "unexpected identifier"
                            )
                        }
                    }
                    TokenKind::String => {
                        self.next();
                        let value = InternedText::new(
                            self.db,
                            token.text(self.db).as_str(self.db).S(),
                        );
                        ast::Expr::String(ast::ExprString { value })
                    }
                    _ => {
                        let ts = self.peek_text_span();
                        self.next(); // Consume unexpected token to prevent infinite loop.
                        self.emit_expr_error(ts,
                            "unexpected token in Parser expression",
                            "D020",
                            "unexpected token"
                        )
                    }
                }
            }
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                // Parenthesized expression: tuple or grouping.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = Parser::from_branch(self.db, inner, self.source_text());
                let (elements, had_comma) = sub_parser.parse_comma_separated_with_trailing(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                // Single element without comma is grouping parens, not a 1-tuple.
                if elements.len() == 1 && !had_comma {
                    elements.into_iter().next().unwrap().expr(self.db).clone()
                } else {
                    ast::Expr::AnonTuple(ast::ExprAnonTuple { elements })
                }
            }
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, .. }) => {
                // Struct.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = Parser::from_branch(self.db, inner, self.source_text());
                let fields = sub_parser.parse_comma_separated(|p| p.parse_expr_struct_field());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::AnonStruct(ast::ExprAnonStruct { fields })
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }) => {
                // List.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = Parser::from_branch(self.db, inner, self.source_text());
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::List(ast::ExprList { elements })
            }
            Some(TreeToken::Branch { sigil: Sigil::PercentBraceOpen, .. }) => {
                // Map: %{k = v, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = Parser::from_branch(self.db, inner, self.source_text());
                let entries = sub_parser.parse_comma_separated(|p| {
                    let key = p.parse_expr_full();
                    if !p.eat_sigil(Sigil::Equals) {
                        let ts = p.peek_text_span();
                        let error_expr = p.emit_expr_error(ts,
                            "expected '=' between map key and value",
                            "D017",
                            "expected '=' after key"
                        );
                        let error_value = ast::ExprFull::new(p.db, None, error_expr);
                        return ast::ExprMapEntry { key, value: error_value };
                    }
                    let value = p.parse_expr_full();
                    ast::ExprMapEntry { key, value }
                });
                sub_parser.error_if_not_exhausted();
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::Map(ast::ExprMap { entries })
            }
            Some(TreeToken::Branch { sigil: Sigil::HashBraceOpen, .. }) => {
                // Set: #{e, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = Parser::from_branch(self.db, inner, self.source_text());
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::Set(ast::ExprSet { elements })
            }
            Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, .. }) => {
                // Table.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                self.parse_table_expr(inner)
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, .. }) => {
                // Tensor: [| data |] with multi-comma separators.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                self.parse_tensor_expr(inner)
            }
            _ => {
                let ts = self.peek_text_span();
                self.next(); // Consume unexpected token to prevent infinite loop.
                self.emit_expr_error(ts,
                    "unexpected tree node in Parser expression",
                    "D018",
                    "unexpected token"
                )
            }
        }
    }

    fn parse_expr_struct_field(&mut self) -> ast::ExprStructField<'db> {
        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                // No name found - emit error and create placeholder.
                let ts = self.peek_text_span();
                let error_expr = self.emit_expr_error(ts.clone(),
                    "expected field name in struct expression",
                    "D018",
                    "expected field name"
                );
                let placeholder_name = InternedText::new(self.db, "<error>".S());
                let error_value = ast::ExprFull::new(self.db, None, error_expr);
                return ast::ExprStructField { name: placeholder_name, value: error_value };
            }
        };
        if !self.eat_sigil(Sigil::Equals) {
            let ts = self.peek_text_span();
            let error_expr = self.emit_expr_error(ts,
                "expected '=' after field name in struct expression",
                "D018",
                "expected '=' after field name"
            );
            let error_value = ast::ExprFull::new(self.db, None, error_expr);
            return ast::ExprStructField { name, value: error_value };
        }
        let value = self.parse_expr_full();
        ast::ExprStructField { name, value }
    }

    fn split_tokens_by_comma(&self, tokens: &[TreeToken<'db>]) -> Vec<Vec<TreeToken<'db>>> {
        let mut rows = Vec::new();
        let mut current_row = Vec::new();

        for token in tokens {
            match token {
                TreeToken::Token(tok) if tok.kind(self.db) == TokenKind::Sigil(Sigil::Comma) => {
                    // Found a comma, finish current row.
                    if !current_row.is_empty() {
                        rows.push(current_row.clone());
                        current_row.clear();
                    }
                }
                _ => {
                    // Add token to current row.
                    current_row.push(token.clone());
                }
            }
        }

        // Add the last row if non-empty.
        if !current_row.is_empty() {
            rows.push(current_row);
        }

        rows
    }

    /// Parse tensor expression with multi-comma syntax: [| data |]
    ///
    /// Spaces separate elements along innermost axis.
    /// `,` separates rows (2nd axis).
    /// `,,` separates slabs (3rd axis).
    /// `,,,` separates blocks (4th axis), etc.
    fn parse_tensor_expr(&mut self, iter: bct::bracer::BracerIter<'db>) -> ast::Expr<'db> {
        let all_tokens: Vec<_> = iter.collect();

        // Filter to non-whitespace tokens for comma-level scanning.
        let tokens_no_ws: Vec<_> = all_tokens.iter()
            .filter_map(|t| t.clone().without_space(self.db))
            .collect();

        if tokens_no_ws.is_empty() {
            // Empty tensor: [| |] - rank 1, no elements.
            return ast::Expr::Tensor(ast::ExprTensor { shape: vec![0], elements: vec![] });
        }

        // Scan for max consecutive comma count to determine rank.
        let max_comma_level = self.scan_max_comma_level(&tokens_no_ws);
        let rank = max_comma_level + 1;

        // Recursively split by comma levels and parse.
        let (shape, elements) = self.parse_tensor_multicomma(&tokens_no_ws, rank as u32);

        ast::Expr::Tensor(ast::ExprTensor { shape, elements })
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
    ///
    /// Returns (shape, flat_elements).
    fn parse_tensor_multicomma(
        &mut self,
        tokens: &[TreeToken<'db>],
        rank: u32,
    ) -> (Vec<u32>, Vec<ast::ExprFull<'db>>) {
        if rank == 1 {
            // Innermost level: space-separated elements (no commas).
            let mut parser = Parser::new(self.db, tokens.to_vec(), self.source_text());
            let mut elements = Vec::new();
            while parser.peek().is_some() {
                elements.push(parser.parse_expr_full());
            }
            self.expr_spans.extend(parser.expr_spans);
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
            let (sub_shape, sub_elements) = self.parse_tensor_multicomma(group, rank - 1);
            // Validate all groups have the same inner shape.
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
    ///
    /// Commas of exactly `level` consecutive commas are treated as separators.
    /// Commas with fewer consecutive occurrences are kept within groups.
    fn split_by_comma_level(&self, tokens: &[TreeToken<'db>], level: usize) -> Vec<Vec<TreeToken<'db>>> {
        // First, identify runs of consecutive commas and their positions.
        let mut groups: Vec<Vec<TreeToken<'db>>> = Vec::new();
        let mut current_group: Vec<TreeToken<'db>> = Vec::new();
        let mut i = 0;

        while i < tokens.len() {
            // Count consecutive commas starting at i.
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
                // This is a split point. End current group.
                if !current_group.is_empty() {
                    groups.push(std::mem::take(&mut current_group));
                }
                // If comma_count > level, keep the extras as lower-level separators
                // in the next group. The extras go at the start of the next group.
                let extras = comma_count - level;
                // Skip all the commas that were consumed as the separator.
                i = j;
                // Push any remaining commas back as part of the next group.
                for _ in 0..extras {
                    // Re-insert comma tokens for lower-level processing.
                    // We need to go back and grab the actual comma tokens.
                    // Actually, the commas after the separator level belong to
                    // the next group's internal structure. But this gets complex.
                    // Simpler: re-examine. A run of N commas where N >= level:
                    // treat as one split at this level. Any remainder (N - level)
                    // commas should NOT be kept - they are consumed.
                    // Actually, the plan says: fewer commas bind tighter.
                    // So ,, means split at level 2. A run of ,,, means split at
                    // level 3. A run of ,, does NOT split at level 3.
                    // We need exact level match, not >=.
                }
                // Actually, let me reconsider. The semantics should be:
                // Split by runs of exactly `level` consecutive commas.
                // But runs of MORE commas should be split at a higher level.
                // Since we process top-down (highest level first), a run of
                // `level` commas is a separator at this level.
                // A run of more than `level` commas would have been caught by
                // a higher-level split already. So at this level, we should
                // only see runs of exactly `level` or fewer.
                // Let's just treat >= level as a split.
            } else if comma_count > 0 {
                // Fewer commas than needed - keep them in the current group.
                for k in i..j {
                    current_group.push(tokens[k].clone());
                }
                i = j;
            } else {
                // Not a comma - add to current group.
                current_group.push(tokens[i].clone());
                i += 1;
            }
        }

        if !current_group.is_empty() {
            groups.push(current_group);
        }

        groups
    }

    fn parse_table_expr(&mut self, iter: bct::bracer::BracerIter<'db>) -> ast::Expr<'db> {
        // Collect all tokens including whitespace.
        let all_tokens: Vec<_> = iter.collect();

        // Split by row delimiters (newline in whitespace, or semicolon).
        let rows = self.split_tokens_by_row(&all_tokens);

        if rows.is_empty() {
            // Empty table: {||}.
            return ast::Expr::Table(ast::ExprTable {
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

        ast::Expr::Table(ast::ExprTable { header, rows: data_rows })
    }

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
                        current_row.push(token.clone());
                    }
                }
                _ => {
                    current_row.push(token.clone());
                }
            }
        }

        if !current_row.is_empty() {
            rows.push(current_row);
        }

        rows
    }

    fn parse_table_header(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<InternedText<'db>> {
        // Filter out whitespace and split by comma.
        let tokens_no_ws: Vec<_> = row_tokens.iter()
            .filter_map(|t| t.clone().without_space(self.db))
            .collect();

        // Split by comma and extract names.
        let parts = self.split_tokens_by_comma(&tokens_no_ws);
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

    fn parse_table_data_row(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<ast::ExprFull<'db>> {
        // Filter out whitespace.
        let tokens_no_ws: Vec<_> = row_tokens.iter()
            .filter_map(|t| t.clone().without_space(self.db))
            .collect();

        // Split by comma and parse each element.
        let parts = self.split_tokens_by_comma(&tokens_no_ws);
        let mut elements = Vec::new();

        for part in parts {
            let mut sub_parser = Parser::new(self.db, part, self.source_text());
            let expr = sub_parser.parse_expr_full();
            sub_parser.error_if_not_exhausted();
            self.expr_spans.extend(sub_parser.expr_spans);
            elements.push(expr);
        }

        elements
    }
}
