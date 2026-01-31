//! Call dispatch trait for extensibility.
//!
//! Allows external code (like a JIT or dynamic inliner) to intercept function calls.

use std::any::Any;

use datalove_datafun_ir::{CallSiteId, FuncId, FuncRef, IrFunction};
use datalove_rt::c::LocalRtHandle;

use crate::error::InterpError;
use crate::env::{ExecutionContext, FunctionRegistry};
use crate::frame::FrameStore;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

/// Script-relative function identifier.
///
/// Combines a unit index with a FuncId to identify functions relative to a
/// script execution context. `unit` is `None` for the current unit's local
/// functions, or `Some(n)` for functions from script unit n in the registry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScriptFuncId {
    /// The script unit containing this function, or None for current unit.
    pub unit: Option<u32>,
    /// The function ID within its unit.
    pub func_id: FuncId,
}

/// Information about the call site in the caller function.
///
/// Used for tracking call sites for dynamic inlining decisions.
#[derive(Clone, Copy, Debug)]
pub struct CallSiteInfo {
    /// The function containing this call site.
    pub caller: ScriptFuncId,
    /// The unique ID of this call site within the caller.
    pub call_site_id: CallSiteId,
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
    /// * `func_ref` - The function reference being called
    /// * `func` - The resolved function
    /// * `args` - Arguments as Value pointers
    /// * `ret_dest` - Destination for return value
    /// * `rt_handle` - Runtime handle
    /// * `call_ctx` - Context for mixed-mode execution (JIT calling interpreter)
    fn dispatch_call(
        &mut self,
        func_ref: &FuncRef,
        func: &IrFunction,
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

    /// Get an optimized version of a function if available.
    ///
    /// Called before executing a function to check if there's an inlined/optimized
    /// version that should be used instead. Returns None to use the original function.
    fn get_optimized_function(&self, _func_id: ScriptFuncId) -> Option<&IrFunction> {
        None
    }
}
