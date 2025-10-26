//! Runtime implementation of List (dynamic array).
//!
//! Uses a simple growable array structure with element-based capacity.

use rmx::prelude::*;
use crate::rt_local::RtLocal;
use crate::rtdt::{self, *};
use crate::c::RtStatus;

// ============================================================================
// Core List Operations
// ============================================================================

/// Create an empty List.
pub unsafe fn list_create_impl(
    rt: &mut RtLocal,
    value_out: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if value_out.is_null() {
            return RtStatus::Error;
        }

        // Initialize an empty list (null data, zero size and capacity).
        let list_ptr = value_out as *mut List;
        (*list_ptr).data = std::ptr::null();
        (*list_ptr).size = 0;
        (*list_ptr).capacity = 0;

        RtStatus::Ok
    }
}

/// Create a List from a slice of elements (clones elements).
pub unsafe fn list_create_from_slice_impl(
    rt: &mut RtLocal,
    slice_ptr_ref: *const u8,
    slice_len: u32,
    element_tydesc: rtdt::TyDescRef,
    list_value_out: *mut u8,
    list_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_out.is_null()
            || slice_ptr_ref.is_null() {
            return RtStatus::Error;
        }

        let list_ptr = list_value_out as *mut List;

        // Create empty list.
        let status = list_create_impl(rt, list_value_out, list_tydesc);
        if status != RtStatus::Ok {
            return status;
        }

        if slice_len == 0 {
            return RtStatus::Ok;
        }

        let element_size = element_tydesc.size();
        let element_align = element_tydesc.align();

        // Allocate buffer with exact capacity.
        let data = rt.alloc.alloc(element_size, element_align, slice_len);
        if data.is_null() {
            return RtStatus::Error;
        }

        (*list_ptr).data = data;
        (*list_ptr).capacity = slice_len;

        // Clone each element from the slice.
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        for i in 0..slice_len {
            let src_ptr = slice_ptr_ref.add((i * element_size) as usize);
            let dest_ptr = data.add((i * element_size) as usize);

            let status = crate::clone::clone_value(rt_handle, src_ptr, element_tydesc.as_ptr(), dest_ptr);
            if status != RtStatus::Ok {
                // Clean up partially created list.
                (*list_ptr).size = i;
                let _ = list_destroy_impl(rt, list_value_out, list_tydesc);
                return status;
            }
        }

        (*list_ptr).size = slice_len;

        RtStatus::Ok
    }
}

/// Destroy a List and free all elements and buffer.
pub unsafe fn list_destroy_impl(
    rt: &mut RtLocal,
    value_in: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if value_in.is_null() {
            return RtStatus::Error;
        }

        let element_ty = tydesc.list_element_ty();
        let element_tydesc = element_ty.as_ptr();

        let list_ptr = value_in as *mut List;
        let data_ptr = (*list_ptr).data as *mut u8;
        let size = (*list_ptr).size;
        let capacity = (*list_ptr).capacity;

        // Destroy all elements.
        if !data_ptr.is_null() && size > 0 {
            let status = destroy_elements(rt, data_ptr, element_ty, 0, size);
            if status != RtStatus::Ok {
                return status;
            }
        }

        // Free the buffer.
        if !data_ptr.is_null() && capacity > 0 {
            rt.alloc.free(element_ty.size(), element_ty.align(), capacity, data_ptr);
        }

        // Reset the list.
        (*list_ptr).data = std::ptr::null();
        (*list_ptr).size = 0;
        (*list_ptr).capacity = 0;

        RtStatus::Ok
    }
}

