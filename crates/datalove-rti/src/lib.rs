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

/// What a rider and a runtime must agree on, as one number.
///
/// A rider calls the runtime through a table it reaches at run time, so
/// nothing is resolved when its library loads and there is no link step to
/// fail. That is what makes the library portable, and it costs us the linker
/// as a checker: two sides that disagree do not fail to load, they call a
/// function with the wrong arguments or read a field at the wrong offset and
/// carry on. So they are compared on this instead, once, when the library is
/// loaded.
///
/// Each side computes it at compile time from its own copy of these crates,
/// so it differs exactly when their idea of the interface differs. What goes
/// in is the shape of the table and the layout of the `rtdt` types that cross
/// the boundary. What does not is the version of either crate: a release that
/// moves no field should not invalidate a rider, and a version number says
/// nothing about whether `index-64` is on, which changes layout by itself.
pub const ABI_VERSION: u64 = abi_version();

const fn abi_version() -> u64 {
    use datalove_rtdt as rtdt;
    use std::mem::{align_of, offset_of, size_of};

    let hash = mix(table::TABLE_SHAPE, table::EXPORTED as u64);

    // A descriptor reaches every native, and its union's size is set by the
    // widest arm, so a new kind of type shows up here.
    let hash = mix(hash, size_of::<rtdt::TyDesc>() as u64);
    let hash = mix(hash, align_of::<rtdt::TyDesc>() as u64);
    let hash = mix(hash, size_of::<rtdt::TyInfo>() as u64);
    let hash = mix(hash, offset_of!(rtdt::TyDesc, type_tag) as u64);
    let hash = mix(hash, offset_of!(rtdt::TyDesc, flags) as u64);
    let hash = mix(hash, rtdt::TyDesc::PLAIN as u64);
    let hash = mix(hash, offset_of!(rtdt::TyDesc, size) as u64);
    let hash = mix(hash, offset_of!(rtdt::TyDesc, align) as u64);
    let hash = mix(hash, offset_of!(rtdt::TyDesc, type_info) as u64);

    // The values a rider reads through and writes back. Sizes alone would
    // miss two fields of one width trading places, so the offsets are here
    // too. This list is what crosses today and has to grow with it.
    let hash = mix(hash, size_of::<rtdt::String>() as u64);
    let hash = mix(hash, offset_of!(rtdt::String, data) as u64);
    let hash = mix(hash, offset_of!(rtdt::String, size) as u64);
    let hash = mix(hash, offset_of!(rtdt::String, capacity) as u64);

    let hash = mix(hash, size_of::<rtdt::Int>() as u64);
    let hash = mix(hash, offset_of!(rtdt::Int, data) as u64);
    let hash = mix(hash, offset_of!(rtdt::Int, size_and_sign) as u64);
    let hash = mix(hash, offset_of!(rtdt::Int, capacity) as u64);

    let hash = mix(hash, size_of::<rtdt::List>() as u64);
    let hash = mix(hash, offset_of!(rtdt::List, data) as u64);
    let hash = mix(hash, offset_of!(rtdt::List, size) as u64);
    let hash = mix(hash, offset_of!(rtdt::List, capacity) as u64);

    // A map or set descriptor carries its node layouts, and a rider is
    // handed descriptors and hands them back.
    let hash = mix(hash, offset_of!(rtdt::TyInfoMap, leaf) as u64);
    let hash = mix(hash, offset_of!(rtdt::TyInfoMap, internal) as u64);
    let hash = mix(hash, offset_of!(rtdt::TyInfoSet, leaf) as u64);
    let hash = mix(hash, offset_of!(rtdt::TyInfoSet, internal) as u64);
    let hash = mix(hash, size_of::<rtdt::MapNodeLeafLayout>() as u64);
    let hash = mix(hash, size_of::<rtdt::SetNodeLeafLayout>() as u64);

    // Which is `index-64`, the one feature that moves layout.
    mix(hash, size_of::<rtdt::IndexRepr>() as u64)
}

/// FNV-1a over one value's bytes. Detects difference; forging it is not a
/// threat anybody has.
const fn mix(mut hash: u64, value: u64) -> u64 {
    let bytes = value.to_le_bytes();
    let mut i = 0;
    while i < 8 {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

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
