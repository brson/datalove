//! Module compilation pipeline with incremental recompilation support.
//!
//! The [`ModuleCompilationPipeline`] manages module compilation through parsing,
//! typechecking, ownership analysis, and IR lowering. It supports both fresh
//! compilation and incremental updates via salsa.
//!
//! # Example
//!
//! ```ignore
//! let mut pipeline = ModuleCompilationPipeline::default();
//! pipeline.add_module(&db, "local", "mypackage", "main", source);
//! let compiled = pipeline.compile_fresh(&db);
//!
//! if compiled.is_successful() {
//!     let compiler = compiled.script_compiler_default(&db).unwrap();
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
    lower_module_graph_with_evaluator, ModuleGraphLoweringResult,
};
use datalove_datafun_ir::CtfeEvaluator;
use datalove_datafun_interp::InterpCtfeEvaluator;
use std::cell::RefCell;
use std::rc::Rc;
use datalove_datafun_tycheck::{DbClone, ParallelMode, parallel_mode_from_env};
use datalove_datafun_compiler::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_interp::ModuleFunctionRegistry;

use crate::incremental::{IncrementalModuleWorld, extract_dependencies};
use super::compiled_modules::{SharedModuleContext, CompiledModules};
use super::workspace::CompilerOptions;

/// Module compilation pipeline with incremental recompilation support.
///
/// Compiles modules through parsing, typechecking, drop analysis, and IR lowering.
/// Use `compile_fresh` for initial compilation, then `compile` for incremental updates.
pub struct ModuleCompilationPipeline {
    world: IncrementalModuleWorld,
    /// Settled when the pipeline is built. Nothing recompiles under different
    /// options than it was made with, so there is nothing here to set later.
    options: CompilerOptions,
    /// Rider interfaces parsed from worldfile rider sections.
    /// Maps rider name to source text for deferred parsing.
    rider_sources: Vec<(String, String)>,
    /// Rider crate directories discovered from package loading.
    rider_crate_dirs: Vec<(String, std::path::PathBuf)>,
}

impl ModuleCompilationPipeline {
    /// Create an empty pipeline.
    pub fn new(options: CompilerOptions) -> Self {
        Self {
            world: IncrementalModuleWorld::new(),
            options,
            rider_sources: Vec::new(),
            rider_crate_dirs: Vec::new(),
        }
    }

    /// Create a pipeline from worldfile sections.
    pub fn from_sections(
        db: &dyn salsa::Database,
        sections: &[WorldfileSection],
        options: CompilerOptions,
    ) -> Self {
        let mut pipeline = Self::new(options);
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
            match section {
                WorldfileSection::Module { library, package, module, source } => {
                    self.add_module(db, library, package, module, source);
                }
                WorldfileSection::Rider { name, source } => {
                    self.rider_sources.push((name.clone(), source.clone()));
                }
                _ => {}
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

        // Extract rider sources and crate directories from loaded packages.
        self.rider_sources.extend(package_world_raw.rider_sources());
        self.rider_crate_dirs.extend(package_world_raw.rider_crate_dirs());

        for (pkg_name, pkg) in &package_world_raw.pkglib_system {
            for (mod_name, pkg_module) in &pkg.modules {
                self.add_module(db, "sys", pkg_name, mod_name, &pkg_module.text);
            }
        }

        Ok(())
    }

    /// Add rider sources and crate directories from a loaded package world.
    pub fn add_riders_from_package_world(&mut self, world: &datalove_datafun_pkg::package_load::PackageWorld) {
        self.rider_sources.extend(world.rider_sources());
        self.rider_crate_dirs.extend(world.rider_crate_dirs());
    }

    /// Get discovered rider crate directories.
    pub fn rider_crate_dirs(&self) -> &[(String, std::path::PathBuf)] {
        &self.rider_crate_dirs
    }

    /// Set rider interface sources (replaces any existing).
    pub fn set_rider_sources(&mut self, sources: Vec<(String, String)>) {
        self.rider_sources = sources;
    }

    /// Set rider crate directories (replaces any existing).
    pub fn set_rider_crate_dirs(&mut self, dirs: Vec<(String, std::path::PathBuf)>) {
        self.rider_crate_dirs = dirs;
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
        let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
        self.compile_fresh_with_mode_and_evaluator(db, mode, evaluator)
    }

    /// Compile all modules with explicit parallelism mode and custom CTFE evaluator.
    pub fn compile_fresh_with_mode_and_evaluator<'db>(
        &mut self,
        db: &'db dyn DbClone,
        mode: ParallelMode,
        evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    ) -> CompiledModules<'db> {
        let path_deps = extract_dependencies(&self.world, db.as_salsa_db());
        let (module_graph, resolved_requires) = self.world.build_fresh(db.as_salsa_db(), &path_deps);
        self.compile_impl(db, module_graph, resolved_requires, mode, evaluator)
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
        let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
        self.compile_with_mode_and_evaluator(db, mode, evaluator)
    }

    /// Compile all modules with explicit parallelism mode and custom CTFE evaluator (incremental).
    pub fn compile_with_mode_and_evaluator<'db, D: DbClone>(
        &mut self,
        db: &'db mut D,
        mode: ParallelMode,
        evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    ) -> (CompiledModules<'db>, &'db D) {
        // Extract dependencies first (reads from db).
        let path_deps = extract_dependencies(&self.world, db);
        let (module_graph, resolved_requires) = self.world.prepare_for_compile(&*db, &path_deps);

        // Reborrow as immutable for the rest of compilation.
        let db_ref: &'db D = &*db;

        let compiled = self.compile_impl(db_ref, module_graph, resolved_requires, mode, evaluator);
        (compiled, db_ref)
    }

    /// Internal compilation implementation.
    fn compile_impl<'db>(
        &self,
        db: &'db dyn DbClone,
        module_graph: ModuleGraph<'db>,
        resolved_requires: BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
        mode: ParallelMode,
        evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    ) -> CompiledModules<'db> {
        // Run parsing, typechecking, and ownership analysis.
        let input = ModuleCompilationInput {
            graph: module_graph,
            resolved_requires,
        };
        let output = compiler_compile_modules(db, input, self.rider_sources.clone(), mode);

        // Only run lowering if analysis succeeded.
        let lowering_result = if output.is_successful() {
            Some(lower_module_graph_with_evaluator(
                db,
                output.parsed_graph,
                output.typecheck_result,
                output.ownership_analysis,
                mode,
                evaluator,
                !self.options.const_inlining,
                self.options.skip_specialization,
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
                let func_id_map: HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)> =
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

                    // Collect IR dumps, when someone has asked for them. Printing
                    // every function costs about what lowering them does, and
                    // only the fixtures read the result.
                    if self.options.keep_ir_dumps {
                        let ir_dumps: Vec<String> = result.functions(db.as_salsa_db())
                            .iter()
                            .map(|ir_unit| format!("{}", ir_unit))
                            .collect();
                        module_ir_dumps.insert(module_path, ir_dumps);
                    }

                    // Add functions to registry.
                    for ir_unit in result.functions(db.as_salsa_db()) {
                        registry.add_module_code_unit(ir_module_id, ir_unit.id, ir_unit.clone());
                    }
                }

                // Emit native code units for rider functions.
                Self::add_native_rider_units(
                    db.as_salsa_db(), &output, &func_id_map, &mut registry,
                );

                (func_id_map, registry, lowering_errors, module_ir_dumps)
            } else {
                // No lowering - use empty structures.
                // Still need func_id_map for script compilation even without lowering.
                use datalove_datafun_compiler::tracked_lower::compute_func_id_map;
                let func_id_map: HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)> =
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

