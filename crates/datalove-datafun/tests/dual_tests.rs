//! Dual interpreter/AOT comparison tests.
//!
//! This test suite runs both the interpreter and AOT compiler on the same worldfiles,
//! comparing their lowered IR (after normalization) and debuglog output.
//!
//! Input fixtures must have:
//! - Any number of module sections
//! - Exactly one scriptunit-fragment section
//! - No scriptunit-expr sections
//!
//! The test:
//! 1. Lowers via interpreter pipeline (for_aot=false) and executes
//! 2. Lowers via AOT pipeline (for_aot=true), compiles, links, executes
//! 3. Normalizes IR to remove for_aot differences (trailing drops)
//! 4. Compares normalized IRs - fails if different
//! 5. Compares debuglog outputs - fails if different

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::process::Command;

use datalove_datafun as datafun;
use datalove_datafun_cranelift_aot::AotCompiler;
use datalove_datafun_interp::FunctionRegistry;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};

/// Result of dual analysis.
#[derive(Debug, Serialize, Deserialize)]
pub struct DualAnalysis {
    /// Per-section results.
    pub sections: Vec<DualSectionResult>,
}

/// Result of analyzing one section via both pipelines.
#[derive(Debug, Serialize, Deserialize)]
pub struct DualSectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Typecheck result (shared).
    pub typecheck: datafun::pipeline::TypecheckResult,
    /// Interpreter lowering result.
    pub interp_lowering: datafun::pipeline::LoweringResult,
    /// AOT lowering result.
    pub aot_lowering: datafun::pipeline::LoweringResult,
    /// Whether normalized IRs matched.
    pub ir_match: bool,
    /// Diff between normalized IRs if they didn't match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ir_diff: Option<String>,
    /// Debuglog output from interpreter.
    pub interp_output: String,
    /// Debuglog output from AOT execution.
    pub aot_output: String,
    /// Whether debuglog outputs matched.
    pub output_match: bool,
    /// AOT compilation result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aot_compile: Option<AotCompileResult>,
    /// Link result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkResult>,
    /// Execution result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionResult>,
}

/// AOT compilation result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum AotCompileResult {
    Success,
    Error { message: String },
}

/// Link result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LinkResult {
    Success,
    Skipped { reason: String },
    Error { message: String },
}

/// Execution result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ExecutionResult {
    Success { exit_code: i32 },
    Skipped { reason: String },
    Error { message: String, exit_code: Option<i32> },
}

/// Build the runtime library once and return the path to the lib directory.
fn ensure_runtime_lib() -> &'static Path {
    datafun::pipeline::aot::ensure_runtime_lib()
}

/// Normalize IR dump by removing trailing Drop instructions before unit_end.
///
/// The for_aot flag causes extra Drop instructions to be emitted at unit end.
/// This function strips those to allow comparison between interpreter and AOT IR.
fn normalize_ir(ir: &str) -> String {
    let mut lines: Vec<&str> = ir.lines().collect();

    // Find the last block and strip drops before unit_end.
    // IR format: instructions end with "    unit_end" or "    unit_end v0" etc.
    let mut i = lines.len();
    while i > 0 {
        i -= 1;
        let line = lines[i].trim();

        // Stop at unit_end line.
        if line.starts_with("unit_end") {
            // Now go backwards and remove drop instructions.
            while i > 0 {
                let prev_line = lines[i - 1].trim();
                if prev_line.starts_with("drop ") {
                    lines.remove(i - 1);
                    i -= 1;
                } else {
                    break;
                }
            }
            break;
        }
    }

    lines.join("\n")
}

