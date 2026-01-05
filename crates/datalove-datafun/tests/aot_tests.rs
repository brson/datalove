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
use std::process::Command;

use datalove_datafun as datafun;
use datalove_datafun_aot_cranelift::AotCompiler;
use datalove_datafun_ir::FunctionRegistry;
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

/// Get the path to the runtime library.
fn get_runtime_lib_dir() -> std::path::PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| ".".to_string());
    let manifest_path = std::path::PathBuf::from(manifest_dir);
    manifest_path.join("../../target/debug").canonicalize()
        .expect("failed to find target/debug directory")
}

/// Analyze a worldfile using AOT compilation.
fn analyze_worldfile_aot(parsed: package_load_worldfile::ParsedWorldfile) -> AotAnalysis {
    let db = datafun::Database::default();
    let mut results = Vec::new();

    // Build pipeline and add modules.
    let mut pipeline = datafun::pipeline::ModuleCompilationPipeline::new(&db);
    pipeline.add_modules_from_sections(&parsed.sections);

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = pipeline.compile();

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

            let drop_key = format!("{}", module_path);
            let has_drop_errors = compiled.drop_analysis_errors.keys()
                .any(|k| k.starts_with(&drop_key));

            let has_typecheck_errors = matches!(&typecheck, datafun::pipeline::TypecheckResult::Error { .. });
            let lowering = if has_drop_errors {
                let errors: Vec<_> = compiled.drop_analysis_errors.iter()
                    .filter(|(k, _)| k.starts_with(&drop_key))
                    .flat_map(|(_, v)| v.iter().cloned())
                    .collect();
                datafun::pipeline::LoweringResult::Error {
                    message: format!("Drop analysis errors: {}", errors.join("; "))
                }
            } else {
                match compiled.module_lowering_results.get(&module_path) {
                    Some(ir_dumps) => datafun::pipeline::format_module_lowering_result(ir_dumps, has_typecheck_errors),
                    None => datafun::pipeline::LoweringResult::Skipped,
                }
            };

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

    // Create script compilation context with Disabled mode (we don't use interpreter).
    let mut ctx = compiled.script_context(&db, datalove_rt::c::DebugOutputMode::Disabled);

    // Process script units via AOT.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Already handled above.
            }
            WorldfileSection::ScriptFragment { source } => {
                let result = compile_and_run_fragment(&mut ctx, source);
                results.push(result);
            }
            WorldfileSection::ScriptExpr { source } => {
                let result = compile_and_run_expr(&mut ctx, source);
                results.push(result);
            }
        }
    }

    ctx.destroy_all();
    AotAnalysis { sections: results }
}

