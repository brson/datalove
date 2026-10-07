//! Runtime implementation of List (dynamic array).
//!
//! Uses a simple growable array structure with element-based capacity.

use crate::impls::rt_local::RtLocal;
use datalove_rtdt as rtdt;
use datalove_rtdt::*;
use crate::c::RtStatus;

// ============================================================================
// Core List Operations
// ============================================================================

/// Create an empty List.
pub unsafe fn list_create_impl(
    _rt: &mut RtLocal,
    value_out: *mut u8,
    _tydesc: rtdt::TyDescRef,
) -> RtStatus {
    // Initialize an empty list (null data, zero size and capacity).
    let list_ptr = value_out as *mut List;
    unsafe {
        (*list_ptr).data = std::ptr::null();
        (*list_ptr).size = rtdt::Index::ZERO;
        (*list_ptr).capacity = rtdt::Index::ZERO;
    }

    RtStatus::Ok
}

/// Create a List from a slice of elements (clones elements).
pub unsafe fn list_create_from_slice_impl(
    rt: &mut RtLocal,
    slice_ptr_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    element_tydesc: rtdt::TyDescRef,
    list_value_out: *mut u8,
    list_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    // Create empty list.
    let status = unsafe { list_create_impl(rt, list_value_out, list_tydesc) };
    if status != RtStatus::Ok {
        return status;
    }

    if slice_len == 0 {
        return RtStatus::Ok;
    }

    let mut list = unsafe { ListMut::new(list_value_out as *mut List, element_tydesc) };
    let element_size = element_tydesc.size();
    let element_align = element_tydesc.align();

    // Allocate buffer with exact capacity.
    let data = unsafe { rt.alloc.alloc(element_size, element_align, slice_len) };
    if data.is_null() {
        return RtStatus::Error;
    }

    list.set_data(data);
    list.set_capacity(slice_len);

    // Clone each element from the slice.
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    for i in 0..slice_len {
        let src_ptr = unsafe { slice_ptr_ref.add((i as usize) * (element_size as usize)) };
        let dest_ptr = unsafe { data.add((i as usize) * (element_size as usize)) };

        let status = unsafe {
            crate::impls::clone::clone_value(rt_handle, src_ptr, element_tydesc.as_ptr(), dest_ptr)
        };
        if status != RtStatus::Ok {
            // Clean up partially created list.
            list.set_size(i);
            let _ = unsafe { list_destroy_impl(rt, list_value_out, list_tydesc) };
            return status;
        }
    }

    list.set_size(slice_len);

    RtStatus::Ok
}

/// Build a List by moving elements from a contiguous buffer.
///
/// Unlike `list_create_from_slice_impl`, this moves elements rather than cloning.
/// The source buffer is consumed and should not be destroyed separately.
pub unsafe fn list_build_from_slice_impl(
    rt: &mut RtLocal,
    list_out: *mut u8,
    element_tydesc: rtdt::TyDescRef,
    elements_ptr: *mut u8,
    num_elements: rtdt::IndexRepr,
) -> RtStatus {
    let list_ptr = list_out as *mut List;

    if num_elements == 0 {
        // Initialize empty list.
        unsafe {
            (*list_ptr).data = std::ptr::null();
            (*list_ptr).size = rtdt::Index::ZERO;
            (*list_ptr).capacity = rtdt::Index::ZERO;
        }
        return RtStatus::Ok;
    }

    let element_size = element_tydesc.size();
    let element_align = element_tydesc.align();

    // Allocate buffer with exact capacity.
    let data = unsafe { rt.alloc.alloc(element_size, element_align, num_elements) };
    if data.is_null() {
        return RtStatus::Error;
    }

    // Move (memcpy) all elements from source to list buffer.
    // Source elements are consumed - no destruction needed.
    let total_bytes = (num_elements as usize) * (element_size as usize);
    unsafe {
        std::ptr::copy_nonoverlapping(elements_ptr, data, total_bytes);
    }

    // Initialize list structure.
    unsafe {
        (*list_ptr).data = data;
        (*list_ptr).size = rtdt::Index::new(num_elements);
        (*list_ptr).capacity = rtdt::Index::new(num_elements);
    }

    RtStatus::Ok
}

