//! IR interpreter tests with JIT enabled.
//!
//! Same as interp_tests, but with the JIT compiler enabled. This tests that
//! JIT compilation produces the same results as interpretation.

use rmx::prelude::*;
use std::path::Path;
use serde::{Serialize, Deserialize};

use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection, ParsedWorldfile};
use datalove_datafun_jit::JitEngine;
use datafun::pipeline::{
    ModuleCompilationPipeline, TypecheckResult, LoweringResult, format_module_lowering_result,
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
    db: &dyn salsa::Database,
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

            let drop_key = format!("{}", module_path);
            let has_drop_errors = compiled.drop_analysis_errors.keys()
                .any(|k| k.starts_with(&drop_key));

            let has_typecheck_errors = matches!(&typecheck, TypecheckResult::Error { .. });
            let lowering = if has_drop_errors {
                let errors: Vec<_> = compiled.drop_analysis_errors.iter()
                    .filter(|(k, _)| k.starts_with(&drop_key))
                    .flat_map(|(_, v)| v.iter().cloned())
                    .collect();
                LoweringResult::Error { message: format!("Drop analysis errors: {}", errors.join("; ")) }
            } else {
                match compiled.module_lowering_results.get(&module_path) {
                    Some(ir_dumps) => format_module_lowering_result(ir_dumps, has_typecheck_errors),
                    None => LoweringResult::Skipped,
                }
            };

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

    // Create script compilation context with Buffer mode and JIT enabled.
    let mut ctx = compiled.script_context(db, datalove_rt::c::DebugOutputMode::Buffer);

    // Enable JIT with threshold=1 (compile on first call).
    let jit = JitEngine::new(1).expect("JitEngine creation failed");
    ctx.set_call_dispatcher(Box::new(jit));

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
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_fragment(source);
                let debug_output = ctx.get_debug_buffer();
                results.push(SectionResult {
                    section_type: "scriptunit-fragment".to_string(),
                    name: None,
                    typecheck: unit_result.typecheck,
                    lowering: unit_result.lowering,
                    output: unit_result.output,
                    debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                });
            }
            WorldfileSection::ScriptExpr { source } => {
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_expr(source);
                let debug_output = ctx.get_debug_buffer();
                results.push(SectionResult {
                    section_type: "scriptunit-expr".to_string(),
                    name: None,
                    typecheck: unit_result.typecheck,
                    lowering: unit_result.lowering,
                    output: unit_result.output,
                    debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                });
            }
        }
    }

    // Cleanup.
    ctx.destroy_all();

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
        let mut db = datafun::Database::default();

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
