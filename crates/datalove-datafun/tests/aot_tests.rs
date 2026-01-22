//! AOT compilation tests for worldfiles.
//!
//! This test suite compiles worldfiles to native code using the Cranelift AOT backend,
//! links with the runtime library, executes the resulting binary, and captures output.
//!
//! Tests fail with specific error categories:
//! - PARSE_ERROR: worldfile parsing failed
//! - TYPECHECK_ERROR: type checking failed
//! - LOWER_ERROR: IR lowering failed
//! - AOT_COMPILE_ERROR: Cranelift compilation failed (guides feature development)
//! - LINK_ERROR: linking with runtime failed
//! - RUNTIME_ERROR: execution crashed or bad exit code

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use std::path::Path;

use datalove_datafun as datafun;
use datafun::pipeline::{aot as pipeline_aot, ScriptCompiler, TypecheckResult, LoweringResult};
use datalove_datafun_interp::FunctionRegistry;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};

/// Result of AOT analysis.
#[derive(Debug, Serialize, Deserialize)]
pub struct AotAnalysis {
    /// Per-section results.
    pub sections: Vec<AotSectionResult>,
}

/// Result of analyzing one section via AOT.
#[derive(Debug, Serialize, Deserialize)]
pub struct AotSectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    pub name: Option<String>,
    /// Typecheck result.
    pub typecheck: datafun::pipeline::TypecheckResult,
    /// Lowering result.
    pub lowering: datafun::pipeline::LoweringResult,
    /// AOT compilation result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aot_compile: Option<AotCompileResult>,
    /// Link result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkResult>,
    /// Execution result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionResult>,
    /// Output (debuglog output from stderr).
    pub output: String,
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

/// Analyze a worldfile using AOT compilation.
fn analyze_worldfile_aot(parsed: package_load_worldfile::ParsedWorldfile) -> AotAnalysis {
    let db = datafun::Database::default();
    let mut results = Vec::new();

    // Build pipeline and add modules.
    let mut pipeline = datafun::pipeline::ModuleCompilationPipeline::new();
    pipeline.add_modules_from_sections(&db, &parsed.sections);

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = pipeline.compile_fresh(&db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(AotSectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error { errors: vec![err.clone()] },
            lowering: datafun::pipeline::LoweringResult::Skipped,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        });
        return AotAnalysis { sections: results };
    }

    // Collect module results (no execution for modules, just typecheck/lower status).
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

            results.push(AotSectionResult {
                section_type: "module".to_string(),
                name: Some(module_path),
                typecheck,
                lowering,
                aot_compile: None,
                link: None,
                execution: None,
                output: String::new(),
            });
        }
    }

    // Create script compiler (we don't need executor for AOT).
    let Some(mut compiler) = compiled.script_compiler(&db) else {
        // Module compilation failed, return skipped results for all script sections.
        for section in &parsed.sections {
            if matches!(section, WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. }) {
                results.push(AotSectionResult {
                    section_type: match section {
                        WorldfileSection::ScriptFragment { .. } => "scriptunit-fragment".to_string(),
                        WorldfileSection::ScriptExpr { .. } => "scriptunit-expr".to_string(),
                        _ => unreachable!(),
                    },
                    name: None,
                    typecheck: TypecheckResult::Skipped,
                    lowering: LoweringResult::Skipped,
                    aot_compile: None,
                    link: None,
                    execution: None,
                    output: String::new(),
                });
            }
        }
        return AotAnalysis { sections: results };
    };

    // Get the module registry for world types.
    let registry = compiled.module_registry();

    // Process script units via AOT.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. }
            | WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {
                // Already handled above (or not relevant for AOT tests).
            }
            WorldfileSection::ScriptFragment { source } => {
                let result = compile_and_run_fragment(&mut compiler, source, &registry);
                results.push(result);
            }
            WorldfileSection::ScriptExpr { source } => {
                let result = compile_and_run_expr(&mut compiler, source, &registry);
                results.push(result);
            }
        }
    }

    AotAnalysis { sections: results }
}