/// Destroy a List and free all elements and buffer.
pub unsafe fn list_destroy_impl(
    rt: &mut RtLocal,
    value_in: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(value_in as *mut List, element_ty) };
    let data_ptr = list.data_mut();
    let size = list.size();
    let capacity = list.capacity();

    // Destroy all elements.
    if !data_ptr.is_null() && size > 0 {
        let status = unsafe { destroy_elements(rt, data_ptr, element_ty, 0, size) };
        if status != RtStatus::Ok {
            return status;
        }
    }

    // Free the buffer.
    if !data_ptr.is_null() && capacity > 0 {
        unsafe { rt.alloc.free(element_ty.size(), element_ty.align(), capacity, data_ptr) };
    }

    // Reset the list.
    list.reset();

    RtStatus::Ok
}

/// Clear a List (destroy all elements and reset to empty, keeping buffer).
pub unsafe fn list_clear_impl(
    rt: &mut RtLocal,
    value_mut: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(value_mut as *mut List, element_ty) };
    let data_ptr = list.data_mut();
    let size = list.size();

    // Destroy all elements.
    if !data_ptr.is_null() && size > 0 {
        let status = unsafe { destroy_elements(rt, data_ptr, element_ty, 0, size) };
        if status != RtStatus::Ok {
            return status;
        }
    }

    // Reset size to 0, keeping capacity and buffer.
    list.set_size(0);

    RtStatus::Ok
}

// ============================================================================
// Element Access and Modification
// ============================================================================

/// Get an element at index.
///
/// Returns Option<T>:
/// - If index is valid, sets option to Some and clones the element.
/// - If index is out of bounds, sets option to None.
pub unsafe fn list_get_impl(
    rt: &mut RtLocal,
    list_value_ref: *const u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list = unsafe { ListRef::new(list_value_ref as *const List, element_ty) };
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    // Check bounds.
    if index >= list.size() {
        opt.write_none();
        return RtStatus::Ok;
    }

    // Clone element to option payload.
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::clone::clone_value(
            rt_handle,
            list.element_ptr(index),
            element_ty.as_ptr(),
            opt.payload_ptr(),
        )
    };

    if status != RtStatus::Ok {
        return status;
    }

    opt.write_some_tag();
    RtStatus::Ok
}

/// Set an element at index (replace existing element).
///
/// Returns Ok if successful, Error if index is out of bounds.
pub unsafe fn list_set_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    _list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    element_in: *mut u8,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let list = unsafe { ListMut::new(list_value_mut as *mut List, element_tydesc) };

    // Check bounds.
    if index >= list.size() {
        return RtStatus::Error;
    }

    // Destroy old element.
    let element_ptr = list.element_ptr_mut(index);
    let element_size = element_tydesc.size() as usize;

    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::destroy::any_destroy_local(rt_handle, element_ptr, element_tydesc.as_ptr())
    };
    if status != RtStatus::Ok {
        return status;
    }

    // Copy new element.
    unsafe { std::ptr::copy_nonoverlapping(element_in, element_ptr, element_size) };

    RtStatus::Ok
}

/// Where the next element of a list being filled in place goes: after its
/// last, where `list_reserve_impl` has made room.
pub(crate) unsafe fn list_end_slot(list_value: *mut u8, element_tydesc: rtdt::TyDescRef) -> *mut u8 {
    let list = unsafe { ListMut::new(list_value as *mut List, element_tydesc) };
    debug_assert!(list.size() < list.capacity(), "no room for the element");
    list.end_ptr()
}

/// Take in the element just written at `list_end_slot`.
pub(crate) unsafe fn list_count_last(list_value: *mut u8, element_tydesc: rtdt::TyDescRef) {
    let mut list = unsafe { ListMut::new(list_value as *mut List, element_tydesc) };
    list.set_size(list.size() + 1);
}

// ============================================================================
// Stack Operations (push/pop)
// ============================================================================

/// Push an element to the end of the list.
///
/// The element is moved into the list.
pub unsafe fn list_push_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    _list_tydesc: rtdt::TyDescRef,
    element_in: *mut u8,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let list_ptr = list_value_mut as *mut List;
    let mut list = unsafe { ListMut::new(list_ptr, element_tydesc) };

    // Grow if needed.
    if list.needs_grow() {
        let new_capacity = calculate_new_capacity(list.capacity(), list.size() + 1);
        let status = unsafe { grow_buffer(rt, list_ptr, element_tydesc, new_capacity) };
        if status != RtStatus::Ok {
            return status;
        }
    }

    // Copy element to end.
    let element_size = element_tydesc.size() as usize;
    unsafe { std::ptr::copy_nonoverlapping(element_in, list.end_ptr(), element_size) };

    list.set_size(list.size() + 1);

    RtStatus::Ok
}

