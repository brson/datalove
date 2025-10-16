use rmx::prelude::*;

use bct::{
    input::Source,
    chunk::Chunk,
    lexer::{
        Token,
        TokenKind,
        Sigil
    },
    bracer::{
        Bracer,
        BracerIter,
        TreeToken,
    },
    text::InternedText,
    source_map,
    lexer,
    bracer,
};

use crate::ast;

#[salsa::tracked]
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer(db, bracer)
}

#[salsa::tracked]
fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
) -> ast::ExprFull<'db> {
    let tokens = bracer.iter(db).filter_map(|t| t.without_space(db)).collect::<Vec<_>>();
    let mut dyn_parser = DynParser {
        db,
        tokens,
        pos: 0,
    };
    dyn_parser.parse_expr_full()
}

struct DynParser<'db> {
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    pos: usize,
}

impl<'db> DynParser<'db> {
    fn parse_type_hint_and_heap(&mut self) -> ast::TypeHintAndHeap<'db> {
        // Heap sigils: @ for local, # for global.
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else if self.peek_sigil(Sigil::Hash) {
            self.eat_sigil(Sigil::Hash);
            ast::Heap::Global
        } else {
            // Parse error: missing heap sigil. Use Omitted heap and create error node.
            let message = InternedText::new(self.db, "expected heap sigil @ or # before type".S());
            let error_node = ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message));
            return ast::TypeHintAndHeap::new(self.db, ast::Heap::Omitted, error_node);
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
                self.eat_word("tuple");
                let name = self.need_name();
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_and_heap());
                        ast::TypeHint::NamedTuple(ast::TypeHintNamedTuple::new(
                            self.db,
                            name,
                            fields,
                        ))
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected () after tuple keyword".S());
                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                    }
                }
            }
            Some("struct") => {
                self.eat_word("struct");
                let name = self.need_name();
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                        ast::TypeHint::NamedStruct(ast::TypeHintNamedStruct::new(
                            self.db,
                            name,
                            fields,
                        ))
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected {} after struct keyword".S());
                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                    }
                }
            }
            Some("enum") => {
                self.eat_word("enum");
                // Check if it's anonymous (starts with {) or named (starts with name).
                if self.peek_sigil(Sigil::BraceOpen) {
                    // Anonymous enum.
                    match self.peek() {
                        Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                            self.next(); // Consume the branch.
                            let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                            let mut sub_parser = DynParser {
                                db: self.db,
                                tokens,
                                pos: 0,
                            };
                            let variants = sub_parser.parse_comma_separated(|p| p.parse_type_hint_enum_variant());
                            ast::TypeHint::AnonEnum(ast::TypeHintAnonEnum::new(
                                self.db,
                                variants,
                            ))
                        }
                        _ => {
                            let message = InternedText::new(self.db, "expected {} after enum keyword".S());
                            ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                        }
                    }
                } else {
                    // Named enum.
                    let name = self.need_name();
                    match self.peek() {
                        Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                            self.next(); // Consume the branch.
                            let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                            let mut sub_parser = DynParser {
                                db: self.db,
                                tokens,
                                pos: 0,
                            };
                            let variants = sub_parser.parse_comma_separated(|p| p.parse_type_hint_enum_variant());
                            ast::TypeHint::NamedEnum(ast::TypeHintNamedEnum::new(
                                self.db,
                                name,
                                variants,
                            ))
                        }
                        _ => {
                            let message = InternedText::new(self.db, "expected {} after enum name".S());
                            ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                        }
                    }
                }
            }
            Some("map") => {
                self.eat_word("map");
                // Expect angle bracket with key and value types.
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::AngleOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let key_type = sub_parser.parse_type_hint_and_heap();
                        sub_parser.need_sigil(Sigil::Comma);
                        let value_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::Map(ast::TypeHintMap::new(self.db, key_type, value_type))
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected <> after map keyword".S());
                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                    }
                }
            }
            Some("set") => {
                self.eat_word("set");
                // Expect angle bracket with element type.
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::AngleOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let element_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::Set(ast::TypeHintSet::new(self.db, element_type))
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected <> after set keyword".S());
                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                    }
                }
            }
            Some("list") => {
                self.eat_word("list");
                // Expect angle bracket with element type.
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::AngleOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let element_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::List(ast::TypeHintList::new(self.db, element_type))
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected <> after list keyword".S());
                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                    }
                }
            }
            _ => {
                // Check for branches: parentheses for tuples, brackets for lists, braces for structs.
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                        // Anonymous tuple.
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_and_heap());
                        ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple::new(self.db, fields))
                    }
                    Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => {
                        // List type.
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let element_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::List(ast::TypeHintList::new(self.db, element_type))
                    }
                    Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                        // Anonymous struct.
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                        ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct::new(self.db, fields))
                    }
                    _ => {
                        let message = InternedText::new(self.db, "unknown type hint in DynParser".S());
                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message))
                    }
                }
            }
        }
    }

    fn parse_type_hint_named_field(&mut self) -> ast::TypeHintNamedField<'db> {
        let name = self.need_name();
        self.need_sigil(Sigil::Colon);
        let type_hint = self.parse_type_hint_and_heap();
        ast::TypeHintNamedField::new(self.db, name, type_hint)
    }

    fn parse_type_hint_enum_variant(&mut self) -> ast::TypeHintEnumVariant<'db> {
        let name = self.need_name();
        let payload = if let Some(TreeToken::Branch(Sigil::ParenOpen, iter)) = self.peek() {
            // Parse a single type as payload.
            self.next(); // Consume the branch.
            let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
            let mut sub_parser = DynParser {
                db: self.db,
                tokens: tokens.clone(),
                pos: 0,
            };
            let payload_type = sub_parser.parse_type_hint_and_heap();

            // Check for unparsed tokens - this indicates a syntax error.
            if sub_parser.pos < tokens.len() {
                // There are extra tokens after the payload type.
                let message = InternedText::new(
                    self.db,
                    "enum variant payload must be a single type (use a tuple for multiple values)".S()
                );
                let error_type = ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, message));
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

    fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        // Check for `: type / expr` pattern.
        if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            self.need_sigil(Sigil::SlashForward);
            let expr = self.parse_expr_and_heap();
            ast::ExprFull::new(self.db, Some(type_hint), expr)
        } else {
            // No type hint, just parse expression.
            let expr = self.parse_expr_and_heap();
            ast::ExprFull::new(self.db, None, expr)
        }
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
            match self.peek() {
                Some(TreeToken::Token(token)) => {
                    match token.kind(self.db) {
                        TokenKind::String => {
                            // Bare string literal - use Omitted heap.
                            ast::Heap::Omitted
                        }
                        TokenKind::Word => {
                            if let Some(word) = token.word_str(self.db) {
                                if word.chars().all(|c| c.is_ascii_digit()) {
                                    // Bare number literal - use Omitted heap.
                                    ast::Heap::Omitted
                                } else {
                                    // Not a number - this is an error.
                                    let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());
                                    let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                                    return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                                }
                            } else {
                                // No word string - error.
                                let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());
                                let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                                return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                            }
                        }
                        _ => {
                            // Unknown token kind - error.
                            let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());
                            let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
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
                    let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());
                    let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                    return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
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
                    if word.chars().all(|c| c.is_ascii_digit()) {
                        // It's a negative number! Consume the digits.
                        self.next();
                        // Check for float pattern (dot then more digits).
                        let is_float = if self.peek_sigil(Sigil::Dot) && self.pos + 1 < self.tokens.len() {
                            if let Some(TreeToken::Token(next_token)) = self.tokens.get(self.pos + 1) {
                                if let Some(decimal_part) = next_token.word_str(self.db) {
                                    decimal_part.chars().all(|c| c.is_ascii_digit())
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if is_float {
                            // Negative float: -number.number
                            self.eat_sigil(Sigil::Dot);
                            let decimal_word = self.need_name();
                            let float_str = format!("-{}.{}", word, decimal_word.as_str(self.db));
                            let value = InternedText::new(self.db, float_str.S());
                            return ast::Expr::Float(ast::ExprFloat::new(self.db, value));
                        } else {
                            // Negative int: -number
                            let int_str = format!("-{}", word);
                            let value = InternedText::new(self.db, int_str.S());
                            return ast::Expr::Int(ast::ExprInt::new(self.db, value));
                        }
                    }
                }
            }
            // Not a negative number - this is an error (unexpected minus).
            let message = InternedText::new(self.db, "unexpected minus sign".S());
            return ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
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
            Some("data") => {
                self.eat_word("data");
                let value = self.parse_expr_full();
                return ast::Expr::Data(ast::ExprData::new(self.db, value));
            }
            Some("error") => {
                self.eat_word("error");
                let value = self.parse_expr_full();
                return ast::Expr::Err(ast::ExprErr::new(self.db, value));
            }
            Some("tuple") => {
                self.eat_word("tuple");
                let name = self.need_name();
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                        return ast::Expr::NamedTuple(ast::ExprNamedTuple::new(
                            self.db,
                            name,
                            elements,
                        ));
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected () after tuple name".S());
                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                    }
                }
            }
            Some("struct") => {
                self.eat_word("struct");
                let name = self.need_name();
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_expr_struct_field());
                        return ast::Expr::NamedStruct(ast::ExprNamedStruct::new(
                            self.db,
                            name,
                            fields,
                        ));
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected {} after struct name".S());
                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                    }
                }
            }
            Some("enum") => {
                self.eat_word("enum");
                // Enum expression syntax:
                // - enum Variant (anonymous, no payload)
                // - enum Variant(...) (anonymous, with single value payload)
                // - enum EnumName.Variant (named, with dot separator)
                // - enum EnumName.Variant(...) (named, with single value payload)
                let first_name = self.need_name();
                if self.peek_sigil(Sigil::Dot) {
                    // Named enum: enum EnumName.Variant [(...)]
                    self.eat_sigil(Sigil::Dot);
                    let variant_name = self.need_name();
                    let payload = if let Some(TreeToken::Branch(Sigil::ParenOpen, iter)) = self.peek() {
                        // Parse a single expression as payload.
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        Some(sub_parser.parse_expr_full())
                    } else {
                        None
                    };
                    return ast::Expr::NamedEnum(ast::ExprNamedEnum::new(
                        self.db,
                        first_name,
                        variant_name,
                        payload,
                    ));
                } else {
                    // Anonymous enum: enum Variant [(...)]
                    let variant_name = first_name;
                    let payload = if let Some(TreeToken::Branch(Sigil::ParenOpen, iter)) = self.peek() {
                        // Parse a single expression as payload.
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        Some(sub_parser.parse_expr_full())
                    } else {
                        None
                    };
                    return ast::Expr::AnonEnum(ast::ExprAnonEnum::new(
                        self.db,
                        variant_name,
                        payload,
                    ));
                }
            }
            Some("map") => {
                self.eat_word("map");
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let entries = sub_parser.parse_comma_separated(|p| {
                            let key = p.parse_expr_full();
                            p.need_sigil(Sigil::Equals);
                            let value = p.parse_expr_full();
                            ast::ExprMapEntry::new(p.db, key, value)
                        });
                        return ast::Expr::Map(ast::ExprMap::new(self.db, entries));
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected {} after map keyword".S());
                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                    }
                }
            }
            Some("set") => {
                self.eat_word("set");
                match self.peek() {
                    Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                        self.next(); // Consume the branch.
                        let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                        };
                        let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                        return ast::Expr::Set(ast::ExprSet::new(self.db, elements));
                    }
                    _ => {
                        let message = InternedText::new(self.db, "expected {} after set keyword".S());
                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, message));
                    }
                }
            }
            _ => {}
        }

        // Not a keyword, check for numbers, tokens, or branches.
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).X();
                        if word.chars().all(|c| c.is_ascii_digit()) {
                            self.next();
                            // Check for float pattern (number followed by dot and number).
                            if self.peek_sigil(Sigil::Dot) && self.pos + 1 < self.tokens.len() {
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
                            // Not a float - all numeric literals are Expr::Int.
                            let value = InternedText::new(self.db, word.S());
                            ast::Expr::Int(ast::ExprInt::new(self.db, value))
                        } else {
                            // Not a number, parse error for bare identifiers.
                            self.next();
                            let message = InternedText::new(
                                self.db,
                                format!("Unexpected identifier: {}", word).S()
                            );
                            ast::Expr::ParseError(ast::ExprParseError::new(self.db, message))
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
                        let message = InternedText::new(self.db, "unexpected token in DynParser expression".S());
                        ast::Expr::ParseError(ast::ExprParseError::new(self.db, message))
                    }
                }
            }
            Some(TreeToken::Branch(Sigil::ParenOpen, iter)) => {
                // Tuple.
                self.next(); // Consume the branch.
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut sub_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                };
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                ast::Expr::AnonTuple(ast::ExprAnonTuple::new(self.db, elements))
            }
            Some(TreeToken::Branch(Sigil::BraceOpen, iter)) => {
                // Struct.
                self.next(); // Consume the branch.
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut sub_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                };
                let fields = sub_parser.parse_comma_separated(|p| p.parse_expr_struct_field());
                ast::Expr::AnonStruct(ast::ExprAnonStruct::new(self.db, fields))
            }
            Some(TreeToken::Branch(Sigil::BracketOpen, iter)) => {
                // List.
                self.next(); // Consume the branch.
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut sub_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                };
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                ast::Expr::List(ast::ExprList::new(self.db, elements))
            }
            _ => {
                let message = InternedText::new(self.db, "unexpected tree node in DynParser expression".S());
                ast::Expr::ParseError(ast::ExprParseError::new(self.db, message))
            }
        }
    }

    fn parse_expr_struct_field(&mut self) -> ast::ExprStructField<'db> {
        let name = self.need_name();
        self.need_sigil(Sigil::Equals);
        let value = self.parse_expr_full();
        ast::ExprStructField::new(self.db, name, value)
    }

    fn parse_comma_separated<T>(&mut self, mut parse_fn: impl FnMut(&mut Self) -> T) -> Vec<T> {
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

    fn peek(&self) -> Option<TreeToken<'db>> {
        self.tokens.get(self.pos).cloned()
    }

    fn peek_sigil(&self, sigil: Sigil) -> bool {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db), TokenKind::Sigil(s) if s == sigil)
            }
            Some(TreeToken::Branch(s, _)) => s == sigil,
            None => false,
        }
    }

    fn peek_word(&self) -> Option<&'db str> {
        match self.peek() {
            Some(TreeToken::Token(token)) => token.word_str(self.db),
            _ => None,
        }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn eat_sigil(&mut self, sigil: Sigil) {
        match self.next() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Sigil(s) if s == sigil => return,
                    _ => {}
                }
            }
            _ => {}
        }
        panic!("expected sigil {}", sigil.as_str());
    }

    fn need_sigil(&mut self, sigil: Sigil) {
        self.eat_sigil(sigil)
    }

    fn eat_word(&mut self, word: &str) {
        match self.next() {
            Some(TreeToken::Token(token)) => {
                match token.word_str(self.db) {
                    Some(w) if w == word => return,
                    _ => {}
                }
            }
            _ => {}
        }
        panic!("expected word '{}'", word);
    }

    fn need_name(&mut self) -> InternedText<'db> {
        match self.next() {
            Some(TreeToken::Token(token)) => {
                match token.word_str(self.db) {
                    Some(word) => InternedText::new(self.db, word.S()),
                    None => panic!("expected name"),
                }
            }
            _ => panic!("expected name"),
        }
    }
}

