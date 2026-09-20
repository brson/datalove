//! Calling convention bridge between interpreter and JIT code.
//!
//! Converts between interpreter's Value/Destination and JIT's raw pointer convention.
//! All function returns use sret (pointer-based), so the bridge only needs void dispatch.

use datalove_datafun_interp::{Destination, Value};
use datalove_datafun_ir::{IrCodeUnit, IrType, ParamId};
use datalove_rt::c::LocalRtHandle;
use datalove_rtdt as rtdt;

/// Maximum number of function parameters supported by direct dispatch.
const MAX_DIRECT_ARGS: usize = 8;

/// How many words entering `func` takes, beside the runtime handle and `sret`.
///
/// One per parameter, then one descriptor per parameter whose own type does not
/// describe what arrives at it, then one per shape the callee declared. This is
/// `build_signature`'s layout, counted.
fn entry_width(func: &IrCodeUnit) -> usize {
    match func.function_context() {
        Some(ctx) => {
            ctx.param_types.len() + ctx.descriptor_params.len() + ctx.descriptor_shapes.len()
        }
        None => 0,
    }
}

/// Whether `func` is narrow enough to be entered from outside compiled code.
///
/// `dispatch_void` transmutes the entry point to one of a fixed set of `extern
/// "C"` function pointer types, one per arity, so a wider function has no type
/// to be called through. Asked before compiling rather than at the call, because
/// a function that cannot be entered is one there is no point compiling, and
/// finding out at the call meant a program that ran under the interpreter failed
/// under the jit.
pub fn enterable(func: &IrCodeUnit) -> bool {
    entry_width(func) <= MAX_DIRECT_ARGS
}

/// Call a JIT-compiled function from the interpreter.
///
/// JIT functions use this calling convention:
/// - First param: rt_handle (pointer to runtime)
/// - Second param (non-Unit returns): sret pointer for return value
/// - Next params: pointers to argument values
/// - Then one descriptor per `descriptor_params` entry, then one per
///   `descriptor_shapes` entry
///
/// The two descriptor groups and their order are `build_signature`'s, in the
/// cranelift codegen the jit shares with the AOT backend. Leaving the second
/// group off does not merely lose a descriptor, it hands the callee a shorter
/// argument list than it was compiled for.
///
/// All returns are written via sret pointer. No register returns.
pub unsafe fn call_jit(
    code_ptr: *const u8,
    uses_sret: bool,
    rt_handle: LocalRtHandle,
    args: &[Value],
    ret_dest: Destination,
    _return_type: &IrType,
    descriptor_params: &[ParamId],
    shape_descriptors: &[*const rtdt::TyDesc],
) {
    assert!(
        args.len() + descriptor_params.len() + shape_descriptors.len() <= MAX_DIRECT_ARGS,
        "entering a function of {} words, which `enterable` should have refused before \
         it was compiled",
        args.len() + descriptor_params.len() + shape_descriptors.len(),
    );

    // Build argument array: [rt_handle, sret?, arg_ptrs...]
    let mut raw_args: [usize; MAX_DIRECT_ARGS + 2] = [0; MAX_DIRECT_ARGS + 2];
    let mut arg_idx = 0;

    // rt_handle is always first.
    raw_args[arg_idx] = rt_handle as usize;
    arg_idx += 1;

    // sret pointer for non-Unit returns.
    if uses_sret {
        raw_args[arg_idx] = ret_dest.ptr as usize;
        arg_idx += 1;
    }

    // User arguments (all passed by pointer).
    for arg in args {
        raw_args[arg_idx] = arg.ptr as usize;
        arg_idx += 1;
    }

    // Then the descriptors a generic callee cannot work out for itself. Here
    // they are simply the ones the argument values carry.
    for param_id in descriptor_params {
        let arg = args.get(param_id.0 as usize).unwrap_or_else(|| panic!(
            "callee wants a descriptor for parameter {} but got {} arguments; the \
             signature and the call site disagree",
            param_id.0,
            args.len(),
        ));
        raw_args[arg_idx] = arg.tydesc as usize;
        arg_idx += 1;
    }

    // Then one for each shape the callee builds a collection of. No argument
    // carries these, so they arrive already worked out: from the call site when
    // the interpreter is calling, or forwarded out of the caller's own trailing
    // arguments when jit code is.
    for tydesc in shape_descriptors {
        raw_args[arg_idx] = *tydesc as usize;
        arg_idx += 1;
    }

    // All functions return void (results written via sret pointer).
    let total_args = arg_idx;
    unsafe { dispatch_void(code_ptr, &raw_args, total_args) }
}

/// Dispatch a JIT call. All functions return void (sret convention).
unsafe fn dispatch_void(
    code_ptr: *const u8,
    args: &[usize; MAX_DIRECT_ARGS + 2],
    arg_count: usize,
) {
    type Fn1 = unsafe extern "C" fn(usize);
    type Fn2 = unsafe extern "C" fn(usize, usize);
    type Fn3 = unsafe extern "C" fn(usize, usize, usize);
    type Fn4 = unsafe extern "C" fn(usize, usize, usize, usize);
    type Fn5 = unsafe extern "C" fn(usize, usize, usize, usize, usize);
    type Fn6 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize);
    type Fn7 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize);
    type Fn8 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize);
    type Fn9 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize);
    type Fn10 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize);

    unsafe {
        match arg_count {
            1 => { let f: Fn1 = std::mem::transmute(code_ptr); f(args[0]); }
            2 => { let f: Fn2 = std::mem::transmute(code_ptr); f(args[0], args[1]); }
            3 => { let f: Fn3 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2]); }
            4 => { let f: Fn4 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3]); }
            5 => { let f: Fn5 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3], args[4]); }
            6 => { let f: Fn6 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3], args[4], args[5]); }
            7 => { let f: Fn7 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3], args[4], args[5], args[6]); }
            8 => { let f: Fn8 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7]); }
            9 => { let f: Fn9 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8]); }
            10 => { let f: Fn10 = std::mem::transmute(code_ptr); f(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8], args[9]); }
            // `enterable` caps the width at `MAX_DIRECT_ARGS`, and the runtime
            // handle and `sret` pointer are the only two on top of it.
            _ => panic!("entering a function of {} words, which has no arm here", arg_count),
        }
    };
}
