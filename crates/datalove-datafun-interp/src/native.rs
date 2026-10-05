//! Native function dispatch table.
//!
//! Maps linker symbols to the functions that implement native rider functions:
//! a rider's own function, called through the C ABI, or a Rust closure. The
//! interpreter calls through this table when executing `NativeContext` code
//! units.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use datalove_rt::c::LocalRtHandle;
use crate::value::{Value, Destination};
use crate::error::InterpError;

/// A native implemented in Rust, called with the arguments, the destination,
/// and the descriptors the call site had to supply.
///
/// Argument values follow the same conventions as interpreter calls: In params
/// are moved values, Out params are destination pointers, Ref/Mut params are
/// borrowed pointers. The descriptors are for the type parameters no argument
/// determines; see `NativeContext::descriptor_shapes`. Almost every native has
/// none.
pub type NativeFnImpl = Box<
    dyn Fn(
        LocalRtHandle,
        &[Value],
        Destination,
        &[*const datalove_rtdt::TyDesc],
    ) -> Result<(), InterpError>,
>;

/// Where a table finds the natives nobody registered with it.
///
/// For an interpreter that cannot know ahead of time which natives it will
/// call. The one compile-time evaluation runs is the case in point: it is made
/// before the program it evaluates for is compiled, and the riders it might
/// call may not even be built yet. A resolver lets that wait until a call
/// actually needs one.
///
/// Unwind safe because the compiled modules carry one, and callers run code
/// against those under `catch_unwind`. A resolver only adds what it finds, so
/// a panic partway through leaves nothing broken for the next caller.
pub trait NativeResolver: Send + Sync + std::panic::RefUnwindSafe {
    /// The implementation of the native the compiler calls `symbol`.
    ///
    /// Asked at most once per symbol per table, which keeps what it returns.
    /// An error is the call failing, not the program being wrong: the rider
    /// would not build, or did not define the function its interface said.
    fn resolve(&self, symbol: &str) -> Result<NativeTarget, String>;
}

/// What a symbol is registered as.
#[derive(Clone)]
pub enum NativeTarget {
    /// A rider function, with the native C ABI; see `call_c`.
    C(*const ()),
    /// A Rust closure, which tests register.
    Rust(Rc<NativeFnImpl>),
}

impl NativeTarget {
    /// A native implemented in Rust.
    pub fn rust(f: NativeFnImpl) -> Self {
        NativeTarget::Rust(Rc::new(f))
    }
}

/// The most words a C-ABI native call takes, the runtime handle included.
pub const MAX_C_WORDS: usize = 13;

/// Call a rider function with the native C ABI.
///
/// See `botdocs/native-abi.md` for the shape of the words. In short:
///
/// ```text
/// fn(rt, arg0_ptr, arg0_td, ..., out_ptr, out_td, shape_td...) -> u8
/// ```
///
/// Every word is pointer-sized, so the call is made by transmuting to a
/// function type of the right arity. The arities are listed rather than
/// generated because there is no variadic form that would keep the C ABI.
///
/// # Safety
///
/// `fn_ptr` must be a rider function taking `words.len()` words, and the words
/// what it requires.
pub unsafe fn call_c(fn_ptr: *const (), words: &[usize]) {
    /// Transmute to a function of `n` pointer arguments and call it.
    macro_rules! arity {
        ($($n:literal => $($i:literal),+ ;)+) => {
            match words.len() {
                $($n => {
                    let f: extern "C-unwind" fn($(arity!(@ptr $i)),+) -> u8 =
                        unsafe { std::mem::transmute(fn_ptr) };
                    f($(words[$i]),+)
                })+
                n => todo!("native functions with {} C args not yet supported", n),
            }
        };
        (@ptr $i:literal) => { usize };
    }

    let _status: u8 = arity! {
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
    };
}

/// Lay out a native call's C words: the runtime handle, each argument's pointer
/// and descriptor, the destination's, then the supplied descriptors. Returns
/// how many words there are.
pub fn c_words(
    words: &mut [usize; MAX_C_WORDS],
    rt: LocalRtHandle,
    args: &[Value],
    dest: Destination,
    supplied: &[*const datalove_rtdt::TyDesc],
) -> usize {
    let len = 1 + args.len() * 2 + 2 + supplied.len();
    if len > MAX_C_WORDS {
        todo!("native functions with {} C args not yet supported", len);
    }
    words[0] = rt as usize;
    for (i, arg) in args.iter().enumerate() {
        words[1 + 2 * i] = arg.ptr as usize;
        words[2 + 2 * i] = arg.tydesc as usize;
    }
    let at = 1 + 2 * args.len();
    words[at] = dest.ptr as usize;
    words[at + 1] = dest.tydesc as usize;
    for (i, tydesc) in supplied.iter().enumerate() {
        words[at + 2 + i] = *tydesc as usize;
    }
    len
}

