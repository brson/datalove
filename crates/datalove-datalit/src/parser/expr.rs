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
            Some("tensor") => {
                self.eat_word("tensor");

                // Parse shape: [dim1, dim2, ...]
                let shape = if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let shape = sub_parser.parse_comma_separated(|p| {
                        match p.parse_u32_literal() {
                            Some(dim) => dim,
                            None => {
                                p.had_error = true;
                                let ts = p.peek_text_span();
                                DiagnosticBuilder::error(p.db, "expected dimension value in tensor shape")
                                    .code("D023")
                                    .primary_label(ts, "expected integer")
                                    .emit_parse();
                                0 // Placeholder dimension.
                            }
                        }
                    });
                    sub_parser.error_if_not_exhausted();
                    shape
                } else {
                    let ts = self.peek_text_span();
                    return self.emit_expr_error(ts,
                        "expected shape [...] after tensor keyword",
                        "D010",
                        "expected '[' for tensor shape"
                    );
                };

                // Parse data: For rank 1, comma-separated elements; for rank 2+, comma-separated rows with space-separated elements.
                let elements = if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    let rank = shape.len();
                    if rank == 0 {
                        let ts = self.peek_text_span();
                        return self.emit_expr_error(ts,
                            "tensor rank must be at least 1",
                            "D012",
                            "invalid rank"
                        );
                    }

                    if rank == 1 {
                        // 1D tensor: comma-separated elements [1, 2, 3, 4, 5].
                        let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                        let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                        sub_parser.error_if_not_exhausted();
                        // Merge spans from sub-parser.
                        self.expr_spans.extend(sub_parser.expr_spans);
                        elements
                    } else {
                        // 2D+ tensor: comma-separated rows, space-separated elements [1 2 3, 4 5 6].
                        let row_size = *shape.last().unwrap() as usize;
                        let all_tokens: Vec<_> = iter.collect();

                        // Split tokens by commas to get rows.
                        let rows = self.split_tokens_by_comma(&all_tokens);
                        let mut all_elements = Vec::new();

                        for row_tokens in rows {
                            // Filter spaces within the row to get individual element tokens.
                            let elem_tokens: Vec<_> = row_tokens.into_iter()
                                .filter_map(|t| t.without_space(self.db))
                                .collect();

                            // Parse each element in the row.
                            let mut row_parser = Parser::new(self.db, elem_tokens.clone(), self.source_text());

                            let mut row_elements = Vec::new();
                            while row_parser.peek().is_some() {
                                row_elements.push(row_parser.parse_expr_full());
                            }

                            // Merge spans from row sub-parser.
                            self.expr_spans.extend(row_parser.expr_spans);

                            // Validate row size matches the last dimension.
                            if row_elements.len() != row_size {
                                let ts = self.peek_text_span();
                                let detailed_message = format!("expected {} elements per row but got {}", row_size, row_elements.len());

                                return self.emit_expr_error(ts,
                                    &detailed_message,
                                    "D013",
                                    &format!("expected {} elements", row_size)
                                );
                            }

                            all_elements.extend(row_elements);
                        }

                        all_elements
                    }
                } else {
                    let ts = self.peek_text_span();
                    return self.emit_expr_error(ts,
                        "expected data [...] after tensor shape",
                        "D011",
                        "expected '[' for tensor data"
                    );
                };

                return ast::Expr::Tensor(ast::ExprTensor { shape, elements });
            }
            Some("enum") => {
                let ts = self.peek_text_span();
                self.eat_word("enum");
                // Enum expression syntax: enum Variant or enum Variant(...).
                let variant_name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        return self.emit_expr_error(ts,
                            "expected variant name after enum keyword",
                            "D016",
                            "expected enum variant name"
                        );
                    }
                };
                let payload = if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Parse a single expression as payload.
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let payload_expr = sub_parser.parse_expr_full();
                    sub_parser.error_if_not_exhausted();
                    // Merge spans from sub-parser.
                    self.expr_spans.extend(sub_parser.expr_spans);
                    Some(payload_expr)
                } else {
                    None
                };
                return ast::Expr::AnonEnum(ast::ExprAnonEnum { variant_name, payload });
            }
            Some("map") => {
                let ts = self.peek_text_span();
                self.eat_word("map");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
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
                    // Merge spans from sub-parser.
                    self.expr_spans.extend(sub_parser.expr_spans);
                    return ast::Expr::Map(ast::ExprMap { entries });
                } else {
                    return self.emit_expr_error(ts,
                        "expected {} after map keyword",
                        "D016",
                        "expected '{' after 'map'"
                    );
                }
            }
            Some("set") => {
                let ts = self.peek_text_span();
                self.eat_word("set");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                    sub_parser.error_if_not_exhausted();
                    // Merge spans from sub-parser.
                    self.expr_spans.extend(sub_parser.expr_spans);
                    return ast::Expr::Set(ast::ExprSet { elements });
                } else {
                    return self.emit_expr_error(ts,
                        "expected {} after set keyword",
                        "D017",
                        "expected '{' after 'set'"
                    );
                }
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
                // Tuple.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = Parser::from_branch(self.db, inner, self.source_text());
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::AnonTuple(ast::ExprAnonTuple { elements })
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
            Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, .. }) => {
                // Table.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => inner,
                    _ => unreachable!(),
                };
                self.parse_table_expr(inner)
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
