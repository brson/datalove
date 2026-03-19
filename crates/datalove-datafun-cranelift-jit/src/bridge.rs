//! Calling convention bridge between interpreter and JIT code.
//!
//! Converts between interpreter's Value/Destination and JIT's raw pointer convention.
//! All function returns use sret (pointer-based), so the bridge only needs void dispatch.

use datalove_datafun_interp::{Destination, Value};
use datalove_datafun_ir::IrType;
use datalove_rt::c::LocalRtHandle;

use crate::JitError;

/// Maximum number of function parameters supported by direct dispatch.
const MAX_DIRECT_ARGS: usize = 8;

/// Call a JIT-compiled function from the interpreter.
///
/// JIT functions use this calling convention:
/// - First param: rt_handle (pointer to runtime)
/// - Second param (non-Unit returns): sret pointer for return value
/// - Remaining params: pointers to argument values
///
/// All returns are written via sret pointer. No register returns.
pub unsafe fn call_jit(
    code_ptr: *const u8,
    uses_sret: bool,
    rt_handle: LocalRtHandle,
    args: &[Value],
    ret_dest: Destination,
    _return_type: &IrType,
) -> Result<(), JitError> {
    if args.len() > MAX_DIRECT_ARGS {
        return Err(JitError::BridgeCallFailed(format!(
            "too many arguments: {} (max {})",
            args.len(),
            MAX_DIRECT_ARGS
        )));
    }

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

    // All functions return void (results written via sret pointer).
    let total_args = arg_idx;
    unsafe { dispatch_void(code_ptr, &raw_args, total_args) }
}

/// Dispatch a JIT call. All functions return void (sret convention).
unsafe fn dispatch_void(
    code_ptr: *const u8,
    args: &[usize; MAX_DIRECT_ARGS + 2],
    arg_count: usize,
) -> Result<(), JitError> {
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
            _ => return Err(JitError::BridgeCallFailed(format!("unsupported argument count: {}", arg_count))),
        }
    };
    Ok(())
}
