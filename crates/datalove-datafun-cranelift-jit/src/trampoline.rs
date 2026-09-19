//! JIT-to-interpreter trampoline for mixed-mode execution.
//!
//! When JIT-compiled code calls a function that hasn't been compiled yet,
//! it needs to call back into the interpreter. This module provides the
//! dispatch mechanism.
//!
//! # Architecture
//!
//! Each call site in JIT code goes through a stub function. The stub calls
//! `dispatch_call` with the target function info. `dispatch_call` routes to
//! either JIT code or the interpreter based on compilation status.
//!
//! # Thread-local Context
//!
//! Since we're single-threaded, we use a thread-local to store the dispatch
//! context (interpreter, JIT engine, function registry) during execution.

use std::cell::RefCell;
use std::ptr::NonNull;

use datalove_datafun_ir::{CodeRef, CodeUnitId, IrModuleId};
use datalove_datafun_interp::{
    Destination, ExecutionContext, FrameStore, FuncIdentity, FunctionRegistry, IrInterpreter,
    Value,
};
use datalove_rt::c::LocalRtHandle;
use datalove_rtdt as rtdt;

use crate::{FunctionKey, JitEngine};

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
    /// Thread-local dispatch context, set during JIT execution.
    static DISPATCH_CONTEXT: RefCell<Option<NonNull<DispatchContext<'static>>>> = const { RefCell::new(None) };
}

/// Set the dispatch context for JIT trampolines.
///
/// # Safety
///
/// The context must remain valid for the duration of JIT execution.
/// Caller must call `clear_dispatch_context` before the context is dropped.
pub unsafe fn set_dispatch_context(ctx: &mut DispatchContext<'_>) {
    // SAFETY: We're extending the lifetime, but caller guarantees validity.
    let ptr = unsafe {
        NonNull::new_unchecked(ctx as *mut DispatchContext<'_> as *mut DispatchContext<'static>)
    };
    DISPATCH_CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(ptr);
    });
}

/// Clear the dispatch context.
pub fn clear_dispatch_context() {
    DISPATCH_CONTEXT.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Get the current dispatch context.
///
/// Returns None if no context is set.
fn get_dispatch_context() -> Option<NonNull<DispatchContext<'static>>> {
    DISPATCH_CONTEXT.with(|cell| *cell.borrow())
}

/// Encoded function key for dispatch.
///
/// Packed as: kind (2 bits) | unit_or_module (30 bits) | func_id (32 bits)
/// - kind 0: local (module_or_unit = 0)
/// - kind 1: module (module_or_unit = module_id)
/// - kind 2: external (module_or_unit = unit)
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct EncodedFuncKey(u64);

impl EncodedFuncKey {
    pub fn from_code_ref(code_ref: &CodeRef) -> Self {
        match code_ref {
            CodeRef::Local(id) => {
                let func = id.0 as u64;
                Self(func) // kind=0
            }
            CodeRef::Module { module, id } => {
                let kind = 1u64 << 62;
                let mod_id = (module.0 as u64 & 0x3FFFFFFF) << 32;
                let func_id = id.0 as u64;
                Self(kind | mod_id | func_id)
            }
            CodeRef::External { unit, id } => {
                let kind = 2u64 << 62;
                let unit_val = (*unit as u64 & 0x3FFFFFFF) << 32;
                let func_id = id.0 as u64;
                Self(kind | unit_val | func_id)
            }
        }
    }

