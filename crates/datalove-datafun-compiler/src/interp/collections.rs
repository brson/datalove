//! Collection construction and inline evaluation.
//!
//! Contains two groups of functions:
//! - Inline evaluation: `eval_inline_*` for list, set, map, tuple, struct
//! - Value construction: `write_*_from_values_to_dest` for building from Values

use super::{InterpContext, InterpError, Value, Destination, eval_expression_frame};
use super::memory::{destroy_value, free_value_structure};
use super::slots::get_destination_for_expr;
use super::dps::{get_tuple_field_dest, get_struct_field_dest};
use crate::ast;

/// Build a Map at dest from key-value pairs.
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

/// Build a Set at dest from element values.
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

// ============================================================================
// Inline Collection Evaluation
// ============================================================================

/// Evaluate inline list expression with DPS.
///
/// Writes the List struct directly to dest. The list's internal data buffer
/// is still heap-allocated via the runtime allocator.
pub(super) fn eval_inline_list<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    list_expr: &ast::ExprList<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    use crate::tycheck::Type;
    use crate::datalit::tycheck::Type as DatalitType;

    let elements = list_expr.elements(ctx.db);

    // Get element type from typechecker.
    let type_and_heap = ctx.get_expr_type(expr).ok_or_else(|| {
        InterpError::RuntimeError("List type not found in typechecker (compiler bug)".to_string())
    })?;

    let Type::Datalit(DatalitType::List(list_type)) = type_and_heap.ty(ctx.db) else {
        return Err(InterpError::RuntimeError(
            "Expected List type (compiler bug)".to_string()
        ));
    };

    let elem_ty = list_type.element_type(ctx.db);
    let elem_tydesc = ctx.tydesc_table.get_or_create(elem_ty.ty(ctx.db));

    eval_list_with_element_tydesc(ctx, elements, elem_tydesc, dest)
}