/// Pop an element from the end of the list.
///
/// Returns Option<T>:
/// - If list is non-empty, sets option to Some and moves the element.
/// - If list is empty, sets option to None.
pub unsafe fn list_pop_impl(
    _rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if list.size() == 0 {
        opt.write_none();
        return RtStatus::Ok;
    }

    // Move last element to option payload.
    let last_index = list.size() - 1;
    let element_size = element_ty.size() as usize;
    unsafe {
        std::ptr::copy_nonoverlapping(
            list.element_ptr(last_index),
            opt.payload_ptr(),
            element_size,
        );
    }

    list.set_size(last_index);
    opt.write_some_tag();

    RtStatus::Ok
}

// ============================================================================
// Operations for a list whose element type is only known at runtime
// ============================================================================
//
// A generic function is compiled once, so it has no static type for the
// elements of the list it was handed and no descriptor for them either. What
// it does have is the list's own descriptor, which names the element type, and
// a static descriptor for `data`, which holds a value of any type along with
// what is needed to clone and drop it. So these carry elements in and out as
// `data`: the runtime reads the element type from the list and packs or
// unpacks against it, and the caller only ever names `data`.

/// The number of elements in a list.
///
/// The one list operation that needs nothing of the element type beyond its
/// size, which the descriptor gives.
pub unsafe fn list_len_impl(
    _rt: &mut RtLocal,
    list_value_ref: *const u8,
    list_tydesc: rtdt::TyDescRef,
    len_out: *mut u8,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list = unsafe { ListRef::new(list_value_ref as *const List, element_ty) };
    unsafe { std::ptr::write(len_out as *mut rtdt::Index, rtdt::Index(list.size())) };
    RtStatus::Ok
}

/// Read the element at `index`, as a `data`, leaving the list intact.
///
/// `option_tydesc` describes a `?data`, which a generic caller has statically
/// however little it knows about the elements.
pub unsafe fn list_get_as_data_impl(
    rt: &mut RtLocal,
    list_value_ref: *const u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list = unsafe { ListRef::new(list_value_ref as *const List, element_ty) };
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if index >= list.size() {
        opt.write_none();
        return RtStatus::Ok;
    }

    // The list keeps its element, so the data gets a clone of it.
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::boxing::data_clone_from_local(
            rt_handle,
            list.element_ptr(index),
            element_ty.as_ptr(),
            opt.payload_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }

    opt.write_some_tag();
    RtStatus::Ok
}

/// Read an element, packing it into a `data` only if it is not one already.
///
/// The two readings of a `data`-shaped slot coincide for a list of `data`: a
/// generic asking for an element it cannot name wants one packed, and a list
/// whose elements really are `data` wants one copied. Comparing the element
/// type as well as the slot's is what tells them apart, and doing it here
/// rather than at each call site is what keeps the four backends from each
/// having their own answer.
pub unsafe fn list_get_erased_impl(
    rt: &mut RtLocal,
    list_value_ref: *const u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let slot_ty = option_tydesc.option_inner_ty();
    let list = unsafe { ListRef::new(list_value_ref as *const List, element_ty) };
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if index >= list.size() {
        opt.write_none();
        return RtStatus::Ok;
    }

    // The list keeps its element, so the slot gets a clone of it, in whatever
    // shape the slot is. `clone_erased_local` is the one place that decides
    // which of the three that is: the slot's own shape, a `data` the element
    // is packed into, or the element's shape with `data` at some position
    // inside it.
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::boxing::clone_erased_local(
            rt_handle,
            list.element_ptr(index),
            element_ty.as_ptr(),
            opt.payload_ptr(),
            slot_ty.as_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }
    opt.write_some_tag();
    RtStatus::Ok
}

/// Take the last element off, in whatever shape the slot is.
///
/// The move-out counterpart of `list_get_erased_impl`: the list gives the
/// element up, so `erase_local` moves it across rather than cloning.
pub unsafe fn list_pop_erased_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let slot_ty = option_tydesc.option_inner_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if list.size() == 0 {
        opt.write_none();
        return RtStatus::Ok;
    }

    let last_index = list.size() - 1;
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::boxing::erase_local(
            rt_handle,
            list.element_ptr(last_index),
            element_ty.as_ptr(),
            opt.payload_ptr(),
            slot_ty.as_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }
    list.set_size(last_index);
    opt.write_some_tag();
    RtStatus::Ok
}

