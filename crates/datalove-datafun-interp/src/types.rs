//! Composite type operations for the IR interpreter.
//!
//! Handles tuples, structs, enums, options, results, and Data/Error types.

use datalove_rt::rtdt::{self, TyDescRef};

use crate::error::InterpError;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

impl IrInterpreter {
    /// Pack fields into a tuple at destination.
    pub(crate) fn execute_pack_tuple(
        &mut self,
        fields: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let tuple_info = (*dest.tydesc).type_info.tuple;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*tuple_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
        Ok(())
    }

    /// Pack fields into a struct at destination.
    pub(crate) fn execute_pack_struct(
        &mut self,
        fields: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let struct_info = (*dest.tydesc).type_info.struct_;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*struct_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
        Ok(())
    }

    /// Wrap a value in Some.
    pub(crate) fn execute_wrap_some(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let option_info = (*dest.tydesc).type_info.option;
            let layout = rtdt::layout::compute_option_layout(TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Some (1).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::Some as u8;
            // Copy inner value.
            let inner_size = (*option_info.inner_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Create a None value.
    pub(crate) fn execute_wrap_none(&self, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            // Tag is at offset 0. Set to None (0).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::None as u8;
        }
        Ok(())
    }

    /// Create an enum variant value.
    pub(crate) fn execute_enum_variant(
        &self,
        variant_index: u32,
        payload: Option<&Value>,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let enum_info = (*dest.tydesc).type_info.enum_;

            // Write discriminant at offset 0.
            *(dest.ptr as *mut u32) = variant_index;

            // Copy payload if present.
            if let Some(payload_val) = payload {
                let variant_info = &*enum_info.variants.add(variant_index as usize);
                let payload_offset = variant_info.offset as usize;
                let payload_size = (*payload_val.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(
                    payload_val.ptr,
                    dest.ptr.add(payload_offset),
                    payload_size,
                );
            }
        }
        Ok(())
    }

    /// Unwrap an Option, producing (inner_value, is_some).
    pub(crate) fn execute_unwrap_option(
        &self,
        src: &Value,
        dest: Destination,
        is_some_dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let option_info = (*src.tydesc).type_info.option;
            let layout = rtdt::layout::compute_option_layout(TyDescRef::from_ptr(src.tydesc));
            // Tag is at offset 0.
            let tag = *(src.ptr as *const u8);
            let is_some = tag != rtdt::OptionTag::None as u8;
            *(is_some_dest.ptr as *mut bool) = is_some;
            if is_some {
                let inner_size = (*option_info.inner_tydesc).size as usize;
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    dest.ptr,
                    inner_size,
                );
            }
        }
        Ok(())
    }

    /// Wrap a value in Ok.
    pub(crate) fn execute_wrap_ok(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let result_info = (*dest.tydesc).type_info.result;
            let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Ok.
            *(dest.ptr as *mut u8) = rtdt::ResultTag::Ok as u8;
            // Copy inner value.
            let inner_size = (*result_info.ok_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Wrap a value in Err.
    pub(crate) fn execute_wrap_err(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Err.
            *(dest.ptr as *mut u8) = rtdt::ResultTag::Err as u8;
            // Copy Error value.
            let inner_size = (*inner.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Unwrap a Result, producing (ok_value, err_value, is_ok).
    ///
    /// - ok_dest: receives Ok payload when is_ok=true
    /// - err_dest: receives Error when is_ok=false
    pub(crate) fn execute_unwrap_result(
        &self,
        src: &Value,
        ok_dest: Destination,
        err_dest: Destination,
        is_ok_dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let result_info = (*src.tydesc).type_info.result;
            let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ptr(src.tydesc));
            // Tag is at offset 0.
            let tag = *(src.ptr as *const u8);
            let is_ok = tag == rtdt::ResultTag::Ok as u8;
            *(is_ok_dest.ptr as *mut bool) = is_ok;
            // Copy payload to appropriate destination.
            if is_ok {
                let ok_size = (*result_info.ok_tydesc).size as usize;
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    ok_dest.ptr,
                    ok_size,
                );
            } else {
                let err_size = std::mem::size_of::<rtdt::Error>();
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    err_dest.ptr,
                    err_size,
                );
            }
        }
        Ok(())
    }

    /// Create Error from any value (consumes inner - linear semantics).
    pub(crate) fn execute_error_from(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        let rt_handle = self.runtime.handle();
        let inner_size = unsafe { (*inner.tydesc).size as usize };

        // Allocate heap storage for the inner value.
        let moved_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner.tydesc, 1)
        };
        if moved_ptr.is_null() {
            return Err(InterpError::RuntimeError(
                "Failed to allocate Error inner storage".to_string(),
            ));
        }

        // Move inner value to heap storage (bitwise copy).
        unsafe {
            std::ptr::copy_nonoverlapping(inner.ptr, moved_ptr, inner_size);
        }

        // Write Error struct to destination.
        // Error has same layout as Data, so we use Data::from_pointers and transmute.
        unsafe {
            let data = rtdt::Data::from_pointers(inner.tydesc, moved_ptr);
            std::ptr::write(dest.ptr as *mut rtdt::Error, std::mem::transmute(data));
        }

        Ok(())
    }

    /// Create Data from any value (consumes inner - linear semantics).
    pub(crate) fn execute_data_from(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        let rt_handle = self.runtime.handle();
        let inner_size = unsafe { (*inner.tydesc).size as usize };

        // Allocate heap storage for the inner value.
        let moved_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner.tydesc, 1)
        };
        if moved_ptr.is_null() {
            return Err(InterpError::RuntimeError(
                "Failed to allocate Data inner storage".to_string(),
            ));
        }

        // Move inner value to heap storage (bitwise copy).
        unsafe {
            std::ptr::copy_nonoverlapping(inner.ptr, moved_ptr, inner_size);
        }

        // Write Data struct to destination.
        unsafe {
            let data = rtdt::Data::from_pointers(inner.tydesc, moved_ptr);
            std::ptr::write(dest.ptr as *mut rtdt::Data, data);
        }

        Ok(())
    }
}
