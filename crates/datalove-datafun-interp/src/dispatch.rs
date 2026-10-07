//! Call dispatch trait for extensibility.
//!
//! Allows external code (like a JIT) to intercept function calls.

use std::any::Any;

use datalove_datafun_ir::{CodeRef, CodeUnitId, IrCodeUnit, IrModuleId};
use datalove_rt::c::LocalRtHandle;

use crate::error::InterpError;
use crate::env::{ExecutionContext, FunctionRegistry};
use crate::frame::FrameStore;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

/// A function, named so that two units cannot mean the same thing by it.
///
/// `CodeRef::Local` is relative to whichever unit's list is in scope, so it is
/// not something to remember a function by: every script unit numbers its own
/// functions from zero. Resolving it against that unit gives a name that holds
/// still, and one a later unit's `CodeRef::External` agrees with, since both
/// denote the same function.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FuncIdentity {
    /// A script unit's function: the unit that owns it, and its id there.
    Unit { unit: u32, id: CodeUnitId },
    /// A module function, which is already unambiguous.
    Module { module: IrModuleId, id: CodeUnitId },
}

impl FuncIdentity {
    /// Name the function a reference reaches.
    ///
    /// `scope_unit` is the unit whose local functions are in scope, which is
    /// what a `Local` reference is relative to.
    pub fn of(code_ref: &CodeRef, scope_unit: u32) -> Self {
        match code_ref {
            CodeRef::Local(id) => FuncIdentity::Unit { unit: scope_unit, id: *id },
            CodeRef::External { unit, id } => FuncIdentity::Unit { unit: *unit, id: *id },
            CodeRef::Module { module, id } => FuncIdentity::Module { module: *module, id: *id },
        }
    }
}

/// Result of dispatching a call.
pub enum DispatchResult {
    /// Call was handled by the dispatcher.
    Handled(Result<(), InterpError>),
    /// Fall through to interpreter.
    NotHandled,
}

/// Context provided to the dispatcher for mixed-mode execution.
///
/// This contains everything needed for JIT code to call back to the interpreter.
pub struct DispatchCallContext<'a, 'b> {
    /// Execution context with local functions.
    pub exec_ctx: &'a ExecutionContext<'a>,
    /// Function registry for module/external function lookup.
    pub registry: &'a FunctionRegistry,
    /// Frame store for interpreter state.
    pub frames: &'a mut FrameStore,
    /// Interpreter for executing non-compiled functions.
    pub interp: &'b mut IrInterpreter,
    /// A descriptor for each shape the callee declared, worked out by the call
    /// site, in the order the callee declared them.
    ///
    /// A shape has no value to travel with, so unlike an argument's descriptor
    /// it has to be handed over on its own. Empty for a callee that builds no
    /// collection of a type it was not told, which is every one outside a
    /// generic.
    pub shape_descriptors: &'a [*const datalove_rtdt::TyDesc],
}

/// Trait for intercepting function calls.
///
/// Implement this to provide custom call dispatch, such as JIT compilation.
pub trait CallDispatcher {
    /// Attempt to dispatch a function call.
    ///
    /// Returns `DispatchResult::Handled` if the call was executed,
    /// or `DispatchResult::NotHandled` to fall through to the interpreter.
    ///
    /// # Arguments
    ///
    /// * `code_ref` - The code reference being called
    /// * `func` - The resolved function as IrCodeUnit
    /// * `args` - Arguments as Value pointers
    /// * `ret_dest` - Destination for return value
    /// * `rt_handle` - Runtime handle
    /// * `call_ctx` - Context for mixed-mode execution (JIT calling interpreter)
    fn dispatch_call(
        &mut self,
        code_ref: &CodeRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult;

    /// Convert to Any for downcasting.
    ///
    /// Used to access dispatcher-specific state (like JIT stats) after execution.
    fn as_any(&self) -> &dyn Any;

    /// Convert to mutable Any for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
