//! Worldgen dual interpreter/AOT comparison tests.
//!
//! This test generates random worldfiles and verifies that interpreter (with chaos JIT)
//! and AOT compilation produce identical results.
//!
//! Seed can be overridden via WORLDGEN_DUAL_SEED environment variable for reproducibility.
//!
//! Known issues:
//! - Some seeds cause AOT "Verifier errors" (pre-existing compiler bug)
//! - Some seeds cause stack overflow in interpreter (deep recursion)
//!
//! Run manually with: WORLDGEN_DUAL_TEST=1 cargo test -p datalove-tests --test worldgen_dual_tests
//! To reproduce a failure: WORLDGEN_DUAL_TEST=1 WORLDGEN_DUAL_SEED=<seed> cargo test -p datalove-tests --test worldgen_dual_tests

use std::cell::RefCell;
use std::process::Command;
use std::path::Path;
use std::rc::Rc;

use rand::Rng;

use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_cranelift_jit::ChaosDispatcher;
use datalove_datafun_cranelift_aot::AotCompiler;
use datalove_datafun_ir::FunctionRegistry;
use datalove_datafun_interp::InterpCtfeEvaluator;
use datafun::pipeline::ModuleCompilationPipeline;

/// Number of worldfiles to generate and test per run.
const ITERATION_COUNT: u64 = 20;

/// Result of running a worldfile.
#[derive(Debug, Clone)]
struct RunResult {
    /// Whether compilation succeeded.
    compiled: bool,
    /// Debuglog output.
    debuglog: String,
    /// Termination status (true = success).
    success: bool,
    /// Error message if any.
    error: Option<String>,
}

/// Get the test seed from environment or generate random.
fn get_seed() -> u64 {
    if let Ok(seed_str) = std::env::var("WORLDGEN_DUAL_SEED") {
        seed_str.parse().expect("WORLDGEN_DUAL_SEED must be a valid u64")
    } else {
        rand::thread_rng().r#gen()
    }
}

/// Run worldfile with interpreter using chaos JIT dispatcher.
fn run_with_chaos_interp(
    db: &datafun::Database,
    parsed: &package_load_worldfile::ParsedWorldfile,
    seed: u64,
) -> RunResult {
    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections);
    let compiled = pipeline.compile_fresh(db);

    if let Some(err) = &compiled.resolution_error {
        return RunResult {
            compiled: false,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Resolution error: {}", err)),
        };
    }

    // Check for typecheck errors.
    for (path, errors) in &compiled.path_to_errors {
        if !errors.is_empty() {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Typecheck error in {}: {:?}", path, errors)),
            };
        }
    }

    // Check for lowering errors.
    for (path, errors) in &compiled.lowering_errors {
        if !errors.is_empty() {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Lowering error in {}: {:?}", path, errors)),
            };
        }
    }

    // Create chaos dispatcher: 75% JIT probability, compile after 1 call.
    let chaos = match ChaosDispatcher::new(seed, 75, 1) {
        Ok(c) => c,
        Err(e) => {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Failed to create ChaosDispatcher: {}", e)),
            };
        }
    };

    let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
    let Some(mut compiler) = compiled.script_compiler(db, evaluator) else {
        return RunResult {
            compiled: true,
            debuglog: String::new(),
            success: false,
            error: Some("Module compilation failed".to_string()),
        };
    };

    let Some(mut executor) = compiled.script_executor(
        datalove_rt::c::DebugOutputMode::Buffer,
        Some(Box::new(chaos)),
    ) else {
        return RunResult {
            compiled: true,
            debuglog: String::new(),
            success: false,
            error: Some("Module compilation failed".to_string()),
        };
    };

    let mut debuglog = String::new();
    let success = true;
    let error = None;

    for section in &parsed.sections {
        if let WorldfileSection::ScriptFragment { source } = section {
            executor.clear_debug_buffer();
            let compiled_unit = compiler.compile_fragment(source);
            if let Some(ir_unit) = &compiled_unit.ir_unit {
                executor.execute_fragment(ir_unit);
            }

            debuglog.push_str(&executor.get_debug_buffer());

            if matches!(compiled_unit.typecheck, datafun::pipeline::TypecheckResult::Error { .. }) {
                return RunResult {
                    compiled: false,
                    debuglog,
                    success: false,
                    error: Some(format!("Script typecheck error: {:?}", compiled_unit.typecheck)),
                };
            }
            if matches!(compiled_unit.lowering, datafun::pipeline::LoweringResult::Error { .. }) {
                return RunResult {
                    compiled: false,
                    debuglog,
                    success: false,
                    error: Some(format!("Script lowering error: {:?}", compiled_unit.lowering)),
                };
            }
        }
    }

    executor.destroy_all();

    RunResult {
        compiled: true,
        debuglog,
        success,
        error,
    }
}