/// Analyze a worldfile using both pipelines.
fn analyze_worldfile_dual(parsed: package_load_worldfile::ParsedWorldfile) -> DualAnalysis {
    let db = datafun::Database::default();
    let mut results = Vec::new();

    // Validate input: exactly one scriptunit-fragment, no scriptunit-expr.
    let fragment_count = parsed.sections.iter()
        .filter(|s| matches!(s, WorldfileSection::ScriptFragment { .. }))
        .count();
    let expr_count = parsed.sections.iter()
        .filter(|s| matches!(s, WorldfileSection::ScriptExpr { .. }))
        .count();

    if fragment_count != 1 {
        results.push(DualSectionResult {
            section_type: "validation".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![format!("Expected exactly 1 scriptunit-fragment, found {}", fragment_count)],
            },
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            ir_match: false,
            ir_diff: None,
            interp_output: String::new(),
            aot_output: String::new(),
            output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    }

    if expr_count > 0 {
        results.push(DualSectionResult {
            section_type: "validation".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![format!("scriptunit-expr sections not allowed, found {}", expr_count)],
            },
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            ir_match: false,
            ir_diff: None,
            interp_output: String::new(),
            aot_output: String::new(),
            output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    }

    // Find the fragment source.
    let fragment_source = parsed.sections.iter()
        .find_map(|s| match s {
            WorldfileSection::ScriptFragment { source } => Some(source.as_str()),
            _ => None,
        })
        .unwrap();

    // Build pipeline and add modules (use consolidated constructor).
    let mut pipeline = datafun::pipeline::ModuleCompilationPipeline::from_sections(&db, &parsed.sections);
    let compiled = pipeline.compile_fresh(&db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(DualSectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error { errors: vec![err.clone()] },
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            ir_match: false,
            ir_diff: None,
            interp_output: String::new(),
            aot_output: String::new(),
            output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    }

    // Collect module results.
    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, .. } = section {
            let module_path = format!("{}/{}/{}", library, package, module);

            let typecheck = match compiled.path_to_errors.get(&module_path) {
                Some(errors) if !errors.is_empty() => {
                    datafun::pipeline::TypecheckResult::Error { errors: errors.clone() }
                }
                _ => datafun::pipeline::TypecheckResult::Success,
            };

            let has_typecheck_errors = matches!(&typecheck, datafun::pipeline::TypecheckResult::Error { .. });
            let ir_dumps = compiled.module_ir_dumps.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let ownership_errs = compiled.ownership_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let lowering_errs = compiled.lowering_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let lowering = datafun::pipeline::format_lowering_result(ir_dumps, ownership_errs, lowering_errs, has_typecheck_errors);

            results.push(DualSectionResult {
                section_type: "module".to_string(),
                name: Some(module_path),
                typecheck,
                interp_lowering: lowering.clone(),
                aot_lowering: lowering,
                ir_match: true,
                ir_diff: None,
                interp_output: String::new(),
                aot_output: String::new(),
                output_match: true,
                aot_compile: None,
                link: None,
                execution: None,
            });
        }
    }

    // Run interpreter pipeline.
    let Some(mut interp_compiler) = compiled.script_compiler(&db) else {
        // Module compilation failed, script execution skipped.
        // Both lowerings are Skipped, so they match.
        results.push(DualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Skipped,
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            ir_match: true,
            ir_diff: None,
            interp_output: String::new(),
            aot_output: String::new(),
            output_match: true,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    };
    let Some(mut interp_executor) = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, None) else {
        return DualAnalysis { sections: results };
    };

    interp_executor.clear_debug_buffer();
    let interp_compiled = interp_compiler.compile_fragment(fragment_source, false);
    let _interp_result_output = if let Some(ir_unit) = &interp_compiled.ir_unit {
        interp_executor.execute_fragment(ir_unit)
    } else {
        String::new()
    };
    let interp_output = interp_executor.get_debug_buffer();
    interp_executor.destroy_all();

    // Build pipeline again for AOT context (necessary because script_compiler borrows compiled).
    let mut pipeline2 = datafun::pipeline::ModuleCompilationPipeline::from_sections(&db, &parsed.sections);
    let compiled2 = pipeline2.compile_fresh(&db);

    // Run AOT pipeline.
    let Some(mut aot_compiler) = compiled2.script_compiler(&db) else {
        // Module compilation failed for AOT, but interp succeeded.
        // This shouldn't happen if both use the same input, but handle it.
        results.push(DualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Skipped,
            interp_lowering: interp_compiled.lowering,
            aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            ir_match: false,
            ir_diff: None,
            interp_output,
            aot_output: String::new(),
            output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    };
    let aot_compiled = aot_compiler.compile_fragment(fragment_source, true);
    let registry = compiled2.module_registry();

    // If typecheck failed, return early.
    if !matches!(&aot_compiled.typecheck, datafun::pipeline::TypecheckResult::Success) {
        results.push(DualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: aot_compiled.typecheck,
            interp_lowering: interp_compiled.lowering,
            aot_lowering: aot_compiled.lowering,
            ir_match: false,
            ir_diff: None,
            interp_output,
            aot_output: String::new(),
            output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    }

    // If lowering failed, return early.
    if !matches!(&aot_compiled.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        results.push(DualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: aot_compiled.typecheck,
            interp_lowering: interp_compiled.lowering,
            aot_lowering: aot_compiled.lowering,
            ir_match: false,
            ir_diff: None,
            interp_output,
            aot_output: String::new(),
            output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return DualAnalysis { sections: results };
    }

    // Get IR dumps and normalize for comparison.
    let interp_ir = match &interp_compiled.lowering {
        datafun::pipeline::LoweringResult::Success { ir } => ir.clone(),
        _ => String::new(),
    };
    let aot_ir = match &aot_compiled.lowering {
        datafun::pipeline::LoweringResult::Success { ir } => ir.clone(),
        _ => String::new(),
    };

    let interp_ir_normalized = normalize_ir(&interp_ir);
    let aot_ir_normalized = normalize_ir(&aot_ir);
    let ir_match = interp_ir_normalized == aot_ir_normalized;
    let ir_diff = if !ir_match {
        Some(format!(
            "=== Interpreter IR ===\n{}\n\n=== AOT IR ===\n{}",
            interp_ir_normalized, aot_ir_normalized
        ))
    } else {
        None
    };

    // Get the IR unit for AOT compilation.
    let ir_unit = match aot_compiled.ir_unit {
        Some(unit) => unit,
        None => {
            results.push(DualSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: aot_compiled.typecheck,
                interp_lowering: interp_compiled.lowering,
                aot_lowering: aot_compiled.lowering,
                ir_match,
                ir_diff,
                interp_output,
                aot_output: String::new(),
                output_match: false,
                aot_compile: Some(AotCompileResult::Error {
                    message: "IR unit not available".to_string(),
                }),
                link: None,
                execution: None,
            });
            return DualAnalysis { sections: results };
        }
    };

    // AOT compile, link, run.
    let (aot_compile, link, execution, aot_output) = aot_compile_link_run(&ir_unit, &registry);

    let output_match = interp_output == aot_output;

    results.push(DualSectionResult {
        section_type: "scriptunit-fragment".to_string(),
        name: None,
        typecheck: aot_compiled.typecheck,
        interp_lowering: interp_compiled.lowering,
        aot_lowering: aot_compiled.lowering,
        ir_match,
        ir_diff,
        interp_output,
        aot_output,
        output_match,
        aot_compile: Some(aot_compile),
        link: Some(link),
        execution: Some(execution),
    });

    DualAnalysis { sections: results }
}

/// AOT compile, link, and run an IR unit. Returns results and captured output.
fn aot_compile_link_run(
    ir_unit: &datalove_datafun_ir::IrScriptUnit,
    registry: &FunctionRegistry,
) -> (AotCompileResult, LinkResult, ExecutionResult, String) {
    // Create AOT compiler.
    let mut compiler = match AotCompiler::new_for_host() {
        Ok(c) => c,
        Err(e) => {
            return (
                AotCompileResult::Error { message: format!("Failed to create AOT compiler: {}", e) },
                LinkResult::Skipped { reason: "AOT compile failed".to_string() },
                ExecutionResult::Skipped { reason: "AOT compile failed".to_string() },
                String::new(),
            );
        }
    };

    // Compile to object file.
    let product = match compiler.compile_script_unit_with_world_types(
        ir_unit,
        registry.iter_all_functions(),
        registry,
    ) {
        Ok(p) => p,
        Err(e) => {
            return (
                AotCompileResult::Error { message: format!("{}", e) },
                LinkResult::Skipped { reason: "AOT compile failed".to_string() },
                ExecutionResult::Skipped { reason: "AOT compile failed".to_string() },
                String::new(),
            );
        }
    };

    let obj_bytes = match product.emit() {
        Ok(b) => b,
        Err(e) => {
            return (
                AotCompileResult::Error { message: format!("Failed to emit object: {}", e) },
                LinkResult::Skipped { reason: "AOT compile failed".to_string() },
                ExecutionResult::Skipped { reason: "AOT compile failed".to_string() },
                String::new(),
            );
        }
    };

    // Write object to temp file.
    let dir = match rmx::tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return (
                AotCompileResult::Success,
                LinkResult::Error { message: format!("Failed to create temp dir: {}", e) },
                ExecutionResult::Skipped { reason: "Link failed".to_string() },
                String::new(),
            );
        }
    };

    let obj_path = dir.path().join("test.o");
    if let Err(e) = std::fs::write(&obj_path, &obj_bytes) {
        return (
            AotCompileResult::Success,
            LinkResult::Error { message: format!("Failed to write object file: {}", e) },
            ExecutionResult::Skipped { reason: "Link failed".to_string() },
            String::new(),
        );
    }

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    if !lib_path.exists() {
        return (
            AotCompileResult::Success,
            LinkResult::Skipped { reason: format!("Runtime library not found at {:?}", lib_path) },
            ExecutionResult::Skipped { reason: "Link skipped".to_string() },
            String::new(),
        );
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
            return (
                AotCompileResult::Success,
                LinkResult::Error { message: format!("Failed to run linker: {}", e) },
                ExecutionResult::Skipped { reason: "Link failed".to_string() },
                String::new(),
            );
        }
    };

    if !link_output.status.success() {
        let stderr = String::from_utf8_lossy(&link_output.stderr);
        return (
            AotCompileResult::Success,
            LinkResult::Error { message: format!("Linker failed: {}", stderr) },
            ExecutionResult::Skipped { reason: "Link failed".to_string() },
            String::new(),
        );
    }

    // Run the executable.
    let run_output = match Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            return (
                AotCompileResult::Success,
                LinkResult::Success,
                ExecutionResult::Error { message: format!("Failed to run executable: {}", e), exit_code: None },
                String::new(),
            );
        }
    };

    let exit_code = run_output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

    if !run_output.status.success() {
        return (
            AotCompileResult::Success,
            LinkResult::Success,
            ExecutionResult::Error { message: format!("Exit code: {}", exit_code), exit_code: Some(exit_code) },
            stderr,
        );
    }

    (
        AotCompileResult::Success,
        LinkResult::Success,
        ExecutionResult::Success { exit_code },
        stderr,
    )
}

