//! Type coercion and narrowing operations.
//!
//! Copies values to destinations when types match exactly.

use super::{InterpContext, InterpError, Value, Destination, ValueOwnership};
use super::memory::destroy_value;

/// Coerce a value to a destination type.
///
/// Currently only handles exact type matches (clone to destination).
pub(super) fn coerce_value_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    dest: Destination,
) -> Result<Value, InterpError> {

    let value_tag = unsafe { (*value.tydesc).type_tag };
    let dest_tag = unsafe { (*dest.tydesc).type_tag };

    // If types match, clone the value to dest (not shallow copy - types may have internal pointers).
    if value.tydesc == dest.tydesc {
        let clone_status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                ctx.runtime.handle(),
                value.ptr,
                value.tydesc,
                dest.ptr,
                dest.tydesc,
            )
        };
        if clone_status != datalove_rt::c::RtStatus::Ok {
            destroy_value(ctx, value);
            return Err(InterpError::RuntimeError("Failed to clone value in coercion".to_string()));
        }
        destroy_value(ctx, value);
        return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::TempOwned });
    }

    // No coercion available - type mismatch.
    destroy_value(ctx, value);
    Err(InterpError::RuntimeError(
        format!("Type mismatch: cannot coerce {:?} to {:?}", value_tag, dest_tag)
    ))
}
