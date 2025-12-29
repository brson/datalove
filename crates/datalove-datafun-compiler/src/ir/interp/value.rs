//! Value and destination types for the interpreter.

use datalove_rt::rtdt::TyDesc;

/// Readable value pointer with type descriptor.
#[derive(Copy, Clone, Debug)]
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