/// Clear a List (destroy all elements and reset to empty, keeping buffer).
pub unsafe fn list_clear_impl(
    rt: &mut RtLocal,
    value_mut: *mut u8,
    tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if value_mut.is_null() {
            return RtStatus::Error;
        }

        let element_ty = tydesc.list_element_ty();
        let element_tydesc = element_ty.as_ptr();

        let list_ptr = value_mut as *mut List;
        let data_ptr = (*list_ptr).data as *mut u8;
        let size = (*list_ptr).size;

        // Destroy all elements.
        if !data_ptr.is_null() && size > 0 {
            let status = destroy_elements(rt, data_ptr, element_ty, 0, size);
            if status != RtStatus::Ok {
                return status;
            }
        }

        // Reset size to 0, keeping capacity and buffer.
        (*list_ptr).size = 0;

        RtStatus::Ok
    }
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
    index: u32,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_ref.is_null()
            || option_value_out.is_null() {
            return RtStatus::Error;
        }

        let element_ty = list_tydesc.list_element_ty();
        let element_tydesc = element_ty.as_ptr();

        let list_ptr = list_value_ref as *const List;
        let size = (*list_ptr).size;

        // Compute option layout.
        let option_layout = rtdt::layout::compute_option_layout(option_tydesc.as_ptr());
        let option_tag_ptr = option_value_out;
        let option_payload_ptr = option_value_out.add(option_layout.payload_offset as usize);

        // Check bounds.
        if index >= size {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }

        // Clone element to option payload.
        let data_ptr = (*list_ptr).data;
        let element_size = element_ty.size() as usize;
        let element_ptr = (data_ptr as *const u8).add(index as usize * element_size);

        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::clone::clone_value(
            rt_handle,
            element_ptr,
            element_tydesc,
            option_payload_ptr,
        );

        if status != RtStatus::Ok {
            return status;
        }

        *option_tag_ptr = rtdt::OptionTag::Some as u8;
        RtStatus::Ok
    }
}

/// Set an element at index (replace existing element).
///
/// Returns Ok if successful, Error if index is out of bounds.
pub unsafe fn list_set_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: u32,
    element_in: *mut u8,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null()
            || element_in.is_null() {
            return RtStatus::Error;
        }

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;

        // Check bounds.
        if index >= size {
            return RtStatus::Error;
        }

        // Destroy old element.
        let data_ptr = (*list_ptr).data as *mut u8;
        let element_size = element_tydesc.size() as usize;
        let element_ptr = data_ptr.add(index as usize * element_size);

        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        let status = crate::destroy::any_destroy_local(rt_handle, element_ptr, element_tydesc.as_ptr());
        if status != RtStatus::Ok {
            return status;
        }

        // Copy new element.
        std::ptr::copy_nonoverlapping(element_in, element_ptr, element_size);

        RtStatus::Ok
    }
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
    list_tydesc: rtdt::TyDescRef,
    element_in: *mut u8,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null()
            || element_in.is_null() {
            return RtStatus::Error;
        }

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;
        let capacity = (*list_ptr).capacity;

        // Grow if needed.
        if size >= capacity {
            let new_capacity = calculate_new_capacity(capacity, size + 1);
            let status = grow_buffer(rt, list_ptr, element_tydesc, new_capacity);
            if status != RtStatus::Ok {
                return status;
            }
        }

        // Copy element to end.
        let data_ptr = (*list_ptr).data as *mut u8;
        let element_size = element_tydesc.size() as usize;
        let dest_ptr = data_ptr.add(size as usize * element_size);
        std::ptr::copy_nonoverlapping(element_in, dest_ptr, element_size);

        (*list_ptr).size = size + 1;

        RtStatus::Ok
    }
}

/// Pop an element from the end of the list.
///
/// Returns Option<T>:
/// - If list is non-empty, sets option to Some and moves the element.
/// - If list is empty, sets option to None.
pub unsafe fn list_pop_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null()
            || option_value_out.is_null() {
            return RtStatus::Error;
        }

        let element_ty = list_tydesc.list_element_ty();
        let element_tydesc = element_ty.as_ptr();

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;

        // Compute option layout.
        let option_layout = rtdt::layout::compute_option_layout(option_tydesc.as_ptr());
        let option_tag_ptr = option_value_out;
        let option_payload_ptr = option_value_out.add(option_layout.payload_offset as usize);

        // Check if empty.
        if size == 0 {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }

        // Move last element to option payload.
        let data_ptr = (*list_ptr).data as *mut u8;
        let element_size = element_ty.size() as usize;
        let last_element_ptr = data_ptr.add((size - 1) as usize * element_size);
        std::ptr::copy_nonoverlapping(last_element_ptr, option_payload_ptr, element_size);

        (*list_ptr).size = size - 1;
        *option_tag_ptr = rtdt::OptionTag::Some as u8;

        RtStatus::Ok
    }
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
    list_tydesc: rtdt::TyDescRef,
    index: u32,
    element_in: *mut u8,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null()
            || element_in.is_null() {
            return RtStatus::Error;
        }

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;
        let capacity = (*list_ptr).capacity;

        // Check bounds (can insert at size for push-like behavior).
        if index > size {
            return RtStatus::Error;
        }

        // Grow if needed.
        if size >= capacity {
            let new_capacity = calculate_new_capacity(capacity, size + 1);
            let status = grow_buffer(rt, list_ptr, element_tydesc, new_capacity);
            if status != RtStatus::Ok {
                return status;
            }
        }

        let data_ptr = (*list_ptr).data as *mut u8;
        let element_size = element_tydesc.size() as usize;

        // Shift elements right.
        if index < size {
            let src = data_ptr.add(index as usize * element_size);
            let dest = data_ptr.add((index + 1) as usize * element_size);
            let count = (size - index) as usize * element_size;
            std::ptr::copy(src, dest, count);
        }

        // Copy new element.
        let dest_ptr = data_ptr.add(index as usize * element_size);
        std::ptr::copy_nonoverlapping(element_in, dest_ptr, element_size);

        (*list_ptr).size = size + 1;

        RtStatus::Ok
    }
}

