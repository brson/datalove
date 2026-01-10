//! Calling convention bridge between interpreter and JIT code.
//!
//! Converts between interpreter's Value/Destination and JIT's raw pointer convention.

use datalove_datafun_interp::{Destination, Value};
use datalove_rt::c::LocalRtHandle;

use crate::JitError;

/// Maximum number of function parameters supported by direct dispatch.
const MAX_DIRECT_ARGS: usize = 8;

/// Call a JIT-compiled function from the interpreter.
///
/// JIT functions use this calling convention:
/// - First param: rt_handle (pointer to runtime)
/// - If uses_sret: second param is return value pointer
/// - Remaining params: pointers to argument values
///
/// Returns via register (scalar) or sret pointer (aggregate).
pub unsafe fn call_jit(
    code_ptr: *const u8,
    uses_sret: bool,
    rt_handle: LocalRtHandle,
    args: &[Value],
    ret_dest: Destination,
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

    // sret pointer if aggregate return.
    if uses_sret {
        raw_args[arg_idx] = ret_dest.ptr as usize;
        arg_idx += 1;
    }

    // User arguments.
    for arg in args {
        raw_args[arg_idx] = arg.ptr as usize;
        arg_idx += 1;
    }

    // Dispatch based on number of arguments.
    // This avoids using libffi by handling common cases directly.
    let total_args = arg_idx;
    // SAFETY: code_ptr is a valid JIT-compiled function, args are valid pointers.
    unsafe { dispatch_call(code_ptr, &raw_args, total_args, uses_sret, ret_dest)? };

    Ok(())
}

/// Dispatch a JIT call with the given arguments.
///
/// # Safety
///
/// code_ptr must be a valid JIT-compiled function pointer with the expected signature.
/// args must contain valid pointers for the function's parameters.
unsafe fn dispatch_call(
    code_ptr: *const u8,
    args: &[usize; MAX_DIRECT_ARGS + 2],
    arg_count: usize,
    uses_sret: bool,
    ret_dest: Destination,
) -> Result<(), JitError> {
    // Type aliases for function pointers with different arities.
    type Fn1 = unsafe extern "C" fn(usize) -> usize;
    type Fn2 = unsafe extern "C" fn(usize, usize) -> usize;
    type Fn3 = unsafe extern "C" fn(usize, usize, usize) -> usize;
    type Fn4 = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;
    type Fn5 = unsafe extern "C" fn(usize, usize, usize, usize, usize) -> usize;
    type Fn6 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize) -> usize;
    type Fn7 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize) -> usize;
    type Fn8 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize) -> usize;
    type Fn9 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;
    type Fn10 = unsafe extern "C" fn(usize, usize, usize, usize, usize, usize, usize, usize, usize, usize) -> usize;

    // SAFETY: caller guarantees code_ptr is valid JIT code and args are valid.
    let result = unsafe {
        match arg_count {
            1 => {
                let f: Fn1 = std::mem::transmute(code_ptr);
                f(args[0])
            }
            2 => {
                let f: Fn2 = std::mem::transmute(code_ptr);
                f(args[0], args[1])
            }
            3 => {
                let f: Fn3 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2])
            }
            4 => {
                let f: Fn4 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3])
            }
            5 => {
                let f: Fn5 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3], args[4])
            }
            6 => {
                let f: Fn6 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            7 => {
                let f: Fn7 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3], args[4], args[5], args[6])
            }
            8 => {
                let f: Fn8 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7])
            }
            9 => {
                let f: Fn9 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8])
            }
            10 => {
                let f: Fn10 = std::mem::transmute(code_ptr);
                f(args[0], args[1], args[2], args[3], args[4], args[5], args[6], args[7], args[8], args[9])
            }
            _ => {
                return Err(JitError::BridgeCallFailed(format!(
                    "unsupported argument count: {}",
                    arg_count
                )));
            }
        }
    };

    // If not sret, write scalar result to destination.
    // The result is in a register (usize-sized). Caller must provide
    // a properly aligned buffer (at least usize alignment).
    if !uses_sret {
        // SAFETY: ret_dest.ptr must be usize-aligned for scalar returns.
        unsafe { *(ret_dest.ptr as *mut usize) = result };
    }
    // If sret, result was written directly to ret_dest.ptr by the callee.

    Ok(())
}
