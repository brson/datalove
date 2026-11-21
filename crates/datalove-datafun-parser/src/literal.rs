//! Literal expression parsing.
//!
//! Handles datalit-style literals within datafun expressions.

use datalove_diagnostic_codes::{Code, DCode, PCode};
use rmx::prelude::*;

use bct::{
    lexer::Sigil,
    bracer::{Cursor, TreeToken},
    split,
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{self, TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_datalit::parser_util::EmitTo;
use super::state::Parser;

impl<'db> Parser<'db> {
    /// Parse `: type / expression`.
    ///
    /// The hint is on a unary expression and the postfix operators written on
    /// it, whatever the expression is: `: u32 / o?` hints what the `?`
    /// produces, `: u8 / (p).x` the field, and `: u8 / (a, b).0` the element.
    /// Nothing about the operand changes how far the hint reaches.
    pub(super) fn parse_hinted(&mut self) -> ast::ExprId {
        // The span runs from the hint to the end of what it is on, so that a
        // diagnostic about either points at both.
        let ts = self.peek_text_span();
        self.eat_sigil(Sigil::Colon);
        let type_hint = self.parse_type_hint();
        let broken = matches!(type_hint, datalit::ast::TypeHint::ParseError(_));
        if broken {
            self.skip_broken_hint_type();
        }
        // Whatever goes wrong goes under the hint, as in datalit, so that a
        // hint is one node however it was written.
        let inner = if !self.eat_sigil(Sigil::SlashForward) {
            if let datalit::ast::TypeHint::ParseError(e) = &type_hint {
                // With no `/` there is no telling where the type was meant to
                // end, and a missing `/` would say the same thing twice.
                let kind = ast::ExprFunKind::ParseError(ast::ExprFunParseError {
                    source: e.source, span: e.span.C(), message: e.message,
                });
                self.create_expr(kind, TextSpan::new(e.source, e.span.C()))
            } else {
                let ts = self.peek_text_span();
                self.emit_expr_error(ts,
                    "expected '/' after type hint in `: type / expr` pattern",
                    DCode::HintWithoutSlash,
                    "expected '/'"
                )
            }
        } else if self.peek_colon_type_hint() {
            // Two hints on one expression would have to agree, and the
            // second says nothing the first does not.
            let ts = self.peek_text_span();
            self.next();
            self.emit_expr_error(ts,
                "a type hint cannot be written directly under another",
                PCode::UnexpectedToken,
                "parenthesize the inner hint"
            )
        } else {
            let start = self.current_byte_pos();
            let inner = self.parse_expr_primary();
            if matches!(self.expr_kind(inner), ast::ExprFunKind::ParseError(_)) {
                inner
            } else {
                self.parse_postfix_try_operators(inner, start)
            }
        };
        let ts = TextSpan::new(ts.source, ts.span.start..self.last_byte_end());
        self.create_expr(ast::ExprFunKind::Hinted(ast::ExprHinted { type_hint, inner }), ts)
    }

    /// Parse a number, `true`, `false`, `none`, or a bracketed literal.
    ///
    /// The rest of the literal forms -- strings, the constructors, atoms,
    /// terms, enums and parentheses -- are read where any expression is, by
    /// [`Self::parse_expr_primary`].
    pub(super) fn parse_lit_expr(&mut self) -> ast::ExprFunKind<'db> {
        // A number. In an expression the prefix operator has already claimed
        // a `-`, so there is no sign for this to take.
        if let Some(number) = parser_util::eat_number(self) {
            return self.number_expr(number);
        }

        match self.peek_word() {
            Some("true") => {
                self.eat_word("true");
                return ast::ExprFunKind::True;
            }
            Some("false") => {
                self.eat_word("false");
                return ast::ExprFunKind::False;
            }
            Some("none") => {
                self.eat_word("none");
                return ast::ExprFunKind::None;
            }
            _ => {}
        }

        match self.peek() {
            Some(TreeToken::Token(_)) | None => {
                unreachable!("parse_expr_primary reads every other token")
            }
            Some(TreeToken::Branch { sigil: Sigil::ParenOpen, .. }) => {
                unreachable!("a parenthesis is read by parse_datafun_tuple")
            }
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, .. }) => {
                // Anonymous struct.
                return self.parse_lit_anon_struct();
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, .. }) => {
                // List.
                return self.parse_lit_list();
            }
            Some(TreeToken::Branch { sigil: Sigil::PercentBraceOpen, .. }) => {
                // Map: %{k -> v, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::PercentBraceOpen, inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let entries = self.parse_comma_separated_map_entries(inner);
                return ast::ExprFunKind::Map(ast::ExprMap { entries });
            }
            Some(TreeToken::Branch { sigil: Sigil::HashBraceOpen, .. }) => {
                // Set: #{e, ...}
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::HashBraceOpen, inner, .. }) => inner,
                    _ => unreachable!(),
                };
                let elements = self.parse_comma_separated_exprs(inner);
                return ast::ExprFunKind::Set(ast::ExprSet { elements });
            }
            Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, .. }) => {
                // Table.
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BracePipeOpen, inner, .. }) => inner,
                    _ => unreachable!(),
                };
                return self.parse_lit_table(inner);
            }
            Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, .. }) => {
                // Tensor: [| data |] with multi-comma separators.
                let inner = match self.next() {
                    Some(TreeToken::Branch { sigil: Sigil::BracketPipeOpen, inner, .. }) => inner,
                    _ => unreachable!(),
                };
                return self.parse_lit_tensor_multicomma(inner);
            }
            _ => {
                // A bracket with no expression of its own, as `(| |)` and
                // `<a>` are; taken whole, so that it is reported once.
                let ts = self.error_span();
                self.next();
                return self.lit_error(ts, "expected expression", PCode::ExpectedExpression,
                    "this bracket starts no expression");
            }
        }
    }


    /// Build the expression for a number, or report how it was written.
    fn number_expr(&mut self, number: parser_util::Number) -> ast::ExprFunKind<'db> {
        let ts = TextSpan::new(self.source_text(), number.span.C());

        if let Some((message, label)) = number.complaint() {
            return self.lit_error(ts, &message, DCode::MalformedNumber, &label);
        }
        if let Some(suffix) = &number.suffix {
            let (message, label) = parser_util::suffix_complaint(suffix);
            return self.lit_error(ts, &message, DCode::NumberSuffix, &label);
        }

        let value = InternedText::new(self.db, number.text());
        match (number.radix, number.float) {
            (parser_util::Radix::Hex, _) => ast::ExprFunKind::Hex(ast::ExprHex { value }),
            (parser_util::Radix::Dec, true) => ast::ExprFunKind::Float(ast::ExprFloat { value }),
            (parser_util::Radix::Dec, false) => ast::ExprFunKind::Int(ast::ExprInt { value }),
        }
    }

    /// Report how a literal was written, and stand in for it.
    fn lit_error(
        &mut self,
        ts: TextSpan,
        message: &str,
        code: impl Code,
        label: &str,
    ) -> ast::ExprFunKind<'db> {
        self.had_error = true;
        DiagnosticBuilder::error(self.db, message)
            .code(code.as_str())
            .primary_label(ts.C(), label)
            .emit_to(&self.sink);
        ast::ExprFunKind::ParseError(ast::ExprFunParseError {
            source: ts.source,
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

    /// Parse anonymous struct: { name = expr, ... }
    ///
    /// Caller must have already peeked and confirmed a `BraceOpen` branch.
    fn parse_lit_anon_struct(
        &mut self,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BraceOpen, inner, .. }) => inner,
            _ => unreachable!("caller must peek for BraceOpen before calling"),
        };

        let fields = self.parse_comma_separated_struct_fields(inner);
        ast::ExprFunKind::AnonStruct(ast::ExprAnonStruct { fields })
    }

    /// Parse list: [expr, expr, ...]
    ///
    /// Caller must have already peeked and confirmed a `BracketOpen` branch.
    fn parse_lit_list(
        &mut self,
    ) -> ast::ExprFunKind<'db> {
        let inner = match self.next() {
            Some(TreeToken::Branch { sigil: Sigil::BracketOpen, inner, .. }) => inner,
            _ => unreachable!("caller must peek for BracketOpen before calling"),
        };

        let elements = self.parse_comma_separated_exprs(inner);
        ast::ExprFunKind::List(ast::ExprList { elements })
    }

    /// Parse tensor with multi-comma syntax: [| data |]
    fn parse_lit_tensor_multicomma(
        &mut self,
        iter: Cursor<'db>,
    ) -> ast::ExprFunKind<'db> {
        let (header, body) = split::split_tensor_header(self.text(), iter);
        let header = match header {
            None => None,
            Some(Ok(header)) => Some(header),
            Some(Err(e)) => {
                let ts = TextSpan::new(self.source_text(), e.span);
                return self.lit_error(ts, &e.message, DCode::BadTensorHeader, &e.label);
            }
        };

        // The widest separator written decides the rank. `[| |]` has no
        // separator and no element, which is the empty tensor of rank 1.
        let rank = split::max_comma_run(body) + 1;
        let (shape, elements) = self.parse_tensor_multicomma_inner(body, rank as u32);

        let Some(header) = header else {
            return ast::ExprFunKind::Tensor(ast::ExprTensor { shape, elements, header: false });
        };

        // Under a header the body is written flat, filled in row-major order,
        // or shaped exactly as the header says.
        let written = parser_util::tensor_body_complaint(&header.extents, rank, &shape, elements.len());
        if let Some((message, label)) = written {
            let ts = TextSpan::new(self.source_text(), header.span);
            return self.lit_error(ts, &message, DCode::TensorBodyMismatch, &label);
        }
        ast::ExprFunKind::Tensor(ast::ExprTensor { shape: header.extents, elements, header: true })
    }

    /// Split a table row into its cells, reporting any stray `,`.
    fn split_table_cells(&mut self, tokens: Cursor<'db>) -> Vec<Cursor<'db>> {
        let groups = split::split_commas(tokens, 1);
        self.report_stray_delimiters(&groups, "columns");
        split::nonempty_groups(groups)
    }

    /// Report every delimiter that was written with nothing before it.
    fn report_stray_delimiters(&mut self, groups: &[split::TokenGroup<'db>], what: &str) {
        for written in split::stray_delimiters(groups) {
            self.had_error = true;
            split::stray_delimiter_error(self.db, self.source_text(), &written, what)
                .code(DCode::EmptyBeforeComma.as_str())
                .emit_to(&self.sink);
        }
    }

    /// Parse tensor data from tokens with multi-comma structure.
    fn parse_tensor_multicomma_inner(
        &mut self,
        tokens: Cursor<'db>,
        rank: u32,
    ) -> (Vec<u32>, Vec<ast::ExprId>) {
        if rank == 1 {
            // Innermost level: space-separated elements.
            let mut sub = self.new_sub(tokens);
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
        let groups = split::split_commas(tokens, split_level as usize);
        self.report_stray_delimiters(&groups, "parts");
        if let Some(written) = split::trailing_delimiter(&groups) {
            self.had_error = true;
            split::trailing_delimiter_error(self.db, self.source_text(), &written, "parts")
                .code(DCode::EmptyBeforeComma.as_str())
                .emit_to(&self.sink);
        }
        let groups = split::nonempty_groups(groups);

        if groups.is_empty() {
            return (vec![0; rank as usize], vec![]);
        }

        let mut all_elements = Vec::new();
        let mut inner_shape: Option<Vec<u32>> = None;

        for (group_idx, &group) in groups.iter().enumerate() {
            let (sub_shape, sub_elements) = self.parse_tensor_multicomma_inner(group, rank - 1);
            match &inner_shape {
                None => inner_shape = Some(sub_shape),
                Some(expected) => {
                    if *expected != sub_shape {
                        let ts = self.extract_group_text_span(group);
                        DiagnosticBuilder::error(self.db,
                            &format!("inconsistent tensor shape at group {}: expected {:?} but got {:?}",
                                group_idx, expected, sub_shape))
                            .code(DCode::TensorShapeMismatch.as_str())
                            .primary_label(ts, "shape mismatch")
                            .emit_to(&self.sink);
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
        iter: Cursor<'db>,
    ) -> ast::ExprFunKind<'db> {
        // A row is a line, ended by a newline or a `;`.
        let rows: Vec<_> = split::Lines::new(iter).collect();

        if rows.is_empty() {
            // Empty table: {||}.
            return ast::ExprFunKind::Table(ast::ExprTable {
                header: vec![],
                rows: vec![],
            });
        }

        // First row is header (column names).
        let header = self.parse_table_header(rows[0]);
        let num_columns = header.len();

        // Remaining rows are data.
        let mut data_rows = Vec::new();
        for (row_idx, &row_tokens) in rows.iter().skip(1).enumerate() {
            let elements = self.parse_table_data_row(row_tokens);
            if elements.len() != num_columns && !elements.is_empty() {
                let ts = self.extract_group_text_span(row_tokens);
                DiagnosticBuilder::error(self.db,
                    &format!("row {} has {} columns but header has {}",
                        row_idx + 1, elements.len(), num_columns))
                    .code(DCode::TableRowWidth.as_str())
                    .primary_label(ts, "column count mismatch")
                    .emit_to(&self.sink);
            }
            data_rows.push(ast::ExprTableRow { elements });
        }

        ast::ExprFunKind::Table(ast::ExprTable { header, rows: data_rows })
    }

    /// Parse table header row (column names).
    ///
    /// A cell is one word and nothing else. Anything after the word is
    /// reported rather than dropped, so that `{| x zzz |}` is a refusal and
    /// not a one-column table.
    fn parse_table_header(&mut self, row_tokens: Cursor<'db>) -> Vec<InternedText<'db>> {
        let parts = self.split_table_cells(row_tokens);
        let mut names = Vec::new();
        let mut seen = parser_util::SeenNames::default();

        for part in parts {
            // A group `nonempty_groups` kept has a first token.
            let first = part.first().X();
            let Some(name) = self.cell_word(&first).filter(|w| parser_util::is_identifier(w)) else {
                self.had_error = true;
                let ts = self.extract_text_span(&first);
                DiagnosticBuilder::error(self.db, "expected column name in table header")
                    .code(DCode::TableHeaderNotName.as_str())
                    .primary_label(ts, "expected name")
                    .emit_to(&self.sink);
                names.push(InternedText::new(self.db, "<error>".S()));
                continue;
            };
            let name_text = InternedText::new(self.db, name.S());
            let ts = self.extract_text_span(&first);
            if !seen.take(self.db, &self.sink, name_text, ts, "column") {
                self.had_error = true;
            }
            names.push(name_text);

            if let Some(extra) = part.peek_nth(1) {
                self.had_error = true;
                let ts = self.extract_text_span(&extra);
                DiagnosticBuilder::error(self.db,
                    &format!("unexpected token after column name `{}`", name))
                    .code(DCode::TableHeaderExtraToken.as_str())
                    .primary_label(ts, "a column is named by one word")
                    .emit_to(&self.sink);
            }
        }

        names
    }

    /// The word a header cell begins with, if it begins with one.
    fn cell_word(&self, token: &TreeToken<'db>) -> Option<&'db str> {
        match token {
            TreeToken::Token(tok) => tok.word_str(self.text()),
            TreeToken::Branch { .. } => None,
        }
    }

    /// Parse table data row (comma-separated expressions).
    fn parse_table_data_row(&mut self, row_tokens: Cursor<'db>) -> Vec<ast::ExprId> {
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
    pub(super) fn parse_comma_separated_exprs(&mut self, iter: Cursor<'db>) -> Vec<ast::ExprId> {
        let mut sub = self.sub_parser(iter, None);
        let elements = sub.parse_comma_separated(|p| p.parse_expr_full());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
        elements
    }

    /// Helper to parse comma-separated struct fields.
    fn parse_comma_separated_struct_fields(&mut self, iter: Cursor<'db>) -> Vec<ast::ExprStructField<'db>> {
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
                if !seen.take(self.db, &self.sink, n, name_ts, "field") {
                    self.had_error = true;
                }
                n
            }
            None => {
                let ts = self.peek_text_span();
                let error_expr = self.emit_expr_error(ts,
                    "expected field name in struct",
                    DCode::StructFieldSyntax,
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
                DCode::StructFieldSyntax,
                "expected '='"
            );
            return ast::ExprStructField { name, value: error_expr };
        }

        let value = self.parse_expr_full();
        ast::ExprStructField { name, value }
    }

    /// Helper to parse comma-separated map entries.
    fn parse_comma_separated_map_entries(&mut self, iter: Cursor<'db>) -> Vec<ast::ExprMapEntry> {
        let mut sub = self.sub_parser(iter, None);
        let entries = sub.parse_comma_separated(|p| p.parse_map_entry());
        sub.error_if_not_exhausted();
        self.merge_from_sub(&mut sub);
        entries
    }

    /// Parse a single map entry: `key -> value`.
    fn parse_map_entry(&mut self) -> ast::ExprMapEntry {
        let key = self.parse_expr_full();

        if !self.eat_sigil(Sigil::Arrow) {
            let ts = self.peek_text_span();
            // An operator written against one side ended the key, as it ends
            // any expression, and that is the error (P091).
            let error_value = match self.lopsided_operator() {
                Some((code, message, label)) => self.emit_expr_error(ts, &message, code, &label),
                None => self.emit_expr_error(ts,
                    "expected '->' between map key and value",
                    DCode::MapEntryWithoutArrow,
                    "expected '->'"
                ),
            };
            return ast::ExprMapEntry { key, value: error_value };
        }

        let value = self.parse_expr_full();
        ast::ExprMapEntry { key, value }
    }
}
