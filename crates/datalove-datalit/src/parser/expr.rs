//! Expression parsing.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::TreeToken,
    split,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{self, TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        // Capture span before parsing.
        let ts = self.peek_text_span();

        // Taken before parsing, so that an expression is numbered before the
        // ones nested inside it.
        let local_index = self.next_expr_index();

        // Check for `: type / expr` pattern.
        let expr_full = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint();
            if let ast::TypeHint::ParseError(e) = &type_hint {
                // The type was reported, and what follows it is whatever the
                // writer meant by it; a missing `/` would say the same thing
                // twice.
                let error_expr = ast::Expr::ParseError(ast::ExprParseError {
                    text: e.text, span: e.span.C(), message: e.message,
                });
                self.eat_sigil(Sigil::SlashForward);
                ast::ExprFull::new(self.db, Some(local_index), Some(type_hint.C()), error_expr)
            } else if !self.eat_sigil(Sigil::SlashForward) {
                let ts = self.peek_text_span();
                let error_expr = self.emit_expr_error(ts,
                    "expected '/' after type hint",
                    "D012",
                    "expected '/' separator between type hint and expression"
                );
                ast::ExprFull::new(self.db, Some(local_index), Some(type_hint), error_expr)
            } else {
                let expr = self.parse_expr();
                ast::ExprFull::new(self.db, Some(local_index), Some(type_hint), expr)
            }
        } else {
            // No type hint, just parse expression. Parentheses around a hinted
            // expression are the hinted expression, there being no second
            // hint for them to keep apart.
            match self.parse_expr() {
                ast::Expr::Group(group) => return group.inner,
                expr => ast::ExprFull::new(self.db, Some(local_index), None, expr),
            }
        };

        // Record span for this expression, from its hint if it has one to the
        // end of the expression.
        let end = self.prev_end().unwrap_or(ts.span.end).max(ts.span.end);
        self.expr_spans.push(ast::ParseSpanEntry::new(
            local_index,
            ts.text.source(self.db),
            ts.span.start..end,
        ));

        expr_full
    }

    /// Build the expression for a number, or report how it was written.
    ///
    /// A sign is part of the literal here, datalit having no operator that
    /// could claim it instead.
    fn number_expr(&mut self, number: parser_util::Number) -> ast::Expr<'db> {
        let ts = TextSpan::new(self.source_text(), number.span.C());

        if let Some((message, label)) = number.complaint() {
            return self.emit_expr_error(ts, &message, "D019", &label);
        }
        if let Some(suffix) = &number.suffix {
            let (message, label) = parser_util::suffix_complaint(suffix);
            return self.emit_expr_error(ts, &message, "D019", &label);
        }

        let value = InternedText::new(self.db, number.text());
        match (number.radix, number.float) {
            (parser_util::Radix::Hex, _) => ast::Expr::Hex(ast::ExprHex { value }),
            (parser_util::Radix::Dec, true) => ast::Expr::Float(ast::ExprFloat { value }),
            (parser_util::Radix::Dec, false) => ast::Expr::Int(ast::ExprInt { value }),
        }
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        // A number, taking the sign written against it where there is one.
        if let Some(number) = parser_util::eat_number(self) {
            return self.number_expr(number);
        }

        // A `-` no number follows is nothing else in datalit.
        if self.peek_sigil(Sigil::Minus) {
            let ts = self.peek_text_span();
            let before_hex = self.glued_right() && matches!(
                self.peek_next(),
                Some(TreeToken::Token(tok)) if tok.word_str(self.db).is_some_and(parser_util::is_hex_word)
            );
            self.next();
            if before_hex {
                self.next();
                return self.emit_expr_error(ts,
                    "a hex literal takes no sign",
                    "D013",
                    "a hex literal is an unsigned bit pattern; write a negative number in decimal"
                );
            }
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
            Some("atom") => {
                let ts = self.peek_text_span();
                self.eat_word("atom");
                let Some(name) = self.eat_name() else {
                    return self.emit_expr_error(ts,
                        "expected name after 'atom'",
                        "D035",
                        "expected atom name"
                    );
                };
                return ast::Expr::Atom(ast::ExprAtom { name });
            }
            Some("term") => {
                let ts = self.peek_text_span();
                self.eat_word("term");
                let Some(name) = self.eat_name() else {
                    return self.emit_expr_error(ts,
                        "expected name after 'term'",
                        "D036",
                        "expected term name"
                    );
                };
                let payload = self.parse_expr_full();
                return ast::Expr::Term(ast::ExprTerm { name, payload });
            }
            Some("enum") => {
                let ts = self.peek_text_span();
                self.eat_word("enum");
                let Some(inner) = self.eat_branch(Sigil::BraceOpen) else {
                    return self.emit_expr_error(ts,
                        "expected '{' after 'enum'",
                        "D037",
                        "an enum literal is written `enum { atom Name }`"
                    );
                };
                let mut sub_parser = self.sub_parser_from_branch(inner);
                let variant = sub_parser.parse_expr_full();
                // A trailing comma closes the one item, as it may anywhere.
                sub_parser.eat_sigil(Sigil::Comma);
                sub_parser.error_if_not_exhausted();
                self.merge_spans_from(&mut sub_parser);
                return ast::Expr::Enum(ast::ExprEnum { variant });
            }
            _ => {}
        }

        // Not a keyword, check for numbers, tokens, or branches.
        match self.peek_owned() {
            Some(TreeToken::Token(token)) => {
                match token.kind {
                    TokenKind::Word => {
                        // A word beginning with a digit was read as a number
                        // above, so whatever is left is a name, and datalit
                        // has nothing for one to mean.
                        let word = token.word_str(self.db).X();
                        let ts = self.peek_text_span();
                        self.next();
                        self.emit_expr_error(ts,
                            &format!("unexpected identifier '{}'", word),
                            "D019",
                            "unexpected identifier"
                        )
                    }
                    TokenKind::String => {
                        let ts = self.peek_text_span();
                        self.next();
                        let raw = token.text.as_str(self.db);
                        if let Err(error) = parser_util::string_literal_value(raw) {
                            let (message, label) = parser_util::escape_complaint(&error);
                            return self.emit_expr_error(ts, &message, "D039", &label);
                        }
                        let value = InternedText::new(self.db, raw.S());
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
                    Some(TreeToken::Branch { inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = self.sub_parser_from_branch(inner);
                let (elements, had_comma) = sub_parser.parse_comma_separated_with_trailing(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.merge_spans_from(&mut sub_parser);
                // Single element without comma is grouping parens, not a 1-tuple.
                if elements.len() == 1 && !had_comma {
                    let inner = elements.into_iter().next().X();
                    match inner.type_hint(self.db) {
                        Some(_) => ast::Expr::Group(ast::ExprGroup { inner }),
                        None => inner.expr(self.db).clone(),
                    }
                } else {
                    ast::Expr::AnonTuple(ast::ExprAnonTuple { elements })
                }
            }
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, .. }) => {
                // Struct.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = self.sub_parser_from_branch(inner);
                let mut seen = parser_util::SeenNames::default();
                let fields = sub_parser.parse_comma_separated(|p| p.parse_expr_struct_field(&mut seen));
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.merge_spans_from(&mut sub_parser);
                ast::Expr::AnonStruct(ast::ExprAnonStruct { fields })
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }) => {
                // List.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = self.sub_parser_from_branch(inner);
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.merge_spans_from(&mut sub_parser);
                ast::Expr::List(ast::ExprList { elements })
            }
            Some(TreeToken::Branch { sigil: Sigil::PercentBraceOpen, .. }) => {
                // Map: %{k = v, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = self.sub_parser_from_branch(inner);
                let entries = sub_parser.parse_comma_separated(|p| {
                    let key = p.parse_expr_full();
                    if !p.eat_sigil(Sigil::Equals) {
                        let ts = p.peek_text_span();
                        let error_expr = p.emit_expr_error(ts,
                            "expected '=' between map key and value",
                            "D017",
                            "expected '=' after key"
                        );
                        let error_value = ast::ExprFull::new(p.db, None, None, error_expr);
                        return ast::ExprMapEntry { key, value: error_value };
                    }
                    let value = p.parse_expr_full();
                    ast::ExprMapEntry { key, value }
                });
                sub_parser.error_if_not_exhausted();
                self.merge_spans_from(&mut sub_parser);
                ast::Expr::Map(ast::ExprMap { entries })
            }
            Some(TreeToken::Branch { sigil: Sigil::HashBraceOpen, .. }) => {
                // Set: #{e, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let mut sub_parser = self.sub_parser_from_branch(inner);
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                self.merge_spans_from(&mut sub_parser);
                ast::Expr::Set(ast::ExprSet { elements })
            }
            Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, .. }) => {
                // Table.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                self.parse_table_expr(inner)
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, .. }) => {
                // Tensor: [| data |] with multi-comma separators.
                let inner = match self.next() {
                    Some(TreeToken::Branch { inner, .. }) => *inner,
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

    fn parse_expr_struct_field(&mut self, seen: &mut parser_util::SeenNames<'db>) -> ast::ExprStructField<'db> {
        let name_ts = self.peek_text_span();
        let name = match self.eat_name() {
            Some(n) => {
                if !seen.take(self.db, n, name_ts, "field") {
                    self.had_error = true;
                }
                n
            }
            None => {
                // No name found - emit error and create placeholder.
                let ts = self.peek_text_span();
                let error_expr = self.emit_expr_error(ts.clone(),
                    "expected field name in struct expression",
                    "D018",
                    "expected field name"
                );
                let placeholder_name = InternedText::new(self.db, "<error>".S());
                let error_value = ast::ExprFull::new(self.db, None, None, error_expr);
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
            let error_value = ast::ExprFull::new(self.db, None, None, error_expr);
            return ast::ExprStructField { name, value: error_value };
        }
        let value = self.parse_expr_full();
        ast::ExprStructField { name, value }
    }

    /// Split a table row into its cells, reporting any stray `,`.
    fn split_table_cells(&mut self, tokens: &[TreeToken<'db>]) -> Vec<Vec<TreeToken<'db>>> {
        let groups = split::split_commas(tokens.iter().cloned(), 1);
        self.report_stray_delimiters(&groups, "columns");
        split::nonempty_groups(groups)
    }

    /// Report every delimiter that was written with nothing before it.
    fn report_stray_delimiters(&mut self, groups: &[split::TokenGroup<'db>], what: &str) {
        for written in split::stray_delimiters(groups) {
            self.had_error = true;
            split::stray_delimiter_error(self.db, self.source_text(), &written, what)
                .code("D033")
                .emit_parse();
        }
    }

    /// Parse tensor expression with multi-comma syntax: [| data |]
    ///
    /// Spaces separate elements along innermost axis.
    /// `,` separates rows (2nd axis).
    /// `,,` separates slabs (3rd axis).
    /// `,,,` separates blocks (4th axis), etc.
    fn parse_tensor_expr(&mut self, iter: bct::bracer::BracerIter<'db>) -> ast::Expr<'db> {
        // Filter to non-whitespace tokens for comma-level scanning.
        let tokens_no_ws: Vec<_> = iter
            .filter_map(|t| t.without_space())
            .collect();

        if tokens_no_ws.is_empty() {
            // Empty tensor: [| |] - rank 1, no elements.
            return ast::Expr::Tensor(ast::ExprTensor { shape: vec![0], elements: vec![] });
        }

        // The widest separator written decides the rank.
        let rank = split::max_comma_run(&tokens_no_ws) + 1;

        // Recursively split by comma levels and parse.
        let (shape, elements) = self.parse_tensor_multicomma(&tokens_no_ws, rank as u32);

        ast::Expr::Tensor(ast::ExprTensor { shape, elements })
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
            let mut parser = self.sub_parser_from_tokens(tokens.to_vec());
            let mut elements = Vec::new();
            while parser.peek().is_some() {
                elements.push(parser.parse_expr_full());
            }
            self.merge_spans_from(&mut parser);
            let shape = vec![elements.len() as u32];
            return (shape, elements);
        }

        // Split by (rank-1) consecutive commas.
        let split_level = rank - 1;
        let groups = split::split_commas(tokens.iter().cloned(), split_level as usize);
        self.report_stray_delimiters(&groups, "parts");
        let groups = split::nonempty_groups(groups);

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

    fn parse_table_expr(&mut self, iter: bct::bracer::BracerIter<'db>) -> ast::Expr<'db> {
        // Split by row delimiters (newline in whitespace, or semicolon).
        let groups = split::split_lines(self.db, iter);
        self.report_stray_delimiters(&groups, "rows");
        let rows = split::nonempty_groups(groups);

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

    /// Parse table header row (column names).
    ///
    /// A cell is one word and nothing else. Anything after the word is
    /// reported rather than dropped, so that `{| x zzz |}` is a refusal and
    /// not a one-column table.
    fn parse_table_header(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<InternedText<'db>> {
        let parts = self.split_table_cells(row_tokens);
        let mut names = Vec::new();
        let mut seen = parser_util::SeenNames::default();

        for part in parts {
            // A group `nonempty_groups` kept has a first token.
            let first = part.first().X();
            let Some(name) = self.cell_word(first).filter(|w| parser_util::is_identifier(w)) else {
                self.had_error = true;
                let ts = self.extract_text_span(first);
                DiagnosticBuilder::error(self.db, "expected column name in table header")
                    .code("D031")
                    .primary_label(ts, "expected name")
                    .emit_parse();
                names.push(InternedText::new(self.db, "<error>".S()));
                continue;
            };
            let name_text = InternedText::new(self.db, name.S());
            let ts = self.extract_text_span(first);
            if !seen.take(self.db, name_text, ts, "column") {
                self.had_error = true;
            }
            names.push(name_text);

            if let Some(extra) = part.get(1) {
                self.had_error = true;
                let ts = self.extract_text_span(extra);
                DiagnosticBuilder::error(self.db,
                    &format!("unexpected token after column name `{}`", name))
                    .code("D034")
                    .primary_label(ts, "a column is named by one word")
                    .emit_parse();
            }
        }

        names
    }

    /// The word a header cell begins with, if it begins with one.
    fn cell_word(&self, token: &TreeToken<'db>) -> Option<&'db str> {
        match token {
            TreeToken::Token(tok) => tok.word_str(self.db),
            TreeToken::Branch { .. } => None,
        }
    }

    fn parse_table_data_row(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<ast::ExprFull<'db>> {
        let parts = self.split_table_cells(row_tokens);
        let mut elements = Vec::new();

        for part in parts {
            let mut sub_parser = self.sub_parser_from_tokens(part);
            let expr = sub_parser.parse_expr_full();
            sub_parser.error_if_not_exhausted();
            self.merge_spans_from(&mut sub_parser);
            elements.push(expr);
        }

        elements
    }
}
