//! Tensor operations.

use crate::c::{RtStatus, impls::rt_local::RtLocal};
use datalove_rtdt as rtdt;
use datalove_rtdt::TyDescRef;

// ============================================================================
// View Arithmetic
// ============================================================================
//
// A tensor is a view of a buffer: element `[i0, i1, ...]` is at position
// `offset + i0 * strides[0] + i1 * strides[1] + ...`, counted in elements
// from the buffer's start. Slicing, transposing, taking a hyperplane and
// reshaping each make a new view of the same buffer, or of a copy of part of
// it, by arithmetic on the offset, the shape and the strides alone. That
// arithmetic is here, apart from the pointers and allocations around it, so
// that Kani can prove each new view picks out the elements of the old one it
// should; see botdocs/kani.md.

use rtdt::{Index, IndexRepr};

/// The position of the element at `indices` in a view's buffer, or `None` if
/// an index is past the end of its axis.
///
/// The arithmetic is plain: in a view that is well formed, every element's
/// position is inside its buffer, so it cannot overflow.
pub(crate) fn element_position(
    offset: IndexRepr,
    shape: &[Index],
    strides: &[Index],
    indices: &[IndexRepr],
) -> Option<IndexRepr> {
    debug_assert!(shape.len() == strides.len() && shape.len() == indices.len());
    let mut position = offset;
    for ((&index, extent), stride) in indices.iter().zip(shape).zip(strides) {
        if index >= extent.0 {
            return None;
        }
        position += index * stride.0;
    }
    Some(position)
}

/// The view of `ranges` of a view: its shape, written to `shape_out`, and its
/// offset, with the strides unchanged. `None` if a range is empty or runs
/// past its axis.
///
/// A slice's first element is an element of the view, so its offset is a
/// position inside the buffer and cannot overflow.
pub(crate) fn slice_view(
    offset: IndexRepr,
    shape: &[Index],
    strides: &[Index],
    ranges: &[rtdt::SliceRange],
    shape_out: &mut [Index],
) -> Option<IndexRepr> {
    if ranges.iter().zip(shape).any(|(range, extent)| range.start >= range.end || range.end > extent.0) {
        return None;
    }
    let mut new_offset = offset;
    for ((range, stride), extent_out) in ranges.iter().zip(strides).zip(shape_out.iter_mut()) {
        new_offset += range.start * stride.0;
        *extent_out = Index(range.end - range.start);
    }
    Some(new_offset)
}

/// Whether `perm` names each axis of a tensor of its length exactly once.
fn is_permutation(perm: &[u32]) -> bool {
    perm.iter().enumerate().all(|(i, &axis)| {
        (axis as usize) < perm.len() && !perm[..i].contains(&axis)
    })
}

/// The view with its axes reordered: axis `i` of the result is axis
/// `perm[i]` of the view, written to `shape_out` and `strides_out`. The
/// offset is unchanged. `false` if `perm` is not a permutation of the axes.
pub(crate) fn transpose_view(
    shape: &[Index],
    strides: &[Index],
    perm: &[u32],
    shape_out: &mut [Index],
    strides_out: &mut [Index],
) -> bool {
    if !is_permutation(perm) {
        return false;
    }
    for (i, &axis) in perm.iter().enumerate() {
        shape_out[i] = shape[axis as usize];
        strides_out[i] = strides[axis as usize];
    }
    true
}

/// The offset of the hyperplane at `index` along a view's first axis, whose
/// shape and strides are the view's after the first. `None` if `index` is
/// past the first axis.
///
/// Only a hyperplane with elements has a position to give; with an empty
/// axis after the first, the first stride may not be one.
pub(crate) fn hyperplane_offset(
    offset: IndexRepr,
    shape: &[Index],
    strides: &[Index],
    index: IndexRepr,
) -> Option<IndexRepr> {
    if index >= shape[0].0 {
        return None;
    }
    Some(offset + index * strides[0].0)
}

/// The strides that lay a shape out row-major: the last axis adjacent, each
/// axis before stepping over the whole of the axes after it.
///
/// Saturating, because an empty tensor keeps its shape: in `[0, n, n]` the
/// first stride can be past what an index can say, and is never used, since
/// there is no element to step to.
pub(crate) fn row_major_strides(shape: &[Index], strides_out: &mut [Index]) {
    let mut stride: IndexRepr = 1;
    for (extent, stride_out) in shape.iter().zip(strides_out.iter_mut()).rev() {
        *stride_out = Index(stride);
        stride = stride.saturating_mul(extent.0);
    }
}

/// The strides that lay a shape out column-major: the first axis adjacent.
/// Saturating as `row_major_strides` is.
pub(crate) fn col_major_strides(shape: &[Index], strides_out: &mut [Index]) {
    let mut stride: IndexRepr = 1;
    for (extent, stride_out) in shape.iter().zip(strides_out.iter_mut()) {
        *stride_out = Index(stride);
        stride = stride.saturating_mul(extent.0);
    }
}

/// Whether `strides` lay `shape` out row-major, as `row_major_strides` would.
fn is_row_major_contiguous(shape: &[Index], strides: &[Index]) -> bool {
    let mut stride: IndexRepr = 1;
    for (extent, actual) in shape.iter().zip(strides).rev() {
        if actual.0 != stride {
            return false;
        }
        stride = stride.saturating_mul(extent.0);
    }
    true
}

/// Whether `strides` lay `shape` out column-major.
fn is_col_major_contiguous(shape: &[Index], strides: &[Index]) -> bool {
    let mut stride: IndexRepr = 1;
    for (extent, actual) in shape.iter().zip(strides) {
        if actual.0 != stride {
            return false;
        }
        stride = stride.saturating_mul(extent.0);
    }
    true
}

/// The number of elements in a shape. Saturating; see `row_major_strides`.
fn element_count(shape: impl IntoIterator<Item = IndexRepr>) -> IndexRepr {
    shape.into_iter().fold(1, |count, extent| count.saturating_mul(extent))
}

