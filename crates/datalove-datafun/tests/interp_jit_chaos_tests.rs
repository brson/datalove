//! IR interpreter tests with chaos JIT dispatcher.
//!
//! Uses a randomized dispatcher that chaotically decides whether to JIT or
//! interpret each function call, exposing edge cases in mixed-mode execution.
//!
//! The random seed is derived from file contents for reproducibility.
//! Tests verify chaos JIT produces same results as pure interpreter.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::rc::Rc;

use datalove_datafun as datafun;
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection, ParsedWorldfile};
use datalove_datafun_cranelift_jit::{JitEngine, ChaosDispatcher};
use datalove_datafun_interp::CallDispatcher;
use datafun::pipeline::ModuleCompilationPipeline;

/// Simplified result for comparison.
#[derive(Debug, Clone, PartialEq)]
struct UnitOutput {
    section_type: String,
    output: String,
    had_error: bool,
}

/// Run a worldfile with optional call dispatcher (interpreter, JIT, or chaos).
fn run_worldfile(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    call_dispatcher: Option<Box<dyn CallDispatcher>>,
) -> Vec<UnitOutput> {
    let mut results = Vec::new();

    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections);
    let compiled = pipeline.compile_fresh(db);

    if compiled.resolution_error.is_some() {
        results.push(UnitOutput {
            section_type: "resolution".into(),
            output: "error".into(),
            had_error: true,
        });
        return results;
    }

    let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
    let Some(mut compiler) = compiled.script_compiler(db, evaluator) else {
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
            | WorldfileSection::ModuleChangeTy { .. } => {}
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

    executor.destroy_all();
    results
}

/// Run a worldfile with pure interpreter (no JIT).
fn run_with_interpreter(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
) -> Vec<UnitOutput> {
    run_worldfile(db, parsed, None)
}

/// Run a worldfile with JIT (threshold=1, compile immediately).
fn run_with_jit(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
) -> Vec<UnitOutput> {
    let jit = JitEngine::new(1).expect("JitEngine creation failed");
    run_worldfile(db, parsed, Some(Box::new(jit)))
}

/// Run a worldfile with chaos JIT.
fn run_with_chaos(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    seed: u64,
) -> Vec<UnitOutput> {
    let chaos = ChaosDispatcher::new(seed, 75, 50).expect("ChaosDispatcher creation failed");
    run_worldfile(db, parsed, Some(Box::new(chaos)))
}

/// Compute a hash of the file contents for reproducible randomness.
fn compute_seed(file_bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    file_bytes.hash(&mut hasher);
    hasher.finish()
}

/// Test a single worldfile with multiple modes.
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

        // Run with regular JIT.
        let jit_results = run_with_jit(&db, &parsed);

        // Run with chaos JIT (multiple iterations with different seeds).
        for iter in 0..5 {
            let chaos_seed = seed.wrapping_add(iter);
            let chaos_results = run_with_chaos(&db, &parsed, chaos_seed);

            // Compare chaos results with interpreter.
            if chaos_results != interp_results {
                return Err(format!(
                    "Chaos JIT (seed={}) differs from interpreter!\n\
                     Interpreter: {:?}\n\
                     Chaos: {:?}",
                    chaos_seed, interp_results, chaos_results
                ));
            }
        }

        // Also verify JIT matches interpreter.
        if jit_results != interp_results {
            return Err(format!(
                "Regular JIT differs from interpreter!\n\
                 Interpreter: {:?}\n\
                 JIT: {:?}",
                interp_results, jit_results
            ));
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

    for entry in entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy();

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

    println!();
    println!("Chaos JIT Tests: {} passed, {} failed", pass_count, fail_count);

    if fail_count > 0 {
        std::process::exit(1);
    }
}
