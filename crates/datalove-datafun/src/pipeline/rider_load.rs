//! Dynamic loading of built rider shared libraries.
//!
//! Loads `.so`/`.dylib` files produced by rider crate builds, looks up native
//! function symbols, and registers them in the interpreter's `NativeFunctionTable`.

use rmx::prelude::*;
use rmx::std::path::Path;

use datalove_datafun_interp::{NativeFunctionTable, Value, Destination, InterpError};
use datalove_rt::c::LocalRtHandle;
use datalove_rtdt as rtdt;

use super::{CompiledModules, ScriptExecutor, WorkspaceDescriptor, rider_build};

/// Build a workspace's native component and register its symbols with an executor.
///
/// The returned riders own the loaded shared library and must outlive the
/// executor they were registered with.
pub fn build_and_load_riders(
    descriptor: &WorkspaceDescriptor,
    compiled: &CompiledModules,
    executor: &mut ScriptExecutor,
) -> AnyResult<Vec<LoadedRider>> {
    let rider_crates = descriptor.rider_crates();
    if rider_crates.is_empty() {
        return Ok(Vec::new());
    }

    let work_dir = descriptor.work_dir.as_ref()
        .ok_or_else(|| anyhow!("workspace has riders but no work dir"))?;
    let dylib = rider_build::build_rider_dylib(work_dir, &rider_crates)
        .map_err(|e| anyhow!("{}", e))?;

    let native_symbols = compiled.native_symbols();
    if native_symbols.is_empty() {
        return Ok(Vec::new());
    }

    let loaded = load_rider_library(
        &dylib,
        "native-component",
        &native_symbols,
        executor.native_table_mut(),
    )?;
    Ok(vec![loaded])
}

/// Whether to drive `sys` riders the way every other rider goes.
///
/// A `sys` rider is linked into this binary, so its addresses are already
/// here and the interpreter takes them directly --- no cargo, no component,
/// no dlopen. That is worth having, and it means the general path goes
/// untested for exactly the riders the test suite leans on hardest.
///
/// Setting `DATALOVE_BUILD_SYS_RIDERS` gives up the shortcut: the riders are
/// discovered, compiled into a component and loaded like a package's own.
/// Same results expected, different arrangement underneath, which is the
/// point of running the suite with it set.
pub fn build_sys_riders() -> bool {
    rmx::std::env::var_os("DATALOVE_BUILD_SYS_RIDERS").is_some()
}

/// The natives an executor was pointed at, and what has to stay alive for it.
pub struct RegisteredNatives {
    /// Raw addresses, which the JIT needs: it calls natives through a
    /// trampoline built from the address rather than through the
    /// interpreter's table.
    pub native_fn_ptrs: Vec<(String, *const u8)>,

    /// Libraries that must outlive the executor, dropping one unmapping the
    /// code every registered pointer points into. Empty when the riders were
    /// linked in, there being nothing loaded to keep.
    pub loaded: Vec<LoadedRider>,
}

/// Point an executor at the native functions the compiled modules call.
///
/// Both ways in, chosen by [`build_sys_riders`]. A driver that skips this and
/// registers nothing does not fail until a script reaches a native call, and
/// under the JIT that is a panic inside compilation rather than an error.
pub fn register_natives(
    descriptor: &WorkspaceDescriptor,
    compiled: &CompiledModules,
    linked: &[(String, *const ())],
    executor: &mut ScriptExecutor,
) -> AnyResult<RegisteredNatives> {
    let symbols = compiled.native_symbols();

    if !build_sys_riders() {
        let native_fn_ptrs = register_linked_natives(
            &symbols, linked, executor.native_table_mut())?;
        return Ok(RegisteredNatives { native_fn_ptrs, loaded: Vec::new() });
    }

    let loaded = build_and_load_riders(descriptor, compiled, executor)?;
    let native_fn_ptrs = loaded.iter()
        .flat_map(|rider| rider.native_fn_ptrs.iter().cloned())
        .collect();
    Ok(RegisteredNatives { native_fn_ptrs, loaded })
}

