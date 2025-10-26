//! Tests for btreemap runtime functions.

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

/// Create a Map<u32, u32> type descriptor.
fn create_map_u32_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let key_tydesc = create_u32_tydesc();
    let value_tydesc = create_u32_tydesc();

    let map_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Map,
        size: std::mem::size_of::<rtdt::Map>() as u32,
        align: std::mem::align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo {
            map: rtdt::TyInfoMap {
                key_tydesc: &*key_tydesc as *const rtdt::TyDesc,
                value_tydesc: &*value_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (map_tydesc, key_tydesc, value_tydesc)
}

/// Test creating an empty btreemap.
#[test]
fn test_btreemap_create_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    // Allocate space for the map
    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create the map
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify the map is empty
    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty btreemap.
#[test]
fn test_btreemap_destroy_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create then destroy the map
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify map is cleared
    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty btreemap.
#[test]
fn test_btreemap_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create the map
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Clear the empty map (should be a no-op but should succeed)
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_clear_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify map is still empty
    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test null pointer handling for create operation.
#[test]
fn test_btreemap_create_null_checks() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Test with null tydesc
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            ptr::null(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Test with null value_out
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            ptr::null_mut(),
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test null pointer handling for destroy operation.
#[test]
fn test_btreemap_destroy_null_checks() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Test with null tydesc
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            ptr::null(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Test with null value_in
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            ptr::null_mut(),
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test null pointer handling for clear operation.
#[test]
fn test_btreemap_clear_null_checks() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Test with null tydesc
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_clear_local(
            rt,
            map_ptr,
            ptr::null(),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    // Test with null value_mut
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_clear_local(
            rt,
            ptr::null_mut(),
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting a single element into an empty map.
#[test]
fn test_btreemap_insert_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create the map
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert (42, 100)
    let mut key = 42u32;
    let mut value = 100u32;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Verify map len
    assert_eq!(map.len, 1);
    assert!(!map.root.is_null());

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting multiple elements.
#[test]
fn test_btreemap_insert_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 5 entries
    for i in 0u32..5 {
        let mut key = i * 10;
        let mut value = i * 100;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify map len
    assert_eq!(map.len, 5);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test updating an existing key.
#[test]
fn test_btreemap_insert_update() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert (42, 100)
    let mut key = 42u32;
    let mut value = 100u32;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 1);

    // Update (42, 200)
    let mut key2 = 42u32;
    let mut value2 = 200u32;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key2 as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value2 as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    // Len should still be 1 (update, not insert)
    assert_eq!(map.len, 1);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting enough elements to trigger a leaf split.
#[test]
fn test_btreemap_insert_with_split() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 15 entries (capacity is 11, so this will trigger splits)
    for i in 0u32..15 {
        let mut key = i * 10;
        let mut value = i * 100;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify map len
    assert_eq!(map.len, 15);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting enough elements to trigger multi-level splits (deeper tree).
#[test]
fn test_btreemap_insert_multi_level_splits() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 30 entries - this should trigger multiple splits including internal node splits
    for i in 0u32..30 {
        let mut key = i * 10;
        let mut value = i * 100;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", i * 10);
    }

    // Verify map len
    assert_eq!(map.len, 30);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting many elements to create a deep tree.
#[test]
fn test_btreemap_insert_deep_tree() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 100 entries to create a deeper tree structure
    for i in 0u32..100 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", i);
    }

    // Verify map len
    assert_eq!(map.len, 100);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting in reverse order (stress test for tree balancing).
#[test]
fn test_btreemap_insert_reverse_order() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 50 entries in reverse order
    for i in (0u32..50).rev() {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", i);
    }

    // Verify map len
    assert_eq!(map.len, 50);

    // Clean up
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== String type helpers ====================

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

/// Create a Map<String, String> type descriptor.
fn create_map_string_string_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let key_tydesc = create_string_tydesc();
    let value_tydesc = create_string_tydesc();

    let map_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Map,
        size: std::mem::size_of::<rtdt::Map>() as u32,
        align: std::mem::align_of::<rtdt::Map>() as u32,
        type_info: rtdt::TyInfo {
            map: rtdt::TyInfoMap {
                key_tydesc: &*key_tydesc as *const rtdt::TyDesc,
                value_tydesc: &*value_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (map_tydesc, key_tydesc, value_tydesc)
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

// ==================== String tests ====================

/// Test creating an empty btreemap with String keys and values.
#[test]
fn test_btreemap_create_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty btreemap with String keys and values.
#[test]
fn test_btreemap_destroy_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty btreemap with String keys and values.
#[test]
fn test_btreemap_clear_empty_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_clear_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting a single String element into an empty map.
#[test]
fn test_btreemap_insert_single_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe {
        let mut key_str = create_runtime_string(rt, "hello", &*key_tydesc);
        let mut value_str = create_runtime_string(rt, "world", &*value_tydesc);

        let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key_str as *mut rtdt::String as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value_str as *mut rtdt::String as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        assert_eq!(map.len, 1);
        assert!(!map.root.is_null());

        // Note: key_str and value_str have been moved into the btreemap.
        // We must NOT destroy them here - the btreemap now owns them.
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting multiple String elements.
#[test]
fn test_btreemap_insert_multiple_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let keys = ["key0", "key1", "key2", "key3", "key4"];
    let values = ["val0", "val1", "val2", "val3", "val4"];

    for i in 0..5 {
        unsafe {
            let mut key_str = create_runtime_string(rt, keys[i], &*key_tydesc);
            let mut value_str = create_runtime_string(rt, values[i], &*value_tydesc);

            let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key_str as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value_str as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Note: key_str and value_str have been moved into the btreemap.
            // We must NOT destroy them here - the btreemap now owns them.
        }
    }

    assert_eq!(map.len, 5);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test updating an existing String key.
#[test]
fn test_btreemap_insert_update_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    unsafe {
        let mut key_str = create_runtime_string(rt, "mykey", &*key_tydesc);
        let mut value_str = create_runtime_string(rt, "value1", &*value_tydesc);

        let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key_str as *mut rtdt::String as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value_str as *mut rtdt::String as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(map.len, 1);

        // Note: key_str and value_str have been moved into the btreemap.
        // We must NOT destroy them here.

        let mut key_str2 = create_runtime_string(rt, "mykey", &*key_tydesc);
        let mut value_str2 = create_runtime_string(rt, "value2", &*value_tydesc);

        let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key_str2 as *mut rtdt::String as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value_str2 as *mut rtdt::String as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(map.len, 1);

        // Note: key_str2 was destroyed by the update operation (since the key already existed).
        // value_str2 was moved into the btreemap, replacing the old value.
        // We must NOT destroy either of them here.
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting enough String elements to trigger a leaf split.
#[test]
fn test_btreemap_insert_with_split_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..15 {
        unsafe {
            let key = format!("key{:03}", i);
            let value = format!("value{:03}", i);

            let mut key_str = create_runtime_string(rt, &key, &*key_tydesc);
            let mut value_str = create_runtime_string(rt, &value, &*value_tydesc);

            let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key_str as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value_str as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Note: key_str and value_str have been moved into the btreemap.
            // We must NOT destroy them here - the btreemap now owns them.
        }
    }

    assert_eq!(map.len, 15);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting enough String elements to trigger multi-level splits.
#[test]
fn test_btreemap_insert_multi_level_splits_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..30 {
        unsafe {
            let key = format!("key{:03}", i);
            let value = format!("value{:03}", i);

            let mut key_str = create_runtime_string(rt, &key, &*key_tydesc);
            let mut value_str = create_runtime_string(rt, &value, &*value_tydesc);

            let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key_str as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value_str as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", key);

            // Note: key_str and value_str have been moved into the btreemap.
            // We must NOT destroy them here - the btreemap now owns them.
        }
    }

    assert_eq!(map.len, 30);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting many String elements to create a deep tree.
#[test]
fn test_btreemap_insert_deep_tree_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..100 {
        unsafe {
            let key = format!("key{:04}", i);
            let value = format!("value{:04}", i);

            let mut key_str = create_runtime_string(rt, &key, &*key_tydesc);
            let mut value_str = create_runtime_string(rt, &value, &*value_tydesc);

            let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key_str as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value_str as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", key);

            // Note: key_str and value_str have been moved into the btreemap.
            // We must NOT destroy them here - the btreemap now owns them.
        }
    }

    assert_eq!(map.len, 100);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting String elements in reverse order.
#[test]
fn test_btreemap_insert_reverse_order_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in (0..50).rev() {
        unsafe {
            let key = format!("key{:04}", i);
            let value = format!("value{:04}", i);

            let mut key_str = create_runtime_string(rt, &key, &*key_tydesc);
            let mut value_str = create_runtime_string(rt, &value, &*value_tydesc);

            let status = datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key_str as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value_str as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            );
            assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", key);

            // Note: key_str and value_str have been moved into the btreemap.
            // We must NOT destroy them here - the btreemap now owns them.
        }
    }

    assert_eq!(map.len, 50);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== btreemap_get tests ====================

/// Test getting from an empty map returns None.
#[test]
fn test_btreemap_get_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to get a key from empty map.
    let key = 42u32;
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test getting an existing key returns Some with correct value.
#[test]
fn test_btreemap_get_existing_key() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert (42, 100).
    let mut key = 42u32;
    let mut value = 100u32;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Get the key.
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got Some.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::Some as u8);

    // Check the value.
    let value_ptr = unsafe {
        option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let retrieved_value = unsafe { *value_ptr };
    assert_eq!(retrieved_value, 100);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test getting a non-existent key returns None.
#[test]
fn test_btreemap_get_nonexistent_key() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert (10, 100), (20, 200), (30, 300).
    for i in 1u32..=3 {
        let mut key = i * 10;
        let mut value = i * 100;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Try to get a key that doesn't exist.
    let key = 42u32;
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
            option_buffer.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Check that we got None.
    let tag = option_buffer[0];
    assert_eq!(tag, rtdt::OptionTag::None as u8);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test getting multiple keys from map.
#[test]
fn test_btreemap_get_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 5 entries.
    for i in 0u32..5 {
        let mut key = i * 10;
        let mut value = i * 100;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    // Get each key and verify value.
    for i in 0u32..5 {
        let key = i * 10;
        let mut option_buffer = vec![0u8; option_layout.size as usize];

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
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
        assert_eq!(retrieved_value, i * 100);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test that get returns updated value after update.
#[test]
fn test_btreemap_get_after_update() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert (42, 100).
    let mut key = 42u32;
    let mut value = 100u32;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    // Get and verify initial value.
    let mut option_buffer = vec![0u8; option_layout.size as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
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
    assert_eq!(retrieved_value, 100);

    // Update (42, 200).
    let mut value2 = 200u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value2 as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Get and verify updated value.
    let mut option_buffer2 = vec![0u8; option_layout.size as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
            option_buffer2.as_mut_ptr(),
            &*option_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let tag2 = option_buffer2[0];
    assert_eq!(tag2, rtdt::OptionTag::Some as u8);

    let value_ptr2 = unsafe {
        option_buffer2.as_ptr().add(option_layout.payload_offset as usize) as *const u32
    };
    let retrieved_value2 = unsafe { *value_ptr2 };
    assert_eq!(retrieved_value2, 200);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test getting keys from a map with many elements (splits).
#[test]
fn test_btreemap_get_with_splits() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 50 entries.
    for i in 0u32..50 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    // Get each key and verify value.
    for i in 0u32..50 {
        let key = i;
        let mut option_buffer = vec![0u8; option_layout.size as usize];

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
                option_buffer.as_mut_ptr(),
                &*option_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let tag = option_buffer[0];
        assert_eq!(tag, rtdt::OptionTag::Some as u8, "Key {} should exist", i);

        let value_ptr = unsafe {
            option_buffer.as_ptr().add(option_layout.payload_offset as usize) as *const u32
        };
        let retrieved_value = unsafe { *value_ptr };
        assert_eq!(retrieved_value, i * 10, "Key {} value mismatch", i);
    }

    // Try to get non-existent keys.
    for i in 50u32..55 {
        let key = i;
        let mut option_buffer = vec![0u8; option_layout.size as usize];

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
                option_buffer.as_mut_ptr(),
                &*option_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let tag = option_buffer[0];
        assert_eq!(tag, rtdt::OptionTag::None as u8, "Key {} should not exist", i);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== btreemap_remove tests ====================

/// Test removing from an empty map.
#[test]
fn test_btreemap_remove_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Try to remove a key from empty map.
    let key = 42u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_remove_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing a single element.
#[test]
fn test_btreemap_remove_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert (42, 100).
    let mut key = 42u32;
    let mut value = 100u32;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_insert_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &mut key as *mut u32 as *mut u8,
            &*key_tydesc as *const rtdt::TyDesc,
            &mut value as *mut u32 as *mut u8,
            &*value_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 1);

    // Remove the key.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_remove_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing non-existent key.
#[test]
fn test_btreemap_remove_nonexistent() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert some keys.
    for i in 0u32..5 {
        let mut key = i * 10;
        let mut value = i * 100;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 5);

    // Try to remove a key that doesn't exist.
    let key = 99u32;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_remove_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const u32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 5); // Len should not change.

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing multiple elements.
#[test]
fn test_btreemap_remove_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 10 entries.
    for i in 0u32..10 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 10);

    // Remove every other key.
    for i in (0u32..10).step_by(2) {
        let key = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 5);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing all elements.
#[test]
fn test_btreemap_remove_all() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 10 entries.
    for i in 0u32..10 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 10);

    // Remove all keys.
    for i in 0u32..10 {
        let key = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 0);
    assert!(map.root.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing from a large tree with splits.
#[test]
fn test_btreemap_remove_with_splits() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 50 entries.
    for i in 0u32..50 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 50);

    // Remove half of them.
    for i in (0u32..50).step_by(2) {
        let key = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 25);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test btreemap removal with rebalancing.
/// This test specifically exercises the rebalancing logic by creating
/// a tree that will require borrowing and merging operations.
#[test]
fn test_btreemap_remove_with_rebalancing() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 100 sequential keys to create a multi-level tree.
    // With MAP_NODE_B=6, capacity is 11 and MIN_KEYS is 5.
    for i in 0u32..100 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 100);

    // Remove keys in a pattern that forces rebalancing.
    // Remove every 3rd key to create underflows.
    for i in (0u32..100).step_by(3) {
        let key = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Verify that the remaining keys are still accessible.
    let expected_remaining = 100 - (100 / 3 + 1);
    assert_eq!(map.len, expected_remaining);

    // Verify we can still get values for non-removed keys.
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    for i in 0u32..100 {
        if i % 3 != 0 {
            // Key should exist.
            let key = i;
            let mut option_result = vec![0u8; option_layout.size as usize];

            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreemap_get_local(
                    rt,
                    map_ptr as *const u8,
                    &*map_tydesc as *const rtdt::TyDesc,
                    &key as *const u32 as *const u8,
                    &*key_tydesc as *const rtdt::TyDesc,
                    option_result.as_mut_ptr(),
                    &*option_tydesc as *const rtdt::TyDesc,
                )
            };
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);

            // Check that the option tag is Some.
            assert_eq!(option_result[0], rtdt::OptionTag::Some as u8);

            // Verify the value is correct.
            unsafe {
                let value_ptr = option_result.as_ptr().add(option_layout.payload_offset as usize) as *const u32;
                let value = *value_ptr;
                assert_eq!(value, i * 10);
            }
        }
    }

    // Continue removing more keys to test deeper rebalancing.
    for i in (1u32..100).step_by(3) {
        let key = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    // Now only keys where i % 3 == 2 should remain.
    let final_remaining = (0u32..100).filter(|i| i % 3 == 2).count() as u32;
    assert_eq!(map.len, final_remaining);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test btreemap removal that exercises internal node rebalancing.
/// This test creates a larger tree to ensure internal nodes need rebalancing.
#[test]
fn test_btreemap_remove_internal_rebalancing() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert 500 keys to ensure we have multiple levels of internal nodes.
    for i in 0u32..500 {
        let mut key = i;
        let mut value = i * 10;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut u32 as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut u32 as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 500);

    // Remove keys from the middle to force internal node rebalancing.
    for i in 200u32..300 {
        let key = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 400);

    // Verify remaining keys are still accessible.
    let (option_tydesc, _inner_tydesc) = create_option_u32_tydesc();
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    for i in 0u32..200 {
        let key = i;
        let mut option_result = vec![0u8; option_layout.size as usize];

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get_local(
                rt,
                map_ptr as *const u8,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
                option_result.as_mut_ptr(),
                &*option_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(option_result[0], rtdt::OptionTag::Some as u8);
    }

    for i in 300u32..500 {
        let key = i;
        let mut option_result = vec![0u8; option_layout.size as usize];

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get_local(
                rt,
                map_ptr as *const u8,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const u32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
                option_result.as_mut_ptr(),
                &*option_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(option_result[0], rtdt::OptionTag::Some as u8);
    }

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test btreemap removal with string keys and values.
/// This tests that allocated memory is properly freed during removal.
#[test]
fn test_btreemap_remove_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert multiple string key-value pairs.
    for i in 0..20 {
        let key_str = format!("key_{:03}", i);
        let value_str = format!("value_{:03}", i);

        let mut key = unsafe {
            create_runtime_string(rt, &key_str, &*key_tydesc as *const rtdt::TyDesc)
        };
        let mut value = unsafe {
            create_runtime_string(rt, &value_str, &*value_tydesc as *const rtdt::TyDesc)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 20);

    // Remove every other key.
    for i in (0..20).step_by(2) {
        let key_str = format!("key_{:03}", i);
        let key = unsafe {
            create_runtime_string(rt, &key_str, &*key_tydesc as *const rtdt::TyDesc)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const rtdt::String as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Clean up the temporary key.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_destroy_local(
                rt,
                &key as *const rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 10);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test btreemap removal with string keys that forces rebalancing.
/// This ensures that borrowed/merged string keys are properly managed.
#[test]
fn test_btreemap_remove_string_with_rebalancing() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, value_tydesc) = create_map_string_string_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Insert enough strings to create a multi-level tree.
    for i in 0..50 {
        let key_str = format!("key_{:03}", i);
        let value_str = format!("value_{:03}", i);

        let mut key = unsafe {
            create_runtime_string(rt, &key_str, &*key_tydesc as *const rtdt::TyDesc)
        };
        let mut value = unsafe {
            create_runtime_string(rt, &value_str, &*value_tydesc as *const rtdt::TyDesc)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_insert_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &mut key as *mut rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
                &mut value as *mut rtdt::String as *mut u8,
                &*value_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }
    assert_eq!(map.len, 50);

    // Remove keys in a pattern that forces rebalancing.
    for i in (0..50).step_by(3) {
        let key_str = format!("key_{:03}", i);
        let key = unsafe {
            create_runtime_string(rt, &key_str, &*key_tydesc as *const rtdt::TyDesc)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_remove_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const rtdt::String as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        // Clean up the temporary key.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_destroy_local(
                rt,
                &key as *const rtdt::String as *mut u8,
                &*key_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let expected_remaining = 50 - (50 / 3 + 1);
    assert_eq!(map.len, expected_remaining);

    // Clean up.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== clone_from_slice tests ====================

/// Helper to create a (u32, u32) tuple type descriptor.
fn create_tuple_u32_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<[rtdt::TyInfoTupleField; 2]>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let key_tydesc = create_u32_tydesc();
    let value_tydesc = create_u32_tydesc();

    // Create tuple fields with proper alignment.
    // (u32, u32) layout: first field at offset 0, second at offset 4.
    let fields = Box::new([
        rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: &*key_tydesc as *const rtdt::TyDesc,
        },
        rtdt::TyInfoTupleField {
            offset: 4,
            tydesc: &*value_tydesc as *const rtdt::TyDesc,
        },
    ]);

    let tuple_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Tuple,
        size: 8,
        align: 4,
        type_info: rtdt::TyInfo {
            tuple: rtdt::TyInfoTuple {
                num_fields: 2,
                fields: fields.as_ptr(),
            },
        },
    });

    (tuple_tydesc, fields, key_tydesc, value_tydesc)
}

#[test]
fn test_btreemap_clone_from_slice_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
    let (tuple_tydesc, _fields, _k_td, _v_td) = create_tuple_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create map from empty slice.
    let status = unsafe {
        datalove_rt::impls::btreemap::btreemap_clone_from_slice_impl(
            &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal),
            map_ptr,
            rtdt::TyDescRef::from_ptr(&*map_tydesc as *const rtdt::TyDesc),
            ptr::null(),
            0,
            &*tuple_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 0);
    assert!(map.root.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_btreemap_clone_from_slice_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
    let (tuple_tydesc, _fields, _k_td, _v_td) = create_tuple_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create a slice with a single (key, value) pair.
    #[repr(C)]
    struct Tuple2U32 {
        key: rtdt::U32,
        value: rtdt::U32,
    }

    let slice_data = vec![
        Tuple2U32 { key: rtdt::U32(42), value: rtdt::U32(100) },
    ];

    let status = unsafe {
        datalove_rt::impls::btreemap::btreemap_clone_from_slice_impl(
            &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal),
            map_ptr,
            rtdt::TyDescRef::from_ptr(&*map_tydesc as *const rtdt::TyDesc),
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*tuple_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 1);

    // Verify we can retrieve the value.
    let (option_tydesc, _inner) = create_option_u32_tydesc();
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let key = rtdt::U32(42);
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const rtdt::U32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
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
    assert_eq!(retrieved_value, 100);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_btreemap_clone_from_slice_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
    let (tuple_tydesc, _fields, _k_td, _v_td) = create_tuple_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    #[repr(C)]
    struct Tuple2U32 {
        key: rtdt::U32,
        value: rtdt::U32,
    }

    let slice_data = vec![
        Tuple2U32 { key: rtdt::U32(10), value: rtdt::U32(100) },
        Tuple2U32 { key: rtdt::U32(20), value: rtdt::U32(200) },
        Tuple2U32 { key: rtdt::U32(30), value: rtdt::U32(300) },
        Tuple2U32 { key: rtdt::U32(40), value: rtdt::U32(400) },
        Tuple2U32 { key: rtdt::U32(50), value: rtdt::U32(500) },
    ];

    let status = unsafe {
        datalove_rt::impls::btreemap::btreemap_clone_from_slice_impl(
            &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal),
            map_ptr,
            rtdt::TyDescRef::from_ptr(&*map_tydesc as *const rtdt::TyDesc),
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*tuple_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 5);

    // Verify all values are retrievable.
    let (option_tydesc, _inner) = create_option_u32_tydesc();
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };

    for (k, v) in &[(10, 100), (20, 200), (30, 300), (40, 400), (50, 500)] {
        let mut option_buffer = vec![0u8; option_layout.size as usize];

        let key = rtdt::U32(*k);
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get_local(
                rt,
                map_ptr,
                &*map_tydesc as *const rtdt::TyDesc,
                &key as *const rtdt::U32 as *const u8,
                &*key_tydesc as *const rtdt::TyDesc,
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
        assert_eq!(retrieved_value, *v, "Expected value {} for key {}", v, k);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

#[test]
fn test_btreemap_clone_from_slice_with_duplicates() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();
    let (tuple_tydesc, _fields, _k_td, _v_td) = create_tuple_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    #[repr(C)]
    struct Tuple2U32 {
        key: rtdt::U32,
        value: rtdt::U32,
    }

    // Slice with duplicate keys - last value should win.
    let slice_data = vec![
        Tuple2U32 { key: rtdt::U32(10), value: rtdt::U32(100) },
        Tuple2U32 { key: rtdt::U32(20), value: rtdt::U32(200) },
        Tuple2U32 { key: rtdt::U32(10), value: rtdt::U32(999) },  // Duplicate key.
    ];

    let status = unsafe {
        datalove_rt::impls::btreemap::btreemap_clone_from_slice_impl(
            &mut *(rt as *mut datalove_rt::impls::rt_local::RtLocal),
            map_ptr,
            rtdt::TyDescRef::from_ptr(&*map_tydesc as *const rtdt::TyDesc),
            slice_data.as_ptr() as *const u8,
            slice_data.len() as u32,
            &*tuple_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(map.len, 2, "Map should have 2 unique keys");

    // Verify key 10 has the updated value.
    let (option_tydesc, _inner) = create_option_u32_tydesc();
    let option_layout = unsafe {
        rtdt::layout::compute_option_layout(&*option_tydesc as *const rtdt::TyDesc)
    };
    let mut option_buffer = vec![0u8; option_layout.size as usize];

    let key = rtdt::U32(10);
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_get_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
            &key as *const rtdt::U32 as *const u8,
            &*key_tydesc as *const rtdt::TyDesc,
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
    assert_eq!(retrieved_value, 999, "Duplicate key should have last value");

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
