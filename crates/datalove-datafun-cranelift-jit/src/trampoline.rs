//! JIT-to-interpreter trampoline for mixed-mode execution.
//!
//! When JIT-compiled code calls a function that hasn't been compiled yet,
//! it needs to call back into the interpreter. This module provides the
//! dispatch mechanism.
//!
//! # Architecture
//!
//! Compiled code calls each callee through a stub with the callee's own
//! signature. Once the callee is compiled the stub calls its code directly;
//! until then it calls `__jit_dispatch_call`, which counts the call toward
//! compiling the callee and otherwise runs it in the interpreter.
//!
//! # Thread-local Context
//!
//! Since we're single-threaded, we use a thread-local to store the dispatch
//! context (interpreter, JIT engine, function registry) during execution.

use std::cell::RefCell;
use std::ptr::NonNull;

use datalove_datafun_ir::{BlockId, CodeRef, CodeUnitId, IrCodeUnit, IrModuleId};
use datalove_datafun_interp::{
    CallDispatcher, CompiledEntry, Destination, DispatchCallContext, DispatchResult,
    ExecutionContext, FrameStore, FuncIdentity, FunctionRegistry, InterpError, IrInterpreter,
    SitePolicy, Value,
};
use datalove_rt::c::{self, LocalRtHandle};
use datalove_rtdt as rtdt;

use crate::{CallFrom, JitEngine, Recorded};

/// Context available during JIT execution for dispatch.
pub struct DispatchContext<'a> {
    /// JIT engine for looking up compiled functions.
    pub jit_engine: &'a mut JitEngine,
    /// Interpreter for executing non-compiled functions.
    pub interp: &'a mut IrInterpreter,
    /// Execution context with local functions.
    pub exec_ctx: &'a ExecutionContext<'a>,
    /// Function registry for module lookups.
    pub registry: &'a FunctionRegistry,
    /// Frame store for interpreter.
    pub frames: &'a mut FrameStore,
}

thread_local! {
    /// Why the compiled code running is abandoning its calls; see
    /// `abandon`.
    static ABANDONED_FOR: RefCell<Option<InterpError>> = const { RefCell::new(None) };

    /// Thread-local dispatch context, set during JIT execution.
    static DISPATCH_CONTEXT: RefCell<Option<NonNull<DispatchContext<'static>>>> = const { RefCell::new(None) };
}

/// The dispatch context a `set_dispatch_context` replaced, to be put back.
#[must_use = "the context replaced has to be restored"]
pub struct PreviousContext(Option<NonNull<DispatchContext<'static>>>);

/// Set the dispatch context for JIT trampolines, until restored.
///
/// Contexts nest: compiled code can call into the interpreter, which can
/// enter compiled code again, with a context of its own, before returning to
/// the code with the first.
///
/// # Safety
///
/// The context must remain valid for the duration of JIT execution.
/// Caller must call `restore_dispatch_context` with what this returns before
/// the context is dropped.
pub unsafe fn set_dispatch_context(ctx: &mut DispatchContext<'_>) -> PreviousContext {
    // SAFETY: We're extending the lifetime, but caller guarantees validity.
    let ptr = unsafe {
        NonNull::new_unchecked(ctx as *mut DispatchContext<'_> as *mut DispatchContext<'static>)
    };
    PreviousContext(DISPATCH_CONTEXT.with(|cell| cell.borrow_mut().replace(ptr)))
}

/// Put back the dispatch context a `set_dispatch_context` replaced.
pub fn restore_dispatch_context(previous: PreviousContext) {
    DISPATCH_CONTEXT.with(|cell| {
        *cell.borrow_mut() = previous.0;
    });
}

/// Abandon the compiled calls under way, for `error`.
///
/// Compiled code cannot return an interpreter's error, so the trampoline
/// notes it here, sets the runtime's flag, and returns; the code returns from
/// each frame as it sees the flag after a call, and where the interpreter
/// entered it, `take_abandonment` gives the error back. What those frames
/// hold is not destroyed one value at a time: once the error is out of every
/// frame, the interpreter has the runtime discard all its memory
/// (`IrInterpreter::discard_runtime_state`).
fn abandon(rt: LocalRtHandle, error: InterpError) {
    ABANDONED_FOR.with(|cell| *cell.borrow_mut() = Some(error));
    // SAFETY: the handle compiled code was given.
    unsafe { c::dtlv_rti_abandon(rt, c::ABANDONED_FOR_ERROR) };
}

/// Get ready to enter compiled code: give it the limit to the stack it checks
/// on entering each function.
pub(crate) fn entering(rt: LocalRtHandle) {
    // SAFETY: the handle compiled code is given.
    unsafe { c::dtlv_rti_set_stack_limit(rt, c::dtlv_rti_native_stack_limit(rt)) };
}

/// The error the compiled code just returned from was abandoned for, if it
/// was, clearing it; see `abandon`.
pub(crate) fn take_abandonment(rt: LocalRtHandle) -> Result<(), InterpError> {
    // SAFETY: the handle compiled code was given.
    match unsafe { c::dtlv_rti_take_abandoned(rt) } {
        0 => Ok(()),
        c::ABANDONED_FOR_ERROR => Err(ABANDONED_FOR.with(|cell| cell.borrow_mut().take())
            .expect("calls abandoned for an error")),
        c::ABANDONED_FOR_STACK => Err(InterpError::StackOverflow),
        other => unreachable!("abandoned for reason {other}"),
    }
}


/// Get the current dispatch context.
///
/// Returns None if no context is set.
fn get_dispatch_context() -> Option<NonNull<DispatchContext<'static>>> {
    DISPATCH_CONTEXT.with(|cell| *cell.borrow())
}

