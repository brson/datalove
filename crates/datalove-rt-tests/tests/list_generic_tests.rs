//! Tests for the list operations a generic function uses.
//!
//! A generic function has no static type for the elements of the list it was
//! handed. These operations read the element type from the list's own
//! descriptor and carry elements in and out as `data`, so the caller only ever
//! names `data`. The tests below check both a scalar element, which a `data`
//! holds in its two words, and a string element, which it holds on the heap
//! and has to clone and drop properly.

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
// Descriptors
// ============================================================================

fn create_u32_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
    })
}

fn create_string_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
    })
}

fn create_data_tydesc(arena: &TyDescArena) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Data,
        size: std::mem::size_of::<rtdt::Data>() as u32,
        align: std::mem::align_of::<rtdt::Data>() as u32,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
    })
}

fn create_list_tydesc(
    arena: &TyDescArena,
    element_tydesc: *const rtdt::TyDesc,
) -> *const rtdt::TyDesc {
    arena.alloc(rtdt::TyDesc {
        type_tag: rtdt::TyTag::List,
        size: std::mem::size_of::<rtdt::List>() as u32,
        align: std::mem::align_of::<rtdt::List>() as u32,
        type_info: rtdt::TyInfo {
            list: rtdt::TyInfoList { element_tydesc },
        },
    })
}

fn create_option_tydesc(
    arena: &TyDescArena,
    inner_tydesc: *const rtdt::TyDesc,
) -> *const rtdt::TyDesc {
    let option_tydesc = arena.alloc_mut(rtdt::TyDesc {
        type_tag: rtdt::TyTag::Option,
        size: 0,
        align: 0,
        type_info: rtdt::TyInfo {
            option: rtdt::TyInfoOption { inner_tydesc },
        },
    });

    let layout = unsafe {
        rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc))
    };
    unsafe {
        (*option_tydesc).size = layout.size;
        (*option_tydesc).align = layout.align;
    }

    option_tydesc
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
            rt, string.as_mut_ptr() as *mut u8, string_tydesc,
        );
        let mut string = string.assume_init();
        if !s.is_empty() {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt, &mut string as *mut rtdt::String as *mut u8, string_tydesc,
                s.as_ptr(), (s.len() as u32).into(),
            );
        }
        string
    }
}

/// Read the payload of a `?data` the runtime wrote.
///
/// Returns None when the option is None.
unsafe fn option_data_payload(
    option_ptr: *const u8,
    option_tydesc: *const rtdt::TyDesc,
) -> Option<rtdt::Data> {
    unsafe {
        let tag = *(option_ptr as *const u8);
        if tag == rtdt::OptionTag::None as u8 {
            return None;
        }
        let layout = rtdt::layout::compute_option_layout(
            rtdt::TyDescRef::from_ptr(option_tydesc),
        );
        let payload = option_ptr.add(layout.payload_offset as usize);
        Some(ptr::read(payload as *const rtdt::Data))
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
            rt, list_ptr, list_tydesc, &mut len as *mut rtdt::Index as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(len.0, 0);

    for i in 0..3u32 {
        let mut value = i;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt, list_ptr, list_tydesc,
                &mut value as *mut u32 as *mut u8, element_tydesc,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_len_local(
            rt, list_ptr, list_tydesc, &mut len as *mut rtdt::Index as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert_eq!(len.0, 3);

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// get as data
// ============================================================================

#[test]
fn test_get_as_data_packs_a_scalar_element() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let mut value = 42u32;
    unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt, list_ptr, list_tydesc,
            &mut value as *mut u32 as *mut u8, element_tydesc,
        )
    };

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_as_data_local(
            rt, list_ptr, list_tydesc, 0,
            option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let data = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
        .expect("in-bounds get should be some");
    assert_eq!(data.as_u32(), Some(42));

    // The list still owns its element: reading it did not take it away.
    let mut len: rtdt::Index = rtdt::Index::ZERO;
    unsafe {
        datalove_rt::c::dtlv_rti_list_len_local(
            rt, list_ptr, list_tydesc, &mut len as *mut rtdt::Index as *mut u8,
        )
    };
    assert_eq!(len.0, 1);

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_get_as_data_clones_a_heap_element() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_string_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let text = "a string element";
    let mut s = unsafe { create_runtime_string(rt, text, element_tydesc) };

    unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt, list_ptr, list_tydesc,
            &mut s as *mut rtdt::String as *mut u8, element_tydesc,
        )
    };

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_as_data_local(
            rt, list_ptr, list_tydesc, 0,
            option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut data = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
        .expect("in-bounds get should be some");

    // The clone is a separate string, so destroying the list leaves it valid.
    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };

    let cloned = unsafe { &*(data.value_ptr() as *const rtdt::String) };
    let bytes = unsafe {
        std::slice::from_raw_parts(cloned.data, cloned.size.0 as usize)
    };
    assert_eq!(std::str::from_utf8(bytes)?, text);

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt, &mut data as *mut rtdt::Data as *mut u8, data_tydesc,
        )
    };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_get_as_data_out_of_bounds_is_none() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_as_data_local(
            rt, list_ptr, list_tydesc, 7,
            option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert!(unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }.is_none());

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// push and pop through data
// ============================================================================

