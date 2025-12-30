//! Tests for string runtime functions.

use rmx::prelude::*;
use datalove_rt::rtdt;
use datalove_rt::c::RtStatus;

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

/// Create a wrong (non-String) type descriptor for error testing.
fn create_wrong_tydesc() -> Box<rtdt::TyDesc> {
    Box::new(rtdt::TyDesc {
        type_tag: rtdt::TyTag::U32,
        size: 4,
        align: 4,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    })
}

/// Helper to get string contents as bytes.
unsafe fn get_string_bytes(s: &rtdt::String) -> &[u8] {
    if s.data.is_null() || s.size == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(s.data, s.size as usize) }
    }
}

// ============================================================================
// Create Tests
// ============================================================================

/// Test creating an empty string.
#[test]
fn test_string_create_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    assert!(!rt.is_null());

    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Ok);

    let string = unsafe { string.assume_init() };
    assert!(string.data.is_null());
    assert_eq!(string.size, 0);
    assert_eq!(string.capacity, 0);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test create with null value_out pointer.
#[test]
fn test_string_create_null_value_out() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            std::ptr::null_mut(),
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test create with null tydesc pointer.
#[test]
fn test_string_create_null_tydesc() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            std::ptr::null(),
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test create with wrong type tag.
#[test]
fn test_string_create_wrong_type() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let wrong_tydesc = create_wrong_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*wrong_tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// Destroy Tests
// ============================================================================

/// Test destroying an empty string.
#[test]
fn test_string_destroy_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert!(string.data.is_null());
    assert_eq!(string.size, 0);
    assert_eq!(string.capacity, 0);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test destroying a string with data.
#[test]
fn test_string_destroy_with_data() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push some data.
    let data = b"hello world";
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            data.as_ptr(),
            data.len() as u32,
        );
    }
    assert!(!string.data.is_null());

    // Destroy.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert!(string.data.is_null());
    assert_eq!(string.size, 0);
    assert_eq!(string.capacity, 0);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test destroy with null rt handle.
#[test]
fn test_string_destroy_null_rt() -> AnyResult<()> {
    let tydesc = create_string_tydesc();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            std::ptr::null_mut(),
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    Ok(())
}

/// Test destroy with null value_in pointer.
#[test]
fn test_string_destroy_null_value_in() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            std::ptr::null_mut(),
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test destroy with null tydesc pointer.
#[test]
fn test_string_destroy_null_tydesc() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            std::ptr::null(),
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test destroy with wrong type tag.
#[test]
fn test_string_destroy_wrong_type() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let wrong_tydesc = create_wrong_tydesc();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*wrong_tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// Push Bytes Tests
// ============================================================================

/// Test pushing bytes to an empty string.
#[test]
fn test_string_push_bytes_to_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    let data = b"hello";
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            data.as_ptr(),
            data.len() as u32,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert!(!string.data.is_null());
    assert_eq!(string.size, 5);
    assert!(string.capacity >= 5);
    assert_eq!(unsafe { get_string_bytes(&string) }, b"hello");

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test pushing multiple times to trigger reallocation.
#[test]
fn test_string_push_bytes_multiple() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push "hello".
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"hello".as_ptr(),
            5,
        );
    }

    // Push " ".
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b" ".as_ptr(),
            1,
        );
    }

    // Push "world".
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"world".as_ptr(),
            5,
        );
    }

    assert_eq!(string.size, 11);
    assert_eq!(unsafe { get_string_bytes(&string) }, b"hello world");

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test pushing empty bytes (zero length).
#[test]
fn test_string_push_bytes_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push zero bytes - should succeed and do nothing.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"".as_ptr(),
            0,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert!(string.data.is_null());
    assert_eq!(string.size, 0);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test pushing large data to trigger capacity growth.
#[test]
fn test_string_push_bytes_large() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push 1000 bytes.
    let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            large_data.as_ptr(),
            large_data.len() as u32,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(string.size, 1000);
    assert!(string.capacity >= 1000);
    assert_eq!(unsafe { get_string_bytes(&string) }, large_data.as_slice());

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test push with null rt handle.
#[test]
fn test_string_push_bytes_null_rt() -> AnyResult<()> {
    let tydesc = create_string_tydesc();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            std::ptr::null_mut(),
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"test".as_ptr(),
            4,
        )
    };
    assert_eq!(status, RtStatus::Error);

    Ok(())
}

/// Test push with null string pointer.
#[test]
fn test_string_push_bytes_null_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            std::ptr::null_mut(),
            &*tydesc,
            b"test".as_ptr(),
            4,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test push with null tydesc.
#[test]
fn test_string_push_bytes_null_tydesc() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            std::ptr::null(),
            b"test".as_ptr(),
            4,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test push with null bytes_ref but non-zero length.