/// Build the runtime library once and return the path to the lib directory.
fn ensure_runtime_lib() -> &'static Path {
    datafun::pipeline::aot::ensure_runtime_lib()
}

/// Run worldfile with AOT compilation.
fn run_with_aot(
    db: &datafun::Database,
    parsed: &package_load_worldfile::ParsedWorldfile,
) -> RunResult {
    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections);
    let compiled = pipeline.compile_fresh(db);

    if let Some(err) = &compiled.resolution_error {
        return RunResult {
            compiled: false,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Resolution error: {}", err)),
        };
    }

    // Check for typecheck errors.
    for (path, errors) in &compiled.path_to_errors {
        if !errors.is_empty() {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Typecheck error in {}: {:?}", path, errors)),
            };
        }
    }

    // Check for lowering errors.
    for (path, errors) in &compiled.lowering_errors {
        if !errors.is_empty() {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Lowering error in {}: {:?}", path, errors)),
            };
        }
    }

    // Find the script fragment.
    let fragment_source = match parsed.sections.iter().find_map(|s| {
        if let WorldfileSection::ScriptFragment { source } = s {
            Some(source.as_str())
        } else {
            None
        }
    }) {
        Some(s) => s,
        None => {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some("No script fragment found".to_string()),
            };
        }
    };

    // Compile for AOT.
    let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
    let Some(mut compiler) = compiled.script_compiler(db, evaluator) else {
        return RunResult {
            compiled: true,
            debuglog: String::new(),
            success: false,
            error: Some("Module compilation failed".to_string()),
        };
    };
    let compiled_unit = compiler.compile_fragment(fragment_source);

    if !matches!(compiled_unit.typecheck, datafun::pipeline::TypecheckResult::Success) {
        return RunResult {
            compiled: false,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Script typecheck error: {:?}", compiled_unit.typecheck)),
        };
    }

    if !matches!(compiled_unit.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        return RunResult {
            compiled: false,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Script lowering error: {:?}", compiled_unit.lowering)),
        };
    }

    let ir_unit = match compiled_unit.ir_unit {
        Some(unit) => unit,
        None => {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some("No IR unit produced".to_string()),
            };
        }
    };

    // AOT compile.
    let registry = compiled.module_registry();
    aot_compile_link_run(&ir_unit, &registry)
}

/// AOT compile, link, and run an IR unit.
fn aot_compile_link_run(
    ir_unit: &datalove_datafun_ir::IrScriptUnit,
    registry: &FunctionRegistry,
) -> RunResult {
    let mut compiler = match AotCompiler::new_for_host() {
        Ok(c) => c,
        Err(e) => {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Failed to create AOT compiler: {}", e)),
            };
        }
    };

    let product = match compiler.compile_script_unit_with_world_types(
        ir_unit,
        registry.iter_all_functions(),
        registry,
    ) {
        Ok(p) => p,
        Err(e) => {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("AOT compile error: {}", e)),
            };
        }
    };

    let obj_bytes = match product.emit() {
        Ok(b) => b,
        Err(e) => {
            return RunResult {
                compiled: false,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Failed to emit object: {}", e)),
            };
        }
    };

    // Write object to temp file.
    let dir = match rmx::tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return RunResult {
                compiled: true,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Failed to create temp dir: {}", e)),
            };
        }
    };

    let obj_path = dir.path().join("test.o");
    if let Err(e) = std::fs::write(&obj_path, &obj_bytes) {
        return RunResult {
            compiled: true,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Failed to write object file: {}", e)),
        };
    }

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    if !lib_path.exists() {
        return RunResult {
            compiled: true,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Runtime library not found at {:?}", lib_path)),
        };
    }

    // Link with cc.
    let exe_path = dir.path().join("test");
    let link_output = Command::new("cc")
        .args([
            obj_path.to_str().unwrap(),
            lib_path.to_str().unwrap(),
            "-ldl", "-lpthread", "-lm",
            "-o", exe_path.to_str().unwrap(),
        ])
        .output();

    let link_output = match link_output {
        Ok(o) => o,
        Err(e) => {
            return RunResult {
                compiled: true,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Failed to run linker: {}", e)),
            };
        }
    };

    if !link_output.status.success() {
        let stderr = String::from_utf8_lossy(&link_output.stderr);
        return RunResult {
            compiled: true,
            debuglog: String::new(),
            success: false,
            error: Some(format!("Linker failed: {}", stderr)),
        };
    }

    // Run the executable.
    let run_output = match Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            return RunResult {
                compiled: true,
                debuglog: String::new(),
                success: false,
                error: Some(format!("Failed to run executable: {}", e)),
            };
        }
    };

    let debuglog = String::from_utf8_lossy(&run_output.stderr).to_string();

    if !run_output.status.success() {
        let exit_code = run_output.status.code().unwrap_or(-1);
        return RunResult {
            compiled: true,
            debuglog,
            success: false,
            error: Some(format!("Execution failed with exit code: {}", exit_code)),
        };
    }

    RunResult {
        compiled: true,
        debuglog,
        success: true,
        error: None,
    }
}

