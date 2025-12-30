//! Tests for btreeset runtime functions.

use rmx::prelude::*;
use datalove_rt::rtdt;
use std::cell::RefCell;
use std::ptr;

// ============================================================================
// Type Descriptor Arena
// ============================================================================

struct TyDescArena {
    ptrs: RefCell<Vec<*mut rtdt::TyDesc>>,
    tuple_fields: RefCell<Vec<*mut [rtdt::TyInfoTupleField; 2]>>,
}

impl TyDescArena {
    fn new() -> Self {
        Self {
            ptrs: RefCell::new(Vec::new()),
            tuple_fields: RefCell::new(Vec::new()),
        }
    }

    fn alloc(&self, td: rtdt::TyDesc) -> *const rtdt::TyDesc {
        let ptr = Box::into_raw(Box::new(td));
        self.ptrs.borrow_mut().push(ptr);
        ptr
    }

    fn alloc_tuple_fields(&self, fields: [rtdt::TyInfoTupleField; 2]) -> *const rtdt::TyInfoTupleField {
        let ptr = Box::into_raw(Box::new(fields));
        self.tuple_fields.borrow_mut().push(ptr);
        ptr as *const rtdt::TyInfoTupleField
    }
}

impl Drop for TyDescArena {
    fn drop(&mut self) {
        for &ptr in self.ptrs.borrow().iter() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
        for &ptr in self.tuple_fields.borrow().iter() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }
}

// ============================================================================
// Type Descriptor Helpers
// ============================================================================

fn create_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

fn create_set_u32_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let element_tydesc = create_u32_tydesc(arena);

    let set_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: rtdt::TyInfoSet {
                element_tydesc,
            },
        },
    });

    (set_tydesc, element_tydesc)
}

fn create_string_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

fn create_set_string_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let element_tydesc = create_string_tydesc(arena);

    let set_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: rtdt::TyInfoSet {
                element_tydesc,
            },
        },
    });

    (set_tydesc, element_tydesc)
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

// ==================== Basic Operations - U32 ====================

/// Test creating an empty btreeset.
#[test]
fn test_btreeset_create_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, _element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test destroying an empty btreeset.
#[test]
fn test_btreeset_destroy_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, _element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing an empty btreeset.
#[test]
fn test_btreeset_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, _element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting a single element.
#[test]
fn test_btreeset_insert_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted, 1);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting multiple elements.
#[test]
fn test_btreeset_insert_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..10 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert element {}", i);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 10);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting a duplicate element.
#[test]
fn test_btreeset_insert_duplicate() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted, 1);
    assert_eq!(set.len, 1);

    // Insert duplicate.
    let mut element2 = 42u32;
    let mut was_inserted2 = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element2 as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted2,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted2, 0);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test contains with existing element.
#[test]
fn test_btreeset_contains_existing() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted, 1);

    let search_element = 42u32;
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(contains, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test contains with nonexistent element.
#[test]
fn test_btreeset_contains_nonexistent() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let search_element = 99u32;
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(contains, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing existing element.
#[test]
fn test_btreeset_remove_existing() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 1);

    let remove_element = 42u32;
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &remove_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_removed, 1);
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing nonexistent element.
#[test]
fn test_btreeset_remove_nonexistent() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let remove_element = 99u32;
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &remove_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_removed, 0);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing a nonempty set.
#[test]
fn test_btreeset_clear_nonempty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..10 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 10);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with a single element.
#[test]
fn test_btreeset_clone_from_slice_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice = vec![42u32];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== Large-Scale Tests - 1000 Elements ====================