#[test]
fn test_push_data_then_pop_as_data_round_trips() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    // Wrap a u32 the way a generic call site would, then push it.
    let mut value = 99u32;
    let mut data = std::mem::MaybeUninit::<rtdt::Data>::uninit();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_data_from_local(
            rt, &mut value as *mut u32 as *const u8, element_tydesc,
            data.as_mut_ptr() as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    let data = unsafe { data.assume_init() };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_push_data_local(
            rt, list_ptr, list_tydesc, &data as *const rtdt::Data as *const u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // The element landed in the list as a u32, not as a data.
    let mut len: rtdt::Index = rtdt::Index::ZERO;
    unsafe {
        datalove_rt::c::dtlv_rti_list_len_local(
            rt, list_ptr, list_tydesc, &mut len as *mut rtdt::Index as *mut u8,
        )
    };
    assert_eq!(len.0, 1);

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_as_data_local(
            rt, list_ptr, list_tydesc,
            option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let popped = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
        .expect("non-empty pop should be some");
    assert_eq!(popped.as_u32(), Some(99));

    unsafe {
        datalove_rt::c::dtlv_rti_list_len_local(
            rt, list_ptr, list_tydesc, &mut len as *mut rtdt::Index as *mut u8,
        )
    };
    assert_eq!(len.0, 0);

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_push_data_moves_a_heap_value_in() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_string_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let text = "moved in through a data";
    let s = unsafe { create_runtime_string(rt, text, element_tydesc) };

    let mut data = std::mem::MaybeUninit::<rtdt::Data>::uninit();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_data_from_local(
            rt, &s as *const rtdt::String as *const u8, element_tydesc,
            data.as_mut_ptr() as *mut u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    let data = unsafe { data.assume_init() };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_push_data_local(
            rt, list_ptr, list_tydesc, &data as *const rtdt::Data as *const u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // Read it back out to confirm it is a string in the list, not a data.
    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_get_as_data_local(
            rt, list_ptr, list_tydesc, 0,
            option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut got = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
        .expect("in-bounds get should be some");
    let seen = unsafe { &*(got.value_ptr() as *const rtdt::String) };
    let bytes = unsafe { std::slice::from_raw_parts(seen.data, seen.size.0 as usize) };
    assert_eq!(std::str::from_utf8(bytes)?, text);

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt, &mut got as *mut rtdt::Data as *mut u8, data_tydesc,
        )
    };
    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_pop_as_data_on_empty_is_none() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_pop_as_data_local(
            rt, list_ptr, list_tydesc,
            option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);
    assert!(unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }.is_none());

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// set, insert and remove through data
// ============================================================================

#[test]
fn test_set_data_replaces_and_destroys_the_old_element() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_string_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let mut original = unsafe { create_runtime_string(rt, "replaced", element_tydesc) };
    unsafe {
        datalove_rt::c::dtlv_rti_list_push_local(
            rt, list_ptr, list_tydesc,
            &mut original as *mut rtdt::String as *mut u8, element_tydesc,
        )
    };

    // Wrap the replacement the way a generic call site would.
    let replacement = unsafe { create_runtime_string(rt, "written", element_tydesc) };
    let mut data = std::mem::MaybeUninit::<rtdt::Data>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_data_from_local(
            rt, &replacement as *const rtdt::String as *const u8, element_tydesc,
            data.as_mut_ptr() as *mut u8,
        )
    };
    let data = unsafe { data.assume_init() };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_set_data_local(
            rt, list_ptr, list_tydesc, 0, &data as *const rtdt::Data as *const u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    // The old element is gone rather than leaked, and the new one is in place.
    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    unsafe {
        datalove_rt::c::dtlv_rti_list_get_as_data_local(
            rt, list_ptr, list_tydesc, 0, option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    let mut got = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
        .expect("in-bounds get should be some");
    let seen = unsafe { &*(got.value_ptr() as *const rtdt::String) };
    let bytes = unsafe { std::slice::from_raw_parts(seen.data, seen.size.0 as usize) };
    assert_eq!(std::str::from_utf8(bytes)?, "written");

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt, &mut got as *mut rtdt::Data as *mut u8, data_tydesc,
        )
    };
    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_insert_data_shifts_the_rest_along() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    for i in [10u32, 30] {
        let mut value = i;
        unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt, list_ptr, list_tydesc,
                &mut value as *mut u32 as *mut u8, element_tydesc,
            )
        };
    }

    let mut value = 20u32;
    let mut data = std::mem::MaybeUninit::<rtdt::Data>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_data_from_local(
            rt, &mut value as *mut u32 as *const u8, element_tydesc,
            data.as_mut_ptr() as *mut u8,
        )
    };
    let data = unsafe { data.assume_init() };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_insert_data_local(
            rt, list_ptr, list_tydesc, 1, &data as *const rtdt::Data as *const u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let mut seen = Vec::new();
    for i in 0..3 {
        unsafe {
            datalove_rt::c::dtlv_rti_list_get_as_data_local(
                rt, list_ptr, list_tydesc, i, option_buf.as_mut_ptr(), option_data_tydesc,
            )
        };
        let d = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
            .expect("in-bounds get should be some");
        seen.push(d.as_u32().expect("element is a u32"));
    }
    assert_eq!(seen, vec![10, 20, 30]);

    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_remove_as_data_takes_the_element_and_closes_the_gap() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_string_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);
    let data_tydesc = create_data_tydesc(&arena);
    let option_data_tydesc = create_option_tydesc(&arena, data_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    for text in ["first", "second"] {
        let mut s = unsafe { create_runtime_string(rt, text, element_tydesc) };
        unsafe {
            datalove_rt::c::dtlv_rti_list_push_local(
                rt, list_ptr, list_tydesc,
                &mut s as *mut rtdt::String as *mut u8, element_tydesc,
            )
        };
    }

    let mut option_buf = vec![0u8; unsafe { (*option_data_tydesc).size } as usize];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_remove_as_data_local(
            rt, list_ptr, list_tydesc, 0, option_buf.as_mut_ptr(), option_data_tydesc,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Ok);

    let mut taken = unsafe { option_data_payload(option_buf.as_ptr(), option_data_tydesc) }
        .expect("in-bounds remove should be some");

    // Destroying the list must not touch what was taken out of it.
    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };

    let seen = unsafe { &*(taken.value_ptr() as *const rtdt::String) };
    let bytes = unsafe { std::slice::from_raw_parts(seen.data, seen.size.0 as usize) };
    assert_eq!(std::str::from_utf8(bytes)?, "first");

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt, &mut taken as *mut rtdt::Data as *mut u8, data_tydesc,
        )
    };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

