//! Tensor operations.

use rmx::prelude::*;
use crate::{RtStatus, rt_local::RtLocal};
use crate::rtdt::TyDescRef;

// ============================================================================
// Stride Computation Helpers
// ============================================================================

/// Compute row-major strides from shape.
///
/// Row-major: strides[i] = product(shape[i+1..])
fn compute_row_major_strides(shape: &[u32]) -> Vec<u32> {
    let rank = shape.len();
    let mut strides = vec![0u32; rank];

    for i in 0..rank {
        let mut stride = 1u32;
        for j in (i + 1)..rank {
            stride = stride.saturating_mul(shape[j]);
        }
        strides[i] = stride;
    }

    strides
}

/// Compute column-major strides from shape.
///
/// Column-major: strides[i] = product(shape[..i])
fn compute_col_major_strides(shape: &[u32]) -> Vec<u32> {
    let rank = shape.len();
    let mut strides = vec![0u32; rank];

    for i in 0..rank {
        let mut stride = 1u32;
        for j in 0..i {
            stride = stride.saturating_mul(shape[j]);
        }
        strides[i] = stride;
    }

    strides
}

/// Creates a tensor from a flat slice of elements and a shape.
///
/// The shape is moved in and ownership is transferred.
/// Allocates the data buffer, shape array, and strides array.
pub unsafe fn tensor_create_from_slice_impl(
    rt_ref: &mut RtLocal,
    slice_ptr_ref: *const u8,
    slice_len: u32,
    element_tydesc_ref: TyDescRef,
    shape_in: *mut u8,
    shape_tydesc_ref: TyDescRef,
    layout: u8,
    tensor_value_out: *mut u8,
    tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        if tensor_value_out.is_null() || slice_ptr_ref.is_null() || shape_in.is_null() {
            return RtStatus::Error;
        }

        // Extract shape from the moved-in List<u32>.
        let shape_list_ptr = shape_in as *mut crate::rtdt::List;
        let shape_data = (*shape_list_ptr).data as *const u32;
        let rank = (*shape_list_ptr).size;

        if rank == 0 || shape_data.is_null() {
            // Destroy the shape list before returning.
            let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Convert shape to slice.
        let shape_slice = std::slice::from_raw_parts(shape_data, rank as usize);

        // Compute total elements from shape.
        let mut total_elems = 1u32;
        for &dim in shape_slice {
            total_elems = total_elems.saturating_mul(dim);
        }

        // Validate slice length matches total elements.
        if slice_len != total_elems {
            let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        let element_size = element_tydesc_ref.size();
        let element_align = element_tydesc_ref.align();

        // Allocate data buffer.
        let data_ptr = rt_ref.alloc.alloc(element_size, element_align, total_elems);
        if data_ptr.is_null() {
            let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Allocate shape array.
        let shape_array_ptr = rt_ref.alloc.alloc(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
        ) as *mut u32;
        if shape_array_ptr.is_null() {
            rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr);
            let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Allocate strides array.
        let strides_array_ptr = rt_ref.alloc.alloc(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
        ) as *mut u32;
        if strides_array_ptr.is_null() {
            rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr);
            rt_ref.alloc.free(
                std::mem::size_of::<u32>() as u32,
                std::mem::align_of::<u32>() as u32,
                rank,
                shape_array_ptr as *mut u8,
            );
            let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Clone elements from slice to data buffer.
        let rt_handle = rt_ref as *mut RtLocal as crate::LocalRtHandle;
        for i in 0..total_elems {
            let src_ptr = slice_ptr_ref.add((i * element_size) as usize);
            let dest_ptr = data_ptr.add((i * element_size) as usize);

            let status = crate::clone::clone_value(
                rt_handle,
                src_ptr,
                element_tydesc_ref.as_ptr(),
                dest_ptr,
            );
            if status != RtStatus::Ok {
                // Clean up partially created tensor.
                // Destroy elements that were successfully cloned.
                for j in 0..i {
                    let elem_ptr = data_ptr.add((j * element_size) as usize);
                    let _ = crate::destroy::any_destroy_local(
                        rt_handle,
                        elem_ptr,
                        element_tydesc_ref.as_ptr(),
                    );
                }
                rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr);
                rt_ref.alloc.free(
                    std::mem::size_of::<u32>() as u32,
                    std::mem::align_of::<u32>() as u32,
                    rank,
                    shape_array_ptr as *mut u8,
                );
                rt_ref.alloc.free(
                    std::mem::size_of::<u32>() as u32,
                    std::mem::align_of::<u32>() as u32,
                    rank,
                    strides_array_ptr as *mut u8,
                );
                let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
                return status;
            }
        }

        // Copy shape values.
        std::ptr::copy_nonoverlapping(shape_data, shape_array_ptr, rank as usize);

        // Compute and write strides based on layout.
        let layout_enum = std::mem::transmute::<u8, crate::rtdt::TensorLayout>(layout);
        let strides = match layout_enum {
            crate::rtdt::TensorLayout::RowMajor | crate::rtdt::TensorLayout::ColMajorTransposed => {
                compute_row_major_strides(shape_slice)
            }
            crate::rtdt::TensorLayout::ColMajor | crate::rtdt::TensorLayout::RowMajorTransposed => {
                compute_col_major_strides(shape_slice)
            }
        };
        std::ptr::copy_nonoverlapping(strides.as_ptr(), strides_array_ptr, rank as usize);

        // Initialize Tensor struct.
        let tensor_ptr = tensor_value_out as *mut crate::rtdt::Tensor;
        (*tensor_ptr).ptr_base = data_ptr;
        (*tensor_ptr).offset_elems = 0;
        (*tensor_ptr).capacity_elems = total_elems;
        (*tensor_ptr).shape = shape_array_ptr;
        (*tensor_ptr).strides = strides_array_ptr;
        (*tensor_ptr).layout = layout_enum;

        // Destroy the moved-in shape list (we've copied its data).
        let _ = crate::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);

        RtStatus::Ok
    }
}

