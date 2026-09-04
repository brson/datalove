//! Driving the C backend: source, compiler, linker, executable.
//!
//! The cranelift backends hand back an object file, so `aot::link_object_to_path`
//! is all they need. This one hands back C, which has to reach a C compiler
//! before it reaches a linker, and there is more than one file because a module
//! becomes a translation unit. That is the whole of the difference.
//!
//! This lived inside `c_dual_tests` while the C backend had no other caller.
//! It is here so that anything wanting a C-compiled program asks for one the
//! same way, which is what keeps the backend from drifting again.

use std::path::{Path, PathBuf};
use std::process::Command;

use rmx::prelude::*;

use datalove_datafun_c_aot::{CAotCompiler, CAotError};
use datalove_datafun_ir::{FunctionRegistry, IrCodeUnit};

/// What went wrong between the IR and a runnable program.
#[derive(Debug)]
pub enum CAotBuildError {
    /// The backend could not turn the IR into C.
    Codegen(CAotError),
    /// A temporary directory for the sources could not be made.
    TempDir(std::io::Error),
    /// A source file could not be written.
    WriteSource(std::io::Error),
    /// The C compiler could not be run at all.
    CompilerExec(std::io::Error),
    /// The C compiler ran and rejected the program.
    ///
    /// Carries the sources, because generated C is not on disk to look at
    /// after this returns and the message alone names lines nobody has.
    CompilerFailed { stderr: String, sources: String },
}

impl std::fmt::Display for CAotBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CAotBuildError::Codegen(e) => write!(f, "c codegen: {}", e),
            CAotBuildError::TempDir(e) => write!(f, "temp dir: {}", e),
            CAotBuildError::WriteSource(e) => write!(f, "write c source: {}", e),
            CAotBuildError::CompilerExec(e) => write!(f, "run c compiler: {}", e),
            CAotBuildError::CompilerFailed { stderr, sources } => {
                write!(f, "c compiler failed:\n{}\n\nC sources:\n{}", stderr, sources)
            }
        }
    }
}

impl std::error::Error for CAotBuildError {}

/// The C a world compiles to, one entry per translation unit.
pub struct CSources {
    pub files: Vec<(String, String)>,
}

impl CSources {
    /// Every source, run together, for putting in an error message.
    pub fn concatenated(&self) -> String {
        self.files.iter()
            .map(|(name, body)| format!("// === {} ===\n{}\n", name, body))
            .collect()
    }
}

/// Compile a world to C.
pub fn compile_world(
    script_unit: &IrCodeUnit,
    registry: &FunctionRegistry,
) -> Result<CSources, CAotBuildError> {
    let mut compiler = CAotCompiler::new();
    let output = compiler.compile_world(script_unit, registry)
        .map_err(CAotBuildError::Codegen)?;
    Ok(CSources { files: output.files })
}

/// Compile and link C sources into an executable at `output_path`.
///
/// `extra_libs` is the native component when the program calls a rider, and
/// empty when it does not. Empty means the standalone runtime library, which
/// is the same choice `aot::link_object_to_path_with_libs` makes and for the
/// same reason: the component bundles the runtime, so the two never both go
/// on the command line.
pub fn link_sources_to_path(
    sources: &CSources,
    output_path: &Path,
    extra_libs: &[PathBuf],
) -> Result<(), CAotBuildError> {
    let dir = rmx::tempfile::tempdir().map_err(CAotBuildError::TempDir)?;

    let mut c_paths = Vec::new();
    for (filename, content) in &sources.files {
        let path = dir.path().join(filename);
        std::fs::write(&path, content).map_err(CAotBuildError::WriteSource)?;
        c_paths.push(path);
    }

    let mut cmd = Command::new("cc");
    cmd.args(["-std=c11", "-O0", "-g"]);
    if super::aot::use_lld() {
        cmd.arg("-fuse-ld=lld");
    }
    for path in &c_paths {
        cmd.arg(path);
    }

    if extra_libs.is_empty() {
        let lib_dir = super::aot::ensure_runtime_lib();
        cmd.arg(lib_dir.join("libdatalove_rt.a"));
    } else {
        for lib in extra_libs {
            cmd.arg(lib);
        }
    }

    cmd.args(["-ldl", "-lpthread", "-lm", "-o"]);
    cmd.arg(output_path);

    let output = cmd.output().map_err(CAotBuildError::CompilerExec)?;
    if !output.status.success() {
        return Err(CAotBuildError::CompilerFailed {
            stderr: String::from_utf8_lossy(&output.stderr).S(),
            sources: sources.concatenated(),
        });
    }
    Ok(())
}
