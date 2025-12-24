//! Allocation functions for compound data structures.
//!
//! Handles creation of maps and sets from vectors of evaluated values.
//! Each function takes ownership of input values and moves their data
//! into the allocated structure.

use super::{InterpContext, InterpError, Value};
use super::memory::{destroy_value, free_value_structure};

/// Write a map from a vector of key-value pairs to a destination.
///
/// Takes ownership of all key and value values. Keys and values are moved into the
/// map's B-tree structure at dest. All keys must have the same type and all values
/// must have the same type.
pub(super) fn write_map_from_values_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    entries: Vec<(Value, Value)>,
    dest: super::Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::TyDescRef;

    if entries.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty map".to_string()));
    }

    // Get key and value tydescs from the map destination type.
    let map_tydesc_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let key_tydesc = map_tydesc_ref.map_key_ty().as_ptr();
    let value_tydesc = map_tydesc_ref.map_value_ty().as_ptr();
    let key_size = unsafe { (*key_tydesc).size as usize };
    let value_size = unsafe { (*value_tydesc).size as usize };
    let key_align = unsafe { (*key_tydesc).align };
    let value_align = unsafe { (*value_tydesc).align };

    let rt_handle = ctx.runtime.handle();

    // Sort entries by key for B-tree construction.
    let mut sorted_entries = entries;
    sorted_entries.sort_by(|a, b| {
        unsafe {
            let a_slice = std::slice::from_raw_parts(a.0.ptr, key_size);
            let b_slice = std::slice::from_raw_parts(b.0.ptr, key_size);
            a_slice.cmp(b_slice)
        }
    });

    // Allocate buffers for keys and values.
    let keys_buffer_size = (sorted_entries.len() * key_size) as u32;
    let values_buffer_size = (sorted_entries.len() * value_size) as u32;

    let keys_buffer = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, keys_buffer_size, key_align, 1)
    };
    if keys_buffer.is_null() {
        for (k, v) in sorted_entries {
            destroy_value(ctx, k);
            destroy_value(ctx, v);
        }
        return Err(InterpError::RuntimeError("Failed to allocate keys buffer".to_string()));
    }

    let values_buffer = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, values_buffer_size, value_align, 1)
    };
    if values_buffer.is_null() {
        for (k, v) in sorted_entries {
            destroy_value(ctx, k);
            destroy_value(ctx, v);
        }
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_raw_local(rt_handle, keys_buffer_size, key_align, 1, keys_buffer);
        }
        return Err(InterpError::RuntimeError("Failed to allocate values buffer".to_string()));
    }

    // Copy keys and values into buffers.
    for (i, (key, value)) in sorted_entries.iter().enumerate() {
        unsafe {
            let key_dest = keys_buffer.add(i * key_size);
            let value_dest = values_buffer.add(i * value_size);
            std::ptr::copy_nonoverlapping(key.ptr, key_dest, key_size);
            std::ptr::copy_nonoverlapping(value.ptr, value_dest, value_size);
        }
    }

    // Build B-tree from sorted slices at dest.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
            rt_handle,
            dest.ptr,
            key_tydesc,
            value_tydesc,
            keys_buffer,
            values_buffer,
            sorted_entries.len() as u32,
        )
    };

    // Free buffers (data has been moved to tree).
    unsafe {
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt_handle, keys_buffer_size, key_align, 1, keys_buffer);
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt_handle, values_buffer_size, value_align, 1, values_buffer);
    }

    if status != datalove_rt::c::RtStatus::Ok {
        for (k, v) in sorted_entries {
            destroy_value(ctx, k);
            destroy_value(ctx, v);
        }
        return Err(InterpError::RuntimeError("Failed to build map B-tree".to_string()));
    }

    // Free original value containers (data has been moved).
    for (k, v) in sorted_entries {
        free_value_structure(ctx, k);
        free_value_structure(ctx, v);
    }

    Ok(())
}

/// Write a set from a vector of evaluated values to a destination.
///
/// Takes ownership of all element values. Elements are moved into the
/// set's B-tree structure at dest. All elements must have the same type.
pub(super) fn write_set_from_values_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    values: Vec<Value>,
    dest: super::Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::TyDescRef;

    if values.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty set".to_string()));
    }

    // Get element tydesc from the set destination type.
    let set_tydesc_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let element_tydesc = set_tydesc_ref.set_element_ty().as_ptr();
    let element_size = unsafe { (*element_tydesc).size as usize };
    let element_align = unsafe { (*element_tydesc).align };

    let rt_handle = ctx.runtime.handle();

    // Sort elements for B-tree construction.
    let mut sorted_values = values;
    sorted_values.sort_by(|a, b| {
        unsafe {
            let a_slice = std::slice::from_raw_parts(a.ptr, element_size);
            let b_slice = std::slice::from_raw_parts(b.ptr, element_size);
            a_slice.cmp(b_slice)
        }
    });

    // Allocate buffer for elements.
    let buffer_size = (sorted_values.len() * element_size) as u32;
    let buffer = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, buffer_size, element_align, 1)
    };
    if buffer.is_null() {
        for v in sorted_values {
            destroy_value(ctx, v);
        }
        return Err(InterpError::RuntimeError("Failed to allocate set buffer".to_string()));
    }

    // Copy elements into buffer.
    for (i, value) in sorted_values.iter().enumerate() {
        unsafe {
            let elem_dest = buffer.add(i * element_size);
            std::ptr::copy_nonoverlapping(value.ptr, elem_dest, element_size);
        }
    }

    // Build B-tree from sorted slice at dest.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
            rt_handle,
            dest.ptr,
            element_tydesc,
            buffer,
            sorted_values.len() as u32,
        )
    };

    // Free buffer (data has been moved to tree).
    unsafe {
        datalove_rt::c::dtlv_rti_mem_free_raw_local(rt_handle, buffer_size, element_align, 1, buffer);
    }

    if status != datalove_rt::c::RtStatus::Ok {
        for v in sorted_values {
            destroy_value(ctx, v);
        }
        return Err(InterpError::RuntimeError("Failed to build set B-tree".to_string()));
    }

    // Free original value containers (data has been moved).
    for v in sorted_values {
        free_value_structure(ctx, v);
    }

    Ok(())
}
