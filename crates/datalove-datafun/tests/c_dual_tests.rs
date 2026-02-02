//! Dual interpreter/C-AOT comparison tests.
//!
//! This test suite runs both the interpreter and C AOT compiler on the same worldfiles,
//! comparing their debuglog output.
//!
//! Input fixtures must have:
//! - Any number of module sections
//! - Exactly one scriptunit-fragment section
//! - No scriptunit-expr sections
//!
//! The test:
//! 1. Lowers and executes via interpreter
//! 2. Lowers, generates C code, compiles, links, and executes via C AOT
//! 3. Compares debuglog outputs - fails if different

use datalove_datafun_ir::expand_ir_strings;
use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::process::Command;

use datalove_datafun as datafun;
use datalove_datafun_c_aot::CAotCompiler;
use datalove_datafun_interp::FunctionRegistry;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datafun::pipeline::ConstInlining;

/// Result of dual analysis.
#[derive(Debug, Serialize, Deserialize)]
pub struct CDualAnalysis {
    /// Per-section results.
    pub sections: Vec<CDualSectionResult>,
}

/// Result of analyzing one section via both pipelines.
#[derive(Debug, Serialize, Deserialize)]
pub struct CDualSectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Typecheck result (shared).
    pub typecheck: datafun::pipeline::TypecheckResult,
    /// Ownership analysis result (shared).
    pub ownership: datafun::pipeline::OwnershipResult,
    /// Interpreter lowering result.
    pub interp_lowering: datafun::pipeline::LoweringResult,
    /// C AOT lowering result.
    pub c_aot_lowering: datafun::pipeline::LoweringResult,
    /// Debuglog output from interpreter.
    pub interp_output: String,
    /// Debuglog output from C AOT execution.
    pub c_aot_output: String,
    /// Whether debuglog outputs matched.
    pub output_match: bool,
    /// C code generation result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_codegen: Option<CCodegenResult>,
    /// C compilation result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_compile: Option<CCompileResult>,
    /// Link result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkResult>,
    /// Execution result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionResult>,
}

/// C code generation result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CCodegenResult {
    Success,
    Error { message: String },
}

/// C compilation result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum CCompileResult {
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

