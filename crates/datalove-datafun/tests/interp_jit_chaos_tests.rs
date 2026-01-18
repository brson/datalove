//! IR interpreter tests with chaos JIT dispatcher.
//!
//! Uses a randomized dispatcher that chaotically decides whether to JIT or
//! interpret each function call, exposing edge cases in mixed-mode execution.
//!
//! The random seed is derived from file contents for reproducibility.
//! Tests verify chaos JIT produces same results as pure interpreter.

use std::path::Path;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection, ParsedWorldfile};
use datalove_datafun_jit::{JitEngine, ChaosDispatcher};
use datafun::pipeline::ModuleCompilationPipeline;

/// Simplified result for comparison.
#[derive(Debug, Clone, PartialEq)]
struct UnitOutput {
    section_type: String,
    output: String,
    had_error: bool,
}

/// Run a worldfile with pure interpreter (no JIT).
fn run_with_interpreter(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
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

    // No call dispatcher - pure interpreter.
    let mut ctx = compiled.script_context(db, datalove_rt::c::DebugOutputMode::Buffer, None);

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {}
            WorldfileSection::ScriptFragment { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_fragment(source);
                results.push(UnitOutput {
                    section_type: "scriptunit-fragment".into(),
                    output: unit_result.output.clone(),
                    had_error: matches!(unit_result.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(unit_result.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
            WorldfileSection::ScriptExpr { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_expr(source);
                results.push(UnitOutput {
                    section_type: "scriptunit-expr".into(),
                    output: unit_result.output.clone(),
                    had_error: matches!(unit_result.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(unit_result.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
        }
    }

    ctx.destroy_all();
    results
}

/// Run a worldfile with JIT (threshold=1, compile immediately).
fn run_with_jit(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
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

    let jit = JitEngine::new(1).expect("JitEngine creation failed");
    let mut ctx = compiled.script_context(db, datalove_rt::c::DebugOutputMode::Buffer, Some(Box::new(jit)));

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {}
            WorldfileSection::ScriptFragment { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_fragment(source);
                results.push(UnitOutput {
                    section_type: "scriptunit-fragment".into(),
                    output: unit_result.output.clone(),
                    had_error: matches!(unit_result.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(unit_result.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
            WorldfileSection::ScriptExpr { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_expr(source);
                results.push(UnitOutput {
                    section_type: "scriptunit-expr".into(),
                    output: unit_result.output.clone(),
                    had_error: matches!(unit_result.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(unit_result.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
        }
    }

    ctx.destroy_all();
    results
}

/// Run a worldfile with chaos JIT.
fn run_with_chaos(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    seed: u64,
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

    let chaos = ChaosDispatcher::new(seed, 75, 50).expect("ChaosDispatcher creation failed");
    let mut ctx = compiled.script_context(db, datalove_rt::c::DebugOutputMode::Buffer, Some(Box::new(chaos)));

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {}
            WorldfileSection::ScriptFragment { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_fragment(source);
                results.push(UnitOutput {
                    section_type: "scriptunit-fragment".into(),
                    output: unit_result.output.clone(),
                    had_error: matches!(unit_result.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(unit_result.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
            WorldfileSection::ScriptExpr { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_expr(source);
                results.push(UnitOutput {
                    section_type: "scriptunit-expr".into(),
                    output: unit_result.output.clone(),
                    had_error: matches!(unit_result.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
                        || matches!(unit_result.lowering, datafun::pipeline::LoweringResult::Error { .. }),
                });
            }
        }
    }

    ctx.destroy_all();
    results
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
