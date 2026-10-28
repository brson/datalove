//! Tests for list operations as a generic function makes them: the element
//! type read off the list's own descriptor, and an element moved in that is
//! the runtime's whether or not it lands.

use rmx::prelude::*;
use datalove_rtdt as rtdt;
use std::cell::RefCell;
use std::ptr;

// ============================================================================
// Type Descriptor Arena
// ============================================================================

struct TyDescArena {
    ptrs: RefCell<Vec<*mut rtdt::TyDesc>>,
}

impl TyDescArena {
    fn new() -> Self {
        Self { ptrs: RefCell::new(Vec::new()) }
    }

    fn alloc(&self, td: rtdt::TyDesc) -> *const rtdt::TyDesc {
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
// Descriptors
// ============================================================================

fn create_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        flags: 0,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
    })
}

fn create_string_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        flags: 0,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
    })
}

fn create_list_tydesc(
    arena: &TyDescArena,
    element_tydesc: *const rtdt::TyDesc,
) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        flags: 0,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList { element_tydesc },
        },
    })
}

// ============================================================================
// Helpers
// ============================================================================

fn empty_list() -> rtdt::List {
    rtdt::List {
        data: ptr::null(),
        size: rtdt::Index::ZERO,
        capacity: rtdt::Index::ZERO,
    }
}

/// Build a runtime string from a `&str`.
unsafe fn create_runtime_string(
    rt: datalove_rt::c::LocalRtHandle,
    s: &str,
    string_tydesc: *const rtdt::TyDesc,
) -> rtdt::String {
    unsafe {
        let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
        datalove_rt::c::dtlv_rti_string_create_local(
            rt, rtdt::AnyOut::new(string.as_mut_ptr() as *mut u8), &*string_tydesc,
        );
        let mut string = string.assume_init();
        if !s.is_empty() {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt, rtdt::AnyMut::from_mut(&mut string), &*string_tydesc,
                s.as_ptr(), (s.len() as u32).into(),
            );
        }
        string
    }
}

// ============================================================================
// len
// ============================================================================

#[test]
fn test_list_len_reads_from_the_descriptor() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let mut len: rtdt::Index = rtdt::Index::ZERO;
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_len_local(
            rt, rtdt::AnyRef::new(list_ptr), &*list_tydesc, rtdt::AnyOut::new(&mut len as *mut rtdt::Index as *mut u8),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(len.0, 0);

    for i in 0..3u32 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt, rtdt::AnyMut::new(list_ptr), &*list_tydesc,
                rtdt::AnyOwned::new(&mut value as *mut u32 as *mut u8), &*element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_len_local(
            rt, rtdt::AnyRef::new(list_ptr), &*list_tydesc, rtdt::AnyOut::new(&mut len as *mut rtdt::Index as *mut u8),
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(len.0, 3);

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, rtdt::AnyOwned::new(list_ptr), &*list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// out of bounds
// ============================================================================

/// An element moved into a refused position is destroyed rather than left
/// with the caller, as every value moved into the runtime is. A string, so that
/// leaving it with the caller leaks and destroying it twice double-frees.
#[test]
fn test_out_of_bounds_set_and_insert_take_the_element() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_string_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let status = unsafe {
        let mut value = create_runtime_string(rt, "set past the end", element_tydesc);
        datalove_rt::c::dtlv_rti_list_set_local(
            rt, rtdt::AnyMut::new(list_ptr), &*list_tydesc, 5,
            rtdt::AnyOwned::new(&mut value as *mut rtdt::String as *mut u8), &*element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe {
        let mut value = create_runtime_string(rt, "inserted past the end", element_tydesc);
        datalove_rt::c::dtlv_rti_list_insert_local(
            rt, rtdt::AnyMut::new(list_ptr), &*list_tydesc, 5,
            rtdt::AnyOwned::new(&mut value as *mut rtdt::String as *mut u8), &*element_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);
    assert_eq!(list.size, rtdt::Index::ZERO);

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, rtdt::AnyOwned::new(list_ptr), &*list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}
