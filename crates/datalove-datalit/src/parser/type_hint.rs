//! Type hint parsing.

use rmx::prelude::*;

use bct::{
    lexer::Sigil,
    text::InternedText,
};

use bct::diagnostic::SpanEntry;
use crate::ast;
use crate::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use super::state::Parser;

/// A token stream a type hint can be read out of.
///
/// Datafun reads one from the middle of a statement, where a type is followed
/// by the `=` of a `let`, the `,` between two parameters, or the `with` that
/// opens a bounds clause. So the parser has to stop where the type stops and
/// leave the rest where it found it. Cutting the tokens at a guessed boundary
/// beforehand cannot do that: a caller that guessed wrong has already consumed
/// what it guessed past, and has nowhere to put it back.
pub trait TypeHintStream<'db>: TokenStream<'db> {
    /// Record a parse error and return the node that stands in for the type.
    ///
    /// Each parser keeps its own error flag and its own accumulator, so the
    /// stream reports rather than the type hint parser.
    fn type_hint_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::TypeHint<'db>;

    /// File a bare name in type position and return the index it was filed at.
    ///
    /// Only the parser knows where its spans go, the same way it knows where
    /// its errors go. An alias is the one type a hint cannot resolve on its
    /// own, so it is the one a diagnostic has to be able to point at.
    fn alias_index(&mut self, ts: TextSpan<'db>) -> u32;

    /// The index the next alias filed against this stream would take.
    fn alias_base(&self) -> u32;

    /// Take over the aliases a branch's own parser filed, and its numbering.
    ///
    /// What is inside a branch is a whole type read by a datalit parser
    /// whatever the outer stream is, so the aliases in it are filed over
    /// there and have to be brought back for the outer stream's table to be
    /// the one the indices count into.
    fn absorb_aliases(&mut self, spans: Vec<SpanEntry>, next: u32);
}

/// Read one type hint, consuming exactly the tokens that are part of it.
pub fn parse_type_hint<'db, S: TypeHintStream<'db>>(stream: &mut S) -> ast::TypeHint<'db> {
    // Check for ? or ! prefix for Option/Result types.
    if stream.peek_sigil(Sigil::Question) {
        stream.eat_sigil(Sigil::Question);
        let inner_type = parse_type_hint(stream);
        return ast::TypeHint::Option(ast::TypeHintOption { inner_type: Box::new(inner_type) });
    } else if stream.peek_sigil(Sigil::Exclamation) {
        stream.eat_sigil(Sigil::Exclamation);
        let inner_type = parse_type_hint(stream);
        return ast::TypeHint::Result(ast::TypeHintResult { inner_type: Box::new(inner_type) });
    }

    // A branch is a whole type on its own, so what is inside one is all type
    // and a datalit parser reads it however the outer stream is spelled.
    let db = stream.db();
    let text = stream.source_text();
    let branch = |iter, base| Parser::from_branch_numbering_aliases(db, iter, text, base);

