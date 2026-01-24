//! Two-stage compilation pipeline: modules first, then scripts.
//!
//! # Overview
//!
//! Compilation proceeds in two stages:
//!
//! 1. **Module compilation** ([`ModuleCompilationPipeline`]): Parses, typechecks,
//!    analyzes drops, and lowers modules to IR. Produces [`CompiledModules`].
//!
//! 2. **Script compilation and execution** ([`ScriptCompiler`] and [`ScriptExecutor`]):
//!    Incrementally compiles script fragments with `ScriptCompiler`, then executes
//!    them with `ScriptExecutor`. Multiple independent contexts can share the same
//!    module compilation.
//!
//! # Example
//!
//! ```ignore
//! // Stage 1: compile modules.
//! let mut pipeline = ModuleCompilationPipeline::new();
//! pipeline.add_module(&db, "local", "mypackage", "main", source);
//! let compiled = pipeline.compile_fresh(&db);
//!
//! // Stage 2: compile and run scripts (only if compilation succeeded).
//! let mut compiler = compiled.script_compiler_default(&db).expect("compilation succeeded");
//! let mut executor = compiled.script_executor(DebugOutputMode::Disabled, None).unwrap();
//!
//! let result = compiler.compile_fragment("let x = 42");
//! if let Some(ir_unit) = &result.ir_unit {
//!     executor.execute_fragment(ir_unit);
//! }
//! ```
//!
//! # Incremental Compilation
//!
//! After initial compilation with `compile_fresh` (requires `&db`), use
//! `update_source` and `compile` for incremental updates (requires `&mut db`).
//! The pipeline preserves salsa identity across updates, enabling memoization.
//!
//! ```ignore
//! let compiled1 = pipeline.compile_fresh(&db);
//!
//! pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
//! let (compiled2, db) = pipeline.compile(&mut db);
//! ```
//!
//! # Parallelism
//!
//! Set `DATALOVE_PARALLEL=1` to enable parallel compilation, or use the
//! `*_with_mode` methods for explicit control.

mod result;
mod compiled_modules;
mod script_compiler;
mod script_executor;
mod module_pipeline;
pub mod aot;

// Re-export main types.
pub use result::{
    TypecheckResult, OwnershipResult, LoweringResult,
    format_ownership_result, format_lowering_result,
    ScriptUnitResult, ScriptCompilationResult,
};
pub use compiled_modules::{SharedModuleContext, CompiledModules};
pub use script_compiler::ScriptCompiler;
pub use script_executor::ScriptExecutor;
pub use module_pipeline::ModuleCompilationPipeline;

#[cfg(test)]
mod tests;
