//! Module compilation pipeline with incremental recompilation support.
//!
//! The [`ModuleCompilationPipeline`] manages module compilation through parsing,
//! typechecking, ownership analysis, and IR lowering. It supports both fresh
//! compilation and incremental updates via salsa.
//!
//! # Example
//!
//! ```ignore
//! use datalove_datafun_interp::InterpCtfeEvaluator;
//!
//! let mut pipeline = ModuleCompilationPipeline::new();
//! pipeline.add_module(&db, "local", "mypackage", "main", source);
//! let compiled = pipeline.compile_fresh(&db);
//!
//! if compiled.is_successful() {
//!     let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
//!     let compiler = compiled.script_compiler(&db, evaluator).unwrap();
//!     let executor = compiled.script_executor(DebugOutputMode::Disabled, None).unwrap();
//!     // ...
//! }
//! ```

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;
use datalove_datafun_ir::{IrModuleId, FuncId};
use datalove_datafun_compiler::compile::{
    ModuleCompilationInput, ModuleCompilationOutput,
    compile_modules as compiler_compile_modules,
};
use datalove_datafun_compiler::tracked_lower::{
    lower_module_graph_with_mode, ModuleGraphLoweringResult,
};
use datalove_datafun_tycheck::{DbClone, ParallelMode, parallel_mode_from_env};
use datalove_datafun_compiler::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_interp::ModuleFunctionRegistry;

use crate::incremental::{IncrementalModuleWorld, extract_dependencies};
use super::compiled_modules::{SharedModuleContext, CompiledModules};

/// Module compilation pipeline with incremental recompilation support.
///
/// Compiles modules through parsing, typechecking, drop analysis, and IR lowering.
/// Use `compile_fresh` for initial compilation, then `compile` for incremental updates.
pub struct ModuleCompilationPipeline {
    world: IncrementalModuleWorld,
}

