//! Execution environment for function resolution.
//!
//! - `FunctionRegistry`: (re-exported from IR crate) Module functions and functions from previous script units.
//! - `ModuleFunctionRegistry`: Shared, immutable after module compilation.
//! - `UnitFunctionRegistry`: Per-script, grows as units execute.
//! - `ExecutionContext`: Local functions in the current unit.
//! - `ScriptEnvironment`: Combines registry with `FrameStore` for script execution.

use std::sync::Arc;
use datalove_datafun_ir::{IrCodeUnit, CodeUnitId, CodeRef, IrModuleId, ValueId, SlotId};
use crate::dispatch::FuncIdentity;
use crate::frame::{FrameStore, ScriptFrame};

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
    pub fn add_module_code_unit(&mut self, module_id: IrModuleId, unit_id: CodeUnitId, unit: std::sync::Arc<IrCodeUnit>) {
        self.registry.add_module_code_unit(module_id, unit_id, unit);
    }

    /// Put a recompiled module registry in place of the one held.
    ///
    /// What an edited module's functions are called through: the frames and the
    /// units' own code stay as they are, since a module edit moves nothing a
    /// script unit holds.
    pub fn set_module_registry(&mut self, module_registry: Arc<ModuleFunctionRegistry>) {
        self.registry.set_module_registry(module_registry);
    }

    /// Add a completed unit's frame and code units.
    pub fn add_unit(
        &mut self,
        frame: ScriptFrame,
        code_units: Vec<IrCodeUnit>,
        unit_end_values: Vec<ValueId>,
        unit_end_slots: Vec<SlotId>,
    ) {
        self.frames.add_frame(frame, unit_end_values, unit_end_slots);
        self.registry.add_unit_code_units(code_units);
    }

    /// Put a re-executed unit's frame and code units in place of the ones it
    /// had, destroying what the old frame owned.
    ///
    /// Both halves have to move together: a `(unit, value)` reference and a
    /// `CodeRef::Local` are both positions in one unit's state, so a frame and
    /// the functions beside it belong at the same index.
    pub fn replace_unit(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit: u32,
        frame: ScriptFrame,
        code_units: Vec<IrCodeUnit>,
        unit_end_values: Vec<ValueId>,
        unit_end_slots: Vec<SlotId>,
    ) {
        self.frames.replace_frame(
            rt_handle, unit as usize, frame, unit_end_values, unit_end_slots);
        self.registry.set_unit_code_units(unit, code_units);
    }

    /// Drop every unit from `len` on, destroying what their frames owned.
    ///
    /// Both halves move together for the reason [`Self::replace_unit`] moves
    /// both: a `(unit, value)` reference and a `CodeRef::Local` are both
    /// positions in one unit's state, so a unit's frame and its functions
    /// belong at the same index and go at the same time.
    pub fn truncate_units(&mut self, rt_handle: datalove_rt::c::LocalRtHandle, len: usize) {
        self.frames.truncate_units(rt_handle, len);
        self.registry.truncate_units(len);
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
#[derive(Clone, Copy)]
pub struct ExecutionContext<'a> {
    /// The script unit these functions belong to.
    ///
    /// A `CodeRef::Local` names one of `functions` by id and says nothing
    /// about whose list that is, so every unit's ids start again at zero.
    /// Anything that remembers a function between calls -- the inliner's
    /// optimized bodies, the JIT's compiled ones -- has to key on this as well,
    /// or unit 1's function answers to unit 2's name.
    unit: u32,
    /// Local functions available for calling (from current unit).
    functions: &'a [IrCodeUnit],
}

impl<'a> ExecutionContext<'a> {
    /// Create a new execution context for one script unit's functions.
    pub fn new(unit: u32, functions: &'a [IrCodeUnit]) -> Self {
        Self { unit, functions }
    }

    /// The script unit whose local scope this is.
    pub fn unit(&self) -> u32 {
        self.unit
    }

    /// Find a local function by ID.
    ///
    /// Returns None if no local function with this ID exists.
    /// A function's id is usually its position, which is tried first; but not
    /// always, since compile-time evaluation runs with a list that leaves
    /// functions out, so the search by id remains.
    pub fn find_local_function(&self, id: CodeUnitId) -> Option<&'a IrCodeUnit> {
        match self.functions.get(id.0 as usize) {
            Some(f) if f.id == id => Some(f),
            _ => self.functions.iter().find(|f| f.id == id),
        }
    }

    /// Look up a function by reference.
    ///
    /// Panics if function not found (compiler bug).
    pub fn get_unit<'b>(&self, code_ref: &CodeRef, registry: &'b FunctionRegistry) -> &'b IrCodeUnit
    where
        'a: 'b,
    {
        match code_ref {
            CodeRef::Local(id) => {
                self.find_local_function(*id)
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

    /// Find the function `func` names: the context it runs in, a reference to
    /// it from that context, and its body.
    ///
    /// A function of this context's own unit is found here, because the unit
    /// that is running is not registered until it finishes. One of an earlier
    /// unit is found in the registry and runs with that unit's functions in
    /// scope.
    ///
    /// Panics if it is not there (compiler bug).
    pub fn resolve_identity<'b>(
        &self,
        func: FuncIdentity,
        registry: &'b FunctionRegistry,
    ) -> (ExecutionContext<'b>, CodeRef, &'b IrCodeUnit)
    where
        'a: 'b,
    {
        let code_ref = match func {
            FuncIdentity::Unit { unit, id } if unit == self.unit => CodeRef::Local(id),
            FuncIdentity::Unit { unit, id } => CodeRef::External { unit, id },
            FuncIdentity::Module { module, id } => CodeRef::Module { module, id },
        };
        let callee = self.get_unit(&code_ref, registry);
        (self.for_callee(&code_ref, registry), code_ref, callee)
    }

    /// The context a call to `code_ref` runs its callee in.
    ///
    /// A `CodeRef::Local` inside a function means that function's own unit, so
    /// a function from an earlier script unit runs with that unit's functions
    /// in scope rather than its caller's. Every other callee shares the
    /// caller's.
    ///
    /// Panics if the unit is not registered (compiler bug).
    pub fn for_callee<'b>(&self, code_ref: &CodeRef, registry: &'b FunctionRegistry) -> ExecutionContext<'b>
    where
        'a: 'b,
    {
        match code_ref {
            CodeRef::External { unit, .. } => {
                let functions = registry.unit_functions(*unit)
                    .unwrap_or_else(|| panic!("external unit {} not found", unit));
                ExecutionContext::new(*unit, functions)
            }
            CodeRef::Local(_) | CodeRef::Module { .. } => *self,
        }
    }
}
