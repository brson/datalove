//! Salsa-tracked API for script unit ownership analysis.
//!
//! Provides memoized, per-unit ownership analysis following the same pattern as
//! module ownership analysis. Each unit is analyzed independently, enabling
//! incremental recompilation when new units are added to a batch.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::text::TextSpan;
use datalove_datafun_ast::ast::{Statement, StmtFun};
use datalove_datafun_ast::spans::DatafunSpans;
use datalove_datafun_ir::IrType;
use datalove_datafun_tycheck::{UnitTypecheckResultTracked};
use datalove_diagnostic::DiagnosticBuilderExt;

use crate::IrTypeExt;
use crate::lower::ScriptFunctionAnalyses;
use datalove_datafun_ownership::{
    self as ownership_analysis, DropSchedule, BindingInfo, FunctionAnalysis,
    TrackingCategory, format_analysis_errors, AutoAdaptMode,
};
pub use datalove_datafun_sema::{
    ScriptAnalysisData, AnalysisError, OwnershipRecoveryHint, AdaptSites,
};

// Re-export AutoAdaptMode for callers.
pub use datalove_datafun_ownership::AutoAdaptMode as OwnershipAutoAdaptMode;

/// Hashable wrapper for FunctionAnalysis.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::SalsaValue)]
pub struct FunctionAnalysisData<'db> {
    pub schedule: DropSchedule,
    pub bindings: Vec<BindingInfo>,
    pub tracking: Vec<TrackingCategory>,
    pub adapt_sites: AdaptSites<'db>,
}

impl<'db> From<FunctionAnalysis<'db>> for FunctionAnalysisData<'db> {
    fn from(analysis: FunctionAnalysis<'db>) -> Self {
        Self {
            schedule: analysis.schedule,
            bindings: analysis.bindings,
            tracking: analysis.tracking,
            adapt_sites: analysis.adapt_sites,
        }
    }
}

/// Result of ownership analysis for a single script unit.
#[salsa::tracked]
pub struct ScriptUnitOwnershipResult<'db> {
    /// Function analyses: (name, analysis data).
    #[returns(ref)]
    pub function_analyses: Vec<(String, FunctionAnalysisData<'db>)>,
    /// Script-level analysis (None for expression units).
    #[returns(ref)]
    pub script_analysis: Option<ScriptAnalysisData<'db>>,
    /// Formatted error messages (for backward compatibility).
    #[returns(ref)]
    pub errors: Vec<String>,
    /// Structured errors for diagnostic emission.
    #[returns(ref)]
    pub structured_errors: Vec<AnalysisError<'db>>,
    /// Names this unit exports that hold no value.
    #[returns(ref)]
    pub dead_exports: Vec<String>,
    /// Names from earlier units this unit assigned to.
    #[returns(ref)]
    pub revived_exports: Vec<String>,
}

