//! Type hint parsing.

use bct::{
    lexer::Sigil,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{TokenStream, TokenStreamExt};
use super::state::Parser;

impl<'db> Parser<'db> {
    pub(super) fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        // Check for ? or ! prefix for Option/Result types.
        if self.peek_sigil(Sigil::Question) {
            self.eat_sigil(Sigil::Question);
            let inner_type = self.parse_type_hint();
            return ast::TypeHint::Option(ast::TypeHintOption { inner_type: Box::new(inner_type) });
        } else if self.peek_sigil(Sigil::Exclamation) {
            self.eat_sigil(Sigil::Exclamation);
            let inner_type = self.parse_type_hint();
            return ast::TypeHint::Result(ast::TypeHintResult { inner_type: Box::new(inner_type) });
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
            Some("index") => { self.eat_word("index"); ast::TypeHint::Index }
            Some("offset") => { self.eat_word("offset"); ast::TypeHint::Offset }
            Some("f32") => { self.eat_word("f32"); ast::TypeHint::F32 }
            Some("f64") => { self.eat_word("f64"); ast::TypeHint::F64 }
            Some("int") => { self.eat_word("int"); ast::TypeHint::Int }
            Some("string") => { self.eat_word("string"); ast::TypeHint::String }
            Some("data") => { self.eat_word("data"); ast::TypeHint::Data }
            Some("error") => { self.eat_word("error"); ast::TypeHint::Error }
            Some("atom") => {
                self.eat_word("atom");
                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        return self.emit_type_hint_error(ts,
                            "expected name after 'atom'",
                            "D030",
                            "expected atom name"
                        );
                    }
                };
                ast::TypeHint::Atom(ast::TypeHintAtom { name })
            }
            Some("term") => {
                self.eat_word("term");
                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        return self.emit_type_hint_error(ts,
                            "expected name after 'term'",
                            "D031",
                            "expected term name"
                        );
                    }
                };
                let payload = self.parse_type_hint();
                ast::TypeHint::Term(ast::TypeHintTerm { name, payload: Box::new(payload) })
            }
            Some("enum") => {
                let ts = self.peek_text_span();
                self.eat_word("enum");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let variants = sub_parser.parse_comma_separated(|p| p.parse_enum_variant());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Enum(ast::TypeHintEnum { variants })
                } else {
                    self.emit_type_hint_error(ts,
                        "expected '{}' after 'enum'",
                        "D032",
                        "expected '{' after 'enum'"
                    )
                }
            }
            Some("tuple") => {
                let ts = self.peek_text_span();
                self.eat_word("tuple");
                // Check if it's anonymous (starts with () or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Anonymous tuple with explicit keyword.
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple { fields })
                } else {
                    self.emit_type_hint_error(ts,
                        "expected () after tuple keyword",
                        "D001",
                        "expected '(' after 'tuple'"
                    )
                }
            }

            Some("map") => {
                let ts = self.peek_text_span();
                self.eat_word("map");
                // Expect angle bracket with key and value types.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let key_type = sub_parser.parse_type_hint();
                    if !sub_parser.eat_sigil(Sigil::Comma) {
                        let ts = sub_parser.peek_text_span();
                        return self.emit_type_hint_error(ts,
                            "expected comma between map key and value types",
                            "D005",
                            "expected ',' between key and value types"
                        );
                    }
                    let value_type = sub_parser.parse_type_hint();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Map(ast::TypeHintMap { key_type: Box::new(key_type), value_type: Box::new(value_type) })
                } else {
                    self.emit_type_hint_error(ts,
                        "expected <> after map keyword",
                        "D005",
                        "expected '<' after 'map'"
                    )
                }
            }
            Some("set") => {
                let ts = self.peek_text_span();
                self.eat_word("set");
                // Expect angle bracket with element type.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let element_type = sub_parser.parse_type_hint();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Set(ast::TypeHintSet { element_type: Box::new(element_type) })
                } else {
                    self.emit_type_hint_error(ts,
                        "expected <> after set keyword",
                        "D006",
                        "expected '<' after 'set'"
                    )
                }
            }
            // tensor keyword no longer valid as type hint; handled via [| |] branch below.
            _ => {
                // Check for branches: parentheses for tuples, brackets for lists, braces for structs.
                if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Anonymous tuple.
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple { fields })
                } else if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    // List type.
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let element_type = sub_parser.parse_type_hint();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::List(ast::TypeHintList { element_type: Box::new(element_type) })
                } else if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous struct.
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct { fields })
                } else if let Some(iter) = self.eat_branch(Sigil::BracePipeOpen) {
                    // Table type hint.
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let columns = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Table(ast::TypeHintTable { columns })
                } else if let Some(iter) = self.eat_branch(Sigil::BracketPipeOpen) {
                    // Tensor type hint: [|T, N|]
                    let mut sub_parser = Parser::from_branch(self.db, iter, self.source_text());
                    let element_type = sub_parser.parse_type_hint();
                    if !sub_parser.eat_sigil(Sigil::Comma) {
                        let ts = sub_parser.peek_text_span();
                        return self.emit_type_hint_error(ts,
                            "expected comma between tensor element type and rank",
                            "D009",
                            "expected ',' after element type"
                        );
                    }
                    let rank = match sub_parser.parse_u32_literal() {
                        Some(r) => r,
                        None => {
                            let ts = sub_parser.peek_text_span();
                            return self.emit_type_hint_error(ts,
                                "expected rank (positive integer)",
                                "D008",
                                "expected rank"
                            );
                        }
                    };
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::Tensor(ast::TypeHintTensor { element_type: Box::new(element_type), rank })
                } else if let Some(word) = self.peek_word() {
                    // Unknown identifier - could be a type alias.
                    // Check if it looks like a mis-cased primitive first.
                    let lower = word.to_lowercase();
                    match lower.as_str() {
                        "int" | "bool" | "string" | "data" | "error" |
                        "u8" | "i8" | "u16" | "i16" | "u32" | "i32" |
                        "u64" | "i64" | "index" | "offset" | "f32" | "f64" => {
                            let ts = self.peek_text_span();
                            let message = format!("unknown type '{}', did you mean '{}'?", word, lower);
                            self.emit_type_hint_error(ts, &message, "D008", "unexpected token in type hint")
                        }
                        _ => {
                            // Treat as type alias reference.
                            use rmx::prelude::*;
                            let name = InternedText::new(self.db, word.S());
                            self.next(); // consume the identifier
                            ast::TypeHint::Alias(name)
                        }
                    }
                } else {
                    let ts = self.peek_text_span();
                    self.emit_type_hint_error(ts,
                        "unexpected token in type hint",
                        "D008",
                        "unexpected token in type hint"
                    )
                }
            }
        }
    }

    /// Parse a single enum variant: `atom Name` or `term Name Type`.
    fn parse_enum_variant(&mut self) -> ast::TypeHintEnumVariant<'db> {
        match self.peek_word() {
            Some("atom") => {
                self.eat_word("atom");
                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        self.emit_type_hint_error(ts, "expected name after 'atom'", "D030", "expected atom name");
                        use rmx::prelude::*;
                        InternedText::new(self.db, "<error>".S())
                    }
                };
                ast::TypeHintEnumVariant { name, payload: None }
            }
            Some("term") => {
                self.eat_word("term");
                let name = match self.eat_name() {
                    Some(n) => n,
                    None => {
                        let ts = self.peek_text_span();
                        self.emit_type_hint_error(ts, "expected name after 'term'", "D031", "expected term name");
                        use rmx::prelude::*;
                        InternedText::new(self.db, "<error>".S())
                    }
                };
                let payload = self.parse_type_hint();
                ast::TypeHintEnumVariant { name, payload: Some(Box::new(payload)) }
            }
            _ => {
                let ts = self.peek_text_span();
                self.emit_type_hint_error(ts, "expected 'atom' or 'term' in enum variant", "D032", "expected 'atom' or 'term'");
                use rmx::prelude::*;
                let name = InternedText::new(self.db, "<error>".S());
                ast::TypeHintEnumVariant { name, payload: None }
            }
        }
    }

    fn parse_type_hint_named_field(&mut self) -> ast::TypeHintNamedField<'db> {
        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                // No name found - emit error and create placeholder.
                let ts = self.peek_text_span();
                let error_hint = self.emit_type_hint_error(ts.clone(),
                    "expected field name in struct definition",
                    "D010",
                    "expected field name"
                );
                // Create a placeholder name for the error field.
                use rmx::prelude::*;
                let placeholder_name = InternedText::new(self.db, "<error>".S());
                return ast::TypeHintNamedField { name: placeholder_name, type_hint: Box::new(error_hint) };
            }
        };
        if !self.eat_sigil(Sigil::Colon) {
            let ts = self.peek_text_span();
            let error_hint = self.emit_type_hint_error(ts,
                "expected ':' after field name in struct definition",
                "D010",
                "expected ':' after field name"
            );
            return ast::TypeHintNamedField { name, type_hint: Box::new(error_hint) };
        }
        let type_hint = self.parse_type_hint();
        ast::TypeHintNamedField { name, type_hint: Box::new(type_hint) }
    }


}