#[test]
fn test_parse_bool() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@true"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_bool_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @bool / @true"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    assert!(matches!(type_hint, ast::TypeHint::Bool));
    let expr = ast.expr(db).expr(db);
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_int() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@42"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Int(e) => assert_eq!(e.value(db).as_str(db), "42"),
        _ => panic!("expected int"),
    }
}

#[test]
fn test_parse_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@(@true, @1)"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements(db).len(), 2),
        _ => panic!("expected tuple"),
    }
}

#[test]
fn test_parse_list() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@[@1, @2, @3]"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements(db).len(), 3),
        _ => panic!("expected list"),
    }
}

#[test]
fn test_parse_float() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@1.0"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Float(e) => assert_eq!(e.value(db).as_str(db), "1.0"),
        ast::Expr::Int(_) => panic!("expected float, got Int"),
        _ => panic!("expected float, got something else"),
    }
}

#[test]
fn test_parse_float_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @f32 / @1.0"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    assert!(matches!(type_hint, ast::TypeHint::F32));
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Float(e) => assert_eq!(e.value(db).as_str(db), "1.0"),
        _ => panic!("expected float"),
    }
}

#[test]
fn test_parse_anon_enum_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @enum { Foo, Bar: @u32 } / @enum Foo"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::AnonEnum(e) => {
            let variants = e.variants(db);
            assert_eq!(variants.len(), 2);
        }
        _ => panic!("expected anonymous enum type hint"),
    }
}

