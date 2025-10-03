use rmx::prelude::*;

use rmx::core::iter::Peekable;
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
    let mut parser = Parser {
        db,
        tokens: bracer.iter(db).filter_map(|t| t.without_space(db)).peekable(),
    };
    parser.parse_expr_full()
}

struct Parser<'db, I>
where I: Iterator<Item = TreeToken<'db>>
{
    db: &'db dyn crate::Db,
    tokens: Peekable<I>,
}

struct DynParser<'db> {
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    pos: usize,
}

impl<'db, I> Parser<'db, I>
where I: Iterator<Item = TreeToken<'db>>
{
    fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        // Check for `: type / expr` pattern.
        if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            self.need_sigil(Sigil::SlashForward);
            let expr = self.parse_expr_and_heap();
            ast::ExprFull::new(self.db, type_hint, expr)
        } else {
            // No type hint, just parse expression.
            let expr = self.parse_expr_and_heap();
            let type_hint = ast::TypeHintAndHeap::new(
                self.db,
                ast::Heap::Omitted,
                ast::TypeHint::Data,
            );
            ast::ExprFull::new(self.db, type_hint, expr)
        }
    }

    fn parse_type_hint_and_heap(&mut self) -> ast::TypeHintAndHeap<'db> {
        // Heap sigils: @ for local. (# for global not yet supported by bct lexer.)
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else {
            panic!("expected heap sigil @ before type");
        };
        let type_hint = self.parse_type_hint();
        ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        // Heap sigil already consumed, now parse the type name.
        match self.peek_word() {
            Some("bool") => {
                self.eat_word("bool");
                ast::TypeHint::Bool
            }
            Some("u32") => {
                self.eat_word("u32");
                ast::TypeHint::U32
            }
            Some("f32") => {
                self.eat_word("f32");
                ast::TypeHint::F32
            }
            Some("int") => {
                self.eat_word("int");
                ast::TypeHint::Int
            }
            Some("nil") => {
                self.eat_word("nil");
                ast::TypeHint::Nil
            }
            Some("string") => {
                self.eat_word("string");
                ast::TypeHint::String
            }
            Some("data") => {
                self.eat_word("data");
                ast::TypeHint::Data
            }
            Some("error") => {
                self.eat_word("error");
                ast::TypeHint::Error
            }
            Some("token") => {
                self.eat_word("token");
                let name = self.need_name();
                ast::TypeHint::Token(ast::TypeHintToken::new(self.db, name))
            }
            Some("tuple") => {
                self.eat_word("tuple");
                let name = self.need_name();
                let fields = self.parse_branch(Sigil::ParenOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_type_hint_and_heap())
                });
                ast::TypeHint::NamedTuple(ast::TypeHintNamedTuple::new(
                    self.db,
                    name,
                    fields,
                ))
            }
            Some("struct") => {
                self.eat_word("struct");
                let name = self.need_name();
                let fields = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_type_hint_named_field())
                });
                ast::TypeHint::NamedStruct(ast::TypeHintNamedStruct::new(
                    self.db,
                    name,
                    fields,
                ))
            }
            Some("enum") => {
                self.eat_word("enum");
                // Check if it's anonymous (starts with {) or named (starts with name).
                if self.peek_sigil(Sigil::BraceOpen) {
                    // Anonymous enum.
                    let variants = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                        p.parse_comma_separated(|p| p.parse_type_hint_enum_variant())
                    });
                    ast::TypeHint::AnonEnum(ast::TypeHintAnonEnum::new(
                        self.db,
                        variants,
                    ))
                } else {
                    // Named enum.
                    let name = self.need_name();
                    let variants = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                        p.parse_comma_separated(|p| p.parse_type_hint_enum_variant())
                    });
                    ast::TypeHint::NamedEnum(ast::TypeHintNamedEnum::new(
                        self.db,
                        name,
                        variants,
                    ))
                }
            }
            Some("map") => {
                self.eat_word("map");
                let (key_type, value_type) = self.parse_branch(Sigil::AngleOpen, &|p: &mut DynParser<'db>| {
                    let key_type = p.parse_type_hint_and_heap();
                    p.need_sigil(Sigil::Comma);
                    let value_type = p.parse_type_hint_and_heap();
                    (key_type, value_type)
                });
                ast::TypeHint::Map(ast::TypeHintMap::new(
                    self.db,
                    key_type,
                    value_type,
                ))
            }
            Some("set") => {
                self.eat_word("set");
                let element_type = self.parse_branch(Sigil::AngleOpen, &|p: &mut DynParser<'db>| {
                    p.parse_type_hint_and_heap()
                });
                ast::TypeHint::Set(ast::TypeHintSet::new(self.db, element_type))
            }
            _ => {
                // Check for brackets [T] for list type.
                if self.peek_sigil(Sigil::BracketOpen) {
                    let element_type = self.parse_branch(Sigil::BracketOpen, &|p: &mut DynParser<'db>| {
                        p.parse_type_hint_and_heap()
                    });
                    ast::TypeHint::List(ast::TypeHintList::new(self.db, element_type))
                } else if self.peek_sigil(Sigil::ParenOpen) {
                    // Anonymous tuple.
                    let fields = self.parse_branch(Sigil::ParenOpen, &|p: &mut DynParser<'db>| {
                        p.parse_comma_separated(|p| p.parse_type_hint_and_heap())
                    });
                    ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple::new(
                        self.db,
                        fields,
                    ))
                } else if self.peek_sigil(Sigil::BraceOpen) {
                    // Anonymous struct.
                    let fields = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                        p.parse_comma_separated(|p| p.parse_type_hint_named_field())
                    });
                    ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct::new(
                        self.db,
                        fields,
                    ))
                } else {
                    todo!("unknown type hint")
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
        let payload = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint_and_heap())
        } else {
            None
        };
        ast::TypeHintEnumVariant::new(self.db, name, payload)
    }

    fn parse_expr_and_heap(&mut self) -> ast::ExprAndHeap<'db> {
        // Heap sigils: @ for local. (# for global not yet supported by bct lexer.)
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else {
            panic!("expected heap sigil @ before expression");
        };
        let expr = self.parse_expr();
        ast::ExprAndHeap::new(self.db, heap, expr)
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        // Heap sigil already consumed. Now parse keywords, literals, and structures.
        // Check for keywords first.
        match self.peek_word() {
            Some("true") => {
                self.eat_word("true");
                return ast::Expr::True;
            }
            Some("false") => {
                self.eat_word("false");
                return ast::Expr::False;
            }
            Some("nil") => {
                self.eat_word("nil");
                return ast::Expr::Nil;
            }
            Some("none") => {
                self.eat_word("none");
                return ast::Expr::None;
            }
            Some("error") => {
                self.eat_word("error");
                let value = self.parse_expr_full();
                return ast::Expr::Err(ast::ExprErr::new(self.db, value));
            }
            Some("tuple") => {
                self.eat_word("tuple");
                let name = self.need_name();
                let elements = self.parse_branch(Sigil::ParenOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_full())
                });
                return ast::Expr::NamedTuple(ast::ExprNamedTuple::new(
                    self.db,
                    name,
                    elements,
                ));
            }
            Some("struct") => {
                self.eat_word("struct");
                let name = self.need_name();
                let fields = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_struct_field())
                });
                return ast::Expr::NamedStruct(ast::ExprNamedStruct::new(
                    self.db,
                    name,
                    fields,
                ));
            }
            Some("enum") => {
                self.eat_word("enum");
                // Enum expression syntax:
                // - enum Variant (anonymous, no payload)
                // - enum Variant payload (anonymous, with payload)
                // - enum EnumName.Variant (named, with dot separator)
                // - enum EnumName.Variant payload (named, with payload)
                let first_name = self.need_name();
                if self.peek_sigil(Sigil::Dot) {
                    // Named enum: enum EnumName.Variant [payload].
                    self.eat_sigil(Sigil::Dot);
                    let variant_name = self.need_name();
                    let payload = if self.is_at_expr_start() {
                        Some(self.parse_expr_full())
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
                    // Anonymous enum: enum Variant [payload].
                    let variant_name = first_name;
                    let payload = if self.is_at_expr_start() {
                        Some(self.parse_expr_full())
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
                let entries = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| {
                        let key = p.parse_expr_full();
                        p.need_sigil(Sigil::Equals);
                        let value = p.parse_expr_full();
                        ast::ExprMapEntry::new(p.db, key, value)
                    })
                });
                return ast::Expr::Map(ast::ExprMap::new(self.db, entries));
            }
            Some("set") => {
                self.eat_word("set");
                let elements = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_full())
                });
                return ast::Expr::Set(ast::ExprSet::new(self.db, elements));
            }
            _ => {}
        }

        // Not a keyword, check for literals and structures.
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).X();
                        // Try parsing as number, check for float by looking ahead for dot.
                        if word.chars().all(|c| c.is_ascii_digit()) {
                            // Check if next tokens form a float pattern (dot then digits).
                            let is_float = {
                                // Consume the number first.
                                self.next();
                                if !self.peek_sigil(Sigil::Dot) {
                                    false
                                } else {
                                    // Consume the dot and check.
                                    self.eat_sigil(Sigil::Dot);
                                    // Now check if we have digits.
                                    if let Some(TreeToken::Token(next_token)) = self.peek() {
                                        if let Some(decimal_part) = next_token.word_str(self.db) {
                                            decimal_part.chars().all(|c| c.is_ascii_digit())
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                }
                            };

                            if is_float {
                                let decimal_word = self.need_name();
                                let float_str = format!("{}.{}", word, decimal_word.as_str(self.db));
                                let value = InternedText::new(self.db, float_str.S());
                                ast::Expr::F32(ast::ExprF32::new(self.db, value))
                            } else {
                                // Not a float, just a U32.
                                if let Ok(val) = word.parse::<u32>() {
                                    ast::Expr::U32(ast::ExprU32::new(self.db, val))
                                } else {
                                    let value = InternedText::new(self.db, word.S());
                                    ast::Expr::Int(ast::ExprInt::new(self.db, value))
                                }
                            }
                        } else {
                            // Not a number, treat as a token.
                            self.next();
                            let name = InternedText::new(self.db, word.S());
                            ast::Expr::Token(ast::ExprToken::new(self.db, name))
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
                    _ => todo!("unexpected token in expr"),
                }
            }
            Some(TreeToken::Branch(Sigil::ParenOpen, _)) => {
                // Tuple or single element in parens.
                let elements = self.parse_branch(Sigil::ParenOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_full())
                });
                ast::Expr::AnonTuple(ast::ExprAnonTuple::new(self.db, elements))
            }
            Some(TreeToken::Branch(Sigil::BraceOpen, _)) => {
                // Struct.
                let fields = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_struct_field())
                });
                ast::Expr::AnonStruct(ast::ExprAnonStruct::new(self.db, fields))
            }
            Some(TreeToken::Branch(Sigil::BracketOpen, _)) => {
                // List.
                let elements = self.parse_branch(Sigil::BracketOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_full())
                });
                ast::Expr::List(ast::ExprList::new(self.db, elements))
            }
            _ => todo!("unexpected token in expr"),
        }
    }

    fn parse_expr_struct_field(&mut self) -> ast::ExprStructField<'db> {
        let name = self.need_name();
        self.need_sigil(Sigil::Equals);
        let value = self.parse_expr_full();
        ast::ExprStructField::new(self.db, name, value)
    }

    fn is_at_expr_start(&mut self) -> bool {
        // With heap sigils required, expressions start with @.
        // (# for global heap not yet supported by bct lexer.)
        self.peek_sigil(Sigil::At)
    }

    fn parse_comma_separated<T>(
        &mut self,
        mut parse_fn: impl FnMut(&mut Self) -> T,
    ) -> Vec<T> {
        let mut items = vec![];
        if !self.is_at_expr_start() {
            return items;
        }
        loop {
            items.push(parse_fn(self));
            if self.peek_sigil(Sigil::Comma) {
                self.eat_sigil(Sigil::Comma);
            } else {
                break;
            }
        }
        items
    }

    fn parse_branch<T>(&mut self, open_sigil: Sigil, parse_fn: &dyn Fn(&mut DynParser<'db>) -> T) -> T {
        match self.next() {
            Some(TreeToken::Branch(sigil, iter)) if sigil == open_sigil => {
                let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                let mut dyn_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                };
                parse_fn(&mut dyn_parser)
            }
            _ => panic!("expected branch with sigil {}", open_sigil.as_str()),
        }
    }

    fn peek(&mut self) -> Option<TreeToken<'db>> {
        self.tokens.peek().cloned()
    }

    fn peek_sigil(&mut self, sigil: Sigil) -> bool {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db), TokenKind::Sigil(s) if s == sigil)
            }
            Some(TreeToken::Branch(s, _)) => s == sigil,
            None => false,
        }
    }

    fn peek_word(&mut self) -> Option<&'db str> {
        match self.peek() {
            Some(TreeToken::Token(token)) => token.word_str(self.db),
            _ => None,
        }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        self.tokens.next()
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

