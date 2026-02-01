//! IR function inlining tests.
//!
//! This test suite loads worldfiles containing modules and inline directives,
//! performs inlining according to the directives, and outputs the IR before
//! and after inlining for snapshot testing.
//!
//! Supports both single-module and cross-module inlining.

use std::path::Path;

use datalove_datafun as datafun;
use datalove_datafun_inline::{
    inline_cross_module, inline_module, parse_inline_directives, CrossModuleInlineContext,
    InlineDirective,
};
use datalove_datafun_ir::{IrModule, IrModuleId, ModuleFunctionRegistry, SymbolTable};
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};

use datafun::pipeline::{ConstInlining, ModuleCompilationPipeline};

/// Extract inline directives from parsed worldfile sections.
fn extract_inline_directives(sections: &[WorldfileSection]) -> Vec<InlineDirective> {
    let mut directives = Vec::new();

    for section in sections {
        if let WorldfileSection::InlineDirectives { source } = section {
            match parse_inline_directives(source) {
                Ok(parsed) => directives.extend(parsed),
                Err(e) => {
                    // Include parse error in output for debugging.
                    eprintln!("Error parsing inline directives: {}", e);
                }
            }
        }
    }

    directives
}

/// Check if any directives are cross-module directives.
fn has_cross_module_directives(directives: &[InlineDirective]) -> bool {
    directives.iter().any(|d| {
        matches!(
            d,
            InlineDirective::InlineCross { .. } | InlineDirective::InlineCrossAll { .. }
        )
    })
}

