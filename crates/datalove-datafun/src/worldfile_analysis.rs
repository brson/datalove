//! Worldfile analysis using the IR interpreter.
//!
//! This module provides test infrastructure for worldfiles that contain
//! module and script sections. It processes sections sequentially:
//!
//! 1. **Module sections**: Compiled (parsed, typechecked, ownership-analyzed, lowered)
//! 2. **Script sections**: Compiled and executed via the IR interpreter
//!
//! Script units share execution state, allowing later units to reference
//! values, slots, and functions from earlier units.
//!
//! # Const Inlining
//!
//! By default, `const` bindings are evaluated at compile time (CTFE) and inlined.
//! For testing purposes, use [`analyze_worldfile_with_options`] with
//! `skip_const_inlining: true` to evaluate const bindings at runtime instead.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};

use crate::pipeline::{
    ModuleCompilationPipeline, CompilerOptions, TypecheckResult, OwnershipResult, LoweringResult,
    format_ownership_result, format_lowering_result,
};

// ============================================================================
// Result Types
// ============================================================================

/// Result of analyzing a worldfile with IR interpreter.
#[derive(Debug, Serialize, Deserialize)]
pub struct Analysis {
    /// Per-section results.
    pub sections: Vec<SectionResult>,
}

/// Result of analyzing one section.
#[derive(Debug, Serialize, Deserialize)]
pub struct SectionResult {
    /// Section type (e.g., "module", "scriptunit-fragment", "scriptunit-expr").
    pub section_type: String,
    /// Section name/identifier (module path for module sections).
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

// ============================================================================
// Analysis Options
// ============================================================================

/// Options for worldfile analysis.
#[derive(Debug, Clone, Default)]
pub struct AnalysisOptions {
    /// Skip const inlining (evaluate const bindings at runtime instead of CTFE).
    pub skip_const_inlining: bool,
    /// Skip comptime specialization (use original function without union-branch dispatch).
    /// Useful for differential testing to verify specialization correctness.
    pub skip_specialization: bool,
}

// ============================================================================
// Analysis Functions
// ============================================================================

/// Analyze a worldfile using the IR interpreter with default options.
///
/// This is equivalent to calling [`analyze_worldfile_with_options`] with
/// default options (const inlining enabled).
pub fn analyze_worldfile(
    db: &mut crate::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Analysis> {
    analyze_worldfile_with_options(db, parsed, AnalysisOptions::default())
}

/// Analyze a worldfile using the IR interpreter with custom options.
///
/// Processes sections in order:
/// 1. Module sections: compiled but not executed
/// 2. scriptunit-fragment sections: compiled and executed
/// 3. scriptunit-expr sections: compiled, executed, result captured
pub fn analyze_worldfile_with_options(
    db: &mut crate::Database,
    parsed: ParsedWorldfile,
    options: AnalysisOptions,
) -> AnyResult<Analysis> {
    analyze_worldfile_with_hooks(db, &parsed, options, &mut NoHooks)
}

/// Hooks into the executor an analysis runs the script units on.
///
/// What the engine tests use to choose the engine and install a dispatcher,
/// and to read the dispatcher's stats when the units have run.
pub trait ExecutorHooks {
    /// Configure the executor before any unit runs.
    fn configure(&mut self, executor: &mut ScriptExecutor);
    /// Inspect the executor after the last unit has run.
    fn finish(&mut self, executor: &mut ScriptExecutor);
}

struct NoHooks;

impl ExecutorHooks for NoHooks {
    fn configure(&mut self, _executor: &mut ScriptExecutor) {}
    fn finish(&mut self, _executor: &mut ScriptExecutor) {}
}

/// Analyze a worldfile as [`analyze_worldfile_with_options`] does, with
/// hooks into the executor.
pub fn analyze_worldfile_with_hooks(
    db: &mut crate::Database,
    parsed: &ParsedWorldfile,
    options: AnalysisOptions,
    hooks: &mut dyn ExecutorHooks,
) -> AnyResult<Analysis> {
    let mut results = Vec::new();

    // This function exists to produce fixture output, which includes the IR.
    let mut pipeline = ModuleCompilationPipeline::from_sections(
        db,
        &parsed.sections,
        CompilerOptions {
            const_inlining: !options.skip_const_inlining,
            skip_specialization: options.skip_specialization,
            keep_ir_dumps: true,
            ..CompilerOptions::default()
        },
    );

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

    // Collect module results.
    collect_module_results(&parsed.sections, &compiled, &mut results);

    // Create script compiler and executor with Buffer mode for capturing debuglog output.
    let mut compiler = compiled.script_compiler_default(db);
    let mut executor = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, None);

    // Configure skip_const_inlining on the script compiler if needed.
    if options.skip_const_inlining {
        if let Some(ref mut compiler) = compiler {
            compiler.set_skip_const_inlining(true);
        }
    }

    // The same flag the module graph was compiled under has to reach the
    // script compiler, or the differential run would specialize one half.
    if options.skip_specialization {
        if let Some(ref mut compiler) = compiler {
            compiler.set_skip_specialization(true);
        }
    }

    if let Some(executor) = &mut executor {
        hooks.configure(executor);
    }

    // Process script sections.
    process_script_sections(&parsed.sections, &mut compiler, &mut executor, &mut results);

    // Cleanup.
    if let Some(ref mut executor) = executor {
        hooks.finish(executor);
        executor.destroy_live_values();
    }

    Ok(Analysis { sections: results })
}

// ============================================================================
// Internal Helpers
// ============================================================================

use crate::pipeline::{CompiledModules, ScriptCompiler, ScriptExecutor};

/// Collect analysis results for module sections.
fn collect_module_results(
    sections: &[WorldfileSection],
    compiled: &CompiledModules<'_>,
    results: &mut Vec<SectionResult>,
) {
    for section in sections {
        if let Some(path) = section.module_path() {
            let module_path = path.to_path_string();

            // A module that did not parse has nothing to typecheck, and saying
            // so is the only way the refusal reaches anyone: a parse
            // diagnostic is not a typecheck error and used to go unread here.
            let typecheck = match compiled.parse_errors.get(&module_path) {
                Some(errors) if !errors.is_empty() => {
                    TypecheckResult::ParseError { errors: errors.clone() }
                }
                _ => match compiled.path_to_errors.get(&module_path) {
                    Some(errors) if !errors.is_empty() => {
                        TypecheckResult::Error { errors: errors.clone() }
                    }
                    _ => TypecheckResult::Success,
                },
            };

            // Look up analysis results for this module.
            let has_typecheck_errors = !matches!(&typecheck, TypecheckResult::Success);
            let ir_dumps = compiled.module_ir_dumps.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let ownership_errs = compiled.ownership_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let lowering_errs = compiled.lowering_errors.get(&module_path).map(|v| v.as_slice()).unwrap_or(&[]);
            let ownership = format_ownership_result(ownership_errs, has_typecheck_errors);
            let has_ownership_errors = matches!(&ownership, OwnershipResult::Error { .. });
            let lowering = format_lowering_result(ir_dumps, lowering_errs, has_typecheck_errors || has_ownership_errors);

            // Only include initial Module sections (not change/add/remove actions).
            if matches!(section, WorldfileSection::Module { .. }) {
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
    }
}

/// Process script sections (fragment and expr).
fn process_script_sections(
    sections: &[WorldfileSection],
    compiler: &mut Option<ScriptCompiler<'_>>,
    executor: &mut Option<ScriptExecutor>,
    results: &mut Vec<SectionResult>,
) {
    for section in sections {
        match section {
            WorldfileSection::ScriptFragment { source } => {
                let result = execute_script_fragment(source, compiler, executor);
                results.push(result);
            }
            WorldfileSection::ScriptExpr { source } => {
                let result = execute_script_expr(source, compiler, executor);
                results.push(result);
            }
            // Module sections already handled; action sections are for memo tests only.
            _ => {}
        }
    }
}

/// Execute a script fragment and return the result.
fn execute_script_fragment(
    source: &str,
    compiler: &mut Option<ScriptCompiler<'_>>,
    executor: &mut Option<ScriptExecutor>,
) -> SectionResult {
    if let (Some(compiler), Some(executor)) = (compiler, executor) {
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

        SectionResult {
            section_type: "scriptunit-fragment".S(),
            name: None,
            typecheck: compiled_unit.typecheck,
            ownership: compiled_unit.ownership,
            lowering: compiled_unit.lowering,
            output,
            debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
        }
    } else {
        // Module compilation failed, skip script execution.
        skipped_section_result("scriptunit-fragment")
    }
}

/// Execute a script expression and return the result.
fn execute_script_expr(
    source: &str,
    compiler: &mut Option<ScriptCompiler<'_>>,
    executor: &mut Option<ScriptExecutor>,
) -> SectionResult {
    if let (Some(compiler), Some(executor)) = (compiler, executor) {
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

        SectionResult {
            section_type: "scriptunit-expr".S(),
            name: None,
            typecheck: compiled_unit.typecheck,
            ownership: compiled_unit.ownership,
            lowering: compiled_unit.lowering,
            output,
            debug_output: if debug_output.is_empty() { None } else { Some(debug_output) },
        }
    } else {
        // Module compilation failed, skip script execution.
        skipped_section_result("scriptunit-expr")
    }
}

/// Create a skipped section result.
fn skipped_section_result(section_type: &str) -> SectionResult {
    SectionResult {
        section_type: section_type.S(),
        name: None,
        typecheck: TypecheckResult::Skipped,
        ownership: OwnershipResult::Skipped,
        lowering: LoweringResult::Skipped,
        output: String::new(),
        debug_output: None,
    }
}
