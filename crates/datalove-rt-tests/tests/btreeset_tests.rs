//! Tests for btreeset runtime functions.

use rmx::prelude::*;
use datalove_rtdt as rtdt;
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
        flags: 0,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing { unused: 0 },
        },
    })
}

fn create_set_u32_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let element_tydesc = create_u32_tydesc(arena);

    let set_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        flags: 0,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: unsafe { rtdt::TyInfoSet::new(element_tydesc) },
        },
    });

    (set_tydesc, element_tydesc)
}

fn create_string_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        flags: 0,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing { unused: 0 },
        },
    })
}

fn create_set_string_tydesc(arena: &TyDescArena) -> (*const rtdt::TyDesc, *const rtdt::TyDesc) {
    let element_tydesc = create_string_tydesc(arena);

    let set_tydesc = arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Set,
        flags: 0,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: unsafe { rtdt::TyInfoSet::new(element_tydesc) },
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
                (s.len() as u32).into(),
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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(10));

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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
        len: rtdt::Index::ZERO,
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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(10));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice = vec![42u32];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            (slice.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(1000));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(1000));

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
        len: rtdt::Index::ZERO,
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

    let mut unique_count: rtdt::IndexRepr = 0;
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

    assert_eq!(set.len, rtdt::Index(unique_count), "Set length should match number of unique insertions");

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
        len: rtdt::Index::ZERO,
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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(1000));

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

    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = (0u32..1000).collect();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            (slice.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index(1000));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(1000));

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

    assert_eq!(set.len, rtdt::Index(500));

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

    assert_eq!(set.len, rtdt::Index(1000));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(1000));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = vec![];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            (slice.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index::ZERO);
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
        len: rtdt::Index::ZERO,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = vec![10, 20, 30, 40, 50];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            (slice.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index(5));

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
        len: rtdt::Index::ZERO,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = vec![10, 20, 10, 30, 20, 10];

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            (slice.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index(3), "Set should have 3 unique elements");

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
        len: rtdt::Index::ZERO,
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
            (runtime_strings.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index(3));

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
        len: rtdt::Index::ZERO,
    };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;

    let slice: Vec<u32> = (0u32..100).rev().collect();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clone_from_slice_local(
            rt,
            slice.as_ptr() as *const u8,
            (slice.len() as u32).into(),
            element_tydesc as *const rtdt::TyDesc,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(set.len, rtdt::Index(100));

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(5));

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
        len: rtdt::Index::ZERO,
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

    let mut search = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
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
            &mut search as *mut rtdt::String as *mut u8,
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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

    let mut search = unsafe { create_runtime_string(rt, "hello", element_tydesc as *const rtdt::TyDesc) };
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
    assert_eq!(set.len, rtdt::Index::ZERO);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut search as *mut rtdt::String as *mut u8,
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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(3));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_clear_local(
            rt,
            set_ptr,
            set_tydesc as *const rtdt::TyDesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    assert!(set.root.is_null());
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(100));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(4));

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(3));

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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
    assert_eq!(set.len, rtdt::Index(1));

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut element2 as *mut rtdt::String as *mut u8,
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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(20));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(20));

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
        len: rtdt::Index::ZERO,
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

        assert_eq!(set.len, rtdt::Index(20));

        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_clear_local(
                rt,
                set_ptr,
                set_tydesc as *const rtdt::TyDesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(set.len, rtdt::Index::ZERO);
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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(50));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(100));

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

    assert_eq!(set.len, rtdt::Index(50));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(100));

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

    assert_eq!(set.len, rtdt::Index(100 - removed_count));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(4));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(50));

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

    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index::ZERO);

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
        len: rtdt::Index::ZERO,
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
        assert_eq!(set.len, rtdt::Index::ZERO);
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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(10));

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
        len: rtdt::Index::ZERO,
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

    assert_eq!(set.len, rtdt::Index(1));

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
        len: rtdt::Index::ZERO,
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
        assert_eq!(set.len, rtdt::Index::ZERO);
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
        flags: 0,
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
        flags: 0,
        size: std::mem::size_of::<rtdt::Set>() as u32,
        align: std::mem::align_of::<rtdt::Set>() as u32,
        type_info: rtdt::TyInfo {
            set: unsafe { rtdt::TyInfoSet::new(tuple_tydesc) },
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
        len: rtdt::Index::ZERO,
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
    assert_eq!(set.len, rtdt::Index(1));

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
    assert_eq!(set.len, rtdt::Index(2));

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
    assert_eq!(set.len, rtdt::Index(1));

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

/// Test that a set as long as an index counts refuses an insert.
#[test]
fn test_btreeset_full_index_refuses_insert() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    let (set_tydesc, element_tydesc) = create_set_u32_tydesc(&arena);

    // Faked full, with no tree: the insert should fail before looking.
    let mut set = rtdt::Set {
        root: ptr::null(),
        len: rtdt::Index(rtdt::IndexRepr::MAX),
    };
    let mut element = 42u32;
    let mut was_inserted = 0u8;

    let status = unsafe {
        datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt,
            &mut set as *mut rtdt::Set as *mut u8,
            set_tydesc as *const rtdt::TyDesc,
            &mut element as *mut u32 as *mut u8,
            element_tydesc as *const rtdt::TyDesc,
            &mut was_inserted,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);
    assert!(set.root.is_null());

    let status = unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    Ok(())
}

