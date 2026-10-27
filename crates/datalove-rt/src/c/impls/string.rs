//! String operations for the Datalove runtime.
//!
//! An `rtdt::String` holds UTF-8, and these are where that is made true:
//! bytes arriving from outside, through `string_from_bytes` and
//! `string_push_bytes_local`, are checked, and anything not UTF-8 is an error
//! rather than a string.

use datalove_rtdt as rtdt;
use crate::c::impls::rt_local::RtLocal;
use crate::c::RtStatus;

/// The bytes at `ptr`, if they are UTF-8.
///
/// # Safety
///
/// `ptr` is readable for `len` bytes, or `len` is zero.
unsafe fn utf8<'a>(ptr: *const u8, len: rtdt::IndexRepr) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(ptr, len as usize) }).ok()
}

/// Creates a new empty string.
pub unsafe fn string_create_local(
    value_out: *mut u8,
    tydesc_in: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if (*tydesc_in).type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }
        std::ptr::write(value_out as *mut rtdt::String, rtdt::String::new());
        RtStatus::Ok
    }
}

/// Destroys a string by freeing its data buffer.
pub unsafe fn string_destroy_local(
    rt: &mut RtLocal,
    value_in: *mut u8,
    tydesc_in: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if (*tydesc_in).type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }
        let string = &mut *(value_in as *mut rtdt::String);
        if !string.data().is_null() && string.capacity() > rtdt::Index::ZERO {
            rt.alloc.free(1, 1, string.capacity().0, string.data() as *mut u8);
        }
        *string = rtdt::String::new();
        RtStatus::Ok
    }
}

/// Appends bytes to a string, reallocating if necessary.
///
/// An error, and the string unchanged, if the bytes are not UTF-8.
pub unsafe fn string_push_bytes_local(
    rt: &mut RtLocal,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
    bytes_ref: *const u8,
    bytes_len: rtdt::IndexRepr,
) -> RtStatus {
    unsafe {
        if (*string_tydesc).type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }
        let Some(bytes) = utf8(bytes_ref, bytes_len) else {
            return RtStatus::Error;
        };
        if bytes.is_empty() {
            return RtStatus::Ok;
        }
        let string = &mut *(string_value_mut as *mut rtdt::String);
        let old_size = string.size().0;
        let Some(new_size) = old_size.checked_add(bytes_len) else {
            return RtStatus::Error;
        };

        // Reallocate if needed, to double the capacity or enough for the new
        // size.
        if new_size > string.capacity().0 {
            let mut new_capacity = string.capacity().0.max(8);
            while new_capacity < new_size {
                new_capacity = new_capacity.saturating_mul(2);
            }
            let new_data = rt.alloc.alloc(1, 1, new_capacity);
            if new_data.is_null() {
                return RtStatus::Error;
            }
            if old_size > 0 {
                std::ptr::copy_nonoverlapping(string.data(), new_data, old_size as usize);
            }
            if !string.data().is_null() && string.capacity().0 > 0 {
                rt.alloc.free(1, 1, string.capacity().0, string.data() as *mut u8);
            }
            // SAFETY: the old contents, copied whole, in a buffer of
            // `new_capacity`.
            string.set_raw_parts(new_data, rtdt::Index(old_size), rtdt::Index(new_capacity));
        }

        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            (string.data() as *mut u8).add(old_size as usize),
            bytes.len(),
        );
        // SAFETY: UTF-8 followed by UTF-8, within the capacity.
        string.set_size(rtdt::Index(new_size));
        RtStatus::Ok
    }
}

/// Creates a string from bytes in a single call.
///
/// An error, and nothing written, if the bytes are not UTF-8.
pub unsafe fn string_from_bytes(
    rt: &mut RtLocal,
    bytes_ptr: *const u8,
    bytes_len: rtdt::IndexRepr,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if (*result_tydesc).type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }
        let Some(text) = utf8(bytes_ptr, bytes_len) else {
            return RtStatus::Error;
        };
        let string = if text.is_empty() {
            rtdt::String::new()
        } else {
            let data = rt.alloc.alloc(1, 1, bytes_len);
            if data.is_null() {
                return RtStatus::Error;
            }
            std::ptr::copy_nonoverlapping(text.as_ptr(), data, text.len());
            // SAFETY: a copy of `text`, filling its buffer.
            rtdt::String::from_raw_parts(data, rtdt::Index(bytes_len), rtdt::Index(bytes_len))
        };
        std::ptr::write(result_out as *mut rtdt::String, string);
        RtStatus::Ok
    }
}

