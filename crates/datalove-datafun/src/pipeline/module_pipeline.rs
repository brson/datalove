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
use datalove_datafun_compiler::compile::{
    ModuleCompilationInput, ModuleCompilationOutput,
    compile_modules as compiler_compile_modules,
};
use datalove_datafun_compiler::tracked_lower::{
    add_native_rider_units, func_id_lookup, lower_module_graph_with_evaluator,
    ModuleGraphLoweringResult,
};
use datalove_datafun_tycheck::ParsedModuleGraph;
use datalove_datafun_ir::CtfeEvaluator;
use datalove_datafun_compiler::const_cache::{ConstCache, ConstCacheReport};
use datalove_datafun_interp::{InterpCtfeEvaluator, NativeResolver};
use datalove_datafun_tycheck::{DbClone, ParallelMode, parallel_mode_from_env};
use datalove_datafun_compiler::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_interp::ModuleFunctionRegistry;
use datalove_datafun_ir::ModuleCodeUnits;

use crate::incremental::{IncrementalModuleWorld, Roots, extract_dependencies};
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
    /// Which of the world's modules to compile.
    ///
    /// `Roots::All` by default, which is the whole world and what every caller
    /// wanted before there was a choice. See [`Roots`].
    roots: Roots,
    /// Where evaluating a const finds the natives it calls.
    ///
    /// `None` for a pipeline whose consts call none, where reaching one is a
    /// panic. See [`set_natives`](Self::set_natives).
    natives: Option<Arc<dyn NativeResolver>>,
    /// Consts evaluated by earlier compiles, by module.
    ///
    /// Keyed on what the modules say; what the natives do is not in that, so
    /// setting them empties it. See `const_cache`.
    const_cache: ConstCache,
}

impl ModuleCompilationPipeline {
    /// Create an empty pipeline.
    pub fn new(options: CompilerOptions) -> Self {
        Self {
            world: IncrementalModuleWorld::new(),
            options,
            rider_sources: Vec::new(),
            roots: Roots::All,
            natives: None,
            const_cache: ConstCache::default(),
        }
    }

    /// Let consts call natives, found through `natives`.
    ///
    /// Asked for a native only when a const's evaluation reaches it, which for
    /// built riders is what builds them; see
    /// [`RiderNatives`](super::rider_load::RiderNatives). The compiled modules
    /// carry it on, to the evaluator a script compiles its consts with.
    pub fn set_natives(&mut self, natives: Arc<dyn NativeResolver>) {
        self.natives = Some(natives);
        self.const_cache.clear();
    }

    /// Check every const the cache supplies against an evaluation; see
    /// [`ConstCache::set_verify`].
    pub fn set_verify_const_cache(&mut self, verify: bool) {
        self.const_cache.set_verify(verify);
    }

    /// Which modules the last compile evaluated consts for, and which it reused.
    pub fn const_cache_report(&self) -> &ConstCacheReport {
        self.const_cache.last_report()
    }