/// Evaluate list elements with known element tydesc, using DPS.
///
/// Writes the List struct to dest.ptr. The internal data buffer is heap-allocated.
fn eval_list_with_element_tydesc<'db>(
    ctx: &mut InterpContext<'db>,
    elements: &[ast::ExprFun<'db>],
    element_tydesc: *const datalove_rt::rtdt::TyDesc,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::List;
    use datalove_rt::c::RtStatus;

    let rt_handle = ctx.runtime.handle();
    let list_ptr = dest.ptr;

    // Initialize empty list at dest.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(rt_handle, list_ptr, dest.tydesc)
    };
    if status != RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create list".to_string()));
    }

    if elements.is_empty() {
        return Ok(());
    }

    // Reserve capacity for all elements (allocates data buffer via runtime allocator).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(rt_handle, list_ptr, dest.tydesc, elements.len() as u32)
    };
    if status != RtStatus::Ok {
        unsafe {
            datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, dest.tydesc);
        }
        return Err(InterpError::RuntimeError("Failed to reserve list capacity".to_string()));
    }

    let element_size = unsafe { (*element_tydesc).size as usize };

    // Evaluate each element with DPS into the list buffer.
    for (i, elem) in elements.iter().enumerate() {
        // Get pointer to element slot in list's data buffer.
        let data_ptr = unsafe { (*(list_ptr as *const List)).data as *mut u8 };
        let elem_dest_ptr = unsafe { data_ptr.add(i * element_size) };
        let elem_dest = Destination { ptr: elem_dest_ptr, tydesc: element_tydesc };

        match eval_expression_frame(ctx, *elem, elem_dest) {
            Ok(()) => {
                // Update list size.
                unsafe {
                    let list = list_ptr as *mut List;
                    (*list).size = (i + 1) as u32;
                }
            }
            Err(e) => {
                // Destroy already-written elements and the list.
                // The list's destroy will clean up all elements and the data buffer.
                unsafe {
                    datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, dest.tydesc);
                }
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Evaluate inline set expression with DPS.
pub(super) fn eval_inline_set<'db>(
    ctx: &mut InterpContext<'db>,
    set_expr: &ast::ExprSet<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    let elements = set_expr.elements(ctx.db);
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        let elem_dest = get_destination_for_expr(ctx, *elem)?;
        match eval_expression_frame(ctx, *elem, elem_dest) {
            Ok(()) => values.push(elem_dest.to_value()),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    write_set_from_values_to_dest(ctx, values, dest)
}

/// Evaluate inline map expression with DPS.
pub(super) fn eval_inline_map<'db>(
    ctx: &mut InterpContext<'db>,
    map_expr: &ast::ExprMap<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    let entries = map_expr.entries(ctx.db);
    let mut kv_pairs = Vec::with_capacity(entries.len());

    for entry in entries {
        let key_expr = entry.key(ctx.db);
        let key_dest = get_destination_for_expr(ctx, key_expr)?;
        if let Err(e) = eval_expression_frame(ctx, key_expr, key_dest) {
            for (k, v) in kv_pairs {
                destroy_value(ctx, k);
                destroy_value(ctx, v);
            }
            return Err(e);
        }
        let key = key_dest.to_value();

        let value_expr = entry.value(ctx.db);
        let value_dest = get_destination_for_expr(ctx, value_expr)?;
        if let Err(e) = eval_expression_frame(ctx, value_expr, value_dest) {
            destroy_value(ctx, key);
            for (k, v) in kv_pairs {
                destroy_value(ctx, k);
                destroy_value(ctx, v);
            }
            return Err(e);
        }
        let value = value_dest.to_value();

        kv_pairs.push((key, value));
    }

    write_map_from_values_to_dest(ctx, kv_pairs, dest)
}

/// Evaluate inline anonymous tuple expression with DPS.
pub(super) fn eval_inline_anon_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    tuple_expr: &ast::ExprAnonTuple<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag};

    let elements = tuple_expr.elements(ctx.db);

    // DPS path: if dest is a tuple with matching field count, write directly.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    if dest_tag == TyTag::Tuple {
        let tuple_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let tuple_info = tuple_ref.tuple_info();

        if tuple_info.num_fields() as usize == elements.len() {
            // DPS: evaluate each element directly into its field slot.
            for (i, elem) in elements.iter().enumerate() {
                let field_dest = get_tuple_field_dest(dest, i)
                    .expect("field index should be valid");

                if let Err(e) = eval_expression_frame(ctx, *elem, field_dest) {
                    // Clean up already-written fields.
                    for j in 0..i {
                        let written_field = get_tuple_field_dest(dest, j)
                            .expect("field index should be valid");
                        destroy_value(ctx, written_field.to_value());
                    }
                    return Err(e);
                }
            }

            return Ok(());
        }
    }

    // Fallback was for type mismatch - but dest should always match the expression type.
    unreachable!(
        "eval_inline_anon_tuple: dest type mismatch - expected Tuple with {} fields, got {:?}",
        elements.len(),
        dest_tag
    );
}

/// Evaluate inline anonymous struct expression with DPS.
pub(super) fn eval_inline_anon_struct<'db>(
    ctx: &mut InterpContext<'db>,
    struct_expr: &ast::ExprAnonStruct<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag};

    let expr_fields = struct_expr.fields(ctx.db);

    // Get precomputed field order from frame layout.
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;
    let permutation = layout.get_struct_field_order(ctx.db, *struct_expr)
        .expect("struct field order should be precomputed during analysis");

    // DPS path: if dest is a struct with matching field count, write directly.
    // Both expression fields (via permutation) and dest fields are in canonical sorted order.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    if dest_tag == TyTag::Struct {
        let struct_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let struct_info = struct_ref.struct_info();

        if struct_info.num_fields() as usize == expr_fields.len() {
            // DPS: evaluate each field directly into its slot, using precomputed order.
            for (canonical_idx, &source_idx) in permutation.iter().enumerate() {
                let value_expr = expr_fields[source_idx].value(ctx.db);
                let field_dest = get_struct_field_dest(dest, canonical_idx)
                    .expect("field index should be valid");

                if let Err(e) = eval_expression_frame(ctx, value_expr, field_dest) {
                    // Clean up already-written fields.
                    for j in 0..canonical_idx {
                        let written_field = get_struct_field_dest(dest, j)
                            .expect("field index should be valid");
                        destroy_value(ctx, written_field.to_value());
                    }
                    return Err(e);
                }
            }

            return Ok(());
        }
    }

    // Fallback was for type mismatch - but dest should always match the expression type.
    unreachable!(
        "eval_inline_anon_struct: dest type mismatch - expected Struct with {} fields, got {:?}",
        expr_fields.len(),
        dest_tag
    );
}
