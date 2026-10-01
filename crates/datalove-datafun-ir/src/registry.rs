//! Function registry for storing compiled IR code units.
//!
//! Split into shareable and per-script components:
//! - `ModuleFunctionRegistry`: shared, immutable after module compilation
//! - `UnitFunctionRegistry`: per-script, grows as units execute
//! - `FunctionRegistry`: combined view

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use crate::{IrCodeUnit, CodeUnitId, IrModuleId};

/// One module's code units, by id.
///
/// Held behind an `Arc` by the registry so that rebuilding the registry when
/// one module's IR moves does not rebuild the entry for every function in the
/// world -- see [`ModuleFunctionRegistry`].
pub type ModuleCodeUnits = BTreeMap<CodeUnitId, Arc<IrCodeUnit>>;

/// Registry of module functions, shared across all scripts.
///
/// Immutable after module compilation completes.
///
/// Ordered, because the backends walk it to declare functions and the order
/// they are declared in decides the identifiers and the layout of the object
/// file. A hash map made that order depend on the seed the process started
/// with, so the same input produced different bytes on every run.
///
/// Two levels rather than one map keyed on the pair, because the whole thing
/// is rebuilt whenever any module's IR moves and there is an entry per function
/// in the program. Split by module, a module the edit did not touch is one
/// `Arc` clone rather than one insert per function it declares. Iterating the
/// two levels in order visits the same units in the same order as the pair-keyed
/// map did, which is the part the backends depend on.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModuleFunctionRegistry {
    /// Behind an `Arc` because the registry is built from IR that is already
    /// owned elsewhere -- the lowered functions, and the assembled module
    /// results -- and copying every instruction to put it here was about a
    /// quarter of what recompiling an unchanged world cost.
    modules: BTreeMap<IrModuleId, Arc<ModuleCodeUnits>>,
}

