//! Salsa-tracked API for script unit ownership analysis.
//!
//! Provides memoized, per-unit ownership analysis following the same pattern as
//! module ownership analysis. Each unit is analyzed independently, enabling
//! incremental recompilation when new units are added to a batch.

use rmx::prelude::*;
use std::collections::HashMap;
use std::ops::Range;
use bct::diagnostic::DiagnosticBuilder;
use bct::input::Source;
use bct::render;
use bct::text::TextSpan;
use datalove_datafun_ast::ast::{ExprKey, FunKey, ParsedExpr, Statement, TopLevel};
use datalove_datafun_ast::spans::DatafunSpans;
use datalove_datafun_tycheck::{UnitTypecheckResultTracked};
use datalove_diagnostic::DiagnosticBuilderExt;

use crate::lower::ScriptFunctionAnalyses;
use datalove_datafun_ownership::{
    self as ownership_analysis, DropSchedule, BindingInfo, FunctionAnalysis,
    TrackingCategory, format_analysis_errors,
};
pub use datalove_datafun_sema::{
    ScriptAnalysisData, AnalysisError, OwnershipRecoveryHint, ExternalBinding, FunctionExit, LoopExit,
};

/// Hashable wrapper for FunctionAnalysis.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct FunctionAnalysisData {
    pub schedule: DropSchedule,
    pub bindings: Vec<BindingInfo>,
    pub tracking: Vec<TrackingCategory>,
}

impl<'db> From<FunctionAnalysis<'db>> for FunctionAnalysisData {
    fn from(analysis: FunctionAnalysis<'db>) -> Self {
        Self {
            schedule: analysis.schedule,
            bindings: analysis.bindings,
            tracking: analysis.tracking,
        }
    }
}

/// Result of ownership analysis for a single script unit.
#[salsa::tracked]
pub struct ScriptUnitOwnershipResult<'db> {
    /// Function analyses: (name, analysis data).
    #[returns(ref)]
    pub function_analyses: Vec<(String, FunctionAnalysisData)>,
    /// Script-level analysis (None for expression units).
    #[returns(ref)]
    pub script_analysis: Option<ScriptAnalysisData>,
    /// Formatted error messages (for backward compatibility).
    #[returns(ref)]
    pub errors: Vec<String>,
    /// Structured errors for diagnostic emission.
    #[returns(ref)]
    pub structured_errors: Vec<AnalysisError<'db>>,
    /// Names this unit exports that hold no value.
    #[returns(ref)]
    pub dead_exports: Vec<String>,
    /// Names an earlier unit gave away that this unit assigned to.
    #[returns(ref)]
    pub revived_exports: Vec<String>,
    /// The earlier units' bindings this unit gave away.
    #[returns(ref)]
    pub taken_externals: Vec<String>,
    /// The earlier units' bindings this unit wrote to.
    #[returns(ref)]
    pub written_externals: Vec<String>,
    /// Those of them it overwrote, writing on every way through the unit
    /// before reading what they held; the rest it updated.
    #[returns(ref)]
    pub overwritten_externals: Vec<String>,
}

impl<'db> ScriptUnitOwnershipResult<'db> {
    /// Convert function_analyses to HashMap<StmtFun, FunctionAnalysis> for lowering.
    ///
    /// Reconstructs the map keyed by StmtFun by finding matching function statements
    /// by name in the provided statements list.
    pub fn to_function_analyses_map(
        &self,
        db: &'db dyn salsa::Database,
        stmts: &'db [Statement<'db>],
    ) -> ScriptFunctionAnalyses<'db> {
        let analyses = self.function_analyses(db);
        let mut map = bct::lookup::LookupMap::new();

        // Build name -> function lookup from statements.
        let mut name_to_stmt: HashMap<&str, FunKey<'db>> = HashMap::new();
        for stmt in stmts {
            if let Statement::Fun(func) = stmt {
                let name = func.name().text(db);
                name_to_stmt.insert(name, func.key());
            }
        }

        // Convert Vec<(String, FunctionAnalysisData)> back to a map by function.
        for (name, data) in analyses {
            if let Some(&func) = name_to_stmt.get(name.as_str()) {
                let analysis = FunctionAnalysis {
                    errors: Vec::new(), // Errors already extracted at analysis boundary.
                    schedule: data.schedule.clone(),
                    bindings: data.bindings.clone(),
                    tracking: data.tracking.clone(),
                };
                map.insert(func, analysis);
            }
        }

        map
    }
}

// ============================================================================
// Type Conversion Helpers
// ============================================================================

