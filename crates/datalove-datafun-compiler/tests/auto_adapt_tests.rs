//! Auto-adapt mode tests.
//!
//! Each test fixture is run in two modes:
//! 1. Normal mode (auto-adapt disabled) - should produce type errors
//! 2. Auto-adapt mode - should succeed by automatically inserting @
//!
//! Expected output files:
//! - {name}.out.expected - normal mode output (errors)
//! - {name}.adapt.expected - auto-adapt mode output (success)

use rmx::prelude::*;
use rmx::serde_json::json;
use std::path::Path;

use datalove_datafun_tycheck::AutoAdaptMode;

/// Analyze a file in both modes and return combined output.
fn analyze_dual_mode(path: &Path) -> Result<String, String> {
    let normal = analyze_with_mode(path, AutoAdaptMode::Disabled)?;
    let adapt = analyze_with_mode(path, AutoAdaptMode::EnabledWithReport)?;

    let normal_json: rmx::serde_json::Value = rmx::serde_json::from_str(&normal).X();
    let adapt_json: rmx::serde_json::Value = rmx::serde_json::from_str(&adapt).X();

    let combined = json!({
        "normal_mode": normal_json,
        "auto_adapt_mode": adapt_json,
    });

    Ok(rmx::serde_json::to_string_pretty(&combined).X())
}

/// Analyze a file with the specified auto-adapt mode.
fn analyze_with_mode(path: &Path, auto_adapt_mode: AutoAdaptMode) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun_parser::parse_for_diagnostics(&db, source);
    let spans = datalove_datafun_parser::datafun_spans(&db, source);

    // Resolve names for the script.
    let name_resolution = datalove_datafun_resolve::resolve_script_names(&db, source, script.clone());

    // Create type context with the specified auto-adapt mode.
    let mut ctx = datalove_datafun_tycheck::TypeContext::with_options(
        &db,
        spans.clone(),
        None,
        auto_adapt_mode,
    );

    // Seed context from name resolution.
    ctx.seed_from_collected_names(&name_resolution, None);

    // Type check all statements.
    for statement in &script.statements {
        datalove_datafun_tycheck::statement::check_statement(&mut ctx, statement);
    }

    // Collect diagnostics.
    let type_diagnostics: Vec<_> = ctx.pending_diagnostics()
        .iter()
        .map(|d| format_pending_diagnostic(&db, d))
        .collect();

    // Collect auto-adaptations.
    let adaptations: Vec<_> = ctx.auto_adaptations()
        .iter()
        .map(|a| {
            json!({
                "expr_id": a.expr_id,
                "from": datalove_datafun_tycheck::type_to_string(&db, &a.from_type),
                "to": datalove_datafun_tycheck::type_to_string(&db, &a.to_type),
            })
        })
        .collect();

    let has_errors = !ctx.errors().is_empty() || !type_diagnostics.is_empty();

    let output = json!({
        "mode": if auto_adapt_mode.is_enabled() { "auto-adapt" } else { "normal" },
        "success": !has_errors,
        "error_count": ctx.errors().len(),
        "diagnostic_count": type_diagnostics.len(),
        "diagnostics": type_diagnostics,
        "adaptations": adaptations,
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

/// Format a pending diagnostic as JSON.
fn format_pending_diagnostic<'db>(
    db: &'db dyn salsa::Database,
    diag: &datalove_datafun_tycheck::PendingDiagnostic<'db>,
) -> rmx::serde_json::Value {
    use datalove_datafun_tycheck::PendingDiagnostic;
    use datalove_datafun_tycheck::RecoveryHint;

    match diag {
        PendingDiagnostic::TypeMismatch { expected, actual, recovery_hint, .. } => {
            let recoverable = matches!(recovery_hint, RecoveryHint::InsertAdapt { .. });
            let hint_desc = if let RecoveryHint::InsertAdapt { description } = recovery_hint {
                Some(description.clone())
            } else {
                None
            };
            json!({
                "code": "F016",
                "kind": "TypeMismatch",
                "expected": expected.as_str(db),
                "actual": actual.as_str(db),
                "recoverable": recoverable,
                "hint": hint_desc,
            })
        }
        PendingDiagnostic::UndefinedVariable { name, .. } => {
            json!({
                "code": "F001",
                "kind": "UndefinedVariable",
                "name": name.as_str(db),
            })
        }
        PendingDiagnostic::UndefinedFunction { name, .. } => {
            json!({
                "code": "F002",
                "kind": "UndefinedFunction",
                "name": name.as_str(db),
            })
        }
        PendingDiagnostic::CannotSynthesize { message, .. } => {
            json!({
                "code": "F011",
                "kind": "CannotSynthesize",
                "message": message.as_str(db),
            })
        }
        PendingDiagnostic::ArityMismatch { func_name, expected, actual, .. } => {
            json!({
                "code": "F045",
                "kind": "ArityMismatch",
                "function": func_name.as_str(db),
                "expected": expected,
                "actual": actual,
            })
        }
        PendingDiagnostic::InvalidOperandType { op, ty, .. } => {
            json!({
                "code": "F026",
                "kind": "InvalidOperandType",
                "operator": op.as_str(db),
                "type": ty.as_str(db),
            })
        }
        PendingDiagnostic::FunctionRequiresReturnValue { .. } => {
            json!({
                "code": "F053",
                "kind": "FunctionRequiresReturnValue",
            })
        }
        PendingDiagnostic::VoidFunctionReturnsValue { .. } => {
            json!({
                "code": "F052",
                "kind": "VoidFunctionReturnsValue",
            })
        }
        _ => {
            // Debug output to see what type we're missing
            json!({
                "kind": "Other",
            })
        }
    }
}

/// Run tests in both modes.
fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_dual_mode)
        .fixture_subdir("auto-adapt")
        .file_extension("dfs")
        .run();
}