#[test]
fn test_parse_string() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(r#": @string / @"hello world""#));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::String(s) => {
            assert_eq!(s.value(db).as_str(db), r#""hello world""#);
        }
        _ => panic!("expected string"),
    }
}

#[test]
fn test_parse_struct() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @struct Foo { field1: @bool } / @struct Foo { field1 = @true }"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::NamedStruct(s) => {
            assert_eq!(s.name(db).as_str(db), "Foo");
            assert_eq!(s.fields(db).len(), 1);
        }
        _ => panic!("expected named struct type hint"),
    }
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::NamedStruct(s) => {
            assert_eq!(s.name(db).as_str(db), "Foo");
            assert_eq!(s.fields(db).len(), 1);
        }
        _ => panic!("expected named struct expr"),
    }
}

#[test]
fn test_parse_map() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @map <@u32, @u32> / @map { @0 = @5, @2 = @2 }"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::Map(_) => {}
        _ => panic!("expected map type hint"),
    }
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Map(m) => {
            assert_eq!(m.entries(db).len(), 2);
        }
        _ => panic!("expected map expr"),
    }
}

#[test]
fn test_parse_set() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @set <@u32> / @set { @1, @2, @3 }"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::Set(_) => {}
        _ => panic!("expected set type hint"),
    }
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::Set(s) => {
            assert_eq!(s.elements(db).len(), 3);
        }
        _ => panic!("expected set expr"),
    }
}