    /// The evaluator to run consts with when the caller does not give one.
    fn ctfe_evaluator(&self) -> InterpCtfeEvaluator {
        match &self.natives {
            Some(natives) => InterpCtfeEvaluator::new().with_native_resolver(natives.clone()),
            None => InterpCtfeEvaluator::new(),
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
        // The system library is read once and does not change while a session
        // runs, so an edit to anybody's own code should not make salsa walk it.
        let durability = match library {
            "sys" => salsa::Durability::HIGH,
            _ => salsa::Durability::LOW,
        };
        self.world.add_module_with_durability(db, &path, source, durability);
    }

    /// Add a data file to the pipeline, for `require data` to name.
    pub fn add_data(
        &mut self,
        db: &dyn salsa::Database,
        library: &str,
        package: &str,
        name: &str,
        text: &str,
    ) {
        let path = format!("{}/{}/{}", library, package, name);
        let durability = match library {
            "sys" => salsa::Durability::HIGH,
            _ => salsa::Durability::LOW,
        };
        self.world.add_data_with_durability(db, &path, text, durability);
    }

    /// Remove a data file from the pipeline.
    pub fn remove_data(&mut self, library: &str, package: &str, name: &str) {
        self.world.remove_data(&format!("{}/{}/{}", library, package, name));
    }

    /// Update a data file's text for incremental recompilation.
    pub fn update_data(
        &mut self,
        db: &mut dyn salsa::Database,
        library: &str,
        package: &str,
        name: &str,
        text: &str,
    ) {
        self.world.update_data(db, &format!("{}/{}/{}", library, package, name), text);
    }

    /// The source a data file is read from, by its path.
    pub fn data_source(&self, path: &str) -> Option<bct::input::Source> {
        self.world.data().get(path).copied()
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
                WorldfileSection::Data { library, package, name, source } => {
                    self.add_data(db, library, package, name, source);
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

        for (pkg_name, pkg) in &package_world_raw.pkglib_system {
            for (mod_name, pkg_module) in &pkg.modules {
                self.add_module(db, "sys", pkg_name, mod_name, &pkg_module.text);
            }
            for (name, file) in &pkg.data {
                self.add_data(db, "sys", pkg_name, name, &file.text);
            }
        }

        Ok(())
    }

    /// Add rider sources and crate directories from a loaded package world.
    pub fn add_riders_from_package_world(&mut self, world: &datalove_datafun_pkg::package_load::PackageWorld) {
        self.rider_sources.extend(world.rider_sources());
    }


    /// Set rider interface sources (replaces any existing).
    pub fn set_rider_sources(&mut self, sources: Vec<(String, String)>) {
        self.rider_sources = sources;
    }

    /// Compile only what these modules reach, rather than the whole world.
    ///
    /// The roots are part of the module graph's identity, so changing them is a
    /// different graph and not a stale memo. Changing them between compiles of
    /// one pipeline is therefore sound, and costs what adding or removing that
    /// many modules costs.
    pub fn set_roots(&mut self, roots: Roots) {
        self.roots = roots;
    }

    /// Which of the world's modules this pipeline compiles.
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// Narrow the roots to the modules a script requires, plus `also`.
    ///
    /// **A `require` naming a module the world does not have is skipped.** The
    /// script's own typecheck reports it, at the require, as F079. This used to
    /// refuse to narrow at all, compiling the whole world instead, from before
    /// that diagnostic existed, when pruning would have left nothing to report.
    /// With a workspace's own library in the world that answer stopped being
    /// harmless: a mistyped require reported every broken module in the
    /// workspace, and not the mistyped require.
    ///
    /// A script that requires nothing narrows to nothing, and compiles no modules
    /// at all. That is the intended answer and not an edge case: a program using
    /// only builtins needs no library.
    ///
    /// **`also` is for modules that belong to the artifact rather than to a
    /// library, and the distinction is the whole reason it exists.** A module the
    /// author wrote in the file being compiled is part of what they asked to be
    /// compiled, whether or not the script happens to call into it -- a worldfile
    /// with a broken module and a script that ignores it is a worldfile with an
    /// error in it, and 19 of `world_error_tests`' fixtures are exactly that
    /// shape. A module from the system library is not: not compiling one the
    /// program never reaches is the point. So a worldfile's own modules are roots
    /// and the system library's are not.
    ///
    /// This parses the script, and the script compiler parses it again later under
    /// a `Source` of its own -- the roots have to be known before the modules are
    /// compiled, and the script's own parse happens after. One file twice is a
    /// fair price for not compiling two dozen modules, but it is a duplicate and
    /// worth removing if the script compiler is ever given its `Source` from
    /// outside.
    pub fn narrow_roots_to_script(
        &mut self,
        db: &dyn salsa::Database,
        script: &str,
        also: &[String],
    ) {
        use datalove_datafun_ast::ast::{Statement, StmtRequire};

        let source = bct::input::Source::new(db, script.S());
        let parsed = datalove_datafun_parser::parse(db, source);

        let mut roots: rmx::std::collections::BTreeSet<String> =
            also.iter().filter(|p| self.world.contains(p)).cloned().collect();
        for statement in parsed.parsed.statements.iter() {
            let Statement::Require(StmtRequire::Module(require)) = statement else {
                continue;
            };
            let path = format!(
                "{}/{}/{}",
                require.import_space.as_str(db),
                require.package_alias.as_str(db),
                require.module_alias.as_str(db),
            );
            if self.world.contains(&path) {
                roots.insert(path);
            }
        }

        self.roots = Roots::From(roots);
    }



    /// Check if a module exists.
    pub fn contains_module(&self, library: &str, package: &str, module: &str) -> bool {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.contains(&path)
    }

    /// The source a module is compiled from, by its path.
    ///
    /// What a diagnostic's text came from, for finding which module, and so
    /// which file, it points into.
    pub fn module_source(&self, path: &str) -> Option<bct::input::Source> {
        self.world.sources().get(path).copied()
    }

    /// The text a module currently holds.
    ///
    /// What an editor of modules needs in order to put an edit back: a module
    /// set that does not compile leaves nothing in the session able to compile,
    /// so a caller that will not tolerate that has to be able to undo.
    pub fn module_text(
        &self,
        db: &dyn salsa::Database,
        library: &str,
        package: &str,
        module: &str,
    ) -> Option<String> {
        let path = format!("{}/{}/{}", library, package, module);
        self.world.sources().get(&path).map(|source| source.text(db).C())
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
        let mut evaluator = self.ctfe_evaluator();
        self.compile_fresh_with_mode_and_evaluator(db, mode, &mut evaluator)
    }

    /// Compile all modules with explicit parallelism mode and custom CTFE evaluator.
    pub fn compile_fresh_with_mode_and_evaluator<'db>(
        &mut self,
        db: &'db dyn DbClone,
        mode: ParallelMode,
        evaluator: &mut dyn CtfeEvaluator,
    ) -> CompiledModules<'db> {
        let path_deps = extract_dependencies(&self.world, db.as_salsa_db(), &self.roots);
        let (module_graph, resolved_requires) =
            self.world.build_graph(db.as_salsa_db(), &path_deps, &self.roots);
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
        let mut evaluator = self.ctfe_evaluator();
        self.compile_with_mode_and_evaluator(db, mode, &mut evaluator)
    }

