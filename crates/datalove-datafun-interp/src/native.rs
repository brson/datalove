//! Native function dispatch table.
//!
//! Maps linker symbols to Rust closures that implement native rider functions.
//! The interpreter calls through this table when executing `NativeContext` code units.

use std::any::Any;
use std::collections::HashMap;
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

/// Table of registered native function implementations.
///
/// Keyed by linker symbol (e.g. `dlr_testlib__int_add`).
pub struct NativeFunctionTable {
    table: HashMap<String, NativeFnImpl>,
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
        Self { table: HashMap::new(), code_owners: Vec::new() }
    }

    /// Register a native function implementation.
    pub fn register(&mut self, symbol: impl Into<String>, f: NativeFnImpl) {
        self.table.insert(symbol.into(), f);
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
        let f = self.table.get(symbol)
            .unwrap_or_else(|| panic!("native function not registered: {}", symbol));
        f(rt, args, dest, supplied)
    }

    /// Check if a symbol is registered.
    pub fn contains(&self, symbol: &str) -> bool {
        self.table.contains_key(symbol)
    }
}

impl Default for NativeFunctionTable {
    fn default() -> Self {
        Self::new()
    }
}