/// Gets a pointer to an element at the specified indices.
///
/// Validates indices against shape and computes linear offset.
pub unsafe fn tensor_get_impl(
    _rt_ref: &mut RtLocal,
    tensor_value_ref: *const u8,
    tensor_tydesc_ref: TyDescRef,
    indices_ptr: *const u32,
    element_ptr_out: *mut *const u8,
) -> RtStatus {
    unsafe {
        if tensor_value_ref.is_null() || indices_ptr.is_null() || element_ptr_out.is_null() {
            return RtStatus::Error;
        }

        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let rank = tensor_tydesc_ref.tensor_rank();

        let tensor_ptr = tensor_value_ref as *const crate::rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape as *const u32;
        let strides_ptr = (*tensor_ptr).strides as *const u32;

        if ptr_base.is_null() || shape_ptr.is_null() || strides_ptr.is_null() || rank == 0 {
            return RtStatus::Error;
        }

        // Validate indices and compute linear offset.
        let indices_slice = std::slice::from_raw_parts(indices_ptr, rank as usize);
        let shape_slice = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides_slice = std::slice::from_raw_parts(strides_ptr, rank as usize);

        let mut linear_offset = offset_elems;
        for i in 0..rank as usize {
            let index = indices_slice[i];
            let dim = shape_slice[i];

            if index >= dim {
                return RtStatus::Error;
            }

            linear_offset = linear_offset.saturating_add(index.saturating_mul(strides_slice[i]));
        }

        // Compute element pointer.
        let element_size = element_ty.size() as usize;
        let element_ptr = ptr_base.add(linear_offset as usize * element_size);

        *element_ptr_out = element_ptr;

        RtStatus::Ok
    }
}

