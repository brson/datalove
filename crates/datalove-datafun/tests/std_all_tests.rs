//! Standard library tests across all backends.
//!
//! Runs each `.dfs` fixture through all three backends (interpreter, JIT, AOT)
//! and verifies they produce the same output. Uses the same fixtures and expected
//! output as `std_tests`.

use rmx::prelude::*;
use std::path::{Path, PathBuf};
use datalove_datafun as datafun;
use datalove_datafun_cranelift_jit::JitEngine;
use datalove_datafun_interp::CallDispatcher;
use datafun::pipeline::{
    WorkspaceDescriptor, TypecheckResult, LoweringResult,
    aot as pipeline_aot,
};

/// Load the package world, set up the pipeline, and compile modules.
fn setup_and_compile(db: &datafun::Database) -> Result<(
    WorkspaceDescriptor,
    datafun::pipeline::CompiledModules<'_>,
), String> {
    let descriptor = rmx::futures::executor::block_on(
        WorkspaceDescriptor::load_default_sys()
    ).map_err(|e| format!("Failed to load package world: {}", e))?;

    let mut pipeline = descriptor.to_pipeline(db);
    let compiled = pipeline.compile_fresh(db);

    if let Some(err) = &compiled.resolution_error {
        return Err(format!("Package resolution error: {}", err));
    }
    for (path, errors) in &compiled.path_to_errors {
        if !errors.is_empty() {
            return Err(format!("Typecheck errors in {}: {:?}", path, errors));
        }
    }

    Ok((descriptor, compiled))
}

/// Build the unified native component and load it into the given executor.
/// Returns the static library path (for AOT linking) and loaded rider handle.
fn build_and_load_riders(
    descriptor: &WorkspaceDescriptor,
    compiled: &datafun::pipeline::CompiledModules<'_>,
    executor: &mut datafun::pipeline::ScriptExecutor,
) -> Result<(Vec<PathBuf>, Vec<datafun::pipeline::rider_load::LoadedRider>), String> {
    let rider_crate_dirs = descriptor.rider_crate_dirs();
    let build_result = datafun::pipeline::rider_build::build_native_component(&rider_crate_dirs)
        .map_err(|e| format!("rider build error: {}", e))?;

    let lib_paths = vec![build_result.staticlib_path.clone()];
    let native_symbols = compiled.native_symbols();
    let mut loaded_riders = Vec::new();

    if !native_symbols.is_empty() {
        let loaded = datafun::pipeline::rider_load::load_rider_library(
            &build_result.cdylib_path,
            "native-component",
            &native_symbols,
            executor.native_table_mut(),
        ).map_err(|e| format!("rider load error: {}", e))?;
        loaded_riders.push(loaded);
    }

    Ok((lib_paths, loaded_riders))
}

/// Run a script through the interpreter, compiling fragment then evaluating `output`.
fn run_with_executor(
    db: &datafun::Database,
    compiled: &datafun::pipeline::CompiledModules<'_>,
    descriptor: &WorkspaceDescriptor,
    script_text: &str,
    jit: Option<Box<dyn CallDispatcher>>,
    backend_name: &str,
) -> Result<(String, Vec<PathBuf>), String> {
    let mut compiler = compiled.script_compiler_default(db)
        .ok_or(format!("{}: module compilation failed (compiler)", backend_name))?;
    let mut executor = compiled.script_executor(datalove_rt::c::DebugOutputMode::Disabled, jit)
        .ok_or(format!("{}: module compilation failed (executor)", backend_name))?;

    let (lib_paths, loaded_riders) = build_and_load_riders(descriptor, compiled, &mut executor)?;

    // Register native rider symbols with JIT engine if present.
    if let Some(dispatcher) = executor.take_dispatcher() {
        if let Some(jit_engine) = dispatcher.as_any().downcast_ref::<JitEngine>() {
            for rider in &loaded_riders {
                for (symbol, ptr) in &rider.native_fn_ptrs {
                    jit_engine.register_native_symbol(symbol, *ptr);
                }
            }
        }
        executor.set_dispatcher(dispatcher);
    }

    // Compile and execute the fragment.
    let result = compiler.compile_fragment(script_text);
    if let TypecheckResult::Error { errors } = &result.typecheck {
        executor.destroy_live_values();
        return Err(format!("{} typecheck errors: {:?}", backend_name, errors));
    }
    if let LoweringResult::Error { message } = &result.lowering {
        executor.destroy_live_values();
        return Err(format!("{} lowering error: {}", backend_name, message));
    }
    if let Some(ir_unit) = &result.ir_unit {
        let output = executor.execute_fragment(ir_unit);
        if output.starts_with("Error:") {
            executor.destroy_live_values();
            return Err(format!("{} execution error: {}", backend_name, output));
        }
    }

    // Evaluate the output variable.
    let output_compiled = compiler.compile_expr("output");
    if let TypecheckResult::Error { errors } = &output_compiled.typecheck {
        executor.destroy_live_values();
        return Err(format!("{} output typecheck errors: {:?}", backend_name, errors));
    }
    if let LoweringResult::Error { message } = &output_compiled.lowering {
        executor.destroy_live_values();
        return Err(format!("{} output lowering error: {}", backend_name, message));
    }
    let output = if let Some(ir_unit) = &output_compiled.ir_unit {
        let (_, value) = executor.execute_expr(ir_unit);
        value
    } else {
        String::new()
    };
    executor.destroy_live_values();

    if output.starts_with("Error:") {
        return Err(format!("{} output error: {}", backend_name, output));
    }

    Ok((output, lib_paths))
}

