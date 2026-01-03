//! Expression parsing.

use rmx::prelude::*;

use bct::{
    lexer::{TokenKind, Sigil},
    bracer::TreeToken,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{self, TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        // Capture span before parsing.
        let (text, span) = self.peek_text_span();

        // Check for `: type / expr` pattern.
        let expr_full = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            if !self.eat_sigil(Sigil::SlashForward) {
                let (err_text, err_span) = self.peek_text_span();
                let error_expr = self.emit_expr_error(
                    err_text,
                    err_span,
                    "expected '/' after type hint",
                    "D012",
                    "expected '/' separator between type hint and expression"
                );
                let expr = ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_expr);
                ast::ExprFull::new(self.db, Some(type_hint), expr)
            } else {
                let expr = self.parse_expr_and_heap();
                ast::ExprFull::new(self.db, Some(type_hint), expr)
            }
        } else {
            // No type hint, just parse expression.
            let expr = self.parse_expr_and_heap();
            ast::ExprFull::new(self.db, None, expr)
        };

        // Record span for this expression.
        use salsa::plumbing::AsId;
        self.expr_spans.push(ast::ParseSpanEntry::new(
            expr_full.as_id(),
            text.as_id(),
            span,
        ));

        expr_full
    }

    fn parse_expr_and_heap(&mut self) -> ast::ExprAndHeap<'db> {
        // Heap sigils: @ for local, # for global.
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else if self.peek_sigil(Sigil::Hash) {
            self.eat_sigil(Sigil::Hash);
            ast::Heap::Global
        } else {
            // No heap sigil - check if it's a bare literal (allowed for inference).
            // Check for negative numbers first (minus sign followed by digits).
            if self.peek_sigil(Sigil::Minus) {
                ast::Heap::Omitted
            } else {
                match self.peek() {
                    Some(TreeToken::Token(token)) => {
                    match token.kind(self.db) {
                        TokenKind::String => {
                            // Bare string literal - use Omitted heap.
                            ast::Heap::Omitted
                        }
                        TokenKind::Word => {
                            if let Some(word) = token.word_str(self.db) {
                                if parser_util::is_numeric_literal(word) {
                                    // Bare number literal (decimal or hex) - use Omitted heap.
                                    ast::Heap::Omitted
                                } else if matches!(word, "data" | "error" | "tensor" | "tuple" | "struct" | "enum" | "map" | "set" | "true" | "false" | "none" | "some" | "ok" | "er") {
                                    // Keywords are allowed without heap sigils.
                                    ast::Heap::Omitted
                                } else {
                                    // Not a number or keyword - this is an error.
                                    let (text, span) = self.peek_text_span();
                                    let error_node = self.emit_expr_error(
                                        text,
                                        span,
                                        "expected heap sigil @ or # before expression",
                                        "D009",
                                        "expected '@' or '#' before expression"
                                    );
                                    return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                                }
                            } else {
                                // No word string - error.
                                let (text, span) = self.peek_text_span();
                                let error_node = self.emit_expr_error(
                                    text,
                                    span,
                                    "expected heap sigil @ or # before expression",
                                    "D010",
                                    "expected '@' or '#' before expression"
                                );
                                return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                            }
                        }
                        _ => {
                            // Unknown token kind - error.
                            let (text, span) = self.peek_text_span();
                            let error_node = self.emit_expr_error(
                                text,
                                span,
                                "expected heap sigil @ or # before expression",
                                "D011",
                                "expected '@' or '#' before expression"
                            );
                            return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                        }
                    }
                }
                Some(TreeToken::Branch(Sigil::ParenOpen, _)) |
                Some(TreeToken::Branch(Sigil::BracketOpen, _)) |
                Some(TreeToken::Branch(Sigil::BraceOpen, _)) => {
                    // Bare branch (anonymous tuple, list, or struct) - use Omitted heap.
                    ast::Heap::Omitted
                }
                _ => {
                    // Not a token or branch - error.
                    let (text, span) = self.peek_text_span();
                    let error_node = self.emit_expr_error(
                        text,
                        span,
                        "expected heap sigil @ or # before expression",
                        "D012",
                        "expected '@' or '#' before expression"
                    );
                    return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                }
                }
            }
        };
        let expr = self.parse_expr();
        ast::ExprAndHeap::new(self.db, heap, expr)
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        // Heap sigil already consumed. Now parse keywords, literals, and structures.
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
                        let is_float = !is_hex && self.peek_sigil(Sigil::Dot) && self.pos + 1 < self.tokens.len() && {
                            if let Some(TreeToken::Token(next_token)) = self.tokens.get(self.pos + 1) {
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
                            let decimal_word = self.need_name();
                            let float_str = format!("-{}.{}", word, decimal_word.as_str(self.db));
                            let value = InternedText::new(self.db, float_str.S());
                            return ast::Expr::Float(ast::ExprFloat::new(self.db, value));
                        } else if is_hex {
                            // Negative hex literal.
                            let hex_str = format!("-{}", word);
                            let value = InternedText::new(self.db, hex_str.S());
                            return ast::Expr::Hex(ast::ExprHex::new(self.db, value));
                        } else {
                            // Negative decimal int.
                            let int_str = format!("-{}", word);
                            let value = InternedText::new(self.db, int_str.S());
                            return ast::Expr::Int(ast::ExprInt::new(self.db, value));
                        }
                    }
                }
            }
            // Not a negative number - this is an error (unexpected minus).
            let (text, span) = self.peek_text_span();
            return self.emit_expr_error(
                text,
                span,
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
                return ast::Expr::Some(ast::ExprSome::new(self.db, payload));
            }
            Some("ok") => {
                self.eat_word("ok");
                let payload = self.parse_expr_full();
                return ast::Expr::Ok(ast::ExprOk::new(self.db, payload));
            }
            Some("er") => {
                self.eat_word("er");
                let payload = self.parse_expr_full();
                return ast::Expr::Er(ast::ExprEr::new(self.db, payload));
            }
            Some("data") => {
                self.eat_word("data");
                let value = self.parse_expr_full();
                return ast::Expr::Data(ast::ExprData::new(self.db, value));
            }
            Some("error") => {
                self.eat_word("error");
                let value = self.parse_expr_full();
                return ast::Expr::Error(ast::ExprError::new(self.db, value));
            }
            Some("tensor") => {
                self.eat_word("tensor");

                // Parse shape: [dim1, dim2, ...]
                let shape = if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens);
                    let shape = sub_parser.parse_comma_separated(|p| {
                        match p.parse_u32_literal() {
                            Some(dim) => dim,
                            None => {
                                p.had_error = true;
                                let (text, span) = p.peek_text_span();
                                DiagnosticBuilder::error(p.db, "expected dimension value in tensor shape")
                                    .code("D023")
                                    .primary_label(text, span, "expected integer")
                                    .emit_parse();
                                0 // Placeholder dimension.
                            }
                        }
                    });
                    sub_parser.error_if_not_exhausted();
                    shape
                } else {
                    let (text, span) = self.peek_text_span();
                    return self.emit_expr_error(
                        text,
                        span,
                        "expected shape [...] after tensor keyword",
                        "D010",
                        "expected '[' for tensor shape"
                    );
                };

                // Parse data: For rank 1, comma-separated elements; for rank 2+, comma-separated rows with space-separated elements.
                let elements = if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    let rank = shape.len();
                    if rank == 0 {
                        let (text, span) = self.peek_text_span();
                        return self.emit_expr_error(
                            text,
                            span,
                            "tensor rank must be at least 1",
                            "D012",
                            "invalid rank"
                        );
                    }

                    if rank == 1 {
                        // 1D tensor: comma-separated elements [1, 2, 3, 4, 5].
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = Parser::new(self.db, tokens);
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
                            let mut row_parser = Parser::new(self.db, elem_tokens.clone());

                            let mut row_elements = Vec::new();
                            while row_parser.pos < row_parser.tokens.len() {
                                row_elements.push(row_parser.parse_expr_full());
                            }

                            // Merge spans from row sub-parser.
                            self.expr_spans.extend(row_parser.expr_spans);

                            // Validate row size matches the last dimension.
                            if row_elements.len() != row_size {
                                let (text, span) = self.peek_text_span();
                                let detailed_message = format!("expected {} elements per row but got {}", row_size, row_elements.len());

                                return self.emit_expr_error(
                                    text,
                                    span,
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
                    let (text, span) = self.peek_text_span();
                    return self.emit_expr_error(
                        text,
                        span,
                        "expected data [...] after tensor shape",
                        "D011",
                        "expected '[' for tensor data"
                    );
                };

                return ast::Expr::Tensor(ast::ExprTensor::new(self.db, shape, elements));
            }
            Some("enum") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("enum");
                // Enum expression syntax: enum Variant or enum Variant(...).
                let variant_name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        return self.emit_expr_error(
                            keyword_text,
                            keyword_span,
                            "expected variant name after enum keyword",
                            "D016",
                            "expected enum variant name"
                        );
                    }
                };
                let payload = if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Parse a single expression as payload.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens);
                    let payload_expr = sub_parser.parse_expr_full();
                    sub_parser.error_if_not_exhausted();
                    // Merge spans from sub-parser.
                    self.expr_spans.extend(sub_parser.expr_spans);
                    Some(payload_expr)
                } else {
                    None
                };
                return ast::Expr::AnonEnum(ast::ExprAnonEnum::new(
                    self.db,
                    variant_name,
                    payload,
                ));
            }
            Some("map") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("map");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens);
                    let entries = sub_parser.parse_comma_separated(|p| {
                        let key = p.parse_expr_full();
                        if !p.eat_sigil(Sigil::Equals) {
                            let (text, span) = p.peek_text_span();
                            let error_expr = p.emit_expr_error(
                                text,
                                span,
                                "expected '=' between map key and value",
                                "D017",
                                "expected '=' after key"
                            );
                            let error_value = ast::ExprFull::new(
                                p.db,
                                None,
                                ast::ExprAndHeap::new(p.db, ast::Heap::Omitted, error_expr)
                            );
                            return ast::ExprMapEntry::new(p.db, key, error_value);
                        }
                        let value = p.parse_expr_full();
                        ast::ExprMapEntry::new(p.db, key, value)
                    });
                    sub_parser.error_if_not_exhausted();
                    // Merge spans from sub-parser.
                    self.expr_spans.extend(sub_parser.expr_spans);
                    return ast::Expr::Map(ast::ExprMap::new(self.db, entries));
                } else {
                    return self.emit_expr_error(
                        keyword_text,
                        keyword_span,
                        "expected {} after map keyword",
                        "D016",
                        "expected '{' after 'map'"
                    );
                }
            }
            Some("set") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("set");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens);
                    let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                    sub_parser.error_if_not_exhausted();
                    // Merge spans from sub-parser.
                    self.expr_spans.extend(sub_parser.expr_spans);
                    return ast::Expr::Set(ast::ExprSet::new(self.db, elements));
                } else {
                    return self.emit_expr_error(
                        keyword_text,
                        keyword_span,
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
                            if !is_hex && self.peek_sigil(Sigil::Dot) && self.pos + 1 < self.tokens.len() {
                                if let Some(TreeToken::Token(next_token)) = self.tokens.get(self.pos + 1) {
                                    if let Some(decimal_part) = next_token.word_str(self.db) {
                                        if decimal_part.chars().all(|c| c.is_ascii_digit()) {
                                            // It's a float!
                                            self.eat_sigil(Sigil::Dot);
                                            let decimal_word = self.need_name();
                                            let float_str = format!("{}.{}", word, decimal_word.as_str(self.db));
                                            let value = InternedText::new(self.db, float_str.S());
                                            return ast::Expr::Float(ast::ExprFloat::new(self.db, value));
                                        }
                                    }
                                }
                            }
                            // Not a float - check if hex or decimal.
                            let value = InternedText::new(self.db, word.S());
                            if is_hex {
                                ast::Expr::Hex(ast::ExprHex::new(self.db, value))
                            } else {
                                ast::Expr::Int(ast::ExprInt::new(self.db, value))
                            }
                        } else {
                            // Not a number, parse error for bare identifiers.
                            let (text, span) = self.peek_text_span();
                            self.next();
                            let _message = InternedText::new(
                                self.db,
                                format!("Unexpected identifier: {}", word).S()
                            );

                            self.emit_expr_error(
                                text,
                                span,
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
                        ast::Expr::String(ast::ExprString::new(self.db, value))
                    }
                    _ => {
                        let (text, span) = self.peek_text_span();
                        self.emit_expr_error(
                            text,
                            span,
                            "unexpected token in Parser expression",
                            "D020",
                            "unexpected token"
                        )
                    }
                }
            }
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                // Tuple.
                self.next(); // Consume the branch.
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut sub_parser = Parser::new(self.db, tokens);
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::AnonTuple(ast::ExprAnonTuple::new(self.db, elements))
            }
            Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                // Struct.
                self.next(); // Consume the branch.
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut sub_parser = Parser::new(self.db, tokens);
                let fields = sub_parser.parse_comma_separated(|p| p.parse_expr_struct_field());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::AnonStruct(ast::ExprAnonStruct::new(self.db, fields))
            }
            Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => {
                // List.
                self.next(); // Consume the branch.
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut sub_parser = Parser::new(self.db, tokens);
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::List(ast::ExprList::new(self.db, elements))
            }
            _ => {
                let (text, span) = self.peek_text_span();
                self.emit_expr_error(
                    text,
                    span,
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
                let (text, span) = self.peek_text_span();
                let error_expr = self.emit_expr_error(
                    text.clone(),
                    span.clone(),
                    "expected field name in struct expression",
                    "D018",
                    "expected field name"
                );
                let placeholder_name = InternedText::new(self.db, "<error>".S());
                let error_value = ast::ExprFull::new(
                    self.db,
                    None,
                    ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_expr)
                );
                return ast::ExprStructField::new(self.db, placeholder_name, error_value);
            }
        };
        if !self.eat_sigil(Sigil::Equals) {
            let (text, span) = self.peek_text_span();
            let error_expr = self.emit_expr_error(
                text,
                span,
                "expected '=' after field name in struct expression",
                "D018",
                "expected '=' after field name"
            );
            let error_value = ast::ExprFull::new(
                self.db,
                None,
                ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_expr)
            );
            return ast::ExprStructField::new(self.db, name, error_value);
        }
        let value = self.parse_expr_full();
        ast::ExprStructField::new(self.db, name, value)
    }

    pub(super) fn parse_comma_separated<T>(&mut self, mut parse_fn: impl FnMut(&mut Self) -> T) -> Vec<T> {
        let mut items = vec![];
        if self.pos >= self.tokens.len() {
            return items;
        }
        loop {
            items.push(parse_fn(self));
            if self.peek_sigil(Sigil::Comma) {
                self.eat_sigil(Sigil::Comma);
                // Handle trailing comma: if we're at the end, stop parsing.
                if self.pos >= self.tokens.len() {
                    break;
                }
            } else {
                break;
            }
        }
        items
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
}
