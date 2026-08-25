//! Auto-adapt mode tests using worldfiles.
//!
//! Each test fixture is a worldfile that is run in two modes:
//! 1. Normal mode (auto-adapt disabled) - should produce type/ownership errors with recovery hints
//! 2. Auto-adapt mode (enabled) - should succeed by automatically inserting @
//!
//! Worldfiles can contain:
//! - Module sections: `module library/package/module` - tested via module graph pipeline
//! - Script sections: `scriptunit-fragment` - tested via script unit pipeline
//!
//! Both type errors and ownership errors are tested:
//! - Type errors: F016 type mismatch (e.g., u8→i16 cross-sign widening)
//! - Ownership errors: D001 UseAfterMove, D002 DoubleMove, D007 MoveInLoop

use rmx::prelude::*;
use rmx::serde_json::json;
use std::path::Path;

use datalove_datafun::{
    Database, package, package_resolve,
    to_module_graph, module_graph,
};
use datalove_datafun_tycheck::{typecheck_module_graph, AutoAdaptMode};
use datalove_datafun_resolve::{resolve_all_names, resolve_all_exports, build_all_function_ast_maps, ParallelMode};
use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, parse_worldfile_sections};
use datalove_datafun::pipeline::{WorkspaceDescriptor, CompilerOptions};
use datalove_datafun_compiler::tracked_ownership_analysis::analyze_module_graph_with_mode;
use datalove_datafun_compiler::tracked_script_ownership::analyze_script_fragment_tracked;