/// Compile and run a script fragment via AOT.
fn compile_and_run_fragment(
    ctx: &mut datafun::pipeline::ScriptCompilationContext<'_>,
    source: &str,
) -> AotSectionResult {
    // Lower to IR without executing via interpreter.
    let lower_result = ctx.lower_fragment(source);

    // If typecheck or lowering failed, return early.
    if !matches!(&lower_result.typecheck, datafun::pipeline::TypecheckResult::Success) {
        return AotSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: lower_result.typecheck,
            lowering: lower_result.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    if !matches!(&lower_result.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        return AotSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: lower_result.typecheck,
            lowering: lower_result.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    // Get the IR unit.
    let ir_unit = match lower_result.ir_unit {
        Some(unit) => unit,
        None => {
            return AotSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: lower_result.lowering,
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
        lower_result.typecheck,
        lower_result.lowering,
        ir_unit,
        &ctx.env.registry,
    )
}

/// Compile and run a script expression via AOT.
fn compile_and_run_expr(
    ctx: &mut datafun::pipeline::ScriptCompilationContext<'_>,
    source: &str,
) -> AotSectionResult {
    // Lower to IR without executing via interpreter.
    let lower_result = ctx.lower_expr(source);

    // If typecheck or lowering failed, return early.
    if !matches!(&lower_result.typecheck, datafun::pipeline::TypecheckResult::Success) {
        return AotSectionResult {
            section_type: "scriptunit-expr".to_string(),
            name: None,
            typecheck: lower_result.typecheck,
            lowering: lower_result.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    if !matches!(&lower_result.lowering, datafun::pipeline::LoweringResult::Success { .. }) {
        return AotSectionResult {
            section_type: "scriptunit-expr".to_string(),
            name: None,
            typecheck: lower_result.typecheck,
            lowering: lower_result.lowering,
            aot_compile: None,
            link: None,
            execution: None,
            output: String::new(),
        };
    }

    // Get the IR unit.
    let ir_unit = match lower_result.ir_unit {
        Some(unit) => unit,
        None => {
            return AotSectionResult {
                section_type: "scriptunit-expr".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: lower_result.lowering,
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
        lower_result.typecheck,
        lower_result.lowering,
        ir_unit,
        &ctx.env.registry,
    )
}

/// AOT compile, link, and run an IR unit.
fn aot_compile_link_run(
    section_type: &str,
    typecheck: datafun::pipeline::TypecheckResult,
    lowering: datafun::pipeline::LoweringResult,
    ir_unit: datalove_datafun_ir::IrScriptUnit,
    registry: &FunctionRegistry,
) -> AotSectionResult {
    // Create AOT compiler.
    let mut compiler = match AotCompiler::new_for_host() {
        Ok(c) => c,
        Err(e) => {
            return AotSectionResult {
                section_type: section_type.to_string(),
                name: None,
                typecheck,
                lowering,
                aot_compile: Some(AotCompileResult::Error {
                    message: format!("Failed to create AOT compiler: {}", e),
                }),
                link: None,
                execution: None,
                output: String::new(),
            };
        }
    };

    // Compile to object file with world types from module functions.
    let product = match compiler.compile_script_unit_with_world_types(
        &ir_unit,
        registry.iter_all_functions(),
        registry,
    ) {
        Ok(p) => p,
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

    let obj_bytes = match product.emit() {
        Ok(b) => b,
        Err(e) => {
            return AotSectionResult {
                section_type: section_type.to_string(),
                name: None,
                typecheck,
                lowering,
                aot_compile: Some(AotCompileResult::Error {
                    message: format!("Failed to emit object: {}", e),
                }),
                link: None,
                execution: None,
                output: String::new(),
            };
        }
    };

    // Write object to temp file.
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return AotSectionResult {
                section_type: section_type.to_string(),
                name: None,
                typecheck,
                lowering,
                aot_compile: Some(AotCompileResult::Success),
                link: Some(LinkResult::Error {
                    message: format!("Failed to create temp dir: {}", e),
                }),
                execution: None,
                output: String::new(),
            };
        }
    };

    let obj_path = dir.path().join("test.o");
    if let Err(e) = std::fs::write(&obj_path, &obj_bytes) {
        return AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Error {
                message: format!("Failed to write object file: {}", e),
            }),
            execution: None,
            output: String::new(),
        };
    }

    // Find runtime library.
    let lib_dir = get_runtime_lib_dir();
    let lib_path = lib_dir.join("libdatalove_rt.so");

    if !lib_path.exists() {
        return AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Skipped {
                reason: format!("Runtime library not found at {:?}", lib_path),
            }),
            execution: None,
            output: String::new(),
        };
    }

    // Link with cc.
    let exe_path = dir.path().join("test");
    let link_output = Command::new("cc")
        .args([
            obj_path.to_str().unwrap(),
            "-L", lib_dir.to_str().unwrap(),
            "-ldatalove_rt",
            "-Wl,-rpath", lib_dir.to_str().unwrap(),
            "-o", exe_path.to_str().unwrap(),
        ])
        .output();

    let link_output = match link_output {
        Ok(o) => o,
        Err(e) => {
            return AotSectionResult {
                section_type: section_type.to_string(),
                name: None,
                typecheck,
                lowering,
                aot_compile: Some(AotCompileResult::Success),
                link: Some(LinkResult::Error {
                    message: format!("Failed to run linker: {}", e),
                }),
                execution: None,
                output: String::new(),
            };
        }
    };

    if !link_output.status.success() {
        let stderr = String::from_utf8_lossy(&link_output.stderr);
        return AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Error {
                message: format!("Linker failed: {}", stderr),
            }),
            execution: None,
            output: String::new(),
        };
    }

    // Run the executable.
    let run_output = match Command::new(&exe_path)
        .env("LD_LIBRARY_PATH", lib_dir.to_str().unwrap())
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return AotSectionResult {
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
            };
        }
    };

    let exit_code = run_output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

    if !run_output.status.success() {
        return AotSectionResult {
            section_type: section_type.to_string(),
            name: None,
            typecheck,
            lowering,
            aot_compile: Some(AotCompileResult::Success),
            link: Some(LinkResult::Success),
            execution: Some(ExecutionResult::Error {
                message: format!("Exit code: {}", exit_code),
                exit_code: Some(exit_code),
            }),
            output: stderr,
        };
    }

    AotSectionResult {
        section_type: section_type.to_string(),
        name: None,
        typecheck,
        lowering,
        aot_compile: Some(AotCompileResult::Success),
        link: Some(LinkResult::Success),
        execution: Some(ExecutionResult::Success { exit_code }),
        output: stderr,
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
