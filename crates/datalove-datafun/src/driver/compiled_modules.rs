//! Compiled module results and shared context.
//!
//! This module contains the output types from module compilation:
//! - [`SharedModuleContext`]: Shared data across script compilers/executors.
//! - [`CompiledModules`]: Result of compiling a module graph, with error info.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use bct::diagnostic::StoredDiagnostic;
use bct::lookup::LookupMap;
use std::sync::Arc;
use datalove_datafun_ir::{ConstValue, IrModuleId, IrType};

use datalove_datafun_compiler::tracked_lower::FuncIdLookup;
use datalove_datafun_interp::{ModuleFunctionRegistry, NativeResolver};
use datalove_datafun_tycheck::typecheck_module_graph;
use datalove_datafun_resolve::resolve_all_names;
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

/// Compiled module data shared across script compilers and executors.
///
/// Contains the module graph, typecheck results, and function registry.
/// Thread-safe via `Arc` wrapping. Multiple `ScriptCompiler` and `ScriptExecutor`
/// instances can share the same context.
pub struct SharedModuleContext<'db> {
    pub module_graph: ModuleGraph<'db>,
    pub parsed_graph: ParsedModuleGraph<'db>,
    pub graph_typecheck: ModuleGraphTypecheckResult<'db>,
    /// Borrowed from the memo rather than copied into here: it holds an entry
    /// per function in the world, and script compilation only reads it.
    pub func_id_map: &'db FuncIdLookup<'db>,
    pub module_registry: Arc<ModuleFunctionRegistry>,
    /// Each module's module-level consts, which a copy of one of its comptime
    /// functions made for a script may name.
    pub module_consts: LookupMap<IrModuleId, LookupMap<String, (IrType, Arc<ConstValue>)>>,
}

/// Result of module compilation.
///
/// Use `script_compiler()` to create a compiler for script units, and
/// `script_executor()` to create an executor for running compiled IR.
/// Multiple independent compilers and executors can share the same compilation.
pub struct CompiledModules<'db> {
    pub shared: Arc<SharedModuleContext<'db>>,
    /// Parse diagnostics by module path, and a rider interface's parse and
    /// resolution diagnostics under `@rider/<name>`.
    pub parse_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    /// Type diagnostics by module path, from each module's own check.
    pub type_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    /// Ownership diagnostics by module path.
    pub ownership_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    /// What lowering refused, by module path: the consts that could not be
    /// evaluated, and the functions that could not be specialized.
    pub lowering_diagnostics: BTreeMap<String, Vec<StoredDiagnostic>>,
    pub module_ir_dumps: BTreeMap<String, Vec<String>>,
    /// Where the modules' consts found their natives, and a script's will.
    pub natives: Option<Arc<dyn NativeResolver>>,
    /// The rider interfaces compiled against, by rider name.
    pub rider_sources: Vec<(String, bct::input::Source)>,
}

impl<'db> CompiledModules<'db> {
    /// Check if compilation succeeded.
    pub fn is_successful(&self) -> bool {
        self.phases().all(|by_path| by_path.values().all(Vec::is_empty))
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.is_successful()
    }

    /// The path of every module, riders included, by the id the IR knows it by.
    pub fn module_paths(&self, db: &'db dyn salsa::Database) -> LookupMap<IrModuleId, String> {
        datalove_datafun_compiler::tracked_lower::ir_module_ids(db, self.shared.parsed_graph)
            .iter_unordered()
            .map(|(module, ir_id)| (*ir_id, module.path(db).C()))
            .collect()
    }

    /// Each phase's diagnostics by module path, in phase order.
    fn phases(&self) -> impl Iterator<Item = &BTreeMap<String, Vec<StoredDiagnostic>>> {
        [&self.parse_diagnostics, &self.type_diagnostics, &self.ownership_diagnostics, &self.lowering_diagnostics]
            .into_iter()
    }

    /// What each module that did not compile said, by path, in phase order.
    pub fn diagnostics_by_module(&self) -> BTreeMap<String, Vec<StoredDiagnostic>> {
        let mut by_module: BTreeMap<String, Vec<StoredDiagnostic>> = BTreeMap::new();
        for by_path in self.phases() {
            for (path, diagnostics) in by_path.iter().filter(|(_, diagnostics)| !diagnostics.is_empty()) {
                by_module.entry(path.C()).or_default().extend(diagnostics.iter().cloned());
            }
        }
        by_module
    }

    /// Every error's message, in phase order: for a test to say what failed.
    pub fn all_errors(&self) -> Vec<String> {
        self.phases()
            .flat_map(|by_path| by_path.values().flatten())
            .map(|diagnostic| diagnostic.message.C())
            .collect()
    }

    /// Every diagnostic the compile reported, in phase order, to be rendered
    /// against `db`.
    ///
    /// Those of the data files the modules require too, which the type
    /// checker files in the graph's check rather than in any module's, and so
    /// are in no map of this.
    pub fn diagnostics(&self, db: &'db dyn salsa::Database) -> Vec<StoredDiagnostic> {
        let graph = self.shared.parsed_graph.graph(db);
        let riders: Vec<_> = self.rider_sources.iter()
            .map(|(_, source)| datalove_datafun_compiler::module_graph::rider_interface_diagnostics(db, *source))
            .collect();
        let parse = graph.iter_modules(db)
            .flat_map(|module| {
                datalove_datafun_parser::parse_module_full::accumulated::<datalove_diagnostic::ParseDiagnostic>(db, module)
            })
            .chain(riders.iter().flat_map(|(parse, _)| parse.iter().copied()))
            .map(|d| d.stored().C());
        let all_names = resolve_all_names(db, self.shared.parsed_graph);
        let types = typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, self.shared.parsed_graph, all_names)
            .into_iter()
            .chain(riders.iter().flat_map(|(_, types)| types.iter().copied()))
            .map(|d| d.stored().C());
        let ownership = datalove_datafun_compiler::tracked_ownership_analysis::analyze_module_graph::accumulated::<datalove_diagnostic::OwnershipDiagnostic>(
            db, self.shared.parsed_graph, self.shared.graph_typecheck)
            .into_iter()
            .map(|d| d.stored().C());
        let lowering = self.lowering_diagnostics.values().flatten().cloned();
        parse.chain(types).chain(ownership).chain(lowering).collect()
    }

    /// Collect native function symbols from the module registry.
    ///
    /// Returns the linker symbols (e.g. `dlr_std__list_push`) for all code units
    /// with `NativeContext`, which need to be loaded from rider shared libraries.
    pub fn native_symbols(&self) -> Vec<String> {
        let mut symbols = Vec::new();
        for unit in self.shared.module_registry.iter_module_code_units() {
            if let Some(ctx) = unit.native_context() {
                symbols.push(ctx.symbol().S());
            }
        }
        symbols
    }
}
