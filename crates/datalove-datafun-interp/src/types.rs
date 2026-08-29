//! Composite type operations for the IR interpreter.
//!
//! Handles tuples, structs, enums, options, results, and Data/Error types.

use datalove_rtdt as rtdt;
use datalove_rtdt::TyDescRef;

use crate::value::{Destination, Value};
use crate::IrInterpreter;

impl IrInterpreter {
    /// Pack fields into a tuple at destination.
    pub(crate) fn execute_pack_tuple(&mut self, fields: &[Value], dest: Destination) {
        unsafe {
            let tuple_info = (*dest.tydesc).type_info.tuple;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*tuple_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
    }

    /// Pack fields into a struct at destination.
    pub(crate) fn execute_pack_struct(&mut self, fields: &[Value], dest: Destination) {
        unsafe {
            let struct_info = (*dest.tydesc).type_info.struct_;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*struct_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
    }

    /// Wrap a value in Some.
    pub(crate) fn execute_wrap_some(&self, inner: &Value, dest: Destination) {
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
    }

    /// Create a None value.
    pub(crate) fn execute_wrap_none(&self, dest: Destination) {
        unsafe {
            // Tag is at offset 0. Set to None (0).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::None as u8;
        }
    }

    /// Create an enum variant value.
    pub(crate) fn execute_enum_variant(
        &self,
        variant_index: u32,
        payload: Option<&Value>,
        dest: Destination,
    ) {
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
    }

    /// Read the discriminant (u32 tag) from an enum value.
    ///
    /// Borrows the source; does not consume.
    pub(crate) fn execute_enum_discriminant(
        &self,
        src: &Value,
        dest: Destination,
    ) {
        unsafe {
            let disc = *(src.ptr as *const u32);
            *(dest.ptr as *mut u32) = disc;
        }
    }

    /// Extract the payload from an enum value.
    ///
    /// Consumes the source. The caller marks the source as moved.
    pub(crate) fn execute_enum_payload(
        &self,
        src: &Value,
        dest: Destination,
        variant_index: u32,
    ) {
        unsafe {
            let enum_info = (*src.tydesc).type_info.enum_;
            let variant_info = &*enum_info.variants.add(variant_index as usize);
            let payload_offset = variant_info.offset as usize;
            let payload_size = (*dest.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                src.ptr.add(payload_offset),
                dest.ptr,
                payload_size,
            );
        }
    }

    /// Unwrap an Option, producing (inner_value, is_some).
    ///
    /// This is a destructive operation - the source Option is consumed.
    /// The interpreter marks the source as moved after this instruction.
    pub(crate) fn execute_unwrap_option(
        &self,
        src: &Value,
        dest: Destination,
        is_some_dest: Destination,
    ) {
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
    }

    /// Wrap a value in Ok.
    pub(crate) fn execute_wrap_ok(&self, inner: &Value, dest: Destination) {
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
    }

    /// Wrap a value in Err.
    pub(crate) fn execute_wrap_err(&self, inner: &Value, dest: Destination) {
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
    }

    /// Unwrap a Result, producing (ok_value, err_value, is_ok).
    ///
    /// - ok_dest: receives Ok payload when is_ok=true
    /// - err_dest: receives Error when is_ok=false
    ///
    /// This is a destructive operation - the source Result is consumed.
    /// The interpreter marks the source as moved after this instruction.
    pub(crate) fn execute_unwrap_result(
        &self,
        src: &Value,
        ok_dest: Destination,
        err_dest: Destination,
        is_ok_dest: Destination,
    ) {
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
    }

    /// Create Error from any value (consumes inner - linear semantics).
    ///
    /// Panics on allocation failure (OOM).
    pub(crate) fn execute_error_from(&self, inner: &Value, dest: Destination) {
        let rt_handle = self.runtime.handle();
        let inner_size = unsafe { (*inner.tydesc).size as usize };

        // Allocate heap storage for the inner value.
        let moved_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner.tydesc, 1)
        };
        assert!(!moved_ptr.is_null(), "OOM: failed to allocate Error inner storage");

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
    }

    /// Create Data from any value (consumes inner - linear semantics).
    ///
    /// Panics on allocation failure (OOM).
    pub(crate) fn execute_data_from(&self, inner: &Value, dest: Destination) {
        // The runtime owns the encoding, including which values are small
        // enough to sit in the two words rather than on the heap. This used to
        // be a second copy of it, so the interpreter kept boxing scalars after
        // the runtime stopped, and the two disagreed on what a `data` was.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_data_from_local(
                self.runtime.handle(),
                inner.ptr,
                inner.tydesc,
                dest.ptr,
            )
        };
        assert_eq!(
            status,
            datalove_rt::c::RtStatus::Ok,
            "failed to build a data value",
        );
    }

    /// Move the value back out of a data, as the destination's type.
    ///
    /// The destination's tydesc says what went in. Getting that wrong is a
    /// lowering bug, not something to check here: this is the reverse of a wrap
    /// the compiler emitted, at a site where it knows the type.
    pub(crate) fn execute_reify(&self, src: &Value, dest: Destination) {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_reify_local(
                self.runtime.handle(),
                src.ptr,
                src.tydesc,
                dest.ptr,
                dest.tydesc,
            )
        };
        assert_eq!(
            status,
            datalove_rt::c::RtStatus::Ok,
            "failed to move a value out of its erased shape",
        );
    }

    /// Move a value into the erased shape a generic callee expects.
    ///
    /// The destination's tydesc is that shape. What differs between the two is
    /// where the callee has `data`, and the runtime walks them together.
    pub(crate) fn execute_erase(&self, src: &Value, dest: Destination) {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_erase_local(
                self.runtime.handle(),
                src.ptr,
                src.tydesc,
                dest.ptr,
                dest.tydesc,
            )
        };
        assert_eq!(
            status,
            datalove_rt::c::RtStatus::Ok,
            "failed to move a value into its erased shape",
        );
    }
}
