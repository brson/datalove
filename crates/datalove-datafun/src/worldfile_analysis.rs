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

use datalove_datafun_compiler::instances::GenericsPolicy;
use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::driver::{
    ModuleCompiler, CompilerOptions, TypecheckResult, OwnershipResult, LoweringResult,
    SystemLibrary, WorkspaceDescriptor, format_ownership_result, format_lowering_result,
};
use crate::driver::rider_load::{RiderNatives, register_natives};
use crate::incremental::Roots;

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
    /// Which calls to generics are pointed at instances, in the module graph
    /// and in the scripts alike.
    pub generics: GenericsPolicy,
    /// A system library the worldfile may require modules of, beside its
    /// own, and whose natives its units may call.
    ///
    /// Only the modules the worldfile requires are compiled, those its own
    /// modules and units name in a `require module` and what those reach;
    /// see [`required_system_modules`]. Without one, a world has only its own
    /// sections, and requiring `sys/std/u8` is an error.
    pub system: Option<Arc<SystemLibrary>>,
}

// ============================================================================
// Analysis Functions
// ============================================================================

/// Analyze a worldfile using the IR interpreter with default options.
///
/// This is equivalent to calling [`analyze_worldfile_with_options`] with
/// default options (const inlining enabled).
pub fn analyze_worldfile(
    db: &crate::Database,
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
    db: &crate::Database,
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
    db: &crate::Database,
    parsed: &ParsedWorldfile,
    options: AnalysisOptions,
    hooks: &mut dyn ExecutorHooks,
) -> AnyResult<Analysis> {
    let mut results = Vec::new();

    // This function exists to produce fixture output, which includes the IR.
    let compiler_options = CompilerOptions {
        const_inlining: !options.skip_const_inlining,
        skip_specialization: options.skip_specialization,
        generics: options.generics.C(),
        keep_ir_dumps: true,
        ..CompilerOptions::default()
    };
    let (mut module_compiler, natives) =
        worldfile_module_compiler(db, &parsed.sections, compiler_options, options.system.as_deref());

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = module_compiler.compile(db);

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        results.push(SectionResult {
            section_type: "resolution".S(),
            name: None,
            typecheck: TypecheckResult::Error { errors: vec![err.C()] },
            ownership: OwnershipResult::Skipped,
            lowering: LoweringResult::Skipped,
            output: String::new(),
            debug_output: None,
        });
        return Ok(Analysis { sections: results });
    }

    // Collect module results.
    collect_module_results(&parsed.sections, &compiled, &mut results);

    // A script compiler, configured as the module graph was compiled: the
    // same flags have to reach it, or a differential run would, say,
    // specialize one half. Made again after a unit fails at run time.
    let new_compiler = || {
        let mut compiler = compiled.script_compiler_default(db)?;
        compiler.set_skip_const_inlining(options.skip_const_inlining);
        compiler.set_skip_specialization(options.skip_specialization);
        compiler.set_generics_policy(options.generics.C());
        Some(compiler)
    };
    let mut compiler = new_compiler();
    // With Buffer mode, for capturing debuglog output.
    let mut executor = compiled.script_executor(datalove_rt::c::DebugOutputMode::Buffer, None);

    if let Some(executor) = &mut executor {
        hooks.configure(executor);
    }
    // After the dispatcher is in, which is told the natives' addresses too.
    let _registered = match (&natives, &mut executor) {
        (Some((natives, _)), Some(executor)) => Some(register_natives(natives, &compiled, executor)?),
        _ => None,
    };

    // Process script sections.
    process_script_sections(&parsed.sections, &mut compiler, &new_compiler, &mut executor, &mut results);

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

/// A module compiler for a worldfile's sections, and beside them the system
/// library's modules the sections require, if there is a library.
///
/// With a library, the natives its riders provide come back too, for an
/// executor to register; the compiler has them already, for consts that call
/// one. And the workspace, which says which riders a program that calls them
/// has to link.
pub fn worldfile_module_compiler(
    db: &crate::Database,
    sections: &[WorldfileSection],
    options: CompilerOptions,
    system: Option<&SystemLibrary>,
) -> (ModuleCompiler, Option<(Arc<RiderNatives>, WorkspaceDescriptor)>) {
    let Some(system) = system else {
        return (ModuleCompiler::from_sections(db, sections, options), None);
    };
    let descriptor = WorkspaceDescriptor::from_system_library(system)
        .merge(&WorkspaceDescriptor::from_worldfile_sections(sections, options));
    let (mut module_compiler, natives) = descriptor.to_compiler_with_natives(db, &system.natives);
    module_compiler.set_roots(Roots::From(required_system_modules(sections)));
    (module_compiler, Some((natives, descriptor)))
}

/// The modules a worldfile's sections are compiled from, as roots: its own,
/// and the system library's that any section requires by a line of its
/// own, `require module sys/...`. Their own requires reach the rest.
fn required_system_modules(sections: &[WorldfileSection]) -> BTreeSet<String> {
    let own = sections.iter().filter_map(|s| s.module_path()).map(|p| p.to_path_string());
    let required = sections.iter()
        .filter_map(|s| match s {
            WorldfileSection::Module { source, .. }
            | WorldfileSection::ScriptFragment { source }
            | WorldfileSection::ScriptExpr { source } => Some(source),
            _ => None,
        })
        .flat_map(|source| source.lines())
        .filter_map(|line| line.trim().strip_prefix("require module "))
        .map(|path| path.trim())
        .filter(|path| path.starts_with("sys/"))
        .map(|path| path.S());
    own.chain(required).collect()
}

use crate::driver::{CompiledModules, ScriptCompiler, ScriptExecutor};

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
                    TypecheckResult::ParseError { errors: errors.C() }
                }
                _ => match compiled.path_to_errors.get(&module_path) {
                    Some(errors) if !errors.is_empty() => {
                        TypecheckResult::Error { errors: errors.C() }
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
///
/// A unit that fails at run time takes the session with it: the executor
/// frees what every unit bound and forgets the units (see
/// `ScriptExecutor::failed`). The units after it are compiled by a new
/// compiler, from `new_compiler`, as a session started again, which is what
/// the REPL does; compiled against the old one, they read bindings the
/// executor no longer has.
fn process_script_sections<'db>(
    sections: &[WorldfileSection],
    compiler: &mut Option<ScriptCompiler<'db>>,
    new_compiler: &dyn Fn() -> Option<ScriptCompiler<'db>>,
    executor: &mut Option<ScriptExecutor>,
    results: &mut Vec<SectionResult>,
) {
    for section in sections {
        let units_before = executor.as_ref().map(|e| e.unit_ids().len());
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
            _ => continue,
        }
        // Every unit is added to the executor's, run or not, but one that
        // failed, which leaves none.
        let units_after = executor.as_ref().map(|e| e.unit_ids().len());
        if let (Some(before), Some(after)) = (units_before, units_after) && after <= before {
            *compiler = new_compiler();
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

        // Execute if compilation succeeded; a unit that did not still takes
        // its place.
        let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
            executor.execute_fragment(ir_unit)
        } else {
            executor.skip_unit(compiler.last_unit_id());
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

        // Execute if compilation succeeded; a unit that did not still takes
        // its place.
        let output = if let Some(ir_unit) = &compiled_unit.ir_unit {
            let (_, value) = executor.execute_expr(ir_unit);
            value
        } else {
            executor.skip_unit(compiler.last_unit_id());
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
