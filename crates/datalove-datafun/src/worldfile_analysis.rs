//! Worldfile analysis using the IR interpreter.
//!
//! This module provides test infrastructure for worldfiles that may contain
//! module sections along with scriptunit-fragment and scriptunit-expr sections.
//! Tests execute script units sequentially using the IR-based interpreter.
//!
//! Script units are executed with a shared environment, allowing later units
//! to reference values, slots, and functions from earlier units.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};

use crate::pipeline::{
    ModuleCompilationPipeline, TypecheckResult, LoweringResult, format_module_lowering_result,
};

/// Result of analyzing a worldfile with IR interpreter.
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
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value (for expression units) or function call result.
    pub output: String,
    /// Debug log output (from debuglog statements).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_output: Option<String>,
}

/// Analyze a worldfile using the IR interpreter.
///
/// This function processes sections in order:
/// 1. Module sections: typechecked but not executed
/// 2. scriptunit-fragment sections: typechecked, lowered to IR, executed
/// 3. scriptunit-expr sections: typechecked, lowered to IR, executed, result captured
///
/// Script units share a `ScriptLowerContext` (for cross-unit name resolution during lowering)
/// and a `ScriptEnvironment` (for cross-unit value/function access during execution).
pub fn analyze_worldfile(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Analysis> {
    let mut results = Vec::new();

    // Build pipeline and add modules.
    let mut pipeline = ModuleCompilationPipeline::new(db);
    pipeline.add_modules_from_sections(&parsed.sections);

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = pipeline.compile();

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(SectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: vec![err.clone()] },
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

            // Look up drop analysis errors.
            let drop_key = format!("{}", module_path);
            let has_drop_errors = compiled.drop_analysis_errors.keys()
                .any(|k| k.starts_with(&drop_key));

            // Look up lowering results for this module.
            let has_typecheck_errors = matches!(&typecheck, TypecheckResult::Error { .. });
            let lowering = if has_drop_errors {
                let errors: Vec<_> = compiled.drop_analysis_errors.iter()
                    .filter(|(k, _)| k.starts_with(&drop_key))
                    .flat_map(|(_, v)| v.iter().cloned())
                    .collect();
                LoweringResult::Error { message: format!("Drop analysis errors: {}", errors.join("; ")) }
            } else {
                match compiled.module_lowering_results.get(&module_path) {
                    Some(ir_dumps) => format_module_lowering_result(ir_dumps, has_typecheck_errors),
                    None => LoweringResult::Skipped,
                }
            };

            results.push(SectionResult {
                section_type: "module".to_string(),
                name: Some(module_path),
                typecheck,
                lowering,
                output: String::new(),
                debug_output: None,
            });
        }
    }

    // Create script compilation context (module specs built internally from module graph).
    let mut ctx = compiled.script_context(db);

    // Enable debug buffer mode for capturing debuglog output.
    ctx.set_debug_mode(datalove_rt::c::DebugOutputMode::Buffer);

    // Process script units using the context.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Already handled above.
            }
            WorldfileSection::ScriptFragment { source } => {
                // Clear debug buffer before execution.
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_fragment(source);
                // Capture debug output.
                let debug_output = ctx.get_debug_buffer();
                results.push(SectionResult {
                    section_type: "scriptunit-fragment".to_string(),
                    name: None,
                    typecheck: unit_result.typecheck,
                    lowering: unit_result.lowering,
                    output: unit_result.output,
                    debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                });
            }
            WorldfileSection::ScriptExpr { source } => {
                // Clear debug buffer before execution.
                ctx.clear_debug_buffer();
                let unit_result = ctx.eval_expr(source);
                // Capture debug output.
                let debug_output = ctx.get_debug_buffer();
                results.push(SectionResult {
                    section_type: "scriptunit-expr".to_string(),
                    name: None,
                    typecheck: unit_result.typecheck,
                    lowering: unit_result.lowering,
                    output: unit_result.output,
                    debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
                });
            }
        }
    }

    // Cleanup.
    ctx.destroy_all();

    Ok(Analysis { sections: results })
}
