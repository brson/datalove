//! Function registry for storing compiled IR functions.
//!
//! Used by both the interpreter and AOT compiler to store and look up
//! module functions and script unit functions.

use std::collections::HashMap;
use crate::{IrFunction, FuncId, IrModuleId};

/// Registry of functions from modules and previous script units.
///
/// Immutable after setup - can be borrowed while frames are mutated.
pub struct FunctionRegistry {
    /// Functions from each unit, indexed by unit number.
    unit_functions: Vec<Vec<IrFunction>>,
    /// Functions from modules, indexed by (module_id, func_id).
    module_functions: HashMap<(IrModuleId, FuncId), IrFunction>,
}

impl FunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            unit_functions: Vec::new(),
            module_functions: HashMap::new(),
        }
    }

    /// Add a module function.
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.module_functions.insert((module_id, func_id), func);
    }

    /// Add functions from a completed unit.
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_functions.push(functions);
    }

    /// Get a module function by module and function ID.
    pub fn get_module_function(&self, module_id: IrModuleId, func_id: FuncId) -> Option<&IrFunction> {
        self.module_functions.get(&(module_id, func_id))
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

    /// Iterate over all module functions.
    pub fn iter_module_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.module_functions.values()
    }

    /// Iterate over all module functions with their IDs.
    pub fn iter_module_functions_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, FuncId), &IrFunction)> {
        self.module_functions.iter().map(|((m, f), func)| ((*m, *f), func))
    }

    /// Iterate over all unit functions.
    pub fn iter_unit_functions(&self) -> impl Iterator<Item = &IrFunction> {
        self.unit_functions.iter().flat_map(|v| v.iter())
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