impl ModuleFunctionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
        }
    }

    /// Add a module code unit.
    pub fn add_module_code_unit(&mut self, module_id: IrModuleId, func_id: CodeUnitId, unit: Arc<IrCodeUnit>) {
        Arc::make_mut(self.modules.entry(module_id).or_default()).insert(func_id, unit);
    }

    /// Put a whole module's units in at once.
    ///
    /// The incremental path: the units come out of a memo keyed on the module's
    /// lowering result, so a module that did not change costs a refcount.
    pub fn set_module_code_units(&mut self, module_id: IrModuleId, units: Arc<ModuleCodeUnits>) {
        self.modules.insert(module_id, units);
    }

    /// Get a module code unit by module and function ID.
    pub fn get_module_function_as_unit(&self, module_id: IrModuleId, func_id: CodeUnitId) -> Option<&IrCodeUnit> {
        self.modules.get(&module_id)?.get(&func_id).map(|unit| &**unit)
    }

    /// Iterate over all module code units.
    pub fn iter_module_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.modules.values().flat_map(|units| units.values()).map(|unit| &**unit)
    }

    /// Iterate over all module code units with their IDs.
    pub fn iter_module_code_units_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, CodeUnitId), &IrCodeUnit)> {
        self.modules.iter().flat_map(|(module_id, units)| {
            units.iter().map(move |(func_id, unit)| ((*module_id, *func_id), &**unit))
        })
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

    /// Add code units from a completed unit.
    pub fn add_unit_code_units(&mut self, units: Vec<IrCodeUnit>) {
        self.unit_functions.push(units);
    }

    /// Put a re-executed unit's code units in place of the ones it had.
    ///
    /// A `CodeRef::Local` names a position in one unit's list, so a unit's
    /// functions have to sit at that unit's index and nowhere else.
    pub fn set_unit_code_units(&mut self, unit: u32, units: Vec<IrCodeUnit>) {
        let index = unit as usize;
        assert!(
            index < self.unit_functions.len(),
            "unit {unit} has no code units to replace; the registry holds {}",
            self.unit_functions.len(),
        );
        self.unit_functions[index] = units;
    }

    /// Drop the code units of every unit from `len` on.
    ///
    /// The other half of truncating the frame store: a `CodeRef::Local` names a
    /// position in one unit's list, so a unit's functions have to go when its
    /// frame does or the next unit to take that index inherits them.
    pub fn truncate_units(&mut self, len: usize) {
        assert!(
            len <= self.unit_functions.len(),
            "cannot truncate to {len} units; the registry holds {}",
            self.unit_functions.len(),
        );
        self.unit_functions.truncate(len);
    }

    /// Look up a code unit from a previous unit.
    pub fn get_external_function_as_unit(&self, unit: u32, func_id: CodeUnitId) -> Option<&IrCodeUnit> {
        let functions = self.unit_functions.get(unit as usize)?;
        functions.iter().find(|f| f.id.0 == func_id.0)
    }

    /// Get all code units from a unit.
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrCodeUnit]> {
        self.unit_functions.get(unit as usize).map(|v| v.as_slice())
    }

    /// How many units have been completed.
    ///
    /// A unit is added once it has finished, so this is also the index the
    /// unit now running will take.
    pub fn unit_count(&self) -> u32 {
        self.unit_functions.len() as u32
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

    /// Put a recompiled module registry in place of the one held.
    ///
    /// A script unit names a module function by `CodeRef::Module`, so editing a
    /// module changes nothing the script unit holds and everything about what
    /// that reference lands on. The whole registry is replaced rather than the
    /// edited module's entry, because the numbering is the compilation's to
    /// decide and a registry half from one compilation and half from another
    /// would be nobody's answer.
    pub fn set_module_registry(&mut self, module_registry: Arc<ModuleFunctionRegistry>) {
        self.module_registry = module_registry;
    }

    /// Get a reference to the unit registry.
    pub fn unit_registry(&self) -> &UnitFunctionRegistry {
        &self.unit_registry
    }

    /// Get a mutable reference to the unit registry.
    pub fn unit_registry_mut(&mut self) -> &mut UnitFunctionRegistry {
        &mut self.unit_registry
    }

    /// Add a module code unit.
    pub fn add_module_code_unit(&mut self, module_id: IrModuleId, func_id: CodeUnitId, unit: Arc<IrCodeUnit>) {
        self.module_registry_mut().add_module_code_unit(module_id, func_id, unit);
    }

    /// Add code units from a completed unit.
    pub fn add_unit_code_units(&mut self, units: Vec<IrCodeUnit>) {
        self.unit_registry.add_unit_code_units(units);
    }

    /// Put a re-executed unit's code units in place of the ones it had.
    pub fn set_unit_code_units(&mut self, unit: u32, units: Vec<IrCodeUnit>) {
        self.unit_registry.set_unit_code_units(unit, units);
    }

    /// Drop the code units of every unit from `len` on.
    pub fn truncate_units(&mut self, len: usize) {
        self.unit_registry.truncate_units(len);
    }

    /// Get a module code unit by module and function ID.
    pub fn get_module_function_as_unit(&self, module_id: IrModuleId, func_id: CodeUnitId) -> Option<&IrCodeUnit> {
        self.module_registry.get_module_function_as_unit(module_id, func_id)
    }

    /// Look up a code unit from a previous unit.
    pub fn get_external_function_as_unit(&self, unit: u32, func_id: CodeUnitId) -> Option<&IrCodeUnit> {
        self.unit_registry.get_external_function_as_unit(unit, func_id)
    }

    /// Get all code units from a unit.
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrCodeUnit]> {
        self.unit_registry.unit_functions(unit)
    }

    /// How many units have been completed; also the index of the one running.
    pub fn unit_count(&self) -> u32 {
        self.unit_registry.unit_count()
    }

    /// Iterate over all module code units.
    pub fn iter_module_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.module_registry.iter_module_code_units()
    }

    /// Iterate over all module code units with their IDs.
    pub fn iter_module_code_units_with_ids(&self) -> impl Iterator<Item = ((IrModuleId, CodeUnitId), &IrCodeUnit)> {
        self.module_registry.iter_module_code_units_with_ids()
    }

    /// Iterate over all unit code units.
    pub fn iter_unit_code_units(&self) -> impl Iterator<Item = &IrCodeUnit> {
        self.unit_registry.iter_unit_code_units()
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
