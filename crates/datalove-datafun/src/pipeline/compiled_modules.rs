//! Compiled module results and shared context.
//!
//! This module contains the output types from module compilation:
//! - [`SharedModuleContext`]: Shared data across script compilers/executors.
//! - [`CompiledModules`]: Result of compiling a module graph, with error info.

use rmx::std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use datalove_datafun_ir::{IrModuleId, FuncId};
use datalove_datafun_interp::ModuleFunctionRegistry;
use datalove_datafun_tycheck::typecheck_module_graph;
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleGraphTypecheckResult, ModuleId,
    ParsedModuleGraph,
};

/// Compiled module data shared across script compilers and executors.
///
/// Contains the module graph, typecheck results, and function registry.
/// Thread-safe via `Arc` wrapping. Multiple `ScriptCompiler` and `ScriptExecutor`
/// instances can share the same context.
pub struct SharedModuleContext<'db> {
    pub module_graph: ModuleGraph,
    pub parsed_graph: ParsedModuleGraph<'db>,
    pub graph_typecheck: ModuleGraphTypecheckResult<'db>,
    pub func_id_map: HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    pub module_registry: Arc<ModuleFunctionRegistry>,
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

    /// Get module type diagnostics with spans for rendering.
    pub fn get_module_type_diagnostics(&self, db: &'db dyn salsa::Database) -> Vec<&'db datalove_diagnostic::TypeDiagnostic> {
        typecheck_module_graph::accumulated::<datalove_diagnostic::TypeDiagnostic>(db, self.shared.parsed_graph)
    }
}