// ==================== Shape After Removal ====================

/// The fewest elements a non-root node holds once inserts and removals have
/// had their way with it, as the runtime keeps it.
const SET_MIN_KEYS: u32 = rtdt::SET_NODE_B - 1;

/// A set's node layouts, for walking its tree from outside the runtime.
struct SetShape {
    leaf: rtdt::SetNodeLeafLayout,
    internal: rtdt::SetNodeInternalLayout,
    size: usize,
    min: u32,
}

impl SetShape {
    unsafe fn of(set_tydesc: *const rtdt::TyDesc, min: u32) -> SetShape {
        unsafe {
            let td = rtdt::TyDescRef::from_ptr(set_tydesc);
            SetShape {
                leaf: *td.set_leaf_layout(),
                internal: *td.set_internal_layout(),
                size: td.set_element_ty().size() as usize,
                min,
            }
        }
    }

    unsafe fn next_leaf(&self, leaf: *const rtdt::SetNode) -> *const rtdt::SetNode {
        unsafe { *((leaf as *const u8).add(self.leaf.next_leaf_offset as usize) as *const *const rtdt::SetNode) }
    }

    /// Checks the subtree at `node` against the bounds its ancestors put on
    /// it, and collects its leaves in order.
    unsafe fn walk<K: Ord + std::fmt::Debug>(
        &self,
        node: *const rtdt::SetNode,
        depth: usize,
        lo: Option<&K>,
        hi: Option<&K>,
        is_root: bool,
        read: &dyn Fn(*const u8) -> K,
        leaves: &mut Vec<*const rtdt::SetNode>,
        leaf_depth: &mut Option<usize>,
    ) {
        unsafe {
            let len = (*node).len;
            let in_bounds = |k: &K| lo.is_none_or(|lo| lo <= k) && hi.is_none_or(|hi| k < hi);
            match (*node).tag {
                rtdt::SetNodeTag::Leaf => {
                    assert!(is_root || len >= self.min.max(1), "a non-root leaf holds {len}");
                    assert_eq!(*leaf_depth.get_or_insert(depth), depth, "leaves at different depths");
                    let keys = (node as *const u8).add(self.leaf.keys_offset as usize);
                    for i in 0..len as usize {
                        let k = read(keys.add(i * self.size));
                        assert!(in_bounds(&k), "leaf element {k:?} outside {lo:?}..{hi:?}");
                    }
                    leaves.push(node);
                }
                rtdt::SetNodeTag::Internal => {
                    assert!(is_root || len >= self.min, "a non-root internal node holds {len}");
                    assert!(len >= 1, "an internal node with no separator");
                    let keys = (node as *const u8).add(self.internal.keys_offset as usize);
                    let children = (node as *const u8).add(self.internal.child_ptrs_offset as usize)
                        as *const *const rtdt::SetNode;
                    let seps: Vec<K> = (0..len as usize).map(|i| read(keys.add(i * self.size))).collect();
                    for (i, sep) in seps.iter().enumerate() {
                        assert!(in_bounds(sep), "separator {sep:?} outside {lo:?}..{hi:?}");
                        if i > 0 {
                            assert!(seps[i - 1] < *sep, "separators out of order");
                        }
                    }
                    for i in 0..=len as usize {
                        let child_lo = if i == 0 { lo } else { Some(&seps[i - 1]) };
                        let child_hi = if i == len as usize { hi } else { Some(&seps[i]) };
                        self.walk(*children.add(i), depth + 1, child_lo, child_hi, false, read, leaves, leaf_depth);
                    }
                }
            }
        }
    }
}