impl<'db> ScriptUnitOwnershipResult<'db> {
    /// Convert function_analyses to HashMap<StmtFun, FunctionAnalysis> for lowering.
    ///
    /// Reconstructs the map keyed by StmtFun by finding matching function statements
    /// by name in the provided statements list.
    pub fn to_function_analyses_map(
        &self,
        db: &'db dyn salsa::Database,
        stmts: &[Statement<'db>],
    ) -> ScriptFunctionAnalyses<'db> {
        let analyses = self.function_analyses(db);
        let mut map = HashMap::new();

        // Build name -> StmtFun lookup from statements.
        let mut name_to_stmt: HashMap<&str, StmtFun<'db>> = HashMap::new();
        for stmt in stmts {
            if let Statement::Fun(func) = stmt {
                let name = func.name(db).text(db);
                name_to_stmt.insert(name, *func);
            }
        }

        // Convert Vec<(String, FunctionAnalysisData)> back to HashMap<StmtFun, FunctionAnalysis>.
        for (name, data) in analyses {
            if let Some(&func) = name_to_stmt.get(name.as_str()) {
                let analysis = FunctionAnalysis {
                    errors: Vec::new(), // Errors already extracted at analysis boundary.
                    schedule: data.schedule.clone(),
                    bindings: data.bindings.clone(),
                    tracking: data.tracking.clone(),
                    adapt_sites: data.adapt_sites.clone(),
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

/// Convert tycheck expression types to IR types.
fn convert_expr_types<'db>(
    db: &'db dyn salsa::Database,
    types: &datalove_datafun_tycheck::ExprTypes<'db>,
) -> datalove_datafun_sema::ExprIrTypes<'db> {
    types.iter()
        .map(|(key, ty)| (*key, IrType::from_tycheck(db, ty)))
        .collect()
}

/// Analyze ownership for a script fragment unit.
///
/// Memoized: if typecheck_result, statements, and auto_adapt_mode match a previous call,
/// returns the cached result.
#[salsa::tracked(returns(copy))]
pub fn analyze_script_fragment_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    statements: Vec<Statement<'db>>,
    auto_adapt_mode: AutoAdaptMode,
    dead_externals: Vec<String>,
) -> ScriptUnitOwnershipResult<'db> {
    // Convert tycheck types to IR types.
    let expr_types = convert_expr_types(db, typecheck_result.expr_types(db));

    // Build map of function name -> resolved param types for type alias support.
    let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
    for (name, func_type) in typecheck_result.function_types(db) {
        let param_types: Vec<IrType> = func_type.param_types(db)
            .iter()
            .map(|ty| IrType::from_tycheck(db, ty))
            .collect();
        func_param_types.insert(name.text(db).S(), param_types);
    }

    // Analyze functions in this unit for ownership.
    let func_analyses_result = ownership_analysis::analyze_script_functions_with_mode(
        db, &expr_types, &statements, Some(&func_param_types), auto_adapt_mode
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
            return ScriptUnitOwnershipResult::new(
                db,
                Vec::new(),
                None,
                error_msgs,
                structured_errors,
                Vec::new(),
                Vec::new(),
            );
        }
    };

    // Analyze script-level statements for drop schedule.
    let script_analysis = ownership_analysis::analyze_script_statements_with_mode(
        db, &expr_types, &statements, auto_adapt_mode, dead_externals
    );

    // Check for script analysis errors.
    if !script_analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&script_analysis.errors);
        let structured_errors = script_analysis.errors.clone();
        return ScriptUnitOwnershipResult::new(
            db,
            Vec::new(),
            None,
            vec![error_msg],
            structured_errors,
            Vec::new(),
            Vec::new(),
        );
    }

    // Convert function analyses to hashable format.
    let func_analyses_data: Vec<(String, FunctionAnalysisData)> = func_analyses
        .into_iter()
        .map(|(stmt_fun, analysis)| {
            let name = stmt_fun.name(db).text(db).S();
            (name, FunctionAnalysisData::from(analysis))
        })
        .collect();

    let dead_exports = script_analysis.dead_exports.clone();
    let revived_exports = script_analysis.revived_exports.clone();
    let script_data = ScriptAnalysisData {
        schedule: script_analysis.schedule,
        bindings: script_analysis.bindings,
        tracking: script_analysis.tracking,
        unit_end: script_analysis.unit_end,
        adapt_sites: script_analysis.adapt_sites,
        dead_exports: script_analysis.dead_exports,
        revived_exports: script_analysis.revived_exports,
    };

    ScriptUnitOwnershipResult::new(
        db,
        func_analyses_data,
        Some(script_data),
        func_errors,
        structured_func_errors,
        dead_exports,
        revived_exports,
    )
}

