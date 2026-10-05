//! Value and destination types.
//!
//! Both are `(ptr, tydesc)` pairs - `Value` for reading, `Destination` for writing.

use datalove_rtdt::TyDesc;

/// Readable value pointer with type descriptor.
///
/// `repr(C)`, pointer then descriptor, because a frame's parameters are laid
/// out as these in the frame's bytes and read back as a slice of them.
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Value {
    pub ptr: *mut u8,
    pub tydesc: *const TyDesc,
}

/// Write destination with type descriptor.
#[derive(Copy, Clone, Debug)]
pub struct Destination {
    pub ptr: *mut u8,
    pub tydesc: *const TyDesc,
}

impl Destination {
    pub fn to_value(self) -> Value {
        Value { ptr: self.ptr, tydesc: self.tydesc }
    }
}