/// Analyze ownership for a script fragment unit, whose source is `source`.
///
/// Memoized: if typecheck_result and the unit's statements match a previous
/// call, returns the cached result. Each error is filed as an
/// `OwnershipDiagnostic`, read back as this query's accumulated diagnostics.
#[salsa::tracked(returns(copy))]
pub fn analyze_script_fragment_tracked<'db>(
    db: &'db dyn salsa::Database,
    source: Source,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    top: TopLevel<'db>,
    externals: Vec<ExternalBinding>,
    external_consts: Vec<String>,
) -> ScriptUnitOwnershipResult<'db> {
    datalove_ct::unit_analysis::record(datalove_ct::unit_analysis::Pass::Ownership, source);
    let expr_types = typecheck_result.expr_types(db);
    let body = top.body(db);

    // Build map of function name -> resolved param types for type alias support.
    let mut func_param_types: HashMap<String, Vec<datalove_datafun_tycheck::Type<'db>>> = HashMap::new();
    for (name, func_type) in typecheck_result.function_types(db) {
        func_param_types.insert(name.text(db).S(), func_type.param_types(db).C());
    }

    // Analyze functions in this unit for ownership.
    let func_analyses_result = ownership_analysis::analyze_script_functions(
        db, expr_types, typecheck_result.call_targets(db), &body.statements, Some(&func_param_types),
    );

    let (func_analyses, func_errors, structured_func_errors) = match func_analyses_result {
        Ok(analyses) => (analyses, Vec::new(), Vec::new()),
        Err(errors) => {
            let mut error_msgs = Vec::new();
            let mut structured_errors = Vec::new();
            for (func_name, errs) in errors {
                error_msgs.push(format!("{}: {}", func_name, format_analysis_errors(&errs)));
                structured_errors.extend(errs);
            }
            emit_ownership_errors(db, &datalove_datafun_parser::datafun_spans(db, source), &structured_errors);
            return ScriptUnitOwnershipResult::new(
                db,
                Vec::new(),
                None,
                error_msgs,
                structured_errors,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
        }
    };

    // Analyze script-level statements for drop schedule.
    let script_analysis = ownership_analysis::analyze_script_statements_with_externals(
        db, expr_types, typecheck_result.call_targets(db), body, externals, external_consts,
    );

    // Check for script analysis errors.
    if !script_analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&script_analysis.errors);
        let structured_errors = script_analysis.errors.clone();
        emit_ownership_errors(db, &datalove_datafun_parser::datafun_spans(db, source), &structured_errors);
        return ScriptUnitOwnershipResult::new(
            db,
            Vec::new(),
            None,
            vec![error_msg],
            structured_errors,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    // Convert function analyses to hashable format, sorted by name because
    // this is part of a memoized value and the map has no order.
    let mut func_analyses_data: Vec<(String, FunctionAnalysisData)> = func_analyses
        .into_iter_unordered()
        .map(|(stmt_fun, analysis)| {
            let name = stmt_fun.name.text(db).S();
            (name, FunctionAnalysisData::from(analysis))
        })
        .collect();
    func_analyses_data.sort_by(|a, b| a.0.cmp(&b.0));

    let dead_exports = script_analysis.dead_exports.clone();
    let revived_exports = script_analysis.revived_exports.clone();
    let taken_externals = script_analysis.taken_externals;
    let written_externals = script_analysis.written_externals;
    let overwritten_externals = script_analysis.overwritten_externals;
    let script_data = ScriptAnalysisData {
        schedule: script_analysis.schedule,
        bindings: script_analysis.bindings,
        tracking: script_analysis.tracking,
        unit_end: script_analysis.unit_end,
        dead_exports: script_analysis.dead_exports,
        revived_exports: script_analysis.revived_exports,
        externals: script_analysis.externals,
    };

    ScriptUnitOwnershipResult::new(
        db,
        func_analyses_data,
        Some(script_data),
        func_errors,
        structured_func_errors,
        dead_exports,
        revived_exports,
        taken_externals,
        written_externals,
        overwritten_externals,
    )
}

