//! IR serialization round-trip tests.
//!
//! This test suite validates that IR can be serialized to RON format and
//! deserialized with identical execution behavior. Tests:
//! 1. Lower worldfile scripts to IR
//! 2. Serialize IR to RON
//! 3. Deserialize IR
//! 4. Execute deserialized IR via interpreter and AOT
//! 5. Compare outputs against direct execution

use rmx::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use datalove_datafun as datafun;
use datalove_datafun_aot_cranelift::AotCompiler;
use datalove_datafun_interp::{Destination, IrInterpreter, ScriptEnvironment};
use datalove_datafun_ir::{FunctionRegistry, IrScriptUnit, IrType};
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_rt::rust::AlignedBuffer;

// ============================================================================
// Result Types
// ============================================================================

/// Result of IR serialization analysis.
#[derive(Debug, Serialize, Deserialize)]
pub struct IrSerialAnalysis {
    pub sections: Vec<IrSerialSectionResult>,
}

/// Result of analyzing one section with serialization round-trip.
#[derive(Debug, Serialize, Deserialize)]
pub struct IrSerialSectionResult {
    pub section_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Typecheck result.
    pub typecheck: datafun::pipeline::TypecheckResult,
    /// Lowering result.
    pub lowering: datafun::pipeline::LoweringResult,
    /// Direct interpreter output.
    pub direct_interp_output: String,
    /// Direct AOT output.
    pub direct_aot_output: String,
    /// Size of serialized IR in bytes.
    pub serialized_ir_size: usize,
    /// Size of serialized registry in bytes.
    pub serialized_registry_size: usize,
    /// Whether deserialization succeeded.
    pub deser_success: bool,
    /// Deserialization error if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deser_error: Option<String>,
    /// Deserialized interpreter output.
    pub deser_interp_output: String,
    /// Deserialized AOT output.
    pub deser_aot_output: String,
    /// Whether interpreter outputs match.
    pub interp_output_match: bool,
    /// Whether AOT outputs match.
    pub aot_output_match: bool,
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

// ============================================================================
// Runtime Library Setup
// ============================================================================

static RUNTIME_LIB_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Build the runtime library once and return the path to the lib directory.
fn ensure_runtime_lib() -> &'static Path {
    RUNTIME_LIB_DIR.get_or_init(|| {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        let manifest_path = std::path::PathBuf::from(manifest_dir);
        let workspace_root = manifest_path
            .join("../..")
            .canonicalize()
            .expect("failed to find workspace root");
        let lib_dir = workspace_root.join("target/debug");

        let status = Command::new("cargo")
            .args(["build", "-p", "datalove-rt"])
            .current_dir(&workspace_root)
            .status()
            .expect("failed to run cargo build");
        if !status.success() {
            panic!("Failed to build datalove-rt");
        }

        lib_dir
    })
}

// ============================================================================
// Execution Helpers
// ============================================================================

/// Execute an IR unit via interpreter, returning debug output.
fn execute_ir_interp(ir_unit: &IrScriptUnit, registry: &FunctionRegistry) -> String {
    let mut interp = IrInterpreter::new_with_debug_mode(datalove_rt::c::DebugOutputMode::Buffer);
    interp.clear_debug_buffer();

    let mut env = ScriptEnvironment::new();

    // Copy module functions from registry to env.
    for ((module_id, func_id), func) in registry.iter_module_functions_with_ids() {
        env.add_module_function(module_id, func_id, func.clone());
    }

    // Set up return destination.
    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table_mut().get_or_create(&ret_type);
    let (ret_size, ret_align) = unsafe { ((*ret_tydesc).size, (*ret_tydesc).align) };
    let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
    let ret_dest = Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    let _ = interp.execute_script_unit_in_env(ir_unit, &mut env, ret_dest, None);

    let output = interp.get_debug_buffer();
    env.destroy_all(interp.runtime_handle());
    output
}