/// Test inserting 1000 elements sequentially.
#[test]
fn test_btreeset_insert_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert element {}", i);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting 1000 elements in reverse order.
#[test]
fn test_btreeset_insert_1000_reverse() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in (0u32..1000).rev() {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert element {}", i);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting 1000 elements in random order (using hash-based pseudo-random).
#[test]
fn test_btreeset_insert_1000_random() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut unique_count = 0u32;
    for i in 0u32..1000 {
        let mut element = (i.wrapping_mul(2654435761)) % 1000;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert element {}", element);
        if was_inserted == 1 {
            unique_count += 1;
        }
    }

    assert_eq!(set.len, unique_count, "Set length should match number of unique insertions");

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test contains check on 1000-element set.
#[test]
fn test_btreeset_contains_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    for i in 0u32..1000 {
        let search_element = i;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed contains check for element {}", i);
        assert_eq!(contains, 1, "Element {} should be in set", i);
    }

    let search_element = 1001u32;
    let mut contains = 0u8;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(contains, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing all 1000 elements.
#[test]
fn test_btreeset_remove_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    for i in 0u32..1000 {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to remove element {}", i);
        assert_eq!(was_removed, 1);
    }

    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with 1000 elements.
#[test]
fn test_btreeset_clone_from_slice_1000() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = (0u32..1000).collect();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 1000);

    for i in 0u32..1000 {
        let search_element = i;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(contains, 1, "Element {} should be in set", i);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test insert/remove cycles with 1000 elements.
#[test]
fn test_btreeset_insert_remove_cycles_1000() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    for i in 0u32..500 {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 500);

    for i in 1000u32..1500 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing set with 1000 elements.
#[test]
fn test_btreeset_clear_1000_elements() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..1000 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 1000);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== Clone From Slice Tests ====================

/// Test clone from empty slice.
#[test]
fn test_btreeset_clone_from_slice_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = vec![];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 0);
    assert!(set.root.is_null());

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with multiple elements.
#[test]
fn test_btreeset_clone_from_slice_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = vec![10, 20, 30, 40, 50];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 5);

    for &val in &slice {
        let search_element = val;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(contains, 1, "Element {} should be in set", val);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with duplicates.
#[test]
fn test_btreeset_clone_from_slice_with_duplicates() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = vec![10, 20, 10, 30, 20, 10];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 3, "Set should have 3 unique elements");

    let expected = vec![10u32, 20, 30];
    for &val in &expected {
        let search_element = val;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(contains, 1, "Element {} should be in set", val);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice with strings.
#[test]
fn test_btreeset_clone_from_slice_strings() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let strings = vec!["apple", "banana", "cherry"];
    let mut runtime_strings: Vec<rtdt::String> = strings
        .iter()
        .map(|s| unsafe { create_runtime_string(rt, s, element_tydesc as *const rtdt::TyDesc) })
        .collect();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            runtime_strings.as_ptr() as *const u8,
            runtime_strings.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 3);

    for runtime_str in &mut runtime_strings {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_destroy_local(
                rt,
                runtime_str as *mut rtdt::String as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clone from slice in reverse order.
#[test]
fn test_btreeset_clone_from_slice_reverse_order() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = (0u32..100).rev().collect();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            slice.len() as u32,
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 100);

    for i in 0u32..100 {
        let search_element = i;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(contains, 1, "Element {} should be in set", i);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== String Element Tests ====================

/// Test inserting a single string element.
#[test]
fn test_btreeset_string_insert_single() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted, 1);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting multiple string elements.
#[test]
fn test_btreeset_string_insert_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = vec!["apple", "banana", "cherry", "date", "elderberry"];

    for s in &strings {
        let mut element = unsafe { create_runtime_string(rt, s, element_tydesc as *const rtdt::TyDesc) };
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut rtdt::String as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 5);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test checking contains for string elements.
#[test]
fn test_btreeset_string_contains() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let search = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search as *const rtdt::String as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(contains, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &search as *const rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test removing string elements.
#[test]
fn test_btreeset_string_remove() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, 1);

    let search = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search as *const rtdt::String as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_removed, 1);
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &search as *const rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test clearing set with strings.
#[test]
fn test_btreeset_string_clear() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = vec!["apple", "banana", "cherry"];

    for s in &strings {
        let mut element = unsafe { create_runtime_string(rt, s, element_tydesc as *const rtdt::TyDesc) };
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut rtdt::String as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 3);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test inserting 100 string elements.
#[test]
fn test_btreeset_string_insert_100() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0..100 {
        let key = format!("key_{:03}", i);
        let mut element = unsafe { create_runtime_string(rt, &key, element_tydesc as *const rtdt::TyDesc) };
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut rtdt::String as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "Failed to insert key {}", key);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 100);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test string ordering.
#[test]
fn test_btreeset_string_ordering() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = vec!["zebra", "apple", "mango", "banana"];

    for s in &strings {
        let mut element = unsafe { create_runtime_string(rt, s, element_tydesc as *const rtdt::TyDesc) };
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut rtdt::String as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 4);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test empty string.
#[test]
fn test_btreeset_string_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element = unsafe { create_runtime_string(rt, "", element_tydesc as *const rtdt::TyDesc) };
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted, 1);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test unicode strings.
#[test]
fn test_btreeset_string_unicode() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let strings = vec!["hello", "world", "rust"];

    for s in &strings {
        let mut element = unsafe { create_runtime_string(rt, s, element_tydesc as *const rtdt::TyDesc) };
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut rtdt::String as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 3);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test duplicate string insertions.
#[test]
fn test_btreeset_string_duplicates() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_string_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut element1 = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut was_inserted1 = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element1 as *mut rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted1,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted1, 1);
    assert_eq!(set.len, 1);

    let mut element2 = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
    let mut was_inserted2 = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut element2 as *mut rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted2,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted2, 0);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &element2 as *const rtdt::String as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== B-tree Structure Tests ====================

