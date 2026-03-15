//! Dynamic loading of built rider shared libraries.
//!
//! Loads `.so`/`.dylib` files produced by rider crate builds, looks up native
//! function symbols, and registers them in the interpreter's `NativeFunctionTable`.

use rmx::prelude::*;
use rmx::std::path::Path;

use datalove_datafun_interp::{NativeFunctionTable, Value, Destination, InterpError};
use datalove_rt::c::LocalRtHandle;

/// A loaded rider shared library.
///
/// Holds the `libloading::Library` handle to keep the loaded code alive.
/// Must outlive any calls through the registered native functions.
pub struct LoadedRider {
    pub rider_name: String,
    _lib: libloading::Library,
}

/// Load a rider shared library and register its symbols in the native function table.
///
/// For each symbol, looks up the raw function pointer via `dlsym` and wraps it in
/// a closure that bridges from the interpreter's `(LocalRtHandle, &[Value], Destination)`
/// calling convention to a C ABI function `fn(LocalRtHandle, i64, i64, ...) -> i64`.
///
/// Currently supports scalar-only functions (i32, i64, bool, etc.) where values
/// are stored as raw integers in the interpreter's value slots.
pub fn load_rider_library(
    lib_path: &Path,
    rider_name: &str,
    symbols: &[String],
    native_table: &mut NativeFunctionTable,
) -> AnyResult<LoadedRider> {
    let lib = unsafe { libloading::Library::new(lib_path) }
        .context(fmt!("failed to load rider library '{}' from {}", rider_name, lib_path.display()))?;

    for symbol in symbols {
        // Look up the raw function pointer.
        let fn_ptr: *const () = unsafe {
            let sym: libloading::Symbol<*const ()> = lib.get(symbol.as_bytes())
                .context(fmt!("symbol '{}' not found in rider '{}'", symbol, rider_name))?;
            *sym
        };

        // Create a bridge closure that calls the raw pointer.
        // The closure captures the raw pointer (which remains valid as long as
        // LoadedRider keeps the library alive).
        let symbol_name = symbol.clone();
        let bridge: Box<dyn Fn(LocalRtHandle, &[Value], Destination) -> Result<(), InterpError>> =
            Box::new(move |rt, args, dest| {
                call_native_scalar(fn_ptr, rt, args, dest, &symbol_name)
            });

        native_table.register(symbol.clone(), bridge);
    }

    Ok(LoadedRider {
        rider_name: rider_name.to_string(),
        _lib: lib,
    })
}

/// Bridge from interpreter values to a C ABI scalar function call.
///
/// Reads each argument as a raw `i64` from the value's pointer, calls the
/// C function with those i64 args + the runtime handle, and writes the i64
/// result to the destination. This works for integer types (i32, i64, etc.)
/// where the interpreter stores values inline in appropriately-sized slots.
///
/// The C function signature is:
/// `extern "C-unwind" fn(LocalRtHandle, arg0: i64, arg1: i64, ...) -> i64`
fn call_native_scalar(
    fn_ptr: *const (),
    rt: LocalRtHandle,
    args: &[Value],
    dest: Destination,
    _symbol: &str,
) -> Result<(), InterpError> {
    // Read argument values as i64.
    let mut arg_vals: Vec<i64> = Vec::with_capacity(args.len());
    for arg in args {
        let val = unsafe {
            // Read up to 8 bytes from the value pointer.
            let size = (*arg.tydesc).size as usize;
            let mut buf = [0i64; 1];
            std::ptr::copy_nonoverlapping(
                arg.ptr,
                &mut buf as *mut _ as *mut u8,
                size.min(8),
            );
            buf[0]
        };
        arg_vals.push(val);
    }

    // Call the C function. We dispatch based on argument count.
    // Each arm casts fn_ptr to the appropriate C function signature.
    let result: i64 = unsafe {
        match arg_vals.len() {
            0 => {
                let f: extern "C-unwind" fn(LocalRtHandle) -> i64 =
                    std::mem::transmute(fn_ptr);
                f(rt)
            }
            1 => {
                let f: extern "C-unwind" fn(LocalRtHandle, i64) -> i64 =
                    std::mem::transmute(fn_ptr);
                f(rt, arg_vals[0])
            }
            2 => {
                let f: extern "C-unwind" fn(LocalRtHandle, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr);
                f(rt, arg_vals[0], arg_vals[1])
            }
            3 => {
                let f: extern "C-unwind" fn(LocalRtHandle, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr);
                f(rt, arg_vals[0], arg_vals[1], arg_vals[2])
            }
            4 => {
                let f: extern "C-unwind" fn(LocalRtHandle, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(fn_ptr);
                f(rt, arg_vals[0], arg_vals[1], arg_vals[2], arg_vals[3])
            }
            n => {
                todo!("native functions with {} args not yet supported", n);
            }
        }
    };

    // Write the result to the destination.
    unsafe {
        let size = (*dest.tydesc).size as usize;
        std::ptr::copy_nonoverlapping(
            &result as *const _ as *const u8,
            dest.ptr,
            size.min(8),
        );
    }

    Ok(())
}
