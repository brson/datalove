//! Worldfile analysis with skip_const_inlining mode enabled.
//!
//! This module provides the same functionality as `worldfile_analysis` but
//! with the skip_const_inlining mode enabled, causing const bindings in functions
//! to be lowered as let bindings instead of being evaluated at compile time.
//!
//! This is useful for testing runtime behavior of const-like values.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};

use crate::pipeline::{
    ModuleCompilationPipeline, TypecheckResult, OwnershipResult, LoweringResult,
    format_ownership_result, format_lowering_result,
};

/// Result of analyzing a worldfile with skip_const_inlining mode.
#[derive(Debug, Serialize, Deserialize)]
pub struct Analysis {
    /// Per-section results.
    pub sections: Vec<SectionResult>,
}

/// Result of analyzing one section.
#[derive(Debug, Serialize, Deserialize)]
pub struct SectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    pub name: Option<String>,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Ownership analysis result.
    pub ownership: OwnershipResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value (for expression units) or function call result.
    pub output: String,
    /// Debug log output (from debuglog statements).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_output: Option<String>,
}

/// Analyze a worldfile with skip_const_inlining mode enabled.
///
/// This is similar to `worldfile_analysis::analyze_worldfile` but with
/// skip_const_inlining mode enabled for script functions.
pub fn analyze_worldfile_constlet(
    db: &mut crate::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Analysis> {
    let mut results = Vec::new();

    // Build pipeline from sections.
    let mut pipeline = ModuleCompilationPipeline::from_sections(db, &parsed.sections);

    // Enable skip_const_inlining mode for module function-level consts.
    pipeline.set_skip_const_inlining(true);

    // Compile modules (typecheck, drop analysis, lower).
    let (compiled, db) = pipeline.compile(db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(SectionResult {
            section_type: "resolution".S(),
            name: None,
            typecheck: TypecheckResult::Error { errors: vec![err.clone()] },
            ownership: OwnershipResult::Skipped,
            lowering: LoweringResult::Skipped,
            output: String::new(),
            debug_output: None,
        });
        return Ok(Analysis { sections: results });
    }

    // Collect module results first.
    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, .. } = section {
            let module_path = format!("{}/{}/{}", library, package, module);

            // Look up typecheck errors for this module.
            let typecheck = match compiled.path_to_errors.get(&module_path) {
                Some(errors) if !errors.is_empty() => {
                    TypecheckResult::Error { errors: errors.clone() }
                }
                _ => TypecheckResult::Success,
            };

            // Look up analysis results for this module.
            let has_typecheck_errors = matches!(&typecheck, TypecheckResult::Error { .. });
            let ir_dumps = compiled.module_ir_dumps.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let ownership_errs = compiled.ownership_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let lowering_errs = compiled.lowering_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let ownership = format_ownership_result(ownership_errs, has_typecheck_errors);
            let has_ownership_errors = matches!(&ownership, OwnershipResult::Error { .. });
            let lowering = format_lowering_result(ir_dumps, lowering_errs, has_typecheck_errors || has_ownership_errors);

            results.push(SectionResult {
                section_type: "module".S(),
                name: Some(module_path),
                typecheck,
                ownership,
                lowering,
                output: String::new(),
                debug_output: None,
            });
        }
    }

    // Create script compiler and executor with skip_const_inlining mode enabled.
    let mut compiler = compiled.script_compiler_default(db);
    let mut executor = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, None);

    // Enable skip_const_inlining mode on the compiler.
    if let Some(ref mut compiler) = compiler {
        compiler.set_skip_const_inlining(true);
    }

    // Process script units using the compiler and executor.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Already handled above.
            }
            WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {
                // Module action sections are for memo tests only.
            }
            WorldfileSection::ScriptFragment { source } => {
                if let (Some(compiler), Some(executor)) = (&mut compiler, &mut executor) {
                    // Clear debug buffer before execution.
                    executor.clear_debug_buffer();
                    // Compile the fragment.
                    let compiled_unit = compiler.compile_fragment(source);
                    // Execute if compilation succeeded.
                    let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                        executor.execute_fragment(ir_unit)
                    } else {
                        String::new()
                    };
                    // Capture debug output.
                    let debug_output = executor.get_debug_buffer();
                    results.push(SectionResult {
                        section_type: "scriptunit-fragment".S(),
                        name: None,
                        typecheck: compiled_unit.typecheck,
                        ownership: compiled_unit.ownership,
                        lowering: compiled_unit.lowering,
                        output,
                        debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                    });
                } else {
                    // Module compilation failed, skip script execution.
                    results.push(SectionResult {
                        section_type: "scriptunit-fragment".S(),
                        name: None,
                        typecheck: TypecheckResult::Skipped,
                        ownership: OwnershipResult::Skipped,
                        lowering: LoweringResult::Skipped,
                        output: String::new(),
                        debug_output: None,
                    });
                }
            }
            WorldfileSection::ScriptExpr { source } => {
                if let (Some(compiler), Some(executor)) = (&mut compiler, &mut executor) {
                    // Clear debug buffer before execution.
                    executor.clear_debug_buffer();
                    // Compile the expression.
                    let compiled_unit = compiler.compile_expr(source);
                    // Execute if compilation succeeded.
                    let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
                        let (_, value) = executor.execute_expr(ir_unit);
                        value
                    } else {
                        String::new()
                    };
                    // Capture debug output.
                    let debug_output = executor.get_debug_buffer();
                    results.push(SectionResult {
                        section_type: "scriptunit-expr".S(),
                        name: None,
                        typecheck: compiled_unit.typecheck,
                        ownership: compiled_unit.ownership,
                        lowering: compiled_unit.lowering,
                        output,
                        debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                    });
                } else {
                    // Module compilation failed, skip script execution.
                    results.push(SectionResult {
                        section_type: "scriptunit-expr".S(),
                        name: None,
                        typecheck: TypecheckResult::Skipped,
                        ownership: OwnershipResult::Skipped,
                        lowering: LoweringResult::Skipped,
                        output: String::new(),
                        debug_output: None,
                    });
                }
            }
        }
    }

    // Cleanup.
    if let Some(ref mut executor) = executor {
        executor.destroy_live_values();
    }

    Ok(Analysis { sections: results })
}
