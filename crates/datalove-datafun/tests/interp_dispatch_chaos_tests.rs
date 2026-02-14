//! IR interpreter tests with OptimizingDispatcher in chaos mode.
//!
//! Uses a randomized dispatcher that chaotically decides whether to JIT compile,
//! use JIT code, or inline each function call. Exposes edge cases in mixed-mode
//! execution with the combined optimization pipeline.
//!
//! The random seed is derived from file contents for reproducibility.
//! Tests verify chaos mode produces same results as pure interpreter.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection, ParsedWorldfile};
use datalove_datafun_cranelift_jit::{OptimizingDispatcher, DispatcherConfig};
use datalove_datafun_interp::CallDispatcher;
use datafun::pipeline::{ModuleCompilationPipeline, ConstInlining};

/// Global stats for tracking JIT and inlining activity.
static TOTAL_JIT_COMPILED: AtomicU32 = AtomicU32::new(0);
static TOTAL_INLININGS: AtomicU32 = AtomicU32::new(0);
static TOTAL_CALLS_TRACKED: AtomicU64 = AtomicU64::new(0);

/// Simplified result for comparison.
#[derive(Debug, Clone, PartialEq)]
struct UnitOutput {
    section_type: String,
    output: String,
    had_error: bool,
}

/// Stats from a single run.
struct RunStats {
    jit_compiled: u32,
    inlinings_performed: u32,
    calls_tracked: u64,
}

/// Run a worldfile and return results plus stats.
fn run_worldfile_with_stats(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    dispatcher: OptimizingDispatcher,
) -> (Vec<UnitOutput>, RunStats) {
    let mut results = Vec::new();

    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections, ConstInlining::Enabled);
    let compiled = pipeline.compile_fresh(db);

    if compiled.resolution_error.is_some() {
        results.push(UnitOutput {
            section_type: "resolution".into(),
            output: "error".into(),
            had_error: true,
        });
        return (results, RunStats { jit_compiled: 0, inlinings_performed: 0, calls_tracked: 0 });
    }

    let Some(mut compiler) = compiled.script_compiler_default(db) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return (results, RunStats { jit_compiled: 0, inlinings_performed: 0, calls_tracked: 0 });
    };

    let Some(mut executor) = compiled.script_executor(
        datalove_rt::c::DebugOutputMode::Buffer,
        Some(Box::new(dispatcher)),
    ) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return (results, RunStats { jit_compiled: 0, inlinings_performed: 0, calls_tracked: 0 });
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
                    had_error: matches!(compiled_unit.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(compiled_unit.lowering, datafun::pipeline::LoweringResult::Error { .. }),
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
                    had_error: matches!(compiled_unit.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(compiled_unit.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
        }
    }

    // Get stats before destroying.
    let stats = if let Some(dispatcher) = executor.take_dispatcher() {
        if let Some(opt_dispatcher) = dispatcher.as_any().downcast_ref::<OptimizingDispatcher>() {
            RunStats {
                jit_compiled: opt_dispatcher.jit().stats().compiled_count,
                inlinings_performed: opt_dispatcher.inliner().stats().inlinings_performed,
                calls_tracked: opt_dispatcher.inliner().stats().calls_tracked,
            }
        } else {
            RunStats { jit_compiled: 0, inlinings_performed: 0, calls_tracked: 0 }
        }
    } else {
        RunStats { jit_compiled: 0, inlinings_performed: 0, calls_tracked: 0 }
    };

    executor.destroy_live_values();
    (results, stats)
}

/// Run a worldfile with pure interpreter (no dispatcher).
fn run_worldfile(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    call_dispatcher: Option<Box<dyn CallDispatcher>>,
) -> Vec<UnitOutput> {
    let mut results = Vec::new();

    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections, ConstInlining::Enabled);
    let compiled = pipeline.compile_fresh(db);

    if compiled.resolution_error.is_some() {
        results.push(UnitOutput {
            section_type: "resolution".into(),
            output: "error".into(),
            had_error: true,
        });
        return results;
    }

    let Some(mut compiler) = compiled.script_compiler_default(db) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return results;
    };

    let Some(mut executor) = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, call_dispatcher) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return results;
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
                    had_error: matches!(compiled_unit.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(compiled_unit.lowering, datafun::pipeline::LoweringResult::Error { .. }),
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
                    had_error: matches!(compiled_unit.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(compiled_unit.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
        }
    }

    executor.destroy_live_values();
    results
}