/// Execute an IR unit via AOT, returning debug output and status.
fn execute_ir_aot(
    ir_unit: &IrScriptUnit,
    registry: &FunctionRegistry,
) -> (String, AotCompileResult, LinkResult, ExecutionResult) {
    // Create AOT compiler.
    let mut compiler = match AotCompiler::new_for_host() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed to create AOT compiler: {}", e);
            return (
                String::new(),
                AotCompileResult::Error { message: msg },
                LinkResult::Skipped {
                    reason: "AOT compile failed".to_string(),
                },
                ExecutionResult::Skipped {
                    reason: "AOT compile failed".to_string(),
                },
            );
        }
    };

    // Compile with world types.
    let product = match compiler.compile_script_unit_with_world_types(
        ir_unit,
        registry.iter_all_functions(),
        registry,
    ) {
        Ok(p) => p,
        Err(e) => {
            return (
                String::new(),
                AotCompileResult::Error {
                    message: format!("{}", e),
                },
                LinkResult::Skipped {
                    reason: "AOT compile failed".to_string(),
                },
                ExecutionResult::Skipped {
                    reason: "AOT compile failed".to_string(),
                },
            );
        }
    };

    let obj_bytes = match product.emit() {
        Ok(b) => b,
        Err(e) => {
            return (
                String::new(),
                AotCompileResult::Error {
                    message: format!("Failed to emit: {}", e),
                },
                LinkResult::Skipped {
                    reason: "AOT compile failed".to_string(),
                },
                ExecutionResult::Skipped {
                    reason: "AOT compile failed".to_string(),
                },
            );
        }
    };

    // Write object to temp file.
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => {
            return (
                String::new(),
                AotCompileResult::Success,
                LinkResult::Error {
                    message: format!("tempdir: {}", e),
                },
                ExecutionResult::Skipped {
                    reason: "Link failed".to_string(),
                },
            );
        }
    };

    let obj_path = dir.path().join("test.o");
    if let Err(e) = std::fs::write(&obj_path, &obj_bytes) {
        return (
            String::new(),
            AotCompileResult::Success,
            LinkResult::Error {
                message: format!("write obj: {}", e),
            },
            ExecutionResult::Skipped {
                reason: "Link failed".to_string(),
            },
        );
    }

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    if !lib_path.exists() {
        return (
            String::new(),
            AotCompileResult::Success,
            LinkResult::Skipped {
                reason: format!("Runtime library not found at {:?}", lib_path),
            },
            ExecutionResult::Skipped {
                reason: "Link skipped".to_string(),
            },
        );
    }

    // Link with cc.
    let exe_path = dir.path().join("test");
    let link_output = Command::new("cc")
        .args([
            obj_path.to_str().unwrap(),
            lib_path.to_str().unwrap(),
            "-ldl",
            "-lpthread",
            "-lm",
            "-o",
            exe_path.to_str().unwrap(),
        ])
        .output();

    let link_output = match link_output {
        Ok(o) => o,
        Err(e) => {
            return (
                String::new(),
                AotCompileResult::Success,
                LinkResult::Error {
                    message: format!("linker exec: {}", e),
                },
                ExecutionResult::Skipped {
                    reason: "Link failed".to_string(),
                },
            );
        }
    };

    if !link_output.status.success() {
        let stderr = String::from_utf8_lossy(&link_output.stderr);
        return (
            String::new(),
            AotCompileResult::Success,
            LinkResult::Error {
                message: format!("linker: {}", stderr),
            },
            ExecutionResult::Skipped {
                reason: "Link failed".to_string(),
            },
        );
    }

    // Run the executable.
    let run_output = match Command::new(&exe_path).output() {
        Ok(o) => o,
        Err(e) => {
            return (
                String::new(),
                AotCompileResult::Success,
                LinkResult::Success,
                ExecutionResult::Error {
                    message: format!("exec: {}", e),
                    exit_code: None,
                },
            );
        }
    };

    let exit_code = run_output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

    if !run_output.status.success() {
        return (
            stderr.clone(),
            AotCompileResult::Success,
            LinkResult::Success,
            ExecutionResult::Error {
                message: format!("exit {}", exit_code),
                exit_code: Some(exit_code),
            },
        );
    }

    (
        stderr,
        AotCompileResult::Success,
        LinkResult::Success,
        ExecutionResult::Success { exit_code },
    )
}

// ============================================================================
// Main Analysis
// ============================================================================

