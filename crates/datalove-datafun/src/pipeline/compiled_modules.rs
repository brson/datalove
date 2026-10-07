//! Compiled module results and shared context.
//!
//! This module contains the output types from module compilation:
//! - [`SharedModuleContext`]: Shared data across script compilers/executors.
//! - [`CompiledModules`]: Result of compiling a module graph, with error info.

use rmx::std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use datalove_datafun_ir::{ConstValue, IrModuleId, IrType};

use datalove_datafun_compiler::tracked_lower::FuncIdLookup;
use datalove_datafun_interp::{ModuleFunctionRegistry, NativeResolver};
use datalove_datafun_tycheck::{typecheck_module_graph, AutoAdaptMode};
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
    pub module_consts: HashMap<IrModuleId, HashMap<String, (IrType, Arc<ConstValue>)>>,
}

/// Result of module compilation.
///
/// Use `script_compiler()` to create a compiler for script units, and
/// `script_executor()` to create an executor for running compiled IR.
/// Multiple independent compilers and executors can share the same compilation.
pub struct CompiledModules<'db> {
    pub shared: Arc<SharedModuleContext<'db>>,
    pub resolution_error: Option<String>,
    pub parse_errors: BTreeMap<String, Vec<String>>,
    pub path_to_errors: BTreeMap<String, Vec<String>>,
    pub ownership_errors: BTreeMap<String, Vec<String>>,
    pub lowering_errors: BTreeMap<String, Vec<String>>,
    pub module_ir_dumps: BTreeMap<String, Vec<String>>,
    /// Where the modules' consts found their natives, and a script's will.
    pub natives: Option<Arc<dyn NativeResolver>>,
}

impl<'db> CompiledModules<'db> {
    /// Check if compilation succeeded.
    pub fn is_successful(&self) -> bool {
        self.resolution_error.is_none()
            && self.parse_errors.values().all(|errors| errors.is_empty())
            && self.path_to_errors.values().all(|errors| errors.is_empty())
            && self.ownership_errors.values().all(|errors| errors.is_empty())
            && self.lowering_errors.values().all(|errors| errors.is_empty())
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.is_successful()
    }

    /// Collect all errors.
    pub fn all_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if let Some(err) = &self.resolution_error {
            errors.push(format!("Resolution error: {}", err));
        }

        for error_list in self.parse_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        for error_list in self.path_to_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        for error_list in self.ownership_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        for error_list in self.lowering_errors.values() {
            errors.extend(error_list.iter().cloned());
        }

        errors
    }

    /// Get all parse errors.
    pub fn all_parse_errors(&self) -> Vec<String> {
        self.parse_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all typecheck errors.
    pub fn all_typecheck_errors(&self) -> Vec<String> {
        self.path_to_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all ownership analysis errors.
    pub fn all_ownership_errors(&self) -> Vec<String> {
        self.ownership_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Get all lowering errors.
    pub fn all_lowering_errors(&self) -> Vec<String> {
        self.lowering_errors.values()
            .flatten()
            .cloned()
            .collect()
    }

    /// Collect native function symbols from the module registry.
    ///
    /// Returns the linker symbols (e.g. `dlr_std__list_push`) for all code units
    /// with `NativeContext`, which need to be loaded from rider shared libraries.
    pub fn native_symbols(&self) -> Vec<String> {
        let mut symbols = Vec::new();
        for unit in self.shared.module_registry.iter_module_code_units() {
            if let Some(ctx) = unit.native_context() {
                symbols.push(ctx.symbol().to_string());
            }
        }
        symbols
    }

    /// Get module parse diagnostics with spans for rendering.
    pub fn get_module_parse_diagnostics(&self, db: &'db dyn salsa::Database) -> Vec<&'db datalove_diagnostic::ParseDiagnostic> {
        let graph = self.shared.parsed_graph.graph(db);
        graph.iter_modules(db)
            .flat_map(|module| {
                datalove_datafun_parser::parse_module_full::accumulated::<datalove_diagnostic::ParseDiagnostic>(db, module)
            })
            .collect()
    }

    /// Get module type diagnostics with spans for rendering.
    pub fn get_module_type_diagnostics(&self, db: &'db dyn salsa::Database) -> Vec<&'db datalove_diagnostic::TypeDiagnostic> {
        // Recompute resolve results (memoized, will be cache hits).
        let all_names = resolve_all_names(db, self.shared.parsed_graph);
        typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, self.shared.parsed_graph, all_names, AutoAdaptMode::Disabled)
    }
}