/// Sets an element at the specified indices.
///
/// Validates indices, destroys old value, and clones new value into place.
pub unsafe fn tensor_set_impl(
    rt_ref: &mut RtLocal,
    tensor_value_ref: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    indices_ptr: *const u32,
    value_ptr: *const u8,
) -> RtStatus {
    unsafe {
        if tensor_value_ref.is_null() || indices_ptr.is_null() || value_ptr.is_null() {
            return RtStatus::Error;
        }

        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let rank = tensor_tydesc_ref.tensor_rank();

        let tensor_ptr = tensor_value_ref as *mut crate::rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape as *const u32;
        let strides_ptr = (*tensor_ptr).strides as *const u32;

        if ptr_base.is_null() || shape_ptr.is_null() || strides_ptr.is_null() || rank == 0 {
            return RtStatus::Error;
        }

        // Validate indices and compute linear offset.
        let indices_slice = std::slice::from_raw_parts(indices_ptr, rank as usize);
        let shape_slice = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides_slice = std::slice::from_raw_parts(strides_ptr, rank as usize);

        let mut linear_offset = offset_elems;
        for i in 0..rank as usize {
            let index = indices_slice[i];
            let dim = shape_slice[i];

            if index >= dim {
                return RtStatus::Error;
            }

            linear_offset = linear_offset.saturating_add(index.saturating_mul(strides_slice[i]));
        }

        // Compute element pointer.
        let element_size = element_ty.size() as usize;
        let element_ptr = ptr_base.add(linear_offset as usize * element_size);

        // Destroy old value at this location.
        let rt_handle = rt_ref as *mut RtLocal as crate::LocalRtHandle;
        let status = crate::destroy::any_destroy_local(
            rt_handle,
            element_ptr,
            element_ty.as_ptr(),
        );
        if status != RtStatus::Ok {
            return status;
        }

        // Clone new value into this location.
        let status = crate::clone::clone_value(
            rt_handle,
            value_ptr,
            element_ty.as_ptr(),
            element_ptr,
        );

        status
    }
}

/// Transposes a tensor by permuting its dimensions.
///
/// Creates a view with dimensions reordered according to the permutation.
/// For 2D tensors, perm is typically [1, 0] to swap rows and columns.
/// The permutation array must have length equal to rank.
pub unsafe fn tensor_transpose_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    perm_ptr: *const u32,
    tensor_value_out: *mut u8,
) -> RtStatus {
    unsafe {
        if tensor_value_in.is_null() || perm_ptr.is_null() || tensor_value_out.is_null() {
            return RtStatus::Error;
        }

        let rank = tensor_tydesc_ref.tensor_rank();
        if rank == 0 {
            return RtStatus::Error;
        }

        let tensor_in_ptr = tensor_value_in as *mut crate::rtdt::Tensor;
        let tensor_out_ptr = tensor_value_out as *mut crate::rtdt::Tensor;

        let ptr_base = (*tensor_in_ptr).ptr_base;
        let offset_elems = (*tensor_in_ptr).offset_elems;
        let capacity_elems = (*tensor_in_ptr).capacity_elems;
        let shape_in_ptr = (*tensor_in_ptr).shape;
        let strides_in_ptr = (*tensor_in_ptr).strides;
        let layout_in = (*tensor_in_ptr).layout;

        if ptr_base.is_null() || shape_in_ptr.is_null() || strides_in_ptr.is_null() {
            return RtStatus::Error;
        }

        // Validate permutation.
        let perm_slice = std::slice::from_raw_parts(perm_ptr, rank as usize);
        let mut seen = vec![false; rank as usize];
        for &p in perm_slice {
            if p >= rank {
                return RtStatus::Error;
            }
            if seen[p as usize] {
                return RtStatus::Error;
            }
            seen[p as usize] = true;
        }

        // Allocate new shape array.
        let shape_out_ptr = rt_ref.alloc.alloc(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
        ) as *mut u32;
        if shape_out_ptr.is_null() {
            return RtStatus::Error;
        }

        // Allocate new strides array.
        let strides_out_ptr = rt_ref.alloc.alloc(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
        ) as *mut u32;
        if strides_out_ptr.is_null() {
            rt_ref.alloc.free(
                std::mem::size_of::<u32>() as u32,
                std::mem::align_of::<u32>() as u32,
                rank,
                shape_out_ptr as *mut u8,
            );
            return RtStatus::Error;
        }

        // Copy permuted shape and strides.
        for i in 0..rank as usize {
            let src_idx = perm_slice[i] as usize;
            *shape_out_ptr.add(i) = *shape_in_ptr.add(src_idx);
            *strides_out_ptr.add(i) = *strides_in_ptr.add(src_idx);
        }

        // Determine output layout.
        // For 2D transpose ([1, 0]): RowMajor <-> ColMajorTransposed, ColMajor <-> RowMajorTransposed
        let layout_out = if rank == 2 && perm_slice == [1, 0] {
            match layout_in {
                crate::rtdt::TensorLayout::RowMajor => crate::rtdt::TensorLayout::ColMajorTransposed,
                crate::rtdt::TensorLayout::ColMajor => crate::rtdt::TensorLayout::RowMajorTransposed,
                crate::rtdt::TensorLayout::RowMajorTransposed => crate::rtdt::TensorLayout::ColMajor,
                crate::rtdt::TensorLayout::ColMajorTransposed => crate::rtdt::TensorLayout::RowMajor,
            }
        } else {
            // For non-standard permutations, keep layout as-is (strides encode the transformation).
            layout_in
        };

        // Initialize output tensor.
        (*tensor_out_ptr).ptr_base = ptr_base;
        (*tensor_out_ptr).offset_elems = offset_elems;
        (*tensor_out_ptr).capacity_elems = capacity_elems;
        (*tensor_out_ptr).shape = shape_out_ptr;
        (*tensor_out_ptr).strides = strides_out_ptr;
        (*tensor_out_ptr).layout = layout_out;

        // Free old shape and strides arrays from input tensor.
        rt_ref.alloc.free(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
            shape_in_ptr as *mut u8,
        );
        rt_ref.alloc.free(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
            strides_in_ptr as *mut u8,
        );

        // Clear input tensor (linear type system: ownership transferred).
        (*tensor_in_ptr).ptr_base = std::ptr::null_mut();
        (*tensor_in_ptr).offset_elems = 0;
        (*tensor_in_ptr).capacity_elems = 0;
        (*tensor_in_ptr).shape = std::ptr::null();
        (*tensor_in_ptr).strides = std::ptr::null();

        RtStatus::Ok
    }
}

