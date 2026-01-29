//! Debug log implementation.

use datalove_rtdt as rtdt;
use crate::c::{LocalRtHandle, RtStatus};
use crate::impls::rt_local::{RtLocal, DebugOutputMode};

/// Create a TyDesc for String type.
fn create_string_tydesc() -> rtdt::TyDesc {
    rtdt::TyDesc {
        type_tag: rtdt::TyTag::String,
        size: std::mem::size_of::<rtdt::String>() as u32,
        align: std::mem::align_of::<rtdt::String>() as u32,
        type_info: rtdt::TyInfo {
            nothing: rtdt::TyInfoNothing,
        },
    }
}

/// Debug log a value (borrows, does not consume).
///
/// Pretty-prints the value and outputs according to the current debug mode.
pub unsafe fn debuglog_local(
    rt: LocalRtHandle,
    value_ref: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let rt_ref = &mut *(rt as *mut RtLocal);

        // Check mode, return early if Disabled.
        if rt_ref.debug_output_mode == DebugOutputMode::Disabled {
            return RtStatus::Ok;
        }

        // Create a TyDesc for String on the stack.
        let string_tydesc_value = create_string_tydesc();
        let string_tydesc = &string_tydesc_value as *const rtdt::TyDesc;

        // Create temp string on stack.
        let mut temp_string = rtdt::String {
            data: std::ptr::null(),
            size: rtdt::Index::ZERO,
            capacity: rtdt::Index::ZERO,
        };
        let string_ptr = &mut temp_string as *mut rtdt::String as *mut u8;

        // String is already initialized as empty, no need to call string_create_local.

        // Pretty-print the value into the temp string.
        let status = crate::impls::pretty::pretty_print_local(
            rt,
            value_ref,
            value_tydesc,
            string_ptr,
            string_tydesc,
        );

        if status != RtStatus::Ok {
            // Clean up on error.
            crate::impls::string::string_destroy_local(rt, string_ptr, string_tydesc);
            return status;
        }

        // Extract the UTF-8 bytes.
        let result = if temp_string.data.is_null() || temp_string.size == rtdt::Index::ZERO {
            String::new()
        } else {
            let bytes = std::slice::from_raw_parts(
                temp_string.data as *const u8,
                temp_string.size.as_usize(),
            );
            String::from_utf8_lossy(bytes).into_owned()
        };

        // Output based on mode.
        match rt_ref.debug_output_mode {
            DebugOutputMode::Stderr => {
                eprintln!("{}", result);
            }
            DebugOutputMode::Buffer => {
                rt_ref.debug_buffer.push_str(&result);
                rt_ref.debug_buffer.push('\n');
            }
            DebugOutputMode::Disabled => {
                // Already handled above, but included for completeness.
            }
        }

        // Destroy the temp string.
        crate::impls::string::string_destroy_local(rt, string_ptr, string_tydesc);

        RtStatus::Ok
    }
}
