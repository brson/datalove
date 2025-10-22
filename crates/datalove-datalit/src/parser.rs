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
use datalove_diagnostic::DiagnosticBuilder;

/// Parse a Source into a datalit expression with span information.
///
/// This function must be called from within a Salsa tracked function context.
/// For tests, use the test helper modules which provide tracked wrappers.
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ParseResult<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer(db, bracer)
}

fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
) -> ast::ParseResult<'db> {
    let chunk_lex = bracer.chunk(db);
    // Get source text from the first token.
    let source_text = chunk_lex.tokens(db).first().map(|token| {
        let subtext = token.text(db);
        subtext.text(db)
    });
    let tokens = bracer.iter(db).filter_map(|t| t.without_space(db)).collect::<Vec<_>>();
    parse_from_tokens_with_source(db, tokens, source_text)
}

/// Parse a datalit expression directly from a vector of tokens.
/// This allows other parsers to delegate to the datalit parser without
/// reconstructing source text from tokens.
pub fn parse_from_tokens<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
) -> ast::ParseResult<'db> {
    parse_from_tokens_with_source(db, tokens, None)
}

/// Parse a datalit expression from tokens with an optional source Text for error reporting.
fn parse_from_tokens_with_source<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    source_text: Option<bct::text::Text<'db>>,
) -> ast::ParseResult<'db> {
    let mut dyn_parser = DynParser {
        db,
        tokens,
        pos: 0,
        source_text,
        expr_spans: Vec::new(),
    };
    let expr = dyn_parser.parse_expr_full();
    ast::ParseResult::new(expr, dyn_parser.expr_spans)
}

/// Parse a type hint and heap from a vector of tokens.
///
/// Returns the parsed type hint and the number of tokens consumed.
pub fn parse_type_hint_and_heap_from_tokens<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
) -> (ast::TypeHintAndHeap<'db>, usize) {
    let mut dyn_parser = DynParser {
        db,
        tokens,
        pos: 0,
        source_text: None,
        expr_spans: Vec::new(),
    };
    let type_hint = dyn_parser.parse_type_hint_and_heap();
    let consumed = dyn_parser.pos;
    (type_hint, consumed)
}

struct DynParser<'db> {
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    pos: usize,
    source_text: Option<bct::text::Text<'db>>,
    expr_spans: Vec<(ast::ExprFull<'db>, bct::text::Text<'db>, datalove_diagnostic::ByteSpan)>,
}

