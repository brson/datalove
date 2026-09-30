//! Deep cloning for runtime values.

use crate::{c::{LocalRtHandle, RtStatus}, impls::rt_local};
use datalove_rtdt as rtdt;

/// Clone any type into the local heap.
///
/// Space is already allocated for the proximate type
/// at the `value_out` location - we just need to allocate
/// any needed buffers.
pub unsafe fn clone_value(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc_in: *const rtdt::TyDesc,
    value_out: *mut u8,
) -> RtStatus {
    assert!(!value_in.is_null());
    assert!(!tydesc_in.is_null());
    assert!(!value_out.is_null());

    unsafe {
        let ty = rtdt::TyDescRef::from_ptr(tydesc_in);
        clone_impl(rt, value_in, ty, value_out)
    }
}

/// Internal clone implementation.
unsafe fn clone_impl(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc: rtdt::TyDescRef,
    value_out: *mut u8,
) -> RtStatus {
    use rtdt::TyTag;

    let ty = tydesc;

    match ty.type_tag() {
        // Scalars - just copy bytes.
        TyTag::Bool | TyTag::U8 | TyTag::I8 | TyTag::U16 | TyTag::I16 |
        TyTag::U32 | TyTag::I32 | TyTag::U64 | TyTag::I64 |
        TyTag::Index | TyTag::Offset |
        TyTag::F32 | TyTag::F64 |
        TyTag::Atom => {
            unsafe {
                std::ptr::copy_nonoverlapping(value_in, value_out, ty.size() as usize);
            }
            RtStatus::Ok
        }

        // Bigint - allocate new buffer and copy limbs.
        TyTag::Int => {
            let int_in = unsafe { &*(value_in as *const rtdt::Int) };
            let int_out = unsafe { &mut *(value_out as *mut rtdt::Int) };

            let num_limbs = int_in.size_and_sign.abs() as u32;

            if num_limbs == 0 || int_in.data.is_null() {
                // Zero or empty.
                int_out.data = std::ptr::null();
                int_out.size_and_sign = 0;
                int_out.capacity = rtdt::Index::ZERO;
            } else {
                // Allocate new limb buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                let limb_size = std::mem::size_of::<u32>() as u32;
                let limb_align = std::mem::align_of::<u32>() as u32;
                let new_data = unsafe { rt_ref.alloc.alloc(limb_size, limb_align, num_limbs.into()) as *mut u32 };

                // Copy limbs.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        int_in.data,
                        new_data,
                        num_limbs as usize
                    );
                }

                int_out.data = new_data;
                int_out.size_and_sign = int_in.size_and_sign;
                int_out.capacity = rtdt::Index(num_limbs as rtdt::IndexRepr);
            }

            RtStatus::Ok
        }

        // String - allocate new buffer and copy bytes.
        TyTag::String => {
            let str_in = unsafe { &*(value_in as *const rtdt::String) };
            let str_out = unsafe { &mut *(value_out as *mut rtdt::String) };

            if str_in.size == rtdt::Index::ZERO || str_in.data.is_null() {
                // Empty string.
                str_out.data = std::ptr::null();
                str_out.size = rtdt::Index::ZERO;
                str_out.capacity = rtdt::Index::ZERO;
            } else {
                // Allocate new string buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                let new_data = unsafe { rt_ref.alloc.alloc(1, 1, str_in.size.0) };

                // Copy bytes.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        str_in.data,
                        new_data,
                        str_in.size.as_usize()
                    );
                }

                str_out.data = new_data;
                str_out.size = str_in.size;
                str_out.capacity = str_in.size;
            }

            RtStatus::Ok
        }

        // Tuple - recursively clone each field.
        TyTag::Tuple => {
            for field in ty.iter_tuple_fields() {
                let field_in = unsafe { value_in.add(field.offset() as usize) };
                let field_out = unsafe { value_out.add(field.offset() as usize) };

                let status = unsafe {
                    clone_impl(rt, field_in, field.tydesc(), field_out)
                };

                if status != RtStatus::Ok {
                    return status;
                }
            }

            RtStatus::Ok
        }

        // Struct - recursively clone each field.
        TyTag::Struct => {
            for field in ty.iter_struct_fields() {
                let field_in = unsafe { value_in.add(field.offset() as usize) };
                let field_out = unsafe { value_out.add(field.offset() as usize) };

                let status = unsafe {
                    clone_impl(rt, field_in, field.tydesc(), field_out)
                };

                if status != RtStatus::Ok {
                    return status;
                }
            }

            RtStatus::Ok
        }

        // Term - clone the payload (same layout).
        TyTag::Term => {
            let (_, payload_ty) = ty.term_info();
            unsafe { clone_impl(rt, value_in, payload_ty, value_out) }
        }

        // Enum - copy discriminant and clone payload if present.
        TyTag::Enum => {
            let enum_info = ty.enum_info();
            let discriminant = unsafe { *(value_in as *const u32) };

            // Copy discriminant.
            unsafe {
                *(value_out as *mut u32) = discriminant;
            }

            // Find variant and clone payload if it exists.
            if (discriminant as usize) < enum_info.num_variants() as usize {
                if let core::option::Option::Some(variant) = enum_info.variant(discriminant as usize) {
                    if let core::option::Option::Some(payload_ty) = variant.payload() {
                        let payload_in = unsafe { value_in.add(variant.offset() as usize) };
                        let payload_out = unsafe { value_out.add(variant.offset() as usize) };

                        return unsafe {
                            clone_impl(rt, payload_in, payload_ty, payload_out)
                        };
                    }
                }
            }

            RtStatus::Ok
        }

        // List - allocate new buffer and recursively clone each element.
        TyTag::List => {
            let list_in = unsafe { &*(value_in as *const rtdt::List) };
            let list_out = unsafe { &mut *(value_out as *mut rtdt::List) };

            let elem_ty = ty.list_element_ty();
            let elem_size = elem_ty.size();

            if list_in.size == rtdt::Index::ZERO || list_in.data.is_null() {
                // Empty list.
                list_out.data = std::ptr::null();
                list_out.size = rtdt::Index::ZERO;
                list_out.capacity = rtdt::Index::ZERO;
            } else {
                // Allocate new list buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                let elem_align = elem_ty.align();
                let new_data = unsafe { rt_ref.alloc.alloc(elem_size, elem_align, list_in.size.0) };

                // Clone each element.
                for i in 0..list_in.size.0 {
                    let offset = (i as usize) * (elem_size as usize);
                    let elem_in = unsafe { list_in.data.add(offset) };
                    let elem_out = unsafe { new_data.add(offset) };

                    let status = unsafe {
                        clone_impl(rt, elem_in, elem_ty, elem_out)
                    };

                    if status != RtStatus::Ok {
                        // Clone failed. Destroy successfully cloned elements and free buffer.
                        unsafe {
                            for j in 0..i {
                                let cleanup_offset = (j as usize) * (elem_size as usize);
                                let elem_to_destroy = new_data.add(cleanup_offset);
                                let _ = crate::impls::destroy::any_destroy_local(rt, elem_to_destroy, elem_ty.as_ptr());
                            }
                            rt_ref.alloc.free(elem_size, elem_align, list_in.size.0, new_data);
                        }
                        return status;
                    }
                }

                list_out.data = new_data;
                list_out.size = list_in.size;
                list_out.capacity = list_in.size;
            }

            RtStatus::Ok
        }

        // Map - recursively clone the tree structure.
        TyTag::Map => {
            let map_in = unsafe { &*(value_in as *const rtdt::Map) };
            let map_out = unsafe { &mut *(value_out as *mut rtdt::Map) };

            if map_in.root.is_null() {
                map_out.root = std::ptr::null();
                map_out.len = rtdt::Index::ZERO;
            } else {
                let key_ty = ty.map_key_ty();
                let value_ty = ty.map_value_ty();
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };

                let new_root = unsafe {
                    crate::impls::btreemap::btreemap_clone_tree(
                        rt_ref,
                        map_in.root,
                        key_ty,
                        value_ty,
                    )
                };

                if new_root.is_null() {
                    return RtStatus::Error;
                }

                map_out.root = new_root;
                map_out.len = map_in.len;
            }

            RtStatus::Ok
        }

        // Set - recursively clone the tree structure.
        TyTag::Set => {
            let set_in = unsafe { &*(value_in as *const rtdt::Set) };
            let set_out = unsafe { &mut *(value_out as *mut rtdt::Set) };

            if set_in.root.is_null() {
                set_out.root = std::ptr::null();
                set_out.len = rtdt::Index::ZERO;
            } else {
                let element_ty = ty.set_element_ty();
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };

                let new_root = unsafe {
                    crate::impls::set::set_clone_tree(
                        rt_ref,
                        set_in.root,
                        element_ty.as_ptr(),
                    )
                };

                if new_root.is_null() {
                    return RtStatus::Error;
                }

                set_out.root = new_root;
                set_out.len = set_in.len;
            }

            RtStatus::Ok
        }

        // Tensor - allocate new buffers and clone each element.
        TyTag::Tensor => {
            let tensor_in = unsafe { &*(value_in as *const rtdt::Tensor) };
            let tensor_out = unsafe { &mut *(value_out as *mut rtdt::Tensor) };

            let element_ty = ty.tensor_element_ty();
            let rank = ty.tensor_rank();
            let elem_size = element_ty.size();
            let elem_align = element_ty.align();

            // No data means no elements, and the shape, which says how many
            // of each there are none of, comes along all the same.
            if tensor_in.ptr_base.is_null() {
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                tensor_out.ptr_base = std::ptr::null_mut();
                tensor_out.offset_elems = rtdt::Index::ZERO;
                tensor_out.capacity_elems = rtdt::Index::ZERO;
                tensor_out.shape = unsafe { copy_index_array(rt_ref, tensor_in.shape, rank) };
                tensor_out.strides = unsafe { copy_index_array(rt_ref, tensor_in.strides, rank) };
                tensor_out.layout = tensor_in.layout;
            } else if tensor_in.capacity_elems == rtdt::Index::ZERO {
                // View tensor (capacity_elems == 0, ptr_base non-null).
                // Clone into a new owned contiguous tensor using strided iteration.
                let status = unsafe {
                    clone_view_tensor(rt, tensor_in, tensor_out, element_ty, rank, elem_size, elem_align)
                };
                if status != RtStatus::Ok {
                    return status;
                }
            } else {
                // Owned tensor: clone contiguous data buffer.
                let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };

                // Allocate new data buffer.
                let new_data = unsafe {
                    rt_ref.alloc.alloc(elem_size, elem_align, tensor_in.capacity_elems.0)
                };

                // Clone each element in the capacity buffer.
                for i in 0..tensor_in.capacity_elems.0 {
                    let offset = (i as usize) * (elem_size as usize);
                    let elem_in = unsafe { tensor_in.ptr_base.add(offset) };
                    let elem_out = unsafe { new_data.add(offset) };

                    let status = unsafe {
                        clone_impl(rt, elem_in, element_ty, elem_out)
                    };

                    if status != RtStatus::Ok {
                        return status;
                    }
                }

                let new_shape = unsafe { copy_index_array(rt_ref, tensor_in.shape, rank) };
                let new_strides = unsafe { copy_index_array(rt_ref, tensor_in.strides, rank) };

                // Copy metadata.
                tensor_out.ptr_base = new_data;
                tensor_out.offset_elems = tensor_in.offset_elems;
                tensor_out.capacity_elems = tensor_in.capacity_elems;
                tensor_out.shape = new_shape;
                tensor_out.strides = new_strides;
                tensor_out.layout = tensor_in.layout;
            }

            RtStatus::Ok
        }

        // Table - clone columnar data.
        TyTag::Table => {
            let table_in = unsafe { &*(value_in as *const rtdt::Table) };
            let table_out = unsafe { &mut *(value_out as *mut rtdt::Table) };

            if table_in.len == rtdt::Index::ZERO || table_in.data.is_null() {
                table_out.len = rtdt::Index::ZERO;
                table_out.capacity = rtdt::Index::ZERO;
                table_out.data = std::ptr::null();
                return RtStatus::Ok;
            }

            let column_tydescs = crate::impls::table::collect_column_tydescs(ty);
            let alloc_size = rtdt::layout::table_data_allocation_size(&column_tydescs, table_in.len.0);
            let alloc_align = rtdt::layout::table_data_alignment(&column_tydescs);

            let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
            let new_data = unsafe { rt_ref.alloc.alloc(alloc_size, alloc_align, 1) };
            if new_data.is_null() {
                return RtStatus::Error;
            }

            // Clone column by column, row by row.
            for (col, col_info) in ty.table_column_tydescs().enumerate() {
                for row in 0..table_in.len.0 {
                    let src = unsafe {
                        crate::impls::table::element_ptr(
                            table_in.data,
                            &column_tydescs,
                            row,
                            col,
                            table_in.capacity.0,
                        )
                    };
                    let dst = unsafe {
                        crate::impls::table::element_ptr_mut(
                            new_data,
                            &column_tydescs,
                            row,
                            col,
                            table_in.len.0,
                        )
                    };
                    let status = unsafe { clone_impl(rt, src, col_info.tydesc(), dst) };
                    if status != RtStatus::Ok {
                        // Cleanup partial clone on failure.
                        for cleanup_col in 0..=col {
                            let cleanup_end_row = if cleanup_col == col { row } else { table_in.len.0 };
                            for cleanup_row in 0..cleanup_end_row {
                                let cleanup_ptr = unsafe {
                                    crate::impls::table::element_ptr_mut(
                                        new_data,
                                        &column_tydescs,
                                        cleanup_row,
                                        cleanup_col,
                                        table_in.len.0,
                                    )
                                };
                                let cleanup_col_info = ty.table_column_tydescs().nth(cleanup_col).unwrap();
                                unsafe {
                                    let _ = crate::impls::destroy::any_destroy_local(
                                        rt,
                                        cleanup_ptr,
                                        cleanup_col_info.tydesc().as_ptr(),
                                    );
                                }
                            }
                        }
                        let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                        unsafe { rt_ref.alloc.free(alloc_size, alloc_align, 1, new_data) };
                        return status;
                    }
                }
            }

            table_out.len = table_in.len;
            table_out.capacity = table_in.len;
            table_out.data = new_data;
            RtStatus::Ok
        }

        // Option - copy tag and clone payload if Some.
        TyTag::Option => {
            let opt_in = unsafe { &*(value_in as *const rtdt::Option) };
            let opt_out = unsafe { &mut *(value_out as *mut rtdt::Option) };

            // Copy tag.
            opt_out.tag = opt_in.tag;

            if opt_in.tag == rtdt::OptionTag::Some {
                let inner_ty = ty.option_inner_ty();
                let payload_offset = rtdt::layout::option_payload_offset(inner_ty.align());

                let payload_in = unsafe { value_in.add(payload_offset as usize) };
                let payload_out = unsafe { value_out.add(payload_offset as usize) };

                return unsafe {
                    clone_impl(rt, payload_in, inner_ty, payload_out)
                };
            }

            RtStatus::Ok
        }

        // Result - copy tag and clone payload.
        TyTag::Result => {
            let res_in = unsafe { &*(value_in as *const rtdt::Result) };
            let res_out = unsafe { &mut *(value_out as *mut rtdt::Result) };

            // Copy tag.
            res_out.tag = res_in.tag;

            let ok_ty = ty.result_ok_ty();
            let payload_offset = rtdt::layout::result_payload_offset(ok_ty.align());

            // For now, we only handle Ok case.
            // Error type cloning would need the error tydesc.
            if res_in.tag == rtdt::ResultTag::Ok {
                let payload_in = unsafe { value_in.add(payload_offset as usize) };
                let payload_out = unsafe { value_out.add(payload_offset as usize) };

                return unsafe {
                    clone_impl(rt, payload_in, ok_ty, payload_out)
                };
            }

            // For Err, deep clone the Error value.
            // Error uses same encoding as Data, so interpret as Data.
            let error_in_ptr = unsafe { value_in.add(payload_offset as usize) };
            let error_out_ptr = unsafe { value_out.add(payload_offset as usize) };

            let data_in = unsafe { &*(error_in_ptr as *const rtdt::Data) };
            let data_out = unsafe { &mut *(error_out_ptr as *mut rtdt::Data) };

            match data_in.tag() {
                rtdt::anypack::Tag::TwoPointers => {
                    // Deep clone: allocate new memory and recursively clone inner value.
                    let inner_tydesc = data_in.tydesc();
                    let inner_value_in = data_in.value_ptr();

                    if inner_tydesc.is_null() || inner_value_in.is_null() {
                        return RtStatus::Error;
                    }

                    let inner_ty = unsafe { rtdt::TyDescRef::from_ptr(inner_tydesc) };
                    let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                    let inner_value_out = unsafe { rt_ref.alloc.alloc(inner_ty.size(), inner_ty.align(), 1) };

                    if inner_value_out.is_null() {
                        return RtStatus::Error;
                    }

                    // Recursively clone the inner value.
                    let status = unsafe { clone_impl(rt, inner_value_in, inner_ty, inner_value_out) };
                    if status != RtStatus::Ok {
                        unsafe { rt_ref.alloc.free(inner_ty.size(), inner_ty.align(), 1, inner_value_out) };
                        return status;
                    }

                    // Write the new Error with cloned pointers (write as Data).
                    unsafe {
                        std::ptr::write(data_out, rtdt::Data::from_pointers(inner_tydesc, inner_value_out));
                    }

                    RtStatus::Ok
                }
                rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                    // Value is inline or immediate, just copy the bytes.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            error_in_ptr,
                            error_out_ptr,
                            std::mem::size_of::<rtdt::Error>()
                        );
                    }
                    RtStatus::Ok
                }
                _ => {
                    // Unknown/reserved tag.
                    RtStatus::Error
                }
            }
        }

        // Data - deep clone the wrapped value.
        TyTag::Data => {
            let data_in = unsafe { &*(value_in as *const rtdt::Data) };
            let data_out = unsafe { &mut *(value_out as *mut rtdt::Data) };

            match data_in.tag() {
                rtdt::anypack::Tag::TwoPointers => {
                    // Deep clone: allocate new memory and recursively clone inner value.
                    let inner_tydesc = data_in.tydesc();
                    let inner_value_in = data_in.value_ptr();

                    if inner_tydesc.is_null() || inner_value_in.is_null() {
                        return RtStatus::Error;
                    }

                    let inner_ty = unsafe { rtdt::TyDescRef::from_ptr(inner_tydesc) };
                    let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                    let inner_value_out = unsafe { rt_ref.alloc.alloc(inner_ty.size(), inner_ty.align(), 1) };

                    if inner_value_out.is_null() {
                        return RtStatus::Error;
                    }

                    // Recursively clone the inner value.
                    let status = unsafe { clone_impl(rt, inner_value_in, inner_ty, inner_value_out) };
                    if status != RtStatus::Ok {
                        unsafe { rt_ref.alloc.free(inner_ty.size(), inner_ty.align(), 1, inner_value_out) };
                        return status;
                    }

                    // Write the new Data with cloned pointers.
                    unsafe {
                        std::ptr::write(data_out, rtdt::Data::from_pointers(inner_tydesc, inner_value_out));
                    }

                    RtStatus::Ok
                }
                rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                    // Value is inline or immediate, just copy the bytes.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            value_in,
                            value_out,
                            std::mem::size_of::<rtdt::Data>()
                        );
                    }
                    RtStatus::Ok
                }
                _ => {
                    // Unknown/reserved tag.
                    RtStatus::Error
                }
            }
        }

        // Error - deep clone the wrapped value.
        // Error uses same encoding as Data, so interpret as Data.
        TyTag::Error => {
            let data_in = unsafe { &*(value_in as *const rtdt::Data) };
            let data_out = unsafe { &mut *(value_out as *mut rtdt::Data) };

            match data_in.tag() {
                rtdt::anypack::Tag::TwoPointers => {
                    // Deep clone: allocate new memory and recursively clone inner value.
                    let inner_tydesc = data_in.tydesc();
                    let inner_value_in = data_in.value_ptr();

                    if inner_tydesc.is_null() || inner_value_in.is_null() {
                        return RtStatus::Error;
                    }

                    let inner_ty = unsafe { rtdt::TyDescRef::from_ptr(inner_tydesc) };
                    let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };
                    let inner_value_out = unsafe { rt_ref.alloc.alloc(inner_ty.size(), inner_ty.align(), 1) };

                    if inner_value_out.is_null() {
                        return RtStatus::Error;
                    }

                    // Recursively clone the inner value.
                    let status = unsafe { clone_impl(rt, inner_value_in, inner_ty, inner_value_out) };
                    if status != RtStatus::Ok {
                        unsafe { rt_ref.alloc.free(inner_ty.size(), inner_ty.align(), 1, inner_value_out) };
                        return status;
                    }

                    // Write the new Error with cloned pointers (write as Data).
                    unsafe {
                        std::ptr::write(data_out, rtdt::Data::from_pointers(inner_tydesc, inner_value_out));
                    }

                    RtStatus::Ok
                }
                rtdt::anypack::Tag::SmallImmediate | rtdt::anypack::Tag::InlineWithTyDesc => {
                    // Value is inline or immediate, just copy the bytes.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            value_in,
                            value_out,
                            std::mem::size_of::<rtdt::Error>()
                        );
                    }
                    RtStatus::Ok
                }
                _ => {
                    // Unknown/reserved tag.
                    RtStatus::Error
                }
            }
        }
    }
}

