//! String operations for the Datalove runtime.

use datalove_rtdt as rtdt;
use crate::impls::rt_local::RtLocal;
use crate::c::{LocalRtHandle, RtStatus};

/// Creates a new empty string.
///
/// Initializes a String at the provided location with null data pointer,
/// zero size, and zero capacity.
pub unsafe fn string_create_local(
    _rt: LocalRtHandle,
    value_out: *mut u8,
    tydesc_in: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let ty = &*tydesc_in;
        if ty.type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }

        let string_ptr = value_out as *mut rtdt::String;
        (*string_ptr).data = std::ptr::null();
        (*string_ptr).size = 0;
        (*string_ptr).capacity = 0;

        RtStatus::Ok
    }
}

/// Destroys a string by freeing its data buffer.
pub unsafe fn string_destroy_local(
    rt: LocalRtHandle,
    value_in: *mut u8,
    tydesc_in: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let ty = &*tydesc_in;
        if ty.type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }

        let string_ptr = value_in as *mut rtdt::String;
        let string = &*string_ptr;

        // Free the data buffer if it exists.
        if !string.data.is_null() && string.capacity > 0 {
            let rt_ref = &mut *(rt as *mut RtLocal);
            rt_ref.alloc.free(1, 1, string.capacity, string.data as *mut u8);
        }

        // Clear the string fields.
        (*string_ptr).data = std::ptr::null();
        (*string_ptr).size = 0;
        (*string_ptr).capacity = 0;

        RtStatus::Ok
    }
}

/// Appends bytes to a string, reallocating if necessary.
pub unsafe fn string_push_bytes_local(
    rt: LocalRtHandle,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
    bytes_ref: *const u8,
    bytes_len: u32,
) -> RtStatus {
    if bytes_len == 0 {
        return RtStatus::Ok;
    }

    unsafe {
        let ty = &*string_tydesc;
        if ty.type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }

        let string_ptr = string_value_mut as *mut rtdt::String;
        let string = &mut *string_ptr;

        let new_size = string.size + bytes_len;

        // Reallocate if needed.
        if new_size > string.capacity {
            let rt_ref = &mut *(rt as *mut RtLocal);

            // Calculate new capacity (double, or enough for new size).
            let mut new_capacity = string.capacity.max(8);
            while new_capacity < new_size {
                new_capacity = new_capacity.saturating_mul(2);
            }

            // Allocate new buffer.
            let new_data = rt_ref.alloc.alloc(1, 1, new_capacity);
            if new_data.is_null() {
                return RtStatus::Error;
            }

            // Copy old data if it exists.
            if string.size > 0 && !string.data.is_null() {
                std::ptr::copy_nonoverlapping(
                    string.data,
                    new_data,
                    string.size as usize,
                );
            }

            // Free old buffer if it exists.
            if !string.data.is_null() && string.capacity > 0 {
                rt_ref.alloc.free(1, 1, string.capacity, string.data as *mut u8);
            }

            string.data = new_data;
            string.capacity = new_capacity;
        }

        // Append the new bytes.
        std::ptr::copy_nonoverlapping(
            bytes_ref,
            (string.data as *mut u8).add(string.size as usize),
            bytes_len as usize,
        );

        string.size = new_size;

        RtStatus::Ok
    }
}

/// Clears a string by setting its size to zero.
///
/// The capacity is retained so the buffer can be reused.
pub unsafe fn string_clear_local(
    _rt: LocalRtHandle,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let ty = &*string_tydesc;
        if ty.type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }

        let string_ptr = string_value_mut as *mut rtdt::String;
        (*string_ptr).size = 0;

        RtStatus::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impls::rt_local;

    unsafe fn create_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);

            let string = string.assume_init();
            assert!(string.data.is_null());
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);

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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            let data = b"hello";
            let status = string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                data.as_ptr(),
                data.len() as u32,
            );

            assert_eq!(status, RtStatus::Ok);
            assert!(!string.data.is_null());
            assert_eq!(string.size, 5);
            assert!(string.capacity >= 5);

            let content = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(content, b"hello");

            string_destroy_local(
                rt_handle,
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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"hello".as_ptr(),
                5,
            );

            string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b" ".as_ptr(),
                1,
            );

            string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"world".as_ptr(),
                5,
            );

            assert_eq!(string.size, 11);
            let content = std::slice::from_raw_parts(string.data, string.size as usize);
            assert_eq!(content, b"hello world");

            string_destroy_local(
                rt_handle,
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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"test".as_ptr(),
                4,
            );

            assert_eq!(string.size, 4);
            let old_capacity = string.capacity;

            let status = string_clear_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, old_capacity);

            string_destroy_local(
                rt_handle,
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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"data".as_ptr(),
                4,
            );

            let status = string_destroy_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert!(string.data.is_null());
            assert_eq!(string.size, 0);
            assert_eq!(string.capacity, 0);

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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            let mut string = string.assume_init();

            let status = string_push_bytes_local(
                rt_handle,
                &mut string as *mut rtdt::String as *mut u8,
                &tydesc,
                b"".as_ptr(),
                0,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(string.size, 0);

            string_destroy_local(
                rt_handle,
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
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &wrong_tydesc,
            );

            assert_eq!(status, RtStatus::Error);

            let rt = Box::from_raw(rt_handle as *mut RtLocal);
            rt.shutdown();
        }
    }
}
