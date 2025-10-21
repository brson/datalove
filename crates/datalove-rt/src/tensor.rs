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
    _rt_ref: &mut RtLocal,
    _tensor_value_in: *mut u8,
    _tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    todo!("tensor_destroy_impl")
}
