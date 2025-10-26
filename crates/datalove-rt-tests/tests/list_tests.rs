//! Tests for list runtime functions.

use rmx::prelude::*;
use datalove_rt::rtdt;
use std::ptr;

/// Create a u32 type descriptor.
fn create_u32_tydesc() -> Box<rtdt::TyDesc> {
    Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

/// Create an Option<u32> type descriptor.
fn create_option_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let inner_tydesc = create_u32_tydesc();

    // Create the option tydesc first with placeholder size/align.
    let mut option_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Option,
        size: 0,
        align: 0,
        type_info: rtdt::TyInfo {
            option: rtdt::TyInfoOption {
                inner_tydesc: &*inner_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    // Now compute the layout.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    // Update size and align.
    option_tydesc.size = option_layout.size;
    option_tydesc.align = option_layout.align;

    (option_tydesc, inner_tydesc)
}

/// Create a List<u32> type descriptor.
fn create_list_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let element_tydesc = create_u32_tydesc();

    let list_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList {
                element_tydesc: &*element_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc();

    // Allocate space for the list.
    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify the list is empty.
    assert!(list.data.is_null());
    assert_eq!(list.size, 0);
    assert_eq!(list.capacity, 0);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create then destroy the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is cleared.
    assert!(list.data.is_null());
    assert_eq!(list.size, 0);
    assert_eq!(list.capacity, 0);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty list.
#[test]
fn test_list_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clear the empty list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_clear_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list is still empty.
    assert_eq!(list.size, 0);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Create the list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 42.
    let mut value = 42u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify list size.
    assert_eq!(list.size, 1);
    assert!(!list.data.is_null());

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify list size.
    assert_eq!(list.size, 5);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to pop from empty list.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Push 42.
    let mut value = 42u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(list.size, 1);

    // Pop.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got Some(42).
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let retrieved_value = unsafe { *value_ptr };
    assert_eq!(retrieved_value, 42);

    // List should be empty now.
    assert_eq!(list.size, 0);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to get element at index 0.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            0,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Get element at index 1.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            1,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got Some(10).
    let tag = option_buffer[0];
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
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Try to get element at index 5.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            5,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Capacity should grow when needed.
        if i > 0 {
            assert!(list.capacity >= list.size);
        }
    }

    assert_eq!(list.size, 10);
    assert!(list.capacity >= 10);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Reserve capacity for 20 elements.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            20,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that capacity is at least 20.
    assert!(list.capacity >= 20);
    assert_eq!(list.size, 0);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Reserve large capacity.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            100,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert!(list.capacity >= 100);

    // Push only 3 elements.
    for i in 0u32..3 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Shrink to fit.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_shrink_to_fit_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Capacity should now be 3.
    assert_eq!(list.capacity, 3);
    assert_eq!(list.size, 3);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
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
            &*list_tydesc as *const rtdt::TyDesc,
            0,
            &mut value as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(list.size, 4);

    // Verify list is now [5, 10, 20, 30].
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    for (idx, expected) in [(0, 5), (1, 10), (2, 20), (3, 30)] {
        let mut option_buffer = vec![0u8; option_layout.size as usize];
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_get(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                idx,
                option_buffer.as_mut_ptr(),
                &*option_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let tag = option_buffer[0];
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
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
                &*list_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Remove element at index 2 (value 20).
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_remove_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            2,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check removed value.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let removed_value = unsafe { *value_ptr };
    assert_eq!(removed_value, 20);

    // List should now be [0, 10, 30, 40].
    assert_eq!(list.size, 4);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
fn create_string_tydesc() -> Box<rtdt::TyDesc> {
    Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

/// Create an Option<String> type descriptor.
fn create_option_string_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let inner_tydesc = create_string_tydesc();

    let mut option_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Option,
        size: 0,
        align: 0,
        type_info: rtdt::TyInfo {
            option: rtdt::TyInfoOption {
                inner_tydesc: &*inner_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    // Compute the layout.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    option_tydesc.size = option_layout.size;
    option_tydesc.align = option_layout.align;

    (option_tydesc, inner_tydesc)
}

/// Create a List<String> type descriptor.
fn create_list_string_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let element_tydesc = create_string_tydesc();

    let list_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList {
                element_tydesc: &*element_tydesc as *const rtdt::TyDesc,
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
                s.len() as u32,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(list.data.is_null());
    assert_eq!(list.size, 0);
    assert_eq!(list.capacity, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(list.data.is_null());
    assert_eq!(list.size, 0);
    assert_eq!(list.capacity, 0);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty List<String>.
#[test]
fn test_list_clear_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_clear_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.size, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe {
        let mut string_val = create_runtime_string(rt, "hello", &*element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            &mut string_val as *mut rtdt::String as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, 1);
    assert!(!list.data.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = ["str0", "str1", "str2", "str3", "str4"];

    for s in &strings {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    assert_eq!(list.size, 5);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe {
        let mut string_val = create_runtime_string(rt, "test_string", &*element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_push_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            &mut string_val as *mut rtdt::String as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, 1);

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    assert_eq!(list.size, 0);

    // Clean up the cloned option value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, _element_tydesc) = create_list_string_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            0,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = ["first", "second", "third"];

    for s in &strings {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            1,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    // Clean up the cloned option value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..2 {
        unsafe {
            let s = format!("str{}", i);
            let mut string_val = create_runtime_string(rt, &s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            5,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..10 {
        unsafe {
            let s = format!("string_{}", i);
            let mut string_val = create_runtime_string(rt, &s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            if i > 0 {
                assert!(list.capacity >= list.size);
            }
        }
    }

    assert_eq!(list.size, 10);
    assert!(list.capacity >= 10);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            100,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert!(list.capacity >= 100);

    for i in 0..3 {
        unsafe {
            let s = format!("str{}", i);
            let mut string_val = create_runtime_string(rt, &s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_shrink_to_fit_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert_eq!(list.capacity, 3);
    assert_eq!(list.size, 3);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for s in &["second", "third", "fourth"] {
        unsafe {
            let mut string_val = create_runtime_string(rt, s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    unsafe {
        let mut string_val = create_runtime_string(rt, "first", &*element_tydesc);
        let status = datalove_rt::c::dtlv_rti_list_insert_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            0,
            &mut string_val as *mut rtdt::String as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(list.size, 4);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
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
    assert!(!rt.is_null());

    let (list_tydesc, element_tydesc) = create_list_string_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_string_tydesc();

    let mut list = rtdt::List {
        data: ptr::null(),
        size: 0,
        capacity: 0,
    };
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..5 {
        unsafe {
            let s = format!("string_{}", i);
            let mut string_val = create_runtime_string(rt, &s, &*element_tydesc);
            let status = datalove_rt::c::dtlv_rti_list_push_local(
                rt,
                list_ptr,
                &*list_tydesc as *const rtdt::TyDesc,
                &mut string_val as *mut rtdt::String as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_remove_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
            2,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    assert_eq!(list.size, 4);

    // Clean up the cloned option value.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_destroy_local(
            rt,
            list_ptr,
            &*list_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