fn analyze_worldfile_ir_serial(
    parsed: package_load_worldfile::ParsedWorldfile,
) -> IrSerialAnalysis {
    let db = datafun::Database::default();
    let mut results = Vec::new();

    // Validate: exactly one scriptunit-fragment.
    let fragment_count = parsed
        .sections
        .iter()
        .filter(|s| matches!(s, WorldfileSection::ScriptFragment { .. }))
        .count();
    let expr_count = parsed
        .sections
        .iter()
        .filter(|s| matches!(s, WorldfileSection::ScriptExpr { .. }))
        .count();

    if fragment_count != 1 {
        results.push(IrSerialSectionResult {
            section_type: "validation".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![format!(
                    "Expected 1 scriptunit-fragment, found {}",
                    fragment_count
                )],
            },
            lowering: datafun::pipeline::LoweringResult::Skipped,
            direct_interp_output: String::new(),
            direct_aot_output: String::new(),
            serialized_ir_size: 0,
            serialized_registry_size: 0,
            deser_success: false,
            deser_error: None,
            deser_interp_output: String::new(),
            deser_aot_output: String::new(),
            interp_output_match: false,
            aot_output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return IrSerialAnalysis { sections: results };
    }

    if expr_count > 0 {
        results.push(IrSerialSectionResult {
            section_type: "validation".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![format!(
                    "scriptunit-expr sections not allowed, found {}",
                    expr_count
                )],
            },
            lowering: datafun::pipeline::LoweringResult::Skipped,
            direct_interp_output: String::new(),
            direct_aot_output: String::new(),
            serialized_ir_size: 0,
            serialized_registry_size: 0,
            deser_success: false,
            deser_error: None,
            deser_interp_output: String::new(),
            deser_aot_output: String::new(),
            interp_output_match: false,
            aot_output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return IrSerialAnalysis { sections: results };
    }

    // Find the fragment source.
    let fragment_source = parsed
        .sections
        .iter()
        .find_map(|s| match s {
            WorldfileSection::ScriptFragment { source } => Some(source.as_str()),
            _ => None,
        })
        .unwrap();

    // Build pipeline and compile.
    let mut pipeline =
        datafun::pipeline::ModuleCompilationPipeline::from_sections(&db, &parsed.sections);
    let compiled = pipeline.compile_fresh(&db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(IrSerialSectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: datafun::pipeline::TypecheckResult::Error {
                errors: vec![err.clone()],
            },
            lowering: datafun::pipeline::LoweringResult::Skipped,
            direct_interp_output: String::new(),
            direct_aot_output: String::new(),
            serialized_ir_size: 0,
            serialized_registry_size: 0,
            deser_success: false,
            deser_error: None,
            deser_interp_output: String::new(),
            deser_aot_output: String::new(),
            interp_output_match: false,
            aot_output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return IrSerialAnalysis { sections: results };
    }

    // Lower for AOT (includes drops).
    let mut ctx = compiled.script_context(&db, datalove_rt::c::DebugOutputMode::Disabled);
    let lower_result = ctx.lower_fragment_for_aot(fragment_source);

    // Get registry before destroying ctx.
    let registry = ctx.env.registry.clone();
    ctx.destroy_all();

    // Check for typecheck/lowering errors.
    if !matches!(
        &lower_result.typecheck,
        datafun::pipeline::TypecheckResult::Success
    ) {
        results.push(IrSerialSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: lower_result.typecheck,
            lowering: lower_result.lowering,
            direct_interp_output: String::new(),
            direct_aot_output: String::new(),
            serialized_ir_size: 0,
            serialized_registry_size: 0,
            deser_success: false,
            deser_error: None,
            deser_interp_output: String::new(),
            deser_aot_output: String::new(),
            interp_output_match: false,
            aot_output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return IrSerialAnalysis { sections: results };
    }

    if !matches!(
        &lower_result.lowering,
        datafun::pipeline::LoweringResult::Success { .. }
    ) {
        results.push(IrSerialSectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: lower_result.typecheck,
            lowering: lower_result.lowering,
            direct_interp_output: String::new(),
            direct_aot_output: String::new(),
            serialized_ir_size: 0,
            serialized_registry_size: 0,
            deser_success: false,
            deser_error: None,
            deser_interp_output: String::new(),
            deser_aot_output: String::new(),
            interp_output_match: false,
            aot_output_match: false,
            aot_compile: None,
            link: None,
            execution: None,
        });
        return IrSerialAnalysis { sections: results };
    }

    let ir_unit = match lower_result.ir_unit {
        Some(unit) => unit,
        None => {
            results.push(IrSerialSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: datafun::pipeline::LoweringResult::Error {
                    message: "No IR unit produced".to_string(),
                },
                direct_interp_output: String::new(),
                direct_aot_output: String::new(),
                serialized_ir_size: 0,
                serialized_registry_size: 0,
                deser_success: false,
                deser_error: None,
                deser_interp_output: String::new(),
                deser_aot_output: String::new(),
                interp_output_match: false,
                aot_output_match: false,
                aot_compile: None,
                link: None,
                execution: None,
            });
            return IrSerialAnalysis { sections: results };
        }
    };

    // Direct execution.
    let direct_interp_output = execute_ir_interp(&ir_unit, &registry);
    let (direct_aot_output, _, _, _) = execute_ir_aot(&ir_unit, &registry);

    // Serialize IR to RON.
    let serialized_ir = match ir_unit.to_ron() {
        Ok(s) => s,
        Err(e) => {
            results.push(IrSerialSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: lower_result.lowering,
                direct_interp_output,
                direct_aot_output,
                serialized_ir_size: 0,
                serialized_registry_size: 0,
                deser_success: false,
                deser_error: Some(format!("IR serialization failed: {}", e)),
                deser_interp_output: String::new(),
                deser_aot_output: String::new(),
                interp_output_match: false,
                aot_output_match: false,
                aot_compile: None,
                link: None,
                execution: None,
            });
            return IrSerialAnalysis { sections: results };
        }
    };

    let serialized_ir_size = serialized_ir.len();

    // Serialize registry to RON.
    let serialized_registry = match registry.to_ron() {
        Ok(s) => s,
        Err(e) => {
            results.push(IrSerialSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: lower_result.lowering,
                direct_interp_output,
                direct_aot_output,
                serialized_ir_size,
                serialized_registry_size: 0,
                deser_success: false,
                deser_error: Some(format!("Registry serialization failed: {}", e)),
                deser_interp_output: String::new(),
                deser_aot_output: String::new(),
                interp_output_match: false,
                aot_output_match: false,
                aot_compile: None,
                link: None,
                execution: None,
            });
            return IrSerialAnalysis { sections: results };
        }
    };

    let serialized_registry_size = serialized_registry.len();

    // Deserialize IR.
    let deser_ir_unit = match IrScriptUnit::from_ron(&serialized_ir) {
        Ok(unit) => unit,
        Err(e) => {
            results.push(IrSerialSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: lower_result.lowering,
                direct_interp_output,
                direct_aot_output,
                serialized_ir_size,
                serialized_registry_size,
                deser_success: false,
                deser_error: Some(format!("IR deserialization failed: {}", e)),
                deser_interp_output: String::new(),
                deser_aot_output: String::new(),
                interp_output_match: false,
                aot_output_match: false,
                aot_compile: None,
                link: None,
                execution: None,
            });
            return IrSerialAnalysis { sections: results };
        }
    };

    // Deserialize registry.
    let deser_registry = match FunctionRegistry::from_ron(&serialized_registry) {
        Ok(reg) => reg,
        Err(e) => {
            results.push(IrSerialSectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: lower_result.typecheck,
                lowering: lower_result.lowering,
                direct_interp_output,
                direct_aot_output,
                serialized_ir_size,
                serialized_registry_size,
                deser_success: false,
                deser_error: Some(format!("Registry deserialization failed: {}", e)),
                deser_interp_output: String::new(),
                deser_aot_output: String::new(),
                interp_output_match: false,
                aot_output_match: false,
                aot_compile: None,
                link: None,
                execution: None,
            });
            return IrSerialAnalysis { sections: results };
        }
    };

    // Deserialized execution - using deserialized registry.
    let deser_interp_output = execute_ir_interp(&deser_ir_unit, &deser_registry);
    let (deser_aot_output, aot_compile, link, execution) =
        execute_ir_aot(&deser_ir_unit, &deser_registry);

    // Compare.
    let interp_output_match = direct_interp_output == deser_interp_output;
    let aot_output_match = direct_aot_output == deser_aot_output;

    results.push(IrSerialSectionResult {
        section_type: "scriptunit-fragment".to_string(),
        name: None,
        typecheck: lower_result.typecheck,
        lowering: lower_result.lowering,
        direct_interp_output,
        direct_aot_output,
        serialized_ir_size,
        serialized_registry_size,
        deser_success: true,
        deser_error: None,
        deser_interp_output,
        deser_aot_output,
        interp_output_match,
        aot_output_match,
        aot_compile: Some(aot_compile),
        link: Some(link),
        execution: Some(execution),
    });

    IrSerialAnalysis { sections: results }
}

fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    let analysis = analyze_worldfile_ir_serial(parsed);

    // Check for failures.
    for section in &analysis.sections {
        if !section.deser_success {
            return Err(format!(
                "Deserialization failed: {:?}",
                section.deser_error
            ));
        }
        if !section.interp_output_match {
            return Err(format!(
                "Interpreter output mismatch:\n  Direct: {:?}\n  Deser: {:?}",
                section.direct_interp_output, section.deser_interp_output
            ));
        }
        if !section.aot_output_match {
            return Err(format!(
                "AOT output mismatch:\n  Direct: {:?}\n  Deser: {:?}",
                section.direct_aot_output, section.deser_aot_output
            ));
        }
    }

    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    ron::ser::to_string_pretty(&analysis, ron_config)
        .map_err(|e| format!("Failed to serialize result: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("ir_serial")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