/// Test sequential insertions (small).
#[test]
fn test_btreeset_small_inserts_sequential() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..20 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 20);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test reverse insertions (small).
#[test]
fn test_btreeset_small_inserts_reverse() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in (0u32..20).rev() {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 20);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test repeated clear and refill.
#[test]
fn test_btreeset_repeated_clear_and_refill() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for _cycle in 0..3 {
        for i in 0u32..20 {
            let mut element = i;
            let mut was_inserted = 0u8;

            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreeset_insert_local(
                    rt,
                    set_ptr,
                    set_tydesc as *const rtdt::TyDesc,
                    &mut element as *mut u32 as *mut u8,
                    element_tydesc as *const rtdt::TyDesc,
                    &mut was_inserted,
                )
            };
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }

        assert_eq!(set.len, 20);

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_clear_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(set.len, 0);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test interleaved operations.
#[test]
fn test_btreeset_interleaved_ops() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..50 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        if i % 3 == 0 {
            let search_element = i / 2;
            let mut contains = 0u8;

            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreeset_contains_local(
                    rt,
                    set_ptr,
                    set_tydesc as *const rtdt::TyDesc,
                    &search_element as *const u32 as *const u8,
                    element_tydesc as *const rtdt::TyDesc,
                    &mut contains,
                )
            };
            assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        }
    }

    assert_eq!(set.len, 50);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test remove every other element.
#[test]
fn test_btreeset_remove_pattern_every_other() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..100 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 100);

    for i in (0u32..100).step_by(2) {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_removed, 1);
    }

    assert_eq!(set.len, 50);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test remove every third element.
#[test]
fn test_btreeset_remove_pattern_every_third() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..100 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 100);

    let mut removed_count = 0;
    for i in (0u32..100).step_by(3) {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        if was_removed == 1 {
            removed_count += 1;
        }
    }

    assert_eq!(set.len, 100 - removed_count);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test boundary values.
#[test]
fn test_btreeset_boundary_values() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let boundary_values = vec![0u32, 1, u32::MAX - 1, u32::MAX];

    for &val in &boundary_values {
        let mut element = val;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 4);

    for &val in &boundary_values {
        let search_element = val;
        let mut contains = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_contains_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &search_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut contains,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(contains, 1, "Boundary value {} should be in set", val);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test ascending insert, descending remove.
#[test]
fn test_btreeset_ascending_descending_pattern() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..50 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 50);

    for i in (0u32..50).rev() {
        let remove_element = i;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_removed, 1);
    }

    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