/// Typecheck worldfile sections with a given auto-adapt mode.
///
/// Handles both module sections (via module graph pipeline) and script sections
/// (via script unit pipeline).
fn typecheck_sections_with_mode(
    sections: &[WorldfileSection],
    mode: AutoAdaptMode,
) -> (Vec<String>, Vec<rmx::serde_json::Value>) {
    let db = Database::default();
    let mut all_errors = Vec::new();
    let mut all_diagnostics = Vec::new();

    // Collect module sections and build package world.
    let mut has_modules = false;
    let mut pkglib_system = std::collections::BTreeMap::new();
    let mut pkglib_local = std::collections::BTreeMap::new();

    for section in sections {
        if let WorldfileSection::Module { library, package: pkg, module, source } = section {
            has_modules = true;
            let library_map = match library.as_str() {
                "sys" => &mut pkglib_system,
                "local" => &mut pkglib_local,
                other => {
                    all_errors.push(format!("unknown library '{}'", other));
                    continue;
                }
            };

            let package = library_map.entry(pkg.C())
                .or_insert_with(|| datalove_datafun_pkg::package_load::Package {
                    name: pkg.C(),
                    modules: std::collections::BTreeMap::new(),
                    rider_source: None,
                    rider_crate_dir: None,
                });

            let module_path_str = format!("{}/{}/{}", library, pkg, module);
            let pkg_module = datalove_datafun_pkg::package_load::PackageModule {
                name: module.C(),
                path: module_path_str.C().into(),
                text: source.clone(),
            };

            package.modules.insert(module.C(), pkg_module);
        }
    }

    // Typecheck modules if we have any.
    if has_modules {
        let package_world = datalove_datafun_pkg::package_load::PackageWorld {
            pkglib_system,
            pkglib_local,
        };
        let package_world = package::import_from_loader(&db, package_world);

        let resolution = package_resolve::resolve_package_world_with_imports(&db, package_world);
        let result = resolution.result(&db);

        if let Err(e) = result {
            all_errors.push(format!("Resolution error: {:?}", e));
        } else {
            let pkg_graph = result.ok().unwrap();
            let graph_with_requires = to_module_graph(&db, package_world, pkg_graph);
            let parsed_graph = module_graph::parse_module_graph(&db, graph_with_requires.graph, graph_with_requires.resolved_requires, Vec::new());
            let all_names = resolve_all_names(&db, parsed_graph);
            let all_exports = resolve_all_exports(&db, parsed_graph);
            let all_function_asts = build_all_function_ast_maps(&db, parsed_graph);
            let typecheck_result = typecheck_module_graph(&db, parsed_graph, all_names, all_exports, all_function_asts, mode);

            // Collect module errors.
            let module_errors = typecheck_result.module_errors(&db);
            for (module_id, errs) in module_errors.iter() {
                for err in errs {
                    all_errors.push(format!("{}: {:?}", module_id.path(&db), err));
                }
            }

            // Collect module diagnostics.
            let type_diagnostics = typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(
                &db, parsed_graph, all_names, all_exports, all_function_asts, mode
            );
            for d in type_diagnostics.iter() {
                let diag = d.to_diagnostic(&db);
                let code = diag.code.map(|c| c.as_str(&db).to_string());
                let labels: Vec<_> = diag.labels.iter().map(|label| {
                    json!({ "span": [label.span.start, label.span.end] })
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
                all_diagnostics.push(obj);
            }

            // Run ownership analysis for modules if no type errors.
            if module_errors.values().all(|e| e.is_empty()) {
                let ownership_result = analyze_module_graph_with_mode(&db, parsed_graph, typecheck_result, ParallelMode::Sequential, mode);
                for (module_id, analysis) in ownership_result.module_results(&db).iter() {
                    for err in analysis.errors(&db) {
                        all_errors.push(format!("{}: {}", module_id.path(&db), err));
                    }
                }
            }
        }
    }

    // Typecheck script sections.
    for section in sections {
        if let WorldfileSection::ScriptFragment { source } = section {
            let db = datalove_datafun_compiler::Database::default();
            let source_input = bct::input::Source::new(&db, source.S());
            let script = datalove_datafun_parser::parse_for_diagnostics(&db, source_input);
            let spans = datalove_datafun_parser::datafun_spans(&db, source_input);
            let name_resolution = datalove_datafun_resolve::resolve_script_names(&db, source_input, script.clone());

            let unit_spec = datalove_datafun_tycheck::ScriptUnitSpec::new(
                source_input,
                spans,
                datalove_datafun_tycheck::ScriptUnitKind::Fragment(script.clone(), name_resolution.clone()),
            );
            let batch_spec = datalove_datafun_tycheck::create_batch_spec_with_auto_adapt(
                &db, source_input, vec![unit_spec], vec![], mode
            );
            let results = datalove_datafun_tycheck::type_check_script_units(&db, batch_spec);
            let tycheck_result = results.results(&db)[0];

            // Collect script type diagnostics.
            let type_diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(&db, batch_spec);
            for d in type_diagnostics.iter() {
                let diag = d.to_diagnostic(&db);
                let code = diag.code.map(|c| c.as_str(&db).to_string());
                let labels: Vec<_> = diag.labels.iter().map(|label| {
                    json!({
                        "span": [label.span.start, label.span.end],
                        "text": source[label.span.clone()].to_string(),
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
                all_diagnostics.push(obj);
            }

            // Collect script type errors.
            for e in tycheck_result.errors(&db).iter() {
                all_errors.push(format!("{:?}", e.error(&db)));
            }

            // Run ownership analysis if no type errors (ownership requires successful typecheck).
            if tycheck_result.errors(&db).is_empty() {
                // Use tracked ownership analysis function (same as ScriptCompiler uses).
                let stmts: Vec<_> = script.statements.iter().cloned().collect();
                // Each fixture is a single unit, so nothing arrives dead.
                let ownership_result = analyze_script_fragment_tracked(
                    &db, tycheck_result, stmts, mode, Vec::new(),
                );
                for err in ownership_result.errors(&db) {
                    all_errors.push(err.clone());
                }
            }
        }
    }

    (all_errors, all_diagnostics)
}

/// Describe why a unit produced no IR.
fn compile_failure(unit: &datalove_datafun::pipeline::ScriptCompilationResult) -> String {
    use datalove_datafun::pipeline::{TypecheckResult, OwnershipResult, LoweringResult};

    match &unit.typecheck {
        TypecheckResult::ParseError { errors } => return format!("parse: {}", errors.join("; ")),
        TypecheckResult::Error { errors } => return format!("typecheck: {}", errors.join("; ")),
        TypecheckResult::Success | TypecheckResult::Skipped => {}
    }
    if let OwnershipResult::Error { message } = &unit.ownership {
        return format!("ownership: {}", message);
    }
    if let LoweringResult::Error { message } = &unit.lowering {
        return format!("lowering: {}", message);
    }
    "no ir and no error".S()
}

/// Run a worldfile's script sections and report what they computed.
///
/// Auto-adapt is meant to insert the `@` the programmer omitted, so checking
/// that the errors went away says nothing on its own: the adapted program has
/// to produce the values it would have produced with `@` written by hand.
/// Fixtures with no script section have nothing to run.
fn execute_sections_with_mode(
    sections: &[WorldfileSection],
    mode: AutoAdaptMode,
) -> rmx::serde_json::Value {
    let db = Database::default();
    let descriptor = WorkspaceDescriptor::from_worldfile_sections(sections, CompilerOptions::default());
    let mut pipeline = descriptor.to_pipeline(&db);
    let compiled = pipeline.compile_fresh(&db);

    if compiled.has_errors() {
        return json!({ "status": "modules did not compile" });
    }

    let (Some(mut compiler), Some(mut executor)) = (
        compiled.script_compiler_default(&db),
        compiled.script_executor(datalove_datafun::DebugOutputMode::Disabled, None),
    ) else {
        return json!({ "status": "modules did not compile" });
    };
    compiler.set_auto_adapt_mode(mode);

    let mut outputs = Vec::new();
    for section in sections {
        let (kind, compiled_unit) = match section {
            WorldfileSection::ScriptFragment { source } => {
                ("fragment", compiler.compile_fragment(source))
            }
            WorldfileSection::ScriptExpr { source } => {
                ("expr", compiler.compile_expr(source))
            }
            _ => continue,
        };

        let output = match &compiled_unit.ir_unit {
            Some(ir_unit) if kind == "expr" => executor.execute_expr(ir_unit).1,
            Some(ir_unit) => executor.execute_fragment(ir_unit),
            // Auto-adapt can leave a unit that passed analysis with no IR,
            // so say which phase stopped it.
            None => format!("did not compile: {}", compile_failure(&compiled_unit)),
        };
        outputs.push(json!({ "kind": kind, "output": output }));
    }

    let environment: Vec<_> = executor.get_environment()
        .into_iter()
        .map(|(name, kind, ty, value)| json!({
            "name": name,
            "kind": kind,
            "ty": ty,
            "value": value,
        }))
        .collect();

    executor.destroy_live_values();

    json!({
        "status": if outputs.is_empty() { "nothing to run" } else { "ran" },
        "outputs": outputs,
        "environment": environment,
    })
}

/// Analyze a worldfile in both modes and return combined output.
fn analyze_both_modes(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path).X();

    // Parse worldfile into sections.
    let parsed = parse_worldfile_sections(source.as_bytes())
        .map_err(|e| format!("Parse error: {}", e))?;

    // Run in normal mode (auto-adapt disabled).
    let (normal_errors, normal_diagnostics) = typecheck_sections_with_mode(&parsed.sections, AutoAdaptMode::Disabled);
    let normal_has_errors = !normal_errors.is_empty() || !normal_diagnostics.is_empty();
    let normal_result = json!({
        "success": !normal_has_errors,
        "error_count": normal_errors.len(),
        "diagnostic_count": normal_diagnostics.len(),
        "diagnostics": normal_diagnostics,
        "errors": normal_errors.iter().map(|e| json!({ "error": e })).collect::<Vec<_>>(),
    });

    // Run in auto-adapt mode (enabled).
    let (adapt_errors, adapt_diagnostics) = typecheck_sections_with_mode(&parsed.sections, AutoAdaptMode::Enabled);
    let adapt_has_errors = !adapt_errors.is_empty() || !adapt_diagnostics.is_empty();
    let adapt_result = json!({
        "success": !adapt_has_errors,
        "error_count": adapt_errors.len(),
        "diagnostic_count": adapt_diagnostics.len(),
        "diagnostics": adapt_diagnostics,
        "errors": adapt_errors.iter().map(|e| json!({ "error": e })).collect::<Vec<_>>(),
    });

    // Run what auto-adapt accepted, to see whether it computes the right thing.
    let adapt_execution = execute_sections_with_mode(&parsed.sections, AutoAdaptMode::Enabled);

    let combined = json!({
        "normal_mode": normal_result,
        "auto_adapt_mode": adapt_result,
        "auto_adapt_execution": adapt_execution,
    });

    Ok(rmx::serde_json::to_string_pretty(&combined).X())
}

/// Run tests to verify recovery hints in diagnostics.
///
/// Uses worldfiles and the full compilation pipeline to test auto-adapt mode
/// in both script and module contexts.
fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_both_modes)
        .fixture_subdir("auto-adapt")
        .file_extension("wf")
        .run();
}
