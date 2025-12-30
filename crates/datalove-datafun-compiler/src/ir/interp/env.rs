//! Execution environment types for script and function execution.

use rmx::std::collections::HashMap;
use super::super::{IrFunction, FuncId, FuncRef};
use super::error::InterpError;
use super::frame::{Frame, FrameStore};

/// Registry of functions from modules and previous script units.
///
/// Immutable after setup - can be borrowed while frames are mutated.
pub struct FunctionRegistry {
    /// Functions from each unit, indexed by unit number.
    unit_functions: Vec<Vec<IrFunction>>,
    /// Functions from modules, indexed by name.
    module_functions: HashMap<String, IrFunction>,
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
    pub fn add_module_function(&mut self, name: String, func: IrFunction) {
        self.module_functions.insert(name, func);
    }

    /// Add functions from a completed unit.
    pub fn add_unit_functions(&mut self, functions: Vec<IrFunction>) {
        self.unit_functions.push(functions);
    }

    /// Get a module function by name.
    pub fn get_module_function(&self, name: &str) -> Option<&IrFunction> {
        self.module_functions.get(name)
    }

    /// Look up a function from a previous unit.
    pub fn external_function(&self, unit: u32, func_id: FuncId) -> Result<&IrFunction, InterpError> {
        let functions = self.unit_functions.get(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        functions.iter()
            .find(|f| f.id == func_id)
            .ok_or(InterpError::FunctionNotFound(func_id))
    }

    /// Get all functions from a unit.
    pub fn unit_functions(&self, unit: u32) -> Option<&[IrFunction]> {
        self.unit_functions.get(unit as usize).map(|v| v.as_slice())
    }
}

impl Default for FunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

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
    pub fn add_module_function(&mut self, name: String, func: IrFunction) {
        self.registry.add_module_function(name, func);
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
                registry.external_function(*unit, *func)
            }
            FuncRef::Module { name } => {
                registry.get_module_function(name)
                    .ok_or(InterpError::ModuleFunctionNotFound(name.clone()))
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
                let callee = registry.external_function(*unit, *func)?;
                Ok((callee, Some(*unit))) // Need context from this unit.
            }
            FuncRef::Module { name } => {
                let callee = registry.get_module_function(name)
                    .ok_or(InterpError::ModuleFunctionNotFound(name.clone()))?;
                Ok((callee, None)) // Module functions don't have local calls.
            }
        }
    }
}
