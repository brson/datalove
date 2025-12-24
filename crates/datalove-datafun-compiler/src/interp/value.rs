//! Value representation and ownership tracking.
//!
//! The interpreter uses destination-passing style (DPS) to minimize allocations.
//! Values track their memory ownership via `ValueOwnership` to prevent leaks.
//!
//! ## Two Ownership Concepts
//!
//! The interpreter separates two distinct ownership concerns:
//!
//! **Structural ownership** (`ValueOwnership`): Who is responsible for freeing
//! the memory structure that contains the value's data.
//! - `Borrowed` → Memory is owned elsewhere (frame slot, caller's dest). Don't free.
//! - `TempOwned` → Temporary heap allocation. Free structure after use.
//!
//! **Semantic ownership** (move vs copy): Whether reading a value consumes it.
//! - Determined by `is_copy_type()` + `SlotState` tracking
//! - Copy types are cloned on use
//! - Non-copy types are moved (slot marked `Moved`)
//!
//! These are independent: a `Borrowed` value can still be moved (ownership of
//! contents transferred), and a `TempOwned` value can be a copy type.

/// Tracks structural ownership of a Value's memory.
///
/// This determines whether the holder is responsible for freeing the memory
/// structure. It does NOT determine move vs copy semantics (that's handled by
/// type checking and SlotState).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ValueOwnership {
    /// Points to memory owned elsewhere (frame slot, caller's dest, etc.).
    /// The holder must NOT free this memory - the owner will.
    Borrowed,
    /// Temporary heap allocation created during expression evaluation.
    /// The holder MUST free this structure after use to prevent leaks.
    TempOwned,
}

/// Runtime value representation.
///
/// Contains a pointer to the data, a type descriptor for runtime type info,
/// and structural ownership tracking to prevent double-frees or leaks.
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
    pub ownership: ValueOwnership,
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
    /// Create a Value pointing to this destination (Borrowed, since caller owns memory).
    pub fn to_borrowed_value(self) -> Value {
        Value {
            ptr: self.ptr,
            tydesc: self.tydesc,
            ownership: ValueOwnership::Borrowed,
        }
    }
}
