//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    module_graph::ModuleId,
    lexer::{Sigil, TokenKind},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
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
        /// The end of the last token consumed, for span tracking.
        last_end: Option<usize>,
    },
    /// Iterator-backed with lookahead buffer (zero allocation).
    Iter {
        iter: BracerIter<'db>,
        /// Lookahead buffer: [0] = current, [1] = next.
        buffer: [Option<TreeToken<'db>>; 2],
        /// The end of the last token consumed, for span tracking.
        ///
        /// The token itself was kept here once, which meant cloning every
        /// `TreeToken` a second time on the way past -- and a branch carries a
        /// whole sub-iterator -- to read one `usize` back out of it.
        last_end: Option<usize>,
    },
}

/// Script-level identity counters carried from one line's parser to the next.
///
/// A `Parser` is built per line, so without this the script-level expression
/// keys would restart at zero on every line and collide.
#[derive(Copy, Clone, Default)]
pub(super) struct ScriptCounters {
    pub expr: u32,
    pub call: u32,
    pub stmts: StatementCounters,
}

/// How many statements of each kind have had their span filed.
///
/// A statement's span is looked up by its position in the vector for its own
/// kind, so the count has to be per kind and has to survive being carried
/// from one line's parser to the next: a script is parsed a line at a time
/// and the vectors are concatenated afterwards. A single counter shared by
/// every kind, restarting with each parser, gives a `ret` in the second
/// function the index of the one in the first.
#[derive(Clone, Copy, Default)]
pub(super) struct StatementCounters {
    pub brk: u32,
    pub cont: u32,
    pub ret: u32,
    pub set: u32,
    pub fun: u32,
    pub type_alias: u32,
    pub import: u32,
    /// Bare names in type position, which are not statements but are numbered
    /// and filed the same way.
    pub alias: u32,
}

