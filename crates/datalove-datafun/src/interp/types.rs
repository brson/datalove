//! Type checking utilities for runtime values.

use datalove_rt::rtdt::TyTag;
use super::Value;

/// Check if a value is a u32 type.
pub(super) fn is_u32_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag == TyTag::U32 }
}

/// Check if a value is an int (bigint) type.
pub(super) fn is_int_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag == TyTag::Int }
}

/// Check if a value is an f32 type.
pub(super) fn is_f32_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag == TyTag::F32 }
}

/// Check if a value is a Bool type.
pub(super) fn is_bool_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag == TyTag::Bool }
}

/// Check if a value is a copy type.
///
/// Copy types (u32, bool, f32, fixed-width ints) can be implicitly cloned.
/// Linear types (Int, String, collections) require explicit moves.
pub(super) fn is_copy_type(value: Value) -> bool {
    unsafe {
        match (*value.tydesc).type_tag {
            TyTag::U32 | TyTag::I32 |
            TyTag::U8 | TyTag::I8 |
            TyTag::U16 | TyTag::I16 |
            TyTag::U64 | TyTag::I64 |
            TyTag::F32 | TyTag::Bool => true,
            _ => false,
        }
    }
}