/// Table of registered native function implementations.
///
/// Keyed by linker symbol (e.g. `dlr_testlib__int_add`).
pub struct NativeFunctionTable {
    /// In a cell because a call can fill it in from the resolver.
    table: RefCell<rustc_hash::FxHashMap<String, NativeTarget>>,
    /// Counts registrations, so that a caller holding a target it looked up
    /// can tell whether the table still says the same.
    generation: Cell<u64>,
    /// Asked for a symbol missing from the table. Without one a missing symbol
    /// is a bug in whoever filled the table, and panics.
    ///
    /// Held, as `code_owners` are, for whatever the code it resolves to lives
    /// in.
    resolver: Option<Arc<dyn NativeResolver>>,
    /// Whatever the registered implementations' code lives in.
    ///
    /// A native taken out of a loaded library is a pointer into mapped code,
    /// and the mapping goes when the last holder of the library does. The
    /// table holds one, so that a table something can still call is a table
    /// whose code is still there. Without it the two lifetimes were unrelated
    /// and every holder of both had to write down the order they went in.
    ///
    /// Opaque because the table has no business knowing what a library is;
    /// what it needs is for the thing to outlive it. Empty when the natives
    /// were linked into this binary, there being nothing to keep.
    code_owners: Vec<Arc<dyn Any>>,
}

impl NativeFunctionTable {
    /// Create an empty table.
    pub fn new() -> Self {
        Self { table: Default::default(), generation: Cell::new(0), resolver: None, code_owners: Vec::new() }
    }

    /// Register a native implemented in Rust.
    pub fn register(&mut self, symbol: impl Into<String>, f: NativeFnImpl) {
        self.insert(symbol.into(), NativeTarget::rust(f));
    }

    /// Register a rider function, called through the C ABI.
    pub fn register_c(&mut self, symbol: impl Into<String>, fn_ptr: *const ()) {
        self.insert(symbol.into(), NativeTarget::C(fn_ptr));
    }

    fn insert(&self, symbol: String, target: NativeTarget) {
        self.table.borrow_mut().insert(symbol, target);
        self.generation.set(self.generation.get() + 1);
    }

    /// Resolve the natives not registered here through `resolver`.
    pub fn set_resolver(&mut self, resolver: Arc<dyn NativeResolver>) {
        self.resolver = Some(resolver);
    }

    /// Hold what a registered native's code lives in for as long as this table.
    ///
    /// Call this with the library a native was read out of, in the same breath
    /// as registering it. See [`NativeFunctionTable::code_owners`].
    pub fn hold_code_owner(&mut self, owner: Arc<dyn Any>) {
        self.code_owners.push(owner);
    }

    /// What a symbol is registered as, asking the resolver if it is not.
    ///
    /// A caller may keep it for as long as `generation` is unchanged: the code
    /// it leads to is held by the table.
    pub fn lookup(&self, symbol: &str) -> Result<NativeTarget, InterpError> {
        if let Some(target) = self.table.borrow().get(symbol) {
            return Ok(target.clone());
        }
        let resolver = self.resolver.as_ref()
            .unwrap_or_else(|| panic!("native function not registered: {}", symbol));
        let target = resolver.resolve(symbol).map_err(InterpError::RuntimeError)?;
        self.insert(symbol.to_string(), target.clone());
        Ok(target)
    }

    /// How many registrations there have been; see `lookup`.
    pub fn generation(&self) -> u64 {
        self.generation.get()
    }

    /// Call a native function by its linker symbol.
    pub fn call(
        &self,
        symbol: &str,
        rt: LocalRtHandle,
        args: &[Value],
        dest: Destination,
        supplied: &[*const datalove_rtdt::TyDesc],
    ) -> Result<(), InterpError> {
        match self.lookup(symbol)? {
            NativeTarget::C(fn_ptr) => {
                let mut words = [0; MAX_C_WORDS];
                let len = c_words(&mut words, rt, args, dest, supplied);
                // SAFETY: registered as a rider function, whose code the table
                // holds, with the words its ABI says.
                unsafe { call_c(fn_ptr, &words[..len]) };
                Ok(())
            }
            NativeTarget::Rust(f) => f(rt, args, dest, supplied),
        }
    }

    /// Check if a symbol is registered.
    pub fn contains(&self, symbol: &str) -> bool {
        self.table.borrow().contains_key(symbol)
    }
}

impl Default for NativeFunctionTable {
    fn default() -> Self {
        Self::new()
    }
}
