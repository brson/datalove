//! Function registry for storing compiled IR functions.
//!
//! Split into shareable and per-script components:
//! - `ModuleFunctionRegistry`: shared, immutable after module compilation
//! - `UnitFunctionRegistry`: per-script, grows as units execute
//! - `FunctionRegistry`: combined view for backward compatibility

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use crate::{IrFunction, FuncId, IrModuleId};

/// Registry of module functions, shared across all scripts.
///
/// Immutable after module compilation completes.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ModuleFunctionRegistry {
    module_functions: HashMap<(IrModuleId, FuncId), IrFunction>,
}

impl ModuleFunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            module_functions: HashMap::new(),
        }
    }

    /// Add a module function.
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.module_functions.insert((module_id, func_id), func);
    }

    /// Get a module function by module and function ID.
    pub fn get_module_function(&self, module_id: IrModuleId, func_id: FuncId) -> Option<&IrFunction> {
        self.module_functions.get(&(module_id, func_id))
    }

    /// Iterate over all module functions.
    pub fn iter_module_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.module_functions.values()
    }

    /// Iterate over all module functions with their IDs.
    pub fn iter_module_functions_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, FuncId), &IrFunction)> {
        self.module_functions.iter().map(|((m, f), func)| ((*m, *f), func))
    }
}

/// Registry of unit functions, per-script.
///
/// Grows as script units are executed.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct UnitFunctionRegistry {
    unit_functions: Vec<Vec<IrFunction>>,
}

impl UnitFunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            unit_functions: Vec::new(),
        }
    }

    /// Add functions from a completed unit.
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_functions.push(functions);
    }

    /// Look up a function from a previous unit.
    pub fn get_external_function(&self, unit: u32, func_id: FuncId) -> Option<&IrFunction> {
        let functions = self.unit_functions.get(unit as usize)?;
        functions.iter().find(|f| f.id == func_id)
    }

    /// Get all functions from a unit.
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrFunction]> {
        self.unit_functions.get(unit as usize).map(|v| v.as_slice())
    }

    /// Iterate over all unit functions.
    pub fn iter_unit_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.unit_functions.iter().flat_map(|v| v.iter())
    }
}

/// Combined registry for backward compatibility.
///
/// Holds references to both module and unit registries.
#[derive(Clone, Serialize, Deserialize)]
pub struct FunctionRegistry {
    module_registry: Arc<ModuleFunctionRegistry>,
    unit_registry: UnitFunctionRegistry,
}

impl FunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            module_registry: Arc::new(ModuleFunctionRegistry::new()),
            unit_registry: UnitFunctionRegistry::new(),
        }
    }

    /// Create a registry with an existing module registry.
    pub fn with_module_registry(module_registry: Arc<ModuleFunctionRegistry>) -> Self {
        Self {
            module_registry,
            unit_registry: UnitFunctionRegistry::new(),
        }
    }

    /// Get a reference to the module registry.
    pub fn module_registry(&self) -> &Arc<ModuleFunctionRegistry> {
        &self.module_registry
    }

    /// Get a mutable reference to the module registry (for initial setup).
    ///
    /// Panics if there are multiple references to the module registry.
    pub fn module_registry_mut(&mut self) -> &mut ModuleFunctionRegistry {
        Arc::get_mut(&mut self.module_registry)
            .expect("Cannot get mutable reference to shared module registry")
    }

    /// Get a reference to the unit registry.
    pub fn unit_registry(&self) -> &UnitFunctionRegistry {
        &self.unit_registry
    }

    /// Get a mutable reference to the unit registry.
    pub fn unit_registry_mut(&mut self) -> &mut UnitFunctionRegistry {
        &mut self.unit_registry
    }

    /// Add a module function.
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.module_registry_mut().add_module_function(module_id, func_id, func);
    }

    /// Add functions from a completed unit.
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_registry.add_unit_functions(functions);
    }

    /// Get a module function by module and function ID.
    pub fn get_module_function(&self, module_id: IrModuleId, func_id: FuncId) -> Option<&IrFunction> {
        self.module_registry.get_module_function(module_id, func_id)
    }

    /// Look up a function from a previous unit.
    pub fn get_external_function(&self, unit: u32, func_id: FuncId) -> Option<&IrFunction> {
        self.unit_registry.get_external_function(unit, func_id)
    }

    /// Get all functions from a unit.
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrFunction]> {
        self.unit_registry.unit_functions(unit)
    }

    /// Iterate over all module functions.
    pub fn iter_module_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.module_registry.iter_module_functions()
    }

    /// Iterate over all module functions with their IDs.
    pub fn iter_module_functions_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, FuncId), &IrFunction)> {
        self.module_registry.iter_module_functions_with_ids()
    }

    /// Iterate over all unit functions.
    pub fn iter_unit_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.unit_registry.iter_unit_functions()
    }

    /// Iterate over all functions (modules + units).
    pub fn iter_all_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.iter_module_functions().chain(self.iter_unit_functions())
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FunctionRegistry {
    /// Serialize to RON format.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        let config = ron::ser::PrettyConfig::new()
            .struct_names(true)
            .enumerate_arrays(false);
        ron::ser::to_string_pretty(self, config)
    }

    /// Deserialize from RON format.
    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(s)
    }
}
