//! IR interpreter tests with chaos inlining.
//!
//! Uses a randomized inliner that chaotically decides which functions to inline
//! based on an input-derived seed. Tests verify that inlining produces the same
//! results as the baseline interpreter (no inlining).
//!
//! The random seed is derived from file contents for reproducibility.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use datalove_datafun as datafun;
use datalove_datafun_inline::{inline_module, InlineDirective};
use datalove_datafun_ir::{IrModule, IrScriptUnit, SymbolTable};
use datalove_datafun_pkg::package_load_worldfile::{self, ParsedWorldfile, WorldfileSection};
use datafun::pipeline::{ConstInlining, ModuleCompilationPipeline};

/// Simplified result for comparison.
#[derive(Debug, Clone, PartialEq)]
struct UnitOutput {
    section_type: String,
    output: String,
    had_error: bool,
}

/// Simple xorshift64 PRNG for reproducible random decisions.
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_bool(&mut self, probability_percent: u32) -> bool {
        (self.next_u64() % 100) < probability_percent as u64
    }
}

/// Generate random inline directives based on available functions.
fn generate_random_directives(
    module: &IrModule,
    rng: &mut SimpleRng,
    inline_probability: u32,
) -> Vec<InlineDirective> {
    let mut directives = Vec::new();

    // Collect function names.
    let func_names: Vec<&str> = module
        .symbols
        .functions
        .iter()
        .map(|def| def.name.as_str())
        .collect();

    if func_names.len() < 2 {
        return directives;
    }

    // For each pair of functions, randomly decide to inline.
    for caller in &func_names {
        for callee in &func_names {
            // Skip self-calls (recursive inlining not supported).
            if caller == callee {
                continue;
            }

            if rng.next_bool(inline_probability) {
                directives.push(InlineDirective::Inline {
                    caller: (*caller).to_string(),
                    callee: (*callee).to_string(),
                });
            }
        }
    }

    directives
}

/// Apply inlining to functions in a script unit.
fn inline_script_unit(unit: &mut IrScriptUnit, rng: &mut SimpleRng, inline_probability: u32) {
    if unit.functions.is_empty() {
        return;
    }

    // Build temporary IrModule from unit functions.
    let mut symbols = SymbolTable::new();
    for func in &unit.functions {
        symbols.define_func_with_id(func.id, func.name.clone(), func.params.len());
    }

    let temp_module = IrModule {
        functions: unit.functions.clone(),
        symbols,
    };

    // Generate random directives.
    let directives = generate_random_directives(&temp_module, rng, inline_probability);

    if directives.is_empty() {
        return;
    }

    // Apply inlining.
    let result = inline_module(&temp_module, &directives);

    // Replace functions in the unit.
    unit.functions = result.module.functions;
}

/// Run a worldfile with optional inlining.
fn run_worldfile(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    inline_seed: Option<u64>,
    inline_probability: u32,
) -> Vec<UnitOutput> {
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

    let Some(mut executor) =
        compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, None)
    else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return results;
    };

    // Create RNG for script unit inlining.
    let mut script_rng = inline_seed.map(|s| SimpleRng::new(s));

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
                let mut compiled_unit = compiler.compile_fragment(source);

                // Apply chaos inlining to script unit functions.
                if let (Some(ir_unit), Some(rng)) =
                    (&mut compiled_unit.ir_unit, &mut script_rng)
                {
                    inline_script_unit(ir_unit, rng, inline_probability);
                }

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
                let mut compiled_unit = compiler.compile_expr(source);

                // Apply chaos inlining to script unit functions.
                if let (Some(ir_unit), Some(rng)) =
                    (&mut compiled_unit.ir_unit, &mut script_rng)
                {
                    inline_script_unit(ir_unit, rng, inline_probability);
                }

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

    executor.destroy_live_values();
    results
}

/// Run a worldfile with pure interpreter (no inlining).
fn run_with_interpreter(db: &datafun::Database, parsed: &ParsedWorldfile) -> Vec<UnitOutput> {
    run_worldfile(db, parsed, None, 0)
}

/// Run a worldfile with chaos inlining.
fn run_with_chaos_inline(
    db: &datafun::Database,
    parsed: &ParsedWorldfile,
    seed: u64,
    probability: u32,
) -> Vec<UnitOutput> {
    run_worldfile(db, parsed, Some(seed), probability)
}

/// Compute a hash of the file contents for reproducible randomness.
fn compute_seed(file_bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    file_bytes.hash(&mut hasher);
    hasher.finish()
}

/// Test a single worldfile with multiple chaos inlining configurations.
fn test_file(path: &Path) -> Result<(), String> {
    let file_bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let seed = compute_seed(&file_bytes);

    let db = datafun::Database::default();

    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Run with interpreter (baseline).
    let interp_results = run_with_interpreter(&db, &parsed);

    // Run with chaos inlining (multiple iterations with different seeds and probabilities).
    for iter in 0..3 {
        let chaos_seed = seed.wrapping_add(iter);

        // Try different inline probabilities.
        for probability in [25, 50, 75] {
            let chaos_results = run_with_chaos_inline(&db, &parsed, chaos_seed, probability);

            // Compare chaos results with interpreter.
            if chaos_results != interp_results {
                return Err(format!(
                    "Chaos inline (seed={}, prob={}%) differs from interpreter!\n\
                     Interpreter: {:?}\n\
                     Chaos: {:?}",
                    chaos_seed, probability, interp_results, chaos_results
                ));
            }
        }
    }

    Ok(())
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
    println!(
        "Chaos Inline Tests: {} passed, {} failed",
        pass_count, fail_count
    );

    if fail_count > 0 {
        std::process::exit(1);
    }
}
