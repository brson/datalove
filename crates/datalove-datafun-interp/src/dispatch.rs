//! Call dispatch trait for extensibility.
//!
//! Allows external code (like a JIT) to intercept function calls.

use datalove_datafun_ir::{FuncRef, IrFunction};
use datalove_rt::c::LocalRtHandle;

use crate::error::InterpError;
use crate::env::{ExecutionContext, FunctionRegistry};
use crate::frame::FrameStore;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

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
}
