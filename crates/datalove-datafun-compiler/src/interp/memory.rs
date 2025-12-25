//! Memory operations: clone, move, and destroy.
//!
//! All values live in caller-owned memory (frame slots). These functions
//! handle cloning for copy semantics, moving for linear semantics, and
//! destroying heap-owned data (strings, collections) without freeing the
//! slot memory itself.

use super::{InterpContext, Value, Destination};

/// Deep-clone a value to a destination.
///
/// Copies the value and any heap-allocated data it owns. For compatible
/// type descriptors, uses the runtime clone. For structurally compatible
/// types with different tydesc pointers, uses memcpy.
pub(super) fn clone_value_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    dest: Destination,
) -> Value {
    use datalove_rt::rtdt::TyDescRef;

    if value.tydesc == dest.tydesc {
        let rt_handle = ctx.runtime.handle();
        unsafe {
            datalove_rt::c::dtlv_rti_clone_local(rt_handle, value.ptr, value.tydesc, dest.ptr, dest.tydesc);
        }
    } else {
        let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
        let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

        if src_ref.type_tag() == dst_ref.type_tag() && src_ref.size() == dst_ref.size() {
            unsafe {
                std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, src_ref.size() as usize);
            }
        } else {
            panic!(
                "clone_value_to_dest: incompatible types {:?} (size {}) vs {:?} (size {})",
                src_ref.type_tag(), src_ref.size(),
                dst_ref.type_tag(), dst_ref.size()
            );
        }
    }
    Value { ptr: dest.ptr, tydesc: dest.tydesc }
}

/// Shallow-move a value to a destination.
///
/// Copies the slot bytes (including any heap pointers) without cloning
/// heap data. The source must be marked `Moved` afterward to prevent
/// double-free.
pub(super) fn move_value_to_dest(
    value: Value,
    dest: Destination,
) -> Value {
    use datalove_rt::rtdt::TyDescRef;

    let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
    let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    if src_ref.size() == dst_ref.size() {
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, src_ref.size() as usize);
        }
    } else {
        panic!(
            "move_value_to_dest: incompatible sizes {} vs {}",
            src_ref.size(), dst_ref.size()
        );
    }

    Value { ptr: dest.ptr, tydesc: dest.tydesc }
}

/// Destroy heap-owned data without freeing slot memory.
///
/// Frees strings, collections, and other heap data owned by the value.
/// The slot memory itself (in the frame buffer) is not freed.
pub fn destroy_value_contents_only<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );
    }
}

/// Destroy a value's heap-owned data.
///
/// Equivalent to `destroy_value_contents_only`. The slot memory is owned
/// by the frame and freed when the frame is dropped.
pub fn destroy_value<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );
    }
}

/// No-op: slot memory is owned by the frame.
///
/// Retained for API compatibility. All values live in frame slots;
/// the frame buffer is freed when the frame is dropped.
#[allow(unused_variables)]
pub fn free_value_structure<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
}