#[test]
fn test_string_push_bytes_null_bytes_ref() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Null pointer with non-zero length should fail.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            std::ptr::null(),
            10,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test push with wrong type tag.
#[test]
fn test_string_push_bytes_wrong_type() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let wrong_tydesc = create_wrong_tydesc();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*wrong_tydesc,
            b"test".as_ptr(),
            4,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// Clear Tests
// ============================================================================

/// Test clearing an empty string.
#[test]
fn test_string_clear_empty() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_clear_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(string.size, 0);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test clearing a string with data (capacity retained).
#[test]
fn test_string_clear_with_data() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push data.
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"hello world".as_ptr(),
            11,
        );
    }
    assert_eq!(string.size, 11);
    let old_capacity = string.capacity;
    let old_data = string.data;

    // Clear.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_clear_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(string.size, 0);
    // Capacity and data pointer should be retained.
    assert_eq!(string.capacity, old_capacity);
    assert_eq!(string.data, old_data);

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test clear then push (reuse buffer).
#[test]
fn test_string_clear_then_push() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push "hello".
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"hello".as_ptr(),
            5,
        );
    }
    let old_data = string.data;

    // Clear.
    unsafe {
        datalove_rt::c::dtlv_rti_string_clear_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
    }

    // Push "hi" - should reuse the buffer.
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"hi".as_ptr(),
            2,
        );
    }
    assert_eq!(string.size, 2);
    assert_eq!(string.data, old_data); // Same buffer.
    assert_eq!(unsafe { get_string_bytes(&string) }, b"hi");

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test clear with null string pointer.
#[test]
fn test_string_clear_null_string() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_clear_local(
            rt,
            std::ptr::null_mut(),
            &*tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test clear with null tydesc.
#[test]
fn test_string_clear_null_tydesc() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_clear_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            std::ptr::null(),
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

/// Test clear with wrong type tag.
#[test]
fn test_string_clear_wrong_type() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let wrong_tydesc = create_wrong_tydesc();

    let mut string = rtdt::String {
        data: std::ptr::null(),
        size: 0,
        capacity: 0,
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_clear_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*wrong_tydesc,
        )
    };
    assert_eq!(status, RtStatus::Error);

    unsafe { datalove_rt::c::dtlv_rti_shutdown(rt) };
    Ok(())
}

// ============================================================================
// Edge Case Tests
// ============================================================================

/// Test UTF-8 data preservation.
#[test]
fn test_string_utf8_data() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push UTF-8 encoded data.
    let utf8_data = "hello \u{1F600} world"; // Contains emoji.
    let bytes = utf8_data.as_bytes();
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            bytes.as_ptr(),
            bytes.len() as u32,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(unsafe { get_string_bytes(&string) }, bytes);

    // Verify it's valid UTF-8.
    let result = std::str::from_utf8(unsafe { get_string_bytes(&string) });
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), utf8_data);

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test binary data (non-UTF8).
#[test]
fn test_string_binary_data() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push binary data including null bytes.
    let binary_data: [u8; 8] = [0x00, 0xFF, 0x01, 0xFE, 0x00, 0x00, 0xAB, 0xCD];
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            binary_data.as_ptr(),
            binary_data.len() as u32,
        )
    };
    assert_eq!(status, RtStatus::Ok);
    assert_eq!(string.size, 8);
    assert_eq!(unsafe { get_string_bytes(&string) }, &binary_data);

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}

/// Test capacity doubling behavior.
#[test]
fn test_string_capacity_growth() -> AnyResult<()> {
    let rt = datalove_rt::c::dtlv_rti_init();
    let tydesc = create_string_tydesc();

    let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
    unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt,
            string.as_mut_ptr() as *mut u8,
            &*tydesc,
        );
    }
    let mut string = unsafe { string.assume_init() };

    // Push small amount to get initial capacity.
    unsafe {
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
            b"a".as_ptr(),
            1,
        );
    }
    let initial_capacity = string.capacity;
    assert!(initial_capacity >= 8); // Minimum capacity is 8.

    // Keep pushing until we exceed capacity.
    let chunk = b"12345678"; // 8 bytes.
    for _ in 0..10 {
        unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt,
                &mut string as *mut rtdt::String as *mut u8,
                &*tydesc,
                chunk.as_ptr(),
                chunk.len() as u32,
            );
        }
    }

    // Should have grown.
    assert!(string.capacity > initial_capacity);
    assert_eq!(string.size, 1 + 80); // 1 + 10*8.

    // Clean up.
    unsafe {
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt,
            &mut string as *mut rtdt::String as *mut u8,
            &*tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt);
    }
    Ok(())
}
