//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    module_graph::ModuleId,
    lexer::Sigil,
    bracer::{Cursor, TreeToken},
    text::InternedText,
};

use datalove_datafun_ast::ast;
use datalove_datalit as datalit;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;

use super::Db;

/// Where a parser reads its tokens: a cursor over a line or a bracket, with
/// the two tokens at it held for lookahead.
pub(super) struct TokenSource<'db> {
    cursor: Cursor<'db>,
    /// Lookahead buffer: [0] = current, [1] = next.
    buffer: [Option<TreeToken<'db>>; 2],
    /// The end of the last token consumed, for span tracking.
    last_end: Option<usize>,
}

impl<'db> TokenSource<'db> {
    pub(super) fn new(mut cursor: Cursor<'db>) -> Self {
        let first = cursor.next();
        let second = cursor.next();
        TokenSource { cursor, buffer: [first, second], last_end: None }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let result = self.buffer[0].take();
        if let Some(token) = &result {
            self.last_end = Some(token.span().end);
        }
        self.buffer[0] = self.buffer[1].take();
        self.buffer[1] = self.cursor.next();
        result
    }
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
    pub require: u32,
    /// Bare names in type position, which are not statements but are numbered
    /// and filed the same way.
    pub alias: u32,
}

/// The side tables a parse fills, in the order it fills them.
///
/// One per parse rather than one per parser. A nested parser, for a line or a
/// bracket, used to keep its own and append them to its parent's when it was
/// merged, so an entry was copied once for each level of nesting above it, and
/// the appending was a sixth of what parsing allocated. The tables are moved
/// in and out instead; see `Parser::sub_parser` and `Parser::merge_spans_from`.
#[derive(Default)]
struct Filed<'db> {
    /// The expressions of the body being parsed, the last numbered one less
    /// than the parser's `expr_counter`.
    ///
    /// A script's top level is parsed a line at a time, each line's parser
    /// starting its own, so this may hold only the latest of a body's
    /// expressions; they are concatenated in the order they were numbered.
    exprs: Vec<ast::ExprFunKind<'db>>,
    /// Accumulated expression spans (side table pattern).
    expr_spans: Vec<ast::ParseTextSpan<'db>>,
    /// Accumulated break statement spans, indexed by local_index.
    break_spans: Vec<TextSpan>,
    /// Accumulated continue statement spans, indexed by local_index.
    continue_spans: Vec<TextSpan>,
    /// Accumulated return statement spans, indexed by local_index.
    ret_spans: Vec<TextSpan>,
    /// Accumulated set statement spans, indexed by local_index.
    set_spans: Vec<TextSpan>,
    /// Accumulated function definition spans, indexed by local_index.
    fun_spans: Vec<TextSpan>,
    /// Accumulated type alias spans, indexed by local_index.
    type_alias_spans: Vec<TextSpan>,
    import_spans: Vec<TextSpan>,
    /// Accumulated require statement spans, indexed by local_index.
    require_spans: Vec<TextSpan>,
    /// Accumulated spans of bare names in type position, indexed by local_index.
    alias_spans: Vec<TextSpan>,
    /// Accumulated `(alias, function)` pairs of qualified calls, as written.
    qualified_calls: Vec<(InternedText<'db>, InternedText<'db>)>,
}

impl<'db> Filed<'db> {
    fn is_empty(&self) -> bool {
        self.exprs.is_empty() && self.expr_spans.is_empty() && self.break_spans.is_empty() && self.continue_spans.is_empty()
            && self.ret_spans.is_empty() && self.set_spans.is_empty() && self.fun_spans.is_empty()
            && self.type_alias_spans.is_empty() && self.import_spans.is_empty()
            && self.require_spans.is_empty() && self.alias_spans.is_empty()
            && self.qualified_calls.is_empty()
    }

    fn append(&mut self, other: &mut Self) {
        self.exprs.append(&mut other.exprs);
        self.expr_spans.append(&mut other.expr_spans);
        self.break_spans.append(&mut other.break_spans);
        self.continue_spans.append(&mut other.continue_spans);
        self.ret_spans.append(&mut other.ret_spans);
        self.set_spans.append(&mut other.set_spans);
        self.fun_spans.append(&mut other.fun_spans);
        self.type_alias_spans.append(&mut other.type_alias_spans);
        self.import_spans.append(&mut other.import_spans);
        self.require_spans.append(&mut other.require_spans);
        self.alias_spans.append(&mut other.alias_spans);
        self.qualified_calls.append(&mut other.qualified_calls);
    }
}

