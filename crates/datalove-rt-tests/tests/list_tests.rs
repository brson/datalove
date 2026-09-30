//! Tests for list runtime functions.

use rmx::prelude::*;
use datalove_rtdt as rtdt;
use std::cell::RefCell;
use std::ptr;

// ============================================================================
// Type Descriptor Arena
// ============================================================================

/// Arena for allocating type descriptors in tests.
struct TyDescArena {
    ptrs: RefCell<Vec<*mut rtdt::TyDesc>>,
}

impl TyDescArena {
    fn new() -> Self {
        Self {
            ptrs: RefCell::new(Vec::new()),
        }
    }

    fn alloc(&self, td: rtdt::TyDesc) -> *const rtdt::TyDesc {
        let ptr = Box::into_raw(Box::new(td));
        self.ptrs.borrow_mut().push(ptr);
        ptr
    }

    fn alloc_mut(&self, td: rtdt::TyDesc) -> *mut rtdt::TyDesc {
        let ptr = Box::into_raw(Box::new(td));
        self.ptrs.borrow_mut().push(ptr);
        ptr
    }
}

impl Drop for TyDescArena {
    fn drop(&mut self) {
        for &ptr in self.ptrs.borrow().iter() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }
}

// ============================================================================
// Test Helper Functions
// ============================================================================

/// Create a u32 type descriptor.
fn create_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing { unused: 0 },
        },
    })
}

/// Create an Option<u32> type descriptor.
fn create_option_u32_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let inner_tydesc = create_u32_tydesc(arena);

    // Create the option tydesc first with placeholder size/align.
    let option_tydesc = arena.alloc_mut(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Option,
        size: 0,
        align: 0,
        type_info: rtdt::TyInfo {
            option: rtdt::TyInfoOption {
                inner_tydesc,
            },
        },
    });

    // Now compute the layout.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc))
    };

    // Update size and align.
    unsafe {
        (*option_tydesc).size = option_layout.size;
        (*option_tydesc).align = option_layout.align;
    }

    (option_tydesc, inner_tydesc)
}

/// Create a List<u32> type descriptor.
fn create_list_u32_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let element_tydesc = create_u32_tydesc(arena);

    let list_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList {
                element_tydesc,
            },
        },
    });

    (list_tydesc, element_tydesc)
}

// ============================================================================
// Basic Tests
// ============================================================================

/// Test creating an empty list.
#[test]
fn test_list_create_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);

    // Allocate space for the list.
    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify the list is empty.
    assert!(list.data.is_null());
    assert_eq!(list.size, rtdt::Index::ZERO);
    assert_eq!(list.capacity, rtdt::Index::ZERO);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty list.