/// Destroys a tensor, freeing all three allocations.
///
/// Frees the shape array, strides array, and data buffer.
pub unsafe fn tensor_destroy_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        if tensor_value_in.is_null() {
            return RtStatus::Error;
        }

        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let rank = tensor_tydesc_ref.tensor_rank();

        let tensor_ptr = tensor_value_in as *mut crate::rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let capacity_elems = (*tensor_ptr).capacity_elems;
        let shape_ptr = (*tensor_ptr).shape as *mut u32;
        let strides_ptr = (*tensor_ptr).strides as *mut u32;

        // Compute total elements from shape.
        let mut total_elems = 1u32;
        if !shape_ptr.is_null() && rank > 0 {
            for i in 0..rank {
                let dim = *shape_ptr.add(i as usize);
                total_elems = total_elems.saturating_mul(dim);
            }
        }

        // Destroy all elements in the data buffer.
        if !ptr_base.is_null() && total_elems > 0 {
            let element_size = element_ty.size() as usize;
            let rt_handle = rt_ref as *mut RtLocal as crate::LocalRtHandle;

            for i in 0..total_elems {
                let element_ptr = ptr_base.add(i as usize * element_size);
                let status = crate::destroy::any_destroy_local(
                    rt_handle,
                    element_ptr,
                    element_ty.as_ptr(),
                );
                if status != RtStatus::Ok {
                    return status;
                }
            }
        }

        // Free the data buffer.
        if !ptr_base.is_null() && capacity_elems > 0 {
            rt_ref.alloc.free(
                element_ty.size(),
                element_ty.align(),
                capacity_elems,
                ptr_base,
            );
        }

        // Free the shape array.
        if !shape_ptr.is_null() && rank > 0 {
            rt_ref.alloc.free(
                std::mem::size_of::<u32>() as u32,
                std::mem::align_of::<u32>() as u32,
                rank,
                shape_ptr as *mut u8,
            );
        }

        // Free the strides array.
        if !strides_ptr.is_null() && rank > 0 {
            rt_ref.alloc.free(
                std::mem::size_of::<u32>() as u32,
                std::mem::align_of::<u32>() as u32,
                rank,
                strides_ptr as *mut u8,
            );
        }

        // Reset the tensor.
        (*tensor_ptr).ptr_base = std::ptr::null_mut();
        (*tensor_ptr).offset_elems = 0;
        (*tensor_ptr).capacity_elems = 0;
        (*tensor_ptr).shape = std::ptr::null();
        (*tensor_ptr).strides = std::ptr::null();

        RtStatus::Ok
    }
}
