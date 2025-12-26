//! Type predicates for runtime values.

use datalove_rt::rtdt::TyTag;
use super::Value;

/// Check if value is Int (bigint).
pub(super) fn is_int_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag == TyTag::Int }
}

/// Check if value is f32.
pub(super) fn is_f32_value(value: Value) -> bool {
    unsafe { (*value.tydesc).type_tag == TyTag::F32 }
}

/// Check if value is a fixed-width integer (u8/i8/.../u64/i64).
pub(super) fn is_fixed_int_value(value: Value) -> bool {
    unsafe {
        matches!(
            (*value.tydesc).type_tag,
            TyTag::U8 | TyTag::I8 |
            TyTag::U16 | TyTag::I16 |
            TyTag::U32 | TyTag::I32 |
            TyTag::U64 | TyTag::I64
        )
    }
}

/// Get the type tag.
pub(super) fn get_type_tag(value: Value) -> TyTag {
    unsafe { (*value.tydesc).type_tag }
}
