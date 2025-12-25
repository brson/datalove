//! Value representation for the interpreter.
//!
//! The interpreter uses destination-passing style (DPS) to minimize allocations.
//! All values are borrowed from caller-owned memory (frame slots or destinations).
//!
//! **Semantic ownership** (move vs copy) determines whether reading a value consumes it:
//! - Determined by `is_copy_type()` + `SlotState` tracking
//! - Copy types are cloned on use
//! - Non-copy types are moved (slot marked `Moved`)

/// Runtime value representation.
///
/// Contains a pointer to the data and a type descriptor for runtime type info.
/// All values are borrowed from caller-owned memory (frame slots or destinations).
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
}

/// Destination for DPS (Destination-Passing Style) expression evaluation.
///
/// When provided, expression evaluation writes directly to this location
/// instead of allocating a temporary.
#[derive(Copy, Clone, Debug)]
pub struct Destination {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
}

impl Destination {
    /// Create a Value pointing to this destination.
    pub fn to_value(self) -> Value {
        Value {
            ptr: self.ptr,
            tydesc: self.tydesc,
        }
    }
}
