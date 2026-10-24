//! Turning datalove's errors into diagnostics, which `datalove::bct::render` prints.

use rmx::prelude::*;
use rmx::std::path::Path;

use datalove::bct::render::{self, LineOffsets, Renderer};
use datalove::bct::diagnostic::Diagnostic;
use datalove::bct::input::Source;

/// Render parse diagnostics to stderr.
///
/// `line_offsets` places sources that are part of `file_path` rather than the
/// whole of it, as a worldfile's sections are; see [`LineOffsets`].
pub fn render_parse_diagnostics<'db>(
    db: &'db dyn datalove::datafun::Db,
    diagnostics: &[&datalove::diagnostic::ParseDiagnostic],
    file_path: &Path,
    line_offsets: &LineOffsets,
    cwd: &Path,
) {
    let mut renderer = Renderer::with_line_offsets(line_offsets.C());
    for diagnostic in diagnostics {
        renderer.diagnostic(db, &diagnostic.to_diagnostic(db), file_path, cwd);
    }
}

/// Render type diagnostics to stderr.
pub fn render_type_diagnostics<'db>(
    db: &'db dyn datalove::datafun::Db,
    diagnostics: &[&datalove::diagnostic::TypeDiagnostic],
    file_path: &Path,
    cwd: &Path,
) {
    let diagnostics = diagnostics.iter().map(|d| d.to_diagnostic(db));
    render::render_diagnostics(db, diagnostics, file_path, cwd);
}

/// Render module diagnostics to stderr.
///
/// A worldfile's modules are separate sources, and a diagnostic about one can
/// point at another, so these are rendered against all of them at once. They
/// come stored, as an engine keeps them, and are read back against `db`.
pub fn render_stored_module_diagnostics<'db>(
    db: &'db dyn datalove::datafun::Db,
    diagnostics: &[datalove::bct::diagnostic::StoredDiagnostic],
    worldfile_path: &Path,
    line_offsets: &LineOffsets,
    cwd: &Path,
) {
    let mut renderer = Renderer::with_line_offsets(line_offsets.C());
    for diagnostic in diagnostics {
        renderer.multi_source_diagnostic(db, &diagnostic.to_diagnostic(db), worldfile_path, cwd);
    }
}

/// Render the diagnostics that stopped modules compiling.
///
/// Each is framed against the file `locate` says its text came from, falling
/// back to `file_path` -- the script, which is what the reader asked to run --
/// for one it does not know.
pub fn render_module_diagnostics<'db>(
    db: &'db dyn datalove::datafun::Db,
    diagnostics: Vec<Diagnostic<'db>>,
    locate: &dyn Fn(Source) -> Option<rmx::std::path::PathBuf>,
    file_path: &Path,
    line_offsets: &LineOffsets,
    cwd: &Path,
) {
    let mut renderer = Renderer::with_line_offsets(line_offsets.C());
    for diagnostic in diagnostics {
        renderer.located_diagnostic(db, &diagnostic, file_path, locate, cwd);
    }
}

/// Render ownership diagnostics to stderr.
pub fn render_ownership_diagnostics<'db>(
    db: &'db dyn datalove::datafun::Db,
    diagnostics: &[&datalove::diagnostic::OwnershipDiagnostic],
    file_path: &Path,
    line_offsets: &LineOffsets,
    cwd: &Path,
) {
    let mut renderer = Renderer::with_line_offsets(line_offsets.C());
    for diagnostic in diagnostics {
        renderer.diagnostic(db, &diagnostic.to_diagnostic(db), file_path, cwd);
    }
}
