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

/// The most words a compiled function is entered with: the runtime handle, the
/// `sret` pointer, and up to eight of arguments and descriptors.
pub const MAX_ENTRY_WORDS: usize = 10;

/// Compiled code for a function, entered with the words its signature takes.
///
/// Those are the runtime handle; the result's address if `uses_sret`; a
/// pointer to each argument; then a descriptor for each of the function's
/// `descriptor_params`, and one for each shape it declares.
#[derive(Clone, Copy, Debug)]
pub struct CompiledEntry {
    pub code_ptr: *const u8,
    pub uses_sret: bool,
}

/// How a call site the interpreter has planned is to make calls to a function.
///
/// A planned call is made without asking anything, so a dispatcher that is to
/// see calls has to say how it wants to see them. The interpreter asks when it
/// plans the site, and asks again when the answer runs out.
#[derive(Clone, Copy, Debug)]
pub enum SitePolicy {
    /// Offer every call to `dispatch_call`, planning none.
    EveryCall,
    /// Run every call in the interpreter, never offering it.
    Interpret,
    /// Run calls in the interpreter, and offer every `n`th to `dispatch_call`
    /// as standing for `n` (`DispatchCallContext::weight`).
    Count(u32),
    /// Enter compiled code, through `call_compiled`.
    Enter(CompiledEntry),
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
    /// How many calls this one stands for: more than one when a planned call
    /// site counted calls it did not offer (`SitePolicy::Count`).
    pub weight: u32,
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

    /// How a planned call site is to make calls to `func`, whose body is
    /// `body`, from now on.
    ///
    /// The default sees every call, as a dispatcher always has.
    fn site_policy(&mut self, func: FuncIdentity, body: &IrCodeUnit) -> SitePolicy {
        let _ = (func, body);
        SitePolicy::EveryCall
    }

    /// Enter `entry`, which `site_policy` gave for `func`, with `words`.
    ///
    /// `call_ctx` is for the compiled code to call back into the interpreter
    /// with; its shape descriptors are already among the words.
    fn call_compiled(
        &mut self,
        func: FuncIdentity,
        entry: CompiledEntry,
        words: &[usize],
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> Result<(), InterpError> {
        let _ = (func, entry, words, call_ctx);
        unreachable!("a dispatcher that gives no compiled entries was asked to enter one")
    }

    /// Convert to Any for downcasting.
    ///
    /// Used to access dispatcher-specific state (like JIT stats) after execution.
    fn as_any(&self) -> &dyn Any;

    /// Convert to mutable Any for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