/// Analyze a worldfile and produce RON output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Analyze using both pipelines.
    let analysis = analyze_worldfile_dual(parsed);

    // Check if any module has typecheck errors (AOT can't compile broken modules).
    let has_module_typecheck_errors = analysis.sections.iter().any(|s| {
        s.section_type == "module"
            && matches!(s.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
    });

    // Check for failures.
    for section in &analysis.sections {
        // Skip AOT checks if modules have typecheck errors (modules with errors aren't lowered).
        if !has_module_typecheck_errors {
            // Check AOT compilation succeeded.
            if let Some(AotCompileResult::Error { message }) = &section.aot_compile {
                return Err(format!("AOT compile failed: {}", message));
            }

            // Check link succeeded.
            if let Some(LinkResult::Error { message }) = &section.link {
                return Err(format!("Link failed: {}", message));
            }

            // Check execution succeeded.
            match &section.execution {
                Some(ExecutionResult::Error { message, .. }) => {
                    return Err(format!("Execution failed: {}", message));
                }
                Some(ExecutionResult::Skipped { reason }) => {
                    // Skipped due to earlier failure is already caught above.
                    if !reason.contains("failed") {
                        return Err(format!("Execution skipped: {}", reason));
                    }
                }
                _ => {}
            }
        }

        if !section.ir_match {
            // If ir_diff is None, report lowering status for debugging.
            let detail = section.ir_diff.as_deref().unwrap_or_else(|| {
                match (&section.interp_lowering, &section.aot_lowering) {
                    (datafun::pipeline::LoweringResult::Error { message }, _) =>
                        return Box::leak(format!("interp lowering: {}", message).into_boxed_str()),
                    (_, datafun::pipeline::LoweringResult::Error { message }) =>
                        return Box::leak(format!("aot lowering: {}", message).into_boxed_str()),
                    (datafun::pipeline::LoweringResult::Skipped, datafun::pipeline::LoweringResult::Skipped) =>
                        return Box::leak(format!("typecheck: {:?}", section.typecheck).into_boxed_str()),
                    _ => "unknown",
                }
            });
            return Err(format!("IR mismatch: {}", detail));
        }
        if !section.output_match {
            return Err(format!(
                "Output mismatch:\n  Interpreter: {:?}\n  AOT: {:?}",
                section.interp_output, section.aot_output
            ));
        }
    }

    // Serialize to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    ron::ser::to_string_pretty(&analysis, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("dual")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