impl<'db> DynParser<'db> {
    fn parse_type_hint_and_heap(&mut self) -> ast::TypeHintAndHeap<'db> {
        // Heap sigils: @ for local. (# for global not yet supported by bct lexer.)
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else {
            panic!("expected heap sigil @ before type");
        };
        let type_hint = self.parse_type_hint();
        ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        // Heap sigil already consumed, now parse the type name.
        match self.peek_word() {
            Some("bool") => { self.eat_word("bool"); ast::TypeHint::Bool }
            Some("u32") => { self.eat_word("u32"); ast::TypeHint::U32 }
            Some("f32") => { self.eat_word("f32"); ast::TypeHint::F32 }
            Some("int") => { self.eat_word("int"); ast::TypeHint::Int }
            Some("nil") => { self.eat_word("nil"); ast::TypeHint::Nil }
            Some("string") => { self.eat_word("string"); ast::TypeHint::String }
            _ => todo!("parse type hint in dyn parser"),
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
        let payload = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            Some(self.parse_type_hint_and_heap())
        } else {
            None
        };
        ast::TypeHintEnumVariant::new(self.db, name, payload)
    }

    fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        let expr = self.parse_expr_and_heap();
        let type_hint = ast::TypeHintAndHeap::new(
            self.db,
            ast::Heap::Omitted,
            ast::TypeHint::Data,
        );
        ast::ExprFull::new(self.db, type_hint, expr)
    }

    fn parse_expr_and_heap(&mut self) -> ast::ExprAndHeap<'db> {
        // Heap sigils: @ for local. (# for global not yet supported by bct lexer.)
        let heap = if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            ast::Heap::Local
        } else {
            panic!("expected heap sigil @ before expression");
        };
        let expr = self.parse_expr();
        ast::ExprAndHeap::new(self.db, heap, expr)
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        // Heap sigil already consumed. Now parse keywords, literals, and structures.
        // Check for keywords first.
        match self.peek_word() {
            Some("true") => { self.eat_word("true"); return ast::Expr::True; }
            Some("false") => { self.eat_word("false"); return ast::Expr::False; }
            Some("nil") => { self.eat_word("nil"); return ast::Expr::Nil; }
            Some("none") => { self.eat_word("none"); return ast::Expr::None; }
            _ => {}
        }

        // Not a keyword, check for numbers or tokens.
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
                                            return ast::Expr::F32(ast::ExprF32::new(self.db, value));
                                        }
                                    }
                                }
                            }
                            // Not a float.
                            if let Ok(val) = word.parse::<u32>() {
                                ast::Expr::U32(ast::ExprU32::new(self.db, val))
                            } else {
                                let value = InternedText::new(self.db, word.S());
                                ast::Expr::Int(ast::ExprInt::new(self.db, value))
                            }
                        } else {
                            // Not a number, treat as a token.
                            self.next();
                            let name = InternedText::new(self.db, word.S());
                            ast::Expr::Token(ast::ExprToken::new(self.db, name))
                        }
                    }
                    _ => todo!("dyn parser token"),
                }
            }
            _ => todo!("dyn parser expr"),
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
    let type_hint = ast.type_hint(db).type_hint(db);
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
        ast::Expr::U32(e) => assert_eq!(e.value(db), 42),
        _ => panic!("expected u32"),
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
        ast::Expr::F32(e) => assert_eq!(e.value(db).as_str(db), "1.0"),
        ast::Expr::U32(_) => panic!("expected f32, got U32"),
        ast::Expr::Int(_) => panic!("expected f32, got Int"),
        _ => panic!("expected f32, got something else"),
    }
}

