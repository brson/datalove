//! Call dispatch trait for extensibility.
//!
//! Allows external code (like a JIT) to intercept function calls.

use datalove_datafun_ir::{FuncRef, IrFunction};
use datalove_rt::c::LocalRtHandle;

use crate::error::InterpError;
use crate::value::{Destination, Value};

/// Result of dispatching a call.
pub enum DispatchResult {
    /// Call was handled by the dispatcher.
    Handled(Result<(), InterpError>),
    /// Fall through to interpreter.
    NotHandled,
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
    fn dispatch_call(
        &mut self,
        func_ref: &FuncRef,
        func: &IrFunction,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
    ) -> DispatchResult;
}
