//! IR interpreter tests with dynamic inlining.
//!
//! Uses the DynamicInliner which tracks call site execution counts and performs
//! inlining when thresholds are reached. Tests verify that dynamic inlining
//! produces the same results as the baseline interpreter (no inlining).
//!
//! Unlike the static inline tests which use predetermined inlining schedules,
//! these tests let the dynamic inliner make decisions at runtime based on
//! actual call counts.

use std::path::Path;
use std::sync::Arc;

use datalove_datafun as datafun;
use datalove_datafun_interp::{CallDispatcher, DynamicInliner, DynamicInlinerConfig, InlinerStats};
use datalove_datafun_pkg::package_load_worldfile::{self, ParsedWorldfile, WorldfileSection};
use datafun::pipeline::{ConstInlining, ModuleCompilationPipeline};

/// Simplified result for comparison.
#[derive(Debug, Clone, PartialEq)]
struct UnitOutput {
    section_type: String,
    output: String,
    had_error: bool,
}

/// Run a worldfile with the interpreter (no inlining).
fn run_with_interpreter(db: &datafun::Database, parsed: &ParsedWorldfile) -> Vec<UnitOutput> {
    run_worldfile(db, parsed, None).0
}

/// Run a worldfile with dynamic inlining enabled.
/// Returns (results, stats).
fn run_with_dynamic_inline(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    threshold: u32,
) -> (Vec<UnitOutput>, Option<InlinerStats>) {
    let config = DynamicInlinerConfig { threshold };
    run_worldfile(db, parsed, Some(config))
}

/// Run a worldfile with optional dynamic inlining.
/// Returns (results, optional_stats).
fn run_worldfile(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    inliner_config: Option<DynamicInlinerConfig>,
) -> (Vec<UnitOutput>, Option<InlinerStats>) {
    let mut results = Vec::new();

    let mut pipeline =
        ModuleCompilationPipeline::from_sections(db, &parsed.sections, ConstInlining::Enabled);
    let compiled = pipeline.compile_fresh(db);

    if compiled.resolution_error.is_some() {
        results.push(UnitOutput {
            section_type: "resolution".into(),
            output: "error".into(),
            had_error: true,
        });
        return (results, None);
    }

    let Some(mut compiler) = compiled.script_compiler_default(db) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return (results, None);
    };

    // Create the dynamic inliner if configured.
    let dispatcher: Option<Box<dyn CallDispatcher>> =
        inliner_config.map(|cfg| Box::new(DynamicInliner::with_config(cfg)) as _);

    // Create executor with optional dynamic inliner as dispatcher.
    let Some(mut executor) = compiled.script_executor_with_module_registry(
        Arc::clone(&compiled.shared.module_registry),
        datalove_rt::c::DebugOutputMode::Buffer,
        dispatcher,
    ) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return (results, None);
    };

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. }
            | WorldfileSection::InlineDirectives { .. } => {}
            WorldfileSection::ScriptFragment { source } => {
                executor.clear_debug_buffer();
                let compiled_unit = compiler.compile_fragment(source);

                let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                    executor.execute_fragment(ir_unit)
                } else {
                    String::new()
                };
                results.push(UnitOutput {
                    section_type: "scriptunit-fragment".into(),
                    output,
                    had_error: matches!(
                        compiled_unit.typecheck,
                        datafun::pipeline::TypecheckResult::Error { .. }
                    ) || matches!(
                        compiled_unit.lowering,
                        datafun::pipeline::LoweringResult::Error { .. }
                    ),
                });
            }
            WorldfileSection::ScriptExpr { source } => {
                executor.clear_debug_buffer();
                let compiled_unit = compiler.compile_expr(source);

                let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                    let (_, value) = executor.execute_expr(ir_unit);
                    value
                } else {
                    String::new()
                };
                results.push(UnitOutput {
                    section_type: "scriptunit-expr".into(),
                    output,
                    had_error: matches!(
                        compiled_unit.typecheck,
                        datafun::pipeline::TypecheckResult::Error { .. }
                    ) || matches!(
                        compiled_unit.lowering,
                        datafun::pipeline::LoweringResult::Error { .. }
                    ),
                });
            }
        }
    }

    // Get stats from the dispatcher before destroying.
    let stats = executor.take_dispatcher().and_then(|d| {
        // Downcast to DynamicInliner to get stats.
        d.as_any().downcast_ref::<DynamicInliner>().map(|inliner| inliner.stats().clone())
    });

    executor.destroy_live_values();
    (results, stats)
}

/// Test a single worldfile with dynamic inlining at various thresholds.
fn test_file(path: &Path) -> Result<(u64, u32, u32), String> {
    let file_bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Run with interpreter (baseline).
    let interp_results = run_with_interpreter(&db, &parsed);

    // Accumulate stats across all runs.
    let mut total_calls: u64 = 0;
    let mut total_inlinings: u32 = 0;
    let mut total_skipped: u32 = 0;

    // Run with dynamic inlining at various thresholds.
    // Lower thresholds = more aggressive inlining.
    for threshold in [1, 5, 10, 50] {
        let (dynamic_results, stats) = run_with_dynamic_inline(&db, &parsed, threshold);

        if dynamic_results != interp_results {
            return Err(format!(
                "Dynamic inline (threshold={}) differs from interpreter!\n\
                 Interpreter: {:?}\n\
                 Dynamic: {:?}",
                threshold, interp_results, dynamic_results
            ));
        }

        if let Some(s) = stats {
            total_calls += s.calls_tracked;
            total_inlinings += s.inlinings_performed;
            total_skipped += s.inlinings_skipped;
        }
    }

    Ok((total_calls, total_inlinings, total_skipped))
}

fn main() {
    // Collect test files.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixture_dir = std::path::PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("interp");

    let mut pass_count = 0;
    let mut fail_count = 0;
    let mut errors = Vec::new();

    // Aggregate stats.
    let mut grand_total_calls: u64 = 0;
    let mut grand_total_inlinings: u32 = 0;
    let mut grand_total_skipped: u32 = 0;

    if !fixture_dir.exists() {
        eprintln!("Fixture directory not found: {:?}", fixture_dir);
        std::process::exit(1);
    }

    let mut entries: Vec<_> = std::fs::read_dir(&fixture_dir)
        .expect("Failed to read fixture directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "world"))
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy();

        match test_file(&path) {
            Ok((calls, inlinings, skipped)) => {
                println!("\x1b[32m  PASS \x1b[0m {}", name);
                pass_count += 1;
                grand_total_calls += calls;
                grand_total_inlinings += inlinings;
                grand_total_skipped += skipped;
            }
            Err(e) => {
                println!("\x1b[31m  FAIL \x1b[0m {}", name);
                println!("    {}", e.replace('\n', "\n    "));
                fail_count += 1;
                errors.push((name.to_string(), e));
            }
        }
    }

    println!();
    println!(
        "Dynamic Inline Tests: {} passed, {} failed",
        pass_count, fail_count
    );
    println!(
        "Inliner Stats: {} calls tracked, {} inlinings performed, {} skipped",
        grand_total_calls, grand_total_inlinings, grand_total_skipped
    );

    if grand_total_calls == 0 {
        eprintln!("\x1b[31mWARNING: No calls were tracked! Dynamic inlining may not be working.\x1b[0m");
    }

    if fail_count > 0 {
        std::process::exit(1);
    }
}