#[test]
fn test_list_destroy_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create then destroy the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is cleared.
    assert!(list.data.is_null());
    assert_eq!(list.size, rtdt::Index::ZERO);
    assert_eq!(list.capacity, rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty list.
#[test]
fn test_list_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clear the empty list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_clear_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is still empty.
    assert_eq!(list.size, rtdt::Index::ZERO);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Push/Pop Tests
// ============================================================================

/// Test pushing a single element.
#[test]
fn test_list_push_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 42.
    let mut value = 42u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            list_tydesc,
            &mut value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list size.
    assert_eq!(list.size, rtdt::Index(1));
    assert!(!list.data.is_null());

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test pushing multiple elements.
#[test]
fn test_list_push_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 5 elements.
    for i in 0u32..5 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify list size.
    assert_eq!(list.size, rtdt::Index(5));

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test popping from an empty list returns None.
#[test]
fn test_list_pop_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to pop from empty list.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            list_tydesc,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test push then pop.
#[test]
fn test_list_push_pop() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 42.
    let mut value = 42u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            list_tydesc,
            &mut value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(list.size, rtdt::Index(1));

    // Pop.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            list_tydesc,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got Some(42).
    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let retrieved_value = unsafe { *value_ptr };
    assert_eq!(retrieved_value, 42);

    // List should be empty now.
    assert_eq!(list.size, rtdt::Index::ZERO);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Get Tests
// ============================================================================

/// Test get from empty list returns None.
#[test]
fn test_list_get_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to get element at index 0.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_local(
            rt,
            list_ptr,
            list_tydesc,
            0,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test get valid index.
#[test]
fn test_list_get_valid() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 3 elements.
    for i in 0u32..3 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Get element at index 1.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_local(
            rt,
            list_ptr,
            list_tydesc,
            1,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got Some(10).
    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let retrieved_value = unsafe { *value_ptr };
    assert_eq!(retrieved_value, 10);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test get out of bounds returns None.
#[test]
fn test_list_get_out_of_bounds() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 2 elements.
    for i in 0u32..2 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Try to get element at index 5.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_local(
            rt,
            list_ptr,
            list_tydesc,
            5,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Capacity Tests
// ============================================================================

/// Test that pushing triggers capacity growth.
#[test]
fn test_list_capacity_growth() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 10 elements to trigger capacity growth.
    for i in 0u32..10 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Capacity should grow when needed.
        if i > 0 {
            assert!(list.capacity >= list.size);
        }
    }

    assert_eq!(list.size, rtdt::Index(10));
    assert!(list.capacity >= rtdt::Index(10));

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test reserve.
#[test]
fn test_list_reserve() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Reserve capacity for 20 elements.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            list_tydesc,
            20,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that capacity is at least 20.
    assert!(list.capacity >= rtdt::Index(20));
    assert_eq!(list.size, rtdt::Index::ZERO);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test shrink_to_fit.
#[test]
fn test_list_shrink_to_fit() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Reserve large capacity.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            list_tydesc,
            100,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert!(list.capacity >= rtdt::Index(100));

    // Push only 3 elements.
    for i in 0u32..3 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Shrink to fit.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_shrink_to_fit_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Capacity should now be 3.
    assert_eq!(list.capacity, rtdt::Index(3));
    assert_eq!(list.size, rtdt::Index(3));

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Insert/Remove Tests
// ============================================================================

/// Test insert at the beginning.
#[test]
fn test_list_insert_at_start() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push [10, 20, 30].
    for i in 1u32..=3 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Insert 5 at index 0.
    let mut value = 5u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_insert_local(
            rt,
            list_ptr,
            list_tydesc,
            0,
            &mut value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(list.size, rtdt::Index(4));

    // Verify list is now [5, 10, 20, 30].
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });

    for (idx, expected) in [(0, 5), (1, 10), (2, 20), (3, 30)] {
        let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                idx,
                option_buffer.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let tag = unsafe { *option_buffer.as_ptr() };
        assert_eq!(tag, rtdt::OptionTag::Some as u8);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, expected);
    }

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test remove from middle.
#[test]
fn test_list_remove_middle() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push [0, 10, 20, 30, 40].
    for i in 0u32..5 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Remove element at index 2 (value 20).
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_remove_local(
            rt,
            list_ptr,
            list_tydesc,
            2,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check removed value.
    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let removed_value = unsafe { *value_ptr };
    assert_eq!(removed_value, 20);

    // List should now be [0, 10, 30, 40].
    assert_eq!(list.size, rtdt::Index(4));

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// String Helper Functions
// ============================================================================

/// Create a String type descriptor.
fn create_string_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing { unused: 0 },
        },
    })
}

/// Create an Option<String> type descriptor.
fn create_option_string_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let inner_tydesc = create_string_tydesc(arena);

    let option_tydesc = arena.alloc_mut(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Option,
        size: 0,
        align: 0,
        type_info: rtdt::TyInfo {
            option: rtdt::TyInfoOption {
                inner_tydesc,
            },
        },
    });

    // Compute the layout.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc))
    };

    unsafe {
        (*option_tydesc).size = option_layout.size;
        (*option_tydesc).align = option_layout.align;
    }

    (option_tydesc, inner_tydesc)
}

/// Create a List<String> type descriptor.
fn create_list_string_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let element_tydesc = create_string_tydesc(arena);

    let list_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList {
                element_tydesc,
            },
        },
    });

    (list_tydesc, element_tydesc)
}

