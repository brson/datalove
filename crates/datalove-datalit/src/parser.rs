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
        // todo: parse heap prefix.
        let heap = ast::Heap::Omitted;
        let type_hint = self.parse_type_hint();
        ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        // Type hints start with @.
        self.need_sigil(Sigil::At);

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
        // todo: parse heap prefix.
        let heap = ast::Heap::Omitted;
        let expr = self.parse_expr();
        ast::ExprAndHeap::new(self.db, heap, expr)
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        // Check for @ prefix (token, named types, etc).
        if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            return self.parse_at_prefixed_expr();
        }

        // Check for literals and structures.
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).X();
                        match word {
                            "true" => {
                                self.next();
                                ast::Expr::True
                            }
                            "false" => {
                                self.next();
                                ast::Expr::False
                            }
                            _ => {
                                // Try parsing as integer.
                                if let Ok(val) = word.parse::<u32>() {
                                    self.next();
                                    ast::Expr::U32(ast::ExprU32::new(self.db, val))
                                } else {
                                    // Assume it's an int literal.
                                    self.next();
                                    let value = InternedText::new(self.db, word.S());
                                    ast::Expr::Int(ast::ExprInt::new(self.db, value))
                                }
                            }
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

    fn parse_at_prefixed_expr(&mut self) -> ast::Expr<'db> {
        // After @, we can have various constructs.
        match self.peek_word() {
            Some("true") => {
                self.eat_word("true");
                ast::Expr::True
            }
            Some("false") => {
                self.eat_word("false");
                ast::Expr::False
            }
            Some("nil") => {
                self.eat_word("nil");
                ast::Expr::Nil
            }
            Some("none") => {
                self.eat_word("none");
                ast::Expr::None
            }
            Some("error") => {
                self.eat_word("error");
                let value = self.parse_expr_full();
                ast::Expr::Err(ast::ExprErr::new(self.db, value))
            }
            Some("tuple") => {
                self.eat_word("tuple");
                let name = self.need_name();
                let elements = self.parse_branch(Sigil::ParenOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_full())
                });
                ast::Expr::NamedTuple(ast::ExprNamedTuple::new(
                    self.db,
                    name,
                    elements,
                ))
            }
            Some("struct") => {
                self.eat_word("struct");
                let name = self.need_name();
                let fields = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_struct_field())
                });
                ast::Expr::NamedStruct(ast::ExprNamedStruct::new(
                    self.db,
                    name,
                    fields,
                ))
            }
            Some("enum") => {
                self.eat_word("enum");
                // Can be: @enum . Variant or @enum | Variant payload.
                if self.peek_sigil(Sigil::Dot) {
                    self.eat_sigil(Sigil::Dot);
                    let variant_name = self.need_name();
                    ast::Expr::AnonEnum(ast::ExprAnonEnum::new(
                        self.db,
                        variant_name,
                        None,
                    ))
                } else if self.peek_sigil(Sigil::Pipe) {
                    self.eat_sigil(Sigil::Pipe);
                    let variant_name = self.need_name();
                    let payload = if self.is_at_expr_start() {
                        Some(self.parse_expr_full())
                    } else {
                        None
                    };
                    ast::Expr::AnonEnum(ast::ExprAnonEnum::new(
                        self.db,
                        variant_name,
                        payload,
                    ))
                } else if let Some(word) = self.peek_word() {
                    // @enum Name | Variant payload.
                    let enum_name = self.need_name();
                    self.need_sigil(Sigil::Pipe);
                    let variant_name = self.need_name();
                    let payload = if self.is_at_expr_start() {
                        Some(self.parse_expr_full())
                    } else {
                        None
                    };
                    ast::Expr::NamedEnum(ast::ExprNamedEnum::new(
                        self.db,
                        enum_name,
                        variant_name,
                        payload,
                    ))
                } else {
                    todo!("unexpected enum syntax")
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
                ast::Expr::Map(ast::ExprMap::new(self.db, entries))
            }
            Some("set") => {
                self.eat_word("set");
                let elements = self.parse_branch(Sigil::BraceOpen, &|p: &mut DynParser<'db>| {
                    p.parse_comma_separated(|p| p.parse_expr_full())
                });
                ast::Expr::Set(ast::ExprSet::new(self.db, elements))
            }
            Some(_) => {
                // Could be @token Name.
                let name = self.need_name();
                ast::Expr::Token(ast::ExprToken::new(self.db, name))
            }
            None => todo!("unexpected end after @"),
        }
    }

    fn parse_expr_struct_field(&mut self) -> ast::ExprStructField<'db> {
        let name = self.need_name();
        self.need_sigil(Sigil::Equals);
        let value = self.parse_expr_full();
        ast::ExprStructField::new(self.db, name, value)
    }

    fn is_at_expr_start(&mut self) -> bool {
        matches!(
            self.peek(),
            Some(TreeToken::Token(_)) | Some(TreeToken::Branch(..))
        )
    }

    fn parse_comma_separated<T>(
        &mut self,
        mut parse_fn: impl FnMut(&mut Self) -> T,
    ) -> Vec<T> {
        let mut items = vec![];
        if !self.is_at_expr_start() && !self.peek_sigil(Sigil::At) {
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
        let heap = ast::Heap::Omitted;
        let type_hint = self.parse_type_hint();
        ast::TypeHintAndHeap::new(self.db, heap, type_hint)
    }

    fn parse_type_hint(&mut self) -> ast::TypeHint<'db> {
        self.need_sigil(Sigil::At);
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
        let heap = ast::Heap::Omitted;
        let expr = self.parse_expr();
        ast::ExprAndHeap::new(self.db, heap, expr)
    }

    fn parse_expr(&mut self) -> ast::Expr<'db> {
        if self.peek_sigil(Sigil::At) {
            self.eat_sigil(Sigil::At);
            return self.parse_at_prefixed_expr();
        }
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                match token.kind(self.db) {
                    TokenKind::Word => {
                        let word = token.word_str(self.db).X();
                        if word == "true" { self.next(); ast::Expr::True }
                        else if word == "false" { self.next(); ast::Expr::False }
                        else if let Ok(val) = word.parse::<u32>() {
                            self.next();
                            ast::Expr::U32(ast::ExprU32::new(self.db, val))
                        } else {
                            self.next();
                            let value = InternedText::new(self.db, word.S());
                            ast::Expr::Int(ast::ExprInt::new(self.db, value))
                        }
                    }
                    _ => todo!("dyn parser token"),
                }
            }
            _ => todo!("dyn parser expr"),
        }
    }

    fn parse_at_prefixed_expr(&mut self) -> ast::Expr<'db> {
        match self.peek_word() {
            Some("true") => { self.eat_word("true"); ast::Expr::True }
            Some("false") => { self.eat_word("false"); ast::Expr::False }
            Some("nil") => { self.eat_word("nil"); ast::Expr::Nil }
            _ => todo!("dyn parser @ expr"),
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
    let source = Source::new(db, S("42"));
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
    let source = Source::new(db, S("(@true, 1)"));
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
    let source = Source::new(db, S("[1, 2, 3]"));
    let ast = parse(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::List(e) => assert_eq!(e.elements(db).len(), 3),
        _ => panic!("expected list"),
    }
}