/// The strides of a view reshaped to `new_shape`, written to `strides_out`,
/// in the order the view already has: row-major if it is, else
/// column-major. `false` if the view does not start its buffer, is not laid
/// out contiguously either way, or holds a different number of elements.
///
/// A reshape keeps the buffer and the offset of 0, so its elements are the
/// view's in the same order.
pub(crate) fn reshape_strides(
    offset: IndexRepr,
    shape: &[Index],
    strides: &[Index],
    new_shape: &[Index],
    strides_out: &mut [Index],
) -> bool {
    let row_major = is_row_major_contiguous(shape, strides);
    if offset != 0 || !(row_major || is_col_major_contiguous(shape, strides)) {
        return false;
    }
    if element_count(shape.iter().map(|e| e.0)) != element_count(new_shape.iter().map(|e| e.0)) {
        return false;
    }
    if row_major {
        row_major_strides(new_shape, strides_out);
    } else {
        col_major_strides(new_shape, strides_out);
    }
    true
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
    slice_len: rtdt::Index,
    element_tydesc_ref: TyDescRef,
    // u32 x rank
    shape_in: *mut u8,
    shape_tydesc_ref: TyDescRef,
    layout: u8,
    tensor_value_out: *mut u8,
    _tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        // An empty slice may have no address; the elements are only read
        // when there are some.
        if tensor_value_out.is_null() || shape_in.is_null()
            || (slice_ptr_ref.is_null() && slice_len != rtdt::Index::ZERO)
        {
            return RtStatus::Error;
        }

        // Extract shape from the moved-in List<u32>.
        let shape_list_ptr = shape_in as *mut rtdt::List;
        let shape_data = (*shape_list_ptr).data as *const u32;
        let rank = (*shape_list_ptr).size;

        if rank == rtdt::Index::ZERO || shape_data.is_null() {
            // Destroy the shape list before returning.
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Convert shape to slice.
        let shape_slice = std::slice::from_raw_parts(shape_data, rank.as_usize());

        // Compute total elements from shape.
        let mut total_elems: rtdt::IndexRepr = 1;
        for &dim in shape_slice {
            total_elems = total_elems.saturating_mul(dim as rtdt::IndexRepr);
        }

        // Validate slice length matches total elements.
        if slice_len.0 != total_elems {
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        let element_size = element_tydesc_ref.size();
        let element_align = element_tydesc_ref.align();

        // Allocate data buffer; an empty tensor has none, and keeps its shape.
        let data_ptr = if total_elems == 0 {
            std::ptr::null_mut()
        } else {
            let data_ptr = rt_ref.alloc.alloc(element_size, element_align, total_elems);
            if data_ptr.is_null() {
                let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
                return RtStatus::Error;
            }
            data_ptr
        };

        // Allocate shape array (Usize per dimension).
        let shape_array_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.0,
        ) as *mut rtdt::Index;
        if shape_array_ptr.is_null() {
            if !data_ptr.is_null() { rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr); }
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Allocate strides array (Usize per dimension).
        let strides_array_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.0,
        ) as *mut rtdt::Index;
        if strides_array_ptr.is_null() {
            if !data_ptr.is_null() { rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr); }
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                rank.0,
                shape_array_ptr as *mut u8,
            );
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
            return RtStatus::Error;
        }

        // Clone elements from slice to data buffer.
        for i in 0..total_elems {
            let src_ptr = slice_ptr_ref.add((i as usize) * (element_size as usize));
            let dest_ptr = data_ptr.add((i as usize) * (element_size as usize));

            let status = crate::c::impls::clone::clone_value(
                rt_ref,
                src_ptr,
                element_tydesc_ref.as_ref(),
                dest_ptr,
            );
            if status != RtStatus::Ok {
                // Clean up partially created tensor.
                // Destroy elements that were successfully cloned.
                for j in 0..i {
                    let elem_ptr = data_ptr.add((j as usize) * (element_size as usize));
                    let _ = crate::c::impls::destroy::any_destroy_local(
                        rt_ref,
                        elem_ptr,
                        element_tydesc_ref.as_ref(),
                    );
                }
                if !data_ptr.is_null() { rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr); }
                rt_ref.alloc.free(
                    rtdt::INDEX_SIZE,
                    rtdt::INDEX_ALIGN,
                    rank.0,
                    shape_array_ptr as *mut u8,
                );
                rt_ref.alloc.free(
                    rtdt::INDEX_SIZE,
                    rtdt::INDEX_ALIGN,
                    rank.0,
                    strides_array_ptr as *mut u8,
                );
                let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);
                return status;
            }
        }

        // Copy shape values (convert u32 to Usize).
        for i in 0..rank.as_usize() {
            *shape_array_ptr.add(i) = rtdt::Index(*shape_data.add(i) as rtdt::IndexRepr);
        }

        // Compute and write strides based on layout.
        let layout_enum = std::mem::transmute::<u8, rtdt::TensorLayout>(layout);
        let shape_written = std::slice::from_raw_parts(shape_array_ptr, rank.as_usize());
        let strides_out = std::slice::from_raw_parts_mut(strides_array_ptr, rank.as_usize());
        match layout_enum {
            rtdt::TensorLayout::RowMajor | rtdt::TensorLayout::ColMajorTransposed => {
                row_major_strides(shape_written, strides_out)
            }
            rtdt::TensorLayout::ColMajor | rtdt::TensorLayout::RowMajorTransposed => {
                col_major_strides(shape_written, strides_out)
            }
        }

        // Initialize Tensor struct.
        let tensor_ptr = tensor_value_out as *mut rtdt::Tensor;
        (*tensor_ptr).ptr_base = data_ptr;
        (*tensor_ptr).offset_elems = rtdt::Index::ZERO;
        (*tensor_ptr).capacity_elems = rtdt::Index(total_elems);
        (*tensor_ptr).shape = shape_array_ptr;
        (*tensor_ptr).strides = strides_array_ptr;
        (*tensor_ptr).layout = layout_enum;

        // Destroy the moved-in shape list (we've copied its data).
        let _ = crate::c::impls::list::list_destroy_impl(rt_ref, shape_in, shape_tydesc_ref);

        RtStatus::Ok
    }
}