    pub fn to_code_ref(self) -> CodeRef {
        let kind = (self.0 >> 62) & 0x3;
        let mid_bits = ((self.0 >> 32) & 0x3FFFFFFF) as u32;
        let id = CodeUnitId(self.0 as u32);

        match kind {
            0 => CodeRef::Local(id),
            1 => CodeRef::Module {
                module: IrModuleId(mid_bits),
                id,
            },
            2 => CodeRef::External {
                unit: mid_bits,
                id,
            },
            _ => unreachable!(),
        }
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn from_u64(v: u64) -> Self {
        Self(v)
    }
}

impl From<&CodeRef> for EncodedFuncKey {
    fn from(code_ref: &CodeRef) -> Self {
        Self::from_code_ref(code_ref)
    }
}

/// Dispatch a call from JIT code to either JIT-compiled code or interpreter.
///
/// This is called by stub functions generated for each call site.
///
/// # Arguments
///
/// * `rt_handle` - Runtime handle for memory operations
/// * `encoded_key` - Encoded FunctionKey identifying the target
/// * `ret_dest` - Pointer to return value destination (NULL for void)
/// * `_ret_is_sret` - Non-zero if return uses sret convention (unused, info in ir_func)
/// * `arg_count` - Number of arguments
/// * `args` - Pointer to array of argument pointers
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
    _ret_is_sret: u8,
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
    let code_ref = EncodedFuncKey::from_u64(encoded_key).to_code_ref();
    let func_key = FunctionKey::of(&code_ref, ctx.exec_ctx.unit());

    // Look up the function IR using ExecutionContext (handles Local, Module, External).
    let ir_unit = ctx.exec_ctx.get_unit(&code_ref, ctx.registry);
    let func_ctx = ir_unit.function_context()
        .expect("trampoline dispatch requires a function code unit");

    // What describes this callee's parameters and return is fixed by its
    // signature, so it is read off the layout the interpreter already keeps for
    // the function rather than looked up a type at a time. Every call out of
    // compiled code arrives here, and looking each one up was the second largest
    // entry in the jit's profile after this function itself.
    let layout = ctx.interp.layout_for(
        FuncIdentity::of(&code_ref, ctx.exec_ctx.unit()), ir_unit);

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

    // Build return destination.
    let dest = Destination {
        ptr: ret_dest,
        tydesc: layout.return_tydesc.expect("a jit callee is a function"),
    };

    // Try to JIT compile the target function (or get existing compiled code).
    // This triggers compilation when the call count threshold is reached.
    match ctx.jit_engine.record_call_with_context(func_key, &ir_unit, ctx.exec_ctx, ctx.registry) {
        Ok(Some((code_ptr, uses_sret))) => {
            // Call JIT code directly.
            // SAFETY: code_ptr is valid JIT code.
            let result = unsafe {
                crate::bridge::call_jit(
                    code_ptr, uses_sret, rt_handle, &arg_vals, dest,
                    &func_ctx.return_type, &func_ctx.descriptor_params, &shape_descriptors,
                )
            };
            if let Err(e) = result {
                panic!("JIT dispatch: JIT call failed: {}", e);
            }
        }
        Ok(None) | Err(_) => {
            // Not yet compiled or compilation failed - fall back to interpreter.
            // For In-mode non-copy args, interpreter takes ownership and destroys them.
            // JIT caller must not access these args after the call returns.
            // Note: We pass None for func_unit since JIT trampolines don't track unit context.
            // This means dynamic inlining won't apply to JIT->interpreter callbacks.
            let result = ctx.interp.call_in_context_with_shapes(
                ir_unit,
                None,
                arg_vals,
                shape_descriptors,
                dest,
                ctx.exec_ctx,
                ctx.registry,
                ctx.frames,
            );
            if let Err(e) = result {
                panic!("JIT dispatch: interpreter call failed: {:?}", e);
            }
        }
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
    fn test_encoded_func_key_local() {
        let code_ref = CodeRef::Local(CodeUnitId(42));
        let encoded = EncodedFuncKey::from_code_ref(&code_ref);
        let decoded = encoded.to_code_ref();
        assert_eq!(decoded, code_ref);
    }

    #[test]
    fn test_encoded_func_key_module() {
        let code_ref = CodeRef::Module {
            module: IrModuleId(123),
            id: CodeUnitId(456),
        };
        let encoded = EncodedFuncKey::from_code_ref(&code_ref);
        let decoded = encoded.to_code_ref();
        assert_eq!(decoded, code_ref);
    }

    #[test]
    fn test_encoded_func_key_external() {
        let code_ref = CodeRef::External {
            unit: 7,
            id: CodeUnitId(99),
        };
        let encoded = EncodedFuncKey::from_code_ref(&code_ref);
        let decoded = encoded.to_code_ref();
        assert_eq!(decoded, code_ref);
    }
}
