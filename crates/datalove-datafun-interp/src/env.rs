//! Execution environment for function resolution.
//!
//! - `FunctionRegistry`: (re-exported from IR crate) Module functions and functions from previous script units.
//! - `ExecutionContext`: Local functions in the current unit.
//! - `ScriptEnvironment`: Combines registry with `FrameStore` for script execution.

use datalove_datafun_ir::{IrFunction, FuncId, FuncRef, IrModuleId};
use crate::error::InterpError;
use crate::frame::{Frame, FrameStore};

// Re-export FunctionRegistry from the IR crate.
pub use datalove_datafun_ir::FunctionRegistry;

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

    /// Add a module function.
    pub fn add_module_function(&mut self, module_id: IrModuleId, func_id: FuncId, func: IrFunction) {
        self.registry.add_module_function(module_id, func_id, func);
    }

    /// Add a completed unit's frame and functions.
    pub fn add_unit(&mut self, frame: Frame, functions: Vec<IrFunction>) {
        self.frames.add_frame(frame);
        self.registry.add_unit_functions(functions);
    }

    /// Destroy all values in all frames.
    pub fn destroy_all(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        self.frames.destroy_all(rt_handle);
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
    pub fn get_function<'b>(
        &'b self,
        func_ref: &FuncRef,
        registry: &'b FunctionRegistry,
    ) -> Result<&'b IrFunction, InterpError> {
        match func_ref {
            FuncRef::Local(id) => {
                self.functions.iter()
                    .find(|f| f.id == *id)
                    .ok_or(InterpError::FunctionNotFound(*id))
            }
            FuncRef::External { unit, func } => {
                registry.get_external_function(*unit, *func)
                    .ok_or(InterpError::ExternalUnitNotFound(*unit))
            }
            FuncRef::Module { module, func } => {
                registry.get_module_function(*module, *func)
                    .ok_or(InterpError::ModuleFunctionNotFound { module: *module, func: *func })
            }
        }
    }

    /// Look up a function and get the appropriate context for calling it.
    ///
    /// For local and module functions, returns the current context.
    /// For external functions, returns a context with that unit's functions.
    pub fn get_function_with_context<'b>(
        &'b self,
        func_ref: &FuncRef,
        registry: &'b FunctionRegistry,
    ) -> Result<(&'b IrFunction, Option<u32>), InterpError> {
        match func_ref {
            FuncRef::Local(id) => {
                let func = self.functions.iter()
                    .find(|f| f.id == *id)
                    .ok_or(InterpError::FunctionNotFound(*id))?;
                Ok((func, None)) // Use current context.
            }
            FuncRef::External { unit, func } => {
                let callee = registry.get_external_function(*unit, *func)
                    .ok_or(InterpError::ExternalUnitNotFound(*unit))?;
                Ok((callee, Some(*unit))) // Need context from this unit.
            }
            FuncRef::Module { module, func } => {
                let callee = registry.get_module_function(*module, *func)
                    .ok_or(InterpError::ModuleFunctionNotFound { module: *module, func: *func })?;
                Ok((callee, None)) // Module functions don't have local calls.
            }
        }
    }
}