/// Initialize a tensor from contiguous element data and raw shape array.
///
/// This is a simpler interface for AOT compilation. The element data is moved
/// (not cloned) into the tensor's internal buffer.
pub unsafe fn tensor_init_impl(
    rt_ref: &mut RtLocal,
    element_data_in: *mut u8,
    element_count: rtdt::Index,
    element_tydesc_ref: TyDescRef,
    shape_ptr: *const u32,
    rank: u32,
    tensor_value_out: *mut u8,
    tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        if tensor_value_out.is_null() {
            return RtStatus::Error;
        }

        // A tensor has at least one axis; the language has no rank-0 tensors.
        assert!(rank > 0, "a tensor has at least one axis");
        if shape_ptr.is_null() {
            return RtStatus::Error;
        }

        // Verify rank matches tensor type.
        let expected_rank = tensor_tydesc_ref.tensor_rank();
        if rank != expected_rank {
            return RtStatus::Error;
        }

        // Compute total elements from shape.
        let shape_slice = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let mut total_elems: rtdt::IndexRepr = 1;
        for &dim in shape_slice {
            total_elems = total_elems.saturating_mul(dim as rtdt::IndexRepr);
        }

        // Validate element count matches shape.
        if element_count.0 != total_elems {
            return RtStatus::Error;
        }

        let element_size = element_tydesc_ref.size();
        let element_align = element_tydesc_ref.align();

        // An empty tensor has no data to hold, and keeps its shape all the
        // same: a 0x3 is not a 3x0.
        let data_ptr = if total_elems == 0 {
            std::ptr::null_mut()
        } else {
            if element_data_in.is_null() {
                return RtStatus::Error;
            }
            // Allocate data buffer and copy elements (move semantics - just memcpy).
            let data_ptr = rt_ref.alloc.alloc(element_size, element_align, total_elems);
            if data_ptr.is_null() {
                return RtStatus::Error;
            }
            std::ptr::copy_nonoverlapping(
                element_data_in,
                data_ptr,
                (total_elems as usize) * (element_size as usize),
            );
            data_ptr
        };

        // Allocate shape array (Usize per dimension).
        let shape_array_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
        ) as *mut rtdt::Index;
        if shape_array_ptr.is_null() {
            if !data_ptr.is_null() {
                rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr);
            }
            return RtStatus::Error;
        }
        // Convert u32 shape values to Usize.
        for i in 0..rank as usize {
            *shape_array_ptr.add(i) = rtdt::Index(*shape_ptr.add(i) as rtdt::IndexRepr);
        }

        // Allocate strides array (Usize per dimension).
        let strides_array_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
        ) as *mut rtdt::Index;
        if strides_array_ptr.is_null() {
            if !data_ptr.is_null() {
                rt_ref.alloc.free(element_size, element_align, total_elems, data_ptr);
            }
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                rank.into(),
                shape_array_ptr as *mut u8,
            );
            return RtStatus::Error;
        }

        row_major_strides(
            std::slice::from_raw_parts(shape_array_ptr, rank as usize),
            std::slice::from_raw_parts_mut(strides_array_ptr, rank as usize),
        );

        // Initialize Tensor struct.
        let tensor_ptr = tensor_value_out as *mut rtdt::Tensor;
        (*tensor_ptr).ptr_base = data_ptr;
        (*tensor_ptr).offset_elems = rtdt::Index::ZERO;
        (*tensor_ptr).capacity_elems = rtdt::Index(total_elems);
        (*tensor_ptr).shape = shape_array_ptr;
        (*tensor_ptr).strides = strides_array_ptr;
        (*tensor_ptr).layout = rtdt::TensorLayout::RowMajor;

        RtStatus::Ok
    }
}

/// Gets an element at the specified indices by cloning it.
///
/// An index past the end of its axis is `Error`.
pub unsafe fn tensor_get_impl(
    rt_ref: &mut RtLocal,
    tensor_value_ref: *const u8,
    tensor_tydesc_ref: TyDescRef,
    indices_ptr: *const rtdt::IndexRepr,
    element_value_out: *mut u8,
) -> RtStatus {
    unsafe {
        let Some(element_ptr) = tensor_element_slot(tensor_value_ref as *mut u8, tensor_tydesc_ref, indices_ptr) else {
            return RtStatus::Error;
        };
        crate::c::impls::clone::clone_value(
            rt_ref,
            element_ptr,
            tensor_tydesc_ref.tensor_element_ty().as_ref(),
            element_value_out,
        )
    }
}

/// Where the element at `indices` is, or `None` if an index is past the end
/// of its axis.
///
/// `indices` has one entry per axis.
pub unsafe fn tensor_element_slot(
    tensor_value_ref: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    indices_ptr: *const rtdt::IndexRepr,
) -> Option<*mut u8> {
    unsafe {
        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let rank = tensor_tydesc_ref.tensor_rank();

        let tensor_ptr = tensor_value_ref as *mut rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape;
        let strides_ptr = (*tensor_ptr).strides;

        // These are programming errors (use-after-destroy, use-after-move).
        assert!(!ptr_base.is_null(), "tensor element: ptr_base is null");
        assert!(!shape_ptr.is_null(), "tensor element: shape is null");
        assert!(!strides_ptr.is_null(), "tensor element: strides is null");
        assert!(rank > 0, "tensor element: rank is 0");

        let indices_slice = std::slice::from_raw_parts(indices_ptr, rank as usize);
        let shape_slice = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides_slice = std::slice::from_raw_parts(strides_ptr, rank as usize);

        let linear_offset = element_position(offset_elems.0, shape_slice, strides_slice, indices_slice)?;

        let element_size = element_ty.size() as usize;
        Some((ptr_base as *mut u8).add(linear_offset as usize * element_size))
    }
}