/// Parser state for datafun parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn Db,
    source: TokenSource<'db>,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::input::Source,
    /// The same text as a string, which the tokens' spans index.
    text: &'db str,
    /// Module ID for stable function identity (None for scripts).
    module_id: Option<ModuleId<'db>>,
    /// What the parse has filed so far, moved into a nested parser while it
    /// runs and back when it is merged; see `Filed`.
    filed: Filed<'db>,
    /// Whether `filed` was lent by an enclosing parser, which a merge has to
    /// give back.
    owes_filed: bool,
    /// Optional context for error messages showing the enclosing branch's opening token.
    branch_context: Option<(TextSpan, &'static str)>,
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
    /// The script-level expressions filed so far, set aside while inside a
    /// function.
    script_exprs: Vec<ast::ExprFunKind<'db>>,
}

/// A sub-parser dropped holding its parent's tables would take everything
/// filed so far with it.
impl Drop for Parser<'_> {
    fn drop(&mut self) {
        debug_assert!(!self.owes_filed || std::thread::panicking(),
            "a sub-parser was dropped without being merged back");
    }
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens (Vec-backed).
    pub(super) fn new(
        db: &'db dyn Db,
        tokens: Cursor<'db>,
        source_text: bct::input::Source,
        module_id: Option<ModuleId<'db>>,
        counters: ScriptCounters,
    ) -> Self {
        Parser {
            db,
            source: TokenSource::new(tokens),
            had_error: false,
            source_text,
            text: source_text.as_str(db),
            module_id,
            filed: Filed::default(),
            owes_filed: false,
            branch_context: None,
            current_fn_name: None,
            expr_counter: counters.expr,
            stmt_counters: counters.stmts,
            script_expr_counter: counters.expr,
            call_counter: counters.call,
            script_call_counter: counters.call,
            script_exprs: vec![],
        }
    }

    /// Create a parser for a nested body, continuing this parser's identity.
    ///
    /// A nested parser that started its counters from zero would hand out keys
    /// that collide with the enclosing body's.
    pub(super) fn new_sub(&mut self, tokens: Cursor<'db>) -> Self {
        let mut sub = Parser::new(
            self.db,
            tokens,
            self.source_text,
            self.module_id,
            ScriptCounters::default(),
        );
        sub.filed = rmx::std::mem::take(&mut self.filed);
        sub.owes_filed = true;
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

    /// Create a new parser from a Cursor with an optional context label.
    ///
    /// The context is used to add a secondary label to error messages showing
    /// the enclosing branch (e.g., "in this argument list").
    pub(super) fn from_branch_with_context(
        db: &'db dyn Db,
        iter: Cursor<'db>,
        source_text: bct::input::Source,
        context: Option<(TextSpan, &'static str)>,
        module_id: Option<ModuleId<'db>>,
    ) -> Self {
        Parser {
            db,
            source: TokenSource::new(iter),
            had_error: false,
            source_text,
            text: source_text.as_str(db),
            module_id,
            filed: Filed::default(),
            owes_filed: false,
            branch_context: context,
            current_fn_name: None,
            expr_counter: 0,
            stmt_counters: StatementCounters::default(),
            script_expr_counter: 0,
            call_counter: 0,
            script_call_counter: 0,
            script_exprs: vec![],
        }
    }

    /// Create a sub-parser that inherits function context from parent.
    ///
    /// This ensures expressions in sub-parsers get the correct function identity.
    pub(super) fn sub_parser(
        &mut self,
        iter: Cursor<'db>,
        context: Option<(TextSpan, &'static str)>,
    ) -> Self {
        Parser {
            db: self.db,
            source: TokenSource::new(iter),
            had_error: false,
            source_text: self.source_text,
            text: self.text,
            module_id: self.module_id,
            filed: rmx::std::mem::take(&mut self.filed),
            owes_filed: true,
            branch_context: context,
            current_fn_name: self.current_fn_name,
            expr_counter: self.expr_counter,
            stmt_counters: self.stmt_counters,
            script_expr_counter: self.script_expr_counter,
            call_counter: self.call_counter,
            script_call_counter: self.script_call_counter,
            script_exprs: vec![],
        }
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

    /// Peek at the next token after the current one (lookahead of 2).
    pub(super) fn peek_next(&self) -> Option<&TreeToken<'db>> {
        self.source.buffer[1].as_ref()
    }

    /// Enter function context for expression identity tracking.
    pub(super) fn enter_function(&mut self, name: InternedText<'db>) {
        self.script_expr_counter = self.expr_counter;
        self.script_call_counter = self.call_counter;
        self.script_exprs = rmx::std::mem::take(&mut self.filed.exprs);
        self.current_fn_name = Some(name);
        self.expr_counter = 0;
        self.call_counter = 0;
    }

    /// Exit function context, restoring the script-level counters, and give
    /// back the function's expressions.
    pub(super) fn exit_function(&mut self) -> ast::ExprArena<'db> {
        debug_assert_eq!(self.filed.exprs.len(), self.expr_counter as usize);
        let arena = ast::ExprArena {
            module_id: self.module_id,
            fn_name: self.current_fn_name,
            exprs: rmx::std::mem::replace(&mut self.filed.exprs, rmx::std::mem::take(&mut self.script_exprs)),
        };
        self.current_fn_name = None;
        self.expr_counter = self.script_expr_counter;
        self.call_counter = self.script_call_counter;
        arena
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

    /// Get the source text for this parser.
    pub(super) fn source_text(&self) -> bct::input::Source {
        self.source_text
    }

    /// Emit both a diagnostic and create a StmtParseError node in one call.
    pub(super) fn emit_stmt_error(
        &mut self,
        ts: TextSpan,
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
        ast::Statement::ParseError(ast::StmtParseError { source: ts.source, span: ts.span, message: message_text })
    }

    /// Emit both a diagnostic and create an ExprFun with ParseError kind in one call.
    pub(super) fn emit_expr_error(
        &mut self,
        ts: TextSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprId {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        let mut builder = DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.C(), label);
        if let Some((ctx_span, ctx_msg)) = &self.branch_context {
            builder = builder.secondary_label(ctx_span.C(), ctx_msg);
        }
        builder.emit_parse();
        self.push_expr(ast::ExprFunKind::ParseError(ast::ExprFunParseError { source: ts.source, span: ts.span, message: message_text }))
    }

    /// Check if looking at a colon type hint.
    pub(super) fn peek_colon_type_hint(&self) -> bool {
        self.peek_sigil(Sigil::Colon)
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
        self.source.last_end.unwrap_or(0)
    }

    /// Get span for error reporting.
    ///
    /// Prefers current peek token; falls back to end of last consumed token.
    /// Use this instead of `peek_text_span()` when the error might occur at EOF.
    pub(super) fn error_span(&self) -> TextSpan {
        if let Some(token) = self.peek() {
            return self.extract_text_span(token);
        }
        // At EOF: zero-width span at end of last token.
        let end = self.last_byte_end();
        TextSpan::new(self.source_text, end..end)
    }

    /// Read the name a declaration introduces, reporting it if it is
    /// reserved for this kind of declaration.
    ///
    /// The name is taken either way, so the declaration parses on and the one
    /// report is the one the reader sees.
    pub(super) fn eat_declared_name(
        &mut self,
        kind: datalove_datalit::parser_util::NameKind,
    ) -> Option<InternedText<'db>> {
        let ts = self.peek_text_span();
        let name = self.eat_name()?;
        self.report_reserved_name(name, ts, kind);
        Some(name)
    }

    /// Report a declared name reserved for this kind of declaration (P064).
    pub(super) fn report_reserved_name(
        &mut self,
        name: InternedText<'db>,
        ts: TextSpan,
        kind: datalove_datalit::parser_util::NameKind,
    ) {
        use datalove_datalit::parser_util::{reserved_complaint, reserved_for};
        if let Some(reason) = reserved_for(name.as_str(self.db), kind) {
            self.had_error = true;
            let (message, label) = reserved_complaint(name.as_str(self.db), reason);
            DiagnosticBuilder::error(self.db, &message)
                .code("P064")
                .primary_label(ts, &label)
                .emit_parse();
        }
    }

    /// Report a postfix operator written apart from what it applies to (P021).
    pub(super) fn report_spaced_postfix(&mut self) {
        self.had_error = true;
        let ts = self.peek_text_span();
        let text = self.text()[ts.span.clone()].to_string();
        DiagnosticBuilder::error(self.db, &fmt!("this `{text}` is written apart from what it applies to"))
            .code("P021")
            .primary_label(ts, "a postfix operator is written against the expression before it")
            .emit_parse();
    }

    /// Where an expression this parser created begins.
    pub(super) fn expr_span_start(&self, expr: ast::ExprId) -> usize {
        // A parse error carries its own span rather than filing one.
        if let ast::ExprFunKind::ParseError(error) = self.expr_kind(expr) {
            return error.span.start;
        }
        let key = ast::ExprKey::new(self.module_id, self.current_fn_name, expr.0);
        self.filed.expr_spans.iter().rev()
            .find(|entry| entry.expr_key == key)
            .map(|entry| entry.span.start)
            .expect("an expression is given its span when it is created")
    }

    /// Create an expression and record its span in the side table.
    pub(super) fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, ts: TextSpan) -> ast::ExprId {
        let expr = self.push_expr(kind);
        self.filed.expr_spans.push(ast::ParseTextSpan::new(
            ast::ExprKey::new(self.module_id, self.current_fn_name, expr.0),
            ts.source,
            ts.span,
        ));
        expr
    }

    fn push_expr(&mut self, kind: ast::ExprFunKind<'db>) -> ast::ExprId {
        self.filed.exprs.push(kind);
        ast::ExprId(self.next_expr_index())
    }

    /// An expression this parser created, in the body it is parsing.
    pub(super) fn expr_kind(&self, expr: ast::ExprId) -> &ast::ExprFunKind<'db> {
        let back = (self.expr_counter - expr.0) as usize;
        &self.filed.exprs[self.filed.exprs.len() - back]
    }

    /// Take the accumulated expressions (consumes them).
    pub(super) fn take_exprs(&mut self) -> Vec<ast::ExprFunKind<'db>> {
        rmx::std::mem::take(&mut self.filed.exprs)
    }

    /// Take the accumulated expression spans (consumes them).
    pub(super) fn take_expr_spans(&mut self) -> Vec<ast::ParseTextSpan<'db>> {
        rmx::std::mem::take(&mut self.filed.expr_spans)
    }

    /// Take the accumulated break statement spans (consumes them).
    pub(super) fn take_break_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.break_spans)
    }

    /// Take the accumulated continue statement spans (consumes them).
    pub(super) fn take_continue_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.continue_spans)
    }

    /// Take the accumulated return statement spans (consumes them).
    pub(super) fn take_ret_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.ret_spans)
    }

    /// Take the accumulated set statement spans (consumes them).
    pub(super) fn take_set_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.set_spans)
    }

    /// Take the accumulated function definition spans (consumes them).
    pub(super) fn take_fun_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.fun_spans)
    }

    /// Take the accumulated type alias spans (consumes them).
    pub(super) fn take_type_alias_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.type_alias_spans)
    }

    pub(super) fn take_alias_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.alias_spans)
    }

    /// Start this parser's alias numbering where `outer`'s has reached.
    pub(super) fn lend_alias_numbering(&mut self, outer: &Self) {
        self.stmt_counters.alias = outer.stmt_counters.alias;
    }

    /// Take back the aliases a sub-parser filed, and its numbering.
    pub(super) fn take_alias_numbering(&mut self, sub: &mut Self) {
        self.stmt_counters.alias = sub.stmt_counters.alias;
        self.filed.alias_spans.append(&mut sub.filed.alias_spans);
    }

    /// Merge spans from a sub-parser into this parser.
    ///
    /// A sub-parser that was lent this parser's tables gives them back whole,
    /// with what it filed after what was there; one that started its own has
    /// them appended.
    pub(super) fn merge_spans_from(&mut self, sub: &mut Self) {
        if sub.owes_filed {
            debug_assert!(self.filed.is_empty(), "a parser filed spans while a sub-parser held its tables");
            sub.owes_filed = false;
            self.filed = rmx::std::mem::take(&mut sub.filed);
        } else {
            self.filed.append(&mut sub.filed);
        }
    }

    /// Note a qualified call, for [`ast::ParsedStatements::qualified_calls`].
    pub(super) fn record_qualified_call(&mut self, alias: InternedText<'db>, name: InternedText<'db>) {
        self.filed.qualified_calls.push((alias, name));
    }

    /// Take the accumulated qualified calls (consumes them).
    pub(super) fn take_qualified_calls(&mut self) -> Vec<(InternedText<'db>, InternedText<'db>)> {
        rmx::std::mem::take(&mut self.filed.qualified_calls)
    }

    /// Record a break statement span and return its local_index.
    pub(super) fn record_break_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.brk, |p| &mut p.filed.break_spans)
    }

    /// Record a continue statement span and return its local_index.
    pub(super) fn record_continue_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.cont, |p| &mut p.filed.continue_spans)
    }

    /// Record a return statement span and return its local_index.
    pub(super) fn record_ret_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.ret, |p| &mut p.filed.ret_spans)
    }

    /// Record a set statement span and return its local_index.
    pub(super) fn record_set_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.set, |p| &mut p.filed.set_spans)
    }

    /// Record a function definition span and return its local_index.
    pub(super) fn record_fun_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.fun, |p| &mut p.filed.fun_spans)
    }

    /// Record a type alias span and return its local_index.
    pub(super) fn record_type_alias_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.type_alias, |p| &mut p.filed.type_alias_spans)
    }

    /// Record an import span and return its local_index.
    pub(super) fn record_import_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.import, |p| &mut p.filed.import_spans)
    }

    pub(super) fn take_import_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.import_spans)
    }

    pub(super) fn record_require_span(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.require, |p| &mut p.filed.require_spans)
    }

    pub(super) fn take_require_spans(&mut self) -> Vec<TextSpan> {
        rmx::std::mem::take(&mut self.filed.require_spans)
    }

    /// File a statement's span under the count for its kind.
    ///
    /// The index handed back is where the span sits in that kind's vector
    /// once every parser's vectors have been concatenated, which is what a
    /// diagnostic looks it up by.
    fn record_stmt_span(
        &mut self,
        ts: TextSpan,
        count: impl Fn(&mut StatementCounters) -> &mut u32,
        spans: impl Fn(&mut Self) -> &mut Vec<TextSpan>,
    ) -> u32 {
        let counter = count(&mut self.stmt_counters);
        let index = *counter;
        *counter += 1;
        let entry = TextSpan::new(ts.source, ts.span);
        spans(self).push(entry);
        index
    }

    /// Check if a line starts with "end <keyword>".
    pub(super) fn line_is_end_keyword(&self, line: &Cursor<'db>, keyword: &str) -> bool {
        let mut line = *line;
        match (line.next(), line.next()) {
            (Some(TreeToken::Token(t1)), Some(TreeToken::Token(t2))) => {
                t1.word_str(self.text()) == Some("end") && t2.word_str(self.text()) == Some(keyword)
            }
            _ => false,
        }
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
    fn alias_index(&mut self, ts: TextSpan) -> u32 {
        self.record_stmt_span(ts, |c| &mut c.alias, |p| &mut p.filed.alias_spans)
    }

    fn alias_base(&self) -> u32 {
        self.stmt_counters.alias
    }

    fn absorb_aliases(&mut self, spans: Vec<TextSpan>, next: u32) {
        self.filed.alias_spans.extend(spans);
        self.stmt_counters.alias = next;
    }

    /// Report a type hint error against this parser rather than datalit's.
    ///
    /// A type hint inside a datafun statement is read straight out of this
    /// parser, so what goes wrong in one is this parser's error to record and
    /// to point at, down to the branch it was found in.
    fn type_hint_error(
        &mut self,
        ts: TextSpan,
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
            source: ts.source,
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
        self.source.buffer[0].as_ref()
    }

    fn peek_next(&self) -> Option<&TreeToken<'db>> {
        self.source.buffer[1].as_ref()
    }

    fn prev_end(&self) -> Option<usize> {
        self.source.last_end
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        self.source.next()
    }

    fn source_text(&self) -> bct::input::Source {
        self.source_text
    }

    fn text(&self) -> &'db str {
        self.text
    }
}
