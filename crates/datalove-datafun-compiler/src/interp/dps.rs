//! Destination-passing style (DPS) field and payload accessors.
//!
//! Helpers for navigating compound types in DPS mode:
//! - Tuple/struct field access
//! - Option/Result payload access
//! - Wrapper evaluation (Some/Ok)

use super::{InterpContext, InterpError, Destination, eval_expression_frame};
use crate::ast;

/// Get a destination for a specific tuple field from the tuple destination.
///
/// Returns the field destination with proper offset and tydesc.
pub(crate) fn get_tuple_field_dest(dest: Destination, field_index: usize) -> Option<Destination> {
    use datalove_rt::rtdt::{TyDescRef, TyDesc};

    let tuple_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let tuple_info = tuple_ref.tuple_info();

    tuple_info.field(field_index).map(|field| {
        let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
        let field_tydesc = field.tydesc().as_ptr() as *mut TyDesc;
        Destination { ptr: field_ptr, tydesc: field_tydesc }
    })
}

/// Get a destination for a specific struct field from the struct destination.
///
/// Returns the field destination with proper offset and tydesc.
/// Fields are accessed by index (assumes canonical sorted order).
pub(crate) fn get_struct_field_dest(dest: Destination, field_index: usize) -> Option<Destination> {
    use datalove_rt::rtdt::{TyDescRef, TyDesc};

    let struct_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let struct_info = struct_ref.struct_info();

    struct_info.field(field_index).map(|field| {
        let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
        let field_tydesc = field.tydesc().as_ptr() as *mut TyDesc;
        Destination { ptr: field_ptr, tydesc: field_tydesc }
    })
}

/// Create a destination for an Option's payload from the Option destination.
pub(crate) fn get_payload_dest_for_option(dest: Destination) -> Destination {
    use datalove_rt::rtdt::{TyDescRef, TyDesc, layout::compute_option_layout};

    let option_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let layout = compute_option_layout(option_ref);
    let payload_tydesc = option_ref.option_inner_ty().as_ptr() as *mut TyDesc;
    let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };

    Destination { ptr: payload_ptr, tydesc: payload_tydesc }
}

/// Create a destination for a Result's Ok payload from the Result destination.
pub(crate) fn get_ok_payload_dest_for_result(dest: Destination) -> Destination {
    use datalove_rt::rtdt::{TyDescRef, TyDesc, layout::compute_result_layout};

    let result_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let layout = compute_result_layout(result_ref);
    let payload_tydesc = result_ref.result_ok_ty().as_ptr() as *mut TyDesc;
    let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };

    Destination { ptr: payload_ptr, tydesc: payload_tydesc }
}

/// Wrapper kind for DPS evaluation of Some/Ok expressions.
#[derive(Copy, Clone)]
pub(crate) enum WrapperKind {
    Some,
    Ok,
}

/// Evaluate a Some or Ok expression with DPS.
///
/// Common logic for evaluating wrapper payloads directly into destination memory.
pub(super) fn eval_wrapper_payload_dps<'db>(
    ctx: &mut InterpContext<'db>,
    kind: WrapperKind,
    dest: Destination,
    payload_expr: ast::ExprFun<'db>,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyTag, OptionTag, ResultTag};

    // Verify destination type and get payload setup.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    let (expected_tag, tag_value, payload_dest) = match kind {
        WrapperKind::Some => {
            if dest_tag != TyTag::Option {
                return Err(InterpError::RuntimeError(
                    format!("some requires Option destination, got {:?}", dest_tag)
                ));
            }
            (TyTag::Option, OptionTag::Some as u8, get_payload_dest_for_option(dest))
        }
        WrapperKind::Ok => {
            if dest_tag != TyTag::Result {
                return Err(InterpError::RuntimeError(
                    format!("ok requires Result destination, got {:?}", dest_tag)
                ));
            }
            (TyTag::Result, ResultTag::Ok as u8, get_ok_payload_dest_for_result(dest))
        }
    };
    let _ = expected_tag; // Used for error check above.

    // Write variant tag.
    unsafe { *(dest.ptr as *mut u8) = tag_value; }

    // Evaluate payload with DPS - writes directly to payload_dest.
    eval_expression_frame(ctx, payload_expr, payload_dest)?;

    Ok(())
}
