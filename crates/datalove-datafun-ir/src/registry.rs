//! Function registry for storing compiled IR code units.
//!
//! Split into shareable and per-script components:
//! - `ModuleFunctionRegistry`: shared, immutable after module compilation
//! - `UnitFunctionRegistry`: per-script, grows as units execute
//! - `FunctionRegistry`: combined view

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use crate::{IrFunction, IrCodeUnit, CodeUnitId, FuncId, IrModuleId};

/// Registry of module functions, shared across all scripts.
///
/// Immutable after module compilation completes.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ModuleFunctionRegistry {
    module_functions: HashMap<(IrModuleId, FuncId), IrCodeUnit>,
}

impl ModuleFunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            module_functions: HashMap::new(),
        }
    }

    /// Add a module function (accepts IrFunction, converts to IrCodeUnit).
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.module_functions.insert((module_id, func_id), IrCodeUnit::from(func));
    }

    /// Add a module code unit.
    pub fn add_module_code_unit(&mut self, module_id: IrModuleId, func_id: FuncId, unit: IrCodeUnit) {
        self.module_functions.insert((module_id, func_id), unit);
    }

    /// Get a module function by module and function ID (legacy, returns IrFunction).
    pub fn get_module_function(&self, module_id: IrModuleId, func_id: FuncId) -> Option<IrFunction> {
        self.module_functions.get(&(module_id, func_id)).map(|u| IrFunction::from(u.clone()))
    }

    /// Get a module code unit by module and function ID.
    pub fn get_module_function_as_unit(&self, module_id: IrModuleId, func_id: FuncId) -> Option<&IrCodeUnit> {
        self.module_functions.get(&(module_id, func_id))
    }

    /// Iterate over all module functions.
    pub fn iter_module_functions(&self) -> impl Iterator<Item = IrFunction> + '_ {
        self.module_functions.values().map(|u| IrFunction::from(u.clone()))
    }

    /// Iterate over all module code units.
    pub fn iter_module_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.module_functions.values()
    }

    /// Iterate over all module functions with their IDs.
    pub fn iter_module_functions_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, FuncId), IrFunction)> + '_ {
        self.module_functions.iter().map(|((m, f), unit)| ((*m, *f), IrFunction::from(unit.clone())))
    }
}

/// Registry of unit functions, per-script.
///
/// Grows as script units are executed.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct UnitFunctionRegistry {
    unit_functions: Vec<Vec<IrCodeUnit>>,
}

impl UnitFunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            unit_functions: Vec::new(),
        }
    }

    /// Add functions from a completed unit (accepts Vec<IrFunction>, converts to IrCodeUnit).
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_functions.push(functions.into_iter().map(IrCodeUnit::from).collect());
    }

    /// Add code units from a completed unit.
    pub fn add_unit_code_units(&mut self, units: Vec<IrCodeUnit>) {
        self.unit_functions.push(units);
    }

    /// Look up a function from a previous unit (legacy, returns IrFunction).
    pub fn get_external_function(&self, unit: u32, func_id: FuncId) -> Option<IrFunction> {
        let functions = self.unit_functions.get(unit as usize)?;
        functions.iter().find(|f| f.id.0 == func_id.0).map(|u| IrFunction::from(u.clone()))
    }

    /// Look up a code unit from a previous unit.
    pub fn get_external_function_as_unit(&self, unit: u32, func_id: FuncId) -> Option<&IrCodeUnit> {
        let functions = self.unit_functions.get(unit as usize)?;
        functions.iter().find(|f| f.id.0 == func_id.0)
    }

    /// Get all functions from a unit (legacy, returns slice of IrCodeUnit for now).
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrCodeUnit]> {
        self.unit_functions.get(unit as usize).map(|v| v.as_slice())
    }

    /// Iterate over all unit functions.
    pub fn iter_unit_functions(&self) -> impl Iterator<Item = IrFunction> + '_ {
        self.unit_functions.iter().flat_map(|v| v.iter()).map(|u| IrFunction::from(u.clone()))
    }

    /// Iterate over all unit code units.
    pub fn iter_unit_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.unit_functions.iter().flat_map(|v| v.iter())
    }
}

/// Combined registry for code units.
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

    /// Add a module function (legacy, accepts IrFunction).
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.module_registry_mut().add_module_function(module_id, func_id, func);
    }

    /// Add a module code unit.
    pub fn add_module_code_unit(&mut self, module_id: IrModuleId, func_id: FuncId, unit: IrCodeUnit) {
        self.module_registry_mut().add_module_code_unit(module_id, func_id, unit);
    }

    /// Add functions from a completed unit (legacy, accepts Vec<IrFunction>).
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_registry.add_unit_functions(functions);
    }

    /// Add code units from a completed unit.
    pub fn add_unit_code_units(&mut self, units: Vec<IrCodeUnit>) {
        self.unit_registry.add_unit_code_units(units);
    }

    /// Get a module function by module and function ID (legacy, returns IrFunction).
    pub fn get_module_function(&self, module_id: IrModuleId, func_id: FuncId) -> Option<IrFunction> {
        self.module_registry.get_module_function(module_id, func_id)
    }

    /// Get a module code unit by module and function ID.
    pub fn get_module_function_as_unit(&self, module_id: IrModuleId, func_id: FuncId) -> Option<&IrCodeUnit> {
        self.module_registry.get_module_function_as_unit(module_id, func_id)
    }

    /// Look up a function from a previous unit (legacy, returns IrFunction).
    pub fn get_external_function(&self, unit: u32, func_id: FuncId) -> Option<IrFunction> {
        self.unit_registry.get_external_function(unit, func_id)
    }

    /// Look up a code unit from a previous unit.
    pub fn get_external_function_as_unit(&self, unit: u32, func_id: FuncId) -> Option<&IrCodeUnit> {
        self.unit_registry.get_external_function_as_unit(unit, func_id)
    }

    /// Get all functions from a unit.
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrCodeUnit]> {
        self.unit_registry.unit_functions(unit)
    }

    /// Iterate over all module functions (legacy, returns IrFunction).
    pub fn iter_module_functions(&self) -> impl Iterator<Item = IrFunction> + '_ {
        self.module_registry.iter_module_functions()
    }

    /// Iterate over all module code units.
    pub fn iter_module_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.module_registry.iter_module_code_units()
    }

    /// Iterate over all module functions with their IDs (legacy).
    pub fn iter_module_functions_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, FuncId), IrFunction)> + '_ {
        self.module_registry.iter_module_functions_with_ids()
    }

    /// Iterate over all unit functions (legacy, returns IrFunction).
    pub fn iter_unit_functions(&self) -> impl Iterator<Item = IrFunction> + '_ {
        self.unit_registry.iter_unit_functions()
    }

    /// Iterate over all unit code units.
    pub fn iter_unit_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.unit_registry.iter_unit_code_units()
    }

    /// Iterate over all functions (modules + units, legacy).
    pub fn iter_all_functions(&self) -> impl Iterator<Item = IrFunction> + '_ {
        self.iter_module_functions().chain(self.iter_unit_functions())
    }

    /// Iterate over all code units (modules + units).
    pub fn iter_all_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.iter_module_code_units().chain(self.iter_unit_code_units())
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
