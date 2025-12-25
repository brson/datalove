//! Value and Destination types for DPS evaluation.
//!
//! The interpreter uses destination-passing style (DPS): expressions receive a
//! write target and produce readable data. Both types are (ptr, tydesc) pairs
//! pointing to caller-owned memory (frame slots).

/// Readable data after expression evaluation.
///
/// Points to data in caller-owned memory (frame slot or DPS destination).
/// Used for: function arguments, operator operands, cleanup, control flow.
#[derive(Copy, Clone, Debug)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
}

/// Write target for DPS expression evaluation.
///
/// Expression evaluation writes directly to this location. After writing,
/// call `to_value()` to get a readable `Value` from the same memory.
#[derive(Copy, Clone, Debug)]
pub struct Destination {
    pub ptr: *mut u8,
    pub tydesc: *const datalove_rt::rtdt::TyDesc,
}

impl Destination {
    /// Convert to a Value after data has been written.
    pub fn to_value(self) -> Value {
        Value {
            ptr: self.ptr,
            tydesc: self.tydesc,
        }
    }
}