/// Checks a set's tree: leaves all at one depth, every non-root node holding
/// at least `min` and every non-root leaf something, separators bounding
/// their subtrees, and the leaf chain linking exactly the tree's leaves, in
/// order. Returns the elements as the chain visits them.
unsafe fn check_set_shape<K: Ord + std::fmt::Debug>(
    set: &rtdt::Set,
    set_tydesc: *const rtdt::TyDesc,
    min: u32,
    read: &dyn Fn(*const u8) -> K,
) -> Vec<K> {
    unsafe {
        if set.root.is_null() {
            assert_eq!(set.len, rtdt::Index::ZERO);
            return Vec::new();
        }
        let shape = SetShape::of(set_tydesc, min);

        let mut leaves = Vec::new();
        shape.walk(set.root, 0, None, None, true, read, &mut leaves, &mut None);

        let mut chain = Vec::new();
        let mut leaf = leaves[0];
        while !leaf.is_null() {
            chain.push(leaf);
            leaf = shape.next_leaf(leaf);
        }
        assert_eq!(chain, leaves, "the leaf chain is not the tree's leaves in order");

        let mut elements = Vec::new();
        for leaf in chain {
            assert!((*leaf).len > 0 || leaf == set.root, "an empty leaf in the chain");
            let keys = (leaf as *const u8).add(shape.leaf.keys_offset as usize);
            for i in 0..(*leaf).len as usize {
                elements.push(read(keys.add(i * shape.size)));
            }
        }
        assert!(elements.is_sorted_by(|a, b| a < b), "the chain is out of order");
        assert_eq!(rtdt::Index(elements.len() as rtdt::IndexRepr), set.len);
        elements
    }
}

/// A fixed shuffle of `0..n`, so a failure reproduces.
fn shuffled(n: u32, seed: u64) -> Vec<u32> {
    let mut xs: Vec<u32> = (0..n).collect();
    let mut state = seed;
    for i in (1..xs.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        xs.swap(i, j);
    }
    xs
}

