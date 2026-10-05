//! Native function dispatch table.
//!
//! Maps linker symbols to Rust closures that implement native rider functions.
//! The interpreter calls through this table when executing `NativeContext` code units.

use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;
use datalove_rt::c::LocalRtHandle;
use crate::value::{Value, Destination};
use crate::error::InterpError;

/// A native function implementation.
///
/// Receives the runtime handle, argument values, and a destination for the return value.
/// Argument values follow the same conventions as interpreter calls: In params are
/// moved values, Out params are destination pointers, Ref/Mut params are borrowed pointers.
/// A native's implementation, called with the arguments, the destination, and
/// the descriptors the call site had to supply.
///
/// The descriptors are for the type parameters no argument determines; see
/// `NativeContext::descriptor_shapes`. Almost every native has none.
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
    fn resolve(&self, symbol: &str) -> Result<NativeFnImpl, String>;
}

/// Table of registered native function implementations.
///
/// Keyed by linker symbol (e.g. `dlr_testlib__int_add`).
pub struct NativeFunctionTable {
    /// Looked up by name at every call, so hashed with Fx rather than SipHash.
    ///
    /// In a cell because a call can fill it in from the resolver.
    table: RefCell<rustc_hash::FxHashMap<String, NativeFnImpl>>,
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
        Self { table: Default::default(), resolver: None, code_owners: Vec::new() }
    }

    /// Register a native function implementation.
    pub fn register(&mut self, symbol: impl Into<String>, f: NativeFnImpl) {
        self.table.get_mut().insert(symbol.into(), f);
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

    /// Call a native function by its linker symbol.
    pub fn call(
        &self,
        symbol: &str,
        rt: LocalRtHandle,
        args: &[Value],
        dest: Destination,
        supplied: &[*const datalove_rtdt::TyDesc],
    ) -> Result<(), InterpError> {
        if !self.table.borrow().contains_key(symbol) {
            let resolver = self.resolver.as_ref()
                .unwrap_or_else(|| panic!("native function not registered: {}", symbol));
            let f = resolver.resolve(symbol).map_err(InterpError::RuntimeError)?;
            self.table.borrow_mut().insert(symbol.to_string(), f);
        }
        // Borrowed across the call, which cannot reach this table again: a
        // native is handed the runtime and its arguments, not the interpreter.
        let table = self.table.borrow();
        let f = &table[symbol];
        f(rt, args, dest, supplied)
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
