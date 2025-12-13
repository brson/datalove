//! Allocation functions for compound data structures.
//!
//! Handles creation of tuples, structs, lists, maps, and sets from
//! vectors of evaluated values. Each function takes ownership of input
//! values and moves their data into the allocated structure.

use bct::text::InternedText;

use super::{InterpContext, InterpError, Value, ValueLocation};
use super::memory::{destroy_value, free_value_structure};

/// Allocate a tuple from a vector of evaluated values.
///
/// Takes ownership of all element values, copying their data into the tuple
/// and freeing their original containers.
pub(super) fn allocate_tuple_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    values: Vec<Value>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if values.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty tuple".to_string()));
    }

    // Collect element tydescs from the values.
    let element_tydescs: Vec<*const rtdt::TyDesc> = values.iter()
        .map(|v| v.tydesc)
        .collect();

    // Create tuple tydesc.
    let tuple_tydesc = ctx.tydesc_table.get_or_create_tuple(&element_tydescs);
    let tuple_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(tuple_tydesc) };

    // Compute layout to get field offsets.
    let layout = unsafe { rtdt::layout::compute_tuple_layout(tuple_tydesc_ref) };

    // Allocate memory for tuple.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tuple_tydesc, 1)
    };

    if ptr.is_null() {
        // Clean up all values on allocation failure.
        for value in values {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to allocate tuple".to_string()));
    }

    // Copy each element to its field offset in the tuple.
    for (i, value) in values.into_iter().enumerate() {
        let field_offset = layout.field_offsets[i] as usize;
        let element_size = unsafe { (*value.tydesc).size as usize };

        unsafe {
            let field_ptr = ptr.add(field_offset);
            std::ptr::copy_nonoverlapping(value.ptr, field_ptr, element_size);
        }

        // Free the element's container (data has been copied to tuple).
        if value.location == ValueLocation::TempOwned {
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(
                    ctx.runtime.handle(),
                    value.tydesc,
                    1,
                    value.ptr,
                );
            }
        }
    }

    Ok(Value {
        ptr,
        tydesc: tuple_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a struct from field names and evaluated values.
///
/// Takes ownership of all field values, copying their data into the struct
/// and freeing their original containers. Fields must be provided in sorted
/// order by name for canonical representation.
pub(super) fn allocate_struct_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    fields: Vec<(InternedText<'db>, Value)>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if fields.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty struct".to_string()));
    }

    // Collect field names and tydescs from the values.
    let field_names_and_tydescs: Vec<(InternedText<'db>, *const rtdt::TyDesc)> = fields.iter()
        .map(|(name, value)| (*name, value.tydesc))
        .collect();

    // Create struct tydesc.
    let struct_tydesc = ctx.tydesc_table.get_or_create_struct(&field_names_and_tydescs);
    let struct_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(struct_tydesc) };

    // Compute layout to get field offsets.
    let layout = unsafe { rtdt::layout::compute_struct_layout(struct_tydesc_ref) };

    // Allocate memory for struct.
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, struct_tydesc, 1)
    };

    if ptr.is_null() {
        // Clean up all values on allocation failure.
        for (_, value) in fields {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to allocate struct".to_string()));
    }

    // Copy each field value to its offset in the struct.
    for (i, (_, value)) in fields.into_iter().enumerate() {
        let field_offset = layout.field_offsets[i] as usize;
        let field_size = unsafe { (*value.tydesc).size as usize };

        unsafe {
            let field_ptr = ptr.add(field_offset);
            std::ptr::copy_nonoverlapping(value.ptr, field_ptr, field_size);
        }

        // Free the field's container (data has been copied to struct).
        if value.location == ValueLocation::TempOwned {
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(
                    ctx.runtime.handle(),
                    value.tydesc,
                    1,
                    value.ptr,
                );
            }
        }
    }

    Ok(Value {
        ptr,
        tydesc: struct_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a list from a vector of evaluated values.
///
/// Takes ownership of all element values, copying their data into the list
/// and freeing their original containers. All elements must have the same type.
pub(super) fn allocate_list_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    values: Vec<Value>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if values.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty list".to_string()));
    }

    // All elements must have same type - use first element's tydesc.
    let element_tydesc = values[0].tydesc;
    let element_size = unsafe { (*element_tydesc).size as usize };

    // Create list tydesc.
    let list_tydesc = ctx.tydesc_table.create_list_from_element_tydesc(element_tydesc);

    // Build contiguous buffer of element data.
    let mut buffer = Vec::with_capacity(values.len() * element_size);
    for value in &values {
        unsafe {
            let slice = std::slice::from_raw_parts(value.ptr, element_size);
            buffer.extend_from_slice(slice);
        }
    }

    // Allocate list value.
    let rt_handle = ctx.runtime.handle();
    let list_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, list_tydesc, 1)
    };

    if list_ptr.is_null() {
        for value in values {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to allocate list".to_string()));
    }

    // Create list from slice.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_from_slice_local(
            rt_handle,
            buffer.as_ptr(),
            values.len() as u32,
            element_tydesc,
            list_ptr,
            list_tydesc,
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        // Free allocated memory and element values.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, list_tydesc, 1, list_ptr);
        }
        for value in values {
            destroy_value(ctx, value);
        }
        return Err(InterpError::RuntimeError("Failed to create list".to_string()));
    }

    // Destroy original elements (list cloned them).
    for value in values {
        destroy_value(ctx, value);
    }

    Ok(Value {
        ptr: list_ptr,
        tydesc: list_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a map from a vector of key-value pairs.
///
/// Takes ownership of all key and value values. Keys and values are moved into the
/// map's B-tree structure. All keys must have the same type and all values must have
/// the same type.
pub(super) fn allocate_map_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    entries: Vec<(Value, Value)>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if entries.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty map".to_string()));
    }

    // All keys must have same type, all values must have same type.
    let key_tydesc = entries[0].0.tydesc;
    let value_tydesc = entries[0].1.tydesc;
    let key_size = unsafe { (*key_tydesc).size as usize };
    let value_size = unsafe { (*value_tydesc).size as usize };
    let key_align = unsafe { (*key_tydesc).align };
    let value_align = unsafe { (*value_tydesc).align };

    // Create map tydesc.
    let map_tydesc = ctx.tydesc_table.create_map_from_key_value_tydescs(key_tydesc, value_tydesc);

    // Allocate map structure.
    let rt_handle = ctx.runtime.handle();
    let map_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, map_tydesc, 1)
    };

    if map_ptr.is_null() {
        for (k, v) in entries {
            destroy_value(ctx, k);
            destroy_value(ctx, v);
        }
        return Err(InterpError::RuntimeError("Failed to allocate map".to_string()));
    }

    // Sort entries by key for B-tree construction.
    // For now, use simple comparison based on raw bytes (works for simple numeric types).
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
        unsafe { datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, map_tydesc, 1, map_ptr); }
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
            datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, map_tydesc, 1, map_ptr);
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

    // Build B-tree from sorted slices.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
            rt_handle,
            map_ptr,
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
        unsafe { datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, map_tydesc, 1, map_ptr); }
        return Err(InterpError::RuntimeError("Failed to build map B-tree".to_string()));
    }

    // Free original value containers (data has been moved).
    for (k, v) in sorted_entries {
        free_value_structure(ctx, k);
        free_value_structure(ctx, v);
    }

    Ok(Value {
        ptr: map_ptr,
        tydesc: map_tydesc,
        location: ValueLocation::TempOwned,
    })
}