    /// Compile all modules with explicit parallelism mode and custom CTFE evaluator (incremental).
    pub fn compile_with_mode_and_evaluator<'db, D: DbClone>(
        &mut self,
        db: &'db mut D,
        mode: ParallelMode,
        evaluator: &mut dyn CtfeEvaluator,
    ) -> (CompiledModules<'db>, &'db D) {
        // Extract dependencies first (reads from db).
        let path_deps = extract_dependencies(&self.world, db, &self.roots);
        let (module_graph, resolved_requires) =
            self.world.build_graph(&*db, &path_deps, &self.roots);

        // Reborrow as immutable for the rest of compilation.
        let db_ref: &'db D = &*db;

        let compiled = self.compile_impl(db_ref, module_graph, resolved_requires, mode, evaluator);
        (compiled, db_ref)
    }

    /// Internal compilation implementation.
    ///
    /// The const cache assumes every compile evaluates with the same kind of
    /// evaluator, which the default and every caller of the `_and_evaluator`
    /// methods in this crate do.
    fn compile_impl<'db>(
        &mut self,
        db: &'db dyn DbClone,
        module_graph: ModuleGraph<'db>,
        resolved_requires: BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
        mode: ParallelMode,
        evaluator: &mut dyn CtfeEvaluator,
    ) -> CompiledModules<'db> {
        // Run parsing, typechecking, and ownership analysis.
        let input = ModuleCompilationInput {
            graph: module_graph,
            resolved_requires,
        };
        let output = compiler_compile_modules(db, input, self.rider_sources.clone(), mode);

        // The riders are the part of the world a module's closure does not
        // cover, so a change to any of them empties the const cache.
        let mut riders = rmx::blake3::Hasher::new();
        for (name, source) in &self.rider_sources {
            for text in [name, source] {
                riders.update(&(text.len() as u64).to_le_bytes());
                riders.update(text.as_bytes());
            }
        }
        self.const_cache.set_world(*riders.finalize().as_bytes());

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
                self.options.cache_consts.then_some(&mut self.const_cache),
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
        let db_salsa = db.as_salsa_db();

        let (func_id_map, module_registry, lowering_errors, module_ir_dumps, module_consts) =
            if let Some(ref lowering) = lowering_result {
                let func_id_map = func_id_lookup(db_salsa, lowering.func_id_map(db_salsa));

                // The registry comes out of a memo. It holds an entry per
                // function in the world, and building one and dropping the
                // previous one was most of what an unchanged recompile of the
                // system library cost.
                let registry = Arc::clone(
                    module_function_registry(db_salsa, *lowering, output.parsed_graph));

                let mut lowering_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
                let mut module_ir_dumps: BTreeMap<String, Vec<String>> = BTreeMap::new();
                let mut module_consts = HashMap::new();

                for (module_id, result) in lowering.module_results(db_salsa) {
                    let module_path = module_id.path(db_salsa).clone();

                    let consts = result.consts(db_salsa);
                    if !consts.is_empty() {
                        module_consts.insert(result.ir_module_id(db_salsa), consts.iter()
                            .map(|(name, ty, value)| (name.clone(), (ty.clone(), std::sync::Arc::clone(value))))
                            .collect());
                    }

                    // Collect lowering errors.
                    let errors = result.errors(db_salsa);
                    if !errors.is_empty() {
                        lowering_errors.insert(module_path.clone(), errors.clone());
                    }

                    // Collect IR dumps, when someone has asked for them. Printing
                    // every function costs about what lowering them does, and
                    // only the fixtures read the result.
                    if self.options.keep_ir_dumps {
                        let ir_dumps: Vec<String> = result.functions(db_salsa)
                            .iter()
                            .map(|ir_unit| format!("{}", ir_unit))
                            .collect();
                        module_ir_dumps.insert(module_path, ir_dumps);
                    }
                }

                (func_id_map, registry, lowering_errors, module_ir_dumps, module_consts)
            } else {
                // No lowering - use empty structures.
                // Still need func_id_map for script compilation even without lowering.
                use datalove_datafun_compiler::tracked_lower::compute_func_id_map;
                let func_id_map = func_id_lookup(
                    db_salsa, compute_func_id_map(db_salsa, output.parsed_graph));
                (
                    func_id_map,
                    Arc::new(ModuleFunctionRegistry::new()),
                    BTreeMap::new(),
                    BTreeMap::new(),
                    HashMap::new(),
                )
            };

        let shared = Arc::new(SharedModuleContext {
            module_graph: output.module_graph,
            parsed_graph: output.parsed_graph,
            graph_typecheck: output.typecheck_result,
            func_id_map,
            module_registry,
            module_consts,
        });

        CompiledModules {
            shared,
            resolution_error: None,
            parse_errors: output.parse_errors,
            path_to_errors: output.typecheck_errors,
            ownership_errors: output.ownership_errors,
            lowering_errors,
            module_ir_dumps,
            natives: self.natives.clone(),
        }
    }
}