// ==================== Error Handling Tests ====================

/// Test contains on empty set.
#[test]
fn test_btreeset_contains_empty_set() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let search_element = 42u32;
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(contains, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test remove from empty set.
#[test]
fn test_btreeset_remove_from_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let remove_element = 42u32;
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &remove_element as *const u32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_removed, 0);
    assert_eq!(set.len, 0);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test multiple clears.
#[test]
fn test_btreeset_multiple_clears() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..10 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    for _i in 0..3 {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_clear_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(set.len, 0);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test insert after clear.
#[test]
fn test_btreeset_insert_after_clear() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 0u32..10 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for i in 20u32..30 {
        let mut element = i;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(was_inserted, 1);
    }

    assert_eq!(set.len, 10);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test duplicate inserts many times.
#[test]
fn test_btreeset_duplicate_inserts_many() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for _i in 0..100 {
        let mut element = 42u32;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

/// Test alternating insert/remove of same element.
#[test]
fn test_btreeset_alternating_insert_remove_same() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    for _i in 0..10 {
        let mut element = 42u32;
        let mut was_inserted = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &mut element as *mut u32 as *mut u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);

        let remove_element = 42u32;
        let mut was_removed = 0u8;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
                &remove_element as *const u32 as *const u8,
                element_tydesc as *const rtdt::TyDesc,
                &mut was_removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(set.len, 0);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}

// ==================== Tuple Element Tests ====================

/// Create a (u32, u32) tuple type descriptor.
fn create_tuple_u32_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    let field0_tydesc = create_u32_tydesc(arena);
    let field1_tydesc = create_u32_tydesc(arena);

    let fields = arena.alloc_tuple_fields([
        rtdt::TyInfoTupleField {
            offset: 0,
            tydesc: field0_tydesc,
        },
        rtdt::TyInfoTupleField {
            offset: 4,
            tydesc: field1_tydesc,
        },
    ]);

    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Tuple,
        size: 8,
        align: 4,
        type_info: rtdt::TyInfo {
            tuple: rtdt::TyInfoTuple {
                num_fields: 2,
                fields,
            },
        },
    })
}

/// Create a Set<(u32, u32)> type descriptor.
fn create_set_tuple_u32_u32_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let tuple_tydesc = create_tuple_u32_u32_tydesc(arena);

    let set_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: rtdt::TyInfoSet {
                element_tydesc: tuple_tydesc,
            },
        },
    });

    (set_tydesc, tuple_tydesc)
}

/// Test set with tuple elements.
#[test]
fn test_btreeset_tuples() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let (set_tydesc, element_tydesc) = create_set_tuple_u32_u32_tydesc(&arena);

    let mut set = rtdt::Set {
        root: ptr::null(),
        len: 0,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_create_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    #[repr(C)]
    struct Tuple2U32 {
        field0: u32,
        field1: u32,
    }

    let mut tuple1 = Tuple2U32 { field0: 10, field1: 100 };
    let mut was_inserted1 = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut tuple1 as *mut Tuple2U32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted1,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted1, 1);
    assert_eq!(set.len, 1);

    let mut tuple2 = Tuple2U32 { field0: 20, field1: 200 };
    let mut was_inserted2 = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &mut tuple2 as *mut Tuple2U32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted2,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_inserted2, 1);
    assert_eq!(set.len, 2);

    let search_tuple = Tuple2U32 { field0: 10, field1: 100 };
    let mut contains = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_contains_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &search_tuple as *const Tuple2U32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut contains,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(contains, 1);

    let remove_tuple = Tuple2U32 { field0: 10, field1: 100 };
    let mut was_removed = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_remove_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
            &remove_tuple as *const Tuple2U32 as *const u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_removed,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(was_removed, 1);
    assert_eq!(set.len, 1);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    Ok(())
}
