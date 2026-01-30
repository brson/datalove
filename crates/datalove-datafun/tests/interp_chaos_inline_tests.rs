//! IR interpreter tests with chaos inlining.
//!
//! Uses a randomized inliner that chaotically decides which functions to inline
//! based on an input-derived seed. Tests verify that inlining produces the same
//! results as the baseline interpreter (no inlining).
//!
//! The random seed is derived from file contents for reproducibility.
//!
//! Tests both:
//! - Script unit function inlining (functions defined in script fragments)
//! - Module function inlining (functions defined in module sections)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use datalove_datafun as datafun;
use datalove_datafun_inline::{
    inline_cross_module, inline_module, CrossModuleInlineContext, GlobalFuncId, InlineDirective,
};
use datalove_datafun_ir::{IrModule, IrModuleId, IrScriptUnit, ModuleFunctionRegistry, SymbolTable};
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
        // Ensure non-zero state for xorshift.
        Self { state: if seed == 0 { 1 } else { seed } }
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

    // Collect all function names.
    let func_names: Vec<&str> = module
        .functions
        .iter()
        .map(|f| f.name.as_str())
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

/// Build an IrModule from functions belonging to a single module in the registry.
fn build_ir_module_for_single_module(
    registry: &ModuleFunctionRegistry,
    target_module_id: IrModuleId,
) -> IrModule {
    let mut functions = Vec::new();

    for ((module_id, _func_id), func) in registry.iter_module_functions_with_ids() {
        if module_id == target_module_id {
            functions.push(func.clone());
        }
    }

    // Sort functions by name for deterministic output.
    functions.sort_by(|a, b| a.name.cmp(&b.name));

    let mut symbols = SymbolTable::new();
    for func in &functions {
        symbols.define_func_with_id(func.id, func.name.clone(), func.params.len());
    }

    IrModule { functions, symbols }
}

/// Collect all unique module IDs in the registry.
fn collect_module_ids(registry: &ModuleFunctionRegistry) -> Vec<IrModuleId> {
    let mut ids: Vec<IrModuleId> = registry
        .iter_module_functions_with_ids()
        .map(|((module_id, _), _)| module_id)
        .collect();
    ids.sort_by_key(|id| id.0);
    ids.dedup();
    ids
}

/// Apply chaos inlining to module functions.
///
/// Inlines within each module separately (not across modules) to preserve FuncId consistency.
fn inline_module_functions(
    registry: &ModuleFunctionRegistry,
    rng: &mut SimpleRng,
    inline_probability: u32,
) -> ModuleFunctionRegistry {
    let module_ids = collect_module_ids(registry);
    let mut new_registry = ModuleFunctionRegistry::new();

    for module_id in module_ids {
        let ir_module = build_ir_module_for_single_module(registry, module_id);

        if ir_module.functions.len() < 2 {
            // Not enough functions to inline in this module, copy as-is.
            for func in &ir_module.functions {
                new_registry.add_module_function(module_id, func.id, func.clone());
            }
            continue;
        }

        let directives = generate_random_directives(&ir_module, rng, inline_probability);

        if directives.is_empty() {
            // No inlining to do, copy as-is.
            for func in &ir_module.functions {
                new_registry.add_module_function(module_id, func.id, func.clone());
            }
            continue;
        }

        let result = inline_module(&ir_module, &directives);

        // Add inlined functions to the new registry.
        for func in &result.module.functions {
            new_registry.add_module_function(module_id, func.id, func.clone());
        }
    }

    new_registry
}

/// Generate random cross-module inline directives.
fn generate_random_cross_module_directives(
    ctx: &CrossModuleInlineContext,
    rng: &mut SimpleRng,
    inline_probability: u32,
) -> Vec<InlineDirective> {
    let mut directives = Vec::new();

    // Collect all (module_name, func_name, global_func_id) tuples.
    let mut all_funcs: Vec<(String, String, GlobalFuncId)> = Vec::new();

    for (mod_name, &mod_id) in ctx.iter_modules() {
        if let Some(symbols) = ctx.module_symbols.get(&mod_id) {
            for func_def in &symbols.functions {
                let global_id = GlobalFuncId { module: mod_id, func: func_def.id };
                all_funcs.push((mod_name.clone(), func_def.name.clone(), global_id));
            }
        }
    }

    if all_funcs.len() < 2 {
        return directives;
    }

    // For each pair of functions (from different modules), randomly decide to inline.
    for (caller_mod, caller_name, caller_id) in &all_funcs {
        for (callee_mod, callee_name, callee_id) in &all_funcs {
            // Skip self-calls.
            if caller_id == callee_id {
                continue;
            }

            // Only consider cross-module pairs.
            if caller_mod == callee_mod {
                continue;
            }

            if rng.next_bool(inline_probability) {
                directives.push(InlineDirective::InlineCross {
                    caller_module: caller_mod.clone(),
                    caller: caller_name.clone(),
                    callee_module: callee_mod.clone(),
                    callee: callee_name.clone(),
                });
            }
        }
    }

    directives
}

/// Apply chaos inlining across modules.
fn inline_cross_module_functions(
    registry: &ModuleFunctionRegistry,
    sections: &[WorldfileSection],
    rng: &mut SimpleRng,
    inline_probability: u32,
) -> ModuleFunctionRegistry {
    // Build the cross-module context.
    let mut ctx = CrossModuleInlineContext::new();
    let module_ids = collect_module_ids(registry);

    let mut module_id_counter = 0;
    for section in sections {
        if let WorldfileSection::Module { module, .. } = section {
            if module_id_counter < module_ids.len() {
                let module_id = module_ids[module_id_counter];
                let ir_module = build_ir_module_for_single_module(registry, module_id);
                ctx.register_module(module.clone(), module_id, ir_module.symbols);
                module_id_counter += 1;
            }
        }
    }

    // Generate cross-module directives.
    let directives = generate_random_cross_module_directives(&ctx, rng, inline_probability);

    if directives.is_empty() {
        return registry.clone();
    }

    // Apply cross-module inlining.
    let result = inline_cross_module(registry, &ctx, &directives);
    result.registry
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

    // Apply module inlining if seed is provided.
    let module_registry = if let Some(seed) = inline_seed {
        // First, apply within-module inlining.
        let mut module_rng = SimpleRng::new(seed.wrapping_mul(0xDEADBEEF));
        let inlined = inline_module_functions(
            &compiled.shared.module_registry,
            &mut module_rng,
            inline_probability,
        );

        // Then, apply cross-module inlining with a different seed.
        let mut cross_rng = SimpleRng::new(seed.wrapping_mul(0xCAFEBABE));
        Arc::new(inline_cross_module_functions(
            &inlined,
            &parsed.sections,
            &mut cross_rng,
            inline_probability / 2, // Lower probability for cross-module inlining
        ))
    } else {
        Arc::clone(&compiled.shared.module_registry)
    };

    // Create executor with possibly inlined module registry.
    let Some(mut executor) = compiled.script_executor_with_module_registry(
        module_registry,
        datalove_rt::c::DebugOutputMode::Buffer,
        None,
    ) else {
        results.push(UnitOutput {
            section_type: "compilation".into(),
            output: "error".into(),
            had_error: true,
        });
        return results;
    };

    // Create RNG for script unit inlining.
    let mut script_rng = inline_seed.map(SimpleRng::new);

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

/// Run a worldfile with chaos inlining (modules and script units).
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

        for probability in [25, 50, 75] {
            let chaos_results = run_with_chaos_inline(&db, &parsed, chaos_seed, probability);

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