/// Take the last element off the list, as a `data`.
///
/// The element is moved rather than cloned: the list gives it up.
pub unsafe fn list_pop_as_data_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if list.size() == 0 {
        opt.write_none();
        return RtStatus::Ok;
    }

    let last_index = list.size() - 1;
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::boxing::data_from_local(
            rt_handle,
            list.element_ptr(last_index),
            element_ty.as_ptr(),
            opt.payload_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }

    list.set_size(last_index);
    opt.write_some_tag();
    RtStatus::Ok
}

/// Append the value a `data` holds to the list.
///
/// The data is consumed: what it held becomes the list's. It has to hold a
/// value of the list's element type, which is the generic call site's
/// responsibility, exactly as it is for reifying a return value.
pub unsafe fn list_push_data_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    data_in: *const u8,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list_ptr = list_value_mut as *mut List;
    let mut list = unsafe { ListMut::new(list_ptr, element_ty) };

    if list.needs_grow() {
        let new_capacity = calculate_new_capacity(list.capacity(), list.size() + 1);
        let status = unsafe { grow_buffer(rt, list_ptr, element_ty, new_capacity) };
        if status != RtStatus::Ok {
            return status;
        }
        list = unsafe { ListMut::new(list_ptr, element_ty) };
    }

    // Move the value out of the data and straight into the new slot.
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let status = unsafe {
        crate::impls::boxing::data_into_local(
            rt_handle,
            data_in,
            list.end_ptr(),
            element_ty.as_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }

    list.set_size(list.size() + 1);
    RtStatus::Ok
}

/// Replace the element at `index` with the value a `data` holds.
///
/// The data is consumed and what it held becomes the list's. The old element
/// is destroyed. Out of bounds is an error, as it is for the typed form.
pub unsafe fn list_set_data_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    data_in: *const u8,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };

    if index >= list.size() {
        return RtStatus::Error;
    }

    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let element_ptr = list.element_ptr_mut(index);
    let status = unsafe {
        crate::impls::destroy::any_destroy_local(rt_handle, element_ptr, element_ty.as_ptr())
    };
    if status != RtStatus::Ok {
        return status;
    }

    unsafe {
        crate::impls::boxing::data_into_local(rt_handle, data_in, element_ptr, element_ty.as_ptr())
    }
}

/// Insert the value a `data` holds at `index`, shifting the rest right.
///
/// `index` may be the length, which appends. The data is consumed.
pub unsafe fn list_insert_data_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    data_in: *const u8,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list_ptr = list_value_mut as *mut List;
    let mut list = unsafe { ListMut::new(list_ptr, element_ty) };
    let size = list.size();

    if index > size {
        return RtStatus::Error;
    }

    if list.needs_grow() {
        let new_capacity = calculate_new_capacity(list.capacity(), size + 1);
        let status = unsafe { grow_buffer(rt, list_ptr, element_ty, new_capacity) };
        if status != RtStatus::Ok {
            return status;
        }
        list = unsafe { ListMut::new(list_ptr, element_ty) };
    }

    // Open a hole at `index` by moving what follows one element along.
    let element_size = element_ty.size() as usize;
    if index < size {
        let src = list.element_ptr_mut(index);
        let count = (size - index) as usize * element_size;
        unsafe { std::ptr::copy(src, src.add(element_size), count) };
    }

    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    // Inserting at `size` appends, and there is no element there to point at.
    let hole = if index < size {
        list.element_ptr_mut(index)
    } else {
        list.end_ptr()
    };
    let status = unsafe {
        crate::impls::boxing::data_into_local(rt_handle, data_in, hole, element_ty.as_ptr())
    };
    if status != RtStatus::Ok {
        return status;
    }

    list.set_size(size + 1);
    RtStatus::Ok
}

