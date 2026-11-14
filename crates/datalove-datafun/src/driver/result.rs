//! Result types for script compilation and execution.
//!
//! These types represent the outcomes of compilation and execution phases:
//! - [`TypecheckResult`], [`OwnershipResult`], [`LoweringResult`]: Phase-specific status.
//! - [`ScriptCompilationResult`]: Output of script compilation (no execution).
//! - [`ScriptUnitResult`]: Combined compile+execute result for tests.

use rmx::prelude::*;
use bct::diagnostic::StoredDiagnostic;
use datalove_datafun_ir::IrCodeUnit;

/// How a phase failed: what it said, as diagnostics.
///
/// Serialized as each diagnostic's code, message and helps, which is what a
/// fixture compares; its spans name sources in a database, and are left out.
fn messages<S: serde::Serializer>(diagnostics: &[StoredDiagnostic], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(diagnostics.iter().map(diagnostic_summary))
}

/// The same, as one string.
fn message<S: serde::Serializer>(diagnostics: &[StoredDiagnostic], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&diagnostics.iter().map(diagnostic_summary).collect::<Vec<_>>().join("\n"))
}

/// A diagnostic as text without its spans: `error[F016]: ...`, then its helps.
pub fn diagnostic_summary(diagnostic: &StoredDiagnostic) -> String {
    let mut text = match &diagnostic.code {
        Some(code) => format!("error[{code}]: {}", diagnostic.message),
        None => format!("error: {}", diagnostic.message),
    };
    for help in &diagnostic.helps {
        text.push_str(&format!("\n  help: {help}"));
    }
    text
}

/// How a unit or module typechecked.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    ParseError {
        #[serde(rename = "errors", serialize_with = "messages")]
        diagnostics: Vec<StoredDiagnostic>,
    },
    Error {
        #[serde(rename = "errors", serialize_with = "messages")]
        diagnostics: Vec<StoredDiagnostic>,
    },
    Skipped,
}

/// How a unit or module came through ownership analysis.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum OwnershipResult {
    Success,
    Error {
        #[serde(rename = "message", serialize_with = "message")]
        diagnostics: Vec<StoredDiagnostic>,
    },
    Skipped,
}

/// How a unit or module lowered.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success { ir: String },
    Error {
        #[serde(rename = "message", serialize_with = "message")]
        diagnostics: Vec<StoredDiagnostic>,
    },
    Skipped,
}

impl TypecheckResult {
    /// A unit or module that did not parse, as `diagnostics` say.
    pub fn unparsed(diagnostics: Vec<StoredDiagnostic>) -> TypecheckResult {
        assert!(!diagnostics.is_empty(), "a parse that failed says why");
        TypecheckResult::ParseError { diagnostics }
    }

    /// A unit or module that did not typecheck, as `diagnostics` say.
    pub fn refused(diagnostics: Vec<StoredDiagnostic>) -> TypecheckResult {
        assert!(!diagnostics.is_empty(), "a typecheck that failed says why");
        TypecheckResult::Error { diagnostics }
    }
}

impl OwnershipResult {
    /// A unit or module ownership analysis refused, as `diagnostics` say.
    pub fn refused(diagnostics: Vec<StoredDiagnostic>) -> OwnershipResult {
        assert!(!diagnostics.is_empty(), "an ownership analysis that failed says why");
        OwnershipResult::Error { diagnostics }
    }
}

impl LoweringResult {
    /// A unit or module lowering refused, as `diagnostics` say.
    pub fn refused(diagnostics: Vec<StoredDiagnostic>) -> LoweringResult {
        assert!(!diagnostics.is_empty(), "a lowering that failed says why");
        LoweringResult::Error { diagnostics }
    }
}

/// Format ownership result for display.
pub fn format_ownership_result(
    ownership_errors: &[StoredDiagnostic],
    has_typecheck_errors: bool,
) -> OwnershipResult {
    if has_typecheck_errors {
        return OwnershipResult::Skipped;
    }

    if !ownership_errors.is_empty() {
        OwnershipResult::refused(ownership_errors.to_vec())
    } else {
        OwnershipResult::Success
    }
}

/// Format lowering result for display.
pub fn format_lowering_result(
    ir_dumps: &[String],
    lowering_errors: &[StoredDiagnostic],
    has_prior_errors: bool,
) -> LoweringResult {
    if has_prior_errors {
        return LoweringResult::Skipped;
    }

    if !lowering_errors.is_empty() {
        LoweringResult::refused(lowering_errors.to_vec())
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
    /// What the first phase that failed said, if one did.
    pub fn diagnostics(&self) -> Option<&[StoredDiagnostic]> {
        match (&self.typecheck, &self.ownership, &self.lowering) {
            (TypecheckResult::ParseError { diagnostics }, _, _)
            | (TypecheckResult::Error { diagnostics }, _, _)
            | (_, OwnershipResult::Error { diagnostics }, _)
            | (_, _, LoweringResult::Error { diagnostics }) => Some(diagnostics),
            _ => None,
        }
    }

    /// The first error this compilation ran into, in phase order, if any:
    /// what that phase said, each diagnostic's message and helps.
    pub fn first_error(&self) -> Option<String> {
        self.diagnostics().map(|diagnostics| {
            diagnostics.iter()
                .map(|diagnostic| {
                    let mut text = diagnostic.message.C();
                    for help in &diagnostic.helps {
                        text.push_str(&format!("\n  help: {help}"));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
    }
}
