//! Dynamic loading of built rider shared libraries.
//!
//! Loads `.so`/`.dylib` files produced by rider crate builds, looks up native
//! function symbols, and registers them in the interpreter's `NativeFunctionTable`.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

use datalove_datafun_interp::{NativeFunctionTable, Value, Destination, InterpError};
use datalove_rt::c::LocalRtHandle;
use datalove_rtdt as rtdt;

use super::{CompiledModules, RiderCrate, ScriptExecutor, WorkspaceDescriptor, rider_build};

/// Where the natives a workspace's modules call are found.
///
/// One per workspace, shared by everything that calls them: the evaluator
/// running consts while the modules compile, and the executor running the
/// program after. That is what makes the riders build once. Nor do they build
/// before something needs them, so a compile whose consts call no native runs
/// no cargo, and one that does gets the library its program runs against.
///
/// Built riders are the general case: [`RiderNatives::built`]. The riders
/// `sys` carries are linked into this binary, and [`RiderNatives::linked`]
/// takes those addresses rather than building anything; see
/// [`build_sys_riders`] for choosing between the two. A workspace with riders
/// of its own besides has both: the linked ones, and its own built.
pub struct RiderNatives {
    /// Linked into this binary, looked in first.
    linked: Option<LinkedNatives>,
    /// Compiled from rider crates, for every symbol the linked ones lack.
    built: Option<BuiltNatives>,
}

/// Rider crates, compiled into one library on first use.
struct BuiltNatives {
    work_dir: Option<PathBuf>,
    riders: Vec<RiderCrate>,
    /// The error kept as text, since every later caller is told it too.
    library: OnceLock<Result<Arc<LoadedLibrary>, String>>,
}

impl BuiltNatives {
    fn new(work_dir: Option<PathBuf>, riders: Vec<RiderCrate>) -> Self {
        BuiltNatives { work_dir, riders, library: OnceLock::new() }
    }

    fn address(&self, symbol: &str) -> AnyResult<*const ()> {
        let library = self.library
            .get_or_init(|| build_and_load(self.work_dir.as_deref(), &self.riders).map_err(|e| fmt!("{:#}", e)))
            .as_ref()
            .map_err(|e| anyhow!("{}", e))?;
        unsafe { library.symbol(symbol) }
    }
}

/// The addresses of natives linked into this binary, by symbol.
struct LinkedNatives(Vec<(String, *const ())>);

// Addresses of code in this binary, which any thread may call and which
// nothing ever unmaps.
unsafe impl Send for LinkedNatives {}
unsafe impl Sync for LinkedNatives {}

impl RiderNatives {
    /// Build `descriptor`'s riders when one of their natives is first wanted.
    pub fn built(descriptor: &WorkspaceDescriptor) -> Self {
        Self {
            linked: None,
            built: Some(BuiltNatives::new(descriptor.work_dir.clone(), descriptor.rider_crates())),
        }
    }

    /// Take the natives linked into this binary, as
    /// [`SystemLibrary::natives`](super::SystemLibrary::natives) lists them.
    pub fn linked(natives: &[(String, *const ())]) -> Self {
        Self { linked: Some(LinkedNatives(natives.to_vec())), built: None }
    }

    /// The natives for `descriptor`, for a driver carrying the `linked` ones.
    ///
    /// The system library's are linked unless [`build_sys_riders`] says to
    /// build them. The riders of the workspace's own libraries are built, there
    /// being nothing of theirs in this binary.
    pub fn for_workspace(descriptor: &WorkspaceDescriptor, linked: &[(String, *const ())]) -> Self {
        if build_sys_riders() {
            return Self::built(descriptor);
        }
        let own = descriptor.user_rider_crates();
        Self {
            linked: Some(LinkedNatives(linked.to_vec())),
            built: (!own.is_empty()).then(|| BuiltNatives::new(descriptor.work_dir.clone(), own)),
        }
    }

    /// The address of the native the compiler calls `symbol`.
    ///
    /// The first call for a built rider's native builds and loads them all.
    pub fn address(&self, symbol: &str) -> AnyResult<*const ()> {
        if let Some(LinkedNatives(natives)) = &self.linked {
            if let Some((_, fn_ptr)) = natives.iter().find(|(name, _)| name == symbol) {
                return Ok(*fn_ptr);
            }
        }
        match &self.built {
            Some(built) => built.address(symbol),
            None => bail!("native function '{}' is not linked into this binary", symbol),
        }
    }
}

impl datalove_datafun_interp::NativeResolver for RiderNatives {
    fn resolve(&self, symbol: &str) -> Result<datalove_datafun_interp::NativeFnImpl, String> {
        let fn_ptr = self.address(symbol).map_err(|e| fmt!("{:#}", e))?;
        Ok(bridge(symbol, fn_ptr))
    }
}

/// Build riders into one library and load it.
fn build_and_load(work_dir: Option<&Path>, riders: &[RiderCrate]) -> AnyResult<Arc<LoadedLibrary>> {
    if riders.is_empty() {
        bail!("a native was called, and the workspace has no riders to build");
    }
    let work_dir = work_dir
        .ok_or_else(|| anyhow!("workspace has riders but no work dir"))?;
    let dylib = rider_build::build_rider_dylib(work_dir, riders)
        .map_err(|e| anyhow!("{}", e))?;
    open_library(&dylib, "native-component")
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

    /// Where the addresses came from, which holds any library they lead into.
    pub natives: Arc<RiderNatives>,
}

impl RegisteredNatives {
    /// A share of whatever these natives' code lives in.
    ///
    /// For whatever else took the raw addresses and has to outlive them --- the
    /// JIT, which emits them into compiled code. The interpreter's table was
    /// given its share when the symbols were registered.
    pub fn code_owners(&self) -> Vec<Arc<dyn Any + Send + Sync>> {
        vec![self.natives.clone()]
    }
}

