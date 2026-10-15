//! Datafun parser module.
//!
//! Parses datafun source text into AST nodes.

mod state;
mod statement;
mod expr;
mod literal;

use rmx::prelude::*;

use bct::{
    input::Source,
    module_graph::ModuleId,
    bracer::{Bracer, Cursor},
    parser_util,
    split,
    lexer,
    bracer,
};

use datalove_datafun_ast::ast;
use bct::text::InternedText;
use datalove_diagnostic::DiagnosticBuilderExt;
use state::{Parser, ScriptCounters};

use salsa::Database as Db;

/// Parse a Source into a datafun script with span information.
///
/// For script parsing (no module context).
#[salsa::tracked(returns(ref))]
pub fn parse<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParseResult<'db> {
    parse_with_module_id(db, source, None)
}

/// Parse a Source into a datafun script with span information and module context.
///
/// The module_id is used to give functions stable identity for memoization.
pub fn parse_with_module_id<'db>(
    db: &'db dyn Db,
    source: Source,
    module_id: Option<ModuleId<'db>>,
) -> ast::ParseResult<'db> {
    let top_level = if module_id.is_some() { TopLevel::Module } else { TopLevel::Script };
    parse_source(db, source, module_id, top_level)
}

/// Parse a rider interface, a `rider.dli`.
///
/// An interface declares what a rider's crate implements, so it holds
/// `native fun` and `type` and nothing else (P084). Parsed on its own so that
/// what it reports can be asked of it, as a module's can.
#[salsa::tracked(returns(ref))]
pub fn parse_rider_interface<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParseResult<'db> {
    parse_source(db, source, None, TopLevel::Interface)
}

/// What a source's top level may hold.
#[derive(Copy, Clone, PartialEq, Eq)]
enum TopLevel {
    /// Anything: a script runs its statements from top to bottom.
    Script,
    /// Declarations only (P077).
    Module,
    /// `native fun` and `type` only (P084).
    Interface,
}

fn parse_source<'db>(
    db: &'db dyn Db,
    source: Source,
    module_id: Option<ModuleId<'db>>,
    top_level: TopLevel,
) -> ast::ParseResult<'db> {
    let bracer = bracer::bracer(db, lexer::lex(db, source));
    datalove_datalit::parser_util::emit_bracer_errors(db, bracer, source, "P050", "P051");
    emit_non_ascii_name_errors(db, bracer, source);
    parse_bracer(db, bracer, source, module_id, top_level)
}

/// Parse a Source as a single expression.
#[salsa::tracked(returns(ref))]
pub fn parse_expr<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedExpr<'db> {
    let bracer = bracer::bracer(db, lexer::lex(db, source));
    datalove_datalit::parser_util::emit_bracer_errors(db, bracer, source, "P050", "P051");
    emit_non_ascii_name_errors(db, bracer, source);
    // Expressions don't have module context.
    parse_bracer_expr(db, bracer, source)
}

fn parse_bracer_expr<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
    source_text: bct::input::Source,
) -> ast::ParsedExpr<'db> {
    // Expressions don't have module context.
    let mut parser = Parser::from_branch_with_context(db, bracer.iter(db), source_text, None, None);
    let root = parser.parse_expr_full();
    parser.error_if_not_exhausted();
    let qualified_calls = each_once(parser.take_qualified_calls());
    let arena = ast::ExprArena { module_id: None, fn_name: None, exprs: parser.take_exprs() };
    let top = ast::TopLevel::new(db, ast::Body { statements: vec![], arena });
    ast::ParsedExpr { top, root, qualified_calls }
}

/// Qualified calls as written, each pair kept at its first appearance.
fn each_once<'db>(calls: Vec<(InternedText<'db>, InternedText<'db>)>) -> ast::QualifiedCalls<'db> {
    let mut once = Vec::with_capacity(calls.len());
    for call in calls {
        if !once.contains(&call) {
            once.push(call);
        }
    }
    std::sync::Arc::new(once)
}

/// Emit parse diagnostics for names written outside ASCII.
fn emit_non_ascii_name_errors<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
    source_text: Source,
) {
    let text = source_text.as_str(db);
    for span in parser_util::non_ascii_names(bracer.lexed(db).tokens(db), text) {
        parser_util::non_ascii_name_error(db, source_text, span)
            .code("P075")
            .emit_parse();
    }
}

fn parse_bracer<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
    source_text: Source,
    module_id: Option<ModuleId<'db>>,
    top_level: TopLevel,
) -> ast::ParseResult<'db> {
    // Split into lines. A line break inside balanced braces is not one here,
    // since the bracer has already put it inside a branch.
    let lines = split::Lines::new(bracer.iter(db));
    let (body, spans, qualified_calls) = parse_statements(db, lines, source_text, module_id, top_level);
    let parsed = ast::ParsedStatements {
        top: ast::TopLevel::new(db, body),
        qualified_calls: each_once(qualified_calls),
    };
    ast::ParseResult {
        parsed,
        expr_spans: spans.expr_spans,
        break_spans: spans.break_spans,
        continue_spans: spans.continue_spans,
        ret_spans: spans.ret_spans,
        set_spans: spans.set_spans,
        fun_spans: spans.fun_spans,
        type_alias_spans: spans.type_alias_spans,
        import_spans: spans.import_spans,
        require_spans: spans.require_spans,
        alias_spans: spans.alias_spans,
    }
}