/// Clears a string by setting its size to zero.
///
/// The capacity is retained so the buffer can be reused.
pub unsafe fn string_clear_local(
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if (*string_tydesc).type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }
        // SAFETY: the empty string is UTF-8.
        (*(string_value_mut as *mut rtdt::String)).set_size(rtdt::Index::ZERO);
        RtStatus::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c::LocalRtHandle;
    use crate::c::impls::rt_local;

    unsafe fn create_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            flags: 0,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    #[test]
    fn test_string_create_local() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = string_create_local(
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let string = string.assume_init();
            assert!(string.data().is_null());
            assert_eq!(string.size(), rtdt::Index::ZERO);
            assert_eq!(string.capacity(), rtdt::Index::ZERO);

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_push_bytes_local() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            string_create_local(
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            let data = b"hello";
            let status = string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                data.as_ptr(),
                (data.len() as u32).into(),
            );

            assert_eq!(status, RtStatus::Ok);
            assert!(!string.data().is_null());
            assert_eq!(string.size(), rtdt::Index(5));
            assert!(string.capacity() >= rtdt::Index(5));

            let content = std::slice::from_raw_parts(string.data(), string.size().as_usize());
            assert_eq!(content, b"hello");

            string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_push_bytes_multiple() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            string_create_local(
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"hello".as_ptr(),
                5,
            );

            string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b" ".as_ptr(),
                1,
            );

            string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"world".as_ptr(),
                5,
            );

            assert_eq!(string.size(), rtdt::Index(11));
            let content = std::slice::from_raw_parts(string.data(), string.size().as_usize());
            assert_eq!(content, b"hello world");

            string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_clear_local() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            string_create_local(
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"test".as_ptr(),
                4,
            );

            assert_eq!(string.size(), rtdt::Index(4));
            let old_capacity = string.capacity();

            let status = string_clear_local(
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(string.size(), rtdt::Index::ZERO);
            assert_eq!(string.capacity(), old_capacity);

            string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_destroy_local() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            string_create_local(
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"data".as_ptr(),
                4,
            );

            let status = string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert!(string.data().is_null());
            assert_eq!(string.size(), rtdt::Index::ZERO);
            assert_eq!(string.capacity(), rtdt::Index::ZERO);

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_push_empty_bytes() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            string_create_local(
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            let status = string_push_bytes_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"".as_ptr(),
                0,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(string.size(), rtdt::Index::ZERO);

            string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_error_wrong_type() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let mut wrong_tydesc = create_string_tydesc();
            wrong_tydesc.type_tag = rtdt::TyTag::U32;

            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = string_create_local(
                string.as_mut_ptr() as *mut u8,
                &wrong_tydesc,
            );

            assert_eq!(status, RtStatus::Error);

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_from_bytes_empty() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = string_from_bytes(
                crate::c::impls::rt_local::from_handle(rt_handle),
                std::ptr::null(),
                0,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let string = string.assume_init();
            assert!(string.data().is_null());
            assert_eq!(string.size(), rtdt::Index::ZERO);
            assert_eq!(string.capacity(), rtdt::Index::ZERO);

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_from_bytes_ascii() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let data = b"hello";
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = string_from_bytes(
                crate::c::impls::rt_local::from_handle(rt_handle),
                data.as_ptr(),
                data.len() as rtdt::IndexRepr,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let mut string = string.assume_init();
            assert!(!string.data().is_null());
            assert_eq!(string.size(), rtdt::Index(5));
            assert!(string.capacity() >= rtdt::Index(5));

            let content = std::slice::from_raw_parts(string.data(), string.size().as_usize());
            assert_eq!(content, b"hello");

            string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_string_from_bytes_unicode() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;
        let tydesc = unsafe { create_string_tydesc() };

        unsafe {
            let data = "hello world".as_bytes();
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = string_from_bytes(
                crate::c::impls::rt_local::from_handle(rt_handle),
                data.as_ptr(),
                data.len() as rtdt::IndexRepr,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let mut string = string.assume_init();
            assert!(!string.data().is_null());
            assert_eq!(string.size().as_usize(), data.len());

            let content = std::slice::from_raw_parts(string.data(), string.size().as_usize());
            assert_eq!(content, data);

            string_destroy_local(
                crate::c::impls::rt_local::from_handle(rt_handle),
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }
}
