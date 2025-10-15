//! Tests for btreemap runtime functions.

use rmx::prelude::*;
use datalove_rt::rtdt;
use std::ptr;

/// Create a Map<u32, u32> type descriptor.
fn create_map_u32_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let key_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    });

    let value_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    });

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
    let rt = datalove_rt::dtlv_rti_init();
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
        datalove_rt::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify the map is empty
    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    // Clean up
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty btreemap.
#[test]
fn test_btreemap_destroy_empty() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create then destroy the map
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify map is cleared
    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty btreemap.
#[test]
fn test_btreemap_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Create the map
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Clear the empty map (should be a no-op but should succeed)
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_clear_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    // Verify map is still empty
    assert!(map.root.is_null());
    assert_eq!(map.len, 0);

    // Clean up
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test null pointer handling for create operation.
#[test]
fn test_btreemap_create_null_checks() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Test with null tydesc
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_create_local(
            rt,
            map_ptr,
            ptr::null(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    // Test with null value_out
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_create_local(
            rt,
            ptr::null_mut(),
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test null pointer handling for destroy operation.
#[test]
fn test_btreemap_destroy_null_checks() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Test with null tydesc
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_destroy_local(
            rt,
            map_ptr,
            ptr::null(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    // Test with null value_in
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_destroy_local(
            rt,
            ptr::null_mut(),
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test null pointer handling for clear operation.
#[test]
fn test_btreemap_clear_null_checks() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (map_tydesc, _key_tydesc, _value_tydesc) = create_map_u32_u32_tydesc();

    let mut map = rtdt::Map {
        root: ptr::null(),
        len: 0,
    };
    let map_ptr = &mut map as *mut rtdt::Map as *mut u8;

    // Test with null tydesc
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_clear_local(
            rt,
            map_ptr,
            ptr::null(),
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    // Test with null value_mut
    let status = unsafe {
        datalove_rt::dtlv_rti_btreemap_clear_local(
            rt,
            ptr::null_mut(),
            &*map_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Error);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}