impl ModuleCompilationPipeline {
    /// Create native IrCodeUnits for rider functions and add them to the registry.
    ///
    /// Linker symbols are generated here at the backend boundary, not in the compiler core.
    fn add_native_rider_units<'db>(
        db: &'db dyn salsa::Database,
        output: &ModuleCompilationOutput<'db>,
        func_id_map: &HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>,
        registry: &mut ModuleFunctionRegistry,
    ) {
        use datalove_datafun_ir::{IrType, CodeUnitId, NativeContext};
        use datalove_datafun_compiler::IrTypeExt;
        use std::collections::BTreeSet;

        let mut seen_riders: BTreeSet<String> = BTreeSet::new();

        for (_module_id, riders) in output.parsed_graph.resolved_riders(db).iter() {
            for (alias, rider) in riders {
                let alias_str = alias.text(db);
                if !seen_riders.insert(alias_str.S()) {
                    continue; // Already processed this rider.
                }

                let synthetic_module_id = rider.module_id;

                for (func_name, func_type) in &rider.functions {
                    let name = func_name.text(db).S();
                    let symbol = format!("dlr_{}__{}", alias_str, name);

                    // Look up the assigned IrModuleId and FuncId.
                    let Some(&(ir_module_id, func_id)) = func_id_map.get(&(synthetic_module_id, name.clone())) else {
                        continue;
                    };

                    // Convert types from tycheck to IR.
                    let param_types: Vec<IrType> = func_type.param_types(db)
                        .iter()
                        .map(|ty| IrType::from_tycheck(db, ty))
                        .collect();
                    // Convert AST ParamMode to IR ParamMode.
                    let param_modes: Vec<datalove_datafun_ir::ParamMode> = func_type.param_modes(db)
                        .iter()
                        .map(|m| match m {
                            datalove_datafun_ast::ast::ParamMode::In => datalove_datafun_ir::ParamMode::In,
                            datalove_datafun_ast::ast::ParamMode::Out => datalove_datafun_ir::ParamMode::Out,
                            datalove_datafun_ast::ast::ParamMode::Ref => datalove_datafun_ir::ParamMode::Ref,
                            datalove_datafun_ast::ast::ParamMode::Mut => datalove_datafun_ir::ParamMode::Mut,
                        })
                        .collect();
                    let return_type = IrType::from_tycheck(db, &func_type.return_type(db));

                    // What the shape closure was told this native needs, said
                    // the same way here so that the two cannot disagree about
                    // the trailing arguments. A native makes no calls, so its
                    // set is exactly what its signature says.
                    let descriptor_shapes = rider.generic_functions.iter()
                        .find(|(n, _)| *n == *func_name)
                        .map(|(_, generics)| generics.undetermined.iter()
                            .map(|i| datalove_datafun_ir::DescriptorShape::Param(*i))
                            .collect())
                        .unwrap_or_default();

                    let native_ctx = NativeContext {
                        param_modes,
                        param_types,
                        return_type,
                        symbol,
                        descriptor_shapes,
                    };
                    let code_unit = datalove_datafun_ir::IrCodeUnit::native(
                        CodeUnitId(func_id.0),
                        name,
                        native_ctx,
                    );
                    registry.add_module_code_unit(ir_module_id, code_unit.id, std::sync::Arc::new(code_unit));
                }
            }
        }
    }
}

impl Default for ModuleCompilationPipeline {
    fn default() -> Self {
        Self::new(CompilerOptions::default())
    }
}
