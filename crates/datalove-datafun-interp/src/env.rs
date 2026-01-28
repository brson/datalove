//! Execution environment for function resolution.
//!
//! - `FunctionRegistry`: (re-exported from IR crate) Module functions and functions from previous script units.
//! - `ModuleFunctionRegistry`: Shared, immutable after module compilation.
//! - `UnitFunctionRegistry`: Per-script, grows as units execute.
//! - `ExecutionContext`: Local functions in the current unit.
//! - `ScriptEnvironment`: Combines registry with `FrameStore` for script execution.

use std::sync::Arc;
use datalove_datafun_ir::{IrFunction, FuncId, FuncRef, IrModuleId, ValueId, SlotId};
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

    /// Add a module function.
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.registry.add_module_function(module_id, func_id, func);
    }

    /// Add a completed unit's frame and functions.
    pub fn add_unit(
        &mut self,
        frame: Frame,
        functions: Vec<IrFunction>,
        unit_end_values: Vec<ValueId>,
        unit_end_slots: Vec<SlotId>,
    ) {
        self.frames.add_frame(frame, unit_end_values, unit_end_slots);
        self.registry.add_unit_functions(functions);
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
    functions: &'a [IrFunction],
}

impl<'a> ExecutionContext<'a> {
    /// Create a new execution context with the given functions.
    pub fn new(functions: &'a [IrFunction]) -> Self {
        Self { functions }
    }

    /// Look up a function by reference.
    ///
    /// Panics if function not found (compiler bug).
    pub fn get_function<'b>(
        &'b self,
        func_ref: &FuncRef,
        registry: &'b FunctionRegistry,
    ) -> &'b IrFunction {
        match func_ref {
            FuncRef::Local(id) => {
                self.functions.iter()
                    .find(|f| f.id == *id)
                    .unwrap_or_else(|| panic!("local function {:?} not found", id))
            }
            FuncRef::External { unit, func } => {
                registry.get_external_function(*unit, *func)
                    .unwrap_or_else(|| panic!("external function unit={} func={:?} not found", unit, func))
            }
            FuncRef::Module { module, func } => {
                registry.get_module_function(*module, *func)
                    .unwrap_or_else(|| panic!("module function {:?}::{:?} not found", module, func))
            }
        }
    }

    /// Look up a function and get the appropriate context for calling it.
    ///
    /// For local and module functions, returns the current context.
    /// For external functions, returns a context with that unit's functions.
    ///
    /// Panics if function not found (compiler bug).
    pub fn get_function_with_context<'b>(
        &'b self,
        func_ref: &FuncRef,
        registry: &'b FunctionRegistry,
    ) -> (&'b IrFunction, Option<u32>) {
        match func_ref {
            FuncRef::Local(id) => {
                let func = self.functions.iter()
                    .find(|f| f.id == *id)
                    .unwrap_or_else(|| panic!("local function {:?} not found", id));
                (func, None) // Use current context.
            }
            FuncRef::External { unit, func } => {
                let callee = registry.get_external_function(*unit, *func)
                    .unwrap_or_else(|| panic!("external function unit={} func={:?} not found", unit, func));
                (callee, Some(*unit)) // Need context from this unit.
            }
            FuncRef::Module { module, func } => {
                let callee = registry.get_module_function(*module, *func)
                    .unwrap_or_else(|| panic!("module function {:?}::{:?} not found", module, func));
                (callee, None) // Module functions don't have local calls.
            }
        }
    }
}