/// Test a single worldfile seed.
fn test_worldfile(base_seed: u64, index: u64) -> Result<(), String> {
    let seed = base_seed.wrapping_add(index);
    let config = WorldGenConfig::default();

    // Generate worldfile.
    let worldfile = gen_worldfile_seeded(seed, config);

    // Parse.
    let parsed = package_load_worldfile::parse_worldfile_sections(worldfile.as_bytes())
        .map_err(|e| format!("Parse error: {}", e))?;

    // Run in spawned thread to work around Cranelift JIT + PIE issues.
    // Use large stack to avoid stack overflow on deeply nested expressions.
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)  // 32MB stack
        .spawn(move || {
        let db = datafun::Database::default();

        // Run with chaos interpreter.
        let interp_result = run_with_chaos_interp(&db, &parsed, seed);

        // Run with AOT.
        let aot_result = run_with_aot(&db, &parsed);

        (interp_result, aot_result)
    }).expect("failed to spawn thread").join().expect("test thread panicked");

    let (interp_result, aot_result) = result;

    // Compare compilation success.
    if interp_result.compiled != aot_result.compiled {
        return Err(format!(
            "Compilation mismatch: interp={}, aot={}\n  Interp error: {:?}\n  AOT error: {:?}",
            interp_result.compiled, aot_result.compiled,
            interp_result.error, aot_result.error
        ));
    }

    // If neither compiled, that's consistent (likely a worldgen bug to fix separately).
    if !interp_result.compiled {
        return Err(format!(
            "Compilation failed:\n  Interp: {:?}\n  AOT: {:?}",
            interp_result.error, aot_result.error
        ));
    }

    // Compare execution success.
    if interp_result.success != aot_result.success {
        return Err(format!(
            "Execution success mismatch: interp={}, aot={}\n  Interp error: {:?}\n  AOT error: {:?}",
            interp_result.success, aot_result.success,
            interp_result.error, aot_result.error
        ));
    }

    // If both failed, that's consistent.
    if !interp_result.success {
        return Err(format!(
            "Execution failed consistently:\n  Interp: {:?}\n  AOT: {:?}",
            interp_result.error, aot_result.error
        ));
    }

    // Compare debuglog output.
    if interp_result.debuglog != aot_result.debuglog {
        return Err(format!(
            "Debuglog mismatch:\n  Interp: {:?}\n  AOT: {:?}",
            interp_result.debuglog, aot_result.debuglog
        ));
    }

    Ok(())
}

fn main() {
    // Skip unless explicitly enabled (can crash due to stack overflow on some seeds).
    if std::env::var("WORLDGEN_DUAL_TEST").is_err() {
        println!("Skipping worldgen dual tests (set WORLDGEN_DUAL_TEST=1 to run)");
        return;
    }

    let base_seed = get_seed();
    println!("Worldgen dual test seed: {}", base_seed);
    println!("(Set WORLDGEN_DUAL_SEED={} to reproduce)", base_seed);
    println!();

    let mut pass_count = 0;
    let mut fail_count = 0;
    let mut errors = Vec::new();

    for i in 0..ITERATION_COUNT {
        let seed = base_seed.wrapping_add(i);
        // Print seed before running so we can see which seed hangs.
        print!("Testing seed {}... ", seed);
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let result = test_worldfile(base_seed, i);

        match result {
            Ok(()) => {
                println!("\x1b[32mPASS\x1b[0m");
                pass_count += 1;
            }
            Err(e) => {
                println!("\x1b[31mFAIL\x1b[0m");
                println!("  {}", e.replace('\n', "\n  "));
                fail_count += 1;
                errors.push((seed, e));
            }
        }
    }

    println!();
    println!("Worldgen dual tests: {} passed, {} failed", pass_count, fail_count);

    if fail_count > 0 {
        println!();
        println!("Failed seeds:");
        for (seed, _) in &errors {
            println!("  WORLDGEN_DUAL_SEED={}", seed);
        }
        std::process::exit(1);
    }
}
