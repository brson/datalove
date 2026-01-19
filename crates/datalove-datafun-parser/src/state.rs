//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    module_graph::ModuleId,
    lexer::{Sigil, TokenKind},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::{DiagnosticBuilder, SpanEntry};
use datalove_diagnostic::DiagnosticBuilderExt;

use super::Db;

/// Token source for parser - either Vec-backed or iterator-backed.
enum TokenSource<'db> {
    /// Vec-backed with position index (original approach).
    Vec {
        tokens: Vec<TreeToken<'db>>,
        pos: usize,
        /// Last consumed token for span tracking.
        last_token: Option<TreeToken<'db>>,
    },
    /// Iterator-backed with lookahead buffer (zero allocation).
    Iter {
        iter: BracerIter<'db>,
        /// Lookahead buffer: [0] = current, [1] = next.
        buffer: [Option<TreeToken<'db>>; 2],
        /// Last consumed token for span tracking.
        last_token: Option<TreeToken<'db>>,
    },
}

/// Parser state for datafun parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn Db,
    source: TokenSource<'db>,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::text::Text<'db>,
    /// Module ID for stable function identity (None for scripts).
    module_id: Option<ModuleId>,
    /// Accumulated expression spans (side table pattern).
    expr_spans: Vec<ast::ParseSpanEntry>,
    /// Accumulated break statement spans, indexed by local_index.
    break_spans: Vec<SpanEntry>,
    /// Accumulated continue statement spans, indexed by local_index.
    continue_spans: Vec<SpanEntry>,
    /// Accumulated return statement spans, indexed by local_index.
    ret_spans: Vec<SpanEntry>,
    /// Accumulated set statement spans, indexed by local_index.
    set_spans: Vec<SpanEntry>,
    /// Accumulated function definition spans, indexed by local_index.
    fun_spans: Vec<SpanEntry>,
    /// Accumulated type alias spans, indexed by local_index.
    type_alias_spans: Vec<SpanEntry>,
    /// Optional context for error messages showing the enclosing branch's opening token.
    branch_context: Option<(TextSpan<'db>, &'static str)>,
    /// Current function name for expression identity (None for script-level).
    current_fn_name: Option<InternedText<'db>>,
    /// Counter for expressions within current function.
    expr_counter: u32,
    /// Counter for function calls within current function.
    call_counter: u32,
    /// Counter for statements needing spans (break, continue).
    stmt_counter: u32,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens (Vec-backed).
    pub(super) fn new(
        db: &'db dyn Db,
        tokens: Vec<TreeToken<'db>>,
        source_text: bct::text::Text<'db>,
        module_id: Option<ModuleId>,
    ) -> Self {
        Parser {
            db,
            source: TokenSource::Vec { tokens, pos: 0, last_token: None },
            had_error: false,
            source_text,
            module_id,
            expr_spans: Vec::new(),
            break_spans: Vec::new(),
            continue_spans: Vec::new(),
            ret_spans: Vec::new(),
            set_spans: Vec::new(),
            fun_spans: Vec::new(),
            type_alias_spans: Vec::new(),
            branch_context: None,
            current_fn_name: None,
            expr_counter: 0,
            call_counter: 0,
            stmt_counter: 0,
        }
    }

    /// Create a new parser from a BracerIter (iterator-backed, zero allocation).
    pub(super) fn from_branch(db: &'db dyn Db, iter: BracerIter<'db>, source_text: bct::text::Text<'db>) -> Self {
        Self::from_branch_with_context(db, iter, source_text, None, None)
    }

    /// Create a new parser from a BracerIter with an optional context label.
    ///
    /// The context is used to add a secondary label to error messages showing
    /// the enclosing branch (e.g., "in this argument list").
    pub(super) fn from_branch_with_context(
        db: &'db dyn Db,
        iter: BracerIter<'db>,
        source_text: bct::text::Text<'db>,
        context: Option<(TextSpan<'db>, &'static str)>,
        module_id: Option<ModuleId>,
    ) -> Self {
        let mut parser = Parser {
            db,
            source: TokenSource::Iter {
                iter,
                buffer: [None, None],
                last_token: None,
            },
            had_error: false,
            source_text,
            module_id,
            expr_spans: Vec::new(),
            break_spans: Vec::new(),
            continue_spans: Vec::new(),
            ret_spans: Vec::new(),
            set_spans: Vec::new(),
            fun_spans: Vec::new(),
            type_alias_spans: Vec::new(),
            branch_context: context,
            current_fn_name: None,
            expr_counter: 0,
            call_counter: 0,
            stmt_counter: 0,
        };
        // Prime the buffer.
        parser.fill_iter_buffer();
        parser
    }

    /// Create a sub-parser that inherits function context from parent.
    ///
    /// This ensures expressions in sub-parsers get the correct function identity.
    pub(super) fn sub_parser(
        &mut self,
        iter: BracerIter<'db>,
        context: Option<(TextSpan<'db>, &'static str)>,
    ) -> Self {
        let mut parser = Parser {
            db: self.db,
            source: TokenSource::Iter {
                iter,
                buffer: [None, None],
                last_token: None,
            },
            had_error: false,
            source_text: self.source_text,
            module_id: self.module_id,
            expr_spans: Vec::new(),
            break_spans: Vec::new(),
            continue_spans: Vec::new(),
            ret_spans: Vec::new(),
            set_spans: Vec::new(),
            fun_spans: Vec::new(),
            type_alias_spans: Vec::new(),
            branch_context: context,
            current_fn_name: self.current_fn_name,
            expr_counter: self.expr_counter,
            call_counter: self.call_counter,
            stmt_counter: self.stmt_counter,
        };
        parser.fill_iter_buffer();
        parser
    }

    /// Merge state back from sub-parser after it finishes.
    pub(super) fn merge_from_sub(&mut self, sub: &mut Self) {
        self.had_error |= sub.had_error;
        self.expr_counter = sub.expr_counter;
        self.call_counter = sub.call_counter;
        self.stmt_counter = sub.stmt_counter;
        self.merge_spans_from(sub);
    }

    /// Get the module ID for this parser (for stable function identity).
    pub(super) fn module_id(&self) -> Option<ModuleId> {
        self.module_id
    }

    /// Get current function name for expression identity.
    pub(super) fn current_fn_name(&self) -> Option<InternedText<'db>> {
        self.current_fn_name
    }

    /// Enter function context for expression identity tracking.
    pub(super) fn enter_function(&mut self, name: InternedText<'db>) {
        self.current_fn_name = Some(name);
        self.expr_counter = 0;
        self.call_counter = 0;
    }

    /// Exit function context.
    pub(super) fn exit_function(&mut self) {
        self.current_fn_name = None;
    }

    /// Get next expression index and increment counter.
    pub(super) fn next_expr_index(&mut self) -> u32 {
        let idx = self.expr_counter;
        self.expr_counter += 1;
        idx
    }

    /// Get next function call index and increment counter.
    pub(super) fn next_call_index(&mut self) -> u32 {
        let idx = self.call_counter;
        self.call_counter += 1;
        idx
    }

    /// Fill the iterator buffer with next non-whitespace tokens.
    fn fill_iter_buffer(&mut self) {
        if let TokenSource::Iter { iter, buffer, .. } = &mut self.source {
            if buffer[0].is_none() {
                buffer[0] = Self::next_non_whitespace(self.db, iter);
            }
            if buffer[1].is_none() {
                buffer[1] = Self::next_non_whitespace(self.db, iter);
            }
        }
    }

    /// Get next non-whitespace token from iterator.
    fn next_non_whitespace(db: &'db dyn Db, iter: &mut BracerIter<'db>) -> Option<TreeToken<'db>> {
        loop {
            match iter.next() {
                Some(token) => {
                    if Self::is_non_whitespace(db, &token) {
                        return Some(token);
                    }
                }
                None => return None,
            }
        }
    }

    /// Check if a token is non-whitespace.
    fn is_non_whitespace(db: &'db dyn Db, token: &TreeToken<'db>) -> bool {
        match token {
            TreeToken::Token(t) => {
                !matches!(t.kind(db), TokenKind::Whitespace | TokenKind::Comment)
            }
            TreeToken::Branch { .. } => true,
        }
    }

    /// Get the source text for this parser.
    pub(super) fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }

    /// Emit both a diagnostic and create a StmtParseError node in one call.
    pub(super) fn emit_stmt_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Statement<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        let mut builder = DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.C(), label);
        if let Some((ctx_span, ctx_msg)) = &self.branch_context {
            builder = builder.secondary_label(ctx_span.C(), ctx_msg);
        }
        builder.emit_parse();
        ast::Statement::ParseError(ast::StmtParseError { text: ts.text, span: ts.span, message: message_text })
    }

    /// Emit both a diagnostic and create an ExprFun with ParseError kind in one call.
    pub(super) fn emit_expr_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprFun<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        let mut builder = DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.C(), label);
        if let Some((ctx_span, ctx_msg)) = &self.branch_context {
            builder = builder.secondary_label(ctx_span.C(), ctx_msg);
        }
        builder.emit_parse();
        ast::ExprFun::new(
            self.db,
            self.module_id,
            self.current_fn_name,
            self.next_expr_index(),
            ast::ExprFunKind::ParseError(ast::ExprFunParseError { text: ts.text, span: ts.span, message: message_text })
        )
    }

    /// Check if looking at a colon type hint.
    pub(super) fn peek_colon_type_hint(&self) -> bool {
        self.peek_sigil(Sigil::Colon)
    }

    /// Peek at the second token (one ahead of current).
    pub(super) fn peek_second(&self) -> Option<&TreeToken<'db>> {
        match &self.source {
            TokenSource::Vec { tokens, pos, .. } => tokens.get(*pos + 1),
            TokenSource::Iter { buffer, .. } => buffer[1].as_ref(),
        }
    }

    /// Check if the second token is a specific sigil.
    pub(super) fn peek_second_sigil(&self, sigil: Sigil) -> bool {
        match self.peek_second() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db), TokenKind::Sigil(s) if s == sigil)
            }
            Some(TreeToken::Branch { sigil: s, .. }) => *s == sigil,
            None => false,
        }
    }

    /// Get the byte position at start of current token (or end of last token).
    pub(super) fn current_byte_pos(&self) -> usize {
        if let Some(token) = self.peek() {
            self.extract_text_span(token).start()
        } else {
            // At end of input, return end of last token.
            self.last_byte_end()
        }
    }

    /// Get the byte position at end of previous token (after consuming).
    pub(super) fn last_byte_end(&self) -> usize {
        match &self.source {
            TokenSource::Vec { last_token, .. } | TokenSource::Iter { last_token, .. } => {
                if let Some(token) = last_token {
                    self.extract_text_span(token).end()
                } else {
                    0
                }
            }
        }
    }

    /// Get span for error reporting.
    ///
    /// Prefers current peek token; falls back to end of last consumed token.
    /// Use this instead of `peek_text_span()` when the error might occur at EOF.
    pub(super) fn error_span(&self) -> TextSpan<'db> {
        if let Some(token) = self.peek() {
            return self.extract_text_span(token);
        }
        // At EOF: zero-width span at end of last token.
        let end = self.last_byte_end();
        TextSpan::new(self.source_text, end..end)
    }

    /// Create an expression and record its span in the side table.
    pub(super) fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, ts: TextSpan<'db>) -> ast::ExprFun<'db> {
        use salsa::plumbing::AsId;
        let expr = ast::ExprFun::new(
            self.db,
            self.module_id,
            self.current_fn_name,
            self.next_expr_index(),
            kind,
        );
        self.expr_spans.push(ast::ParseSpanEntry::new(
            expr.as_id(),
            ts.text.as_id(),
            ts.span,
        ));
        expr
    }

    /// Take the accumulated expression spans (consumes them).
    pub(super) fn take_expr_spans(&mut self) -> Vec<ast::ParseSpanEntry> {
        rmx::std::mem::take(&mut self.expr_spans)
    }

    /// Take the accumulated break statement spans (consumes them).
    pub(super) fn take_break_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.break_spans)
    }

    /// Take the accumulated continue statement spans (consumes them).
    pub(super) fn take_continue_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.continue_spans)
    }

    /// Take the accumulated return statement spans (consumes them).
    pub(super) fn take_ret_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.ret_spans)
    }

    /// Take the accumulated set statement spans (consumes them).
    pub(super) fn take_set_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.set_spans)
    }

    /// Take the accumulated function definition spans (consumes them).
    pub(super) fn take_fun_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.fun_spans)
    }

    /// Take the accumulated type alias spans (consumes them).
    pub(super) fn take_type_alias_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.type_alias_spans)
    }

    /// Merge spans from a sub-parser into this parser.
    pub(super) fn merge_spans_from(&mut self, sub: &mut Self) {
        self.expr_spans.append(&mut sub.expr_spans);
        self.break_spans.append(&mut sub.break_spans);
        self.continue_spans.append(&mut sub.continue_spans);
        self.ret_spans.append(&mut sub.ret_spans);
        self.set_spans.append(&mut sub.set_spans);
        self.fun_spans.append(&mut sub.fun_spans);
        self.type_alias_spans.append(&mut sub.type_alias_spans);
    }

    /// Get next statement index and increment counter.
    pub(super) fn next_stmt_index(&mut self) -> u32 {
        let idx = self.stmt_counter;
        self.stmt_counter += 1;
        idx
    }

    /// Record a break statement span and return its local_index.
    pub(super) fn record_break_span(&mut self, ts: TextSpan<'db>) -> u32 {
        use salsa::plumbing::AsId;
        let index = self.next_stmt_index();
        self.break_spans.push(SpanEntry::new(ts.text.as_id(), ts.span));
        index
    }

    /// Record a continue statement span and return its local_index.
    pub(super) fn record_continue_span(&mut self, ts: TextSpan<'db>) -> u32 {
        use salsa::plumbing::AsId;
        let index = self.next_stmt_index();
        self.continue_spans.push(SpanEntry::new(ts.text.as_id(), ts.span));
        index
    }

    /// Record a return statement span and return its local_index.
    pub(super) fn record_ret_span(&mut self, ts: TextSpan<'db>) -> u32 {
        use salsa::plumbing::AsId;
        let index = self.next_stmt_index();
        self.ret_spans.push(SpanEntry::new(ts.text.as_id(), ts.span));
        index
    }

    /// Record a set statement span and return its local_index.
    pub(super) fn record_set_span(&mut self, ts: TextSpan<'db>) -> u32 {
        use salsa::plumbing::AsId;
        let index = self.next_stmt_index();
        self.set_spans.push(SpanEntry::new(ts.text.as_id(), ts.span));
        index
    }

    /// Record a function definition span and return its local_index.
    pub(super) fn record_fun_span(&mut self, ts: TextSpan<'db>) -> u32 {
        use salsa::plumbing::AsId;
        let index = self.next_stmt_index();
        self.fun_spans.push(SpanEntry::new(ts.text.as_id(), ts.span));
        index
    }

    /// Record a type alias span and return its local_index.
    pub(super) fn record_type_alias_span(&mut self, ts: TextSpan<'db>) -> u32 {
        use salsa::plumbing::AsId;
        let index = self.next_stmt_index();
        self.type_alias_spans.push(SpanEntry::new(ts.text.as_id(), ts.span));
        index
    }

    /// Emit error if tokens remain unconsumed after a successful parse.
    pub(super) fn error_if_not_exhausted(&mut self) {
        if self.peek().is_some() && !self.had_error {
            self.had_error = true;
            let ts = self.peek_text_span();
            let mut builder = DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("P021")
                .primary_label(ts, "unexpected token");
            if let Some((ctx_span, ctx_msg)) = &self.branch_context {
                builder = builder.secondary_label(ctx_span.C(), ctx_msg);
            }
            builder.emit_parse();
        }
    }
}

impl<'db> TokenStream<'db> for Parser<'db> {
    fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    fn peek(&self) -> Option<&TreeToken<'db>> {
        match &self.source {
            TokenSource::Vec { tokens, pos, .. } => tokens.get(*pos),
            TokenSource::Iter { buffer, .. } => buffer[0].as_ref(),
        }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        match &mut self.source {
            TokenSource::Vec { tokens, pos, last_token } => {
                let token = tokens.get(*pos).cloned();
                if token.is_some() {
                    *pos += 1;
                    // Remember last consumed token.
                    *last_token = token.C();
                }
                token
            }
            TokenSource::Iter { iter, buffer, last_token } => {
                // Take from slot 0.
                let result = buffer[0].take();
                // Remember last consumed token.
                *last_token = result.C();
                // Shift slot 1 to slot 0.
                buffer[0] = buffer[1].take();
                // Fill slot 1 from iterator.
                buffer[1] = Self::next_non_whitespace(self.db, iter);
                result
            }
        }
    }

    fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }
}