/// Parser state for datafun parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn Db,
    source: TokenSource<'db>,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::text::Text<'db>,
    /// Module ID for stable function identity (None for scripts).
    module_id: Option<ModuleId<'db>>,
    /// Accumulated expression spans (side table pattern).
    expr_spans: Vec<ast::ParseSpanEntry<'db>>,
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
    import_spans: Vec<SpanEntry>,
    /// Accumulated spans of bare names in type position, indexed by local_index.
    alias_spans: Vec<SpanEntry>,
    /// Optional context for error messages showing the enclosing branch's opening token.
    branch_context: Option<(TextSpan<'db>, &'static str)>,
    /// Current function name for expression identity (None for script-level).
    current_fn_name: Option<InternedText<'db>>,
    /// Counter for expressions within current function.
    ///
    /// Reset on entering a function, since a key names the function it is in.
    /// At script level it runs for the whole parse, across the per-line
    /// parsers, so that script-level keys stay distinct.
    expr_counter: u32,
    stmt_counters: StatementCounters,
    /// Script-level expression counter, saved while inside a function.
    script_expr_counter: u32,
    /// Counter for function calls within current function.
    call_counter: u32,
    /// Script-level call counter, saved while inside a function.
    script_call_counter: u32,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens (Vec-backed).
    pub(super) fn new(
        db: &'db dyn Db,
        tokens: Vec<TreeToken<'db>>,
        source_text: bct::text::Text<'db>,
        module_id: Option<ModuleId<'db>>,
        counters: ScriptCounters,
    ) -> Self {
        Parser {
            db,
            source: TokenSource::Vec { tokens, pos: 0, last_end: None },
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
            import_spans: Vec::new(),
            alias_spans: Vec::new(),
            branch_context: None,
            current_fn_name: None,
            expr_counter: counters.expr,
            stmt_counters: counters.stmts,
            script_expr_counter: counters.expr,
            call_counter: counters.call,
            script_call_counter: counters.call,
        }
    }

    /// Create a parser for a nested body, continuing this parser's identity.
    ///
    /// A nested parser that started its counters from zero would hand out keys
    /// that collide with the enclosing body's.
    pub(super) fn new_sub(&self, tokens: Vec<TreeToken<'db>>) -> Self {
        let mut sub = Parser::new(
            self.db,
            tokens,
            self.source_text,
            self.module_id,
            ScriptCounters::default(),
        );
        sub.current_fn_name = self.current_fn_name;
        sub.expr_counter = self.expr_counter;
        sub.script_expr_counter = self.script_expr_counter;
        sub.stmt_counters = self.stmt_counters;
        sub.call_counter = self.call_counter;
        sub.script_call_counter = self.script_call_counter;
        sub
    }

    /// The script-level counters this parser reached, to seed the next line.
    pub(super) fn script_counters(&self) -> ScriptCounters {
        ScriptCounters {
            expr: if self.current_fn_name.is_some() { self.script_expr_counter } else { self.expr_counter },
            stmts: self.stmt_counters,
            call: if self.current_fn_name.is_some() { self.script_call_counter } else { self.call_counter },
        }
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
        module_id: Option<ModuleId<'db>>,
    ) -> Self {
        let mut parser = Parser {
            db,
            source: TokenSource::Iter {
                iter,
                buffer: [None, None],
                last_end: None,
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
            import_spans: Vec::new(),
            alias_spans: Vec::new(),
            branch_context: context,
            current_fn_name: None,
            expr_counter: 0,
            stmt_counters: StatementCounters::default(),
            script_expr_counter: 0,
            call_counter: 0,
            script_call_counter: 0,
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
                last_end: None,
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
            import_spans: Vec::new(),
            alias_spans: Vec::new(),
            branch_context: context,
            current_fn_name: self.current_fn_name,
            expr_counter: self.expr_counter,
            stmt_counters: self.stmt_counters,
            script_expr_counter: self.script_expr_counter,
            call_counter: self.call_counter,
            script_call_counter: self.script_call_counter,
        };
        parser.fill_iter_buffer();
        parser
    }

    /// Merge a nested parser's spans and identity counters, but not its
    /// statement counter, which indexes a positional span vector.
    pub(super) fn merge_identity_from(&mut self, sub: &mut Self) {
        self.had_error |= sub.had_error;
        self.expr_counter = sub.expr_counter;
        self.script_expr_counter = sub.script_expr_counter;
        self.call_counter = sub.call_counter;
        self.script_call_counter = sub.script_call_counter;
        self.stmt_counters = sub.stmt_counters;
        self.merge_spans_from(sub);
    }

    /// Merge state back from sub-parser after it finishes.
    pub(super) fn merge_from_sub(&mut self, sub: &mut Self) {
        self.had_error |= sub.had_error;
        self.expr_counter = sub.expr_counter;
        self.script_expr_counter = sub.script_expr_counter;
        self.call_counter = sub.call_counter;
        self.script_call_counter = sub.script_call_counter;
        self.stmt_counters = sub.stmt_counters;
        self.merge_spans_from(sub);
    }

    /// Get the module ID for this parser (for stable function identity).
    pub(super) fn module_id(&self) -> Option<ModuleId<'db>> {
        self.module_id
    }

    /// Get current function name for expression identity.
    pub(super) fn current_fn_name(&self) -> Option<InternedText<'db>> {
        self.current_fn_name
    }

    /// Peek at the next token after the current one (lookahead of 2).
    pub(super) fn peek_next(&self) -> Option<&TreeToken<'db>> {
        match &self.source {
            TokenSource::Vec { tokens, pos, .. } => tokens.get(*pos + 1),
            TokenSource::Iter { buffer, .. } => buffer[1].as_ref(),
        }
    }

    /// Enter function context for expression identity tracking.
    pub(super) fn enter_function(&mut self, name: InternedText<'db>) {
        self.script_expr_counter = self.expr_counter;
        self.script_call_counter = self.call_counter;
        self.current_fn_name = Some(name);
        self.expr_counter = 0;
        self.call_counter = 0;
    }

    /// Exit function context, restoring the script-level counters.
    pub(super) fn exit_function(&mut self) {
        self.current_fn_name = None;
        self.expr_counter = self.script_expr_counter;
        self.call_counter = self.script_call_counter;
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
                buffer[0] = Self::next_non_whitespace(iter);
            }
            if buffer[1].is_none() {
                buffer[1] = Self::next_non_whitespace(iter);
            }
        }
    }

    /// Get next non-whitespace token from iterator.
    fn next_non_whitespace(iter: &mut BracerIter<'db>) -> Option<TreeToken<'db>> {
        loop {
            match iter.next() {
                Some(token) => {
                    if Self::is_non_whitespace(&token) {
                        return Some(token);
                    }
                }
                None => return None,
            }
        }
    }

    /// Check if a token is non-whitespace.
    fn is_non_whitespace(token: &TreeToken<'db>) -> bool {
        match token {
            TreeToken::Token(t) => {
                !matches!(t.kind, TokenKind::Whitespace | TokenKind::Comment)
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
                matches!(token.kind, TokenKind::Sigil(s) if s == sigil)
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
            TokenSource::Vec { last_end, .. } | TokenSource::Iter { last_end, .. } => {
                last_end.unwrap_or(0)
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
    /// Read the name a declaration introduces, reporting it if it is
    /// reserved for this kind of declaration.
    ///
    /// The name is taken either way, so the declaration parses on and the one
    /// report is the one the reader sees.
    pub(super) fn eat_declared_name(
        &mut self,
        kind: datalove_datalit::parser_util::NameKind,
    ) -> Option<InternedText<'db>> {
        use datalove_datalit::parser_util::{reserved_complaint, reserved_for};
        let ts = self.peek_text_span();
        let name = self.eat_name()?;
        if let Some(reason) = reserved_for(name.as_str(self.db), kind) {
            self.had_error = true;
            let (message, label) = reserved_complaint(name.as_str(self.db), reason);
            DiagnosticBuilder::error(self.db, &message)
                .code("P064")
                .primary_label(ts, &label)
                .emit_parse();
        }
        Some(name)
    }

    pub(super) fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, ts: TextSpan<'db>) -> ast::ExprFun<'db> {
        let expr = ast::ExprFun::new(
            self.db,
            self.module_id,
            self.current_fn_name,
            self.next_expr_index(),
            kind,
        );
        self.expr_spans.push(ast::ParseSpanEntry::new(
            ast::ExprKey::of(self.db, expr),
            ts.text.source(self.db),
            ts.span,
        ));
        expr
    }

    /// Take the accumulated expression spans (consumes them).
    pub(super) fn take_expr_spans(&mut self) -> Vec<ast::ParseSpanEntry<'db>> {
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

    pub(super) fn take_alias_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.alias_spans)
    }

    /// Start this parser's alias numbering where `outer`'s has reached.
    pub(super) fn lend_alias_numbering(&mut self, outer: &Self) {
        self.stmt_counters.alias = outer.stmt_counters.alias;
    }

    /// Take back the aliases a sub-parser filed, and its numbering.
    pub(super) fn take_alias_numbering(&mut self, sub: &mut Self) {
        self.stmt_counters.alias = sub.stmt_counters.alias;
        self.alias_spans.append(&mut sub.alias_spans);
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
        self.import_spans.append(&mut sub.import_spans);
        self.alias_spans.append(&mut sub.alias_spans);
    }

    /// Record a break statement span and return its local_index.
    pub(super) fn record_break_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.brk, |p| &mut p.break_spans)
    }

    /// Record a continue statement span and return its local_index.
    pub(super) fn record_continue_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.cont, |p| &mut p.continue_spans)
    }

    /// Record a return statement span and return its local_index.
    pub(super) fn record_ret_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.ret, |p| &mut p.ret_spans)
    }

    /// Record a set statement span and return its local_index.
    pub(super) fn record_set_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.set, |p| &mut p.set_spans)
    }

    /// Record a function definition span and return its local_index.
    pub(super) fn record_fun_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.fun, |p| &mut p.fun_spans)
    }

    /// Record a type alias span and return its local_index.
    pub(super) fn record_type_alias_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.type_alias, |p| &mut p.type_alias_spans)
    }

    /// Record an import span and return its local_index.
    pub(super) fn record_import_span(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.import, |p| &mut p.import_spans)
    }

    pub(super) fn take_import_spans(&mut self) -> Vec<SpanEntry> {
        rmx::std::mem::take(&mut self.import_spans)
    }

    /// File a statement's span under the count for its kind.
    ///
    /// The index handed back is where the span sits in that kind's vector
    /// once every parser's vectors have been concatenated, which is what a
    /// diagnostic looks it up by.
    fn record_stmt_span(
        &mut self,
        ts: TextSpan<'db>,
        count: impl Fn(&mut StatementCounters) -> &mut u32,
        spans: impl Fn(&mut Self) -> &mut Vec<SpanEntry>,
    ) -> u32 {
        let counter = count(&mut self.stmt_counters);
        let index = *counter;
        *counter += 1;
        let entry = SpanEntry::new(ts.text.source(self.db), ts.span);
        spans(self).push(entry);
        index
    }

    /// Check if a line starts with "end <keyword>".
    pub(super) fn line_is_end_keyword(&self, line: &[TreeToken<'db>], keyword: &str) -> bool {
        if line.len() >= 2 {
            if let (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) = (line.get(0), line.get(1)) {
                return t1.word_str(self.db) == Some("end") && t2.word_str(self.db) == Some(keyword);
            }
        }
        false
    }

    /// Emit error if tokens remain unconsumed after a successful parse.
    pub(super) fn error_if_not_exhausted(&mut self) {
        if self.peek().is_some() && !self.had_error {
            self.had_error = true;
            let ts = self.peek_text_span();
            let (message, label) = match self.lopsided_operator() {
                Some(complaint) => complaint,
                None => (S("unexpected token after expression"), S("unexpected token")),
            };
            let mut builder = DiagnosticBuilder::error(self.db, &message)
                .code("P021")
                .primary_label(ts, &label);
            if let Some((ctx_span, ctx_msg)) = &self.branch_context {
                builder = builder.secondary_label(ctx_span.C(), ctx_msg);
            }
            builder.emit_parse();
        }
    }
}