impl ModuleCompilationPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self {
            world: IncrementalModuleWorld::new(),
        }
    }

    /// Create a pipeline from worldfile sections.
    pub fn from_sections(
        db: &dyn salsa::Database,
        sections: &[WorldfileSection],
    ) -> Self {
        let mut pipeline = Self::new();
        pipeline.add_modules_from_sections(db, sections);
        pipeline
    }

    /// Add a module to the pipeline.
    pub fn add_module(
        &mut self,
        db: &dyn salsa::Database,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.add_module(db, &path, source);
    }

    /// Add modules from worldfile sections.
    pub fn add_modules_from_sections(
        &mut self,
        db: &dyn salsa::Database,
        sections: &[WorldfileSection],
    ) {
        for section in sections {
            if let WorldfileSection::Module { library, package, module, source } = section {
                self.add_module(db, library, package, module, source);
            }
        }
    }

    /// Load sys library from directory.
    pub async fn load_sys_library_from_dir(
        &mut self,
        db: &dyn salsa::Database,
        sys_dir: std::path::PathBuf,
    ) -> AnyResult<()> {
        use datalove_datafun_pkg::package_load;

        let config = package_load::PackageWorldConfig {
            dir_pkglib_system: sys_dir,
            dir_pkglib_local: None,
        };

        let package_world_raw = package_load::load_world(config).await?;

        for (pkg_name, pkg) in &package_world_raw.pkglib_system {
            for (mod_name, pkg_module) in &pkg.modules {
                self.add_module(db, "sys", pkg_name, mod_name, &pkg_module.text);
            }
        }

        Ok(())
    }

    /// Load sys library from default location.
    pub async fn load_sys_library_default(&mut self, db: &dyn salsa::Database) -> AnyResult<()> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = std::path::PathBuf::from(manifest_dir);
        let parent = manifest_path.parent()
            .ok_or_else(|| anyhow!("Failed to get parent directory"))?;
        let grandparent = parent.parent()
            .ok_or_else(|| anyhow!("Failed to get grandparent directory"))?;
        let sys_dir = grandparent.join("sys");

        self.load_sys_library_from_dir(db, sys_dir).await
    }

    /// Check if a module exists.
    pub fn contains_module(&self, library: &str, package: &str, module: &str) -> bool {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.contains(&path)
    }

    /// Remove a module from the pipeline.
    pub fn remove_module(&mut self, library: &str, package: &str, module: &str) {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.remove_module(&path);
    }

    /// Update a module's source text for incremental recompilation.
    pub fn update_source(
        &mut self,
        db: &mut dyn salsa::Database,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.update_source(db, &path, source);
    }

    /// Compile all modules (first compilation).
    ///
    /// Uses `DATALOVE_PARALLEL` env var to determine parallelism mode.
    pub fn compile_fresh<'db>(&mut self, db: &'db dyn DbClone) -> CompiledModules<'db> {
        let mode = parallel_mode_from_env();
        self.compile_fresh_with_mode(db, mode)
    }

    /// Compile all modules with explicit parallelism mode.
    pub fn compile_fresh_with_mode<'db>(
        &mut self,
        db: &'db dyn DbClone,
        mode: ParallelMode,
    ) -> CompiledModules<'db> {
        let path_deps = extract_dependencies(&self.world, db.as_salsa_db());
        let (module_graph, resolved_requires) = self.world.build_fresh(db.as_salsa_db(), &path_deps);
        self.compile_impl_with_mode(db, module_graph, resolved_requires, mode)
    }

    /// Compile all modules (incremental, needs `&mut db`).
    ///
    /// Uses `DATALOVE_PARALLEL` env var to determine parallelism mode.
    /// Requires a concrete type implementing `DbClone` for potential parallelism.
    pub fn compile<'db, D: DbClone>(
        &mut self,
        db: &'db mut D,
    ) -> (CompiledModules<'db>, &'db D) {
        let mode = parallel_mode_from_env();
        self.compile_with_mode(db, mode)
    }

    /// Compile all modules with explicit parallelism mode (incremental).
    pub fn compile_with_mode<'db, D: DbClone>(
        &mut self,
        db: &'db mut D,
        mode: ParallelMode,
    ) -> (CompiledModules<'db>, &'db D) {
        // Extract dependencies first (reads from db).
        let path_deps = extract_dependencies(&self.world, db);
        let (module_graph, resolved_requires) = self.world.prepare_for_compile(db, &path_deps);

        // Reborrow as immutable for the rest of compilation.
        let db_ref: &'db D = &*db;

        let compiled = self.compile_impl_with_mode(db_ref, module_graph, resolved_requires, mode);
        (compiled, db_ref)
    }

    /// Internal compilation implementation with configurable parallelism.
    fn compile_impl_with_mode<'db>(
        &self,
        db: &'db dyn DbClone,
        module_graph: ModuleGraph,
        resolved_requires: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
        mode: ParallelMode,
    ) -> CompiledModules<'db> {
        // Run parsing, typechecking, and ownership analysis.
        let input = ModuleCompilationInput {
            graph: module_graph,
            resolved_requires,
        };
        let output = compiler_compile_modules(db, input, mode);

        // Only run lowering if analysis succeeded.
        let lowering_result = if output.is_successful() {
            Some(lower_module_graph_with_mode(
                db,
                output.parsed_graph,
                output.typecheck_result,
                output.ownership_analysis,
                mode,
            ))
        } else {
            None
        };

        // Build interpreter structures from results.
        self.wrap_compiler_output(db, output, lowering_result)
    }

    /// Wrap compiler output with interpreter-specific structures.
    fn wrap_compiler_output<'db>(
        &self,
        db: &'db dyn DbClone,
        output: ModuleCompilationOutput<'db>,
        lowering_result: Option<ModuleGraphLoweringResult<'db>>,
    ) -> CompiledModules<'db> {
        // Build func_id_map and module registry from lowering result if available.
        let (func_id_map, module_registry, lowering_errors, module_ir_dumps) =
            if let Some(ref lowering) = lowering_result {
                // Convert FuncIdMap to HashMap.
                let func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)> =
                    lowering.func_id_map(db.as_salsa_db()).to_hashmap(db.as_salsa_db());

                // Build module registry from IR functions.
                let mut registry = ModuleFunctionRegistry::new();
                let mut lowering_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
                let mut module_ir_dumps: BTreeMap<String, Vec<String>> = BTreeMap::new();

                for (module_id, result) in lowering.module_results(db.as_salsa_db()) {
                    let ir_module_id = result.ir_module_id(db.as_salsa_db());
                    let module_path = module_id.path(db.as_salsa_db()).clone();

                    // Collect lowering errors.
                    let errors = result.errors(db.as_salsa_db());
                    if !errors.is_empty() {
                        lowering_errors.insert(module_path.clone(), errors.clone());
                    }

                    // Collect IR dumps.
                    let ir_dumps: Vec<String> = result.functions(db.as_salsa_db())
                        .iter()
                        .map(|ir_func| format!("{}", ir_func))
                        .collect();
                    module_ir_dumps.insert(module_path, ir_dumps);

                    // Add functions to registry.
                    for ir_func in result.functions(db.as_salsa_db()) {
                        registry.add_module_function(ir_module_id, ir_func.id, ir_func.clone());
                    }
                }

                (func_id_map, registry, lowering_errors, module_ir_dumps)
            } else {
                // No lowering - use empty structures.
                // Still need func_id_map for script compilation even without lowering.
                use datalove_datafun_compiler::tracked_lower::compute_func_id_map;
                let func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)> =
                    compute_func_id_map(db.as_salsa_db(), output.parsed_graph)
                        .to_hashmap(db.as_salsa_db());
                (func_id_map, ModuleFunctionRegistry::new(), BTreeMap::new(), BTreeMap::new())
            };

        let shared = Arc::new(SharedModuleContext {
            module_graph: output.module_graph,
            parsed_graph: output.parsed_graph,
            graph_typecheck: output.typecheck_result,
            func_id_map,
            module_registry: Arc::new(module_registry),
        });

        CompiledModules {
            shared,
            resolution_error: None,
            parse_errors: output.parse_errors,
            path_to_errors: output.typecheck_errors,
            ownership_errors: output.ownership_errors,
            lowering_errors,
            module_ir_dumps,
        }
    }
}

impl Default for ModuleCompilationPipeline {
    fn default() -> Self {
        Self::new()
    }
}
