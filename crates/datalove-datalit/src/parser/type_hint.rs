//! Type hint parsing.

use bct::{
    lexer::Sigil,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::TokenStreamExt;
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_type_hint_and_heap(&mut self) -> ast::TypeHintAndHeap<'db> {
        // Heap sigils: @ for local, # for global.
        // If omitted, defaults to Heap::Omitted (inferred).
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else if self.peek_sigil(Sigil::Hash) {
            self.eat_sigil(Sigil::Hash);
            ast::Heap::Global
        } else {
            // No heap sigil - default to Omitted (inferred).
            ast::Heap::Omitted
        };
        let type_hint = self.parse_type_hint();
        ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        // Heap sigil already consumed. Check for ? or ! prefix for Option/Result types.
        if self.peek_sigil(Sigil::Question) {
            self.eat_sigil(Sigil::Question);
            let inner_type = self.parse_type_hint_and_heap();
            return ast::TypeHint::Option(ast::TypeHintOption::new(self.db, inner_type));
        } else if self.peek_sigil(Sigil::Exclamation) {
            self.eat_sigil(Sigil::Exclamation);
            let inner_type = self.parse_type_hint_and_heap();
            return ast::TypeHint::Result(ast::TypeHintResult::new(self.db, inner_type));
        }

        // Parse base type.
        match self.peek_word() {
            Some("bool") => { self.eat_word("bool"); ast::TypeHint::Bool }
            Some("u8") => { self.eat_word("u8"); ast::TypeHint::U8 }
            Some("i8") => { self.eat_word("i8"); ast::TypeHint::I8 }
            Some("u16") => { self.eat_word("u16"); ast::TypeHint::U16 }
            Some("i16") => { self.eat_word("i16"); ast::TypeHint::I16 }
            Some("u32") => { self.eat_word("u32"); ast::TypeHint::U32 }
            Some("i32") => { self.eat_word("i32"); ast::TypeHint::I32 }
            Some("u64") => { self.eat_word("u64"); ast::TypeHint::U64 }
            Some("i64") => { self.eat_word("i64"); ast::TypeHint::I64 }
            Some("f32") => { self.eat_word("f32"); ast::TypeHint::F32 }
            Some("int") => { self.eat_word("int"); ast::TypeHint::Int }
            Some("string") => { self.eat_word("string"); ast::TypeHint::String }
            Some("data") => { self.eat_word("data"); ast::TypeHint::Data }
            Some("error") => { self.eat_word("error"); ast::TypeHint::Error }
            Some("tuple") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("tuple");
                // Check if it's anonymous (starts with () or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Anonymous tuple with explicit keyword.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_and_heap());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple::new(self.db, fields))
                } else {
                    self.emit_type_hint_error(
                        keyword_text,
                        keyword_span,
                        "expected () after tuple keyword",
                        "D001",
                        "expected '(' after 'tuple'"
                    )
                }
            }
            Some("struct") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("struct");
                // Check if it's anonymous (starts with {) or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous struct with explicit keyword.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct::new(self.db, fields))
                } else {
                    self.emit_type_hint_error(
                        keyword_text,
                        keyword_span,
                        "expected {} after struct keyword",
                        "D002",
                        "expected '{' after 'struct'"
                    )
                }
            }
            Some("enum") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("enum");
                // Check if it's anonymous (starts with {) or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous enum.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let variants = sub_parser.parse_comma_separated(|p| p.parse_type_hint_enum_variant());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonEnum(ast::TypeHintAnonEnum::new(
                        self.db,
                        variants,
                    ))
                } else {
                    self.emit_type_hint_error(
                        keyword_text,
                        keyword_span,
                        "expected {} after enum keyword",
                        "D003",
                        "expected '{' after 'enum'"
                    )
                }
            }
            Some("map") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("map");
                // Expect angle bracket with key and value types.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let key_type = sub_parser.parse_type_hint_and_heap();
                    if !sub_parser.eat_sigil(Sigil::Comma) {
                        let (text, span) = sub_parser.peek_text_span();
                        return self.emit_type_hint_error(
                            text,
                            span,
                            "expected comma between map key and value types",
                            "D005",
                            "expected ',' between key and value types"
                        );
                    }
                    let value_type = sub_parser.parse_type_hint_and_heap();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Map(ast::TypeHintMap::new(self.db, key_type, value_type))
                } else {
                    self.emit_type_hint_error(
                        keyword_text,
                        keyword_span,
                        "expected <> after map keyword",
                        "D005",
                        "expected '<' after 'map'"
                    )
                }
            }
            Some("set") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("set");
                // Expect angle bracket with element type.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let element_type = sub_parser.parse_type_hint_and_heap();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Set(ast::TypeHintSet::new(self.db, element_type))
                } else {
                    self.emit_type_hint_error(
                        keyword_text,
                        keyword_span,
                        "expected <> after set keyword",
                        "D006",
                        "expected '<' after 'set'"
                    )
                }
            }
            Some("tensor") => {
                let (keyword_text, keyword_span) = self.peek_text_span();
                self.eat_word("tensor");
                // Expect angle bracket with <element_type, rank, optional_layout>.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let element_type = sub_parser.parse_type_hint_and_heap();
                    if !sub_parser.eat_sigil(Sigil::Comma) {
                        let (text, span) = sub_parser.peek_text_span();
                        return self.emit_type_hint_error(
                            text,
                            span,
                            "expected comma between tensor element type and rank",
                            "D009",
                            "expected ',' after element type"
                        );
                    }

                    let rank = match sub_parser.parse_u32_literal() {
                        Some(r) => r,
                        None => {
                            let (text, span) = sub_parser.peek_text_span();
                            return self.emit_type_hint_error(
                                text,
                                span,
                                "expected rank (positive integer)",
                                "D008",
                                "expected rank"
                            );
                        }
                    };

                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Tensor(ast::TypeHintTensor::new(
                        self.db,
                        element_type,
                        rank,
                    ))
                } else {
                    self.emit_type_hint_error(
                        keyword_text,
                        keyword_span,
                        "expected <> after tensor keyword",
                        "D009",
                        "expected '<' after 'tensor'"
                    )
                }
            }
            _ => {
                // Check for branches: parentheses for tuples, brackets for lists, braces for structs.
                if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Anonymous tuple.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_and_heap());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple::new(self.db, fields))
                } else if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    // List type.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let element_type = sub_parser.parse_type_hint_and_heap();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::List(ast::TypeHintList::new(self.db, element_type))
                } else if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous struct.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = Parser::new(self.db, tokens, self.source_text);
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct::new(self.db, fields))
                } else {
                    let (text, span) = self.peek_text_span();
                    // Check if this looks like a capitalized type name.
                    let message = if let Some(word) = self.peek_word() {
                        let lower = word.to_lowercase();
                        match lower.as_str() {
                            "int" | "bool" | "string" | "data" | "error" |
                            "u8" | "i8" | "u16" | "i16" | "u32" | "i32" |
                            "u64" | "i64" | "f32" => {
                                format!("unknown type '{}', did you mean '{}'?", word, lower)
                            }
                            _ => format!("unknown type '{}'", word)
                        }
                    } else {
                        "unexpected token in type hint".to_string()
                    };
                    self.emit_type_hint_error(
                        text,
                        span,
                        &message,
                        "D008",
                        "unexpected token in type hint"
                    )
                }
            }
        }
    }

    fn parse_type_hint_named_field(&mut self) -> ast::TypeHintNamedField<'db> {
        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                // No name found - emit error and create placeholder.
                let (text, span) = self.peek_text_span();
                let error_hint = self.emit_type_hint_error(
                    text.clone(),
                    span.clone(),
                    "expected field name in struct definition",
                    "D010",
                    "expected field name"
                );
                // Create a placeholder name for the error field.
                use rmx::prelude::*;
                let placeholder_name = InternedText::new(self.db, "<error>".S());
                let type_hint = ast::TypeHintAndHeap::new(self.db, ast::Heap::Omitted, error_hint);
                return ast::TypeHintNamedField::new(self.db, placeholder_name, type_hint);
            }
        };
        if !self.eat_sigil(Sigil::Colon) {
            let (text, span) = self.peek_text_span();
            let error_hint = self.emit_type_hint_error(
                text,
                span,
                "expected ':' after field name in struct definition",
                "D010",
                "expected ':' after field name"
            );
            let type_hint = ast::TypeHintAndHeap::new(self.db, ast::Heap::Omitted, error_hint);
            return ast::TypeHintNamedField::new(self.db, name, type_hint);
        }
        let type_hint = self.parse_type_hint_and_heap();
        ast::TypeHintNamedField::new(self.db, name, type_hint)
    }

    fn parse_type_hint_enum_variant(&mut self) -> ast::TypeHintEnumVariant<'db> {
        use rmx::prelude::*;
        use datalove_diagnostic::DiagnosticBuilder;

        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                // No name found - emit error and create placeholder.
                self.had_error = true;
                let (text, span) = self.peek_text_span();
                DiagnosticBuilder::error(self.db, "expected variant name in enum definition")
                    .code("D011")
                    .primary_label(text, span, "expected variant name")
                    .emit_parse();
                // Create a placeholder name for the error variant.
                return ast::TypeHintEnumVariant::new(
                    self.db,
                    InternedText::new(self.db, "<error>".S()),
                    None
                );
            }
        };
        let payload = if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
            // Parse a single type as payload.
            let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
            let mut sub_parser = Parser::new(self.db, tokens.clone(), self.source_text);
            let payload_type = sub_parser.parse_type_hint_and_heap();

            // Check for unparsed tokens - this indicates a syntax error.
            if sub_parser.pos < tokens.len() {
                // There are extra tokens after the payload type.
                let (text, span) = sub_parser.peek_text_span();
                let message = InternedText::new(
                    self.db,
                    "enum variant payload must be a single type (use a tuple for multiple values)".S()
                );
                let error_type = ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message));
                return ast::TypeHintEnumVariant::new(
                    self.db,
                    name,
                    Some(ast::TypeHintAndHeap::new(self.db, ast::Heap::Omitted, error_type))
                );
            }

            Some(payload_type)
        } else {
            None
        };
        ast::TypeHintEnumVariant::new(self.db, name, payload)
    }
}
