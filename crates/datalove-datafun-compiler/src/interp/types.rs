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

/// Check if a value is a copy type.
///
/// Copy types (u32, bool, f32, fixed-width ints) can be implicitly cloned.
/// Linear types (Int, String, collections) require explicit moves.
/// Option<T> is copy if T is copy.
pub(super) fn is_copy_type(value: Value) -> bool {
    is_copy_type_by_tydesc(value.tydesc)
}

/// Check if a type descriptor represents a copy type.
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
