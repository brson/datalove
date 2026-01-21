//! Result types for script compilation and execution.

use datalove_datafun_ir::IrScriptUnit;

/// Typecheck result summary (serializable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    ParseError { errors: Vec<String> },
    Error { errors: Vec<String> },
    Skipped,
}

/// Lowering result summary (serializable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success { ir: String },
    Error { message: String },
    Skipped,
}

/// Format lowering result for display.
pub fn format_lowering_result(
    ir_dumps: &[String],
    ownership_errors: &[String],
    lowering_errors: &[String],
    has_typecheck_errors: bool,
) -> LoweringResult {
    if has_typecheck_errors {
        return LoweringResult::Skipped;
    }

    let all_errors: Vec<_> = ownership_errors.iter()
        .chain(lowering_errors.iter())
        .cloned()
        .collect();

    if !all_errors.is_empty() {
        LoweringResult::Error { message: all_errors.join("\n") }
    } else if ir_dumps.is_empty() {
        LoweringResult::Skipped
    } else {
        LoweringResult::Success { ir: ir_dumps.join("\n") }
    }
}

/// Result of `eval_fragment` or `eval_expr`.
pub struct ScriptUnitResult {
    pub typecheck: TypecheckResult,
    pub lowering: LoweringResult,
    /// Type of the result expression, if any.
    pub ty: Option<String>,
    /// Pretty-printed output value or execution error.
    pub output: String,
}

/// Result of `lower_fragment_for_aot` or `lower_expr_for_aot` (IR without execution).
pub struct ScriptLowerResult {
    pub typecheck: TypecheckResult,
    pub lowering: LoweringResult,
    /// The lowered IR unit, if successful.
    pub ir_unit: Option<IrScriptUnit>,
}

/// Internal result of compilation phases (no execution).
pub(super) struct ScriptCompilationResult {
    pub(super) typecheck: TypecheckResult,
    pub(super) lowering: LoweringResult,
    pub(super) ir_unit: Option<IrScriptUnit>,
}
