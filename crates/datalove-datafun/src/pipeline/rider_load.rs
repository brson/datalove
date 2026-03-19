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
    /// Raw native function pointers extracted from the library.
    ///
    /// Used by the JIT to register symbols for direct native calls.
    pub native_fn_ptrs: Vec<(String, *const u8)>,
    _lib: libloading::Library,
}

/// Load a rider shared library and register its symbols in the native function table.
///
/// For each symbol, looks up the raw function pointer via `dlsym` and wraps it in
/// a closure that bridges from the interpreter's `(LocalRtHandle, &[Value], Destination)`
/// calling convention to the rider C ABI:
/// `fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> RtStatus`
pub fn load_rider_library(
    lib_path: &Path,
    rider_name: &str,
    symbols: &[String],
    native_table: &mut NativeFunctionTable,
) -> AnyResult<LoadedRider> {
    let lib = unsafe { libloading::Library::new(lib_path) }
        .context(fmt!("failed to load rider library '{}' from {}", rider_name, lib_path.display()))?;

    let mut native_fn_ptrs = Vec::new();

    for symbol in symbols {
        // Look up the raw function pointer.
        let fn_ptr: *const () = unsafe {
            let sym: libloading::Symbol<*const ()> = lib.get(symbol.as_bytes())
                .context(fmt!("symbol '{}' not found in rider '{}'", symbol, rider_name))?;
            *sym
        };

        // Save raw pointer for JIT registration.
        native_fn_ptrs.push((symbol.clone(), fn_ptr as *const u8));

        let symbol_name = symbol.clone();
        let bridge: Box<dyn Fn(LocalRtHandle, &[Value], Destination) -> Result<(), InterpError>> =
            Box::new(move |rt, args, dest| {
                call_native_bridge(fn_ptr, rt, args, dest, &symbol_name)
            });

        native_table.register(symbol.clone(), bridge);
    }

    Ok(LoadedRider {
        rider_name: rider_name.to_string(),
        native_fn_ptrs,
        _lib: lib,
    })
}

/// Bridge from interpreter values to rider C ABI.
///
/// Builds a flat array of pointer-sized args: `[rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc]`
/// and calls the native function via `libffi`-style dispatch.
///
/// The C function signature is:
/// `extern "C-unwind" fn(rt, ptr, tydesc, ptr, tydesc, ..., out_ptr, out_tydesc) -> u8`
fn call_native_bridge(
    fn_ptr: *const (),
    rt: LocalRtHandle,
    args: &[Value],
    dest: Destination,
    _symbol: &str,
) -> Result<(), InterpError> {
    // Build flat arg array: [rt, arg0_ptr, arg0_tydesc, ..., dest_ptr, dest_tydesc]
    let mut c_args: Vec<usize> = Vec::with_capacity(1 + args.len() * 2 + 2);
    c_args.push(rt as usize);
    for arg in args {
        c_args.push(arg.ptr as usize);
        c_args.push(arg.tydesc as usize);
    }
    c_args.push(dest.ptr as usize);
    c_args.push(dest.tydesc as usize);

    // Call the C function. Dispatch based on total C arg count.
    let _status: u8 = unsafe {
        type Ptr = usize;
        match c_args.len() {
            // 0 args + result = rt, out, out_td
            3 => {
                let f: extern "C-unwind" fn(Ptr, Ptr, Ptr) -> u8 =
                    std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2])
            }
            // 1 arg + result = rt, p, td, out, out_td
            5 => {
                let f: extern "C-unwind" fn(Ptr, Ptr, Ptr, Ptr, Ptr) -> u8 =
                    std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4])
            }
            // 2 args + result = rt, p, td, p, td, out, out_td
            7 => {
                let f: extern "C-unwind" fn(Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr) -> u8 =
                    std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4], c_args[5], c_args[6])
            }
            // 3 args + result
            9 => {
                let f: extern "C-unwind" fn(Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr) -> u8 =
                    std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4], c_args[5], c_args[6], c_args[7], c_args[8])
            }
            // 4 args + result
            11 => {
                let f: extern "C-unwind" fn(Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr, Ptr) -> u8 =
                    std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4], c_args[5], c_args[6], c_args[7], c_args[8], c_args[9], c_args[10])
            }
            n => {
                todo!("native functions with {} C args not yet supported", n);
            }
        }
    };

    Ok(())
}
