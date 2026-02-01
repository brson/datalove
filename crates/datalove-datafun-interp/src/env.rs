//! Execution environment for function resolution.
//!
//! - `FunctionRegistry`: (re-exported from IR crate) Module functions and functions from previous script units.
//! - `ModuleFunctionRegistry`: Shared, immutable after module compilation.
//! - `UnitFunctionRegistry`: Per-script, grows as units execute.
//! - `ExecutionContext`: Local functions in the current unit.
//! - `ScriptEnvironment`: Combines registry with `FrameStore` for script execution.

use std::sync::Arc;
use datalove_datafun_ir::{IrCodeUnit, CodeUnitId, CodeRef, IrModuleId, ValueId, SlotId};
use crate::frame::{Frame, FrameStore};

// Re-export registry types from the IR crate.
pub use datalove_datafun_ir::{FunctionRegistry, ModuleFunctionRegistry, UnitFunctionRegistry};

/// Combined environment for script execution (convenience wrapper).
pub struct ScriptEnvironment {
    pub registry: FunctionRegistry,
    pub frames: FrameStore,
}

impl ScriptEnvironment {
    pub fn new() -> Self {
        Self {
            registry: FunctionRegistry::new(),
            frames: FrameStore::new(),
        }
    }

    /// Create with an existing module registry.
    ///
    /// Used when creating multiple script contexts that share module functions.
    pub fn with_module_registry(module_registry: Arc<ModuleFunctionRegistry>) -> Self {
        Self {
            registry: FunctionRegistry::with_module_registry(module_registry),
            frames: FrameStore::new(),
        }
    }

    /// Add a module code unit.
    pub fn add_module_code_unit(&mut self, module_id: IrModuleId, unit_id: CodeUnitId, unit: IrCodeUnit) {
        self.registry.add_module_code_unit(module_id, unit_id, unit);
    }

    /// Add a completed unit's frame and code units.
    pub fn add_unit(
        &mut self,
        frame: Frame,
        code_units: Vec<IrCodeUnit>,
        unit_end_values: Vec<ValueId>,
        unit_end_slots: Vec<SlotId>,
    ) {
        self.frames.add_frame(frame, unit_end_values, unit_end_slots);
        self.registry.add_unit_code_units(code_units);
    }

    /// Destroy live values in all frames.
    pub fn destroy_live_values(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        self.frames.destroy_live_values(rt_handle);
    }
}

impl Default for ScriptEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

/// Execution context holding available functions.
pub struct ExecutionContext<'a> {
    /// Local functions available for calling (from current unit).
    functions: &'a [IrCodeUnit],
}

impl<'a> ExecutionContext<'a> {
    /// Create a new execution context with the given functions.
    pub fn new(functions: &'a [IrCodeUnit]) -> Self {
        Self { functions }
    }

    /// Find a local function by ID.
    ///
    /// Returns None if no local function with this ID exists.
    pub fn find_local_function(&self, unit_id: CodeUnitId) -> Option<&IrCodeUnit> {
        self.functions.iter().find(|f| f.id.0 == unit_id.0)
    }

    /// Look up a function by reference.
    ///
    /// Panics if function not found (compiler bug).
    pub fn get_unit<'b>(
        &'b self,
        code_ref: &CodeRef,
        registry: &'b FunctionRegistry,
    ) -> &'b IrCodeUnit {
        match code_ref {
            CodeRef::Local(id) => {
                self.functions.iter()
                    .find(|f| f.id.0 == id.0)
                    .unwrap_or_else(|| panic!("local unit {:?} not found", id))
            }
            CodeRef::External { unit, id } => {
                registry.get_external_function_as_unit(*unit, *id)
                    .unwrap_or_else(|| panic!("external unit unit={} id={:?} not found", unit, id))
            }
            CodeRef::Module { module, id } => {
                registry.get_module_function_as_unit(*module, *id)
                    .unwrap_or_else(|| panic!("module unit {:?}::{:?} not found", module, id))
            }
        }
    }

    /// Look up a function and get the appropriate context for calling it.
    ///
    /// For local and module functions, returns the current context.
    /// For external functions, returns a context with that unit's functions.
    ///
    /// Panics if function not found (compiler bug).
    pub fn get_unit_with_context<'b>(
        &'b self,
        code_ref: &CodeRef,
        registry: &'b FunctionRegistry,
    ) -> (&'b IrCodeUnit, Option<u32>) {
        match code_ref {
            CodeRef::Local(id) => {
                let func = self.functions.iter()
                    .find(|f| f.id.0 == id.0)
                    .unwrap_or_else(|| panic!("local unit {:?} not found", id));
                (func, None) // Use current context.
            }
            CodeRef::External { unit, id } => {
                let callee = registry.get_external_function_as_unit(*unit, *id)
                    .unwrap_or_else(|| panic!("external unit unit={} id={:?} not found", unit, id));
                (callee, Some(*unit)) // Need context from this unit.
            }
            CodeRef::Module { module, id } => {
                // Module calls resolve via CodeRef::Module (through registry),
                // not CodeRef::Local, so they don't use ExecutionContext.
                let callee = registry.get_module_function_as_unit(*module, *id)
                    .unwrap_or_else(|| panic!("module unit {:?}::{:?} not found", module, id));
                (callee, None)
            }
        }
    }
}