    // Parse base type.
    match stream.peek_word() {
        Some("bool") => { stream.eat_word("bool"); ast::TypeHint::Bool }
        Some("u8") => { stream.eat_word("u8"); ast::TypeHint::U8 }
        Some("i8") => { stream.eat_word("i8"); ast::TypeHint::I8 }
        Some("u16") => { stream.eat_word("u16"); ast::TypeHint::U16 }
        Some("i16") => { stream.eat_word("i16"); ast::TypeHint::I16 }
        Some("u32") => { stream.eat_word("u32"); ast::TypeHint::U32 }
        Some("i32") => { stream.eat_word("i32"); ast::TypeHint::I32 }
        Some("u64") => { stream.eat_word("u64"); ast::TypeHint::U64 }
        Some("i64") => { stream.eat_word("i64"); ast::TypeHint::I64 }
        Some("index") => { stream.eat_word("index"); ast::TypeHint::Index }
        Some("offset") => { stream.eat_word("offset"); ast::TypeHint::Offset }
        Some("f32") => { stream.eat_word("f32"); ast::TypeHint::F32 }
        Some("f64") => { stream.eat_word("f64"); ast::TypeHint::F64 }
        Some("int") => { stream.eat_word("int"); ast::TypeHint::Int }
        Some("string") => { stream.eat_word("string"); ast::TypeHint::String }
        Some("data") => { stream.eat_word("data"); ast::TypeHint::Data }
        Some("error") => { stream.eat_word("error"); ast::TypeHint::Error }
        Some("atom") => {
            stream.eat_word("atom");
            let name = match stream.eat_name() {
                Some(n) => n,
                None => {
                    let ts = stream.peek_text_span();
                    return stream.type_hint_error(ts,
                        "expected name after 'atom'",
                        "D030",
                        "expected atom name"
                    );
                }
            };
            ast::TypeHint::Atom(ast::TypeHintAtom { name })
        }
        Some("term") => {
            stream.eat_word("term");
            let name = match stream.eat_name() {
                Some(n) => n,
                None => {
                    let ts = stream.peek_text_span();
                    return stream.type_hint_error(ts,
                        "expected name after 'term'",
                        "D031",
                        "expected term name"
                    );
                }
            };
            let payload = parse_type_hint(stream);
            ast::TypeHint::Term(ast::TypeHintTerm { name, payload: Box::new(payload) })
        }
        Some("enum") => {
            let ts = stream.peek_text_span();
            stream.eat_word("enum");
            if let Some(iter) = stream.eat_branch(Sigil::BraceOpen) {
                let mut sub_parser = branch(iter, stream.alias_base());
                let variants = sub_parser.parse_comma_separated(|p| p.parse_enum_variant());
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::Enum(ast::TypeHintEnum { variants })
            } else {
                stream.type_hint_error(ts,
                    "expected '{}' after 'enum'",
                    "D032",
                    "expected '{' after 'enum'"
                )
            }
        }
        Some("tuple") => {
            let ts = stream.peek_text_span();
            stream.eat_word("tuple");
            // Check if it's anonymous (starts with () or named (starts with name).
            if let Some(iter) = stream.eat_branch(Sigil::ParenOpen) {
                // Anonymous tuple with explicit keyword.
                let mut sub_parser = branch(iter, stream.alias_base());
                let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint());
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple { fields })
            } else {
                stream.type_hint_error(ts,
                    "expected () after tuple keyword",
                    "D001",
                    "expected '(' after 'tuple'"
                )
            }
        }

        // tensor/map/set keywords no longer valid as type hints; handled via sigil branches below.
        _ => {
            // Check for branches: parentheses for tuples, brackets for lists, braces for structs.
            if let Some(iter) = stream.eat_branch(Sigil::ParenOpen) {
                // Anonymous tuple.
                let mut sub_parser = branch(iter, stream.alias_base());
                let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint());
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple { fields })
            } else if let Some(iter) = stream.eat_branch(Sigil::BracketOpen) {
                // List type.
                let mut sub_parser = branch(iter, stream.alias_base());
                let element_type = sub_parser.parse_type_hint();
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::List(ast::TypeHintList { element_type: Box::new(element_type) })
            } else if let Some(iter) = stream.eat_branch(Sigil::BraceOpen) {
                // Anonymous struct.
                let mut sub_parser = branch(iter, stream.alias_base());
                let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct { fields })
            } else if let Some(iter) = stream.eat_branch(Sigil::BracePipeOpen) {
                // Table type hint.
                let mut sub_parser = branch(iter, stream.alias_base());
                let columns = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::Table(ast::TypeHintTable { columns })
            } else if let Some(iter) = stream.eat_branch(Sigil::PercentBraceOpen) {
                // Map type hint: %{K = V}
                let mut sub_parser = branch(iter, stream.alias_base());
                let key_type = sub_parser.parse_type_hint();
                if !sub_parser.eat_sigil(Sigil::Equals) {
                    let ts = sub_parser.peek_text_span();
                    return stream.type_hint_error(ts,
                        "expected '=' between map key and value types",
                        "D005",
                        "expected '=' between key and value types"
                    );
                }
                let value_type = sub_parser.parse_type_hint();
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::Map(ast::TypeHintMap { key_type: Box::new(key_type), value_type: Box::new(value_type) })
            } else if let Some(iter) = stream.eat_branch(Sigil::HashBraceOpen) {
                // Set type hint: #{T}
                let mut sub_parser = branch(iter, stream.alias_base());
                let element_type = sub_parser.parse_type_hint();
                sub_parser.error_if_not_exhausted_type_hint();
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                ast::TypeHint::Set(ast::TypeHintSet { element_type: Box::new(element_type) })
            } else if let Some(iter) = stream.eat_branch(Sigil::BracketPipeOpen) {
                // Tensor type hint: [|T, N|]
                let mut sub_parser = branch(iter, stream.alias_base());
                let element_type = sub_parser.parse_type_hint();
                // What goes wrong is carried out rather than returned from
                // here, so that the aliases the element type filed are handed
                // back on every way out of this branch and not just the one
                // that reaches a tensor.
                let rank = if !sub_parser.eat_sigil(Sigil::Comma) {
                    Err((
                        sub_parser.peek_text_span(),
                        "expected comma between tensor element type and rank",
                        "D009",
                        "expected ',' after element type",
                    ))
                } else {
                    match sub_parser.parse_u32_literal() {
                        Some(r) => Ok(r),
                        None => Err((
                            sub_parser.peek_text_span(),
                            "expected rank (positive integer)",
                            "D008",
                            "expected rank",
                        )),
                    }
                };
                if rank.is_ok() {
                    sub_parser.error_if_not_exhausted_type_hint();
                }
                stream.absorb_aliases(sub_parser.take_alias_spans(), sub_parser.alias_counter);
                match rank {
                    Ok(rank) => ast::TypeHint::Tensor(
                        ast::TypeHintTensor { element_type: Box::new(element_type), rank },
                    ),
                    Err((ts, message, code, label)) => {
                        stream.type_hint_error(ts, message, code, label)
                    }
                }
            } else if let Some(word) = stream.peek_word() {
                // Unknown identifier - could be a type alias.
                // Check if it looks like a mis-cased primitive first.
                let lower = word.to_lowercase();
                match lower.as_str() {
                    "int" | "bool" | "string" | "data" | "error" |
                    "u8" | "i8" | "u16" | "i16" | "u32" | "i32" |
                    "u64" | "i64" | "index" | "offset" | "f32" | "f64" => {
                        let ts = stream.peek_text_span();
                        let message = format!("unknown type '{}', did you mean '{}'?", word, lower);
                        stream.type_hint_error(ts, &message, "D008", "unexpected token in type hint")
                    }
                    // A collection type is written with its sigil, and these
                    // are the names of the collections rather than the types.
                    // `tuple` and `enum` are read before this and say their
                    // own piece about the brace that has to follow them.
                    "list" | "map" | "set" | "table" | "tensor" => {
                        let ts = stream.peek_text_span();
                        let spelled = match lower.as_str() {
                            "list" => "[T]",
                            "map" => "%{K = V}",
                            "set" => "#{T}",
                            "table" => "{| name: T |}",
                            "tensor" => "[|T, N|]",
                            _ => unreachable!("matched one of the five just above"),
                        };
                        let message = format!(
                            "unknown type '{}', did you mean '{}'?", word, spelled,
                        );
                        stream.type_hint_error(ts, &message, "D008", "a collection type is written with its sigil")
                    }
                    _ => {
                        // Treat as type alias reference.
                        let name = InternedText::new(stream.db(), word.S());
                        let ts = stream.peek_text_span();
                        stream.next(); // consume the identifier
                        let local_index = Some(stream.alias_index(ts));
                        ast::TypeHint::Alias(ast::TypeHintAlias { name, local_index })
                    }
                }
            } else {
                let ts = stream.peek_text_span();
                stream.type_hint_error(ts,
                    "unexpected token in type hint",
                    "D008",
                    "unexpected token in type hint"
                )
            }
        }
    }
}

impl<'db> TypeHintStream<'db> for Parser<'db> {
    fn type_hint_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::TypeHint<'db> {
        self.emit_type_hint_error(ts, message, code, label)
    }

    /// Number the alias without keeping where it was.
    ///
    /// Datalit resolves no aliases -- a bare name in a data literal's type is
    /// always an error, and the datafun layer above is what has a map to look
    /// one up in -- so there is nothing here that would point at one.
    fn alias_index(&mut self, ts: TextSpan<'db>) -> u32 {
        let index = self.alias_counter;
        self.alias_counter += 1;
        self.alias_spans.push(SpanEntry::new(ts.text.source(self.db), ts.span));
        index
    }

    fn alias_base(&self) -> u32 {
        self.alias_counter
    }

    fn absorb_aliases(&mut self, spans: Vec<SpanEntry>, next: u32) {
        self.alias_spans.extend(spans);
        self.alias_counter = next;
    }
}

impl<'db> Parser<'db> {
    pub(super) fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        parse_type_hint(self)
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
