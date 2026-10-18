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
//! let mut pipeline = ModuleCompilationPipeline::default();
//! pipeline.add_module(&db, "local", "mypackage", "main", source);
//! let compiled = pipeline.compile(&db);
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
//! Edit a module with `update_source`, which needs `&mut db` since it sets an
//! input, and call `compile` again. The pipeline keeps its inputs across edits,
//! so the recompile reuses everything the edit did not reach.
//!
//! ```ignore
//! let compiled1 = pipeline.compile(&db);
//! drop(compiled1);
//!
//! pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
//! let compiled2 = pipeline.compile(&db);
//! ```
//!
//! # Parallelism
//!
//! Compiles are parallel unless `DATALOVE_PARALLEL=0` (see
//! `parallel_mode_from_env`); the `*_with_mode` methods choose explicitly.

mod result;
mod compiled_modules;
mod compiled_world;
mod script_compiler;
mod script_executor;
mod module_pipeline;
pub mod workspace;
#[cfg(not(target_arch = "wasm32"))]
pub mod aot;
pub mod rider_build;
pub mod rider_load;
#[cfg(not(target_arch = "wasm32"))]
pub mod c_aot;

// Re-export main types.
pub use result::{
    TypecheckResult, OwnershipResult, LoweringResult,
    format_ownership_result, format_lowering_result,
    ScriptUnitResult, ScriptCompilationResult,
};
pub use compiled_modules::{SharedModuleContext, CompiledModules};
pub use compiled_world::CompiledWorld;
pub use script_compiler::{ScriptCompiler, ScriptSession};
pub use script_executor::ScriptExecutor;
pub use module_pipeline::ModuleCompilationPipeline;
pub use workspace::{
    WorkspaceDescriptor, WorkspaceDelta, SystemLibrary,
    PackageLibrary, PackageDescriptor, ModuleDescriptor,
    RiderDescriptor, RiderCrate, CompilerOptions, find_local_library, load_local_library,
};

#[cfg(test)]
mod tests;
