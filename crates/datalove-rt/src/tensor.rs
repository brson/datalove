//! Tensor operations.

use rmx::prelude::*;
use crate::{RtStatus, rt_local::RtLocal};
use crate::rtdt::TyDescRef;

/// Creates a tensor from a flat slice of elements and a shape.
///
/// The shape is moved in and ownership is transferred.
/// Allocates the data buffer, shape array, and strides array.
pub unsafe fn tensor_create_from_slice_impl(
    _rt_ref: &mut RtLocal,
    _slice_ptr_ref: *const u8,
    _slice_len: u32,
    _element_tydesc_ref: TyDescRef,
    _shape_in: *mut u8,
    _shape_tydesc_ref: TyDescRef,
    _layout: u8,
    _tensor_value_out: *mut u8,
    _tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    todo!("tensor_create_from_slice_impl")
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
