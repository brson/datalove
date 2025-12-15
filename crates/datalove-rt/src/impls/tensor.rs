//! Tensor operations.

use rmx::prelude::*;
use crate::{c::RtStatus, impls::rt_local::RtLocal};
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

// ============================================================================
// Contiguity Check Helpers
// ============================================================================

/// Check if strides match row-major contiguous pattern.
///
/// Row-major: strides[i] == product(shape[i+1..])
fn is_row_major_contiguous(shape: &[u32], strides: &[u32]) -> bool {
    if shape.len() != strides.len() {
        return false;
    }

    for i in 0..shape.len() {
        let mut expected = 1u32;
        for j in (i + 1)..shape.len() {
            expected = expected.saturating_mul(shape[j]);
        }
        if strides[i] != expected {
            return false;
        }
    }

    true
}

/// Check if strides match column-major contiguous pattern.
///
/// Column-major: strides[i] == product(shape[..i])
fn is_col_major_contiguous(shape: &[u32], strides: &[u32]) -> bool {
    if shape.len() != strides.len() {
        return false;
    }

    for i in 0..shape.len() {
        let mut expected = 1u32;
        for j in 0..i {
            expected = expected.saturating_mul(shape[j]);
        }
        if strides[i] != expected {
            return false;
        }
    }

    true
}

/// Check if tensor is contiguous (either row-major or column-major).
fn is_contiguous(shape: &[u32], strides: &[u32]) -> bool {
    is_row_major_contiguous(shape, strides) || is_col_major_contiguous(shape, strides)
}

// ============================================================================
// Tensor Creation
// ============================================================================