/// Analyze ownership for a script expression unit.
///
/// Expression units have no functions but still need ownership analysis to detect
/// use-after-move errors within the expression. For example, `(x, x)` where x is non-Copy.
///
/// Memoized: if typecheck_result, expr, and auto_adapt_mode match a previous call,
/// returns the cached result.
#[salsa::tracked(returns(copy))]
pub fn analyze_script_expr_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    expr: datalove_datafun_ast::ast::ExprFun<'db>,
    auto_adapt_mode: AutoAdaptMode,
    dead_externals: Vec<String>,
) -> ScriptUnitOwnershipResult<'db> {
    // Convert tycheck types to IR types.
    let expr_types = convert_expr_types(db, typecheck_result.expr_types(db));

    let analysis = ownership_analysis::analyze_expr_with_mode(
        db, expr, &expr_types, auto_adapt_mode, dead_externals,
    );

    if !analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&analysis.errors);
        let structured_errors = analysis.errors.clone();
        return ScriptUnitOwnershipResult::new(
            db, Vec::new(), None, vec![error_msg], structured_errors,
            Vec::new(), Vec::new(),
        );
    }

    // Expression units have no functions and no script-level drop schedule.
    // The expression result is consumed by the caller, so no drops needed.
    ScriptUnitOwnershipResult::new(
        db, Vec::new(), None, Vec::new(), Vec::new(), Vec::new(), Vec::new(),
    )
}

// ============================================================================
// Ownership Diagnostic Emission
// ============================================================================

/// Emit ownership diagnostics for the given errors using the provided spans.
///
/// This should be called from the script compiler where spans are available.
pub fn emit_ownership_diagnostics<'db>(
    db: &'db dyn salsa::Database,
    errors: &[AnalysisError],
    spans: &DatafunSpans,
) {
    for error in errors {
        emit_single_ownership_diagnostic(db, error, spans);
    }
}