/// Take the element at `index` out of the list, as a `data`.
///
/// The element is moved rather than cloned: the list gives it up, and what
/// follows shifts down. Out of bounds gives none.
/// Take the element at an index out, in whatever shape the slot is.
///
/// The move-out counterpart of `list_get_erased_impl`, for a position rather
/// than the end: the list gives the element up, so `erase_local` moves it
/// across rather than cloning, and decides which of the three shapes the slot
/// is in.
pub unsafe fn list_remove_erased_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let slot_ty = option_tydesc.option_inner_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let size = list.size();
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if index >= size {
        opt.write_none();
        return RtStatus::Ok;
    }

    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let element_size = element_ty.size() as usize;
    let element_ptr = list.element_ptr_mut(index);
    let status = unsafe {
        crate::impls::boxing::erase_local(
            rt_handle, element_ptr, element_ty.as_ptr(), opt.payload_ptr(), slot_ty.as_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }

    if index < size - 1 {
        let src = unsafe { element_ptr.add(element_size) };
        let count = (size - index - 1) as usize * element_size;
        unsafe { std::ptr::copy(src, element_ptr, count) };
    }

    list.set_size(size - 1);
    opt.write_some_tag();
    RtStatus::Ok
}

pub unsafe fn list_remove_as_data_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let size = list.size();
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    if index >= size {
        opt.write_none();
        return RtStatus::Ok;
    }

    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let element_size = element_ty.size() as usize;
    let element_ptr = list.element_ptr_mut(index);
    let status = unsafe {
        crate::impls::boxing::data_from_local(
            rt_handle, element_ptr, element_ty.as_ptr(), opt.payload_ptr(),
        )
    };
    if status != RtStatus::Ok {
        return status;
    }

    if index < size - 1 {
        let src = unsafe { element_ptr.add(element_size) };
        let count = (size - index - 1) as usize * element_size;
        unsafe { std::ptr::copy(src, element_ptr, count) };
    }

    list.set_size(size - 1);
    opt.write_some_tag();
    RtStatus::Ok
}

// ============================================================================
// Insert and Remove Operations
// ============================================================================

/// Insert an element at index, shifting subsequent elements right.
///
/// Returns Ok if successful, Error if index > len.
/// index == len is equivalent to push.
pub unsafe fn list_insert_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    _list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    element_in: *mut u8,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let list_ptr = list_value_mut as *mut List;
    let mut list = unsafe { ListMut::new(list_ptr, element_tydesc) };
    let size = list.size();

    // Check bounds (can insert at size for push-like behavior).
    if index > size {
        return RtStatus::Error;
    }

    // Grow if needed.
    if list.needs_grow() {
        let new_capacity = calculate_new_capacity(list.capacity(), size + 1);
        let status = unsafe { grow_buffer(rt, list_ptr, element_tydesc, new_capacity) };
        if status != RtStatus::Ok {
            return status;
        }
    }

    let element_size = element_tydesc.size() as usize;

    // Shift elements right.
    if index < size {
        let src = list.element_ptr_mut(index);
        let dest = unsafe { src.add(element_size) };
        let count = (size - index) as usize * element_size;
        unsafe { std::ptr::copy(src, dest, count) };
    }

    // Copy new element.
    let dest_ptr = if index < size {
        list.element_ptr_mut(index)
    } else {
        list.end_ptr()
    };
    unsafe { std::ptr::copy_nonoverlapping(element_in, dest_ptr, element_size) };

    list.set_size(size + 1);

    RtStatus::Ok
}

/// Remove an element at index, shifting subsequent elements left.
///
/// Returns Option<T>:
/// - If index is valid, sets option to Some and moves the element.
/// - If index is out of bounds, sets option to None.
pub unsafe fn list_remove_impl(
    _rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: rtdt::IndexRepr,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let size = list.size();
    let option_layout = rtdt::layout::compute_option_layout(option_tydesc);
    let mut opt = unsafe { OptionWriter::new(option_value_out, &option_layout) };

    // Check bounds.
    if index >= size {
        opt.write_none();
        return RtStatus::Ok;
    }

    let element_size = element_ty.size() as usize;
    let element_ptr = list.element_ptr_mut(index);

    // Move element to option payload.
    unsafe { std::ptr::copy_nonoverlapping(element_ptr, opt.payload_ptr(), element_size) };

    // Shift elements left.
    if index < size - 1 {
        let src = unsafe { element_ptr.add(element_size) };
        let count = (size - index - 1) as usize * element_size;
        unsafe { std::ptr::copy(src, element_ptr, count) };
    }

    list.set_size(size - 1);
    opt.write_some_tag();

    RtStatus::Ok
}

// ============================================================================
// Capacity Management
// ============================================================================

