//! Salsa-tracked API for script unit ownership analysis.
//!
//! Provides memoized, per-unit ownership analysis following the same pattern as
//! module ownership analysis. Each unit is analyzed independently, enabling
//! incremental recompilation when new units are added to a batch.

use rmx::prelude::*;
use std::collections::HashMap;
use datalove_datafun_ast::ast::{Statement, StmtFun};
use datalove_datafun_ir::IrType;
use datalove_datafun_tycheck::{UnitTypecheckResultTracked, ResolvedCallTarget, Type};

use crate::ir_ext::IrTypeExt;
use datalove_datafun_ownership::{
    self as ownership_analysis, DropSchedule, BindingInfo, BindingId, FunctionAnalysis,
    TrackingCategory, ScriptFunctionAnalyses, format_analysis_errors, CallInfo,
};

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

/// Hashable wrapper for ScriptAnalysis.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(salsa::Update)]
pub struct ScriptAnalysisData {
    pub schedule: DropSchedule,
    pub bindings: Vec<BindingInfo>,
    pub tracking: Vec<TrackingCategory>,
    pub unit_end: Vec<BindingId>,
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
    /// Formatted error messages.
    #[returns(ref)]
    pub errors: Vec<String>,
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
/// Memoized: if typecheck_result and statements match a previous call,
/// returns the cached result.
#[salsa::tracked]
pub fn analyze_script_fragment_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    statements: Vec<Statement<'db>>,
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
    let func_analyses_result = ownership_analysis::analyze_script_functions(
        db, &expr_types, &call_info, &statements, Some(&func_param_types)
    );

    let (func_analyses, func_errors) = match func_analyses_result {
        Ok(analyses) => (analyses, Vec::new()),
        Err(errors) => {
            let error_msgs: Vec<String> = errors.into_iter()
                .map(|(func_name, errs)| {
                    format!("{}: {}", func_name, format_analysis_errors(&errs))
                })
                .collect();
            return ScriptUnitOwnershipResult::new(
                db,
                Vec::new(),
                None,
                error_msgs,
            );
        }
    };

    // Analyze script-level statements for drop schedule.
    let script_analysis = ownership_analysis::analyze_script_statements(
        db, &expr_types, &call_info, &statements
    );

    // Check for script analysis errors.
    if !script_analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&script_analysis.errors);
        return ScriptUnitOwnershipResult::new(
            db,
            Vec::new(),
            None,
            vec![error_msg],
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
    )
}

/// Analyze ownership for a script expression unit.
///
/// Expression units have no functions but still need ownership analysis to detect
/// use-after-move errors within the expression. For example, `(x, x)` where x is non-Copy.
///
/// Memoized: if typecheck_result and expr match a previous call, returns the cached result.
#[salsa::tracked]
pub fn analyze_script_expr_tracked<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: UnitTypecheckResultTracked<'db>,
    expr: datalove_datafun_ast::ast::ExprFun<'db>,
) -> ScriptUnitOwnershipResult<'db> {
    // Convert tycheck types to IR types.
    let expr_types = convert_expr_types(db, typecheck_result.expr_types(db));
    let call_info = convert_call_targets(db, typecheck_result.call_targets(db));

    let analysis = ownership_analysis::analyze_expr(db, expr, &expr_types, &call_info);

    if !analysis.errors.is_empty() {
        let error_msg = format_analysis_errors(&analysis.errors);
        return ScriptUnitOwnershipResult::new(db, Vec::new(), None, vec![error_msg]);
    }

    // Expression units have no functions and no script-level drop schedule.
    // The expression result is consumed by the caller, so no drops needed.
    ScriptUnitOwnershipResult::new(db, Vec::new(), None, Vec::new())
}