/// Analyze ownership for a script expression unit.
///
/// Expression units have no functions but still need ownership analysis to detect
/// use-after-move errors within the expression. For example, `(x, x)` where x is non-Copy.
///
/// Memoized: if typecheck_result and expr match a previous call, returns the
/// cached result. Errors are filed as for [`analyze_script_fragment_tracked`].
#[salsa::tracked(returns(copy))]
pub fn analyze_script_expr_tracked<'db>(
    db: &'db dyn salsa::Database,
    source: Source,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    parsed: ParsedExpr<'db>,
    externals: Vec<ExternalBinding>,
    external_consts: Vec<String>,
) -> ScriptUnitOwnershipResult<'db> {
    datalove_ct::unit_analysis::record(datalove_ct::unit_analysis::Pass::Ownership, source);
    let expr_types = typecheck_result.expr_types(db);

    let analysis = ownership_analysis::analyze_expr_with_externals(
        db, parsed.expr(db), expr_types, typecheck_result.call_targets(db),
        externals, external_consts,
    );

    if !analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&analysis.errors);
        let structured_errors = analysis.errors.clone();
        emit_ownership_errors(db, &datalove_datafun_parser::datafun_expr_spans(db, source), &structured_errors);
        return ScriptUnitOwnershipResult::new(
            db, Vec::new(), None, vec![error_msg], structured_errors,
            Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        );
    }

    // Expression units have no functions and no script-level drop schedule.
    // The expression result is consumed by the caller, so no drops needed.
    ScriptUnitOwnershipResult::new(
        db, Vec::new(), None, Vec::new(), Vec::new(), Vec::new(), Vec::new(),
        analysis.taken_externals, analysis.written_externals, analysis.overwritten_externals,
    )
}

// ============================================================================
// Ownership Diagnostics
// ============================================================================

/// File a diagnostic for each of `errors`, against `spans`.
fn emit_ownership_errors<'db>(db: &'db dyn salsa::Database, spans: &DatafunSpans<'db>, errors: &[AnalysisError<'db>]) {
    for error in errors {
        ownership_diagnostic(db, error, spans).emit_ownership();
    }
}

/// The source and span of an expression.
fn expr_span<'db>(spans: &DatafunSpans<'db>, expr_key: ExprKey<'db>) -> Option<(Source, Range<usize>)> {
    spans.lookup_key(expr_key).map(|entry| (entry.source, entry.span.C()))
}

/// The source and span of an expression an error points at, which the parse
/// that made the expression gave a span.
fn error_span<'db>(spans: &DatafunSpans<'db>, expr_key: ExprKey<'db>) -> (Source, Range<usize>) {
    expr_span(spans, expr_key).expect("an expression an ownership error points at has a span")
}

/// The span of a statement an error points at, which the parse that made the
/// statement gave one.
fn stmt_span(span: Option<&TextSpan>) -> TextSpan {
    span.expect("a statement an ownership error points at has a span").C()
}

/// The help for an error a `@` would fix, showing the line as it would read.
fn adapt_help<'db>(
    recovery_hint: &OwnershipRecoveryHint,
    source: &str,
    span: &Range<usize>,
    name: &str,
) -> Option<String> {
    let OwnershipRecoveryHint::InsertAdapt { .. } = recovery_hint else {
        return None;
    };
    Some(match render::insertion_suggestion(source, span, "@") {
        Some(suggestion) => fmt!("insert `@` to clone:\n{suggestion}"),
        None => fmt!("insert `@` to clone: `{name}@`"),
    })
}