/// Parsed statement spans result.
struct ParsedSpans<'db> {
    expr_spans: Vec<ast::ParseSpanEntry<'db>>,
    break_spans: Vec<bct::text::TextSpan>,
    continue_spans: Vec<bct::text::TextSpan>,
    ret_spans: Vec<bct::text::TextSpan>,
    set_spans: Vec<bct::text::TextSpan>,
    fun_spans: Vec<bct::text::TextSpan>,
    type_alias_spans: Vec<bct::text::TextSpan>,
    import_spans: Vec<bct::text::TextSpan>,
    require_spans: Vec<bct::text::TextSpan>,
    alias_spans: Vec<bct::text::TextSpan>,
}

/// Parse statements from lines, creating a Parser for each line.
///
/// Returns statements and accumulated spans.
fn parse_statements<'db>(
    db: &'db dyn Db,
    lines: impl Iterator<Item = Cursor<'db>>,
    source_text: bct::input::Source,
    module_id: Option<ModuleId<'db>>,
    top_level: TopLevel,
) -> (ast::Body<'db>, ParsedSpans<'db>, Vec<(InternedText<'db>, InternedText<'db>)>) {
    let mut statements = vec![];
    let mut exprs = vec![];
    let mut qualified_calls: Vec<(InternedText<'db>, InternedText<'db>)> = vec![];
    let mut all_expr_spans = vec![];
    let mut all_break_spans = vec![];
    let mut all_continue_spans = vec![];
    let mut all_ret_spans = vec![];
    let mut all_set_spans = vec![];
    let mut all_fun_spans = vec![];
    let mut all_type_alias_spans = vec![];
    let mut all_import_spans = vec![];
    let mut all_require_spans = vec![];
    let mut all_alias_spans = vec![];
    let mut line_iter = lines.enumerate().peekable();
    let mut counters = ScriptCounters::default();

    while let Some((_line_num, line)) = line_iter.next() {
        let mut parser = Parser::new(db, line, source_text, module_id, counters);
        match top_level {
            TopLevel::Script => {}
            TopLevel::Module => parser.refuse_at_module_top_level(),
            TopLevel::Interface => parser.refuse_at_interface_top_level(),
        }
        let statement = parser.parse_statement(&mut line_iter);
        counters = parser.script_counters();
        statements.push(statement);
        exprs.extend(parser.take_exprs());
        all_expr_spans.extend(parser.take_expr_spans());
        all_break_spans.extend(parser.take_break_spans());
        all_continue_spans.extend(parser.take_continue_spans());
        all_ret_spans.extend(parser.take_ret_spans());
        all_set_spans.extend(parser.take_set_spans());
        all_fun_spans.extend(parser.take_fun_spans());
        all_type_alias_spans.extend(parser.take_type_alias_spans());
        all_import_spans.extend(parser.take_import_spans());
        all_require_spans.extend(parser.take_require_spans());
        all_alias_spans.extend(parser.take_alias_spans());
        qualified_calls.extend(parser.take_qualified_calls());
    }

    let spans = ParsedSpans {
        expr_spans: all_expr_spans,
        break_spans: all_break_spans,
        continue_spans: all_continue_spans,
        ret_spans: all_ret_spans,
        set_spans: all_set_spans,
        fun_spans: all_fun_spans,
        type_alias_spans: all_type_alias_spans,
        import_spans: all_import_spans,
        require_spans: all_require_spans,
        alias_spans: all_alias_spans,
    };
    debug_assert_eq!(exprs.len(), counters.expr as usize);
    let body = ast::Body {
        statements,
        arena: ast::ExprArena { module_id, fn_name: None, exprs },
    };
    (body, spans, qualified_calls)
}

/// Tracked wrapper for parser tests that only need the ParsedStatements.
#[salsa::tracked(returns(ref))]
pub fn parse_for_test<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedStatements<'db> {
    parse(db, source).parsed.clone()
}

/// Public tracked wrapper for integration tests that returns just the ParsedStatements.
///
/// Integration tests are compiled as separate binaries and need pub access.
#[salsa::tracked(returns(ref))]
pub fn parse_integration_test<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedStatements<'db> {
    parse(db, source).parsed.clone()
}

/// Public tracked wrapper for integration code to enable diagnostic accumulation.
///
/// This function should be called before parse() to accumulate diagnostics,
/// then parse() can be called separately to get the full ParseResult.
/// Returns just the ParsedStatements to satisfy Salsa's type requirements.
#[salsa::tracked(returns(ref))]
pub fn parse_for_diagnostics<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedStatements<'db> {
    parse(db, source).parsed.clone()
}

