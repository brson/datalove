//! Auto-adapt mode tests.
//!
//! Each test fixture is run in two modes:
//! 1. Normal mode (auto-adapt disabled) - should produce type errors with recovery hints
//! 2. Auto-adapt mode (enabled) - should succeed by automatically inserting @
//!
//! The combined output shows both modes, allowing verification that:
//! - Normal mode produces the expected type error with helpful recovery hints
//! - Auto-adapt mode successfully adapts the types

use rmx::prelude::*;
use rmx::serde_json::json;
use std::path::Path;

use datalove_datafun_tycheck::AutoAdaptMode;

/// Analyze a file in both modes and return combined output.
///
/// Runs analysis in:
/// 1. Normal mode (auto-adapt disabled) - should produce type errors for recoverable cases
/// 2. Auto-adapt mode (enabled) - should succeed by automatically inserting @
///
/// The combined output includes both results for comparison.
fn analyze_both_modes(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun_parser::parse_for_diagnostics(&db, source);
    let spans = datalove_datafun_parser::datafun_spans(&db, source);

    // Resolve names for the script.
    let name_resolution = datalove_datafun_resolve::resolve_script_names(&db, source, script.clone());

    // Helper to run analysis with a given mode.
    let run_analysis = |mode: AutoAdaptMode| {
        let unit_spec = datalove_datafun_tycheck::ScriptUnitSpec::new(
            source,
            spans.clone(),
            datalove_datafun_tycheck::ScriptUnitKind::Fragment(script.clone(), name_resolution.clone()),
        );
        let batch_spec = datalove_datafun_tycheck::create_batch_spec_with_auto_adapt(
            &db, source, vec![unit_spec], vec![], mode
        );
        let results = datalove_datafun_tycheck::type_check_script_units(&db, batch_spec);
        let tycheck_result = results.results(&db)[0];

        // Collect accumulated type diagnostics.
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

        json!({
            "success": !has_errors,
            "error_count": errors.len(),
            "diagnostic_count": diagnostics.len(),
            "diagnostics": diagnostics,
            "errors": errors,
        })
    };

    // Run in both modes.
    let normal_result = run_analysis(AutoAdaptMode::Disabled);
    let adapt_result = run_analysis(AutoAdaptMode::Enabled);

    let combined = json!({
        "normal_mode": normal_result,
        "auto_adapt_mode": adapt_result,
    });

    Ok(rmx::serde_json::to_string_pretty(&combined).X())
}

/// Run tests to verify recovery hints in diagnostics.
///
/// Uses the dual-mode analyzer which runs both normal and auto-adapt modes
/// and outputs combined results.
fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_both_modes)
        .fixture_subdir("auto-adapt")
        .file_extension("dfs")
        .run();
}
