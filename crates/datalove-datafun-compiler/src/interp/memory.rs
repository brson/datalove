//! Memory management for interpreter values.
//!
//! Functions for destroying, freeing, and cloning values.
//! All values are now Borrowed (owned by caller's frame).

use super::{InterpContext, Value, Destination, ValueOwnership};

/// Clone a value into a pre-allocated destination.
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
        // Tydescs differ but might represent the same type.
        let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
        let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

        if src_ref.type_tag() == dst_ref.type_tag() && src_ref.size() == dst_ref.size() {
            // Types are structurally compatible, use raw memcpy.
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
    Value {
        ptr: dest.ptr,
        tydesc: dest.tydesc,
        ownership: ValueOwnership::Borrowed,
    }
}

/// Move a value into a pre-allocated destination (shallow copy).
///
/// Unlike `clone_value_to_dest`, this does a shallow memcpy of the structure
/// bytes, transferring ownership of any heap-allocated data. The source
/// should be marked as Moved after calling this to prevent double-free.
pub(super) fn move_value_to_dest(
    value: Value,
    dest: Destination,
) -> Value {
    use datalove_rt::rtdt::TyDescRef;

    let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
    let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    if src_ref.size() == dst_ref.size() {
        // Shallow copy: just copy the structure bytes (including any pointers).
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, src_ref.size() as usize);
        }
    } else {
        panic!(
            "move_value_to_dest: incompatible sizes {} vs {}",
            src_ref.size(), dst_ref.size()
        );
    }

    Value {
        ptr: dest.ptr,
        tydesc: dest.tydesc,
        ownership: ValueOwnership::Borrowed,
    }
}

/// Destroy only the contents of a value without freeing its memory.
///
/// Use this for values stored inline in frame buffers, where the memory
/// is owned by the frame Vec<u8> and should not be freed individually.
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

/// Destroy a value's contents (frame owns the memory structure).
///
/// All values are now Borrowed, so this only destroys contents.
/// The memory structure is owned by the caller's frame.
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

/// Free only the value structure without destroying contents.
///
/// Since all values are now Borrowed (owned by caller's frame),
/// this is a no-op. Retained for API compatibility.
#[allow(unused_variables)]
pub fn free_value_structure<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    // All values are Borrowed - structure owned by caller's frame.
    // No action needed.
}
