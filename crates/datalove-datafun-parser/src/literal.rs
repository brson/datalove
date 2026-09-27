//! Literal expression parsing.
//!
//! Handles datalit-style literals within datafun expressions.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::{BracerIter, TreeToken},
    split,
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{self, TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use super::state::Parser;

impl<'db> Parser<'db> {
    /// Parse a literal expression into new inline variants.
    ///
    /// This handles the `: type / expr` pattern.
    pub(super) fn parse_lit_expr_full(&mut self) -> ast::ExprFun<'db> {
        // The span runs from the hint, if there is one, to the end of what it
        // is on, so that a diagnostic about either points at both.
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
            // A literal form has a field for the hint and takes it directly.
            // Anything else -- a name, a call -- has none, so the hint goes on
            // a wrapper around it. The spec's `: type / expression` admits
            // both; only the literal grammar is narrower.
            let expr_kind = if self.peek_starts_lit_expr() {
                self.parse_lit_expr(Some(type_hint))
            } else {
                let inner = self.parse_expr_primary();
                // Postfix onto the expression under the hint, the way `some`
                // and its fellows take it onto their payload: `: u32 / o?`
                // hints what the `?` produces, not the option it unwraps.
                let inner = self.parse_postfix_try_operators(inner);
                ast::ExprFunKind::Hinted(ast::ExprHinted { type_hint, inner })
            };
            let ts = TextSpan::new(ts.text, ts.span.start..self.last_byte_end());
            return self.create_expr(expr_kind, ts);
        }

        let expr_kind = self.parse_lit_expr(None);
        self.create_expr(expr_kind, ts)
    }

    /// Whether what comes next is a form [`Self::parse_lit_expr`] can read.
    ///
    /// Everything but a bare name is: numbers, strings, the literal keywords,
    /// and the bracketed collection forms. A name reaching `parse_lit_expr`
    /// became an "unexpected identifier" parse error, which is what made
    /// `: u32 / n` fail for any `n`.
    fn peek_starts_lit_expr(&self) -> bool {
        let Some(TreeToken::Token(token)) = self.peek() else {
            // A branch -- `(`, `[`, `{`, `%{`, `#{`, `[|`, `{|` -- is a
            // collection or aggregate literal.
            return true;
        };
        // A `-` is a literal's sign only where a decimal number is written
        // against it, which is where the number reader takes it. Anywhere
        // else it is the negation operator, over a name or a hex literal.
        if token.kind == TokenKind::Sigil(Sigil::Minus) {
            return self.glued_right() && matches!(
                self.peek_next(),
                Some(TreeToken::Token(next))
                    if next.word_str(self.db).is_some_and(|w| Self::is_number_word(w) && !parser_util::is_hex_word(w))
            );
        }
        if token.kind != TokenKind::Word {
            return true;
        }
        let Some(word) = token.word_str(self.db) else {
            return true;
        };
        if Self::is_number_word(word) {
            return true;
        }
        matches!(
            word,
            "true" | "false" | "none" | "some" | "ok" | "er"
                | "data" | "error" | "atom" | "term" | "enum"
        )
    }

    /// Parse a literal expression (keywords and literals).
    pub(super) fn parse_lit_expr(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        // A number, taking the sign written against it where there is one.
        // In an expression the prefix operator has already claimed a `-`, so
        // one reaching here belongs to the literal.
        if let Some(number) = parser_util::eat_number(self) {
            return self.number_expr(type_hint, number);
        }

        if self.peek_sigil(Sigil::Minus) {
            let ts = self.peek_text_span();
            self.next();
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
            Some("atom") => {
                let ts = self.peek_text_span();
                self.eat_word("atom");
                let name = match self.eat_name() {
                    Some(n) => n,
                    None => return self.lit_error(ts,
                        "expected name after 'atom'",
                        "P042",
                        "expected atom name",
                    ),
                };
                return ast::ExprFunKind::Atom(ast::ExprAtom { type_hint, name });
            }
            Some("term") => {
                let ts = self.peek_text_span();
                self.eat_word("term");
                let name = match self.eat_name() {
                    Some(n) => n,
                    None => return self.lit_error(ts,
                        "expected name after 'term'",
                        "P043",
                        "expected term name",
                    ),
                };
                let payload = self.parse_expr_primary();
                return ast::ExprFunKind::Term(ast::ExprTerm { type_hint, name, payload });
            }
            Some("enum") if self.peek_second_sigil(Sigil::BraceOpen) => {
                self.eat_word("enum");
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => *inner,
                    _ => unreachable!("peek_second_sigil said a brace follows"),
                };
                let mut sub = self.sub_parser(inner, None);
                let variant = sub.parse_expr_full();
                sub.error_if_not_exhausted();
                self.merge_from_sub(&mut sub);
                return ast::ExprFunKind::EnumLiteral(ast::ExprEnumLiteral { type_hint, variant });
            }
            _ => {}
        }

        // Check for numbers, strings, or branches.
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind {
                    TokenKind::Word => {
                        // A word beginning with a digit was read as a number
                        // above, so whatever is left here is a name.
                        let word = token.word_str(self.db).X();
                        let ts = self.peek_text_span();
                        self.next();
                        return ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                            text: ts.text,
                            span: ts.span.C(),
                            message: InternedText::new(self.db, format!("unexpected identifier '{}'", word).S()),
                        });
                    }
                    TokenKind::String => {
                        let raw = token.text.as_str(self.db);
                        let ts = self.peek_text_span();
                        self.next();
                        if let Err(error) = parser_util::string_literal_value(raw) {
                            let (message, label) = parser_util::escape_complaint(&error);
                            return self.lit_error(ts, &message, "D039", &label);
                        }
                        let value = InternedText::new(self.db, raw.S());
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
            Some(TreeToken::Branch { sigil: Sigil::PercentBraceOpen, .. }) => {
                // Map: %{k = v, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::PercentBraceOpen, inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let entries = self.parse_comma_separated_map_entries(inner);
                return ast::ExprFunKind::Map(ast::ExprMap { type_hint, entries });
            }
            Some(TreeToken::Branch { sigil: Sigil::HashBraceOpen, .. }) => {
                // Set: #{e, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::HashBraceOpen, inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                let elements = self.parse_comma_separated_exprs(inner);
                return ast::ExprFunKind::Set(ast::ExprSet { type_hint, elements });
            }
            Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, .. }) => {
                // Table.
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, inner, .. }) => *inner,
                    _ => unreachable!(),
                };
                return self.parse_lit_table(type_hint, inner);
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, .. }) => {
                // Tensor: [| data |] with multi-comma separators.
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, inner, .. }) => *inner,
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


    /// Build the expression for a number, or report how it was written.
    fn number_expr(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
        number: parser_util::Number,
    ) -> ast::ExprFunKind<'db> {
        let ts = TextSpan::new(self.source_text(), number.span.C());

        if let Some((message, label)) = number.complaint() {
            return self.lit_error(ts, &message, "P052", &label);
        }
        if let Some(suffix) = &number.suffix {
            let (message, label) = parser_util::suffix_complaint(suffix);
            return self.lit_error(ts, &message, "P053", &label);
        }

        let value = InternedText::new(self.db, number.text());
        match (number.radix, number.float) {
            (parser_util::Radix::Hex, _) => ast::ExprFunKind::Hex(ast::ExprHex { type_hint, value }),
            (parser_util::Radix::Dec, true) => ast::ExprFunKind::Float(ast::ExprFloat { type_hint, value }),
            (parser_util::Radix::Dec, false) => ast::ExprFunKind::Int(ast::ExprInt { type_hint, value }),
        }
    }

    /// Report how a literal was written, and stand in for it.
    fn lit_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprFunKind<'db> {
        self.had_error = true;
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.C(), label)
            .emit_parse();
        ast::ExprFunKind::ParseError(ast::ExprFunParseError {
            text: ts.text,
            span: ts.span,
            message: InternedText::new(self.db, message.S()),
        })
    }

    /// Whether a word begins a number.
    ///
    /// Shared with datalit, so that the two read the same words as numbers.
    pub(super) fn is_number_word(s: &str) -> bool {
        parser_util::is_number_word(s)
    }

    /// Parse anonymous tuple: (expr, expr, ...)
    ///
    /// Caller must have already peeked and confirmed a `ParenOpen` branch.
    fn parse_lit_anon_tuple(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, inner, .. }) => *inner,
            _ => unreachable!("caller must peek for ParenOpen before calling"),
        };

        let mut sub = self.sub_parser(inner, None);
        let (mut elements, had_comma) = sub.parse_comma_separated_with_trailing(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);

        // One element and no comma is a parenthesized expression, as it is
        // with no hint, and the hint is on what the parentheses hold.
        if elements.len() == 1 && !had_comma {
            let inner = elements.pop().X();
            return match type_hint {
                Some(type_hint) => ast::ExprFunKind::Hinted(ast::ExprHinted { type_hint, inner }),
                None => inner.expr(self.db).clone(),
            };
        }
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
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => *inner,
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
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => *inner,
            _ => unreachable!("caller must peek for BracketOpen before calling"),
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::List(ast::ExprList { type_hint, elements })
    }

    /// Parse tensor with multi-comma syntax: [| data |]
    fn parse_lit_tensor_multicomma(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
        iter: BracerIter<'db>,
    ) -> ast::ExprFunKind<'db> {
        // Filter to non-whitespace tokens for comma-level scanning.
        let tokens_no_ws: Vec<_> = iter
            .filter_map(|t| t.without_space())
            .collect();

        if tokens_no_ws.is_empty() {
            return ast::ExprFunKind::Tensor(ast::ExprTensor {
                type_hint, shape: vec![0], elements: vec![],
            });
        }

        // The widest separator written decides the rank.
        let rank = split::max_comma_run(&tokens_no_ws) + 1;

        // Recursively split by comma levels and parse.
        let (shape, elements) = self.parse_tensor_multicomma_inner(&tokens_no_ws, rank as u32);

        ast::ExprFunKind::Tensor(ast::ExprTensor { type_hint, shape, elements })
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

    /// Parse tensor data from tokens with multi-comma structure.
    fn parse_tensor_multicomma_inner(
        &mut self,
        tokens: &[TreeToken<'db>],
        rank: u32,
    ) -> (Vec<u32>, Vec<ast::ExprFun<'db>>) {
        if rank == 1 {
            // Innermost level: space-separated elements.
            let mut sub = self.new_sub(tokens.to_vec());
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
        let groups = split::split_commas(tokens.iter().cloned(), split_level as usize);
        self.report_stray_delimiters(&groups, "parts");
        let groups = split::nonempty_groups(groups);

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

    /// Parse table: {| header; row1; row2 |}
    fn parse_lit_table(
        &mut self,
        type_hint: Option<datalit::ast::TypeHint<'db>>,
        iter: BracerIter<'db>,
    ) -> ast::ExprFunKind<'db> {
        // Split by row delimiters (newline in whitespace, or semicolon).
        let groups = split::split_lines(self.db, iter);
        self.report_stray_delimiters(&groups, "rows");
        let rows = split::nonempty_groups(groups);

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

    /// Parse table data row (comma-separated expressions).
    fn parse_table_data_row(&mut self, row_tokens: &[TreeToken<'db>]) -> Vec<ast::ExprFun<'db>> {
        let parts = self.split_table_cells(row_tokens);
        let mut elements = Vec::new();

        for part in parts {
            let mut sub = self.new_sub(part);
            let expr = sub.parse_expr_full();
            sub.error_if_not_exhausted();
            self.merge_from_sub(&mut sub);
            elements.push(expr);
        }

        elements
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
        let mut seen = parser_util::SeenNames::default();
        let fields = sub.parse_comma_separated(|p| p.parse_struct_field(&mut seen));
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
        fields
    }

    /// Parse a single struct field: `name = value`.
    fn parse_struct_field(&mut self, seen: &mut parser_util::SeenNames<'db>) -> ast::ExprStructField<'db> {
        let name_ts = self.peek_text_span();
        let name = match self.eat_name() {
            Some(n) => {
                if !seen.take(self.db, n, name_ts, "field") {
                    self.had_error = true;
                }
                n
            }
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
