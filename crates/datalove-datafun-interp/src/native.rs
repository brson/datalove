//! Native function dispatch table.
//!
//! Maps linker symbols to Rust closures that implement native rider functions.
//! The interpreter calls through this table when executing `NativeContext` code units.

use std::collections::HashMap;
use datalove_rt::c::LocalRtHandle;
use crate::value::{Value, Destination};
use crate::error::InterpError;

/// A native function implementation.
///
/// Receives the runtime handle, argument values, and a destination for the return value.
/// Argument values follow the same conventions as interpreter calls: In params are
/// moved values, Out params are destination pointers, Ref/Mut params are borrowed pointers.
pub type NativeFnImpl = Box<dyn Fn(LocalRtHandle, &[Value], Destination) -> Result<(), InterpError>>;

/// Table of registered native function implementations.
///
/// Keyed by linker symbol (e.g. `dlr_testlib__int_add`).
pub struct NativeFunctionTable {
    table: HashMap<String, NativeFnImpl>,
}

impl NativeFunctionTable {
    /// Create an empty table.
    pub fn new() -> Self {
        Self { table: HashMap::new() }
    }

    /// Register a native function implementation.
    pub fn register(&mut self, symbol: impl Into<String>, f: NativeFnImpl) {
        self.table.insert(symbol.into(), f);
    }

    /// Call a native function by its linker symbol.
    pub fn call(
        &self,
        symbol: &str,
        rt: LocalRtHandle,
        args: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        let f = self.table.get(symbol)
            .unwrap_or_else(|| panic!("native function not registered: {}", symbol));
        f(rt, args, dest)
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