/// Refuse a library built against a different runtime interface.
///
/// A rider calls the runtime through a table it reaches at run time, so its
/// library resolves nothing when it loads and no link step can catch the two
/// sides disagreeing. Left unchecked, a mismatch is a call with the wrong
/// arguments or a field read at the wrong offset, which is to say nothing
/// reports it. So the library carries the interface it was built against and
/// it is compared here, before any of its functions are looked up.
///
/// See [`datalove_rti::ABI_VERSION`] for what the number covers.
fn check_abi_version(
    lib: &libloading::Library,
    rider_name: &str,
    lib_path: &Path,
) -> AnyResult<()> {
    let theirs: libloading::Symbol<*const u64> = unsafe { lib.get(b"DLR_ABI_VERSION") }
        .context(fmt!(
            "rider library '{}' at {} does not say which runtime interface it \
             was built against; it is either not a rider library or was built \
             by a datalove too old to say",
            rider_name, lib_path.display(),
        ))?;

    let theirs = unsafe { **theirs };
    if theirs != datalove_rti::ABI_VERSION {
        bail!(
            "rider library '{}' at {} was built against a different runtime \
             interface than this datalove: library {:#018x}, datalove {:#018x}",
            rider_name, lib_path.display(), theirs, datalove_rti::ABI_VERSION,
        );
    }

    Ok(())
}

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

    check_abi_version(&lib, rider_name, lib_path)?;

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

        register_native(symbol, fn_ptr, native_table);
    }

    Ok(LoadedRider {
        rider_name: rider_name.to_string(),
        native_fn_ptrs,
        _lib: lib,
    })
}

/// Register rider functions that are linked into this binary.
///
/// `natives` is the whole table the binary carries, as
/// [`SystemLibrary::natives`](super::SystemLibrary::natives); `symbols` names
/// the subset the compiled modules actually call. Returns the raw pointers,
/// which the JIT needs to call natives directly.
pub fn register_linked_natives(
    symbols: &[String],
    natives: &[(String, *const ())],
    native_table: &mut NativeFunctionTable,
) -> AnyResult<Vec<(String, *const u8)>> {
    let mut native_fn_ptrs = Vec::new();

    for symbol in symbols {
        let (_, fn_ptr) = natives.iter().find(|(name, _)| name == symbol)
            .ok_or_else(|| anyhow!("native function '{}' is not linked into this binary", symbol))?;

        native_fn_ptrs.push((symbol.clone(), *fn_ptr as *const u8));

        register_native(symbol, *fn_ptr, native_table);
    }

    Ok(native_fn_ptrs)
}

/// Register one native function under the symbol the compiler calls it by.
fn register_native(
    symbol: &str,
    fn_ptr: *const (),
    native_table: &mut NativeFunctionTable,
) {
    let symbol_name = symbol.to_string();
    let bridge: datalove_datafun_interp::NativeFnImpl =
        Box::new(move |rt, args, dest, supplied| {
            call_native_bridge(fn_ptr, rt, args, dest, supplied, &symbol_name)
        });

    native_table.register(symbol.to_string(), bridge);
}

/// Bridge from interpreter values to the rider C ABI.
///
/// See `botdocs/native-abi.md` for the shape this builds. In short:
///
/// ```text
/// fn(rt, arg0_ptr, arg0_td, ..., out_ptr, out_td, shape_td...) -> u8
/// ```
///
/// Every word is pointer-sized, so the call is made by transmuting to a
/// function type of the right arity. The arities are listed rather than
/// generated because there is no variadic form that would keep the C ABI.
fn call_native_bridge(
    fn_ptr: *const (),
    rt: LocalRtHandle,
    args: &[Value],
    dest: Destination,
    supplied: &[*const rtdt::TyDesc],
    _symbol: &str,
) -> Result<(), InterpError> {
    let mut c_args: Vec<usize> = Vec::with_capacity(1 + args.len() * 2 + 2 + supplied.len());
    c_args.push(rt as usize);
    for arg in args {
        c_args.push(arg.ptr as usize);
        c_args.push(arg.tydesc as usize);
    }
    c_args.push(dest.ptr as usize);
    c_args.push(dest.tydesc as usize);
    for tydesc in supplied {
        c_args.push(*tydesc as usize);
    }

    /// Transmute to a function of `n` pointer arguments and call it.
    macro_rules! arity {
        ($($n:literal => $($i:literal),+ ;)+) => {
            match c_args.len() {
                $($n => {
                    let f: extern "C-unwind" fn($(arity!(@ptr $i)),+) -> u8 =
                        std::mem::transmute(fn_ptr);
                    f($(c_args[$i]),+)
                })+
                n => todo!("native functions with {} C args not yet supported", n),
            }
        };
        (@ptr $i:literal) => { usize };
    }

    let _status: u8 = unsafe {
        arity! {
            3  => 0, 1, 2;
            4  => 0, 1, 2, 3;
            5  => 0, 1, 2, 3, 4;
            6  => 0, 1, 2, 3, 4, 5;
            7  => 0, 1, 2, 3, 4, 5, 6;
            8  => 0, 1, 2, 3, 4, 5, 6, 7;
            9  => 0, 1, 2, 3, 4, 5, 6, 7, 8;
            10 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9;
            11 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10;
            12 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11;
            13 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12;
        }
    };

    Ok(())
}