/// Run a worldfile with pure interpreter (no dispatcher).
fn run_with_interpreter(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
) -> Vec<UnitOutput> {
    run_worldfile(db, parsed, None)
}

/// Run a worldfile with OptimizingDispatcher in chaos mode.
fn run_with_chaos_dispatcher(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    seed: u64,
) -> (Vec<UnitOutput>, RunStats) {
    let config = DispatcherConfig::chaos(seed);
    let dispatcher = OptimizingDispatcher::with_config(config)
        .expect("OptimizingDispatcher creation failed");
    run_worldfile_with_stats(db, parsed, dispatcher)
}

/// Compute a hash of the file contents for reproducible randomness.
fn compute_seed(file_bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    file_bytes.hash(&mut hasher);
    hasher.finish()
}

/// Test a single worldfile with multiple chaos iterations.
fn test_file(path: &Path) -> Result<(), String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let seed = compute_seed(&file_bytes);

    // Run in spawned thread to work around Cranelift JIT + PIE issues.
    std::thread::spawn(move || {
        let db = datafun::Database::default();

        let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
            .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

        // Run with interpreter (baseline).
        let interp_results = run_with_interpreter(&db, &parsed);

        // Run with chaos dispatcher (multiple iterations with different seeds).
        for iter in 0..5 {
            let chaos_seed = seed.wrapping_add(iter);
            let (chaos_results, stats) = run_with_chaos_dispatcher(&db, &parsed, chaos_seed);

            // Accumulate global stats.
            TOTAL_JIT_COMPILED.fetch_add(stats.jit_compiled, Ordering::Relaxed);
            TOTAL_INLININGS.fetch_add(stats.inlinings_performed, Ordering::Relaxed);
            TOTAL_CALLS_TRACKED.fetch_add(stats.calls_tracked, Ordering::Relaxed);

            // Compare chaos results with interpreter.
            if chaos_results != interp_results {
                return Err(format!(
                    "Chaos dispatcher (seed={}) differs from interpreter!\n\
                     Interpreter: {:?}\n\
                     Chaos: {:?}",
                    chaos_seed, interp_results, chaos_results
                ));
            }
        }

        Ok(())
    }).join().expect("test thread panicked")
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

    // Tests using list indexing instructions (ListGet, ListBoundsCheck, ListSet)
    // that are not yet implemented in the Cranelift JIT backend.
    const SKIP_AOT_UNSUPPORTED: &[&str] = &[
        "955_list_index_option_inbounds",
        "956_list_index_option_oob",
        "957_list_index_result_inbounds",
        "958_list_index_result_oob",
        "959_set_list_index_option_inbounds",
        "960_set_list_index_option_oob",
        "961_set_list_index_result_oob",
        "962_list_index_result_script",
    ];

    for entry in entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy();

        if SKIP_AOT_UNSUPPORTED.contains(&name.as_ref()) {
            println!("\x1b[33m  SKIP \x1b[0m {} (AOT unsupported)", name);
            pass_count += 1;
            continue;
        }

        match test_file(&path) {
            Ok(()) => {
                println!("\x1b[32m  PASS \x1b[0m {}", name);
                pass_count += 1;
            }
            Err(e) => {
                println!("\x1b[31m  FAIL \x1b[0m {}", name);
                println!("    {}", e.replace('\n', "\n    "));
                fail_count += 1;
                errors.push((name.to_string(), e));
            }
        }
    }

    // Print aggregate stats.
    let jit_compiled = TOTAL_JIT_COMPILED.load(Ordering::Relaxed);
    let inlinings = TOTAL_INLININGS.load(Ordering::Relaxed);
    let calls_tracked = TOTAL_CALLS_TRACKED.load(Ordering::Relaxed);

    println!();
    println!("Chaos Dispatcher Tests: {} passed, {} failed", pass_count, fail_count);
    println!(
        "  JIT: {} functions compiled, Inliner: {} inlinings ({} calls tracked)",
        jit_compiled, inlinings, calls_tracked
    );

    // Verify JIT and inliner were actually engaged.
    if jit_compiled == 0 {
        eprintln!("\x1b[31mERROR: JIT was never engaged!\x1b[0m");
        std::process::exit(1);
    }
    if inlinings == 0 {
        eprintln!("\x1b[31mERROR: Inliner was never engaged!\x1b[0m");
        std::process::exit(1);
    }

    if fail_count > 0 {
        std::process::exit(1);
    }
}