/// Reserve capacity for at least `additional` more elements.
///
/// Does nothing if capacity is already sufficient.
pub unsafe fn list_reserve_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    additional: rtdt::IndexRepr,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let list_ptr = list_value_mut as *mut List;
    let list = unsafe { ListMut::new(list_ptr, element_ty) };

    let required = list.size().saturating_add(additional);
    if required <= list.capacity() {
        return RtStatus::Ok;
    }

    let new_capacity = calculate_new_capacity(list.capacity(), required);
    unsafe { grow_buffer(rt, list_ptr, element_ty, new_capacity) }
}

/// Shrink capacity to fit current size.
pub unsafe fn list_shrink_to_fit_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    let element_ty = list_tydesc.list_element_ty();
    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_ty) };
    let size = list.size();
    let capacity = list.capacity();

    if size >= capacity {
        return RtStatus::Ok;
    }

    let old_data = list.data_mut();
    let element_size = element_ty.size();
    let element_align = element_ty.align();

    // If size is 0, just free the buffer.
    if size == 0 {
        if !old_data.is_null() && capacity > 0 {
            unsafe { rt.alloc.free(element_size, element_align, capacity, old_data) };
            list.set_data(std::ptr::null());
            list.set_capacity(0);
        }
        return RtStatus::Ok;
    }

    // Allocate new buffer with exact size.
    let new_data = unsafe { rt.alloc.alloc(element_size, element_align, size) };
    if new_data.is_null() {
        return RtStatus::Error;
    }

    // Copy elements to new buffer.
    let bytes_to_copy = (size as usize) * (element_size as usize);
    unsafe { std::ptr::copy_nonoverlapping(old_data, new_data, bytes_to_copy) };

    // Free old buffer.
    unsafe { rt.alloc.free(element_size, element_align, capacity, old_data) };

    // Update list.
    list.set_data(new_data);
    list.set_capacity(size);

    RtStatus::Ok
}

// ============================================================================
// Bulk Operations
// ============================================================================

/// Append all elements from a slice to the list (clones elements).
pub unsafe fn list_extend_from_slice_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    slice_ptr_ref: *const u8,
    slice_len: rtdt::IndexRepr,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    if slice_len == 0 {
        return RtStatus::Ok;
    }

    let mut list = unsafe { ListMut::new(list_value_mut as *mut List, element_tydesc) };
    let size = list.size();

    // Reserve space for all new elements.
    let status = unsafe { list_reserve_impl(rt, list_value_mut, list_tydesc, slice_len) };
    if status != RtStatus::Ok {
        return status;
    }

    let element_size = element_tydesc.size();

    // Clone each element from the slice.
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    for i in 0..slice_len {
        let src_ptr = unsafe { slice_ptr_ref.add((i as usize) * (element_size as usize)) };
        let dest_ptr = unsafe { list.data_mut().add(((size + i) as usize) * (element_size as usize)) };

        let status = unsafe {
            crate::impls::clone::clone_value(rt_handle, src_ptr, element_tydesc.as_ptr(), dest_ptr)
        };
        if status != RtStatus::Ok {
            // Update size to reflect what was successfully added.
            list.set_size(size + i);
            return status;
        }
    }

    list.set_size(size + slice_len);

    RtStatus::Ok
}

// ============================================================================
// Helper Functions (private)
// ============================================================================

/// Calculate the new capacity when growing.
///
/// Follows Vec's growth strategy: double capacity or use required, whichever is larger.
#[inline]
fn calculate_new_capacity(current: rtdt::IndexRepr, required: rtdt::IndexRepr) -> rtdt::IndexRepr {
    let doubled = current.saturating_mul(2);
    doubled.max(required).max(4) // Minimum capacity of 4.
}

/// Grow the list buffer to accommodate at least `new_capacity` elements.
unsafe fn grow_buffer(
    rt: &mut RtLocal,
    list_ptr: *mut List,
    element_tydesc: rtdt::TyDescRef,
    new_capacity: rtdt::IndexRepr,
) -> RtStatus {
    let mut list = unsafe { ListMut::new(list_ptr, element_tydesc) };
    let old_capacity = list.capacity();
    let size = list.size();
    let old_data = list.data_mut();

    if new_capacity <= old_capacity {
        return RtStatus::Ok;
    }

    let element_size = element_tydesc.size();
    let element_align = element_tydesc.align();

    // Allocate new buffer.
    let new_data = unsafe { rt.alloc.alloc(element_size, element_align, new_capacity) };
    if new_data.is_null() {
        return RtStatus::Error;
    }

    // Copy existing elements to new buffer.
    if !old_data.is_null() && size > 0 {
        let bytes_to_copy = (size as usize) * (element_size as usize);
        unsafe { std::ptr::copy_nonoverlapping(old_data, new_data, bytes_to_copy) };
    }

    // Free old buffer.
    if !old_data.is_null() && old_capacity > 0 {
        unsafe { rt.alloc.free(element_size, element_align, old_capacity, old_data) };
    }

    // Update list.
    list.set_data(new_data);
    list.set_capacity(new_capacity);

    RtStatus::Ok
}