/// Replace the element at the specified indices, moving the new value in.
///
/// Destroys the old value. An index past the end of its axis is `Error`, with
/// the tensor as it was and the value destroyed: it is taken either way.
pub unsafe fn tensor_set_impl(
    rt_ref: &mut RtLocal,
    tensor_value_ref: *mut u8,
    tensor_tydesc_ref: TyDescRef,
    indices_ptr: *const rtdt::IndexRepr,
    value_in: *mut u8,
) -> RtStatus {
    unsafe {
        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let Some(element_ptr) = tensor_element_slot(tensor_value_ref, tensor_tydesc_ref, indices_ptr) else {
            crate::c::impls::destroy::any_destroy_local(rt_ref, value_in, element_ty.as_ref());
            return RtStatus::Error;
        };

        let status = crate::c::impls::destroy::any_destroy_local(
            rt_ref,
            element_ptr,
            element_ty.as_ref(),
        );
        if status != RtStatus::Ok {
            return status;
        }

        std::ptr::copy_nonoverlapping(value_in as *const u8, element_ptr, element_ty.size() as usize);
        RtStatus::Ok
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

        let tensor_in_ptr = tensor_value_in as *mut rtdt::Tensor;
        let tensor_out_ptr = tensor_value_out as *mut rtdt::Tensor;

        let ptr_base = (*tensor_in_ptr).ptr_base;
        let offset_elems = (*tensor_in_ptr).offset_elems;
        let capacity_elems = (*tensor_in_ptr).capacity_elems;
        let shape_in_ptr = (*tensor_in_ptr).shape;
        let strides_in_ptr = (*tensor_in_ptr).strides;
        let layout_in = (*tensor_in_ptr).layout;

        // These are programming errors (use-after-destroy, use-after-move).
        assert!(!ptr_base.is_null(), "tensor_transpose: ptr_base is null");
        assert!(!shape_in_ptr.is_null(), "tensor_transpose: shape is null");
        assert!(!strides_in_ptr.is_null(), "tensor_transpose: strides is null");

        // Validate permutation.
        let perm_slice = std::slice::from_raw_parts(perm_ptr, rank as usize);
        if !is_permutation(perm_slice) {
            return RtStatus::Error;
        }

        // Allocate new shape array (Usize per dimension).
        let shape_out_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
        ) as *mut rtdt::Index;
        if shape_out_ptr.is_null() {
            return RtStatus::Error;
        }

        // Allocate new strides array (Usize per dimension).
        let strides_out_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
        ) as *mut rtdt::Index;
        if strides_out_ptr.is_null() {
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                rank.into(),
                shape_out_ptr as *mut u8,
            );
            return RtStatus::Error;
        }

        let permuted = transpose_view(
            std::slice::from_raw_parts(shape_in_ptr, rank as usize),
            std::slice::from_raw_parts(strides_in_ptr, rank as usize),
            perm_slice,
            std::slice::from_raw_parts_mut(shape_out_ptr, rank as usize),
            std::slice::from_raw_parts_mut(strides_out_ptr, rank as usize),
        );
        debug_assert!(permuted, "the permutation was checked above");

        // Determine output layout.
        // For 2D transpose ([1, 0]): RowMajor <-> ColMajorTransposed, ColMajor <-> RowMajorTransposed
        let layout_out = if rank == 2 && perm_slice == [1, 0] {
            match layout_in {
                rtdt::TensorLayout::RowMajor => rtdt::TensorLayout::ColMajorTransposed,
                rtdt::TensorLayout::ColMajor => rtdt::TensorLayout::RowMajorTransposed,
                rtdt::TensorLayout::RowMajorTransposed => rtdt::TensorLayout::ColMajor,
                rtdt::TensorLayout::ColMajorTransposed => rtdt::TensorLayout::RowMajor,
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
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
            shape_in_ptr as *mut u8,
        );
        rt_ref.alloc.free(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
            strides_in_ptr as *mut u8,
        );

        // Clear input tensor (linear type system: ownership transferred).
        (*tensor_in_ptr).ptr_base = std::ptr::null_mut();
        (*tensor_in_ptr).offset_elems = rtdt::Index::ZERO;
        (*tensor_in_ptr).capacity_elems = rtdt::Index::ZERO;
        (*tensor_in_ptr).shape = std::ptr::null();
        (*tensor_in_ptr).strides = std::ptr::null();

        RtStatus::Ok
    }
}

/// Destroys a tensor, freeing all three allocations.
///
/// Frees the shape array, strides array, and data buffer.
/// Views (capacity_elems == 0) are no-ops: they don't own the data.
pub unsafe fn tensor_destroy_impl(
    rt_ref: &mut RtLocal,
    tensor_value_in: *mut u8,
    tensor_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        if tensor_value_in.is_null() {
            return RtStatus::Error;
        }

        let tensor_ptr = tensor_value_in as *mut rtdt::Tensor;
        let capacity_elems = (*tensor_ptr).capacity_elems;

        // Views (capacity_elems == 0) don't own any allocations.
        if capacity_elems == rtdt::Index::ZERO {
            (*tensor_ptr).ptr_base = std::ptr::null_mut();
            (*tensor_ptr).offset_elems = rtdt::Index::ZERO;
            (*tensor_ptr).shape = std::ptr::null();
            (*tensor_ptr).strides = std::ptr::null();
            return RtStatus::Ok;
        }

        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let rank = tensor_tydesc_ref.tensor_rank();

        let ptr_base = (*tensor_ptr).ptr_base;
        let shape_ptr = (*tensor_ptr).shape as *mut rtdt::Index;
        let strides_ptr = (*tensor_ptr).strides as *mut rtdt::Index;

        // Compute total elements from shape.
        let mut total_elems: rtdt::IndexRepr = 1;
        if !shape_ptr.is_null() && rank > 0 {
            for i in 0..rank {
                let dim = (*shape_ptr.add(i as usize)).0;
                total_elems = total_elems.saturating_mul(dim);
            }
        }

        // Destroy all elements in the data buffer.
        if !ptr_base.is_null() && total_elems > 0 {
            let element_size = element_ty.size() as usize;

            for i in 0..total_elems {
                let element_ptr = ptr_base.add(i as usize * element_size);
                let status = crate::c::impls::destroy::any_destroy_local(
                    rt_ref,
                    element_ptr,
                    element_ty.as_ref(),
                );
                if status != RtStatus::Ok {
                    return status;
                }
            }
        }

        // Free the data buffer.
        if !ptr_base.is_null() {
            rt_ref.alloc.free(
                element_ty.size(),
                element_ty.align(),
                capacity_elems.0,
                ptr_base,
            );
        }

        // Free the shape array.
        if !shape_ptr.is_null() && rank > 0 {
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                rank.into(),
                shape_ptr as *mut u8,
            );
        }

        // Free the strides array.
        if !strides_ptr.is_null() && rank > 0 {
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                rank.into(),
                strides_ptr as *mut u8,
            );
        }

        // Reset the tensor.
        (*tensor_ptr).ptr_base = std::ptr::null_mut();
        (*tensor_ptr).offset_elems = rtdt::Index::ZERO;
        (*tensor_ptr).capacity_elems = rtdt::Index::ZERO;
        (*tensor_ptr).shape = std::ptr::null();
        (*tensor_ptr).strides = std::ptr::null();

        RtStatus::Ok
    }
}

