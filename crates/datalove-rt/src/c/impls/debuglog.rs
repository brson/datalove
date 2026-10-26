//! Debug log implementation.

use datalove_rtdt as rtdt;
use crate::c::RtStatus;
use crate::c::impls::rt_local::{RtLocal, DebugOutputMode};

/// Create a TyDesc for String type.
fn create_string_tydesc() -> rtdt::TyDesc {
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

/// Debug log a value (borrows, does not consume).
///
/// Pretty-prints the value and outputs according to the current debug mode.
pub unsafe fn debuglog_local(
    rt: &mut RtLocal,
    value_ref: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let mode = rt.debug_output_mode;

        // Check mode, return early if Disabled.
        if mode == DebugOutputMode::Disabled {
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
        let status = crate::c::impls::pretty::pretty_print_local(
            rt,
            value_ref,
            value_tydesc,
            string_ptr,
            string_tydesc,
        );

        if status != RtStatus::Ok {
            // Clean up on error.
            crate::c::impls::string::string_destroy_local(rt, string_ptr, string_tydesc);
            return status;
        }

        // Extract the UTF-8 bytes, read through `string_ptr`: reading
        // `temp_string` itself would end the pointer's write access, which
        // the destroy below needs.
        let printed = &*(string_ptr as *const rtdt::String);
        let result = if printed.data.is_null() || printed.size == rtdt::Index::ZERO {
            String::new()
        } else {
            let bytes = std::slice::from_raw_parts(
                printed.data as *const u8,
                printed.size.as_usize(),
            );
            String::from_utf8_lossy(bytes).into_owned()
        };

        // Output based on mode.
        match mode {
            DebugOutputMode::Stderr => {
                eprintln!("{}", result);
            }
            DebugOutputMode::Buffer => {
                rt.debug_buffer.push_str(&result);
                rt.debug_buffer.push('\n');
            }
            DebugOutputMode::Disabled => {
                // Already handled above, but included for completeness.
            }
        }

        // Destroy the temp string.
        crate::c::impls::string::string_destroy_local(rt, string_ptr, string_tydesc);

        RtStatus::Ok
    }
}

#[cfg(test)]
mod tests {
    use crate::c;

    /// Logging into the buffer pretty-prints through the runtime, which
    /// allocates through its handle, then appends to the buffer: two borrows
    /// of the runtime, one after the other.
    #[test]
    fn a_logged_value_lands_in_the_buffer() {
        let u64_tydesc = datalove_rtdt::TyDesc {
            type_tag: datalove_rtdt::TyTag::U64,
            flags: 0,
            size: 8,
            align: 8,
            type_info: datalove_rtdt::TyInfo { nothing: datalove_rtdt::TyInfoNothing { unused: 0 } },
        };
        let value = 18_000_000_000_000_000_000u64;
        unsafe {
            let rt = c::dtlv_rti_init();
            c::dtlv_rti_set_debug_mode(rt, c::impls::rt_local::DebugOutputMode::Buffer);
            let status = c::dtlv_rti_debuglog_local(rt, &value as *const u64 as *const u8, &u64_tydesc);
            assert_eq!(status, c::RtStatus::Ok);
            let mut ptr = std::ptr::null();
            let mut len = 0;
            c::dtlv_rti_get_debug_buffer(rt, &mut ptr, &mut len);
            let logged = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap();
            assert_eq!(logged, "18000000000000000000\n");
            c::dtlv_rti_shutdown(rt);
        }
    }
}