/// Helper to create a String from a &str using the runtime.
unsafe fn create_runtime_string(
    rt: datalove_rt::c::LocalRtHandle,
    s: &str,
    string_tydesc: *const rtdt::TyDesc,
) -> rtdt::String {
    unsafe {
        let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            string_tydesc,
        );
        let mut string = string.assume_init();

        if !s.is_empty() {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt,
                &mut string as *mut rtdt::String as *mut u8,
                string_tydesc,
                s.as_ptr(),
                (s.len() as u32).into(),
            );
        }

        string
    }
}

// ============================================================================
// String Tests
// ============================================================================

/// Test creating an empty List<String>.
#[test]
fn test_list_create_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(list.data.is_null());
    assert_eq!(list.size, rtdt::Index::ZERO);
    assert_eq!(list.capacity, rtdt::Index::ZERO);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty List<String>.
#[test]
fn test_list_destroy_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(list.data.is_null());
    assert_eq!(list.size, rtdt::Index::ZERO);
    assert_eq!(list.capacity, rtdt::Index::ZERO);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty List<String>.
#[test]
fn test_list_clear_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_clear_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, rtdt::Index::ZERO);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test pushing a single String.
#[test]
fn test_list_push_single_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe {
        let mut string_val = create_runtime_string(rt, "hello", element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            list_tydesc,
            &mut string_val as *mut rtdt::String as *mut u8,
            element_tydesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, rtdt::Index(1));
    assert!(!list.data.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test pushing multiple Strings.
#[test]
fn test_list_push_multiple_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = ["str0", "str1", "str2", "str3", "str4"];

    for s in &strings {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    assert_eq!(list.size, rtdt::Index(5));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test pop from empty List<String> returns None.
#[test]
fn test_list_pop_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            list_tydesc,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test push then pop String.
#[test]
fn test_list_push_pop_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe {
        let mut string_val = create_runtime_string(rt, "test_string", element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            list_tydesc,
            &mut string_val as *mut rtdt::String as *mut u8,
            element_tydesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, rtdt::Index(1));

    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            list_tydesc,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    assert_eq!(list.size, rtdt::Index::ZERO);

    // Clean up the cloned option value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test get from empty List<String> returns None.
#[test]
fn test_list_get_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_local(
            rt,
            list_ptr,
            list_tydesc,
            0,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test get valid String element.
#[test]
fn test_list_get_valid_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = ["first", "second", "third"];

    for s in &strings {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_local(
            rt,
            list_ptr,
            list_tydesc,
            1,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    // Clean up the cloned option value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test get out of bounds String returns None.
#[test]
fn test_list_get_out_of_bounds_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..2 {
        unsafe {
            let s = format!("str{}", i);
            let mut string_val = create_runtime_string(rt, &s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_local(
            rt,
            list_ptr,
            list_tydesc,
            5,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test capacity growth with Strings.
#[test]
fn test_list_capacity_growth_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..10 {
        unsafe {
            let s = format!("string_{}", i);
            let mut string_val = create_runtime_string(rt, &s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            if i > 0 {
                assert!(list.capacity >= list.size);
            }
        }
    }

    assert_eq!(list.size, rtdt::Index(10));
    assert!(list.capacity >= rtdt::Index(10));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test shrink_to_fit with Strings.
#[test]
fn test_list_shrink_to_fit_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            list_tydesc,
            100,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert!(list.capacity >= rtdt::Index(100));

    for i in 0..3 {
        unsafe {
            let s = format!("str{}", i);
            let mut string_val = create_runtime_string(rt, &s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_shrink_to_fit_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.capacity, rtdt::Index(3));
    assert_eq!(list.size, rtdt::Index(3));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test insert String at the beginning.
#[test]
fn test_list_insert_at_start_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for s in &["second", "third", "fourth"] {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    unsafe {
        let mut string_val = create_runtime_string(rt, "first", element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_insert_local(
            rt,
            list_ptr,
            list_tydesc,
            0,
            &mut string_val as *mut rtdt::String as *mut u8,
            element_tydesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, rtdt::Index(4));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test remove String from middle.
#[test]
fn test_list_remove_middle_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..5 {
        unsafe {
            let s = format!("string_{}", i);
            let mut string_val = create_runtime_string(rt, &s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_remove_local(
            rt,
            list_ptr,
            list_tydesc,
            2,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    assert_eq!(list.size, rtdt::Index(4));

    // Clean up the cloned option value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Set (Replace) Tests
// ============================================================================

/// Test set element at valid index.
#[test]
fn test_list_set_valid() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push [10, 20, 30].
    for i in 1u32..=3 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Set index 1 to 999.
    let mut new_value = 999u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_set_local(
            rt,
            list_ptr,
            list_tydesc,
            1,
            &mut new_value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is [10, 999, 30].
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });

    for (idx, expected) in [(0, 10), (1, 999), (2, 30)] {
        let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                idx,
                option_buffer.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let tag = unsafe { *option_buffer.as_ptr() };
        assert_eq!(tag, rtdt::OptionTag::Some as u8);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, expected);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test set element at out of bounds index returns error.
#[test]
fn test_list_set_out_of_bounds() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 2 elements.
    for i in 0u32..2 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Try to set at index 5 (out of bounds).
    let mut new_value = 999u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_set_local(
            rt,
            list_ptr,
            list_tydesc,
            5,
            &mut new_value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test set String element.
#[test]
fn test_list_set_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push ["first", "second", "third"].
    for s in &["first", "second", "third"] {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                element_tydesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    // Set index 1 to "REPLACED".
    unsafe {
        let mut new_string = create_runtime_string(rt, "REPLACED", element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_set_local(
            rt,
            list_ptr,
            list_tydesc,
            1,
            &mut new_string as *mut rtdt::String as *mut u8,
            element_tydesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, rtdt::Index(3));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Create From Slice Tests
// ============================================================================

/// Test create list from empty slice.
#[test]
fn test_list_create_from_slice_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create from empty slice (use a non-null but empty slice).
    let slice: [u32; 0] = [];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            0,
            element_tydesc,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, rtdt::Index::ZERO);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test create list from slice with multiple elements.
#[test]
fn test_list_create_from_slice_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let slice: [u32; 5] = [10, 20, 30, 40, 50];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            5,
            element_tydesc,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, rtdt::Index(5));
    assert!(list.capacity >= rtdt::Index(5));

    // Verify all elements.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });

    for (idx, expected) in slice.iter().enumerate() {
        let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                (idx as u32).into(),
                option_buffer.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let tag = unsafe { *option_buffer.as_ptr() };
        assert_eq!(tag, rtdt::OptionTag::Some as u8);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, *expected);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Extend From Slice Tests
// ============================================================================

/// Test extend empty list from slice.
#[test]
fn test_list_extend_from_slice_empty_list() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Extend with [10, 20, 30].
    let slice: [u32; 3] = [10, 20, 30];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_extend_from_slice_local(
            rt,
            list_ptr,
            list_tydesc,
            slice.as_ptr() as *const u8,
            3,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, rtdt::Index(3));

    // Verify all elements.
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });

    for (idx, expected) in slice.iter().enumerate() {
        let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                (idx as u32).into(),
                option_buffer.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, *expected);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test extend non-empty list from slice.
#[test]
fn test_list_extend_from_slice_nonempty_list() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push initial elements [1, 2].
    for i in 1u32..=2 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, rtdt::Index(2));

    // Extend with [3, 4, 5].
    let slice: [u32; 3] = [3, 4, 5];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_extend_from_slice_local(
            rt,
            list_ptr,
            list_tydesc,
            slice.as_ptr() as *const u8,
            3,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, rtdt::Index(5));

    // Verify all elements [1, 2, 3, 4, 5].
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let expected: [u32; 5] = [1, 2, 3, 4, 5];

    for (idx, exp) in expected.iter().enumerate() {
        let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                (idx as u32).into(),
                option_buffer.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, *exp);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test extend with empty slice (no-op).
#[test]
fn test_list_extend_from_slice_empty_slice() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push initial elements.
    for i in 0u32..3 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let size_before = list.size;

    // Extend with empty slice.
    let slice: [u32; 0] = [];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_extend_from_slice_local(
            rt,
            list_ptr,
            list_tydesc,
            slice.as_ptr() as *const u8,
            0,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Size should be unchanged.
    assert_eq!(list.size, size_before);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ============================================================================
// Edge Case Tests
// ============================================================================

/// Test shrink_to_fit on empty list with capacity frees the buffer.
#[test]
fn test_list_shrink_to_fit_empty_with_capacity() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Reserve capacity.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            list_tydesc,
            50,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(list.capacity >= rtdt::Index(50));
    assert!(!list.data.is_null());
    assert_eq!(list.size, rtdt::Index::ZERO);

    // Shrink to fit on empty list should free the buffer.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_shrink_to_fit_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.capacity, rtdt::Index::ZERO);
    assert!(list.data.is_null());
    assert_eq!(list.size, rtdt::Index::ZERO);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test insert at end (equivalent to push).
#[test]
fn test_list_insert_at_end() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push [10, 20].
    for i in 1u32..=2 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Insert 30 at end (index == size).
    let mut value = 30u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_insert_local(
            rt,
            list_ptr,
            list_tydesc,
            2, // index == size
            &mut value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, rtdt::Index(3));

    // Verify [10, 20, 30].
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });

    for (idx, expected) in [(0, 10), (1, 20), (2, 30)] {
        let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                idx,
                option_buffer.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, expected);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test insert beyond end returns error.
#[test]
fn test_list_insert_beyond_end() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 2 elements.
    for i in 0u32..2 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Try to insert at index 5 (beyond end).
    let mut value = 999u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_insert_local(
            rt,
            list_ptr,
            list_tydesc,
            5,
            &mut value as *mut u32 as *mut u8,
            element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Size should be unchanged.
    assert_eq!(list.size, rtdt::Index(2));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test remove last element (no shifting needed).
#[test]
fn test_list_remove_last() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push [10, 20, 30].
    for i in 1u32..=3 {
        let mut value = i * 10;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Remove last element (index 2).
    let option_layout = rtdt::layout::compute_option_layout(unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) });
    let mut option_buffer = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_remove_local(
            rt,
            list_ptr,
            list_tydesc,
            2, // last index
            option_buffer.as_mut_ptr(),
            option_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = unsafe { *option_buffer.as_ptr() };
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let removed_value = unsafe { *value_ptr };
    assert_eq!(removed_value, 30);

    assert_eq!(list.size, rtdt::Index(2));

    // Verify remaining elements [10, 20].
    for (idx, expected) in [(0, 10), (1, 20)] {
        let mut opt_buf = datalove_rt::rust::AlignedBuffer::new(option_layout.size as usize);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get_local(
                rt,
                list_ptr,
                list_tydesc,
                idx,
                opt_buf.as_mut_ptr(),
                option_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let val_ptr = unsafe {
            opt_buf.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let val = unsafe { *val_ptr };
        assert_eq!(val, expected);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clear non-empty list.
#[test]
fn test_list_clear_nonempty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push elements.
    for i in 0u32..5 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, rtdt::Index(5));
    let capacity_before = list.capacity;

    // Clear the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_clear_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Size should be 0, capacity should be preserved.
    assert_eq!(list.size, rtdt::Index::ZERO);
    assert_eq!(list.capacity, capacity_before);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test reserve when capacity is already sufficient (no-op).
#[test]
fn test_list_reserve_already_sufficient() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc(&arena);

    let mut list = rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Reserve 20.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            list_tydesc,
            20,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let capacity_after_first = list.capacity;
    assert!(capacity_after_first >= rtdt::Index(20));

    // Push some elements.
    for i in 0u32..5 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                list_tydesc,
                &mut value as *mut u32 as *mut u8,
                element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Reserve 10 (less than available).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            list_tydesc,
            10,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Capacity should be unchanged.
    assert_eq!(list.capacity, capacity_after_first);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            list_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