#[test]
fn test_out_of_bounds_set_and_insert_report_error() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let arena = TyDescArena::new();
    assert!(!rt.is_null());

    let element_tydesc = create_u32_tydesc(&arena);
    let list_tydesc = create_list_tydesc(&arena, element_tydesc);

    let mut list = empty_list();
    let list_ptr = &mut list as *mut rtdt::List as *mut u8;

    let mut value = 1u32;
    let mut data = std::mem::MaybeUninit::<rtdt::Data>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_data_from_local(
            rt, &mut value as *mut u32 as *const u8, element_tydesc,
            data.as_mut_ptr() as *mut u8,
        )
    };
    let mut data = unsafe { data.assume_init() };

    // The element is not consumed when the index is refused, so the caller
    // still owns it and has to destroy it.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_set_data_local(
            rt, list_ptr, list_tydesc, 5, &data as *const rtdt::Data as *const u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_insert_data_local(
            rt, list_ptr, list_tydesc, 5, &data as *const rtdt::Data as *const u8,
        )
    };
    assert_eq!(status, datalove_rt::c::RtStatus::Error);

    unsafe {
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt, &mut data as *mut rtdt::Data as *mut u8,
            create_data_tydesc(&arena),
        )
    };
    unsafe { datalove_rt::c::dtlv_rti_list_destroy_local(rt, list_ptr, list_tydesc) };
    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}