/// Point an executor at the native functions the compiled modules call.
///
/// Takes the same `natives` the modules were compiled with, so riders a const
/// already needed are not built or loaded a second time. A driver that skips
/// this and registers nothing does not fail until a script reaches a native
/// call, and under the JIT that is a panic inside compilation rather than an
/// error.
pub fn register_natives(
    natives: &Arc<RiderNatives>,
    compiled: &CompiledModules,
    executor: &mut ScriptExecutor,
) -> AnyResult<RegisteredNatives> {
    let table = executor.native_table_mut();
    let mut native_fn_ptrs = Vec::new();

    for symbol in compiled.native_symbols() {
        let fn_ptr = natives.address(&symbol)?;
        native_fn_ptrs.push((symbol.clone(), fn_ptr as *const u8));
        table.register(symbol.clone(), bridge(&symbol, fn_ptr));
    }

    // The table now holds pointers into whatever the natives loaded.
    table.hold_code_owner(natives.clone());

    Ok(RegisteredNatives { native_fn_ptrs, natives: natives.clone() })
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

/// A rider library mapped into this process.
///
/// Dropping the last of these closes the library, which unmaps the code every
/// pointer taken out of it leads to. Nothing else closes one: there is no
/// unload to call, because what decides a library is finished with is that
/// nobody holds it, and that is this count.
pub struct LoadedLibrary {
    path: PathBuf,
    lib: libloading::Library,
}

impl LoadedLibrary {
    /// Look up a symbol's address.
    ///
    /// # Safety
    ///
    /// The caller is trusting the library to define `symbol` with the type it
    /// goes on to use it as.
    unsafe fn symbol(&self, symbol: &str) -> AnyResult<*const ()> {
        let sym: libloading::Symbol<*const ()> = unsafe { self.lib.get(symbol.as_bytes()) }
            .context(fmt!("symbol '{}' not found in {}", symbol, self.path.display()))?;
        Ok(*sym)
    }
}

impl std::fmt::Debug for LoadedLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedLibrary").field("path", &self.path).finish()
    }
}

/// The rider libraries this process has open, by the path each was opened from.
///
/// Weak, so that the registry is a way of finding a library rather than a
/// reason for one to stay open. An entry whose library has gone is replaced by
/// the next load of that path.
///
/// Held because `dlopen` of a path already open returns the same mapping
/// whatever we do, so two interpreters sharing a rider share its code either
/// way. What this adds is a count we can see: the library closes when the last
/// interpreter holding it is dropped, and not at whatever point a libc decides,
/// and the symbols are looked up once per library rather than once per
/// interpreter.
static OPEN_LIBRARIES: LazyLock<Mutex<HashMap<PathBuf, Weak<LoadedLibrary>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Open a rider library, or take a share of one already open.
fn open_library(lib_path: &Path, rider_name: &str) -> AnyResult<Arc<LoadedLibrary>> {
    let mut open = OPEN_LIBRARIES.lock().expect("rider library registry poisoned");

    if let Some(existing) = open.get(lib_path).and_then(Weak::upgrade) {
        return Ok(existing);
    }

    let lib = unsafe { libloading::Library::new(lib_path) }
        .context(fmt!("failed to load rider library '{}' from {}", rider_name, lib_path.display()))?;

    check_abi_version(&lib, rider_name, lib_path)?;

    let loaded = Arc::new(LoadedLibrary { path: lib_path.to_path_buf(), lib });
    open.insert(lib_path.to_path_buf(), Arc::downgrade(&loaded));
    Ok(loaded)
}

/// How many rider libraries this process has open.
///
/// For tests, which is the only place the count is interesting; a caller that
/// wanted to act on it would be asking about a library it does not hold.
pub fn open_library_count() -> usize {
    let open = OPEN_LIBRARIES.lock().expect("rider library registry poisoned");
    open.values().filter(|weak| weak.strong_count() > 0).count()
}

/// A loaded rider shared library.
///
/// Holds a share of the mapped library to keep the loaded code alive. Must
/// outlive any calls through the registered native functions.
pub struct LoadedRider {
    pub rider_name: String,
    /// Raw native function pointers extracted from the library.
    ///
    /// Used by the JIT to register symbols for direct native calls.
    pub native_fn_ptrs: Vec<(String, *const u8)>,
    /// The library those pointers lead into.
    pub library: Arc<LoadedLibrary>,
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
    let library = open_library(lib_path, rider_name)?;

    let mut native_fn_ptrs = Vec::new();

    for symbol in symbols {
        let fn_ptr = unsafe { library.symbol(symbol) }
            .context(fmt!("rider '{}'", rider_name))?;

        // Save raw pointer for JIT registration.
        native_fn_ptrs.push((symbol.clone(), fn_ptr as *const u8));

        native_table.register(symbol.clone(), bridge(symbol, fn_ptr));
    }

    // The table now holds pointers into this library, so it holds the library.
    // Nothing the caller does with the returned rider can take the code out
    // from under a table that could still be called through.
    native_table.hold_code_owner(library.clone());

    Ok(LoadedRider {
        rider_name: rider_name.to_string(),
        native_fn_ptrs,
        library,
    })
}

/// The interpreter's way to call the native at `fn_ptr`.
fn bridge(symbol: &str, fn_ptr: *const ()) -> datalove_datafun_interp::NativeFnImpl {
    let symbol_name = symbol.to_string();
    Box::new(move |rt, args, dest, supplied| {
        call_native_bridge(fn_ptr, rt, args, dest, supplied, &symbol_name)
    })
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
