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

/// Check if value is a copy type (clones transparently).
///
/// Copy: fixed-width ints, bool, f32, Option<copy>, Tuple of copy.
/// Linear: Int, String, collections.
pub(super) fn is_copy_type(value: Value) -> bool {
    is_copy_type_by_tydesc(value.tydesc)
}

fn is_copy_type_by_tydesc(tydesc: *const datalove_rt::rtdt::TyDesc) -> bool {
    unsafe {
        match (*tydesc).type_tag {
            TyTag::U32 | TyTag::I32 |
            TyTag::U8 | TyTag::I8 |
            TyTag::U16 | TyTag::I16 |
            TyTag::U64 | TyTag::I64 |
            TyTag::F32 | TyTag::Bool => true,
            TyTag::Option => {
                // Option is copy if inner type is copy.
                let tydesc_ref = datalove_rt::rtdt::TyDescRef::from_ptr(tydesc);
                is_copy_type_by_tydesc(tydesc_ref.option_inner_ty().as_ptr())
            }
            TyTag::Tuple => {
                // Tuple is copy if all fields are copy.
                let tydesc_ref = datalove_rt::rtdt::TyDescRef::from_ptr(tydesc);
                for field in tydesc_ref.iter_tuple_fields() {
                    if !is_copy_type_by_tydesc(field.tydesc().as_ptr()) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }
}
