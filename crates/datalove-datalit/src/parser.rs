use rmx::prelude::*;

use bct::{
    input::Source,
    lexer::{
        TokenKind,
        Sigil
    },
    bracer::{
        Bracer,
        TreeToken,
    },
    text::InternedText,
    source_map,
    lexer,
    bracer,
};

use crate::ast;
use crate::parser_util::{self, TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;

/// Parse a Source into a datalit expression with span information.
#[salsa::tracked]
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
    // Try to extract source text from the first token for better error reporting.
    let source_text = tokens.first().and_then(|token| {
        match token {
            TreeToken::Token(tok) => {
                let subtext = tok.text(db);
                Some(subtext.text(db))
            }
            TreeToken::Branch(_, iter) => {
                // Look inside the branch for a token.
                iter.clone().find_map(|inner| {
                    inner.without_space(db).and_then(|t| {
                        match t {
                            TreeToken::Token(tok) => {
                                let subtext = tok.text(db);
                                Some(subtext.text(db))
                            }
                            TreeToken::Branch(_, _) => None
                        }
                    })
                })
            }
        }
    });
    parse_from_tokens_with_source(db, tokens, source_text)
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
        had_error: false,
    };
    let expr = dyn_parser.parse_expr_full();
    ast::ParseResult::new(db, expr, dyn_parser.expr_spans)
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
        had_error: false,
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
    expr_spans: Vec<ast::ParseSpanEntry>,
    had_error: bool,
}

impl<'db> TokenStream<'db> for DynParser<'db> {
    fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    fn peek(&self) -> Option<&TreeToken<'db>> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }
}

