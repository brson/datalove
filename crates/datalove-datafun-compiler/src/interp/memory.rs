//! Memory management for interpreter values.
//!
//! Functions for destroying, freeing, and cloning values while respecting
//! ownership semantics tracked by `ValueLocation`.

use super::{InterpContext, Value, Destination, ValueLocation};

/// Clone a value (for copy types or explicit cloning).
pub(super) fn clone_value<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) -> Value {
    let rt_handle = ctx.runtime.handle();

    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(
            rt_handle,
            value.tydesc,
            1
        )
    };

    unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt_handle,
            value.ptr,
            value.tydesc,
            cloned_ptr,
            value.tydesc,
        );
    }

    Value {
        ptr: cloned_ptr,
        tydesc: value.tydesc,
        location: ValueLocation::TempOwned,
    }
}

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
        location: ValueLocation::Borrowed,
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
        location: ValueLocation::Borrowed,
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

/// Destroy a value, respecting its location.
///
/// For TempOwned values: destroys contents AND frees the memory structure.
/// For Borrowed values: destroys contents only (frame owns the memory).
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

        if value.location == ValueLocation::TempOwned {
            datalove_rt::c::dtlv_rti_mem_free_local(
                rt_handle,
                value.tydesc,
                1,
                value.ptr,
            );
        }
    }
}

/// Free only the value structure without destroying contents.
///
/// Use this when a value's bytes have been copied to a frame slot,
/// and the frame now owns the pointers. This frees the temporary
/// heap-allocated structure but leaves sub-allocations intact.
pub fn free_value_structure<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    if value.location != ValueLocation::TempOwned {
        return;
    }

    unsafe {
        let rt_handle = ctx.runtime.handle();
        datalove_rt::c::dtlv_rti_mem_free_local(
            rt_handle,
            value.tydesc,
            1,
            value.ptr,
        );
    }
}
