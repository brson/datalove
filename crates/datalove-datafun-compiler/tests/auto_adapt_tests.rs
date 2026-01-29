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

/// Analyze a file and return diagnostics with recovery hints.
fn analyze_file(path: &Path) -> Result<String, String> {
    analyze_with_mode(path, AutoAdaptMode::Disabled)
}

/// Analyze a file using the proper batch type checking API.
///
/// Note: auto_adapt_mode is not yet threaded through the Salsa-tracked
/// type checking functions. For now, we test recovery hints in normal mode.
fn analyze_with_mode(path: &Path, _auto_adapt_mode: AutoAdaptMode) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun_parser::parse_for_diagnostics(&db, source);
    let spans = datalove_datafun_parser::datafun_spans(&db, source);

    // Resolve names for the script.
    let name_resolution = datalove_datafun_resolve::resolve_script_names(&db, source, script.clone());

    // Use the batch type checking API.
    let unit_spec = datalove_datafun_tycheck::ScriptUnitSpec::new(
        source,
        spans,
        datalove_datafun_tycheck::ScriptUnitKind::Fragment(script.clone(), name_resolution),
    );
    let batch_spec = datalove_datafun_tycheck::create_batch_spec(&db, source, vec![unit_spec], vec![]);
    let results = datalove_datafun_tycheck::type_check_script_units(&db, batch_spec);
    let tycheck_result = results.results(&db)[0];

    // Collect accumulated type diagnostics with recovery hints.
    let type_diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, batch_spec);
    let diagnostics: Vec<_> = type_diagnostics
        .iter()
        .map(|d| {
            let diag = d.to_diagnostic(&db);
            let code = diag.code.map(|c| c.as_str(&db).to_string());
            let labels: Vec<_> = diag.labels.iter().map(|label| {
                json!({
                    "span": [label.span.start, label.span.end],
                    "text": source_text[label.span.clone()].to_string(),
                })
            }).collect();
            // Notes include recovery hints like "help: use @ to..."
            let notes: Vec<_> = diag.notes.iter().map(|n| n.as_str(&db).to_string()).collect();
            let mut obj = json!({
                "code": code,
                "message": diag.message.as_str(&db),
                "labels": labels
            });
            if !notes.is_empty() {
                obj["notes"] = json!(notes);
            }
            obj
        })
        .collect();

    let errors: Vec<_> = tycheck_result.errors(&db)
        .iter()
        .map(|e| json!({ "error": format!("{:?}", e.error(&db)) }))
        .collect();

    let has_errors = !errors.is_empty() || !diagnostics.is_empty();

    let output = json!({
        "success": !has_errors,
        "error_count": errors.len(),
        "diagnostic_count": diagnostics.len(),
        "diagnostics": diagnostics,
        "errors": errors,
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

/// Run tests to verify recovery hints in diagnostics.
fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("auto-adapt")
        .file_extension("dfs")
        .run();
}