/// Destroy elements in a range [start, end).
unsafe fn destroy_elements(
    rt: &mut RtLocal,
    data_ptr: *mut u8,
    element_tydesc: rtdt::TyDescRef,
    start: rtdt::IndexRepr,
    end: rtdt::IndexRepr,
) -> RtStatus {
    let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
    let iter = unsafe { DynElementIter::new(data_ptr, element_tydesc.size(), start, end) };

    for element_ptr in iter {
        let status = unsafe {
            crate::impls::destroy::any_destroy_local(rt_handle, element_ptr, element_tydesc.as_ptr())
        };
        if status != RtStatus::Ok {
            return status;
        }
    }

    RtStatus::Ok
}

// ============================================================================
// Safe Wrapper Types (internal)
// ============================================================================

/// Safe read-only accessor for a List.
///
/// Wraps a raw `*const List` pointer with safe field access methods.
/// Bounds checking uses `debug_assert!` (zero cost in release).
struct ListRef {
    ptr: *const List,
    element_size: u32,
}

impl ListRef {
    /// Create a new ListRef from a raw pointer and element type descriptor.
    ///
    /// # Safety
    /// - `ptr` must be a valid, aligned, non-null pointer to a `List`.
    #[inline]
    unsafe fn new(ptr: *const List, element_tydesc: rtdt::TyDescRef) -> Self {
        debug_assert!(!ptr.is_null());
        Self {
            ptr,
            element_size: element_tydesc.size(),
        }
    }

    /// Get the data pointer.
    #[inline]
    fn data(&self) -> *const u8 {
        unsafe { (*self.ptr).data }
    }

    /// Get the current size (element count).
    #[inline]
    fn size(&self) -> rtdt::IndexRepr {
        unsafe { (*self.ptr).size.0 }
    }

    /// Get a pointer to the element at `index`.
    ///
    /// Debug-only bounds check: panics if `index >= size`.
    #[inline]
    fn element_ptr(&self, index: rtdt::IndexRepr) -> *const u8 {
        debug_assert!(index < self.size(), "element_ptr: index {} >= size {}", index, self.size());
        unsafe {
            self.data().add((index as usize) * (self.element_size as usize))
        }
    }
}

/// Safe mutable accessor for a List.
///
/// Wraps a raw `*mut List` pointer with safe field access methods.
/// Bounds checking uses `debug_assert!` (zero cost in release).
struct ListMut {
    ptr: *mut List,
    element_size: u32,
}

impl ListMut {
    /// Create a new ListMut from a raw pointer and element type descriptor.
    ///
    /// # Safety
    /// - `ptr` must be a valid, aligned, non-null pointer to a `List`.
    /// - The `List` must remain valid for the lifetime of this `ListMut`.
    #[inline]
    unsafe fn new(ptr: *mut List, element_tydesc: rtdt::TyDescRef) -> Self {
        debug_assert!(!ptr.is_null());
        Self {
            ptr,
            element_size: element_tydesc.size(),
        }
    }

    /// Get the data pointer.
    #[inline]
    fn data(&self) -> *const u8 {
        unsafe { (*self.ptr).data }
    }

    /// Get the data pointer as mutable.
    #[inline]
    fn data_mut(&self) -> *mut u8 {
        unsafe { (*self.ptr).data as *mut u8 }
    }

    /// Get the current size (element count).
    #[inline]
    fn size(&self) -> rtdt::IndexRepr {
        unsafe { (*self.ptr).size.0 }
    }

    /// Get the current capacity (element count).
    #[inline]
    fn capacity(&self) -> rtdt::IndexRepr {
        unsafe { (*self.ptr).capacity.0 }
    }

    /// Set the data pointer.
    #[inline]
    fn set_data(&mut self, data: *const u8) {
        unsafe { (*self.ptr).data = data; }
    }

    /// Set the size.
    #[inline]
    fn set_size(&mut self, size: rtdt::IndexRepr) {
        unsafe { (*self.ptr).size = rtdt::Index(size); }
    }