/// Remove an element at index, shifting subsequent elements left.
///
/// Returns Option<T>:
/// - If index is valid, sets option to Some and moves the element.
/// - If index is out of bounds, sets option to None.
pub unsafe fn list_remove_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
    index: u32,
    option_value_out: *mut u8,
    option_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null()
            || option_value_out.is_null() {
            return RtStatus::Error;
        }

        let element_ty = list_tydesc.list_element_ty();
        let element_tydesc = element_ty.as_ptr();

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;

        // Compute option layout.
        let option_layout = rtdt::layout::compute_option_layout(option_tydesc.as_ptr());
        let option_tag_ptr = option_value_out;
        let option_payload_ptr = option_value_out.add(option_layout.payload_offset as usize);

        // Check bounds.
        if index >= size {
            *option_tag_ptr = rtdt::OptionTag::None as u8;
            return RtStatus::Ok;
        }

        let data_ptr = (*list_ptr).data as *mut u8;
        let element_size = element_ty.size() as usize;
        let element_ptr = data_ptr.add(index as usize * element_size);

        // Move element to option payload.
        std::ptr::copy_nonoverlapping(element_ptr, option_payload_ptr, element_size);

        // Shift elements left.
        if index < size - 1 {
            let src = data_ptr.add((index + 1) as usize * element_size);
            let dest = element_ptr;
            let count = (size - index - 1) as usize * element_size;
            std::ptr::copy(src, dest, count);
        }

        (*list_ptr).size = size - 1;
        *option_tag_ptr = rtdt::OptionTag::Some as u8;

        RtStatus::Ok
    }
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
    additional: u32,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null() {
            return RtStatus::Error;
        }

        let element_ty = list_tydesc.list_element_ty();

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;
        let capacity = (*list_ptr).capacity;

        let required = size.saturating_add(additional);
        if required <= capacity {
            return RtStatus::Ok;
        }

        let new_capacity = calculate_new_capacity(capacity, required);
        grow_buffer(rt, list_ptr, element_ty, new_capacity)
    }
}

/// Shrink capacity to fit current size.
pub unsafe fn list_shrink_to_fit_impl(
    rt: &mut RtLocal,
    list_value_mut: *mut u8,
    list_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null() {
            return RtStatus::Error;
        }

        let element_ty = list_tydesc.list_element_ty();

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;
        let capacity = (*list_ptr).capacity;

        if size >= capacity {
            return RtStatus::Ok;
        }

        let old_data = (*list_ptr).data as *mut u8;
        let element_size = element_ty.size();
        let element_align = element_ty.align();

        // If size is 0, just free the buffer.
        if size == 0 {
            if !old_data.is_null() && capacity > 0 {
                rt.alloc.free(element_size, element_align, capacity, old_data);
                (*list_ptr).data = std::ptr::null();
                (*list_ptr).capacity = 0;
            }
            return RtStatus::Ok;
        }

        // Allocate new buffer with exact size.
        let new_data = rt.alloc.alloc(element_size, element_align, size);
        if new_data.is_null() {
            return RtStatus::Error;
        }

        // Copy elements to new buffer.
        let bytes_to_copy = (size * element_size) as usize;
        std::ptr::copy_nonoverlapping(old_data, new_data, bytes_to_copy);

        // Free old buffer.
        rt.alloc.free(element_size, element_align, capacity, old_data);

        // Update list.
        (*list_ptr).data = new_data;
        (*list_ptr).capacity = size;

        RtStatus::Ok
    }
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
    slice_len: u32,
    element_tydesc: rtdt::TyDescRef,
) -> RtStatus {
    unsafe {
        if list_value_mut.is_null()
            || slice_ptr_ref.is_null() {
            return RtStatus::Error;
        }

        if slice_len == 0 {
            return RtStatus::Ok;
        }

        let list_ptr = list_value_mut as *mut List;
        let size = (*list_ptr).size;

        // Reserve space for all new elements.
        let status = list_reserve_impl(rt, list_value_mut, list_tydesc, slice_len);
        if status != RtStatus::Ok {
            return status;
        }

        let data_ptr = (*list_ptr).data as *mut u8;
        let element_size = element_tydesc.size();

        // Clone each element from the slice.
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;
        for i in 0..slice_len {
            let src_ptr = slice_ptr_ref.add((i * element_size) as usize);
            let dest_ptr = data_ptr.add(((size + i) * element_size) as usize);

            let status = crate::clone::clone_value(rt_handle, src_ptr, element_tydesc.as_ptr(), dest_ptr);
            if status != RtStatus::Ok {
                // Update size to reflect what was successfully added.
                (*list_ptr).size = size + i;
                return status;
            }
        }

        (*list_ptr).size = size + slice_len;

        RtStatus::Ok
    }
}