/// Analyze a worldfile using both pipelines.
fn analyze_worldfile_dual(parsed: package_load_worldfile::ParsedWorldfile) -> CDualAnalysis {
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
        results.push(CDualSectionResult {
            section_type: "validation".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![format!("Expected exactly 1 scriptunit-fragment, found {}", fragment_count)],
            },
            ownership: datafun::pipeline::OwnershipResult::Skipped,
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            c_aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            interp_output: String::new(),
            c_aot_output: String::new(),
            output_match: false,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    }

    if expr_count > 0 {
        results.push(CDualSectionResult {
            section_type: "validation".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![format!("scriptunit-expr sections not allowed, found {}", expr_count)],
            },
            ownership: datafun::pipeline::OwnershipResult::Skipped,
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            c_aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            interp_output: String::new(),
            c_aot_output: String::new(),
            output_match: false,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    }

    // Find the fragment source.
    let fragment_source = parsed.sections.iter()
        .find_map(|s| match s {
            WorldfileSection::ScriptFragment { source } => Some(source.as_str()),
            _ => None,
        })
        .unwrap();

    // Build pipeline and add modules (use consolidated constructor).
    let mut pipeline = datafun::pipeline::ModuleCompilationPipeline::from_sections(&db, &parsed.sections, ConstInlining::Enabled);
    let compiled = pipeline.compile_fresh(&db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(CDualSectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error { errors: vec![err.clone()] },
            ownership: datafun::pipeline::OwnershipResult::Skipped,
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            c_aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            interp_output: String::new(),
            c_aot_output: String::new(),
            output_match: false,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
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
            let ownership = datafun::pipeline::format_ownership_result(ownership_errs, has_typecheck_errors);
            let has_ownership_errors = matches!(&ownership, datafun::pipeline::OwnershipResult::Error { .. });
            let lowering = datafun::pipeline::format_lowering_result(ir_dumps, lowering_errs, has_typecheck_errors || has_ownership_errors);

            results.push(CDualSectionResult {
                section_type: "module".to_string(),
                name: Some(module_path),
                typecheck,
                ownership,
                interp_lowering: lowering.clone(),
                c_aot_lowering: lowering,
                interp_output: String::new(),
                c_aot_output: String::new(),
                output_match: true,
                c_codegen: None,
                c_compile: None,
                link: None,
                execution: None,
            });
        }
    }

    // Run interpreter pipeline.
    let Some(mut interp_compiler) = compiled.script_compiler_default(&db) else {
        results.push(CDualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Skipped,
            ownership: datafun::pipeline::OwnershipResult::Skipped,
            interp_lowering: datafun::pipeline::LoweringResult::Skipped,
            c_aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            interp_output: String::new(),
            c_aot_output: String::new(),
            output_match: true,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    };
    let Some(mut interp_executor) = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, None) else {
        return CDualAnalysis { sections: results };
    };

    interp_executor.clear_debug_buffer();
    let interp_compiled = interp_compiler.compile_fragment(fragment_source);
    let _interp_result_output = if let Some(ir_unit) = &interp_compiled.ir_unit {
        interp_executor.execute_fragment(ir_unit)
    } else {
        String::new()
    };
    let interp_output = interp_executor.get_debug_buffer();
    interp_executor.destroy_live_values();

    // Build pipeline again for C AOT context.
    let mut pipeline2 = datafun::pipeline::ModuleCompilationPipeline::from_sections(&db, &parsed.sections, ConstInlining::Enabled);
    let compiled2 = pipeline2.compile_fresh(&db);

    // Run C AOT pipeline.
    let Some(mut c_aot_compiler) = compiled2.script_compiler_default(&db) else {
        results.push(CDualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Skipped,
            ownership: datafun::pipeline::OwnershipResult::Skipped,
            interp_lowering: interp_compiled.lowering,
            c_aot_lowering: datafun::pipeline::LoweringResult::Skipped,
            interp_output,
            c_aot_output: String::new(),
            output_match: false,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    };
    let c_aot_compiled = c_aot_compiler.compile_fragment(fragment_source);
    let registry = compiled2.module_registry();

    // If typecheck failed, return early.
    if !matches!(&c_aot_compiled.typecheck, datafun::pipeline::TypecheckResult::Success) {
        results.push(CDualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: c_aot_compiled.typecheck,
            ownership: c_aot_compiled.ownership,
            interp_lowering: interp_compiled.lowering,
            c_aot_lowering: c_aot_compiled.lowering,
            interp_output,
            c_aot_output: String::new(),
            output_match: false,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    }

    // If ownership analysis failed, return early.
    if !matches!(&c_aot_compiled.ownership, datafun::pipeline::OwnershipResult::Success) {
        let output_match = match (&interp_compiled.ownership, &c_aot_compiled.ownership) {
            (
                datafun::pipeline::OwnershipResult::Error { message: m1 },
                datafun::pipeline::OwnershipResult::Error { message: m2 },
            ) => m1 == m2,
            _ => false,
        };
        results.push(CDualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: c_aot_compiled.typecheck,
            ownership: c_aot_compiled.ownership,
            interp_lowering: interp_compiled.lowering,
            c_aot_lowering: c_aot_compiled.lowering,
            interp_output,
            c_aot_output: String::new(),
            output_match,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    }

    // If lowering failed, return early.
    if !matches!(&c_aot_compiled.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        let output_match = match (&interp_compiled.lowering, &c_aot_compiled.lowering) {
            (
                datafun::pipeline::LoweringResult::Error { message: m1 },
                datafun::pipeline::LoweringResult::Error { message: m2 },
            ) => m1 == m2,
            _ => false,
        };
        results.push(CDualSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: c_aot_compiled.typecheck,
            ownership: c_aot_compiled.ownership,
            interp_lowering: interp_compiled.lowering,
            c_aot_lowering: c_aot_compiled.lowering,
            interp_output,
            c_aot_output: String::new(),
            output_match,
            c_codegen: None,
            c_compile: None,
            link: None,
            execution: None,
        });
        return CDualAnalysis { sections: results };
    }

    // Get the IR unit for C AOT compilation.
    let ir_unit = match c_aot_compiled.ir_unit {
        Some(unit) => unit,
        None => {
            results.push(CDualSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: c_aot_compiled.typecheck,
                ownership: c_aot_compiled.ownership,
                interp_lowering: interp_compiled.lowering,
                c_aot_lowering: c_aot_compiled.lowering,
                interp_output,
                c_aot_output: String::new(),
                output_match: false,
                c_codegen: Some(CCodegenResult::Error {
                    message: "IR unit not available".to_string(),
                }),
                c_compile: None,
                link: None,
                execution: None,
            });
            return CDualAnalysis { sections: results };
        }
    };

    // C AOT compile, link, run.
    let (c_codegen, c_compile, link, execution, c_aot_output) = c_aot_compile_link_run(&ir_unit, &registry);

    let output_match = interp_output == c_aot_output;

    results.push(CDualSectionResult {
        section_type: "scriptunit-fragment".to_string(),
        name: None,
        typecheck: c_aot_compiled.typecheck,
        ownership: c_aot_compiled.ownership,
        interp_lowering: interp_compiled.lowering,
        c_aot_lowering: c_aot_compiled.lowering,
        interp_output,
        c_aot_output,
        output_match,
        c_codegen: Some(c_codegen),
        c_compile: Some(c_compile),
        link: Some(link),
        execution: Some(execution),
    });

    CDualAnalysis { sections: results }
}

/// C AOT compile, link, and run an IR unit. Returns results and captured output.
fn c_aot_compile_link_run(
    ir_unit: &datalove_datafun_ir::IrCodeUnit,
    registry: &FunctionRegistry,
) -> (CCodegenResult, CCompileResult, LinkResult, ExecutionResult, String) {
    // Create C AOT compiler.
    let mut compiler = CAotCompiler::new();

    // Generate C source code.
    let c_source = match compiler.compile_script_unit_with_registry(ir_unit, registry) {
        Ok(src) => src,
        Err(e) => {
            return (
                CCodegenResult::Error { message: format!("{}", e) },
                CCompileResult::Error { message: "C codegen failed".to_string() },
                LinkResult::Skipped { reason: "C codegen failed".to_string() },
                ExecutionResult::Skipped { reason: "C codegen failed".to_string() },
                String::new(),
            );
        }
    };

    // Write C source to temp file.
    let dir = match rmx::tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return (
                CCodegenResult::Success,
                CCompileResult::Error { message: format!("Failed to create temp dir: {}", e) },
                LinkResult::Skipped { reason: "C compile failed".to_string() },
                ExecutionResult::Skipped { reason: "C compile failed".to_string() },
                String::new(),
            );
        }
    };

    let c_path = dir.path().join("test.c");
    if let Err(e) = std::fs::write(&c_path, &c_source) {
        return (
            CCodegenResult::Success,
            CCompileResult::Error { message: format!("Failed to write C file: {}", e) },
            LinkResult::Skipped { reason: "C compile failed".to_string() },
            ExecutionResult::Skipped { reason: "C compile failed".to_string() },
            String::new(),
        );
    }

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    if !lib_path.exists() {
        return (
            CCodegenResult::Success,
            CCompileResult::Error { message: format!("Runtime library not found at {:?}", lib_path) },
            LinkResult::Skipped { reason: "Runtime library missing".to_string() },
            ExecutionResult::Skipped { reason: "Runtime library missing".to_string() },
            String::new(),
        );
    }

    // Compile and link with cc.
    let exe_path = dir.path().join("test");
    let compile_output = Command::new("cc")
        .args([
            "-std=c11",
            "-O0",
            "-g",
            c_path.to_str().unwrap(),
            lib_path.to_str().unwrap(),
            "-ldl", "-lpthread", "-lm",
            "-o", exe_path.to_str().unwrap(),
        ])
        .output();

    let compile_output = match compile_output {
        Ok(o) => o,
        Err(e) => {
            return (
                CCodegenResult::Success,
                CCompileResult::Error { message: format!("Failed to run compiler: {}", e) },
                LinkResult::Skipped { reason: "C compile failed".to_string() },
                ExecutionResult::Skipped { reason: "C compile failed".to_string() },
                String::new(),
            );
        }
    };

    if !compile_output.status.success() {
        let stderr = String::from_utf8_lossy(&compile_output.stderr);
        // Include the C source for debugging.
        return (
            CCodegenResult::Success,
            CCompileResult::Error { message: format!("Compiler failed:\n{}\n\nC source:\n{}", stderr, c_source) },
            LinkResult::Skipped { reason: "C compile failed".to_string() },
            ExecutionResult::Skipped { reason: "C compile failed".to_string() },
            String::new(),
        );
    }

    // Run the executable.
    let run_output = match Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            return (
                CCodegenResult::Success,
                CCompileResult::Success,
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
            CCodegenResult::Success,
            CCompileResult::Success,
            LinkResult::Success,
            ExecutionResult::Error { message: format!("Exit code: {}", exit_code), exit_code: Some(exit_code) },
            stderr,
        );
    }

    (
        CCodegenResult::Success,
        CCompileResult::Success,
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

    // Check if any module has typecheck errors.
    let has_module_typecheck_errors = analysis.sections.iter().any(|s| {
        s.section_type == "module"
            && matches!(s.typecheck, datafun::pipeline::TypecheckResult::Error { .. })
    });

    // Check for failures.
    for section in &analysis.sections {
        // Skip C AOT checks if modules have typecheck errors.
        if !has_module_typecheck_errors {
            // Check C codegen succeeded.
            if let Some(CCodegenResult::Error { message }) = &section.c_codegen {
                return Err(format!("C codegen failed: {}", message));
            }

            // Check C compile succeeded.
            if let Some(CCompileResult::Error { message }) = &section.c_compile {
                return Err(format!("C compile failed: {}", message));
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
                    if !reason.contains("failed") {
                        return Err(format!("Execution skipped: {}", reason));
                    }
                }
                _ => {}
            }
        }

        if !section.output_match {
            return Err(format!(
                "Output mismatch:\n  Interpreter: {:?}\n  C AOT: {:?}",
                section.interp_output, section.c_aot_output
            ));
        }
    }

    // Serialize to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    let ron_output = ron::ser::to_string_pretty(&analysis, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))?;

    // Expand IR strings for readability.
    Ok(expand_ir_strings(&ron_output))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("c_dual")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