/// Every function in the world, under the ids the backends address it by.
///
/// Tracked. It is a pure function of what phase 5 produced plus the riders, and
/// it holds an entry per function, so rebuilding it on every compile -- and
/// dropping the one before it -- was around two thirds of what an unchanged
/// recompile of the system library cost.
///
/// Keyed on the lowering result, which is a tracked struct whose identity is
/// the per-module results it holds, so it is a word to hash and it moves
/// exactly when some module's assembled IR does.
///
/// Capped, because that key moves on every edit and the memo for the key
/// before it is of no use to anyone: left uncapped this grew by about 140KB an
/// edit, which is half again what the rest of the pipeline already retains per
/// revision. Eviction runs once per revision, so nothing is dropped underneath
/// a compile, and the capacity only has to cover the one a recompile asks for
/// plus the one an edit displaces.
#[salsa::tracked(returns(ref), lru = 4)]
fn module_function_registry<'db>(
    db: &'db dyn salsa::Database,
    lowering: ModuleGraphLoweringResult<'db>,
    parsed_graph: ParsedModuleGraph<'db>,
) -> Arc<ModuleFunctionRegistry> {
    let mut registry = ModuleFunctionRegistry::new();

    for result in lowering.module_results(db).values() {
        registry.set_module_code_units(
            result.ir_module_id(db),
            Arc::clone(module_code_units(db, *result)),
        );
    }

    add_native_rider_units(
        db,
        parsed_graph,
        func_id_lookup(db, lowering.func_id_map(db)),
        &mut registry,
    );

    Arc::new(registry)
}

/// One module's code units, keyed by the id the backends address them by.
///
/// Tracked on the lowering result, which is a tracked struct with no tracked
/// fields, so its identity moves exactly when that module's assembled IR does.
/// The registry above is rebuilt whenever *any* module's does, and it holds an
/// entry per function in the program; this is what stops that costing an insert
/// per function rather than a refcount per module.
#[salsa::tracked(returns(ref))]
fn module_code_units<'db>(
    db: &'db dyn salsa::Database,
    result: datalove_datafun_compiler::tracked_lower::SingleModuleLoweringResult<'db>,
) -> Arc<ModuleCodeUnits> {
    Arc::new(
        result.functions(db).iter()
            .map(|unit| (unit.id, Arc::clone(unit)))
            .collect(),
    )
}

impl Default for ModuleCompilationPipeline {
    fn default() -> Self {
        Self::new(CompilerOptions::default())
    }
}
