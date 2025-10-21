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
