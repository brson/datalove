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
use datalove_datafun_tycheck::{UnitTypecheckResultTracked, ResolvedCallTarget, Type};
use datalove_diagnostic::DiagnosticBuilderExt;
use salsa::plumbing::FromId;

use crate::IrTypeExt;
use crate::lower::ScriptFunctionAnalyses;
use datalove_datafun_ownership::{
    self as ownership_analysis, DropSchedule, BindingInfo, FunctionAnalysis,
    TrackingCategory, format_analysis_errors, CallInfo, AutoAdaptMode,
};
pub use datalove_datafun_sema::{ScriptAnalysisData, AnalysisError, OwnershipRecoveryHint};

// Re-export AutoAdaptMode for callers.
pub use datalove_datafun_ownership::AutoAdaptMode as OwnershipAutoAdaptMode;

/// Hashable wrapper for FunctionAnalysis.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::Update)]
pub struct FunctionAnalysisData {
    pub schedule: DropSchedule,
    pub bindings: Vec<BindingInfo>,
    pub tracking: Vec<TrackingCategory>,
}

impl From<FunctionAnalysis> for FunctionAnalysisData {
    fn from(analysis: FunctionAnalysis) -> Self {
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
    pub structured_errors: Vec<AnalysisError>,
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
    types: &[Option<Type<'db>>],
) -> Vec<Option<IrType>> {
    types.iter()
        .map(|opt| opt.as_ref().map(|ty| IrType::from_tycheck(db, ty)))
        .collect()
}

/// Convert resolved call targets to CallInfo.
fn convert_call_targets<'db>(
    db: &'db dyn salsa::Database,
    targets: &[Option<ResolvedCallTarget<'db>>],
) -> Vec<Option<CallInfo>> {
    targets.iter()
        .map(|opt| opt.as_ref().map(|target| CallInfo {
            param_modes: target.func(db).params(db)
                .iter()
                .map(|p| p.mode)
                .collect()
        }))
        .collect()
}

/// Analyze ownership for a script fragment unit.
///
/// Memoized: if typecheck_result, statements, and auto_adapt_mode match a previous call,
/// returns the cached result.
#[salsa::tracked]
pub fn analyze_script_fragment_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    statements: Vec<Statement<'db>>,
    auto_adapt_mode: AutoAdaptMode,
) -> ScriptUnitOwnershipResult<'db> {
    // Convert tycheck types to IR types.
    let expr_types = convert_expr_types(db, typecheck_result.expr_types(db));
    let call_info = convert_call_targets(db, typecheck_result.call_targets(db));

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
        db, &expr_types, &call_info, &statements, Some(&func_param_types), auto_adapt_mode
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
            );
        }
    };

    // Analyze script-level statements for drop schedule.
    let script_analysis = ownership_analysis::analyze_script_statements_with_mode(
        db, &expr_types, &call_info, &statements, auto_adapt_mode
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

    let script_data = ScriptAnalysisData {
        schedule: script_analysis.schedule,
        bindings: script_analysis.bindings,
        tracking: script_analysis.tracking,
        unit_end: script_analysis.unit_end,
    };

    ScriptUnitOwnershipResult::new(
        db,
        func_analyses_data,
        Some(script_data),
        func_errors,
        structured_func_errors,
    )
}

/// Analyze ownership for a script expression unit.
///
/// Expression units have no functions but still need ownership analysis to detect
/// use-after-move errors within the expression. For example, `(x, x)` where x is non-Copy.
///
/// Memoized: if typecheck_result, expr, and auto_adapt_mode match a previous call,
/// returns the cached result.
#[salsa::tracked]
pub fn analyze_script_expr_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    expr: datalove_datafun_ast::ast::ExprFun<'db>,
    auto_adapt_mode: AutoAdaptMode,
) -> ScriptUnitOwnershipResult<'db> {
    // Convert tycheck types to IR types.
    let expr_types = convert_expr_types(db, typecheck_result.expr_types(db));
    let call_info = convert_call_targets(db, typecheck_result.call_targets(db));

    let analysis = ownership_analysis::analyze_expr_with_mode(db, expr, &expr_types, &call_info, auto_adapt_mode);

    if !analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&analysis.errors);
        let structured_errors = analysis.errors.clone();
        return ScriptUnitOwnershipResult::new(db, Vec::new(), None, vec![error_msg], structured_errors);
    }

    // Expression units have no functions and no script-level drop schedule.
    // The expression result is consumed by the caller, so no drops needed.
    ScriptUnitOwnershipResult::new(db, Vec::new(), None, Vec::new(), Vec::new())
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
        AnalysisError::UseAfterMove { local_index, moved_at: _, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
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
        AnalysisError::DoubleMove { local_index, moved_at: _, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
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
        AnalysisError::CannotMoveBorrowed { local_index, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
                let msg = format!("cannot move borrowed value: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D003")
                    .primary_label(ts, "cannot move borrowed value")
                    .note("borrowed parameters (ref, mut, out) cannot be moved")
                    .emit_ownership();
            }
        }
        AnalysisError::CannotMutFromRef { local_index, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
                let msg = format!("cannot get mutable reference from immutable: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D004")
                    .primary_label(ts, "ref parameter cannot be passed as mut")
                    .emit_ownership();
            }
        }
        AnalysisError::AliasedMutableArgument { local_index, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
                let msg = format!("aliased mutable argument: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D010")
                    .primary_label(ts, "passed again in the same call")
                    .note("an argument passed as `mut` or `out` cannot also be passed to another parameter")
                    .emit_ownership();
            }
        }
        AnalysisError::CannotMutateImmutable { local_index, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
                let msg = format!("cannot pass immutable binding as mutable: `{}`", name);
                bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                    .code("D011")
                    .primary_label(ts, "passed to a `mut` or `out` parameter")
                    .note(&format!("declare `{}` with `var` to allow mutation", name))
                    .emit_ownership();
            }
        }
        AnalysisError::CannotMutateTemporary { local_index } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
                bct::diagnostic::DiagnosticBuilder::error(db, "cannot pass a temporary as mutable")
                    .code("D012")
                    .primary_label(ts, "passed to a `mut` or `out` parameter")
                    .note("bind the value to a `var` first, so the mutation is observable")
                    .emit_ownership();
            }
        }
        AnalysisError::ReadUninitialized { local_index, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
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
        AnalysisError::MoveInLoop { local_index, name, recovery_hint } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
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
        AnalysisError::InconsistentBranchMove { stmt_idx: _, name, moved_in } => {
            // Similar to OutParamNotInitialized, we don't have an expression span.
            let msg = format!("`{}` moved in {} branch but not the other", name, moved_in);
            bct::diagnostic::DiagnosticBuilder::error(db, &msg)
                .code("D008")
                .emit_ownership();
        }
        AnalysisError::OutParamPartialWrite { local_index, name } => {
            if let Some(ts) = lookup_expr_span(db, spans, *local_index) {
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
    spans: &DatafunSpans,
    local_index: u32,
) -> Option<TextSpan<'db>> {
    // The local_index is a salsa ID index for the expression.
    let id = unsafe { salsa::Id::from_index(local_index) };
    let expr = datalove_datafun_ast::ast::ExprFun::from_id(id);
    spans.lookup(expr).map(|entry| {
        let (text, span) = entry.to_text_and_span(db);
        TextSpan::new(text, span)
    })
}
