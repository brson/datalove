//! Call dispatch trait for extensibility.
//!
//! Allows external code (like a JIT or dynamic inliner) to intercept function calls.

use std::any::Any;
use std::rc::Rc;

use datalove_datafun_ir::{CallSiteId, CodeRef, CodeUnitId, IrCodeUnit, IrModuleId};
use datalove_rt::c::LocalRtHandle;

use crate::error::InterpError;
use crate::env::{ExecutionContext, FunctionRegistry};
use crate::frame::FrameStore;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

/// Information about the call site in the caller function.
///
/// Used for tracking call sites for dynamic inlining decisions.
#[derive(Clone, Debug)]
pub struct CallSiteInfo {
    /// The function containing this call site.
    pub caller: CodeRef,
    /// The script unit whose local scope `caller` names, where it is `Local`.
    pub caller_unit: u32,
    /// The unique ID of this call site within the caller.
    pub call_site_id: CallSiteId,
}

impl CallSiteInfo {
    /// Which function this call site is in, unambiguously.
    pub fn caller_identity(&self) -> FuncIdentity {
        FuncIdentity::of(&self.caller, self.caller_unit)
    }
}

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
    /// Information about the call site (for dynamic inlining).
    ///
    /// None for calls from JIT code or when caller context is unavailable.
    pub call_site_info: Option<CallSiteInfo>,
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
    /// Used to access dispatcher-specific state (like inliner stats) after execution.
    fn as_any(&self) -> &dyn Any;

    /// Convert to mutable Any for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Get an optimized version of a code unit if available.
    ///
    /// Called before executing a function to check if there's an inlined/optimized
    /// version that should be used instead. Returns None to use the original code unit.
    ///
    /// Shared rather than borrowed because the caller holds the dispatcher through
    /// a `RefCell` and cannot keep that borrow across the call it is about to
    /// make. Copying the body out instead was a deep clone of every block and
    /// instruction, once per entry to an optimized function, and it cost more
    /// than the inlining saved.
    fn get_optimized_function(&self, _func: FuncIdentity) -> Option<Rc<IrCodeUnit>> {
        None
    }
}