#[test]
fn test_parse_named_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @tuple Bar (@bool, @u32) / @tuple Bar (@true, @1)"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::NamedTuple(t) => {
            assert_eq!(t.name(db).as_str(db), "Bar");
            assert_eq!(t.fields(db).len(), 2);
        }
        _ => panic!("expected named tuple type hint"),
    }
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::NamedTuple(t) => {
            assert_eq!(t.name(db).as_str(db), "Bar");
            assert_eq!(t.elements(db).len(), 2);
        }
        _ => panic!("expected named tuple expr"),
    }
}

#[test]
fn test_parse_enum_variant_no_payload() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Foo"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Foo");
            assert!(e.payload(db).is_none());
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_enum_variant_with_payload() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Bar(@2)"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Bar");
            assert!(e.payload(db).is_some());
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_enum_variant_with_tuple() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Baz(@(@true, @1))"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Baz");
            assert!(e.payload(db).is_some());
            // Verify the payload is a tuple.
            let payload = e.payload(db).unwrap();
            match payload.expr(db).expr(db) {
                ast::Expr::AnonTuple(t) => assert_eq!(t.elements(db).len(), 2),
                _ => panic!("expected tuple payload"),
            }
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_named_enum_with_dot() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Quux.Bar(@(@true, @1))"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::NamedEnum(e) => {
            assert_eq!(e.enum_name(db).as_str(db), "Quux");
            assert_eq!(e.variant_name(db).as_str(db), "Bar");
            assert!(e.payload(db).is_some());
            // Verify the payload is a tuple.
            let payload = e.payload(db).unwrap();
            match payload.expr(db).expr(db) {
                ast::Expr::AnonTuple(t) => assert_eq!(t.elements(db).len(), 2),
                _ => panic!("expected tuple payload"),
            }
        }
        _ => panic!("expected named enum"),
    }
}

