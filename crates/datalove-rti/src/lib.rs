//! The datalove runtime interface: what the runtime exports, without it.
//!
//! A rider is a package of native functions that a datalove program calls. It
//! calls the runtime back, and until now it did that by depending on
//! `datalove-rt`, which meant every rider carried a runtime of its own. Two
//! runtimes in one process is two copies of the code operating on one
//! `RtLocal`, so their idea of that type's layout had to agree, and nothing
//! said so or checked it.
//!
//! This crate holds the other half: the handle, the status types, and the
//! shape of every exported function, with no implementation of any of them.
//!
//! A rider does not link the runtime and does not name its symbols either. It
//! is handed a runtime handle, and the first word of what that points at is a
//! [`table`] of the runtime's functions; [`call`] wraps reading it. So a
//! rider library resolves nothing at load time -- it has no undefined
//! runtime symbols at all, and the process loading it need export none.
//!
//! There is then one runtime, one `RtLocal`, and the handle is opaque to
//! everyone holding it apart from that first word, which is what
//! `LocalRtHandle` being `*mut u8` was meant to mean.
//!
//! [`rtdt`](datalove_rtdt) is the companion to this: the data types that cross
//! the boundary, declared `#[repr(C)]` so their layout is specified rather
//! than agreed by accident. Between the two, a rider needs nothing of the
//! runtime's insides.

pub mod call;
pub mod rider_helpers;
pub mod table;

/// The table of runtime functions a handle carries.
///
/// A handle points at the runtime's own state, whose first word is a pointer
/// to its table. That first word is the whole of what a holder of a handle
/// may assume about what it points at; the rest belongs to the runtime.
///
/// [`call`] is the readable way to reach these. Use this directly for the few
/// functions that take no handle and so have no wrapper.
///
/// # Safety
///
/// `rt` must be a handle the runtime gave out and has not shut down.
#[inline]
pub unsafe fn table(rt: LocalRtHandle) -> &'static table::RtiTable {
    unsafe { &**(rt as *const *const table::RtiTable) }
}

/// A runtime handle. Needed for all calls.
///
/// This is the only native type used in the ABI directly;
/// everything else is an rtdt argument type.
///
/// It is deliberately opaque. What it points at belongs to the runtime, and a
/// caller holding one has no business knowing the shape of it.
pub type LocalRtHandle = *mut u8;

/// A simple status code.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RtStatus {
    Ok = 1,
    Error = 2,
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum RtEq {
    Equals = 1,
    NotEquals = 2,
    /// Type mismatch.
    Error = 3,
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum RtOrdering {
    Less = 1,
    Equal = 2,
    Greater = 3,
    /// Type mismatch.
    Error = 4,
}

/// Debug output mode for debuglog statements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(C)]
pub enum DebugOutputMode {
    /// Print to stderr with newline.
    Stderr = 0,
    /// Store in internal buffer (for tests).
    Buffer = 1,
    /// Do nothing (default).
    #[default]
    Disabled = 2,
}
