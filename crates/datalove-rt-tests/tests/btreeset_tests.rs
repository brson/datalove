//! Tests for btreeset runtime functions.

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

/// Create a Set<u32> type descriptor.
fn create_set_u32_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let element_tydesc = create_u32_tydesc();

    let set_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: rtdt::TyInfoSet {
                element_tydesc: &*element_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (set_tydesc, element_tydesc)
}

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

/// Create a Set<String> type descriptor.
fn create_set_string_tydesc() -> (Box<rtdt::TyDesc>, Box<rtdt::TyDesc>) {
    let element_tydesc = create_string_tydesc();

    let set_tydesc = Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: rtdt::TyInfoSet {
                element_tydesc: &*element_tydesc as *const rtdt::TyDesc,
            },
        },
    });

    (set_tydesc, element_tydesc)
}

/// Helper to create a String from a &str using the runtime.
unsafe fn create_runtime_string(
    rt: datalove_rt::LocalRtHandle,
    s: &str,
    string_tydesc: *const rtdt::TyDesc,
) -> rtdt::String {
    unsafe {
        let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
        datalove_rt::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            string_tydesc,
        );
        let mut string = string.assume_init();

        if !s.is_empty() {
            datalove_rt::dtlv_rti_string_push_bytes_local(
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

// ==================== Basic Operations - U32 ====================

/// Test creating an empty btreeset.
#[test]
fn test_btreeset_create_empty() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, _element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty btreeset.
#[test]
fn test_btreeset_destroy_empty() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, _element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty btreeset.
#[test]
fn test_btreeset_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, _element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test inserting a single element.
#[test]
fn test_btreeset_insert_single() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(was_inserted, 1);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test inserting multiple elements.
#[test]
fn test_btreeset_insert_multiple() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..10 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok, "Failed to insert element {}", i);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 10);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test inserting a duplicate element.
#[test]
fn test_btreeset_insert_duplicate() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(was_inserted, 1);
    assert_eq!(set.len, 1);

    // Insert duplicate.
    let mut element2 = 42u32;
    let mut was_inserted2 = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element2 as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted2,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(was_inserted2, 0);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test contains with existing element.
#[test]
fn test_btreeset_contains_existing() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(was_inserted, 1);

    let search_element = 42u32;
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(contains, 1);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test contains with nonexistent element.
#[test]
fn test_btreeset_contains_nonexistent() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let search_element = 99u32;
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(contains, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test removing existing element.
#[test]
fn test_btreeset_remove_existing() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(set.len, 1);

    let remove_element = 42u32;
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &remove_element as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(was_removed, 1);
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test removing nonexistent element.
#[test]
fn test_btreeset_remove_nonexistent() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let remove_element = 99u32;
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &remove_element as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(was_removed, 0);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test clearing a nonempty set.
#[test]
fn test_btreeset_clear_nonempty() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..10 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    assert_eq!(set.len, 10);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with a single element.
#[test]
fn test_btreeset_clone_from_slice_single() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice = vec![42u32];

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

// ==================== Large-Scale Tests - 1000 Elements ====================

/// Test inserting 1000 elements sequentially.
#[test]
fn test_btreeset_insert_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok, "Failed to insert element {}", i);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test inserting 1000 elements in reverse order.
#[test]
fn test_btreeset_insert_1000_reverse() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in (0u32..1000).rev() {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok, "Failed to insert element {}", i);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test inserting 1000 elements in random order (using hash-based pseudo-random).
#[test]
fn test_btreeset_insert_1000_random() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let mut unique_count = 0u32;
    for i in 0u32..1000 {
        let mut element = (i.wrapping_mul(2654435761)) % 1000;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok, "Failed to insert element {}", element);
        if was_inserted == 1 {
            unique_count += 1;
        }
    }

    assert_eq!(set.len, unique_count, "Set length should match number of unique insertions");

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test contains check on 1000-element set.
#[test]
fn test_btreeset_contains_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    for i in 0u32..1000 {
        let search_element = i;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok, "Failed contains check for element {}", i);
        assert_eq!(contains, 1, "Element {} should be in set", i);
    }

    let search_element = 1001u32;
    let mut contains = 0u8;
    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            &*element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(contains, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test removing all 1000 elements.
#[test]
fn test_btreeset_remove_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    for i in 0u32..1000 {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok, "Failed to remove element {}", i);
        assert_eq!(was_removed, 1);
    }

    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with 1000 elements.
#[test]
fn test_btreeset_clone_from_slice_1000() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = (0u32..1000).collect();

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            &*element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);
    assert_eq!(set.len, 1000);

    for i in 0u32..1000 {
        let search_element = i;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
        assert_eq!(contains, 1, "Element {} should be in set", i);
    }

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test insert/remove cycles with 1000 elements.
#[test]
fn test_btreeset_insert_remove_cycles_1000() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    for i in 0u32..500 {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    assert_eq!(set.len, 500);

    for i in 1000u32..1500 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}

/// Test clearing set with 1000 elements.
#[test]
fn test_btreeset_clear_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::dtlv_rti_init();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc();

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                &*set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                &*element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            &*set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    let status = unsafe { datalove_rt::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::RtStatus::Ok);

    Ok(())
}