/// Compile and run a script fragment via AOT.
fn compile_and_run_fragment(
    compiler: &mut ScriptCompiler<'_>,
    source: &str,
    registry: &FunctionRegistry,
) -> AotSectionResult {
    // Compile to IR for AOT (for_aot=true emits drops for script-level bindings).
    let compiled = compiler.compile_fragment(source, true);

    // If typecheck or lowering failed, return early.
    if !matches!(&compiled.typecheck, datafun::pipeline::TypecheckResult::Success) {
        return AotSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: compiled.typecheck,
            lowering: compiled.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    if !matches!(&compiled.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        return AotSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: compiled.typecheck,
            lowering: compiled.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    // Get the IR unit.
    let ir_unit = match compiled.ir_unit {
        Some(unit) => unit,
        None => {
            return AotSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: compiled.typecheck,
                lowering: compiled.lowering,
                aot_compile: Some(AotCompileResult::Error {
                    message: "IR unit not available".to_string(),
                }),
                link: None,
                execution: None,
                output: String::new(),
            };
        }
    };

    // AOT compile with world types from module functions.
    aot_compile_link_run(
        "scriptunit-fragment",
        compiled.typecheck,
        compiled.lowering,
        ir_unit,
        registry,
    )
}

/// Compile and run a script expression via AOT.
fn compile_and_run_expr(
    compiler: &mut ScriptCompiler<'_>,
    source: &str,
    registry: &FunctionRegistry,
) -> AotSectionResult {
    // Compile to IR for AOT.
    let compiled = compiler.compile_expr(source);

    // If typecheck or lowering failed, return early.
    if !matches!(&compiled.typecheck, datafun::pipeline::TypecheckResult::Success) {
        return AotSectionResult {
            section_type: "scriptunit-expr".to_string(),
            name: None,
            typecheck: compiled.typecheck,
            lowering: compiled.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    if !matches!(&compiled.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        return AotSectionResult {
            section_type: "scriptunit-expr".to_string(),
            name: None,
            typecheck: compiled.typecheck,
            lowering: compiled.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    // Get the IR unit.
    let ir_unit = match compiled.ir_unit {
        Some(unit) => unit,
        None => {
            return AotSectionResult {
                section_type: "scriptunit-expr".to_string(),
                name: None,
                typecheck: compiled.typecheck,
                lowering: compiled.lowering,
                aot_compile: Some(AotCompileResult::Error {
                    message: "IR unit not available".to_string(),
                }),
                link: None,
                execution: None,
                output: String::new(),
            };
        }
    };

    // AOT compile with world types from module functions.
    aot_compile_link_run(
        "scriptunit-expr",
        compiled.typecheck,
        compiled.lowering,
        ir_unit,
        registry,
    )
}

/// AOT compile, link, and run an IR unit using pipeline::aot utilities.
fn aot_compile_link_run(
    section_type: &str,
    typecheck: datafun::pipeline::TypecheckResult,
    lowering: datafun::pipeline::LoweringResult,
    ir_unit: datalove_datafun_ir::IrScriptUnit,
    registry: &FunctionRegistry,
) -> AotSectionResult {
    // Compile to object bytes.
    let obj_bytes = match pipeline_aot::compile_script_to_object_with_world(
        &ir_unit,
        registry.iter_all_functions(),
        registry,
    ) {
        Ok(b) => b,
        Err(e) => {
            return AotSectionResult {
                section_type: section_type.to_string(),
                name: None,
                typecheck,
                lowering,
                aot_compile: Some(AotCompileResult::Error {
                    message: format!("{}", e),
                }),
                link: None,
                execution: None,
                output: String::new(),
            };
        }
    };

    // Link to executable.
    let (exe_path, _dir) = match pipeline_aot::link_object_to_temp_executable(&obj_bytes) {
        Ok(r) => r,
        Err(e) => {
            return AotSectionResult {
                section_type: section_type.to_string(),
                name: None,
                typecheck,
                lowering,
                aot_compile: Some(AotCompileResult::Success),
                link: Some(LinkResult::Error {
                    message: format!("{}", e),
                }),
                execution: None,
                output: String::new(),
            };
        }
    };

    // Run the executable.
    match pipeline_aot::run_executable(&exe_path) {
        Ok(output) => AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Success),
            execution: Some(ExecutionResult::Success { exit_code: output.exit_code }),
            output: output.stderr,
        },
        Err(pipeline_aot::ExecError::ExitCode { code, stderr }) => AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Success),
            execution: Some(ExecutionResult::Error {
                message: format!("Exit code: {}", code),
                exit_code: Some(code),
            }),
            output: stderr,
        },
        Err(pipeline_aot::ExecError::Exec(e)) => AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Success),
            execution: Some(ExecutionResult::Error {
                message: format!("Failed to run executable: {}", e),
                exit_code: None,
            }),
            output: String::new(),
        },
    }
}

/// Analyze a worldfile and produce RON output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Analyze using AOT compilation.
    let analysis = analyze_worldfile_aot(parsed);

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
        .fixture_subdir("aot")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