/// The callee a stub calls, packed into the one word it hands the trampoline.
///
/// An identity rather than the `CodeRef` the call was written with, because a
/// `CodeRef::Local` means a function of whichever unit's code it appears in,
/// and the trampoline cannot tell which that was: compiled code from one unit
/// calls compiled code from an earlier one directly, so the unit compiled code
/// was entered from need not be the unit whose code is calling. Resolved when
/// the stub is built, where the unit is known, it holds still.
///
/// Packed as: kind (2 bits) | unit or module (30 bits) | function id (32 bits),
/// with kind 0 a script unit's function and kind 1 a module's.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct EncodedFuncKey(u64);

impl EncodedFuncKey {
    pub fn from_identity(func: FuncIdentity) -> Self {
        let (kind, owner, id) = match func {
            FuncIdentity::Unit { unit, id } => (0u64, unit, id),
            FuncIdentity::Module { module, id } => (1u64, module.0, id),
        };
        assert!(owner < (1 << 30), "unit or module {} does not fit the encoding", owner);
        Self((kind << 62) | ((owner as u64) << 32) | id.0 as u64)
    }

    pub fn to_identity(self) -> FuncIdentity {
        let owner = ((self.0 >> 32) & 0x3FFFFFFF) as u32;
        let id = CodeUnitId(self.0 as u32);
        match self.0 >> 62 {
            0 => FuncIdentity::Unit { unit: owner, id },
            1 => FuncIdentity::Module { module: IrModuleId(owner), id },
            kind => unreachable!("no function kind {}", kind),
        }
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn from_u64(v: u64) -> Self {
        Self(v)
    }
}

/// Run a call from compiled code to a callee with no compiled code yet.
///
/// Called by the callee's stub. The call is counted, which may compile the
/// callee, in which case this call runs the new code and later ones reach it
/// from the stub directly; otherwise the interpreter runs it.
///
/// # Arguments
///
/// * `rt_handle` - Runtime handle for memory operations
/// * `encoded_key` - The callee's identity, encoded by `EncodedFuncKey`
/// * `ret_dest` - Pointer to return value destination (NULL for void)
/// * `arg_count` - Number of arguments
/// * `args` - Pointer to array of argument pointers
/// * `descriptors` - Pointer to the callee's descriptor arguments: one per
///   entry of its `descriptor_params`, then one per entry of its
///   `descriptor_shapes`, in those orders; NULL if it has none
///
/// # Safety
///
/// Caller must ensure:
/// - `dispatch_context` is set and valid
/// - `ret_dest` is valid if non-null
/// - `args` points to `arg_count` valid pointers
///
/// All results are written via ret_dest (sret convention). No register return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __jit_dispatch_call(
    rt_handle: LocalRtHandle,
    encoded_key: u64,
    ret_dest: *mut u8,
    arg_count: u32,
    args: *const *const u8,
    descriptors: *const *const rtdt::TyDesc,
) {
    // Get dispatch context.
    let ctx_ptr = match get_dispatch_context() {
        Some(ptr) => ptr,
        None => {
            panic!("JIT dispatch: no context set");
        }
    };

    // SAFETY: context is valid per set_dispatch_context contract.
    let ctx = unsafe { &mut *ctx_ptr.as_ptr() };

    // Decode target function.
    let key = EncodedFuncKey::from_u64(encoded_key).to_identity();
    let (callee_ctx, code_ref, ir_unit) = ctx.exec_ctx.resolve_identity(key, ctx.registry);
    let func_ctx = ir_unit.function_context()
        .expect("trampoline dispatch requires a function code unit");

    // What describes this callee's parameters and return is fixed by its
    // signature, so it is read off the layout the interpreter already keeps for
    // the function rather than looked up a type at a time. Every call out of
    // compiled code to a function not yet compiled arrives here.
    let layout = ctx.interp.layout_for(key, ir_unit);

    // Build argument Values.
    //
    // A parameter's own type describes what arrives at it, except where the
    // callee is generic and that type says `data` in place of a type
    // parameter. For those the caller supplied the real one, and using the
    // signature's would say `[data]` of memory that holds a `[u32]`.
    let mut arg_vals = Vec::with_capacity(arg_count as usize);
    for i in 0..arg_count as usize {
        let arg_ptr = unsafe { *args.add(i) };
        arg_vals.push(Value {
            ptr: arg_ptr as *mut u8,
            tydesc: layout.param_tydescs[i],
        });
    }
    for (k, param_id) in func_ctx.descriptor_params.iter().enumerate() {
        let supplied = unsafe { *descriptors.add(k) };
        arg_vals[param_id.0 as usize].tydesc = supplied;
    }

    // The shapes the callee builds a collection of come after the ones that
    // describe a parameter, which is the order `build_signature` lays out and
    // the order the caller's compiled code wrote them in. Reading only the
    // first group and dropping this one is what made a jit-compiled function
    // calling `sys/std/list.reversed` build its result from whatever was in the
    // register.
    let shape_start = func_ctx.descriptor_params.len();
    let shape_descriptors: Vec<*const rtdt::TyDesc> = (0..func_ctx.descriptor_shapes.len())
        .map(|k| unsafe { *descriptors.add(shape_start + k) })
        .collect();

    // Under generics by reference, one per type parameter comes last. A
    // dynamic argument arrives as a pointer alone, and what it is follows
    // from them; so does a dynamic return.
    let type_param_start = shape_start + func_ctx.descriptor_shapes.len();
    let type_params: Vec<*const rtdt::TyDesc> = (0..func_ctx.type_param_count as usize)
        .map(|k| unsafe { *descriptors.add(type_param_start + k) })
        .collect();
    for (i, value) in arg_vals.iter_mut().enumerate() {
        if value.tydesc.is_null() {
            value.tydesc = ctx.interp.describe(&func_ctx.param_types[i], &type_params);
        }
    }
    let mut return_tydesc = layout.return_tydesc.expect("a jit callee is a function");
    if return_tydesc.is_null() {
        return_tydesc = ctx.interp.describe(&func_ctx.return_type, &type_params);
    }
    // What goes after the arguments: the shapes', or the type parameters'.
    let trailing = if type_params.is_empty() { shape_descriptors } else { type_params };

    let dest = Destination {
        ptr: ret_dest,
        tydesc: return_tydesc,
    };

    // Count the call, which may compile the callee. Once it is compiled its
    // stubs call it directly and no longer come here.
    match ctx.jit_engine.record_call(key, ir_unit, 1, &callee_ctx, ctx.registry, ctx.interp) {
        Ok(Recorded::Compiled { entry, .. }) => {
            ctx.jit_engine.note_call(key, ir_unit, CallFrom::Native, true, 1);
            // SAFETY: code_ptr is compiled code for this callee.
            unsafe {
                crate::bridge::call_jit(
                    entry.code_ptr, entry.uses_sret, rt_handle, &arg_vals, dest,
                    &func_ctx.descriptor_params, &trailing,
                )
            }
        }
        Ok(Recorded::Interpret) => {
            ctx.jit_engine.note_call(key, ir_unit, CallFrom::Native, false, 1);
            // The engine, for what the interpreter calls in turn: the
            // dispatcher it belongs to is out of the interpreter for as long as
            // the compiled code calling this runs.
            ctx.interp.lend_dispatcher(Box::new(LentEngine(&mut *ctx.jit_engine)));
            // An `in` argument that is not copied belongs to the interpreter
            // now, which destroys it; the compiled caller does not touch it
            // after the call.
            let result = ctx.interp.call_in_context_with_shapes(
                ir_unit,
                Some(code_ref.clone()),
                arg_vals,
                trailing,
                dest,
                &callee_ctx,
                ctx.registry,
                ctx.frames,
            );
            drop(ctx.interp.reclaim_dispatcher());
            if let Err(e) = result {
                abandon(rt_handle, e);
            }
        }
        Err(e) => abandon(rt_handle, InterpError::RuntimeError(
            format!("compiling {} failed: {}", ir_unit.name, e))),
    }
}

