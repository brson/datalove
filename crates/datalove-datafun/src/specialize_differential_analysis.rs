//! Differential testing for comptime specialization.
//!
//! This module provides functions to compare execution results with and without
//! comptime specialization enabled. The outputs should be identical, verifying
//! that the union-branch transformation is semantically correct.
//!
//! # Usage
//!
//! ```ignore
//! let result = analyze_worldfile_differential_from_bytes(&mut db, file_bytes)?;
//! if !result.outputs_match {
//!     panic!("Specialization changed behavior: specialized vs unspecialized differ");
//! }
//! ```

use rmx::prelude::*;

use datalove_datafun_pkg::package_load_worldfile;

use crate::worldfile_analysis::{Analysis, AnalysisOptions};

/// Result of differential analysis comparing specialized vs unspecialized execution.
#[derive(Debug)]
pub struct DifferentialResult {
    /// Analysis result with specialization enabled (default).
    pub specialized: Analysis,
    /// Analysis result with specialization disabled.
    pub unspecialized: Analysis,
    /// Whether the debug outputs match for all sections.
    pub outputs_match: bool,
    /// Differences found between specialized and unspecialized outputs.
    pub differences: Vec<SectionDifference>,
}

/// A difference between specialized and unspecialized output for a section.
#[derive(Debug, Clone)]
pub struct SectionDifference {
    /// Section index in the results.
    pub section_index: usize,
    /// Section name (if available).
    pub section_name: Option<String>,
    /// Output from specialized execution.
    pub specialized_output: String,
    /// Output from unspecialized execution.
    pub unspecialized_output: String,
}

/// Analyze a worldfile twice from raw bytes: with and without specialization.
///
/// Uses separate database instances to avoid salsa caching conflicts.
/// Compares the debug outputs to verify that specialization doesn't change behavior.
/// Returns a `DifferentialResult` containing both analysis results and any differences.
pub fn analyze_worldfile_differential_from_bytes(
    _db: &mut crate::Database,
    file_bytes: &[u8],
) -> AnyResult<DifferentialResult> {
    // Parse the worldfile twice for independent runs.
    let parsed_specialized = package_load_worldfile::parse_worldfile_sections(file_bytes)?;
    let parsed_unspecialized = package_load_worldfile::parse_worldfile_sections(file_bytes)?;

    // Use fresh database for specialized run.
    let mut db_specialized = crate::Database::default();
    let options_specialized = AnalysisOptions {
        skip_const_inlining: false,
        skip_specialization: false,
    };
    let specialized = crate::worldfile_analysis::analyze_worldfile_with_options(
        &mut db_specialized,
        parsed_specialized,
        options_specialized,
    )?;

    // Use fresh database for unspecialized run.
    let mut db_unspecialized = crate::Database::default();
    let options_unspecialized = AnalysisOptions {
        skip_const_inlining: false,
        skip_specialization: true,
    };
    let unspecialized = crate::worldfile_analysis::analyze_worldfile_with_options(
        &mut db_unspecialized,
        parsed_unspecialized,
        options_unspecialized,
    )?;

    // Compare outputs.
    let mut differences = Vec::new();
    let mut outputs_match = true;

    // Compare section outputs.
    let max_len = specialized.sections.len().max(unspecialized.sections.len());
    for i in 0..max_len {
        let spec_section = specialized.sections.get(i);
        let unspec_section = unspecialized.sections.get(i);

        match (spec_section, unspec_section) {
            (Some(s), Some(u)) => {
                // Compare debug outputs (the main execution result).
                let spec_debug = s.debug_output.as_deref().unwrap_or("");
                let unspec_debug = u.debug_output.as_deref().unwrap_or("");

                if spec_debug != unspec_debug {
                    outputs_match = false;
                    differences.push(SectionDifference {
                        section_index: i,
                        section_name: s.name.clone(),
                        specialized_output: spec_debug.to_string(),
                        unspecialized_output: unspec_debug.to_string(),
                    });
                }

                // Also compare regular output.
                if s.output != u.output {
                    outputs_match = false;
                    differences.push(SectionDifference {
                        section_index: i,
                        section_name: s.name.clone(),
                        specialized_output: s.output.clone(),
                        unspecialized_output: u.output.clone(),
                    });
                }
            }
            (Some(s), None) => {
                outputs_match = false;
                differences.push(SectionDifference {
                    section_index: i,
                    section_name: s.name.clone(),
                    specialized_output: format!("[section exists]"),
                    unspecialized_output: format!("[section missing]"),
                });
            }
            (None, Some(u)) => {
                outputs_match = false;
                differences.push(SectionDifference {
                    section_index: i,
                    section_name: u.name.clone(),
                    specialized_output: format!("[section missing]"),
                    unspecialized_output: format!("[section exists]"),
                });
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(DifferentialResult {
        specialized,
        unspecialized,
        outputs_match,
        differences,
    })
}
