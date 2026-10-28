//! Compiling a script file the way the commands that run one do, and saying
//! what stopped it.

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use rmx::std::path::{Path, PathBuf};
use rmx::std::sync::Arc;

use datalove::bct::input::Source;
use datalove::bct::render::LineOffsets;
use datalove::datafun::pipeline::{
    CompiledModules, LoweringResult, ModuleCompilationPipeline, OwnershipResult,
    ScriptCompilationResult, ScriptCompiler, TypecheckResult, WorkspaceDescriptor,
};
use datalove::datafun::pipeline::rider_load::RiderNatives;
use datalove::datafun_ir::IrCodeUnit;

use crate::render;

/// A script file, compiled with the modules it requires.
///
/// Fields drop in the order they are declared, the compiler before what it
/// was compiled against.
pub struct CompiledScript<'db> {
    /// Kept so that it drops after the executor a caller makes: its
    /// compile-time interpreter checks for leaks as it drops.
    _compiler: ScriptCompiler<'db>,
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
    db: &'db datalove::datafun::Database,
    path: &Path,
    no_sys: bool,
) -> AnyResult<CompiledScript<'db>> {
    let cwd = rmx::std::env::current_dir().context("failed to read the current directory")?;
    let sys = datalove::sys_packages::system_library();
    let descriptor = WorkspaceDescriptor::for_command(&sys, no_sys, Some(path), &cwd)?
        .with_work_dir(datalove::paths::work_dir()?);

    // The script is what says where compilation starts.
    let source = rmx::std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read script file: {}", path.display()))?;

    let (mut pipeline, natives) = descriptor.to_pipeline_with_natives(db, &sys.natives);
    // Compile only what the script reaches. A program that requires two
    // stdlib modules should not pay for two dozen; `narrow_roots_to_script`
    // refuses when a require does not resolve, leaving the whole world for
    // resolution to report on.
    pipeline.narrow_roots_to_script(db, &source, &[]);
    let compiled = pipeline.compile(db);
    bail_on_module_errors(db, &compiled, &pipeline, &descriptor, path, &cwd)?;

    let mut compiler = compiled.script_compiler_default(db)
        .expect("script_compiler should succeed after error check");
    let unit = compiler.compile_fragment(&source);
    let files = descriptor.source_files(&pipeline);
    check_unit(&compiler, &unit, &files, path, &LineOffsets::new(), &cwd)?;
    let ir_unit = unit.ir_unit
        .ok_or_else(|| anyhow!("IR unit not available after lowering"))?;

    Ok(CompiledScript { _compiler: compiler, ir_unit, compiled, natives, descriptor })
}

/// Report what stopped the modules compiling, and fail, if anything did.
///
/// Parse, type and ownership errors are framed against the file each module
/// was read from. Lowering errors carry no spans and are printed as they are.
fn bail_on_module_errors<'db>(
    db: &'db datalove::datafun::Database,
    compiled: &CompiledModules<'db>,
    pipeline: &ModuleCompilationPipeline,
    descriptor: &WorkspaceDescriptor,
    script_path: &Path,
    cwd: &Path,
) -> AnyResult<()> {
    if !compiled.has_errors() {
        return Ok(());
    }

    let files = descriptor.source_files(pipeline);
    let locate = |source: Source| files.get(&source).cloned();

    let diagnostics: Vec<_> = compiled.get_module_parse_diagnostics(db).into_iter().map(|d| d.to_diagnostic(db))
        .chain(compiled.get_module_type_diagnostics(db).into_iter().map(|d| d.to_diagnostic(db)))
        .chain(compiled.get_module_ownership_diagnostics(db).into_iter().map(|d| d.to_diagnostic(db)))
        .collect();
    let rendered = !diagnostics.is_empty();
    render::render_module_diagnostics(db, diagnostics, &locate, script_path, &LineOffsets::new(), cwd);

    let unrendered = compiled.all_lowering_errors();
    for error in &unrendered {
        eprintln!("{error}");
    }
    // Nothing above says what went wrong, so the errors say it however they can.
    if !rendered && unrendered.is_empty() {
        for error in compiled.all_errors() {
            eprintln!("{error}");
        }
    }
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
    files: &HashMap<Source, PathBuf>,
    file_path: &Path,
    line_offsets: &LineOffsets,
    cwd: &Path,
) -> AnyResult<()> {
    if let TypecheckResult::ParseError { errors: _ } = &unit.typecheck {
        let parse_diags = compiler.get_parse_diagnostics();
        render::render_parse_diagnostics(compiler.db(), &parse_diags, file_path, line_offsets, cwd);
        bail!("Parse error");
    }
    if let TypecheckResult::Error { errors } = &unit.typecheck {
        // Typechecking a unit asks of the units before it, so its diagnostics
        // include theirs, which were reported with them. What is this unit's
        // points into its own text, or into a module or data file.
        let own = compiler.last_source().expect("a unit was compiled");
        let type_diags: Vec<_> = compiler.get_type_diagnostics().into_iter()
            .filter(|diag| {
                let diag = diag.to_diagnostic(compiler.db());
                diag.labels.is_empty()
                    || diag.labels.iter().any(|label| label.source == own || files.contains_key(&label.source))
            })
            .collect();
        let locate = |source: Source| files.get(&source).cloned();
        let diagnostics = type_diags.iter().map(|d| d.to_diagnostic(compiler.db())).collect();
        render::render_module_diagnostics(compiler.db(), diagnostics, &locate, file_path, line_offsets, cwd);
        // A diagnostic carries a span and renders itself. An error without
        // one -- a duplicate import or type alias, say -- renders nothing, and
        // a bare "type error" is all the user would otherwise get.
        if type_diags.is_empty() && !errors.is_empty() {
            bail!("Type error:\n{}", errors.join("\n"));
        }
        bail!("Type error");
    }
    if let OwnershipResult::Error { message: _ } = &unit.ownership {
        let diagnostics = compiler.get_ownership_diagnostics();
        render::render_ownership_diagnostics(compiler.db(), &diagnostics, file_path, line_offsets, cwd);
        bail!("Ownership error");
    }
    if let LoweringResult::Error { message } = &unit.lowering {
        bail!("Lowering error: {}", message);
    }
    Ok(())
}