/// Emit a single ownership diagnostic.
fn emit_single_ownership_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    error: &AnalysisError,
    spans: &DatafunSpans,
) {
    match error {
        AnalysisError::UseAfterMoveInEarlierUnit { expr_key, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("`{}` was given away by an earlier input", name);
                let mut builder = bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D013")
                    .primary_label(ts, "value used after move");

                if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                    builder = builder.note(&format!("help: use `@` to {}", description));
                }

                builder.emit_ownership();
            }
        }
        AnalysisError::UseAfterMove { expr_key, moved_at: _, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("use of moved value: `{}`", name);
                let mut builder = bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D001")
                    .primary_label(ts, "value used after move");

                if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                    builder = builder.note(&format!("help: use `@` to {}", description));
                }

                builder.emit_ownership();
            }
        }
        AnalysisError::DoubleMove { expr_key, moved_at: _, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("value moved twice: `{}`", name);
                let mut builder = bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D002")
                    .primary_label(ts, "second move here");

                if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                    builder = builder.note(&format!("help: use `@` to {}", description));
                }

                builder.emit_ownership();
            }
        }
        AnalysisError::CannotMoveBorrowed { expr_key, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot move borrowed value: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D003")
                    .primary_label(ts, "cannot move borrowed value")
                    .note("borrowed parameters (ref, mut, out) cannot be moved")
                    .emit_ownership();
            }
        }
        AnalysisError::CannotMutFromRef { expr_key, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot get mutable reference from immutable: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D004")
                    .primary_label(ts, "ref parameter cannot be passed as mut")
                    .emit_ownership();
            }
        }
        AnalysisError::AliasedMutableArgument { expr_key, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("aliased mutable argument: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D010")
                    .primary_label(ts, "passed again in the same call")
                    .note("an argument passed as `mut` or `out` cannot also be passed to another parameter")
                    .emit_ownership();
            }
        }
        AnalysisError::CannotMutateImmutable { expr_key, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot pass immutable binding as mutable: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D011")
                    .primary_label(ts, "passed to a `mut` or `out` parameter")
                    .note(&format!("declare `{}` with `var` to allow mutation", name))
                    .emit_ownership();
            }
        }
        AnalysisError::CannotMutateTemporary { expr_key } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                bct::diagnostic::DiagnosticBuilder::error(db, "cannot pass a temporary as mutable")
                    .code("D012")
                    .primary_label(ts, "passed to a `mut` or `out` parameter")
                    .note("bind the value to a `var` first, so the mutation is observable")
                    .emit_ownership();
            }
        }
        AnalysisError::ReadUninitialized { expr_key, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("read of uninitialized binding: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D005")
                    .primary_label(ts, "used before initialization")
                    .emit_ownership();
            }
        }
        AnalysisError::OutParamNotInitialized { ret_stmt_idx: _, name } => {
            // For out param not initialized, we don't have a good span to use
            // since ret_stmt_idx is a statement index, not an expression index.
            // For now, emit a diagnostic without a primary span.
            let msg = format!("out parameter not initialized: `{}`", name);
            bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                .code("D006")
                .emit_ownership();
        }
        AnalysisError::MoveInLoop { expr_key, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot move `{}` in loop", name);
                let mut builder = bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D007")
                    .primary_label(ts, "value moved inside loop");

                if let OwnershipRecoveryHint::InsertAdapt { description } = recovery_hint {
                    builder = builder.note(&format!("help: use `@` to {}", description));
                }

                builder.emit_ownership();
            }
        }
        AnalysisError::InconsistentBranchMove {
            at, name, gave_away, fix_in, changed_at, moved_before, ..
        } => {
            let msg = datalove_datafun_sema::inconsistent_branch_message(error).expect("is D008");
            let mut builder = bct::diagnostic::DiagnosticBuilder::error(db, &msg).code("D008");

            if let Some(ts) = changed_at.and_then(|key| lookup_expr_span(db, spans, key)) {
                let label = match gave_away {
                    true => "given away here",
                    false => "given a new value here",
                };
                builder = builder.primary_label(ts, label);
            } else if let Some(ts) = lookup_expr_span(db, spans, *at) {
                builder = builder.primary_label(ts, "the branches of this disagree");
            }
            if let Some(ts) = moved_before.and_then(|key| lookup_expr_span(db, spans, key)) {
                let label = format!("`{}` given away here, before the branches", name);
                builder = builder.secondary_label(ts, &label);
            }

            let help = match gave_away {
                true => format!(
                    "give `{}` away {} too, or clone it with `@` where it is given away",
                    name, fix_in,
                ),
                false => format!("give `{}` a value {} too", name, fix_in),
            };
            builder
                .note(&format!(
                    "whether `{}` is held after the branches depends on which one ran, \
                     and it has to be dropped without knowing",
                    name,
                ))
                .help(&help)
                .emit_ownership();
        }
        AnalysisError::InconsistentLoopExit { stmt_idx: _, name } => {
            let msg = format!(
                "`{}` is given away on one way out of this loop and not another",
                name,
            );
            bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                .code("D014")
                .note(
                    "the move itself is fine -- it cannot happen twice. What cannot be \
                     settled is whether the binding is still held after the loop, which \
                     depends on which exit ran. Give it away on every exit, or clone it \
                     with `@` at the move.",
                )
                .emit_ownership();
        }
        AnalysisError::OutParamPartialWrite { expr_key, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *expr_key) {
                let msg = format!("cannot partially write to out parameter: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D009")
                    .primary_label(ts, "partial write to out parameter")
                    .note("out parameters must be written as a whole value")
                    .emit_ownership();
            }
        }
    }
}

/// Look up span for an expression by local_index.
fn lookup_expr_span<'db>(
    db: &'db dyn salsa::Database,
    spans: &DatafunSpans<'db>,
    expr_key: datalove_datafun_ast::ast::ExprKey<'db>,
) -> Option<TextSpan<'db>> {
    spans.lookup_key(expr_key).map(|entry| {
        let (text, span) = entry.to_text_and_span(db);
        TextSpan::new(text, span)
    })
}