/// Creates a tensor from a flat slice of elements and a shape.
///
/// The shape is moved in and ownership is transferred.
/// Allocates the data buffer, shape array, and strides array.
pub unsafe fn tensor_create_from_slice_impl(
    rt_ref: &mut RtLocal,
    slice_ptr_ref: *const u8,
    slice_len: u32,
    element_tydesc_ref: TyDescRef,
    // u32 x rank
    shape_in: *mut u8,
    shape_tydesc_ref: TyDescRef,
    layout: u8,
    tensor_value_out: *mut u8,
    _tensor_tydesc_ref: TyDescRef,
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
            let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
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
            let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        let element_size = element_tydesc_ref.size();
        let element_align = element_tydesc_ref.align();

        // Allocate data buffer.
        let data_ptr = rt_ref.alloc.alloc(element_size, element_align, total_elems);
        if data_ptr.is_null() {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
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
            let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
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
            let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Clone elements from slice to data buffer.
        let rt_handle = rt_ref as *mut RtLocal as crate::c::LocalRtHandle;
        for i in 0..total_elems {
            let src_ptr = slice_ptr_ref.add((i * element_size) as usize);
            let dest_ptr = data_ptr.add((i * element_size) as usize);

            let status = crate::impls::clone::clone_value(
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
                    let _ = crate::impls::destroy::any_destroy_local(
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
                let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
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
        let _ = crate::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);

        RtStatus::Ok
    }
}

/// Gets an element at the specified indices by cloning it.
///
/// Validates indices, computes linear offset, and clones element to output.
pub unsafe fn tensor_get_impl(
    rt_ref: &mut RtLocal,
    tensor_value_ref: *const u8,
    tensor_tydesc_ref: TyDescRef,
    // u32 x rank
    indices_ptr: *const u32,
    element_value_out: *mut u8,
) -> RtStatus {
    unsafe {
        if tensor_value_ref.is_null() || indices_ptr.is_null() || element_value_out.is_null() {
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

        // Clone element to output buffer.
        let rt_handle = rt_ref as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::clone::clone_value(
            rt_handle,
            element_ptr,
            element_ty.as_ptr(),
            element_value_out,
        );

        status
    }
}

/// Sets an element at the specified indices.
///
/// Validates indices, destroys old value, and clones new value into place.
pub unsafe fn tensor_set_impl(
    rt_ref: &mut RtLocal,
    tensor_value_ref: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    // fixme in pointer
    indices_ptr: *const u32,
    // fixme in pointer
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
        let rt_handle = rt_ref as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::impls::destroy::any_destroy_local(
            rt_handle,
            element_ptr,
            element_ty.as_ptr(),
        );
        if status != RtStatus::Ok {
            return status;
        }

        // Clone new value into this location.
        let status = crate::impls::clone::clone_value(
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
//
// fixme verify the semantics of the in/out args here
pub unsafe fn tensor_transpose_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    // fixme in/out/ref etc?
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
            let rt_handle = rt_ref as *mut RtLocal as crate::c::LocalRtHandle;

            for i in 0..total_elems {
                let element_ptr = ptr_base.add(i as usize * element_size);
                let status = crate::impls::destroy::any_destroy_local(
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

/// Creates a slice view of a tensor.
///
/// The input tensor is moved in and transformed to a view by updating its
/// offset and shape. Returns Result<Tensor, Error> - on error, the original
/// tensor is packed into the Error value for reclamation.
pub unsafe fn tensor_slice_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    ranges_ptr: *const crate::rtdt::SliceRange,
    result_value_out: *mut u8,
    result_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        // Null pointer checks.
        if tensor_value_in.is_null() || ranges_ptr.is_null() || result_value_out.is_null() {
            return RtStatus::Error;
        }

        // Extract tensor fields.
        let tensor_ptr = tensor_value_in as *mut crate::rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let capacity_elems = (*tensor_ptr).capacity_elems;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape;
        let strides_ptr = (*tensor_ptr).strides;
        let layout = (*tensor_ptr).layout;

        // Extract rank from tydesc.
        let rank = tensor_tydesc_ref.tensor_rank();

        // Validate rank > 0.
        if rank == 0 {
            return error_path(
                rt_ref,
                tensor_value_in,
                tensor_tydesc_ref,
                result_value_out,
                result_tydesc_ref,
            );
        }

        // Convert to slices for easier access.
        let shape = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides = std::slice::from_raw_parts(strides_ptr, rank as usize);
        let ranges = std::slice::from_raw_parts(ranges_ptr, rank as usize);

        // Validate ranges.
        for i in 0..rank as usize {
            let start = ranges[i].start;
            let end = ranges[i].end;
            let dim_size = shape[i];

            // Check: 0 <= start < end <= shape[i]
            if start >= end || end > dim_size {
                return error_path(
                    rt_ref,
                    tensor_value_in,
                    tensor_tydesc_ref,
                    result_value_out,
                    result_tydesc_ref,
                );
            }
        }

        // Allocate new shape array.
        let new_shape_ptr = rt_ref.alloc.alloc(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
        ) as *mut u32;

        if new_shape_ptr.is_null() {
            return error_path(
                rt_ref,
                tensor_value_in,
                tensor_tydesc_ref,
                result_value_out,
                result_tydesc_ref,
            );
        }

        // Allocate new strides array.
        let new_strides_ptr = rt_ref.alloc.alloc(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
        ) as *mut u32;

        if new_strides_ptr.is_null() {
            // Clean up shape allocation.
            rt_ref.alloc.free(
                std::mem::size_of::<u32>() as u32,
                std::mem::align_of::<u32>() as u32,
                rank,
                new_shape_ptr as *mut u8,
            );
            return error_path(
                rt_ref,
                tensor_value_in,
                tensor_tydesc_ref,
                result_value_out,
                result_tydesc_ref,
            );
        }

        // Compute new offset and populate new shape/strides.
        let mut new_offset = offset_elems;
        for i in 0..rank as usize {
            // Update offset: offset += ranges[i].start * strides[i]
            new_offset = new_offset.saturating_add(
                ranges[i].start.saturating_mul(strides[i])
            );

            // New shape: new_shape[i] = ranges[i].end - ranges[i].start
            *new_shape_ptr.add(i) = ranges[i].end - ranges[i].start;

            // Copy stride unchanged.
            *new_strides_ptr.add(i) = strides[i];
        }

        // Free old shape and strides arrays.
        rt_ref.alloc.free(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
            shape_ptr as *mut u8,
        );
        rt_ref.alloc.free(
            std::mem::size_of::<u32>() as u32,
            std::mem::align_of::<u32>() as u32,
            rank,
            strides_ptr as *mut u8,
        );

        // Construct output tensor.
        let out_tensor_ptr = compute_result_payload_ptr(result_value_out, result_tydesc_ref)
            as *mut crate::rtdt::Tensor;

        let out_tensor = crate::rtdt::Tensor {
            ptr_base,
            capacity_elems,
            offset_elems: new_offset,
            shape: new_shape_ptr,
            strides: new_strides_ptr,
            layout,
        };

        std::ptr::write(out_tensor_ptr, out_tensor);

        // Set Result tag to Ok.
        let result_ptr = result_value_out as *mut crate::rtdt::Result;
        std::ptr::write(&mut (*result_ptr).tag, crate::rtdt::ResultTag::Ok);

        return RtStatus::Ok;

        // Helper function for error path.
        unsafe fn error_path(
            _rt_ref: &mut RtLocal,
            tensor_value_in: *mut u8,
            _tensor_tydesc_ref: TyDescRef,
            result_value_out: *mut u8,
            result_tydesc_ref: TyDescRef,
        ) -> RtStatus {
            unsafe {
                // Copy the tensor into the result's error payload.
                let payload_ptr = compute_result_payload_ptr(result_value_out, result_tydesc_ref);
                let tensor_src = tensor_value_in as *const crate::rtdt::Tensor;
                let tensor_dst = payload_ptr as *mut crate::rtdt::Tensor;
                let tensor_value = std::ptr::read(tensor_src);
                std::ptr::write(tensor_dst, tensor_value);

                // Set Result tag to Err.
                let result_ptr = result_value_out as *mut crate::rtdt::Result;
                std::ptr::write(&mut (*result_ptr).tag, crate::rtdt::ResultTag::Err);

                RtStatus::Ok
            }
        }

        // Helper to compute payload offset in Result<T>.
        unsafe fn compute_result_payload_ptr(
            result_ptr: *mut u8,
            result_tydesc_ref: TyDescRef,
        ) -> *mut u8 {
            unsafe {
                // Extract ok_tydesc from result type descriptor.
                let ok_tydesc_ref = result_tydesc_ref.result_ok_ty();

                let ok_align = ok_tydesc_ref.align();
                let payload_offset = crate::rtdt::layout::result_payload_offset(ok_align);

                result_ptr.add(payload_offset as usize)
            }
        }
    }
}

// ============================================================================
// Tensor Reshape
// ============================================================================

/// Reshape a tensor to a new shape.
///
/// Validates that the tensor is contiguous and the new shape has the same total
/// number of elements. Returns Result<Tensor, Error> where Error contains the
/// original tensor on failure.
pub unsafe fn tensor_reshape_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    new_shape_in: *mut u8,
    new_shape_tydesc_ref: TyDescRef,
    result_value_out: *mut u8,
    result_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        // Validate inputs.
        if tensor_value_in.is_null() || new_shape_in.is_null() || result_value_out.is_null() {
            return RtStatus::Error;
        }

        // Extract new_shape from the moved-in List<u32>.
        let new_shape_list_ptr = new_shape_in as *mut crate::rtdt::List;
        let new_shape_data = (*new_shape_list_ptr).data as *const u32;
        let new_rank = (*new_shape_list_ptr).size;

        // Validate new rank.
        if new_rank == 0 || new_shape_data.is_null() {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Extract tensor fields.
        let rank = tensor_tydesc_ref.tensor_rank();
        if rank == 0 {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        let tensor_ptr = tensor_value_in as *mut crate::rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let capacity_elems = (*tensor_ptr).capacity_elems;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape;
        let strides_ptr = (*tensor_ptr).strides;
        let layout = (*tensor_ptr).layout;

        // Create slices for validation.
        let shape = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides = std::slice::from_raw_parts(strides_ptr, rank as usize);
        let new_shape = std::slice::from_raw_parts(new_shape_data, new_rank as usize);

        // Validate offset_elems == 0 (can only reshape full tensor, not view).
        if offset_elems != 0 {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Validate tensor is contiguous.
        if !is_contiguous(shape, strides) {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Compute current total elements.
        let mut current_total = 1u32;
        for &dim in shape {
            current_total = current_total.saturating_mul(dim);
        }

        // Compute new total elements.
        let mut new_total = 1u32;
        for &dim in new_shape {
            new_total = new_total.saturating_mul(dim);
        }

        // Validate total elements match.
        if current_total != new_total {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Determine layout from current strides.
        let is_row_major = is_row_major_contiguous(shape, strides);

        // Compute new strides based on layout.
        let new_strides_vec = if is_row_major {
            compute_row_major_strides(new_shape)
        } else {
            compute_col_major_strides(new_shape)
        };

        // Allocate new strides array.
        let strides_size = std::mem::size_of::<u32>() as u32;
        let strides_align = std::mem::align_of::<u32>() as u32;
        let new_strides_ptr = rt_ref.alloc.alloc(strides_size, strides_align, new_rank);
        if new_strides_ptr.is_null() {
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Copy new strides into allocated array.
        let new_strides_ptr_typed = new_strides_ptr as *mut u32;
        for (i, &stride) in new_strides_vec.iter().enumerate() {
            std::ptr::write(new_strides_ptr_typed.add(i), stride);
        }

        // Allocate new shape array and copy from new_shape List.
        let shape_size = std::mem::size_of::<u32>() as u32;
        let shape_align = std::mem::align_of::<u32>() as u32;
        let new_shape_ptr = rt_ref.alloc.alloc(shape_size, shape_align, new_rank);
        if new_shape_ptr.is_null() {
            // Free new_strides before returning.
            rt_ref.alloc.free(strides_size, strides_align, new_rank, new_strides_ptr);
            let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Copy shape data from List to new array.
        let new_shape_ptr_typed = new_shape_ptr as *mut u32;
        for i in 0..new_rank as usize {
            std::ptr::write(new_shape_ptr_typed.add(i), new_shape[i]);
        }

        // Free old strides array.
        rt_ref.alloc.free(strides_size, strides_align, rank, strides_ptr as *mut u8);

        // Free old shape array.
        rt_ref.alloc.free(shape_size, shape_align, rank, shape_ptr as *mut u8);

        // Destroy the new_shape List (including its data).
        let _ = crate::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);

        // Construct output tensor.
        let payload_ptr = compute_result_payload_ptr(result_value_out, result_tydesc_ref);
        let out_tensor_ptr = payload_ptr as *mut crate::rtdt::Tensor;

        let out_tensor = crate::rtdt::Tensor {
            ptr_base,
            capacity_elems,
            offset_elems: 0,
            shape: new_shape_ptr_typed,
            strides: new_strides_ptr_typed,
            layout,
        };

        std::ptr::write(out_tensor_ptr, out_tensor);

        // Set Result tag to Ok.
        let result_ptr = result_value_out as *mut crate::rtdt::Result;
        std::ptr::write(&mut (*result_ptr).tag, crate::rtdt::ResultTag::Ok);

        return RtStatus::Ok;

        // Helper function for error path.
        unsafe fn error_path(
            _rt_ref: &mut RtLocal,
            tensor_value_in: *mut u8,
            _tensor_tydesc_ref: TyDescRef,
            result_value_out: *mut u8,
            result_tydesc_ref: TyDescRef,
        ) -> RtStatus {
            unsafe {
                // Copy the tensor into the result's error payload.
                let payload_ptr = compute_result_payload_ptr(result_value_out, result_tydesc_ref);
                let tensor_src = tensor_value_in as *const crate::rtdt::Tensor;
                let tensor_dst = payload_ptr as *mut crate::rtdt::Tensor;
                let tensor_value = std::ptr::read(tensor_src);
                std::ptr::write(tensor_dst, tensor_value);

                // Set Result tag to Err.
                let result_ptr = result_value_out as *mut crate::rtdt::Result;
                std::ptr::write(&mut (*result_ptr).tag, crate::rtdt::ResultTag::Err);

                RtStatus::Ok
            }
        }

        // Helper to compute payload offset in Result<T>.
        unsafe fn compute_result_payload_ptr(
            result_ptr: *mut u8,
            result_tydesc_ref: TyDescRef,
        ) -> *mut u8 {
            unsafe {
                // Extract ok_tydesc from result type descriptor.
                let ok_tydesc_ref = result_tydesc_ref.result_ok_ty();

                let ok_align = ok_tydesc_ref.align();
                let payload_offset = crate::rtdt::layout::result_payload_offset(ok_align);

                result_ptr.add(payload_offset as usize)
            }
        }
    }
}
