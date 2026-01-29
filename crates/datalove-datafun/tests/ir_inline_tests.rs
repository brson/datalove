//! IR function inlining tests.
//!
//! This test suite loads worldfiles containing modules and inline directives,
//! performs inlining according to the directives, and outputs the IR before
//! and after inlining for snapshot testing.

use std::path::Path;

use datalove_datafun as datafun;
use datalove_datafun_inline::{inline_module, parse_inline_directives, InlineDirective};
use datalove_datafun_ir::{IrModule, ModuleFunctionRegistry, SymbolTable};
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
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build an IrModule from the module registry's functions.
fn build_ir_module_from_registry(registry: &ModuleFunctionRegistry) -> IrModule {
    // Collect all functions and build symbol table.
    let mut functions: Vec<_> = registry.iter_module_functions().cloned().collect();

    // Sort functions by name for deterministic output.
    functions.sort_by(|a, b| a.name.cmp(&b.name));

    let mut symbols = SymbolTable::new();
    for func in &functions {
        // Register in symbol table using the function's existing ID.
        symbols.define_func_with_id(func.id, func.name.clone(), func.params.len());
    }

    IrModule { functions, symbols }
}

/// Analyze a worldfile and produce inlining test output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

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

    // Build IR module from the module registry.
    let ir_module = build_ir_module_from_registry(&compiled.shared.module_registry);

    // Extract inline directives.
    let directives = extract_inline_directives(&parsed.sections);

    // Format before IR.
    let before_ir = format!("{}", ir_module);

    // Perform inlining.
    let result = inline_module(&ir_module, &directives);

    // Format after IR.
    let after_ir = format!("{}", result.module);

    // Build output.
    let mut output = String::new();

    output.push_str("=== BEFORE INLINING ===\n");
    output.push_str(&before_ir);
    output.push('\n');

    output.push_str("=== INLINE DIRECTIVES ===\n");
    output.push_str(&format_directives(&directives));
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

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("ir_inline")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
