//! AOT compilation: compile scripts to native executables via Cranelift.
//!
//! This module provides utilities for ahead-of-time compilation of IR units
//! to native code. The workflow is: compile IR to object file, link with the
//! runtime library, and optionally execute the result.
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
use std::sync::{OnceLock, atomic::{AtomicU64, Ordering}};
use std::time::Instant;

// Timing counters for profiling (only in debug builds)
#[cfg(debug_assertions)]
pub static COMPILE_TIME_NS: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
pub static LINK_TIME_NS: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
pub static LINK_CC_TIME_NS: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
pub static EXEC_TIME_NS: AtomicU64 = AtomicU64::new(0);
#[cfg(debug_assertions)]
pub static COMPILE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Print timing summary (call at end of test run).
#[cfg(debug_assertions)]
pub fn print_timing_summary() {
    let compile_ms = COMPILE_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let link_ms = LINK_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let link_cc_ms = LINK_CC_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let exec_ms = EXEC_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let count = COMPILE_COUNT.load(Ordering::Relaxed);
    eprintln!("AOT Timing: compile={}ms, link={}ms (cc={}ms), exec={}ms (count={})",
              compile_ms, link_ms, link_cc_ms, exec_ms, count);
}

use datalove_datafun_cranelift_aot::AotCompiler;
use datalove_datafun_ir::{IrCodeUnit, FunctionRegistry};

/// Linking error.
#[derive(Debug)]
pub enum LinkError {
    TempDir(std::io::Error),
    WriteObject(std::io::Error),
    RuntimeNotFound { debug_path: PathBuf, release_path: PathBuf },
    LinkerExec(std::io::Error),
    LinkerFailed(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::TempDir(e) => write!(f, "failed to create temp directory: {}", e),
            LinkError::WriteObject(e) => write!(f, "failed to write object file: {}", e),
            LinkError::RuntimeNotFound { debug_path, release_path } => {
                write!(f, "runtime library not found at {} or {}", debug_path.display(), release_path.display())
            }
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

static RUNTIME_LIB_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Cached result of whether lld linker is available.
static USE_LLD: OnceLock<bool> = OnceLock::new();

/// Check if lld linker is available.
fn use_lld() -> bool {
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

/// Ensure the runtime library is built and return its directory.
pub fn ensure_runtime_lib() -> &'static Path {
    RUNTIME_LIB_DIR.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .unwrap_or_else(|_| ".".to_string());
        let manifest_path = PathBuf::from(manifest_dir);
        let workspace_root = manifest_path.join("../..").canonicalize()
            .expect("failed to find workspace root");
        let lib_dir = workspace_root.join("target/debug");

        // Build quietly to avoid polluting test output.
        // Use index-64 feature if this crate was compiled with it.
        #[cfg(feature = "index-64")]
        let args = ["build", "-p", "datalove-rt", "--features", "index-64", "--quiet"];
        #[cfg(not(feature = "index-64"))]
        let args = ["build", "-p", "datalove-rt", "--quiet"];

        let output = Command::new("cargo")
            .args(args)
            .current_dir(&workspace_root)
            .output()
            .expect("failed to run cargo build");
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("Failed to build datalove-rt: {}", stderr);
        }

        lib_dir
    })
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
pub fn compile_script_to_object_with_world<'a>(
    unit: &IrCodeUnit,
    world_units: impl Iterator<Item = &'a IrCodeUnit>,
    registry: &FunctionRegistry,
) -> AnyResult<Vec<u8>> {
    #[cfg(debug_assertions)]
    let start = Instant::now();

    let mut compiler = AotCompiler::new_for_host()
        .map_err(|e| anyhow!("failed to create AOT compiler: {}", e))?;
    let product = compiler.compile_script_unit_with_world_types(unit, world_units, registry)
        .map_err(|e| anyhow!("AOT compilation failed: {}", e))?;
    let obj_bytes = product.emit()
        .map_err(|e| anyhow!("failed to emit object: {}", e))?;

    #[cfg(debug_assertions)]
    {
        COMPILE_TIME_NS.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        COMPILE_COUNT.fetch_add(1, Ordering::Relaxed);
    }

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
    #[cfg(debug_assertions)]
    let start = Instant::now();

    let dir = rmx::tempfile::tempdir().map_err(LinkError::TempDir)?;
    let obj_path = dir.path().join("script.o");
    std::fs::write(&obj_path, obj_bytes).map_err(LinkError::WriteObject)?;

    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    #[cfg(debug_assertions)]
    let cc_start = Instant::now();

    // Use lld for faster linking if available
    let mut cmd = Command::new("cc");
    if use_lld() {
        cmd.arg("-fuse-ld=lld");
    }
    cmd.args([
        obj_path.to_str().unwrap(),
        lib_path.to_str().unwrap(),
        "-ldl", "-lpthread", "-lm",
        "-o", output_path.to_str().unwrap(),
    ]);

    let output = cmd.output().map_err(LinkError::LinkerExec)?;

    #[cfg(debug_assertions)]
    LINK_CC_TIME_NS.fetch_add(cc_start.elapsed().as_nanos() as u64, Ordering::Relaxed);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(LinkError::LinkerFailed(stderr));
    }

    #[cfg(debug_assertions)]
    LINK_TIME_NS.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);

    Ok(())
}

/// Run an AOT-compiled executable.
pub fn run_executable(exe_path: &Path) -> Result<ExecOutput, ExecError> {
    #[cfg(debug_assertions)]
    let start = Instant::now();

    let output = Command::new(exe_path)
        .output()
        .map_err(ExecError::Exec)?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    #[cfg(debug_assertions)]
    EXEC_TIME_NS.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);

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

/// Compile, link, and run a script unit with module code units.
pub fn compile_link_run_with_world<'a>(
    unit: &IrCodeUnit,
    world_units: impl Iterator<Item = &'a IrCodeUnit>,
    registry: &FunctionRegistry,
) -> AnyResult<ExecOutput> {
    let obj_bytes = compile_script_to_object_with_world(unit, world_units, registry)?;
    let (exe_path, _dir) = link_object_to_temp_executable(&obj_bytes)
        .map_err(|e| anyhow!("{}", e))?;
    run_executable(&exe_path).map_err(|e| anyhow!("{}", e))
}
