//! Compiling a script file the way the commands that run one do, and saying
//! what stopped it.

use rmx::prelude::*;
use bct::lookup::LookupMap;
use rmx::std::path::{Path, PathBuf};
use rmx::std::sync::Arc;

use datalove::bct::input::Source;
use datalove::bct::render::LineOffsets;
use datalove::datafun::driver::{
    CompiledModules, ModuleCompiler, OwnershipResult,
    ScriptCompilationResult, ScriptCompiler, TypecheckResult, WorkspaceDescriptor,
};
use datalove::datafun::driver::rider_load::RiderNatives;
use datalove::datafun_ir::IrCodeUnit;
use datalove::vio::Fs;

use crate::render;

/// A script file, compiled with the modules it requires.
///
/// Fields drop in the order they are declared, the compiler before what it
/// was compiled against.
pub struct CompiledScript<'db> {
    /// Kept so that it drops after the executor a caller makes: its
    /// compile-time interpreter checks for leaks as it drops.
    compiler: ScriptCompiler<'db>,
    pub ir_unit: IrCodeUnit,
    pub compiled: CompiledModules<'db>,
    /// The natives the modules were compiled with, for an executor to take
    /// the same ones.
    pub natives: Arc<RiderNatives>,
    pub descriptor: WorkspaceDescriptor,
}

/// Compile the script at `path` in the workspace a command finds for it.
///
/// Only the modules the script reaches are compiled, and what stops any of
/// them, or the script, is rendered before this fails.
///
/// The workspace gets this installation's work dir whether or not it is used:
/// with `DATALOVE_BUILD_SYS_RIDERS` set the riders are built rather than taken
/// from this binary, and that is where they build.
pub fn compile_script<'db>(
    fs: &dyn Fs,
    db: &'db datalove::datafun::Database,
    path: &Path,
    no_sys: bool,
) -> AnyResult<CompiledScript<'db>> {
    let cwd = fs.current_dir();
    let sys = datalove::sys_packages::system_library();
    let descriptor = WorkspaceDescriptor::for_command(fs, &sys, no_sys, Some(path))?
        .with_work_dir(datalove::paths::work_dir()?);

    // The script is what says where compilation starts.
    let source = fs.read_to_string(path)
        .with_context(|| format!("Failed to read script file: {}", path.display()))?;

    let (mut module_compiler, natives) = descriptor.to_compiler_with_natives(db, &sys.natives);
    // Compile only what the script reaches. A program that requires two
    // stdlib modules should not pay for two dozen; `narrow_roots_to_script`
    // refuses when a require does not resolve, leaving the whole world for
    // resolution to report on.
    module_compiler.narrow_roots_to_script(db, &source, &[]);
    let compiled = module_compiler.compile(db);
    bail_on_module_errors(db, &compiled, &module_compiler, &descriptor, path, cwd)?;

    let mut compiler = compiled.script_compiler_default(db)
        .expect("script_compiler should succeed after error check");
    let unit = compiler.compile_fragment(&source);
    let files = descriptor.source_files(&module_compiler);
    check_unit(&compiler, &unit, &files, path, &LineOffsets::default(), cwd)?;
    let ir_unit = unit.ir_unit
        .ok_or_else(|| anyhow!("IR unit not available after lowering"))?;

    Ok(CompiledScript { compiler, ir_unit, compiled, natives, descriptor })
}

impl<'db> CompiledScript<'db> {
    /// The compiler the script was compiled with.
    pub fn compiler(&self) -> &ScriptCompiler<'db> {
        &self.compiler
    }
}

/// Report what stopped the modules compiling, and fail, if anything did.
///
/// Parse, type and ownership errors are framed against the file each module
/// was read from.
fn bail_on_module_errors<'db>(
    db: &'db datalove::datafun::Database,
    compiled: &CompiledModules<'db>,
    module_compiler: &ModuleCompiler,
    descriptor: &WorkspaceDescriptor,
    script_path: &Path,
    cwd: &Path,
) -> AnyResult<()> {
    if !compiled.has_errors() {
        return Ok(());
    }

    let files = descriptor.source_files(module_compiler);
    let locate = |source: Source| files.get(&source).cloned();

    let diagnostics: Vec<_> = compiled.diagnostics(db).iter().map(|d| d.to_diagnostic(db)).collect();
    assert!(!diagnostics.is_empty(), "modules that failed to compile have something to say why");
    render::render_module_diagnostics(db, diagnostics, &locate, script_path, &LineOffsets::default(), cwd);
    bail!("Module compilation error");
}

/// Report why a script unit did not compile, if it did not, and fail saying
/// what stopped it.
///
/// `files` is where each module and data file was read from, by its source,
/// as [`WorkspaceDescriptor::source_files`] gives. `line_offsets` places
/// sources that are part of `file_path` rather than the whole of it, as a
/// worldfile's sections are.
///
/// An ownership failure has to be caught here like the rest. A command that
/// skips it gets as far as asking for the IR, finds none, and says "IR unit
/// not available after lowering" -- which names the step that produced
/// nothing rather than the analysis that refused to produce it.
pub fn check_unit(
    compiler: &ScriptCompiler<'_>,
    unit: &ScriptCompilationResult,
    files: &LookupMap<Source, PathBuf>,
    file_path: &Path,
    line_offsets: &LineOffsets,
    cwd: &Path,
) -> AnyResult<()> {
    let Some(diagnostics) = unit.diagnostics() else {
        return Ok(());
    };
    let locate = |source: Source| files.get(&source).cloned();
    let diagnostics = diagnostics.iter().map(|d| d.to_diagnostic(compiler.db())).collect();
    render::render_module_diagnostics(compiler.db(), diagnostics, &locate, file_path, line_offsets, cwd);
    match (&unit.typecheck, &unit.ownership) {
        (TypecheckResult::ParseError { .. }, _) => bail!("Parse error"),
        (TypecheckResult::Error { .. }, _) => bail!("Type error"),
        (_, OwnershipResult::Error { .. }) => bail!("Ownership error"),
        _ => bail!("Lowering error"),
    }
}
