//! Cursors: walking a collection one element at a time.
//!
//! The protocol is begin, then advance until done. Lists and rank-1 tensors
//! walk with a `StrideCursor`, tensors of higher rank with a `RowCursor`, and
//! sets and maps with a `TreeCursor`, whose begin and advance live with the
//! trees in `set` and `btreemap`. Whether a walk is done and its current
//! element are the cursor's own fields, read by the caller.

use datalove_rtdt as rtdt;
use datalove_rtdt::{RowCursor, StrideCursor, Tensor, TyDescRef};

/// Starts a walk over a list at its first element.
pub unsafe fn list_cursor_begin_impl(
    list_ref: *const u8,
    list_tydesc: TyDescRef,
    cursor_out: *mut StrideCursor,
) {
    unsafe {
        let list = &*(list_ref as *const rtdt::List);
        cursor_out.write(StrideCursor {
            elem: list.data,
            stride: list_tydesc.list_element_ty().size() as usize,
            index: rtdt::Index::ZERO,
            len: list.size,
        });
    }
}

/// Starts a walk over a rank-1 tensor at its first element.
pub unsafe fn tensor_cursor_begin_impl(
    tensor_ref: *const u8,
    tensor_tydesc: TyDescRef,
    cursor_out: *mut StrideCursor,
) {
    unsafe {
        debug_assert_eq!(tensor_tydesc.tensor_rank(), 1, "a stride cursor walks a rank-1 tensor");
        let tensor = &*(tensor_ref as *const Tensor);
        let size = tensor_tydesc.tensor_element_ty().size() as usize;
        cursor_out.write(StrideCursor {
            elem: tensor.ptr_base.wrapping_add(tensor.offset_elems.0 as usize * size),
            stride: ((*tensor.strides).0 as usize).wrapping_mul(size),
            index: rtdt::Index::ZERO,
            len: *tensor.shape,
        });
    }
}

/// Moves a walk with a stride cursor to the next element, or to done after
/// the last.
pub unsafe fn stride_cursor_advance_impl(cursor_mut: *mut StrideCursor) {
    unsafe {
        let cursor = &mut *cursor_mut;
        debug_assert!(cursor.index != cursor.len, "advancing a stride cursor that is done");
        cursor.elem = cursor.elem.wrapping_add(cursor.stride);
        cursor.index += rtdt::Index::ONE;
    }
}

/// Starts a walk over the rows of a tensor of rank above 1 at its first row.
pub unsafe fn tensor_row_cursor_begin_impl(
    tensor_ref: *const u8,
    tensor_tydesc: TyDescRef,
    cursor_out: *mut RowCursor,
) {
    unsafe {
        debug_assert!(tensor_tydesc.tensor_rank() > 1, "a row cursor walks a tensor of rank above 1");
        let tensor = &*(tensor_ref as *const Tensor);
        cursor_out.write(RowCursor {
            view: Tensor {
                ptr_base: tensor.ptr_base,
                capacity_elems: rtdt::Index::ZERO,
                offset_elems: tensor.offset_elems,
                shape: tensor.shape.add(1),
                strides: tensor.strides.add(1),
                layout: tensor.layout,
            },
            stride: *tensor.strides,
            index: rtdt::Index::ZERO,
            len: *tensor.shape,
        });
    }
}

/// Moves a walk with a row cursor to the next row, or to done after the last.
pub unsafe fn row_cursor_advance_impl(cursor_mut: *mut RowCursor) {
    unsafe {
        let cursor = &mut *cursor_mut;
        debug_assert!(cursor.index != cursor.len, "advancing a row cursor that is done");
        cursor.view.offset_elems = rtdt::Index(cursor.view.offset_elems.0.wrapping_add(cursor.stride.0));
        cursor.index += rtdt::Index::ONE;
    }
}