/// The diagnostic for one ownership error, against the spans of its unit or
/// module.
///
pub fn ownership_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    error: &AnalysisError<'db>,
    spans: &DatafunSpans<'db>,
) -> DiagnosticBuilder<'db> {
    match error {
        AnalysisError::UseAfterMoveInEarlierUnit { expr_key, name, recovery_hint } => {
            let (text, span) = error_span(spans, *expr_key);
            let mut builder =
                DiagnosticBuilder::error(db, &fmt!("`{name}` was given away by an earlier input"))
                    .code("D013")
                    .primary_label(TextSpan::new(text, span), "value used after move");
            if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                builder = builder.help(&fmt!("use `@` to {description}"));
            }
            builder
        }
        AnalysisError::UseAfterMove { expr_key, moved_at, name, recovery_hint } => {
            let (text, span) = error_span(spans, *expr_key);
            let mut builder = DiagnosticBuilder::error(db, &fmt!("use of moved value: `{name}`"))
                .code("D001")
                .primary_label(TextSpan::new(text, span.C()), "value used after move");

            // The move is labelled too where it is somewhere else, and is
            // where the `@` goes.
            let move_span = match moved_at == expr_key {
                true => None,
                false => expr_span(spans, *moved_at).map(|(_, span)| span),
            };
            if let Some(move_span) = &move_span {
                builder = builder
                    .secondary_label(TextSpan::new(text, move_span.C()), "value moved here");
            }

            let source = text.as_str(db);
            let suggestion_span = move_span.unwrap_or(span);
            if let Some(help) = adapt_help(recovery_hint, source, &suggestion_span, name) {
                builder = builder.help(&help);
            }
            builder
        }
        AnalysisError::DoubleMove { expr_key, moved_at, name, recovery_hint } => {
            let (text, span) = error_span(spans, *expr_key);
            let mut builder = DiagnosticBuilder::error(db, &fmt!("value moved twice: `{name}`"))
                .code("D002")
                .primary_label(TextSpan::new(text, span.C()), "second move here");

            let move_span = match moved_at == expr_key {
                true => None,
                false => expr_span(spans, *moved_at).map(|(_, span)| span),
            };
            if let Some(move_span) = &move_span {
                builder = builder
                    .secondary_label(TextSpan::new(text, move_span.C()), "first move here");
            }

            let source = text.as_str(db);
            let suggestion_span = move_span.unwrap_or(span);
            if let Some(help) = adapt_help(recovery_hint, source, &suggestion_span, name) {
                builder = builder.help(&help);
            }
            builder
        }
        AnalysisError::CannotMoveBorrowed { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(db, &fmt!("cannot move borrowed value: `{name}`"))
                .code("D003")
                .primary_label(TextSpan::new(text, span), "cannot move borrowed value")
                .note("borrowed parameters (ref, mut, out) cannot be moved")
        }
        AnalysisError::CannotMoveConst { expr_key, name, recovery_hint } => {
            let (text, span) = error_span(spans, *expr_key);
            let mut builder = DiagnosticBuilder::error(db, &fmt!("cannot move out of const: `{name}`"))
                .code("D003")
                .primary_label(TextSpan::new(text, span.C()), "cannot move out of const")
                .note("a const is borrowed wherever it is named");
            if let Some(help) = adapt_help(recovery_hint, text.as_str(db), &span, name) {
                builder = builder.help(&help);
            }
            builder
        }
        AnalysisError::WalkedCollectionChanged { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(db, &fmt!("cannot change `{name}` while a `for` walks it"))
                .code("D015")
                .primary_label(TextSpan::new(text, span), "changes the collection being walked")
                .note("a `for` holds references into its collection until it ends")
        }
        AnalysisError::WalkedCollectionAssigned { set_index, name } => {
            let ts = spans.lookup_set(*set_index).expect("a `set` an ownership error points at has a span");
            DiagnosticBuilder::error(db, &fmt!("cannot change `{name}` while a `for` walks it"))
                .code("D015")
                .primary_label(ts.C(), "assigns to the collection being walked")
                .note("a `for` holds references into its collection until it ends")
        }
        AnalysisError::CannotMutFromRef { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(
                db,
                &fmt!("cannot get mutable reference from immutable: `{name}`"),
            )
            .code("D004")
            .primary_label(TextSpan::new(text, span), "ref parameter cannot be passed as mut")
        }
        AnalysisError::AliasedMutableArgument { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(db, &fmt!("aliased mutable argument: `{name}`"))
                .code("D010")
                .primary_label(TextSpan::new(text, span), "passed again in the same call")
                .note("an argument passed as `mut` or `out` cannot also be passed to another parameter")
        }
        AnalysisError::CannotMutateImmutable { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(
                db,
                &fmt!("cannot pass immutable binding as mutable: `{name}`"),
            )
            .code("D011")
            .primary_label(TextSpan::new(text, span), "passed to a `mut` or `out` parameter")
            .note(&fmt!("declare `{name}` with `var` to allow mutation"))
        }
        AnalysisError::CannotMutateTemporary { expr_key } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(db, "cannot pass a temporary as mutable")
                .code("D012")
                .primary_label(TextSpan::new(text, span), "passed to a `mut` or `out` parameter")
                .note("bind the value to a `var` first, so the mutation is observable")
        }
        AnalysisError::ReadUninitialized { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(db, &fmt!("read of uninitialized binding: `{name}`"))
                .code("D005")
                .primary_label(TextSpan::new(text, span), "used before initialization")
        }
        AnalysisError::MoveInLoop { expr_key, name, recovery_hint } => {
            let (text, span) = error_span(spans, *expr_key);
            let mut builder = DiagnosticBuilder::error(db, &fmt!("cannot move `{name}` in loop"))
                .code("D007")
                .primary_label(TextSpan::new(text, span.C()), "value moved inside loop");
            if let Some(help) = adapt_help(recovery_hint, text.as_str(db), &span, name) {
                builder = builder.help(&help);
            }
            builder
        }
        AnalysisError::OutParamPartialWrite { expr_key, name } => {
            let (text, span) = error_span(spans, *expr_key);
            DiagnosticBuilder::error(
                db,
                &fmt!("cannot partially write to out parameter: `{name}`"),
            )
            .code("D009")
            .primary_label(TextSpan::new(text, span), "partial write to out parameter")
            .note("out parameters must be written as a whole value")
        }
        AnalysisError::InconsistentBranchMove {
            at, name, gave_away, fix_in, changed_at, moved_before, ..
        } => {
            let message = datalove_datafun_sema::inconsistent_branch_message(error)
                .expect("is D008");
            let mut builder = DiagnosticBuilder::error(db, &message).code("D008");

            // The branch's own move or `set` is what to look at; the condition
            // only says which `if` or `match` it was.
            builder = match changed_at.and_then(|key| expr_span(spans, key)) {
                Some((text, span)) => {
                    let label = match gave_away {
                        true => "given away here",
                        false => "given a new value here",
                    };
                    builder.primary_label(TextSpan::new(text, span), label)
                }
                None => {
                    let (text, span) = error_span(spans, *at);
                    builder.primary_label(TextSpan::new(text, span), "the branches of this disagree")
                }
            };
            if let Some((text, span)) = moved_before.and_then(|key| expr_span(spans, key)) {
                builder = builder.secondary_label(
                    TextSpan::new(text, span),
                    &fmt!("`{name}` given away here, before the branches"),
                );
            }

            let help = match gave_away {
                true => fmt!("give `{name}` away {fix_in} too, or clone it with `@` where it is given away"),
                false => fmt!("give `{name}` a value {fix_in} too"),
            };
            builder
                .note(&fmt!(
                    "whether `{name}` is held after the branches depends on which one ran, \
                     and it has to be dropped without knowing"
                ))
                .help(&help)
        }
        AnalysisError::OutParamNotInitialized { name, exits } => {
            let mut builder = DiagnosticBuilder::error(db, &fmt!("out parameter not initialized: `{name}`"))
                .code("D006");
            for (index, exit) in exits.iter().enumerate() {
                let (span, label) = match exit {
                    FunctionExit::Ret { local_index } => {
                        (stmt_span(spans.lookup_ret(*local_index)), fmt!("returns without writing `{name}`"))
                    }
                    FunctionExit::Early { expr_key } => {
                        let (text, span) = error_span(spans, *expr_key);
                        (TextSpan::new(text, span), fmt!("can return early here, without writing `{name}`"))
                    }
                    FunctionExit::Set { set_index } => {
                        (stmt_span(spans.lookup_set(*set_index)), fmt!("this `set` can return early, without writing `{name}`"))
                    }
                    FunctionExit::End { fun_index } => {
                        (stmt_span(spans.lookup_fun(*fun_index)), fmt!("the body can run to its end without writing `{name}`"))
                    }
                };
                builder = match index {
                    0 => builder.primary_label(span, &label),
                    _ => builder.secondary_label(span, &label),
                };
            }
            builder.note("an out parameter has to be written on every way out of its function")
        }
        AnalysisError::InconsistentLoopExit { name, given_away, held, moved_at } => {
            let mut builder = DiagnosticBuilder::error(db, &fmt!("`{name}` is given away on one way out of this loop and not another"))
                .code("D014");
            let exit_span = |exit: &LoopExit<'db>| match exit {
                LoopExit::Break { local_index } => (stmt_span(spans.lookup_break(*local_index)), "this `break`"),
                LoopExit::ConditionFails { condition } => {
                    let (text, span) = error_span(spans, *condition);
                    (TextSpan::new(text, span), "the condition failing")
                }
                LoopExit::WalkEnds { collection } => {
                    let (text, span) = error_span(spans, *collection);
                    (TextSpan::new(text, span), "running out of elements")
                }
            };
            for (index, exit) in given_away.iter().enumerate() {
                let (span, way) = exit_span(exit);
                let label = fmt!("{way} leaves `{name}` given away");
                builder = match index {
                    0 => builder.primary_label(span, &label),
                    _ => builder.secondary_label(span, &label),
                };
            }
            for exit in held {
                let (span, way) = exit_span(exit);
                builder = builder.secondary_label(span, &fmt!("{way} leaves `{name}` still held"));
            }
            if let Some(moved_at) = moved_at {
                let (text, span) = error_span(spans, *moved_at);
                builder = builder.secondary_label(TextSpan::new(text, span), &fmt!("`{name}` given away here"));
            }
            builder
                .note(&fmt!(
                    "the move cannot happen twice, so it is not the problem. Whether `{name}` is \
                     still held after the loop depends on which way out ran, and a drop has to be \
                     placed without knowing."
                ))
                .help("give it away on every way out, or clone it with `@` at the move")
        }
    }
}