impl<'db> DynParser<'db> {
    /// Emit both a diagnostic and create an ExprParseError node in one call.
    fn emit_expr_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Expr<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message_text))
    }

    /// Emit both a diagnostic and create a TypeHintParseError node in one call.
    fn emit_type_hint_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::TypeHint<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message_text))
    }

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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("tuple");
                // Check if it's anonymous (starts with () or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::ParenOpen) {
                    // Anonymous tuple with explicit keyword.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("struct");
                // Check if it's anonymous (starts with {) or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous struct with explicit keyword.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("enum");
                // Check if it's anonymous (starts with {) or named (starts with name).
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous enum.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("map");
                // Expect angle bracket with key and value types.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let key_type = sub_parser.parse_type_hint_and_heap();
                    if !sub_parser.eat_sigil(Sigil::Comma) {
                        let (text, span) = sub_parser.current_text_span();
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("set");
                // Expect angle bracket with element type.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("tensor");
                // Expect angle bracket with <element_type, rank, optional_layout>.
                if let Some(iter) = self.eat_branch(Sigil::AngleOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let element_type = sub_parser.parse_type_hint_and_heap();
                    if !sub_parser.eat_sigil(Sigil::Comma) {
                        let (text, span) = sub_parser.current_text_span();
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
                            let (text, span) = sub_parser.current_text_span();
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
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_and_heap());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonTuple(ast::TypeHintAnonTuple::new(self.db, fields))
                } else if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    // List type.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let element_type = sub_parser.parse_type_hint_and_heap();
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::List(ast::TypeHintList::new(self.db, element_type))
                } else if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    // Anonymous struct.
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let fields = sub_parser.parse_comma_separated(|p| p.parse_type_hint_named_field());
                    sub_parser.error_if_not_exhausted_type_hint();
                    ast::TypeHint::AnonStruct(ast::TypeHintAnonStruct::new(self.db, fields))
                } else {
                    let (text, span) = self.current_text_span();
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
                let (text, span) = self.current_text_span();
                let error_hint = self.emit_type_hint_error(
                    text.clone(),
                    span.clone(),
                    "expected field name in struct definition",
                    "D010",
                    "expected field name"
                );
                // Create a placeholder name for the error field.
                let placeholder_name = InternedText::new(self.db, "<error>".S());
                let type_hint = ast::TypeHintAndHeap::new(self.db, ast::Heap::Omitted, error_hint);
                return ast::TypeHintNamedField::new(self.db, placeholder_name, type_hint);
            }
        };
        if !self.eat_sigil(Sigil::Colon) {
            let (text, span) = self.current_text_span();
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
        let name = match self.eat_name() {
            Some(n) => n,
            None => {
                // No name found - emit error and create placeholder.
                self.had_error = true;
                let (text, span) = self.current_text_span();
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
            let mut sub_parser = DynParser {
                db: self.db,
                tokens: tokens.clone(),
                pos: 0,
                source_text: self.source_text,
                expr_spans: Vec::new(),
                had_error: false,
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
            if !self.eat_sigil(Sigil::SlashForward) {
                let (err_text, err_span) = self.current_text_span();
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
                                    let (text, span) = self.current_text_span();
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
                                let (text, span) = self.current_text_span();
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
                            let (text, span) = self.current_text_span();
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
                    let (text, span) = self.current_text_span();
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
            let (text, span) = self.current_text_span();
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
                return ast::Expr::Err(ast::ExprErr::new(self.db, value));
            }
            Some("tensor") => {
                self.eat_word("tensor");

                // Parse shape: [dim1, dim2, ...]
                let shape = if let Some(iter) = self.eat_branch(Sigil::BracketOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let shape = sub_parser.parse_comma_separated(|p| {
                        match p.parse_u32_literal() {
                            Some(dim) => dim,
                            None => {
                                p.had_error = true;
                                let (text, span) = p.current_text_span();
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
                    let (text, span) = self.current_text_span();
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
                        let (text, span) = self.current_text_span();
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
                        let mut sub_parser = DynParser {
                            db: self.db,
                            tokens,
                            pos: 0,
                            source_text: self.source_text,
                            expr_spans: Vec::new(),
                            had_error: false,
                        };
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
                            let mut row_parser = DynParser {
                                db: self.db,
                                tokens: elem_tokens.clone(),
                                pos: 0,
                                source_text: self.source_text,
                                expr_spans: Vec::new(),
                                had_error: false,
                            };

                            let mut row_elements = Vec::new();
                            while row_parser.pos < row_parser.tokens.len() {
                                row_elements.push(row_parser.parse_expr_full());
                            }

                            // Merge spans from row sub-parser.
                            self.expr_spans.extend(row_parser.expr_spans);

                            // Validate row size matches the last dimension.
                            if row_elements.len() != row_size {
                                let (text, span) = self.current_text_span();
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
                    let (text, span) = self.current_text_span();
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
                let (keyword_text, keyword_span) = self.current_text_span();
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
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("map");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
                    let entries = sub_parser.parse_comma_separated(|p| {
                        let key = p.parse_expr_full();
                        if !p.eat_sigil(Sigil::Equals) {
                            let (text, span) = p.current_text_span();
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
                let (keyword_text, keyword_span) = self.current_text_span();
                self.eat_word("set");
                if let Some(iter) = self.eat_branch(Sigil::BraceOpen) {
                    let tokens = iter.filter_map(|t| t.without_space(self.db)).collect::<Vec<_>>();
                    let mut sub_parser = DynParser {
                        db: self.db,
                        tokens,
                        pos: 0,
                        source_text: self.source_text,
                        expr_spans: Vec::new(),
                        had_error: false,
                    };
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
                            let (text, span) = self.current_text_span();
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
                        let (text, span) = self.current_text_span();
                        self.emit_expr_error(
                            text,
                            span,
                            "unexpected token in DynParser expression",
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
                let mut sub_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                    source_text: self.source_text,
                    expr_spans: Vec::new(),
                    had_error: false,
                };
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
                let mut sub_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                    source_text: self.source_text,
                    expr_spans: Vec::new(),
                    had_error: false,
                };
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
                let mut sub_parser = DynParser {
                    db: self.db,
                    tokens,
                    pos: 0,
                    source_text: self.source_text,
                    expr_spans: Vec::new(),
                    had_error: false,
                };
                let elements = sub_parser.parse_comma_separated(|p| p.parse_expr_full());
                sub_parser.error_if_not_exhausted();
                // Merge spans from sub-parser.
                self.expr_spans.extend(sub_parser.expr_spans);
                ast::Expr::List(ast::ExprList::new(self.db, elements))
            }
            _ => {
                let (text, span) = self.current_text_span();
                self.emit_expr_error(
                    text,
                    span,
                    "unexpected tree node in DynParser expression",
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
                let (text, span) = self.current_text_span();
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
            let (text, span) = self.current_text_span();
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

    // TokenStream trait provides: peek(), next()
    // TokenStreamExt trait provides: peek_sigil(), eat_sigil(), peek_word(), try_eat_word(), eat_name(), need_name()

    /// Alias for try_eat_word for backward compatibility.
    fn eat_word(&mut self, word: &str) -> bool {
        self.try_eat_word(word)
    }

    /// Consume a branch if it matches the given sigil, returning its iterator.
    fn eat_branch(&mut self, sigil: Sigil) -> Option<bct::bracer::BracerIter<'db>> {
        if self.peek_sigil(sigil) {
            match self.next() {
                Some(TreeToken::Branch(_, iter)) => Some(iter),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Peek returning an owned token (cloned) for patterns that need to capture branch content.
    fn peek_owned(&self) -> Option<TreeToken<'db>> {
        self.tokens.get(self.pos).cloned()
    }

    fn parse_u32_literal(&mut self) -> Option<u32> {
        match self.peek() {
            Some(TreeToken::Token(tok)) => {
                if let Some(word) = tok.word_str(self.db) {
                    if let Ok(value) = word.parse::<u32>() {
                        self.next();
                        return Some(value);
                    }
                }
                None
            }
            _ => None,
        }
    }


    /// Emit error if tokens remain unconsumed after a successful parse.
    ///
    /// Only emits if the parser succeeded (no prior errors). This catches
    /// both parser bugs and user syntax errors.
    fn error_if_not_exhausted(&mut self) {
        if self.pos < self.tokens.len() && !self.had_error {
            self.had_error = true;
            let (text, span) = self.current_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("D021")
                .primary_label(text, span, "unexpected token")
                .emit_parse();
        }
    }

    /// Emit error if tokens remain unconsumed after a successful type hint parse.
    ///
    /// Only emits if the parser succeeded (no prior errors). This catches
    /// both parser bugs and user syntax errors.
    fn error_if_not_exhausted_type_hint(&mut self) {
        if self.pos < self.tokens.len() && !self.had_error {
            self.had_error = true;
            let (text, span) = self.current_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after type")
                .code("D022")
                .primary_label(text, span, "unexpected token")
                .emit_parse();
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
    parse(db, source).expr(db)
}

/// Public wrapper for integration tests.
/// Integration tests are compiled as separate binaries and need pub access.
#[salsa::tracked]
pub fn parse_integration_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    parse(db, source).expr(db)
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
    let source = Source::new(db, S(": @enum { Foo, Bar(@u32) } / @enum Foo"));
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