/// Deep-clone a hyperplane (axis-0 slice) of a tensor into a new owned tensor.
///
/// Given a tensor and an axis-0 index, produces an owned sub-tensor with
/// shape[1..], contiguous row-major layout. Elements are cloned from the
/// strided sub-region.
pub unsafe fn tensor_hyperplane_clone_impl(
    rt_ref: &mut RtLocal,
    tensor_value_ref: *const u8,
    tensor_tydesc_ref: TyDescRef,
    axis0_index: rtdt::IndexRepr,
    sub_tensor_out: *mut u8,
) -> RtStatus {
    unsafe {
        if tensor_value_ref.is_null() || sub_tensor_out.is_null() {
            return RtStatus::Error;
        }

        let element_ty = tensor_tydesc_ref.tensor_element_ty();
        let rank = tensor_tydesc_ref.tensor_rank();
        assert!(rank >= 2, "tensor_hyperplane_clone: rank must be >= 2");

        let tensor_ptr = tensor_value_ref as *const rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape;
        let strides_ptr = (*tensor_ptr).strides;

        assert!(!ptr_base.is_null(), "tensor_hyperplane_clone: ptr_base is null");
        assert!(!shape_ptr.is_null(), "tensor_hyperplane_clone: shape is null");
        assert!(!strides_ptr.is_null(), "tensor_hyperplane_clone: strides is null");

        // Bounds check.
        let dim0 = (*shape_ptr).0;
        if axis0_index >= dim0 {
            return RtStatus::Error;
        }

        let sub_rank = rank - 1;
        let element_size = element_ty.size();
        let element_align = element_ty.align();

        // Compute sub-tensor total elements from shape[1..].
        let mut sub_total: rtdt::IndexRepr = 1;
        for i in 1..rank as usize {
            sub_total = sub_total.saturating_mul((*shape_ptr.add(i)).0);
        }

        // Allocate data buffer for the new contiguous sub-tensor.
        let new_data = rt_ref.alloc.alloc(element_size, element_align, sub_total);
        if new_data.is_null() {
            return RtStatus::Error;
        }

        // Allocate shape array for sub-tensor.
        let new_shape = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            sub_rank.into(),
        ) as *mut rtdt::Index;
        if new_shape.is_null() {
            rt_ref.alloc.free(element_size, element_align, sub_total, new_data);
            return RtStatus::Error;
        }

        // Allocate strides array for sub-tensor.
        let new_strides = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            sub_rank.into(),
        ) as *mut rtdt::Index;
        if new_strides.is_null() {
            rt_ref.alloc.free(element_size, element_align, sub_total, new_data);
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                sub_rank.into(),
                new_shape as *mut u8,
            );
            return RtStatus::Error;
        }

        // The copy is laid out row-major, whatever the source's strides.
        let shape = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides = std::slice::from_raw_parts(strides_ptr, rank as usize);
        let (sub_shape, sub_strides) = (&shape[1..], &strides[1..]);
        std::ptr::copy_nonoverlapping(sub_shape.as_ptr(), new_shape, sub_rank as usize);
        row_major_strides(sub_shape, std::slice::from_raw_parts_mut(new_strides, sub_rank as usize));

        // Clone the plane's elements from the source, in row-major order.
        let plane_offset = hyperplane_offset(offset_elems.0, shape, strides, axis0_index)
            .expect("the index was checked against the first axis above");
        let elem_size_usize = element_size as usize;

        let mut dest_idx: rtdt::IndexRepr = 0;
        let mut indices: Vec<rtdt::IndexRepr> = vec![0; sub_rank as usize];
        loop {
            let src_offset = element_position(plane_offset, sub_shape, sub_strides, &indices)
                .expect("the multi-index steps within the plane's shape");

            let src_ptr = ptr_base.add(src_offset as usize * elem_size_usize);
            let dst_ptr = new_data.add(dest_idx as usize * elem_size_usize);

            let status = crate::c::impls::clone::clone_value(
                rt_ref,
                src_ptr,
                element_ty.as_ref(),
                dst_ptr,
            );
            if status != RtStatus::Ok {
                // Clean up partially cloned elements.
                for j in 0..dest_idx {
                    let elem_ptr = new_data.add(j as usize * elem_size_usize);
                    let _ = crate::c::impls::destroy::any_destroy_local(
                        rt_ref,
                        elem_ptr,
                        element_ty.as_ref(),
                    );
                }
                rt_ref.alloc.free(element_size, element_align, sub_total, new_data);
                rt_ref.alloc.free(
                    rtdt::INDEX_SIZE,
                    rtdt::INDEX_ALIGN,
                    sub_rank.into(),
                    new_shape as *mut u8,
                );
                rt_ref.alloc.free(
                    rtdt::INDEX_SIZE,
                    rtdt::INDEX_ALIGN,
                    sub_rank.into(),
                    new_strides as *mut u8,
                );
                return status;
            }

            dest_idx += 1;

            // Advance multi-index (row-major order).
            let mut carry = true;
            for d in (0..sub_rank as usize).rev() {
                if carry {
                    indices[d] += 1;
                    if indices[d] < sub_shape[d].0 {
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

        // Initialize the output sub-tensor.
        let out_ptr = sub_tensor_out as *mut rtdt::Tensor;
        (*out_ptr).ptr_base = new_data;
        (*out_ptr).offset_elems = rtdt::Index::ZERO;
        (*out_ptr).capacity_elems = rtdt::Index(sub_total);
        (*out_ptr).shape = new_shape;
        (*out_ptr).strides = new_strides;
        (*out_ptr).layout = rtdt::TensorLayout::RowMajor;

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
    ranges_ptr: *const rtdt::SliceRange,
    result_value_out: *mut u8,
    result_tydesc_ref: TyDescRef,
) -> RtStatus {
    unsafe {
        // Null pointer checks.
        if tensor_value_in.is_null() || ranges_ptr.is_null() || result_value_out.is_null() {
            return RtStatus::Error;
        }

        // Extract tensor fields.
        let tensor_ptr = tensor_value_in as *mut rtdt::Tensor;
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

        // Every range must be within its axis and not empty.
        if ranges.iter().zip(shape).any(|(range, extent)| range.start >= range.end || range.end > extent.0) {
            return error_path(
                rt_ref,
                tensor_value_in,
                tensor_tydesc_ref,
                result_value_out,
                result_tydesc_ref,
            );
        }

        // Allocate new shape array (Usize per dimension).
        let new_shape_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
        ) as *mut rtdt::Index;

        if new_shape_ptr.is_null() {
            return error_path(
                rt_ref,
                tensor_value_in,
                tensor_tydesc_ref,
                result_value_out,
                result_tydesc_ref,
            );
        }

        // Allocate new strides array (Usize per dimension).
        let new_strides_ptr = rt_ref.alloc.alloc(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
        ) as *mut rtdt::Index;

        if new_strides_ptr.is_null() {
            // Clean up shape allocation.
            rt_ref.alloc.free(
                rtdt::INDEX_SIZE,
                rtdt::INDEX_ALIGN,
                rank.into(),
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

        let new_offset = slice_view(
            offset_elems.0,
            shape,
            strides,
            ranges,
            std::slice::from_raw_parts_mut(new_shape_ptr, rank as usize),
        ).expect("the ranges were checked above");
        std::ptr::copy_nonoverlapping(strides.as_ptr(), new_strides_ptr, rank as usize);

        // Free old shape and strides arrays.
        rt_ref.alloc.free(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
            shape_ptr as *mut u8,
        );
        rt_ref.alloc.free(
            rtdt::INDEX_SIZE,
            rtdt::INDEX_ALIGN,
            rank.into(),
            strides_ptr as *mut u8,
        );

        // Construct output tensor.
        let out_tensor_ptr = compute_result_payload_ptr(result_value_out, result_tydesc_ref)
            as *mut rtdt::Tensor;

        let out_tensor = rtdt::Tensor {
            ptr_base,
            capacity_elems,
            offset_elems: rtdt::Index(new_offset),
            shape: new_shape_ptr,
            strides: new_strides_ptr,
            layout,
        };

        std::ptr::write(out_tensor_ptr, out_tensor);

        // Set Result tag to Ok.
        let result_ptr = result_value_out as *mut rtdt::Result;
        std::ptr::write(&mut (*result_ptr).tag, rtdt::ResultTag::Ok);

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
                let tensor_src = tensor_value_in as *const rtdt::Tensor;
                let tensor_dst = payload_ptr as *mut rtdt::Tensor;
                let tensor_value = std::ptr::read(tensor_src);
                std::ptr::write(tensor_dst, tensor_value);

                // Set Result tag to Err.
                let result_ptr = result_value_out as *mut rtdt::Result;
                std::ptr::write(&mut (*result_ptr).tag, rtdt::ResultTag::Err);

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
                let payload_offset = rtdt::layout::result_payload_offset(ok_align);

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
        let new_shape_list_ptr = new_shape_in as *mut rtdt::List;
        let new_shape_data = (*new_shape_list_ptr).data as *const u32;
        let new_rank = (*new_shape_list_ptr).size;

        // Validate new rank.
        if new_rank == rtdt::Index::ZERO || new_shape_data.is_null() {
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Extract tensor fields.
        let rank = tensor_tydesc_ref.tensor_rank();
        if rank == 0 {
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        let tensor_ptr = tensor_value_in as *mut rtdt::Tensor;
        let ptr_base = (*tensor_ptr).ptr_base;
        let capacity_elems = (*tensor_ptr).capacity_elems;
        let offset_elems = (*tensor_ptr).offset_elems;
        let shape_ptr = (*tensor_ptr).shape;
        let strides_ptr = (*tensor_ptr).strides;
        let layout = (*tensor_ptr).layout;

        // Create slices for validation.
        let shape = std::slice::from_raw_parts(shape_ptr, rank as usize);
        let strides = std::slice::from_raw_parts(strides_ptr, rank as usize);
        let new_shape = std::slice::from_raw_parts(new_shape_data, new_rank.as_usize());

        // The view must start its buffer, be laid out contiguously and hold
        // as many elements as the new shape.
        let new_shape_indices: Vec<rtdt::Index> = new_shape.iter().map(|&e| rtdt::Index(e as rtdt::IndexRepr)).collect();
        let mut new_strides_vec = vec![rtdt::Index::ZERO; new_shape.len()];
        if !reshape_strides(offset_elems.0, shape, strides, &new_shape_indices, &mut new_strides_vec) {
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Allocate new strides array (Usize per dimension).
        let new_strides_ptr = rt_ref.alloc.alloc(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, new_rank.0);
        if new_strides_ptr.is_null() {
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        // Copy new strides into allocated array.
        let new_strides_ptr_typed = new_strides_ptr as *mut rtdt::Index;
        for (i, &stride) in new_strides_vec.iter().enumerate() {
            std::ptr::write(new_strides_ptr_typed.add(i), stride);
        }

        // Allocate new shape array (Usize per dimension).
        let new_shape_alloc_ptr = rt_ref.alloc.alloc(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, new_rank.0);
        if new_shape_alloc_ptr.is_null() {
            // Free new_strides before returning.
            rt_ref.alloc.free(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, new_rank.0, new_strides_ptr);
            let _ = crate::c::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);
            return error_path(rt_ref, tensor_value_in, tensor_tydesc_ref, result_value_out, result_tydesc_ref);
        }

        let new_shape_ptr_typed = new_shape_alloc_ptr as *mut rtdt::Index;
        std::ptr::copy_nonoverlapping(new_shape_indices.as_ptr(), new_shape_ptr_typed, new_rank.as_usize());

        // Free old strides array.
        rt_ref.alloc.free(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into(), strides_ptr as *mut u8);

        // Free old shape array.
        rt_ref.alloc.free(rtdt::INDEX_SIZE, rtdt::INDEX_ALIGN, rank.into(), shape_ptr as *mut u8);

        // Destroy the new_shape List (including its data).
        let _ = crate::c::impls::list::list_destroy_impl(rt_ref, new_shape_in, new_shape_tydesc_ref);

        // Construct output tensor.
        let payload_ptr = compute_result_payload_ptr(result_value_out, result_tydesc_ref);
        let out_tensor_ptr = payload_ptr as *mut rtdt::Tensor;

        let out_tensor = rtdt::Tensor {
            ptr_base,
            capacity_elems,
            offset_elems: rtdt::Index::ZERO,
            shape: new_shape_ptr_typed,
            strides: new_strides_ptr_typed,
            layout,
        };

        std::ptr::write(out_tensor_ptr, out_tensor);

        // Set Result tag to Ok.
        let result_ptr = result_value_out as *mut rtdt::Result;
        std::ptr::write(&mut (*result_ptr).tag, rtdt::ResultTag::Ok);

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
                let tensor_src = tensor_value_in as *const rtdt::Tensor;
                let tensor_dst = payload_ptr as *mut rtdt::Tensor;
                let tensor_value = std::ptr::read(tensor_src);
                std::ptr::write(tensor_dst, tensor_value);

                // Set Result tag to Err.
                let result_ptr = result_value_out as *mut rtdt::Result;
                std::ptr::write(&mut (*result_ptr).tag, rtdt::ResultTag::Err);

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
                let payload_offset = rtdt::layout::result_payload_offset(ok_align);

                result_ptr.add(payload_offset as usize)
            }
        }
    }
}

#[cfg(kani)]
mod kani_proofs {
    //! Bounded proofs that each new view picks out the elements of the old
    //! one it should; see botdocs/kani.md.
    //!
    //! Views are of rank one to three, with extents to 7, strides to 255 and
    //! offsets to 65535: far enough apart that every axis is distinct, and
    //! small enough that no position can overflow, which is what being well
    //! formed in a buffer promises. The loops run at most three times.
    //!
    //! Each value is drawn as a narrow integer and widened, not drawn as an
    //! index and bounded by an assumption. Widened, its high bits are
    //! constant zeros the solver folds away, and a product of two is a small
    //! multiplier; bounded, every bit is still symbolic, and the slice
    //! harness did not finish in ten minutes.

    use super::*;

    const MAX_RANK: usize = 3;

    /// A view's offset, shape and strides, and its rank.
    fn any_view(min_rank: usize) -> (IndexRepr, [Index; MAX_RANK], [Index; MAX_RANK], usize) {
        let rank: usize = kani::any();
        kani::assume(min_rank <= rank && rank <= MAX_RANK);
        let offset = kani::any::<u16>() as IndexRepr;
        let mut shape = [Index::ZERO; MAX_RANK];
        let mut strides = [Index::ZERO; MAX_RANK];
        for axis in 0..MAX_RANK {
            shape[axis] = Index((kani::any::<u8>() & 7) as IndexRepr);
            strides[axis] = Index(kani::any::<u8>() as IndexRepr);
        }
        (offset, shape, strides, rank)
    }

    /// Any index within `shape`.
    fn any_index_within(shape: &[Index]) -> [IndexRepr; MAX_RANK] {
        let mut index = [0; MAX_RANK];
        for (i, extent) in index.iter_mut().zip(shape) {
            *i = (kani::any::<u8>() & 7) as IndexRepr;
            kani::assume(*i < extent.0);
        }
        index
    }

    /// A slice is valid exactly when every range is within its axis and not
    /// empty, and then its element at any index is the view's at that index
    /// plus the ranges' starts.
    ///
    /// To rank two, where the other harnesses go to three: the property is
    /// that `(start + j) * stride` is `start * stride + j * stride` on every
    /// axis at once, which is a multiplier equivalence per axis. It takes 13
    /// seconds at rank one and 70 at two, and did not finish in ten minutes
    /// at three. Each axis adds its own product, so two already shows that
    /// one axis's slice does not disturb another's.
    #[kani::proof]
    #[kani::unwind(4)]
    fn a_slice_picks_out_the_views_elements() {
        let (offset, shape, strides, rank) = any_view(1);
        kani::assume(rank <= 2);
        let mut ranges = [rtdt::SliceRange { start: 0, end: 0 }; MAX_RANK];
        for range in ranges.iter_mut() {
            let (start, end) = (kani::any::<u8>(), kani::any::<u8>());
            *range = rtdt::SliceRange { start: start as IndexRepr, end: end as IndexRepr };
        }
        let (shape, strides, ranges) = (&shape[..rank], &strides[..rank], &ranges[..rank]);
        let mut sliced_shape = [Index::ZERO; MAX_RANK];
        let sliced_shape = &mut sliced_shape[..rank];

        let sliced = slice_view(offset, shape, strides, ranges, sliced_shape);
        let mut valid = true;
        for (range, extent) in ranges.iter().zip(shape) {
            valid &= range.start < range.end && range.end <= extent.0;
        }
        assert!(sliced.is_some() == valid);

        if let Some(sliced_offset) = sliced {
            let index = any_index_within(sliced_shape);
            let mut in_view = [0; MAX_RANK];
            for axis in 0..rank {
                in_view[axis] = ranges[axis].start + index[axis];
            }
            let element = element_position(sliced_offset, sliced_shape, strides, &index[..rank]);
            assert!(element.is_some());
            assert!(element == element_position(offset, shape, strides, &in_view[..rank]));
        }
    }

    /// A transpose is valid exactly when every axis is named once, and then
    /// its element at any index is the view's at that index with the axes
    /// put back where `perm` took them from.
    #[kani::proof]
    #[kani::unwind(4)]
    fn a_transpose_reorders_the_views_axes() {
        let (offset, shape, strides, rank) = any_view(1);
        let perm: [u32; MAX_RANK] = kani::any();
        let (shape, strides, perm) = (&shape[..rank], &strides[..rank], &perm[..rank]);
        let mut shape_out = [Index::ZERO; MAX_RANK];
        let mut strides_out = [Index::ZERO; MAX_RANK];
        let (shape_out, strides_out) = (&mut shape_out[..rank], &mut strides_out[..rank]);

        let transposed = transpose_view(shape, strides, perm, shape_out, strides_out);
        let mut named_once = true;
        for axis in 0..rank as u32 {
            named_once &= perm.iter().filter(|&&p| p == axis).count() == 1;
        }
        assert!(transposed == named_once);

        if transposed {
            let index = any_index_within(shape_out);
            let mut in_view = [0; MAX_RANK];
            for axis in 0..rank {
                in_view[perm[axis] as usize] = index[axis];
            }
            let element = element_position(offset, shape_out, strides_out, &index[..rank]);
            assert!(element.is_some());
            assert!(element == element_position(offset, shape, strides, &in_view[..rank]));
        }
    }

    /// The hyperplane at an index along the first axis exists exactly when
    /// the index is within it, and then its element at any index is the
    /// view's at that index with the plane's index in front.
    #[kani::proof]
    #[kani::unwind(4)]
    fn a_hyperplane_is_the_views_elements_at_its_index() {
        let (offset, shape, strides, rank) = any_view(2);
        let (shape, strides) = (&shape[..rank], &strides[..rank]);
        let at = kani::any::<u8>() as IndexRepr;

        let plane = hyperplane_offset(offset, shape, strides, at);
        assert!(plane.is_some() == (at < shape[0].0));

        if let Some(plane_offset) = plane {
            let index = any_index_within(&shape[1..]);
            let mut in_view = [0; MAX_RANK];
            in_view[0] = at;
            for axis in 1..rank {
                in_view[axis] = index[axis - 1];
            }
            let element = element_position(plane_offset, &shape[1..], &strides[1..], &index[..rank - 1]);
            assert!(element.is_some());
            assert!(element == element_position(offset, shape, strides, &in_view[..rank]));
        }
    }

    /// Row-major strides number a shape's elements in row-major order from
    /// 0, each below the element count, and column-major strides in
    /// column-major order; each is recognized as what it is.
    ///
    /// The orders are stated by Horner's rule, not by strides, so they are
    /// not the code under proof written again.
    #[kani::proof]
    #[kani::unwind(4)]
    fn contiguous_strides_number_the_elements_in_order() {
        let (_, shape, _, rank) = any_view(1);
        let shape = &shape[..rank];
        let index = any_index_within(shape);
        let index = &index[..rank];
        let count = element_count(shape.iter().map(|e| e.0));

        let mut strides = [Index::ZERO; MAX_RANK];
        let strides = &mut strides[..rank];
        row_major_strides(shape, strides);
        let mut row_order = 0;
        for (i, extent) in index.iter().zip(shape) {
            row_order = row_order * extent.0 + i;
        }
        assert!(element_position(0, shape, strides, index) == Some(row_order));
        assert!(row_order < count);
        assert!(is_row_major_contiguous(shape, strides));

        col_major_strides(shape, strides);
        let mut col_order = 0;
        for (i, extent) in index.iter().zip(shape).rev() {
            col_order = col_order * extent.0 + i;
        }
        assert!(element_position(0, shape, strides, index) == Some(col_order));
        assert!(is_col_major_contiguous(shape, strides));
    }

    /// A reshape is valid exactly when the view starts its buffer, is laid
    /// out row-major or column-major, and holds as many elements as the new
    /// shape, and then gives the new shape strides of the same order.
    ///
    /// With the harness above, the reshaped view numbers its elements in the
    /// order the old one did, from the same buffer: the element at each
    /// position is the one that was there.
    #[kani::proof]
    #[kani::unwind(4)]
    fn a_reshape_keeps_the_order_of_the_elements() {
        let (offset, shape, strides, rank) = any_view(1);
        let (_, new_shape, _, new_rank) = any_view(1);
        let (shape, strides, new_shape) = (&shape[..rank], &strides[..rank], &new_shape[..new_rank]);
        let mut new_strides = [Index::ZERO; MAX_RANK];
        let new_strides = &mut new_strides[..new_rank];

        let reshaped = reshape_strides(offset, shape, strides, new_shape, new_strides);

        let mut row = [Index::ZERO; MAX_RANK];
        let mut col = [Index::ZERO; MAX_RANK];
        row_major_strides(shape, &mut row[..rank]);
        col_major_strides(shape, &mut col[..rank]);
        let was_row = strides == &row[..rank];
        let was_col = strides == &col[..rank];
        let same_count = element_count(shape.iter().map(|e| e.0)) == element_count(new_shape.iter().map(|e| e.0));
        assert!(reshaped == (offset == 0 && (was_row || was_col) && same_count));

        if reshaped {
            let mut expected = [Index::ZERO; MAX_RANK];
            if was_row {
                row_major_strides(new_shape, &mut expected[..new_rank]);
            } else {
                col_major_strides(new_shape, &mut expected[..new_rank]);
            }
            assert!(new_strides == &expected[..new_rank]);
        }
    }
}
