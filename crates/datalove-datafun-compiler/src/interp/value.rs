//! Value representation and ownership tracking.
//!
//! The interpreter uses destination-passing style (DPS) to minimize allocations.
//! Values track their memory ownership via `ValueLocation` to prevent leaks.

/// Tracks whether a Value's memory needs freeing after use.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ValueLocation {
    /// Points to frame buffer or caller's data via reference. Don't free.
    Borrowed,
    /// Temp heap allocation for expression evaluation. Free structure after use.
    TempOwned,
}

/// Runtime value representation.
///
/// Contains a pointer to the data, a type descriptor for runtime type info,
/// and ownership tracking to prevent double-frees or leaks.
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
    pub location: ValueLocation,
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
            location: ValueLocation::Borrowed,
        }
    }
}

/// Evaluation context for unified expression evaluation.
///
/// This enum allows sharing expression evaluation code between script scope
/// and frame-based execution while keeping context-specific operations separate.
#[derive(Copy, Clone, Debug)]
pub enum EvalContext {
    /// Script scope evaluation - uses ScriptScope for variables.
    ScriptScope,
    /// Frame-based evaluation - uses the current stack frame.
    Frame,
}
