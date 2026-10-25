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
/// determines; see `NativeContext::undetermined_type_params`. Almost every native has
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

/// The most words the interpreter calls a C-ABI native with, the runtime
/// handle included: fourteen parameters and the result, or fewer with
/// descriptors. The compiled backends call any width; a wider native is an
/// error here.
pub const MAX_C_WORDS: usize = 32;

/// One word of a C-ABI native call.
///
/// A pointer, since every word is one: the runtime handle, a value, a
/// descriptor. Passing them as integers would be a call through the wrong
/// type, integers and pointers not being ABI-compatible, and would drop the
/// pointers' provenance besides. Any thin pointer type is compatible with any
/// other, so this one serves for all of a rider's parameters.
pub type CWord = *mut u8;

/// Call a rider function with the native C ABI.
///
/// See `botdocs/native-abi.md` for the shape of the words. In short:
///
/// ```text
/// fn(rt, arg0_ptr, arg0_td, ..., out_ptr, out_td, shape_td...) -> u8
/// ```
///
/// Every word is a pointer, so the call is made by transmuting to a function
/// type of the right arity. The arities are listed rather than
/// generated because there is no variadic form that would keep the C ABI.
///
/// # Safety
///
/// `fn_ptr` must be a rider function taking `words.len()` words, and the words
/// what it requires.
pub unsafe fn call_c(fn_ptr: *const (), words: &[CWord]) {
    /// Transmute to a function of `n` pointer arguments and call it.
    macro_rules! arity {
        ($($n:literal => $($i:literal),+ ;)+) => {
            match words.len() {
                $($n => {
                    let f: extern "C-unwind" fn($(arity!(@ptr $i)),+) -> u8 =
                        unsafe { std::mem::transmute(fn_ptr) };
                    f($(words[$i]),+)
                })+
                n => unreachable!("a native call of {n} words; `c_words` refuses more than {MAX_C_WORDS}"),
            }
        };
        (@ptr $i:literal) => { CWord };
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
        14 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13;
        15 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14;
        16 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15;
        17 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16;
        18 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17;
        19 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18;
        20 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19;
        21 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20;
        22 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21;
        23 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22;
        24 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23;
        25 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24;
        26 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25;
        27 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26;
        28 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27;
        29 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28;
        30 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29;
        31 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30;
        32 => 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31;
    };
}

/// Lay out a native call's C words: the runtime handle, each argument's pointer
/// and descriptor, the destination's, then the supplied descriptors. Returns
/// how many words there are, or an error past `MAX_C_WORDS`.
///
/// The arguments come as they are read, so a caller with them in hand passes
/// them and one that reads each from somewhere makes no list of them first.
pub fn c_words(
    words: &mut [CWord; MAX_C_WORDS],
    rt: LocalRtHandle,
    args: impl ExactSizeIterator<Item = Value>,
    dest: Destination,
    supplied: &[*const datalove_rtdt::TyDesc],
) -> Result<usize, InterpError> {
    let n = args.len();
    let len = 1 + n * 2 + 2 + supplied.len();
    if len > MAX_C_WORDS {
        return Err(InterpError::RuntimeError(format!(
            "a native function called with {len} words; the interpreter calls at most {MAX_C_WORDS}")));
    }
    words[0] = rt as CWord;
    for (i, arg) in args.enumerate() {
        words[1 + 2 * i] = arg.ptr as CWord;
        words[2 + 2 * i] = arg.tydesc as CWord;
    }
    let at = 1 + 2 * n;
    words[at] = dest.ptr as CWord;
    words[at + 1] = dest.tydesc as CWord;
    for (i, tydesc) in supplied.iter().enumerate() {
        words[at + 2 + i] = *tydesc as CWord;
    }
    Ok(len)
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
                let mut words = [std::ptr::null_mut(); MAX_C_WORDS];
                let len = c_words(&mut words, rt, args.iter().copied(), dest, supplied)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static SEEN: Cell<usize> = const { Cell::new(0) };
    }

    /// A rider function of the widest arity, recording a fold of its words
    /// that tells each position apart.
    extern "C-unwind" fn widest(
        w0: CWord, w1: CWord, w2: CWord, w3: CWord, w4: CWord, w5: CWord, w6: CWord,
        w7: CWord, w8: CWord, w9: CWord, w10: CWord, w11: CWord, w12: CWord, w13: CWord,
        w14: CWord, w15: CWord, w16: CWord, w17: CWord, w18: CWord, w19: CWord,
        w20: CWord, w21: CWord, w22: CWord, w23: CWord, w24: CWord, w25: CWord,
        w26: CWord, w27: CWord, w28: CWord, w29: CWord, w30: CWord, w31: CWord,
    ) -> u8 {
        let fold =
            w0.addr().rotate_left(0) ^ w1.addr().rotate_left(1) ^ w2.addr().rotate_left(2) ^
            w3.addr().rotate_left(3) ^ w4.addr().rotate_left(4) ^ w5.addr().rotate_left(5) ^
            w6.addr().rotate_left(6) ^ w7.addr().rotate_left(7) ^ w8.addr().rotate_left(8) ^
            w9.addr().rotate_left(9) ^ w10.addr().rotate_left(10) ^ w11.addr().rotate_left(11) ^
            w12.addr().rotate_left(12) ^ w13.addr().rotate_left(13) ^ w14.addr().rotate_left(14) ^
            w15.addr().rotate_left(15) ^ w16.addr().rotate_left(16) ^ w17.addr().rotate_left(17) ^
            w18.addr().rotate_left(18) ^ w19.addr().rotate_left(19) ^ w20.addr().rotate_left(20) ^
            w21.addr().rotate_left(21) ^ w22.addr().rotate_left(22) ^ w23.addr().rotate_left(23) ^
            w24.addr().rotate_left(24) ^ w25.addr().rotate_left(25) ^ w26.addr().rotate_left(26) ^
            w27.addr().rotate_left(27) ^ w28.addr().rotate_left(28) ^ w29.addr().rotate_left(29) ^
            w30.addr().rotate_left(30) ^ w31.addr().rotate_left(31);
        SEEN.with(|seen| seen.set(fold));
        1
    }

    #[test]
    fn the_widest_native_gets_every_word_in_place() {
        let words: Vec<CWord> = (0..MAX_C_WORDS).map(|i| std::ptr::without_provenance_mut(1000 + i)).collect();
        let want = words.iter().enumerate().fold(0, |acc, (i, w)| acc ^ w.addr().rotate_left(i as u32));
        unsafe { call_c(widest as *const (), &words) };
        assert_eq!(SEEN.with(Cell::get), want);
    }

    #[test]
    fn a_native_wider_than_the_interpreter_calls_is_an_error() {
        let mut words = [std::ptr::null_mut(); MAX_C_WORDS];
        let dest = Destination { ptr: std::ptr::null_mut(), tydesc: std::ptr::null() };
        let args = vec![Value { ptr: std::ptr::null_mut(), tydesc: std::ptr::null() }; 15];
        let result = c_words(&mut words, std::ptr::null_mut(), args.into_iter(), dest, &[]);
        assert!(matches!(result, Err(InterpError::RuntimeError(_))));
    }
}