/// A `JitEngine` lent to the interpreter while compiled code calls into it;
/// see `IrInterpreter::lend_dispatcher`.
///
/// Holds the engine by the pointer the trampoline's context has to it, which
/// nothing else uses until the interpreter returns and the loan is taken back.
struct LentEngine(*mut JitEngine);

impl LentEngine {
    fn engine(&mut self) -> &mut JitEngine {
        // SAFETY: lent for the length of one interpreter call, during which
        // the trampoline that lent it does not use it.
        unsafe { &mut *self.0 }
    }
}

impl CallDispatcher for LentEngine {
    fn dispatch_call(
        &mut self,
        code_ref: &CodeRef,
        func: &IrCodeUnit,
        args: &[Value],
        ret_dest: Destination,
        rt_handle: LocalRtHandle,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> DispatchResult {
        self.engine().dispatch_call(code_ref, func, args, ret_dest, rt_handle, call_ctx)
    }

    fn site_policy(&mut self, func: FuncIdentity, body: &IrCodeUnit) -> SitePolicy {
        self.engine().site_policy(func, body)
    }

    fn call_compiled(
        &mut self,
        func: FuncIdentity,
        entry: CompiledEntry,
        words: &[usize],
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> Result<(), InterpError> {
        self.engine().call_compiled(func, entry, words, call_ctx)
    }

    fn loop_policy(
        &mut self,
        func: FuncIdentity,
        body: &IrCodeUnit,
        header: BlockId,
        iterations: u32,
        call_ctx: DispatchCallContext<'_, '_>,
    ) -> Result<SitePolicy, InterpError> {
        self.engine().loop_policy(func, body, header, iterations, call_ctx)
    }


    fn bodies_replaced(&mut self) {
        self.engine().bodies_replaced();
    }

    fn unit_bodies_replaced(&mut self, unit: u32) {
        self.engine().unit_bodies_replaced(unit);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Get the dispatch function pointer for registration with JITBuilder.
pub fn dispatch_fn_ptr() -> *const u8 {
    __jit_dispatch_call as *const u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoded_func_key_unit() {
        let func = FuncIdentity::Unit { unit: 7, id: CodeUnitId(99) };
        assert_eq!(EncodedFuncKey::from_identity(func).to_identity(), func);
    }

    #[test]
    fn test_encoded_func_key_module() {
        let func = FuncIdentity::Module { module: IrModuleId(123), id: CodeUnitId(456) };
        assert_eq!(EncodedFuncKey::from_identity(func).to_identity(), func);
    }
}