/// Allocate a set from a vector of evaluated values.
///
/// Takes ownership of all element values. Elements are moved into the
/// set's B-tree structure. All elements must have the same type.
pub(super) fn allocate_set_from_values<'db>(
    ctx: &mut InterpContext<'db>,
    values: Vec<Value>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    if values.is_empty() {
        return Err(InterpError::RuntimeError("Cannot create empty set".to_string()));
    }

    // All elements must have same type.
    let element_tydesc = values[0].tydesc;
    let element_size = unsafe { (*element_tydesc).size as usize };
    let element_align = unsafe { (*element_tydesc).align };

    // Create set tydesc.
    let set_tydesc = ctx.tydesc_table.create_set_from_element_tydesc(element_tydesc);

    // Allocate set structure.
    let rt_handle = ctx.runtime.handle();
    let set_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, set_tydesc, 1)
    };

    if set_ptr.is_null() {
        for v in values {
            destroy_value(ctx, v);
        }
        return Err(InterpError::RuntimeError("Failed to allocate set".to_string()));
    }

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
        unsafe { datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, set_tydesc, 1, set_ptr); }
        return Err(InterpError::RuntimeError("Failed to allocate set buffer".to_string()));
    }

    // Copy elements into buffer.
    for (i, value) in sorted_values.iter().enumerate() {
        unsafe {
            let elem_dest = buffer.add(i * element_size);
            std::ptr::copy_nonoverlapping(value.ptr, elem_dest, element_size);
        }
    }

    // Build B-tree from sorted slice.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
            rt_handle,
            set_ptr,
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
        unsafe { datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, set_tydesc, 1, set_ptr); }
        return Err(InterpError::RuntimeError("Failed to build set B-tree".to_string()));
    }

    // Free original value containers (data has been moved).
    for v in sorted_values {
        free_value_structure(ctx, v);
    }

    Ok(Value {
        ptr: set_ptr,
        tydesc: set_tydesc,
        location: ValueLocation::TempOwned,
    })
}