#[test]
fn test_parse_float_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @f32 / @1.0"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).type_hint(db);
    assert!(matches!(type_hint, ast::TypeHint::F32));
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::F32(e) => assert_eq!(e.value(db).as_str(db), "1.0"),
        _ => panic!("expected f32"),
    }
}

#[test]
fn test_parse_anon_enum_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @enum { Foo, Bar: @u32 } / @enum Foo"));
    let ast = parse(db, source);
    let type_hint = ast.type_hint(db).type_hint(db);
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
    let type_hint = ast.type_hint(db).type_hint(db);
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
    let type_hint = ast.type_hint(db).type_hint(db);
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
    let type_hint = ast.type_hint(db).type_hint(db);
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
    let type_hint = ast.type_hint(db).type_hint(db);
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
    let source = Source::new(db, S("@enum Bar @2"));
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
    let source = Source::new(db, S("@enum Baz @(@true, @1)"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonEnum(e) => {
            assert_eq!(e.variant_name(db).as_str(db), "Baz");
            assert!(e.payload(db).is_some());
        }
        _ => panic!("expected anonymous enum"),
    }
}

#[test]
fn test_parse_named_enum_with_dot() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@enum Quux.Bar @(@true, @1)"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::NamedEnum(e) => {
            assert_eq!(e.enum_name(db).as_str(db), "Quux");
            assert_eq!(e.variant_name(db).as_str(db), "Bar");
            assert!(e.payload(db).is_some());
        }
        _ => panic!("expected named enum"),
    }
}
