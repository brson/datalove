//! Worldfile analysis with const inlining disabled.
//!
//! This module re-exports the analysis types from [`crate::worldfile_analysis`]
//! and provides [`analyze_worldfile_constlet`] which skips const inlining,
//! causing `const` bindings to be evaluated at runtime instead of compile time.
//!
//! This is useful for testing that runtime evaluation matches CTFE results.

use rmx::prelude::*;

use datalove_datafun_pkg::package_load_worldfile::ParsedWorldfile;

// Re-export types for backwards compatibility.
pub use crate::worldfile_analysis::{Analysis, SectionResult, AnalysisOptions};

/// Analyze a worldfile with const inlining disabled.
///
/// This is equivalent to calling [`crate::worldfile_analysis::analyze_worldfile_with_options`]
/// with `skip_const_inlining: true`.
///
/// When const inlining is disabled, `const` bindings in functions are lowered
/// as `let` bindings and evaluated at runtime rather than compile time. This
/// is useful for testing that runtime behavior matches compile-time evaluation.
pub fn analyze_worldfile_constlet(
    db: &mut crate::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Analysis> {
    let options = AnalysisOptions {
        skip_const_inlining: true,
    };
    crate::worldfile_analysis::analyze_worldfile_with_options(db, parsed, options)
}