impl<'db> DynParser<'db> {
    fn parse_type_hint_and_heap(&mut self) -> ast::TypeHintAndHeap<'db> {
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_and_heap());
                        ast::TypeHint::NamedTuple(ast::TypeHintNamedTuple::new(
                            self.db,
                            name,
                            fields,
                        ))
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected () after tuple keyword".S());

                        DiagnosticBuilder::error(self.db, "expected () after tuple keyword")
                            .code("D001")
                            .primary_label(text, span.clone(), "expected '(' after 'tuple'")
                            .emit_parse();

                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                        ast::TypeHint::NamedStruct(ast::TypeHintNamedStruct::new(
                            self.db,
                            name,
                            fields,
                        ))
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected {} after struct keyword".S());

                        DiagnosticBuilder::error(self.db, "expected {} after struct keyword")
                            .code("D002")
                            .primary_label(text, span.clone(), "expected '{' after 'struct'")
                            .emit_parse();

                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                                source_text: self.source_text,
                                expr_spans: Vec::new(),
                            };
                            let variants = sub_parser.parse_comma_separated(|p| p.parse_type_hint_enum_variant());
                            ast::TypeHint::AnonEnum(ast::TypeHintAnonEnum::new(
                                self.db,
                                variants,
                            ))
                        }
                        _ => {
                            let (text, span) = self.current_text_span();
                            let message = InternedText::new(self.db, "expected {} after enum keyword".S());

                            DiagnosticBuilder::error(self.db, "expected {} after enum keyword")
                                .code("D003")
                                .primary_label(text, span.clone(), "expected '{' after 'enum'")
                                .emit_parse();

                            ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                                source_text: self.source_text,
                                expr_spans: Vec::new(),
                            };
                            let variants = sub_parser.parse_comma_separated(|p| p.parse_type_hint_enum_variant());
                            ast::TypeHint::NamedEnum(ast::TypeHintNamedEnum::new(
                                self.db,
                                name,
                                variants,
                            ))
                        }
                        _ => {
                            let (text, span) = self.current_text_span();
                            let message = InternedText::new(self.db, "expected {} after enum name".S());

                            DiagnosticBuilder::error(self.db, "expected {} after enum name")
                                .code("D004")
                                .primary_label(text, span.clone(), "expected '{' after enum name")
                                .emit_parse();

                            ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let key_type = sub_parser.parse_type_hint_and_heap();
                        sub_parser.need_sigil(Sigil::Comma);
                        let value_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::Map(ast::TypeHintMap::new(self.db, key_type, value_type))
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected <> after map keyword".S());

                        DiagnosticBuilder::error(self.db, "expected <> after map keyword")
                            .code("D005")
                            .primary_label(text, span.clone(), "expected '<' after 'map'")
                            .emit_parse();

                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let element_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::Set(ast::TypeHintSet::new(self.db, element_type))
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected <> after set keyword".S());

                        DiagnosticBuilder::error(self.db, "expected <> after set keyword")
                            .code("D006")
                            .primary_label(text, span.clone(), "expected '<' after 'set'")
                            .emit_parse();

                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let element_type = sub_parser.parse_type_hint_and_heap();
                        ast::TypeHint::List(ast::TypeHintList::new(self.db, element_type))
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected <> after list keyword".S());

                        DiagnosticBuilder::error(self.db, "expected <> after list keyword")
                            .code("D007")
                            .primary_label(text, span.clone(), "expected '<' after 'list'")
                            .emit_parse();

                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                        ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct::new(self.db, fields))
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "unknown type hint in DynParser".S());

                        DiagnosticBuilder::error(self.db, "unknown type hint")
                            .code("D008")
                            .primary_label(text, span.clone(), "unexpected token in type hint")
                            .emit_parse();

                        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
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
                source_text: self.source_text,
                expr_spans: Vec::new(),
            };
            let payload_type = sub_parser.parse_type_hint_and_heap();

            // Check for unparsed tokens - this indicates a syntax error.
            if sub_parser.pos < tokens.len() {
                // There are extra tokens after the payload type.
                let (text, span) = sub_parser.current_text_span();
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

    fn parse_expr_full(&mut self) -> ast::ExprFull<'db> {
        // Capture span before parsing.
        let (text, span) = self.current_text_span();

        // Check for `: type / expr` pattern.
        let expr_full = if self.peek_sigil(Sigil::Colon) {
            self.eat_sigil(Sigil::Colon);
            let type_hint = self.parse_type_hint_and_heap();
            self.need_sigil(Sigil::SlashForward);
            let expr = self.parse_expr_and_heap();
            ast::ExprFull::new(self.db, Some(type_hint), expr)
        } else {
            // No type hint, just parse expression.
            let expr = self.parse_expr_and_heap();
            ast::ExprFull::new(self.db, None, expr)
        };

        // Record span for this expression.
        self.expr_spans.push((expr_full, text, span));

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
                                    let (text, span) = self.current_text_span();
                                    let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());

                                    DiagnosticBuilder::error(self.db, "expected heap sigil @ or # before expression")
                                        .code("D009")
                                        .primary_label(text, span.clone(), "expected '@' or '#' before expression")
                                        .emit_parse();

                                    let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
                                    return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                                }
                            } else {
                                // No word string - error.
                                let (text, span) = self.current_text_span();
                                let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());

                                DiagnosticBuilder::error(self.db, "expected heap sigil @ or # before expression")
                                    .code("D010")
                                    .primary_label(text, span.clone(), "expected '@' or '#' before expression")
                                    .emit_parse();

                                let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
                                return ast::ExprAndHeap::new(self.db, ast::Heap::Omitted, error_node);
                            }
                        }
                        _ => {
                            // Unknown token kind - error.
                            let (text, span) = self.current_text_span();
                            let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());

                            DiagnosticBuilder::error(self.db, "expected heap sigil @ or # before expression")
                                .code("D011")
                                .primary_label(text, span.clone(), "expected '@' or '#' before expression")
                                .emit_parse();

                            let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
                    let (text, span) = self.current_text_span();
                    let message = InternedText::new(self.db, "expected heap sigil @ or # before expression".S());

                    DiagnosticBuilder::error(self.db, "expected heap sigil @ or # before expression")
                        .code("D012")
                        .primary_label(text, span.clone(), "expected '@' or '#' before expression")
                        .emit_parse();

                    let error_node = ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
            let (text, span) = self.current_text_span();
            let message = InternedText::new(self.db, "unexpected minus sign".S());

            DiagnosticBuilder::error(self.db, "unexpected minus sign")
                .code("D013")
                .primary_label(text, span.clone(), "unexpected '-' not followed by number")
                .emit_parse();

            return ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                        return ast::Expr::NamedTuple(ast::ExprNamedTuple::new(
                            self.db,
                            name,
                            elements,
                        ));
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected () after tuple name".S());

                        DiagnosticBuilder::error(self.db, "expected () after tuple name")
                            .code("D014")
                            .primary_label(text, span.clone(), "expected '(' after tuple name")
                            .emit_parse();

                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let fields = sub_parser.parse_comma_separated(|p| p.parse_expr_struct_field());
                        return ast::Expr::NamedStruct(ast::ExprNamedStruct::new(
                            self.db,
                            name,
                            fields,
                        ));
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected {} after struct name".S());

                        DiagnosticBuilder::error(self.db, "expected {} after struct name")
                            .code("D015")
                            .primary_label(text, span.clone(), "expected '{' after struct name")
                            .emit_parse();

                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
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
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected {} after map keyword".S());

                        DiagnosticBuilder::error(self.db, "expected {} after map keyword")
                            .code("D016")
                            .primary_label(text, span.clone(), "expected '{' after 'map'")
                            .emit_parse();

                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                        };
                        let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                        return ast::Expr::Set(ast::ExprSet::new(self.db, elements));
                    }
                    _ => {
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "expected {} after set keyword".S());

                        DiagnosticBuilder::error(self.db, "expected {} after set keyword")
                            .code("D017")
                            .primary_label(text, span.clone(), "expected '{' after 'set'")
                            .emit_parse();

                        return ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message));
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
                            let (text, span) = self.current_text_span();
                            self.next();
                            let message = InternedText::new(
                                self.db,
                                format!("Unexpected identifier: {}", word).S()
                            );

                            DiagnosticBuilder::error(self.db, &format!("unexpected identifier '{}'", word))
                                .code("D019")
                                .primary_label(text, span.clone(), "unexpected identifier")
                                .emit_parse();

                            ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message))
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
                        let (text, span) = self.current_text_span();
                        let message = InternedText::new(self.db, "unexpected token in DynParser expression".S());

                        DiagnosticBuilder::error(self.db, "unexpected token in expression")
                            .code("D020")
                            .primary_label(text, span.clone(), "unexpected token")
                            .emit_parse();

                        ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message))
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
                    source_text: self.source_text,
                    expr_spans: Vec::new(),
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
                    source_text: self.source_text,
                    expr_spans: Vec::new(),
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
                    source_text: self.source_text,
                    expr_spans: Vec::new(),
                };
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                ast::Expr::List(ast::ExprList::new(self.db, elements))
            }
            _ => {
                let (text, span) = self.current_text_span();
                let message = InternedText::new(self.db, "unexpected tree node in DynParser expression".S());

                DiagnosticBuilder::error(self.db, "unexpected token in expression")
                    .code("D018")
                    .primary_label(text, span.clone(), "unexpected token")
                    .emit_parse();

                ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message))
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

    /// Get Text for error reporting.
    /// Try source_text first, otherwise extract from current token.
    fn get_error_text(&self) -> bct::text::Text<'db> {
        if let Some(text) = self.source_text {
            return text;
        }
        // Try to get from the first token.
        if let Some(token) = self.tokens.first() {
            match token {
                TreeToken::Token(tok) => {
                    let subtext = tok.text(self.db);
                    return subtext.text(self.db);
                }
                TreeToken::Branch(_, iter) => {
                    // Try to find a Token inside the branch.
                    for inner_token in iter.clone() {
                        if let Some(TreeToken::Token(tok)) = inner_token.without_space(self.db) {
                            let subtext = tok.text(self.db);
                            return subtext.text(self.db);
                        }
                    }
                }
                _ => {}
            }
        }
        // Last resort: create an empty text as a fallback.
        // This can happen when parsing tokens without source_text (e.g., from datafun parser).
        bct::text::Text::new(self.db, String::new())
    }

    /// Extract Text and ByteSpan from current position for error reporting.
    fn current_text_span(&self) -> (bct::text::Text<'db>, datalove_diagnostic::ByteSpan) {
        if self.pos < self.tokens.len() {
            match &self.tokens[self.pos] {
                TreeToken::Token(tok) => {
                    let subtext = tok.text(self.db);
                    (subtext.text(self.db), subtext.range(self.db))
                }
                TreeToken::Branch(_, _) => {
                    // For branches, use 0..0 span.
                    (self.get_error_text(), 0..0)
                }
            }
        } else {
            // End of input.
            (self.get_error_text(), 0..0)
        }
    }
}

/// Test-only tracked wrapper around parse() to provide Salsa context.
///
/// This allows tests to call parse() which creates tracked AST nodes.
/// Regular code should call parse() from within a tracked function context.
#[salsa::tracked]
#[cfg(test)]
pub(crate) fn parse_for_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    parse(db, source).expr
}

#[test]
fn test_parse_bool() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@true"));
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_bool_with_type() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S(": @bool / @true"));
    let ast = parse_for_test(db, source);
    let type_hint = ast.type_hint(db).unwrap().type_hint(db);
    assert!(matches!(type_hint, ast::TypeHint::Bool));
    let expr = ast.expr(db).expr(db);
    assert!(matches!(expr, ast::Expr::True));
}

#[test]
fn test_parse_int() {
    let ref db = crate::Database::default();
    let source = Source::new(db, S("@42"));
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
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
    let ast = parse_for_test(db, source);
    let expr = ast.expr(db).expr(db);
    match expr {
        ast::Expr::AnonTuple(e) => assert_eq!(e.elements(db).len(), 2),
        _ => panic!("expected tuple"),
    }
}