impl<'db> datalit::parser::TypeHintStream<'db> for Parser<'db> {
    /// File where a bare name in type position was written.
    ///
    /// Only this layer resolves one, so only this layer has anything to say
    /// when it cannot, and saying it needs the name's own span.
    fn alias_index(&mut self, ts: TextSpan<'db>) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.alias, |p| &mut p.alias_spans)
    }

    fn alias_base(&self) -> u32 {
        self.stmt_counters.alias
    }

    fn absorb_aliases(&mut self, spans: Vec<SpanEntry>, next: u32) {
        self.alias_spans.extend(spans);
        self.stmt_counters.alias = next;
    }

    /// Report a type hint error against this parser rather than datalit's.
    ///
    /// A type hint inside a datafun statement is read straight out of this
    /// parser, so what goes wrong in one is this parser's error to record and
    /// to point at, down to the branch it was found in.
    fn type_hint_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> datalit::ast::TypeHint<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        let mut builder = DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.C(), label);
        if let Some((ctx_span, ctx_msg)) = &self.branch_context {
            builder = builder.secondary_label(ctx_span.C(), ctx_msg);
        }
        builder.emit_parse();
        datalit::ast::TypeHint::ParseError(datalit::ast::TypeHintParseError {
            text: ts.text,
            span: ts.span,
            message: message_text,
        })
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

    fn peek_next(&self) -> Option<&TreeToken<'db>> {
        match &self.source {
            TokenSource::Vec { tokens, pos, .. } => tokens.get(pos.checked_add(1).X()),
            TokenSource::Iter { buffer, .. } => buffer[1].as_ref(),
        }
    }

    fn prev_end(&self) -> Option<usize> {
        match &self.source {
            TokenSource::Vec { last_end, .. } | TokenSource::Iter { last_end, .. } => *last_end,
        }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        match &mut self.source {
            TokenSource::Vec { tokens, pos, last_end } => {
                let token = tokens.get(*pos).cloned();
                if let Some(token) = token.as_ref() {
                    *pos += 1;
                    *last_end = Some(token.span().end);
                }
                token
            }
            TokenSource::Iter { iter, buffer, last_end } => {
                // Take from slot 0.
                let result = buffer[0].take();
                *last_end = result.as_ref().map(|token| token.span().end);
                // Shift slot 1 to slot 0.
                buffer[0] = buffer[1].take();
                // Fill slot 1 from iterator.
                buffer[1] = Self::next_non_whitespace(iter);
                result
            }
        }
    }

    fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }
}
