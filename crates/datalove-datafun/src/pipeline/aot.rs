//! AOT compilation: compile scripts to native executables via Cranelift.
//!
//! This module provides utilities for ahead-of-time compilation of IR units
//! to native code. The workflow is: compile IR to object file, link with the
//! native component, and optionally execute the result.
//!
//! # Functions
//!
//! - [`compile_script_to_object`]: Compile IR to object bytes.
//! - [`link_object_to_temp_executable`]: Link object to executable in temp dir.
//! - [`run_executable`]: Run an AOT-compiled executable.
//! - [`compile_link_run`]: Convenience function combining all steps.
//!
//! # Example
//!
//! ```ignore
//! use datalove_datafun::pipeline::aot;
//!
//! let obj_bytes = aot::compile_script_to_object(&ir_unit)?;
//! let (exe_path, _dir) = aot::link_object_to_temp_executable(&obj_bytes)?;
//! let output = aot::run_executable(&exe_path)?;
//! println!("stderr: {}", output.stderr);
//! ```

use rmx::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use datalove_datafun_cranelift_aot::AotCompiler;
use datalove_datafun_ir::{IrCodeUnit, FunctionRegistry};

use super::rider_build;

/// Linking error.
#[derive(Debug)]
pub enum LinkError {
    TempDir(std::io::Error),
    WriteObject(std::io::Error),
    Component(String),
    LinkerExec(std::io::Error),
    LinkerFailed(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::TempDir(e) => write!(f, "failed to create temp directory: {}", e),
            LinkError::WriteObject(e) => write!(f, "failed to write object file: {}", e),
            LinkError::Component(msg) => write!(f, "{}", msg),
            LinkError::LinkerExec(e) => write!(f, "failed to execute linker: {}", e),
            LinkError::LinkerFailed(msg) => write!(f, "linker failed: {}", msg),
        }
    }
}

impl std::error::Error for LinkError {}

/// Execution error.
#[derive(Debug)]
pub enum ExecError {
    Exec(std::io::Error),
    ExitCode { code: i32, stderr: String },
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Exec(e) => write!(f, "failed to execute: {}", e),
            ExecError::ExitCode { code, stderr } => write!(f, "exit code {}: {}", code, stderr),
        }
    }
}

impl std::error::Error for ExecError {}

/// Output from executing an AOT-compiled binary.
pub struct ExecOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Cached result of whether lld linker is available.
static USE_LLD: OnceLock<bool> = OnceLock::new();

/// Check if lld linker is available.
pub fn use_lld() -> bool {
    *USE_LLD.get_or_init(|| {
        // Check if lld is available by checking --version output
        Command::new("ld.lld")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// The component a program with no riders links, which is the runtime alone.
///
/// Built through the same path as a component with riders in it, so there is
/// one way a program acquires the runtime rather than two.
pub fn runtime_only_component() -> Result<PathBuf, LinkError> {
    rider_build::build_component_staticlib(&rider_build::default_work_dir(), &[])
        .map_err(|e| LinkError::Component(e.to_string()))
}

/// Compile a script unit to object bytes.
pub fn compile_script_to_object(unit: &IrCodeUnit) -> AnyResult<Vec<u8>> {
    let mut compiler = AotCompiler::new_for_host()
        .map_err(|e| anyhow!("failed to create AOT compiler: {}", e))?;
    let product = compiler.compile_script_unit(unit)
        .map_err(|e| anyhow!("AOT compilation failed: {}", e))?;
    let obj_bytes = product.emit()
        .map_err(|e| anyhow!("failed to emit object: {}", e))?;
    Ok(obj_bytes)
}

/// Compile a script unit with module code units to object bytes.
pub fn compile_script_to_object_with_world(
    unit: &IrCodeUnit,
    registry: &FunctionRegistry,
) -> AnyResult<Vec<u8>> {
    let mut compiler = AotCompiler::new_for_host()
        .map_err(|e| anyhow!("failed to create AOT compiler: {}", e))?;
    let product = compiler.compile_script_unit_in_world(unit, registry)
        .map_err(|e| anyhow!("AOT compilation failed: {}", e))?;
    let obj_bytes = product.emit()
        .map_err(|e| anyhow!("failed to emit object: {}", e))?;
    Ok(obj_bytes)
}

/// Link object bytes to an executable in a temp directory.
pub fn link_object_to_temp_executable(
    obj_bytes: &[u8],
) -> Result<(PathBuf, rmx::tempfile::TempDir), LinkError> {
    let dir = rmx::tempfile::tempdir().map_err(LinkError::TempDir)?;
    let exe_path = dir.path().join("script");
    link_object_to_path(obj_bytes, &exe_path)?;
    Ok((exe_path, dir))
}

/// Link object bytes to an executable at the specified path.
pub fn link_object_to_path(obj_bytes: &[u8], output_path: &Path) -> Result<(), LinkError> {
    link_object_to_path_with_libs(obj_bytes, output_path, &[])
}

/// Link object bytes to an executable, including extra libraries.
///
/// `extra_libs` is the native component the workspace built, which carries the
/// runtime along with the riders the program calls. Empty means the caller has
/// no workspace to have built one, so a rider-free component is used instead;
/// either way exactly one component goes on the command line, because each of
/// them carries the runtime.
pub fn link_object_to_path_with_libs(
    obj_bytes: &[u8],
    output_path: &Path,
    extra_libs: &[PathBuf],
) -> Result<(), LinkError> {
    let dir = rmx::tempfile::tempdir().map_err(LinkError::TempDir)?;
    let obj_path = dir.path().join("script.o");
    std::fs::write(&obj_path, obj_bytes).map_err(LinkError::WriteObject)?;

    // Use lld for faster linking if available.
    let mut cmd = Command::new("cc");
    if use_lld() {
        cmd.arg("-fuse-ld=lld");
    }
    cmd.arg(obj_path.to_str().unwrap());

    if extra_libs.is_empty() {
        cmd.arg(runtime_only_component()?);
    } else {
        for lib in extra_libs {
            cmd.arg(lib.to_str().unwrap());
        }
    }

    cmd.args(["-ldl", "-lpthread", "-lm", "-o", output_path.to_str().unwrap()]);

    let output = cmd.output().map_err(LinkError::LinkerExec)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(LinkError::LinkerFailed(stderr));
    }

    Ok(())
}

/// Run an AOT-compiled executable.
pub fn run_executable(exe_path: &Path) -> Result<ExecOutput, ExecError> {
    let output = Command::new(exe_path)
        .output()
        .map_err(ExecError::Exec)?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    if !output.status.success() {
        return Err(ExecError::ExitCode { code: exit_code, stderr });
    }

    Ok(ExecOutput { exit_code, stdout, stderr })
}

/// Compile, link, and run a script unit.
pub fn compile_link_run(unit: &IrCodeUnit) -> AnyResult<ExecOutput> {
    let obj_bytes = compile_script_to_object(unit)?;
    let (exe_path, _dir) = link_object_to_temp_executable(&obj_bytes)
        .map_err(|e| anyhow!("{}", e))?;
    run_executable(&exe_path).map_err(|e| anyhow!("{}", e))
}