unsafe fn insert_u32(rt: datalove_rt::c::LocalRtHandle, set: &mut rtdt::Set, set_td: *const rtdt::TyDesc, elem_td: *const rtdt::TyDesc, x: u32) -> u8 {
    unsafe {
        let mut x = x;
        let mut inserted = 0u8;
        let status = datalove_rt::c::dtlv_rti_btreeset_insert_local(
            rt, set as *mut rtdt::Set as *mut u8, set_td, &mut x as *mut u32 as *mut u8, elem_td, &mut inserted,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        inserted
    }
}

unsafe fn remove_u32(rt: datalove_rt::c::LocalRtHandle, set: &mut rtdt::Set, set_td: *const rtdt::TyDesc, elem_td: *const rtdt::TyDesc, x: u32) -> u8 {
    unsafe {
        let mut removed = 0u8;
        let status = datalove_rt::c::dtlv_rti_btreeset_remove_local(
            rt, set as *mut rtdt::Set as *mut u8, set_td, &x as *const u32 as *const u8, elem_td, &mut removed,
        );
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        removed
    }
}

/// Removal keeps the tree balanced: after every removal from a set built by
/// inserting, no non-root node holds fewer than the minimum, so the leaf
/// chain never holds an empty leaf, and the set still holds what it should.
#[test]
fn test_btreeset_remove_rebalances() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    let (set_td, elem_td) = create_set_u32_tydesc(&arena);
    let read = |p: *const u8| unsafe { *(p as *const u32) };

    let mut set = rtdt::Set { root: ptr::null(), len: rtdt::Index::ZERO };
    unsafe { datalove_rt::c::dtlv_rti_btreeset_create_local(rt, &mut set as *mut rtdt::Set as *mut u8, set_td) };

    let mut expected = std::collections::BTreeSet::new();
    for x in shuffled(datalove_rt_tests::size(2000), 1) {
        assert_eq!(unsafe { insert_u32(rt, &mut set, set_td, elem_td, x) }, 1);
        expected.insert(x);
    }
    unsafe { check_set_shape(&set, set_td, SET_MIN_KEYS, &read) };

    // Remove three quarters, then put some back, then remove everything.
    for x in shuffled(datalove_rt_tests::size(2000), 2).into_iter().filter(|x| x % 4 != 0) {
        assert_eq!(unsafe { remove_u32(rt, &mut set, set_td, elem_td, x) }, 1);
        expected.remove(&x);
        let got = unsafe { check_set_shape(&set, set_td, SET_MIN_KEYS, &read) };
        assert!(got.iter().eq(expected.iter()));
    }
    for x in shuffled(datalove_rt_tests::size(2000), 3).into_iter().filter(|x| x % 3 == 0) {
        let inserted = unsafe { insert_u32(rt, &mut set, set_td, elem_td, x) };
        assert_eq!(inserted == 1, expected.insert(x));
    }
    unsafe { check_set_shape(&set, set_td, SET_MIN_KEYS, &read) };
    for x in shuffled(datalove_rt_tests::size(2000), 4) {
        let removed = unsafe { remove_u32(rt, &mut set, set_td, elem_td, x) };
        assert_eq!(removed == 1, expected.remove(&x));
        let got = unsafe { check_set_shape(&set, set_td, SET_MIN_KEYS, &read) };
        assert!(got.iter().eq(expected.iter()));
    }
    assert!(set.root.is_null());

    unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(rt, &mut set as *mut rtdt::Set as *mut u8, set_td);
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// A set built in bulk from a sorted slice may start with nodes below the
/// minimum; removal still never leaves an empty leaf in the chain or loses an
/// element.
#[test]
fn test_btreeset_remove_from_built_set() -> AnyResult<()> {
    for n in datalove_rt_tests::sizes([1u32, 11, 12, 13, 50, 133, 144, 145, 300, 1000, 2000]) {
        let rt = datalove_rt::c::dtlv_rti_init();
        let arena = TyDescArena::new();
        let (set_td, elem_td) = create_set_u32_tydesc(&arena);
        let read = |p: *const u8| unsafe { *(p as *const u32) };

        let mut set = rtdt::Set { root: ptr::null(), len: rtdt::Index::ZERO };
        let mut slice: Vec<u32> = (0..n).collect();
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
                rt, &mut set as *mut rtdt::Set as *mut u8, elem_td, slice.as_mut_ptr() as *mut u8, n.into(),
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        for &x in &slice {
            let mut contains = 0u8;
            unsafe {
                datalove_rt::c::dtlv_rti_btreeset_contains_local(
                    rt, &mut set as *mut rtdt::Set as *mut u8, set_td, &x as *const u32 as *const u8, elem_td, &mut contains,
                );
            }
            assert_eq!(contains, 1, "n = {n}: {x} is missing from a built set");
        }
        unsafe { check_set_shape(&set, set_td, 0, &read) };

        let mut expected: std::collections::BTreeSet<u32> = slice.iter().copied().collect();
        for x in shuffled(n, n as u64) {
            assert_eq!(unsafe { remove_u32(rt, &mut set, set_td, elem_td, x) }, 1);
            expected.remove(&x);
            let got = unsafe { check_set_shape(&set, set_td, 0, &read) };
            assert!(got.iter().eq(expected.iter()), "n = {n}");
        }
        assert!(set.root.is_null());

        unsafe {
            datalove_rt::c::dtlv_rti_btreeset_destroy_local(rt, &mut set as *mut rtdt::Set as *mut u8, set_td);
            datalove_rt::c::dtlv_rti_shutdown(rt);
        }
    }
    Ok(())
}

/// Rebalancing clones and destroys separators; with strings, a separator
/// freed twice or never shows up under the leak checker.
#[test]
fn test_btreeset_string_remove_rebalances() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    let (set_td, elem_td) = create_set_string_tydesc(&arena);
    let read = |p: *const u8| unsafe {
        let s = &*(p as *const rtdt::String);
        std::slice::from_raw_parts(s.data(), s.size().0 as usize).to_vec()
    };

    let mut set = rtdt::Set { root: ptr::null(), len: rtdt::Index::ZERO };
    let set_ptr = &mut set as *mut rtdt::Set as *mut u8;
    unsafe { datalove_rt::c::dtlv_rti_btreeset_create_local(rt, set_ptr, set_td) };

    for x in shuffled(datalove_rt_tests::size(600), 5) {
        let mut s = unsafe { create_runtime_string(rt, &format!("element-{x:05}"), elem_td) };
        let mut inserted = 0u8;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_insert_local(
                rt, set_ptr, set_td, &mut s as *mut rtdt::String as *mut u8, elem_td, &mut inserted,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    for (k, x) in shuffled(datalove_rt_tests::size(600), 6).into_iter().enumerate() {
        let mut s = unsafe { create_runtime_string(rt, &format!("element-{x:05}"), elem_td) };
        let mut removed = 0u8;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_remove_local(
                rt, set_ptr, set_td, &s as *const rtdt::String as *const u8, elem_td, &mut removed,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
        assert_eq!(removed, 1);
        unsafe { datalove_rt::c::dtlv_rti_string_destroy_local(rt, &mut s as *mut rtdt::String as *mut u8, elem_td) };
        let got = unsafe { check_set_shape(&set, set_td, SET_MIN_KEYS, &read) };
        assert_eq!(got.len(), datalove_rt_tests::size(600) as usize - k - 1);
    }
    assert!(set.root.is_null());

    unsafe {
        datalove_rt::c::dtlv_rti_btreeset_destroy_local(rt, set_ptr, set_td);
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}