/// Format inline directives for output.
fn format_directives(directives: &[InlineDirective]) -> String {
    if directives.is_empty() {
        return "(none)".to_string();
    }

    directives
        .iter()
        .map(|d| match d {
            InlineDirective::Inline { caller, callee } => {
                format!("inline {} {}", caller, callee)
            }
            InlineDirective::InlineAt {
                caller,
                callee,
                call_index,
            } => {
                format!("inline {} {} at {}", caller, callee, call_index)
            }
            InlineDirective::InlineAll { callee } => {
                format!("inline-all {}", callee)
            }
            InlineDirective::InlineCross {
                caller_module,
                caller,
                callee_module,
                callee,
            } => {
                format!(
                    "inline-cross {}::{} {}::{}",
                    caller_module, caller, callee_module, callee
                )
            }
            InlineDirective::InlineCrossAll {
                callee_module,
                callee,
            } => {
                format!("inline-cross-all {}::{}", callee_module, callee)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build an IrModule from the module registry's functions.
fn build_ir_module_from_registry(registry: &ModuleFunctionRegistry) -> IrModule {
    // Collect all functions and build symbol table.
    let mut functions: Vec<_> = registry.iter_module_functions().collect();

    // Sort functions by name for deterministic output.
    functions.sort_by(|a, b| a.name.cmp(&b.name));

    let mut symbols = SymbolTable::new();
    for func in &functions {
        // Register in symbol table using the function's existing ID.
        symbols.define_func_with_id(func.id, func.name.clone(), func.params.len());
    }

    IrModule { functions, symbols }
}

/// Build an IrModule from functions of a single module in the registry.
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

/// Analyze a worldfile and produce inlining test output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Build pipeline from sections.
    let mut pipeline =
        ModuleCompilationPipeline::from_sections(&db, &parsed.sections, ConstInlining::Enabled);

    // Verify local/test/main module exists.
    if !pipeline.contains_module("local", "test", "main") {
        return Err("No local/test/main module found".to_string());
    }

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = pipeline.compile_fresh(&db);

    // Check for errors.
    if let Some(err) = &compiled.resolution_error {
        return Ok(format!("Resolution error: {}\n", err));
    }

    let all_parse_errors = compiled.all_parse_errors();
    if !all_parse_errors.is_empty() {
        return Ok(format!("Parse error: {}\n", all_parse_errors.join("; ")));
    }

    let all_typecheck_errors = compiled.all_typecheck_errors();
    if !all_typecheck_errors.is_empty() {
        return Ok(format!(
            "Typecheck error: {}\n",
            all_typecheck_errors.join("; ")
        ));
    }

    let all_lowering_errors = compiled.all_lowering_errors();
    if !all_lowering_errors.is_empty() {
        return Ok(format!("{}\n", all_lowering_errors.join("\n")));
    }

    // Extract inline directives.
    let directives = extract_inline_directives(&parsed.sections);

    // Check if we need cross-module inlining.
    if has_cross_module_directives(&directives) {
        // Cross-module inlining path.
        analyze_cross_module(&compiled.shared.module_registry, &parsed.sections, &directives)
    } else {
        // Single-module inlining path (original behavior).
        analyze_single_module(&compiled.shared.module_registry, &directives)
    }
}

/// Analyze using single-module inlining (merges all modules into one).
fn analyze_single_module(
    registry: &ModuleFunctionRegistry,
    directives: &[InlineDirective],
) -> Result<String, String> {
    // Build IR module from the module registry.
    let ir_module = build_ir_module_from_registry(registry);

    // Format before IR.
    let before_ir = format!("{}", ir_module);

    // Perform inlining.
    let result = inline_module(&ir_module, directives);

    // Format after IR.
    let after_ir = format!("{}", result.module);

    // Build output.
    let mut output = String::new();

    output.push_str("=== BEFORE INLINING ===\n");
    output.push_str(&before_ir);
    output.push('\n');

    output.push_str("=== INLINE DIRECTIVES ===\n");
    output.push_str(&format_directives(directives));
    output.push_str("\n\n");

    output.push_str("=== AFTER INLINING ===\n");
    output.push_str(&after_ir);
    output.push('\n');

    output.push_str("=== INLINING STATS ===\n");
    output.push_str(&format!("Inlined: {} call(s)\n", result.inlined_count));

    if !result.skipped.is_empty() {
        output.push_str(&format!("Skipped: {} directive(s)\n", result.skipped.len()));
        for skip in &result.skipped {
            output.push_str(&format!("  - {:?}\n", skip));
        }
    } else {
        output.push_str("Skipped: 0\n");
    }

    Ok(output)
}

/// Analyze using cross-module inlining.
fn analyze_cross_module(
    registry: &ModuleFunctionRegistry,
    sections: &[WorldfileSection],
    directives: &[InlineDirective],
) -> Result<String, String> {
    // Build cross-module context from the worldfile sections.
    let mut ctx = CrossModuleInlineContext::new();
    let module_ids = collect_module_ids(registry);

    // Map module names from sections to IrModuleIds.
    // We use the module name (e.g., "main") for directive resolution.
    let mut module_id_counter = 0;
    for section in sections {
        if let WorldfileSection::Module { module, .. } = section {
            if module_id_counter < module_ids.len() {
                let module_id = module_ids[module_id_counter];

                // Build symbol table for this module.
                let ir_module = build_ir_module_for_single_module(registry, module_id);
                ctx.register_module(module.clone(), module_id, ir_module.symbols);

                module_id_counter += 1;
            }
        }
    }

    // Sort modules by name for deterministic output.
    let mut sorted_modules: Vec<_> = ctx.iter_modules().collect();
    sorted_modules.sort_by_key(|(name, _)| (*name).clone());

    // Format before IR (per-module).
    let mut before_ir = String::new();
    for (name, module_id) in &sorted_modules {
        let ir_module = build_ir_module_for_single_module(registry, **module_id);
        before_ir.push_str(&format!("--- Module: {} ---\n", name));
        before_ir.push_str(&format!("{}\n", ir_module));
    }

    // Perform cross-module inlining.
    let result = inline_cross_module(registry, &ctx, directives);

    // Format after IR (per-module).
    let mut after_ir = String::new();
    for (name, module_id) in &sorted_modules {
        let ir_module = build_ir_module_for_single_module(&result.registry, **module_id);
        after_ir.push_str(&format!("--- Module: {} ---\n", name));
        after_ir.push_str(&format!("{}\n", ir_module));
    }

    // Build output.
    let mut output = String::new();

    output.push_str("=== BEFORE INLINING ===\n");
    output.push_str(&before_ir);

    output.push_str("=== INLINE DIRECTIVES ===\n");
    output.push_str(&format_directives(directives));
    output.push_str("\n\n");

    output.push_str("=== AFTER INLINING ===\n");
    output.push_str(&after_ir);

    output.push_str("=== INLINING STATS ===\n");
    output.push_str(&format!("Inlined: {} call(s)\n", result.inlined_count));

    if !result.skipped.is_empty() {
        output.push_str(&format!("Skipped: {} directive(s)\n", result.skipped.len()));
        for skip in &result.skipped {
            output.push_str(&format!("  - {:?}\n", skip));
        }
    } else {
        output.push_str("Skipped: 0\n");
    }

    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("ir_inline")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