/// Run through the AOT backend, returning the debuglog output.
///
/// Appends `debuglog output` to the script source so the AOT binary prints the
/// output value to stderr, then compiles, links (with rider libs), and runs it.
fn run_aot(
    db: &datafun::Database,
    compiled: &datafun::pipeline::CompiledModules<'_>,
    script_text: &str,
    rider_lib_paths: &[PathBuf],
) -> Result<String, String> {
    let modified_source = format!("{}\ndebuglog output", script_text);

    let mut compiler = compiled.script_compiler_default(db)
        .ok_or("AOT: module compilation failed (compiler)")?;

    let result = compiler.compile_fragment(&modified_source);
    if let TypecheckResult::Error { errors } = &result.typecheck {
        return Err(format!("AOT typecheck errors: {:?}", errors));
    }
    if let LoweringResult::Error { message } = &result.lowering {
        return Err(format!("AOT lowering error: {}", message));
    }

    let ir_unit = result.ir_unit
        .ok_or("AOT: IR unit not available")?;

    let registry = compiled.module_registry();

    // AOT compile with world types.
    let obj_bytes = pipeline_aot::compile_script_to_object_with_world(
        &ir_unit,
        registry.iter_all_code_units(),
        &registry,
    ).map_err(|e| format!("AOT compile error: {}", e))?;

    // Link with rider shared libraries.
    let dir = rmx::tempfile::tempdir()
        .map_err(|e| format!("AOT temp dir error: {}", e))?;
    let exe_path = dir.path().join("test");
    pipeline_aot::link_object_to_path_with_libs(&obj_bytes, &exe_path, rider_lib_paths)
        .map_err(|e| format!("AOT link error: {}", e))?;

    // Run and capture stderr (debuglog output).
    let exec_output = pipeline_aot::run_executable(&exe_path)
        .map_err(|e| format!("AOT execution error: {}", e))?;

    // Debuglog appends a newline; trim to match execute_expr format.
    Ok(exec_output.stderr.trim_end().to_string())
}

/// Analyze a single .dfs file across all three backends.
fn analyze_file(path: &Path) -> Result<String, String> {
    let script_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Backend 1: Interpreter.
    let (interp_value, rider_lib_paths) = {
        let db = datafun::Database::default();
        let (descriptor, compiled) = setup_and_compile(&db)?;
        run_with_executor(&db, &compiled, &descriptor, &script_text, None, "Interp")?
    };

    // Backend 2: JIT (in spawned thread for Cranelift PIE workaround).
    let jit_result: Result<String, String> = {
        let script_for_jit = script_text.clone();
        std::thread::spawn(move || -> Result<String, String> {
            let db = datafun::Database::default();
            let (descriptor, compiled) = setup_and_compile(&db)?;
            let jit = JitEngine::new(1).map_err(|e| format!("JIT engine creation failed: {}", e))?;
            let (value, _) = run_with_executor(&db, &compiled, &descriptor, &script_for_jit, Some(Box::new(jit)), "JIT")?;
            Ok(value)
        }).join().unwrap_or_else(|panic| {
            let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            Err(format!("JIT thread panicked: {}", msg))
        })
    };

    // Backend 3: AOT.
    let aot_result = {
        let db = datafun::Database::default();
        let (ref _descriptor, ref compiled) = setup_and_compile(&db)?;
        run_aot(&db, compiled, &script_text, &rider_lib_paths)
    };

    // Compare all backends against interpreter. All errors are fatal.
    let mut mismatches = Vec::new();
    match &jit_result {
        Ok(jit_value) if jit_value != &interp_value => {
            mismatches.push(format!(
                "JIT output mismatch:\n  interp: {}\n  jit: {}",
                interp_value, jit_value
            ));
        }
        Err(e) => {
            mismatches.push(format!("JIT error: {}", e));
        }
        _ => {}
    }
    match &aot_result {
        Ok(aot_value) if aot_value != &interp_value => {
            mismatches.push(format!(
                "AOT output mismatch:\n  interp: {}\n  aot: {}",
                interp_value, aot_value
            ));
        }
        Err(e) => {
            mismatches.push(format!("AOT error: {}", e));
        }
        _ => {}
    }
    if !mismatches.is_empty() {
        return Err(mismatches.join("\n"));
    }

    Ok(interp_value)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("std_tests")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