#[test]
fn test_parse_enum_variant_with_extra_tokens_error() {
    let ref db = crate::Database::default();
    // This should error: Ok(@u32, @string) - multiple types without explicit tuple.
    let source = Source::new(db, S(": @enum { Ok(@u32, @string) } / @enum Ok(@1)"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    match type_hint {
        ast::TypeHint::AnonEnum(e) => {
            let variants = e.variants(db);
            assert_eq!(variants.len(), 1);
            let variant = &variants[0];
            assert_eq!(variant.name(db).as_str(db), "Ok");
            // Check that the payload contains a parse error.
            match variant.payload(db) {
                Some(payload_type) => {
                    match payload_type.type_hint(db) {
                        ast::TypeHint::ParseError(_) => {
                            // Expected! This is the parse error for extra tokens.
                        }
                        _ => panic!("expected parse error for extra tokens in enum variant payload"),
                    }
                }
                None => panic!("expected payload with parse error"),
            }
        }
        _ => panic!("expected anonymous enum type hint"),
    }
}

#[test]
fn test_parse_list_multiline() {
    // Datalit parser doesn't split on newlines, but newlines in whitespace are fine.
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@[\n@1,\n@2,\n@3\n]"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements(db).len(), 3),
        _ => panic!("expected list"),
    }
}

#[test]
fn test_parse_tuple_multiline() {
    // Datalit parser doesn't split on newlines, but newlines in whitespace are fine.
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@(\n@true,\n@1\n)"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements(db).len(), 2),
        _ => panic!("expected tuple"),
    }
}
