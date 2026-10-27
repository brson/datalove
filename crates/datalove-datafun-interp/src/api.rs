//! What the interpreter offers its embedder beyond running code: the debug
//! buffer, printing a value, destroying one.

use super::*;

impl IrInterpreter {
    /// Get the contents of the debug buffer.
    ///
    /// Returns the accumulated debug output as a string.
    pub fn get_debug_buffer(&self) -> String {
        unsafe {
            let mut ptr: *const u8 = std::ptr::null();
            let mut len: usize = 0;
            let status = datalove_rt::c::dtlv_rti_get_debug_buffer(
                self.runtime.handle(),
                &mut ptr,
                &mut len,
            );
            if status != datalove_rt::c::RtStatus::Ok || ptr.is_null() || len == 0 {
                return String::new();
            }
            let bytes = std::slice::from_raw_parts(ptr, len);
            String::from_utf8_lossy(bytes).to_string()
        }
    }

    /// Clear the debug buffer.
    pub fn clear_debug_buffer(&self) {
        unsafe {
            datalove_rt::c::dtlv_rti_clear_debug_buffer(self.runtime.handle());
        }
    }

    /// Pretty-print a value using the runtime's pretty printer.
    pub fn pretty_print_value(&mut self, value: &Value) -> Result<String, InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let string_tydesc = self.tydesc_table.get_or_create(&IrType::String);

        unsafe {
            // Create output string.
            let mut output_string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = datalove_rt::c::dtlv_rti_string_create_local(
                rt_handle,
                output_string.as_mut_ptr() as *mut u8,
                &*string_tydesc,
            );

            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to create output string".to_string(),
                ));
            }

            let mut output_string = output_string.assume_init();

            // Pretty-print the value.
            let status = datalove_rt::c::dtlv_rti_pretty_print_local(
                rt_handle,
                value.ptr,
                &*value.tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &*string_tydesc,
            );

            if status != RtStatus::Ok {
                datalove_rt::c::dtlv_rti_string_destroy_local(
                    rt_handle,
                    &mut output_string as *mut rtdt::String as *mut u8,
                    &*string_tydesc,
                );
                return Err(InterpError::RuntimeError(
                    "Failed to pretty-print value".to_string(),
                ));
            }

            // Extract string contents.
            let result = output_string.as_str().to_string();

            // Destroy the output string.
            datalove_rt::c::dtlv_rti_string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &*string_tydesc,
            );

            Ok(result)
        }
    }

    /// Destroy a value, freeing any associated allocations.
    pub fn destroy_value(&mut self, value: &Value) {
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.runtime.handle(),
                value.ptr,
                &*value.tydesc,
            );
        }
    }
}
