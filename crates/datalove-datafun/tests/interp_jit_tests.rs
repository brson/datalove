//! IR interpreter tests with JIT enabled.
//!
//! Same as interp_tests, but with the JIT compiler enabled. This tests that
//! JIT compilation produces the same results as interpretation.

use rmx::prelude::*;
use std::path::Path;
use serde::{Serialize, Deserialize};

use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection, ParsedWorldfile};
use datalove_datafun_cranelift_jit::JitEngine;
use datafun::pipeline::{
    ModuleCompilationPipeline, TypecheckResult, LoweringResult, format_lowering_result,
};

/// Result of analyzing a worldfile with IR interpreter and JIT.
#[derive(Debug, Serialize, Deserialize)]
pub struct Analysis {
    /// Per-section results.
    pub sections: Vec<SectionResult>,
}

/// Result of analyzing one section.
#[derive(Debug, Serialize, Deserialize)]
pub struct SectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    pub name: Option<String>,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value (for expression units) or function call result.
    pub output: String,
    /// Debug log output (from debuglog statements).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_output: Option<String>,
}

/// Analyze a worldfile using the IR interpreter with JIT enabled.
///
/// JIT threshold is set to 1, so functions are compiled on first call.
pub fn analyze_worldfile_with_jit(
    db: &datafun::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Analysis> {
    let mut results = Vec::new();

    // Build pipeline from sections.
    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections);

    // Compile modules.
    let compiled = pipeline.compile_fresh(db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(SectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: vec![err.clone()] },
            lowering: LoweringResult::Skipped,
            output: String::new(),
            debug_output: None,
        });
        return Ok(Analysis { sections: results });
    }

    // Collect module results first.
    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, .. } = section {
            let module_path = format!("{}/{}/{}", library, package, module);

            let typecheck = match compiled.path_to_errors.get(&module_path) {
                Some(errors) if !errors.is_empty() => {
                    TypecheckResult::Error { errors: errors.clone() }
                }
                _ => TypecheckResult::Success,
            };

            let has_typecheck_errors = matches!(&typecheck, TypecheckResult::Error { .. });
            let ir_dumps = compiled.module_ir_dumps.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let ownership_errs = compiled.ownership_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let lowering_errs = compiled.lowering_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let lowering = format_lowering_result(ir_dumps, ownership_errs, lowering_errs, has_typecheck_errors);

            results.push(SectionResult {
                section_type: "module".to_string(),
                name: Some(module_path),
                typecheck,
                lowering,
                output: String::new(),
                debug_output: None,
            });
        }
    }

    // Create JIT engine with threshold=1 (compile on first call).
    let jit = JitEngine::new(1).expect("JitEngine creation failed");

    // Create script compiler and executor with Buffer mode and JIT enabled.
    let Some(mut compiler) = compiled.script_compiler(db) else {
        // Module compilation failed, return skipped results for script sections.
        for section in &parsed.sections {
            match section {
                WorldfileSection::ScriptFragment { .. } => {
                    results.push(SectionResult {
                        section_type: "scriptunit-fragment".to_string(),
                        name: None,
                        typecheck: TypecheckResult::Skipped,
                        lowering: LoweringResult::Skipped,
                        output: String::new(),
                        debug_output: None,
                    });
                }
                WorldfileSection::ScriptExpr { .. } => {
                    results.push(SectionResult {
                        section_type: "scriptunit-expr".to_string(),
                        name: None,
                        typecheck: TypecheckResult::Skipped,
                        lowering: LoweringResult::Skipped,
                        output: String::new(),
                        debug_output: None,
                    });
                }
                _ => {}
            }
        }
        return Ok(Analysis { sections: results });
    };

    let Some(mut executor) = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, Some(Box::new(jit))) else {
        return Ok(Analysis { sections: results });
    };

    // Process script units.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Already handled above.
            }
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {
                // Module action sections are for memo tests only.
            }
            WorldfileSection::ScriptFragment { source } => {
                executor.clear_debug_buffer();
                let compiled_unit = compiler.compile_fragment(source);
                let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                    executor.execute_fragment(ir_unit)
                } else {
                    String::new()
                };
                let debug_output = executor.get_debug_buffer();
                results.push(SectionResult {
                    section_type: "scriptunit-fragment".to_string(),
                    name: None,
                    typecheck: compiled_unit.typecheck,
                    lowering: compiled_unit.lowering,
                    output,
                    debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
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
                let debug_output = executor.get_debug_buffer();
                results.push(SectionResult {
                    section_type: "scriptunit-expr".to_string(),
                    name: None,
                    typecheck: compiled_unit.typecheck,
                    lowering: compiled_unit.lowering,
                    output,
                    debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                });
            }
        }
    }

    // Cleanup.
    executor.destroy_all();

    Ok(Analysis { sections: results })
}

/// Analyze a worldfile and produce RON output.
///
/// WORKAROUND: Cranelift JIT has issues when running in the main thread of a
/// PIE binary. We spawn a thread to run the analysis, which works around this
/// by placing the JIT memory in the mmap region rather than near the PIE base.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Run analysis in a spawned thread to work around Cranelift JIT limitations.
    let result = std::thread::spawn(move || {
        let db = datafun::Database::default();

        // Parse the worldfile into sections.
        let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
            .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

        // Analyze using IR interpreter with JIT.
        let analysis = analyze_worldfile_with_jit(&db, parsed)
            .map_err(|e| format!("Analysis failed: {}", e))?;

        // Serialize to RON format.
        let ron_config = ron::ser::PrettyConfig::new()
            .struct_names(true)
            .enumerate_arrays(false)
            .compact_arrays(false);

        ron::ser::to_string_pretty(&analysis, ron_config)
            .map_err(|e| format!("Failed to serialize to RON: {}", e))
    }).join().expect("analysis thread panicked");

    result
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("interp")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