// ============================================================================
// Helper Functions (private)
// ============================================================================

/// Get pointer to the data buffer of a list.
#[inline]
unsafe fn list_data_ptr(list_ptr: *const List) -> *const u8 {
    unsafe { (*list_ptr).data }
}

/// Get mutable pointer to the data buffer of a list.
#[inline]
unsafe fn list_data_ptr_mut(list_ptr: *mut List) -> *mut u8 {
    unsafe { (*list_ptr).data as *mut u8 }
}

/// Get the current size of a list.
#[inline]
unsafe fn list_size(list_ptr: *const List) -> u32 {
    unsafe { (*list_ptr).size }
}

/// Get the current capacity of a list.
#[inline]
unsafe fn list_capacity(list_ptr: *const List) -> u32 {
    unsafe { (*list_ptr).capacity }
}

/// Calculate the new capacity when growing.
///
/// Follows Vec's growth strategy: double capacity or use required, whichever is larger.
#[inline]
fn calculate_new_capacity(current: u32, required: u32) -> u32 {
    let doubled = current.saturating_mul(2);
    doubled.max(required).max(4) // Minimum capacity of 4.
}

/// Grow the list buffer to accommodate at least `new_capacity` elements.
unsafe fn grow_buffer(
    rt: &mut RtLocal,
    list_ptr: *mut List,
    element_tydesc: rtdt::TyDescRef,
    new_capacity: u32,
) -> RtStatus {
    unsafe {
        let old_capacity = (*list_ptr).capacity;
        let size = (*list_ptr).size;
        let old_data = (*list_ptr).data as *mut u8;

        if new_capacity <= old_capacity {
            return RtStatus::Ok;
        }

        let element_size = element_tydesc.size();
        let element_align = element_tydesc.align();

        // Allocate new buffer.
        let new_data = rt.alloc.alloc(element_size, element_align, new_capacity);
        if new_data.is_null() {
            return RtStatus::Error;
        }

        // Copy existing elements to new buffer.
        if !old_data.is_null() && size > 0 {
            let bytes_to_copy = (size * element_size) as usize;
            std::ptr::copy_nonoverlapping(old_data, new_data, bytes_to_copy);
        }

        // Free old buffer.
        if !old_data.is_null() && old_capacity > 0 {
            rt.alloc.free(element_size, element_align, old_capacity, old_data);
        }

        // Update list.
        (*list_ptr).data = new_data;
        (*list_ptr).capacity = new_capacity;

        RtStatus::Ok
    }
}

/// Destroy elements in a range [start, end).
unsafe fn destroy_elements(
    rt: &mut RtLocal,
    data_ptr: *mut u8,
    element_tydesc: rtdt::TyDescRef,
    start: u32,
    end: u32,
) -> RtStatus {
    unsafe {
        let element_size = element_tydesc.size() as usize;
        let rt_handle = rt as *mut RtLocal as crate::c::LocalRtHandle;

        for i in start..end {
            let element_ptr = data_ptr.add((i as usize) * element_size);
            let status = crate::destroy::any_destroy_local(rt_handle, element_ptr, element_tydesc.as_ptr());
            if status != RtStatus::Ok {
                return status;
            }
        }

        RtStatus::Ok
    }
}