/// Clones a view tensor into a new owned contiguous tensor.
///
/// Views have `capacity_elems == 0` and share `ptr_base` with the parent.
/// This function computes total elements from the shape, allocates fresh
/// buffers, and clones elements by walking the source strides.
unsafe fn clone_view_tensor(
    rt: LocalRtHandle,
    tensor_in: &rtdt::Tensor,
    tensor_out: &mut rtdt::Tensor,
    element_ty: rtdt::TyDescRef,
    rank: u32,
    elem_size: u32,
    elem_align: u32,
) -> RtStatus {
    assert!(!tensor_in.ptr_base.is_null());
    assert!(!tensor_in.shape.is_null());
    assert!(!tensor_in.strides.is_null());

    let rt_ref = unsafe { &mut *(rt as *mut rt_local::RtLocal) };

    // Compute total elements from shape.
    let mut total_elems: rtdt::IndexRepr = 1;
    for i in 0..rank as usize {
        total_elems = total_elems.saturating_mul(unsafe { (*tensor_in.shape.add(i)).0 });
    }

    if total_elems == 0 {
        // Nothing to copy, and no element for the strides ever to reach.
        tensor_out.ptr_base = std::ptr::null_mut();
        tensor_out.offset_elems = rtdt::Index::ZERO;
        tensor_out.capacity_elems = rtdt::Index::ZERO;
        tensor_out.shape = unsafe { copy_index_array(rt_ref, tensor_in.shape, rank) };
        tensor_out.strides = unsafe { copy_index_array(rt_ref, tensor_in.strides, rank) };
        tensor_out.layout = tensor_in.layout;
        return RtStatus::Ok;
    }

    // Allocate data buffer.
    let new_data = unsafe { rt_ref.alloc.alloc(elem_size, elem_align, total_elems) };
    if new_data.is_null() {
        return RtStatus::Error;
    }

    // Allocate shape array.
    let new_shape = unsafe {
        rt_ref.alloc.alloc(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into())
    } as *mut rtdt::Index;
    if new_shape.is_null() {
        unsafe { rt_ref.alloc.free(elem_size, elem_align, total_elems, new_data) };
        return RtStatus::Error;
    }

    // Allocate strides array.
    let new_strides = unsafe {
        rt_ref.alloc.alloc(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into())
    } as *mut rtdt::Index;
    if new_strides.is_null() {
        unsafe {
            rt_ref.alloc.free(elem_size, elem_align, total_elems, new_data);
            rt_ref.alloc.free(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into(), new_shape as *mut u8);
        }
        return RtStatus::Error;
    }

    // Copy shape and compute row-major strides.
    unsafe {
        for i in 0..rank as usize {
            *new_shape.add(i) = *tensor_in.shape.add(i);
        }
        for i in 0..rank as usize {
            let mut stride: rtdt::IndexRepr = 1;
            for j in (i + 1)..rank as usize {
                stride = stride.saturating_mul((*new_shape.add(j)).0);
            }
            *new_strides.add(i) = rtdt::Index(stride);
        }
    }

    // Clone elements by walking source strides.
    let src_shape: Vec<rtdt::IndexRepr> = (0..rank as usize)
        .map(|i| unsafe { (*tensor_in.shape.add(i)).0 })
        .collect();
    let src_strides: Vec<rtdt::IndexRepr> = (0..rank as usize)
        .map(|i| unsafe { (*tensor_in.strides.add(i)).0 })
        .collect();

    let elem_size_usize = elem_size as usize;
    let base_offset = tensor_in.offset_elems.0;
    let mut dest_idx: rtdt::IndexRepr = 0;
    let mut indices: Vec<rtdt::IndexRepr> = vec![0; rank as usize];

    loop {
        // Compute source linear offset for current multi-index.
        let mut src_offset = base_offset;
        for d in 0..rank as usize {
            src_offset = src_offset.saturating_add(indices[d].saturating_mul(src_strides[d]));
        }

        let src_ptr = unsafe { tensor_in.ptr_base.add(src_offset as usize * elem_size_usize) };
        let dst_ptr = unsafe { new_data.add(dest_idx as usize * elem_size_usize) };

        let status = unsafe { clone_impl(rt, src_ptr, element_ty, dst_ptr) };
        if status != RtStatus::Ok {
            // Clean up partially cloned elements.
            for j in 0..dest_idx {
                let elem_ptr = unsafe { new_data.add(j as usize * elem_size_usize) };
                unsafe {
                    let _ = crate::impls::destroy::any_destroy_local(rt, elem_ptr, element_ty.as_ptr());
                }
            }
            unsafe {
                rt_ref.alloc.free(elem_size, elem_align, total_elems, new_data);
                rt_ref.alloc.free(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into(), new_shape as *mut u8);
                rt_ref.alloc.free(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into(), new_strides as *mut u8);
            }
            return status;
        }

        dest_idx += 1;

        // Advance multi-index (row-major order).
        let mut carry = true;
        for d in (0..rank as usize).rev() {
            if carry {
                indices[d] += 1;
                if indices[d] < src_shape[d] {
                    carry = false;
                } else {
                    indices[d] = 0;
                }
            }
        }
        if carry {
            break;
        }
    }

    tensor_out.ptr_base = new_data;
    tensor_out.offset_elems = rtdt::Index::ZERO;
    tensor_out.capacity_elems = rtdt::Index(total_elems);
    tensor_out.shape = new_shape;
    tensor_out.strides = new_strides;
    tensor_out.layout = rtdt::TensorLayout::RowMajor;

    RtStatus::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmx::proptest::prelude::*;

    /// Helper to create a test runtime.
    fn make_rt() -> Box<rt_local::RtLocal> {
        rt_local::RtLocal::new()
    }

    /// Helper to create a simple scalar tydesc.
    fn make_u32_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::U32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    fn make_f32_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::F32,
            size: 4,
            align: 4,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    fn make_bool_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Bool,
            size: 1,
            align: 1,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    fn make_int_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::Int,
            size: std::mem::size_of::<rtdt::Int>() as u32,
            align: std::mem::align_of::<rtdt::Int>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    fn make_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    #[test]
    fn test_clone_u32_zero() {
        let mut rt = make_rt();
        let tydesc = make_u32_tydesc();

        let value_in: u32 = 0;
        let mut value_out: u32 = 0xDEADBEEF;

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert_eq!(value_out, 0);
    }

    proptest! {
        #[test]
        fn prop_clone_u32(value in any::<u32>()) {
            let mut rt = make_rt();
            let tydesc = make_u32_tydesc();

            let mut value_out: u32 = 0;

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out, value);
        }

        #[test]
        fn prop_clone_f32(value in any::<f32>()) {
            let mut rt = make_rt();
            let tydesc = make_f32_tydesc();

            let mut value_out: f32 = 0.0;

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            if value.is_nan() {
                prop_assert!(value_out.is_nan());
            } else {
                prop_assert_eq!(value_out, value);
            }
        }

        #[test]
        fn prop_clone_bool(value in any::<bool>()) {
            let mut rt = make_rt();
            let tydesc = make_bool_tydesc();

            let value_in: u8 = value as u8;
            let mut value_out: u8 = 0;

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out, value_in);
        }
    }

    #[test]
    fn test_clone_empty_int() {
        let mut rt = make_rt();
        let tydesc = make_int_tydesc();

        let value_in = rtdt::Int {
            data: std::ptr::null(),
            size_and_sign: 0,
            capacity: rtdt::Index::ZERO,
        };

        let mut value_out = rtdt::Int {
            data: std::ptr::null_mut(),
            size_and_sign: 999,
            capacity: rtdt::Index(999),
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.data.is_null());
        assert_eq!(value_out.size_and_sign, 0);
        assert_eq!(value_out.capacity, rtdt::Index::ZERO);
    }

    proptest! {
        #[test]
        fn prop_clone_int(
            limbs in prop::collection::vec(any::<u32>(), 0..10),
            is_negative in any::<bool>()
        ) {
            let mut rt = make_rt();
            let tydesc = make_int_tydesc();

            // Allocate limbs.
            let limb_data = if limbs.is_empty() {
                std::ptr::null()
            } else {
                unsafe {
                    let ptr = rt.alloc.alloc(4, 4, (limbs.len() as u32).into()) as *mut u32;
                    for (i, &limb) in limbs.iter().enumerate() {
                        *ptr.add(i) = limb;
                    }
                    ptr as *const u32
                }
            };

            let size = limbs.len() as i32;
            let value_in = rtdt::Int {
                data: limb_data,
                size_and_sign: if is_negative { -size } else { size },
                capacity: rtdt::Index(limbs.len() as rtdt::IndexRepr),
            };

            let mut value_out = rtdt::Int {
                data: std::ptr::null(),
                size_and_sign: 0,
                capacity: rtdt::Index::ZERO,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.size_and_sign, value_in.size_and_sign);

            if !limbs.is_empty() {
                // Verify data was copied and pointers are different.
                prop_assert_ne!(value_out.data, value_in.data);

                // Verify limb values are equal.
                for i in 0..limbs.len() {
                    let in_limb = unsafe { *value_in.data.add(i) };
                    let out_limb = unsafe { *value_out.data.add(i) };
                    prop_assert_eq!(in_limb, out_limb);
                }

                // Cleanup.
                unsafe {
                    rt.alloc.free(4, 4, (limbs.len() as u32).into(), value_out.data as *mut u8);
                }
            }

            // Cleanup input.
            if !limbs.is_empty() {
                unsafe {
                    rt.alloc.free(4, 4, (limbs.len() as u32).into(), limb_data as *mut u8);
                }
            }
        }
    }

    #[test]
    fn test_clone_empty_string() {
        let mut rt = make_rt();
        let tydesc = make_string_tydesc();

        let value_in = rtdt::String {
            data: std::ptr::null(),
            size: rtdt::Index::ZERO,
            capacity: rtdt::Index::ZERO,
        };

        let mut value_out = rtdt::String {
            data: std::ptr::null(),
            size: rtdt::Index(999),
            capacity: rtdt::Index(999),
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.data.is_null());
        assert_eq!(value_out.size, rtdt::Index::ZERO);
        assert_eq!(value_out.capacity, rtdt::Index::ZERO);
    }

    proptest! {
        #[test]
        fn prop_clone_string(bytes in prop::collection::vec(any::<u8>(), 0..100)) {
            let mut rt = make_rt();
            let tydesc = make_string_tydesc();

            // Allocate string data.
            let str_data = if bytes.is_empty() {
                std::ptr::null()
            } else {
                unsafe {
                    let ptr = rt.alloc.alloc(1, 1, (bytes.len() as u32).into());
                    for (i, &byte) in bytes.iter().enumerate() {
                        *ptr.add(i) = byte;
                    }
                    ptr as *const u8
                }
            };

            let value_in = rtdt::String {
                data: str_data,
                size: rtdt::Index(bytes.len() as rtdt::IndexRepr),
                capacity: rtdt::Index(bytes.len() as rtdt::IndexRepr),
            };

            let mut value_out = rtdt::String {
                data: std::ptr::null(),
                size: rtdt::Index::ZERO,
                capacity: rtdt::Index::ZERO,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.size, value_in.size);

            if !bytes.is_empty() {
                // Verify data was copied and pointers are different.
                prop_assert_ne!(value_out.data, value_in.data);

                // Verify byte values are equal.
                for i in 0..bytes.len() {
                    let in_byte = unsafe { *value_in.data.add(i) };
                    let out_byte = unsafe { *value_out.data.add(i) };
                    prop_assert_eq!(in_byte, out_byte);
                }

                // Cleanup.
                unsafe {
                    rt.alloc.free(1, 1, (bytes.len() as u32).into(), value_out.data as *mut u8);
                }
            }

            // Cleanup input.
            if !bytes.is_empty() {
                unsafe {
                    rt.alloc.free(1, 1, (bytes.len() as u32).into(), str_data as *mut u8);
                }
            }
        }
    }

    #[test]
    fn test_clone_tuple_u32_u32() {
        let mut rt = make_rt();

        // Build type descriptor for (u32, u32).
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let fields = Box::new([
            rtdt::TyInfoTupleField {
                offset: 0,
                tydesc: u32_tydesc_ptr,
            },
            rtdt::TyInfoTupleField {
                offset: 4,
                tydesc: u32_tydesc_ptr,
            },
        ]);

        let fields_ptr = fields.as_ptr();

        let tuple_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Tuple,
            size: 8,
            align: 4,
            type_info: rtdt::TyInfo {
                tuple: rtdt::TyInfoTuple {
                    num_fields: 2,
                    fields: fields_ptr,
                },
            },
        };

        #[repr(C)]
        struct Tuple {
            a: u32,
            b: u32,
        }

        let value_in = Tuple { a: 42, b: 100 };
        let mut value_out = Tuple { a: 0, b: 0 };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &tuple_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert_eq!(value_out.a, 42);
        assert_eq!(value_out.b, 100);
    }

    #[test]
    fn test_clone_empty_list() {
        let mut rt = make_rt();

        // Build type descriptor for [u32].
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let list_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::List,
            size: std::mem::size_of::<rtdt::List>() as u32,
            align: std::mem::align_of::<rtdt::List>() as u32,
            type_info: rtdt::TyInfo {
                list: rtdt::TyInfoList {
                    element_tydesc: u32_tydesc_ptr,
                },
            },
        };

        let value_in = rtdt::List {
            data: std::ptr::null(),
            size: rtdt::Index::ZERO,
            capacity: rtdt::Index::ZERO,
        };

        let mut value_out = rtdt::List {
            data: std::ptr::null(),
            size: rtdt::Index(999),
            capacity: rtdt::Index(999),
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &list_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.data.is_null());
        assert_eq!(value_out.size, rtdt::Index::ZERO);
        assert_eq!(value_out.capacity, rtdt::Index::ZERO);
    }

    proptest! {
        #[test]
        fn prop_clone_list_u32(elements in prop::collection::vec(any::<u32>(), 0..20)) {
            let mut rt = make_rt();

            // Build type descriptor for [u32].
            let u32_tydesc = Box::new(make_u32_tydesc());
            let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

            let list_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::List,
                size: std::mem::size_of::<rtdt::List>() as u32,
                align: std::mem::align_of::<rtdt::List>() as u32,
                type_info: rtdt::TyInfo {
                    list: rtdt::TyInfoList {
                        element_tydesc: u32_tydesc_ptr,
                    },
                },
            };

            // Allocate list data.
            let list_data = if elements.is_empty() {
                std::ptr::null()
            } else {
                unsafe {
                    let ptr = rt.alloc.alloc(4, 4, (elements.len() as u32).into()) as *mut u32;
                    for (i, &elem) in elements.iter().enumerate() {
                        *ptr.add(i) = elem;
                    }
                    ptr as *const u8
                }
            };

            let value_in = rtdt::List {
                data: list_data,
                size: rtdt::Index(elements.len() as rtdt::IndexRepr),
                capacity: rtdt::Index(elements.len() as rtdt::IndexRepr),
            };

            let mut value_out = rtdt::List {
                data: std::ptr::null(),
                size: rtdt::Index::ZERO,
                capacity: rtdt::Index::ZERO,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &list_tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.size, value_in.size);

            if !elements.is_empty() {
                // Verify data was copied and pointers are different.
                prop_assert_ne!(value_out.data, value_in.data);

                // Verify element values are equal.
                for i in 0..elements.len() {
                    let in_elem = unsafe { *(value_in.data as *const u32).add(i) };
                    let out_elem = unsafe { *(value_out.data as *const u32).add(i) };
                    prop_assert_eq!(in_elem, out_elem);
                }

                // Cleanup.
                unsafe {
                    rt.alloc.free(4, 4, (elements.len() as u32).into(), value_out.data as *mut u8);
                }
            }

            // Cleanup input.
            if !elements.is_empty() {
                unsafe {
                    rt.alloc.free(4, 4, (elements.len() as u32).into(), list_data as *mut u8);
                }
            }
        }
    }

    #[test]
    fn test_clone_option_none() {
        let mut rt = make_rt();

        // Build type descriptor for ?u32.
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let option_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Option,
            size: 8,
            align: 4,
            type_info: rtdt::TyInfo {
                option: rtdt::TyInfoOption {
                    inner_tydesc: u32_tydesc_ptr,
                },
            },
        };

        #[repr(C)]
        struct OptionU32 {
            tag: rtdt::OptionTag,
            _pad: [u8; 3],
            value: u32,
        }

        let value_in = OptionU32 {
            tag: rtdt::OptionTag::None,
            _pad: [0; 3],
            value: 0,
        };

        let mut value_out = OptionU32 {
            tag: rtdt::OptionTag::Some,
            _pad: [0xFF; 3],
            value: 0xDEADBEEF,
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &option_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert_eq!(value_out.tag, rtdt::OptionTag::None);
    }

    proptest! {
        #[test]
        fn prop_clone_option_some(value in any::<u32>()) {
            let mut rt = make_rt();

            // Build type descriptor for ?u32.
            let u32_tydesc = Box::new(make_u32_tydesc());
            let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

            let option_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Option,
                size: 8,
                align: 4,
                type_info: rtdt::TyInfo {
                    option: rtdt::TyInfoOption {
                        inner_tydesc: u32_tydesc_ptr,
                    },
                },
            };

            #[repr(C)]
            struct OptionU32 {
                tag: rtdt::OptionTag,
                _pad: [u8; 3],
                value: u32,
            }

            let value_in = OptionU32 {
                tag: rtdt::OptionTag::Some,
                _pad: [0; 3],
                value,
            };

            let mut value_out = OptionU32 {
                tag: rtdt::OptionTag::None,
                _pad: [0; 3],
                value: 0,
            };

            let status = unsafe {
                clone_value(
                    Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                    &value_in as *const _ as *const u8,
                    &option_tydesc,
                    &mut value_out as *mut _ as *mut u8,
                )
            };

            prop_assert_eq!(status, RtStatus::Ok);
            prop_assert_eq!(value_out.tag, rtdt::OptionTag::Some);
            prop_assert_eq!(value_out.value, value);
        }
    }

    #[test]
    fn test_clone_empty_map() {
        let mut rt = make_rt();

        // Build type descriptor for {u32: u32}.
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let map_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Map,
            size: std::mem::size_of::<rtdt::Map>() as u32,
            align: std::mem::align_of::<rtdt::Map>() as u32,
            type_info: rtdt::TyInfo {
                map: rtdt::TyInfoMap {
                    key_tydesc: u32_tydesc_ptr,
                    value_tydesc: u32_tydesc_ptr,
                },
            },
        };

        let value_in = rtdt::Map {
            root: std::ptr::null(),
            len: rtdt::Index::ZERO,
        };

        let mut value_out = rtdt::Map {
            root: std::ptr::null(),
            len: rtdt::Index(999),
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &map_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.root.is_null());
        assert_eq!(value_out.len, rtdt::Index::ZERO);
    }

    #[test]
    fn test_clone_empty_set() {
        let mut rt = make_rt();

        // Build type descriptor for {u32}.
        let u32_tydesc = Box::new(make_u32_tydesc());
        let u32_tydesc_ptr = Box::as_ref(&u32_tydesc) as *const rtdt::TyDesc;

        let set_tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::Set,
            size: std::mem::size_of::<rtdt::Set>() as u32,
            align: std::mem::align_of::<rtdt::Set>() as u32,
            type_info: rtdt::TyInfo {
                set: rtdt::TyInfoSet {
                    element_tydesc: u32_tydesc_ptr,
                },
            },
        };

        let value_in = rtdt::Set {
            root: std::ptr::null(),
            len: rtdt::Index::ZERO,
        };

        let mut value_out = rtdt::Set {
            root: std::ptr::null(),
            len: rtdt::Index(999),
        };

        let status = unsafe {
            clone_value(
                Box::as_mut(&mut rt) as *mut _ as LocalRtHandle,
                &value_in as *const _ as *const u8,
                &set_tydesc,
                &mut value_out as *mut _ as *mut u8,
            )
        };

        assert_eq!(status, RtStatus::Ok);
        assert!(value_out.root.is_null());
        assert_eq!(value_out.len, rtdt::Index::ZERO);
    }
}

/// Allocate a copy of a tensor's shape or strides, one index per axis.
///
/// # Safety
///
/// `src` must point at `rank` indices, and `rank` be at least one.
unsafe fn copy_index_array(
    rt_ref: &mut rt_local::RtLocal,
    src: *const rtdt::Index,
    rank: u32,
) -> *const rtdt::Index {
    assert!(!src.is_null(), "a tensor keeps its shape and strides, empty or not");
    unsafe {
        let dst = rt_ref.alloc.alloc(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into()) as *mut rtdt::Index;
        std::ptr::copy_nonoverlapping(src, dst, rank as usize);
        dst as *const rtdt::Index
    }
}