    /// Set the capacity.
    #[inline]
    fn set_capacity(&mut self, capacity: rtdt::IndexRepr) {
        unsafe { (*self.ptr).capacity = rtdt::Index(capacity); }
    }

    /// Get a pointer to the element at `index`.
    ///
    /// Debug-only bounds check: panics if `index >= size`.
    #[inline]
    fn element_ptr(&self, index: rtdt::IndexRepr) -> *const u8 {
        debug_assert!(index < self.size(), "element_ptr: index {} >= size {}", index, self.size());
        unsafe {
            self.data().add((index as usize) * (self.element_size as usize))
        }
    }

    /// Get a mutable pointer to the element at `index`.
    ///
    /// Debug-only bounds check: panics if `index >= size`.
    #[inline]
    fn element_ptr_mut(&self, index: rtdt::IndexRepr) -> *mut u8 {
        debug_assert!(index < self.size(), "element_ptr_mut: index {} >= size {}", index, self.size());
        unsafe {
            self.data_mut().add((index as usize) * (self.element_size as usize))
        }
    }

    /// Get a pointer to where the next element would go (at index == size).
    ///
    /// Used for push operations. Debug-only check: panics if `size > capacity`.
    #[inline]
    fn end_ptr(&self) -> *mut u8 {
        let size = self.size();
        debug_assert!(size <= self.capacity(), "end_ptr: size {} > capacity {}", size, self.capacity());
        unsafe {
            self.data_mut().add((size as usize) * (self.element_size as usize))
        }
    }

    /// Check if the list needs to grow to accommodate one more element.
    #[inline]
    fn needs_grow(&self) -> bool {
        self.size() >= self.capacity()
    }

    /// Reset the list to empty state (null data, zero size/capacity).
    #[inline]
    fn reset(&mut self) {
        self.set_data(std::ptr::null());
        self.set_size(0);
        self.set_capacity(0);
    }
}

/// Safe writer for Option values.
///
/// Encapsulates the tag/payload layout and provides safe methods
/// for writing None or Some variants.
struct OptionWriter {
    tag_ptr: *mut u8,
    payload_ptr: *mut u8,
}

impl OptionWriter {
    /// Create an OptionWriter from a base pointer and layout.
    ///
    /// # Safety
    /// - `base` must be a valid, aligned, non-null pointer to Option storage.
    /// - The storage must remain valid for the lifetime of this writer.
    #[inline]
    unsafe fn new(base: *mut u8, layout: &rtdt::OptionLayout) -> Self {
        debug_assert!(!base.is_null());
        Self {
            tag_ptr: base,
            payload_ptr: unsafe { base.add(layout.payload_offset as usize) },
        }
    }

    /// Write a None variant (tag only, no payload).
    #[inline]
    fn write_none(&mut self) {
        unsafe {
            *self.tag_ptr = rtdt::OptionTag::None as u8;
        }
    }

    /// Get the payload pointer for writing Some's value.
    ///
    /// After writing the payload, call `write_some_tag()`.
    #[inline]
    fn payload_ptr(&self) -> *mut u8 {
        self.payload_ptr
    }

    /// Write the Some tag after payload has been written.
    #[inline]
    fn write_some_tag(&mut self) {
        unsafe {
            *self.tag_ptr = rtdt::OptionTag::Some as u8;
        }
    }
}

/// Iterator over element pointers in a contiguous buffer.
///
/// Yields mutable pointers to each element in order.
struct DynElementIter {
    current: *mut u8,
    end: *mut u8,
    element_size: usize,
}

impl DynElementIter {
    /// Create an iterator over elements in a range [start_index, end_index).
    ///
    /// # Safety
    /// - `data` must be a valid pointer to element storage.
    /// - The range must be within bounds of the allocated buffer.
    #[inline]
    unsafe fn new(data: *mut u8, element_size: u32, start_index: rtdt::IndexRepr, end_index: rtdt::IndexRepr) -> Self {
        debug_assert!(start_index <= end_index);
        let element_size = element_size as usize;
        Self {
            current: unsafe { data.add((start_index as usize) * element_size) },
            end: unsafe { data.add((end_index as usize) * element_size) },
            element_size,
        }
    }
}

impl Iterator for DynElementIter {
    type Item = *mut u8;

    #[inline]
    fn next(&mut self) -> std::option::Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }
        let ptr = self.current;
        self.current = unsafe { self.current.add(self.element_size) };
        Some(ptr)
    }
}
