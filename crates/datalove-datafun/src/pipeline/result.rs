//! Result types for script compilation and execution.
//!
//! These types represent the outcomes of compilation and execution phases:
//! - [`TypecheckResult`], [`OwnershipResult`], [`LoweringResult`]: Phase-specific status.
//! - [`ScriptCompilationResult`]: Output of script compilation (no execution).
//! - [`ScriptUnitResult`]: Combined compile+execute result for tests.

use datalove_datafun_ir::IrCodeUnit;

/// Typecheck result summary (serializable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    ParseError { errors: Vec<String> },
    Error { errors: Vec<String> },
    Skipped,
}

/// Ownership analysis result summary (serializable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
pub enum OwnershipResult {
    Success,
    Error { message: String },
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

/// Format ownership result for display.
pub fn format_ownership_result(
    ownership_errors: &[String],
    has_typecheck_errors: bool,
) -> OwnershipResult {
    if has_typecheck_errors {
        return OwnershipResult::Skipped;
    }

    if !ownership_errors.is_empty() {
        OwnershipResult::Error { message: ownership_errors.join("\n") }
    } else {
        OwnershipResult::Success
    }
}

/// Format lowering result for display.
pub fn format_lowering_result(
    ir_dumps: &[String],
    lowering_errors: &[String],
    has_prior_errors: bool,
) -> LoweringResult {
    if has_prior_errors {
        return LoweringResult::Skipped;
    }

    if !lowering_errors.is_empty() {
        LoweringResult::Error { message: lowering_errors.join("\n") }
    } else if ir_dumps.is_empty() {
        LoweringResult::Skipped
    } else {
        LoweringResult::Success { ir: ir_dumps.join("\n") }
    }
}

/// Combined result of script compilation and execution.
///
/// Used primarily in tests to capture both compilation status and execution output.
pub struct ScriptUnitResult {
    pub typecheck: TypecheckResult,
    pub ownership: OwnershipResult,
    pub lowering: LoweringResult,
    /// Type of the result expression, if any.
    pub ty: Option<String>,
    /// Pretty-printed output value or execution error.
    pub output: String,
}

/// Result of script compilation (no execution).
///
/// Returned by [`ScriptCompiler::compile_fragment`](super::ScriptCompiler::compile_fragment)
/// and [`ScriptCompiler::compile_expr`](super::ScriptCompiler::compile_expr).
pub struct ScriptCompilationResult {
    pub typecheck: TypecheckResult,
    pub ownership: OwnershipResult,
    pub lowering: LoweringResult,
    /// The lowered IR code unit, if compilation succeeded.
    pub ir_unit: Option<IrCodeUnit>,
}

impl ScriptCompilationResult {
    /// The first error this compilation ran into, in phase order, if any.
    pub fn first_error(&self) -> Option<String> {
        match (&self.typecheck, &self.ownership, &self.lowering) {
            (TypecheckResult::ParseError { errors }, _, _) => Some(errors.join("; ")),
            (TypecheckResult::Error { errors }, _, _) => Some(errors.join("; ")),
            (_, OwnershipResult::Error { message }, _) => Some(message.clone()),
            (_, _, LoweringResult::Error { message }) => Some(message.clone()),
            _ => None,
        }
    }
}