use datalove_datafun_ast::spans::{SpanMapEntry, DatafunSpans};

/// Extract datafun expression spans from a parsed source.
///
/// Reads spans from the ParseResult side table (no accumulators).
pub fn datafun_spans<'db>(
    db: &'db dyn Db,
    source: Source,
) -> DatafunSpans<'db> {
    use bct::text::TextSpan;

    let parse_result = parse(db, source);
    let entries: Vec<SpanMapEntry> = parse_result.expr_spans
        .iter()
        .map(|e| SpanMapEntry {
            expr_key: e.expr_key,
            entry: TextSpan::new(e.source, e.span.C()),
        })
        .collect();

    DatafunSpans::with_stmt_spans(
        entries,
        parse_result.break_spans.C(),
        parse_result.continue_spans.C(),
        parse_result.ret_spans.C(),
        parse_result.set_spans.C(),
        parse_result.fun_spans.C(),
        parse_result.type_alias_spans.C(),
        parse_result.import_spans.C(),
        parse_result.require_spans.C(),
        parse_result.alias_spans.C(),
    )
}

// ============================================================================
// Parsing a module
// ============================================================================

/// Parse a module and return just its statements.
///
/// Derived from [`parse_module_full`] rather than parsing again, so a module is
/// parsed once however many callers want only its statements. Splitting the
/// statements out this way also firewalls them: an edit that moves spans
/// without changing any statement re-runs `parse_module_full`, but this returns
/// an equal value and backdates, so name resolution downstream does not re-run.
#[salsa::tracked(returns(ref))]
pub fn parse_module_ast<'db>(
    db: &'db dyn Db,
    module: bct::module_graph::Module<'db>,
) -> ast::ParsedStatements<'db> {
    parse_module_full(db, module).parsed.clone()
}

/// What a module declares, its function bodies left out.
///
/// Name resolution and import resolution read this rather than the statements,
/// so an edit inside a body, which changes the statements, leaves them be.
#[salsa::tracked(returns(ref))]
pub fn module_items<'db>(
    db: &'db dyn Db,
    module: bct::module_graph::Module<'db>,
) -> ast::Items<'db> {
    ast::Items::of(db, parse_module_ast(db, module))
}

/// What a module requires, imports and calls through an alias: its
/// `module_items` without the declarations.
///
/// What import resolution reads. A signature edit changes `module_items`, and
/// with them the module's exports, which reach the importers that way; the
/// module's own imports are unaffected, and this keeps them from re-resolving.
#[salsa::tracked(returns(ref))]
pub fn module_uses<'db>(
    db: &'db dyn Db,
    module: bct::module_graph::Module<'db>,
) -> ast::Items<'db> {
    let items = module_items(db, module);
    ast::Items {
        items: items.items.iter()
            .filter(|item| matches!(item, ast::Item::Require(_) | ast::Item::Import(_)))
            .cloned()
            .collect(),
        qualified_calls: items.qualified_calls.clone(),
    }
}

/// The spans of one module, in the form diagnostics look things up in.
///
/// `parse_module_full` returns the side tables as the parser filled them;
/// this is the same data keyed for lookup. Tracked and keyed on the module,
/// so building it is a cost only the modules something actually reports a
/// diagnostic against pay, rather than every module on every compile.
#[salsa::tracked(returns(ref))]
pub fn module_spans<'db>(
    db: &'db dyn Db,
    module: bct::module_graph::Module<'db>,
) -> DatafunSpans<'db> {
    parse_result_spans(parse_module_full(db, module))
}

/// The spans of one parse, in the form diagnostics look things up in.
pub fn parse_result_spans<'db>(full: &ast::ParseResult<'db>) -> DatafunSpans<'db> {
    DatafunSpans::with_stmt_spans(
        full.expr_spans.iter().map(|e| SpanMapEntry {
            expr_key: e.expr_key,
            entry: bct::text::TextSpan::new(e.source, e.span.clone()),
        }).collect(),
        full.break_spans.clone(),
        full.continue_spans.clone(),
        full.ret_spans.clone(),
        full.set_spans.clone(),
        full.fun_spans.clone(),
        full.type_alias_spans.clone(),
        full.import_spans.clone(),
        full.require_spans.clone(),
        full.alias_spans.clone(),
    )
}

/// Parse a module and return its statements and spans together.
///
/// The two come from one parse, so the expression keys in the spans match the
/// statements beside them.
#[salsa::tracked(returns(ref))]
pub fn parse_module_full<'db>(
    db: &'db dyn Db,
    module: bct::module_graph::Module<'db>,
) -> ast::ParseResult<'db> {
    let module_id = module.id(db);
    datalove_ct::query_log::log_query("parse", module_id.path(db), datalove_ct::query_log::QueryPhase::Start);
    let result = parse_with_module_id(db, module.source(db), Some(module_id));
    datalove_ct::query_log::log_query("parse", module_id.path(db), datalove_ct::query_log::QueryPhase::End);
    result
}
