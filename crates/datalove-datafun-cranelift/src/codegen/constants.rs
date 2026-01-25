//! Constant instruction compilation.

use std::sync::atomic::{AtomicU64, Ordering};

use cranelift_codegen::ir::{types as cl_types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{Linkage, Module};

use datalove_datafun_ir::{ConstValue, IrType, ValueId};

use crate::types::PTR_TYPE;
use crate::CraneliftError;

use super::FunctionCompiler;

/// Global counter for unique static data names across all function compilations.
static GLOBAL_STATIC_DATA_COUNTER: AtomicU64 = AtomicU64::new(0);

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a constant instruction.
    pub(super) fn compile_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        value: &ConstValue,
    ) -> Result<(), CraneliftError> {
        let cl_val = match value {
            ConstValue::Unit => {
                // Unit is zero-sized, but we need a valid address for debuglog.
                // Store the frame address like other aggregates.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for Unit constant".into())
                })?;
                let dest_offset = self.layout.value_offset(dest.0);
                let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
                self.values.insert(dest, base);
                return Ok(());
            }
            ConstValue::Bool(b) => {
                builder.ins().iconst(cl_types::I8, *b as i64)
            }
            ConstValue::U8(v) => {
                builder.ins().iconst(cl_types::I8, *v as i64)
            }
            ConstValue::U16(v) => {
                builder.ins().iconst(cl_types::I16, *v as i64)
            }
            ConstValue::U32(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            ConstValue::U64(v) => {
                builder.ins().iconst(cl_types::I64, *v as i64)
            }
            ConstValue::I8(v) => {
                builder.ins().iconst(cl_types::I8, *v as i64)
            }
            ConstValue::I16(v) => {
                builder.ins().iconst(cl_types::I16, *v as i64)
            }
            ConstValue::I32(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            ConstValue::I64(v) => {
                builder.ins().iconst(cl_types::I64, *v)
            }
            #[cfg(not(feature = "index-64"))]
            ConstValue::Usize(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            #[cfg(feature = "index-64")]
            ConstValue::Usize(v) => {
                builder.ins().iconst(cl_types::I64, *v as i64)
            }
            #[cfg(not(feature = "index-64"))]
            ConstValue::Isize(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            #[cfg(feature = "index-64")]
            ConstValue::Isize(v) => {
                builder.ins().iconst(cl_types::I64, *v)
            }
            ConstValue::F32(v) => {
                builder.ins().f32const(*v)
            }
            ConstValue::F64(v) => {
                builder.ins().f64const(*v)
            }
            ConstValue::Int { limbs, negative } => {
                // Int is an aggregate type - write directly to frame.
                return self.compile_int_const(builder, dest, limbs, *negative);
            }
            ConstValue::String(s) => {
                // String needs runtime calls.
                return self.compile_string_const(builder, dest, s);
            }
            ConstValue::OptionNone => {
                // Option None: write tag=1 at offset 0.
                return self.compile_option_none_const(builder, dest);
            }
            ConstValue::OptionSome(inner) => {
                // Option Some: write tag=2 and inner value.
                return self.compile_option_some_const(builder, dest, inner);
            }
            // Aggregate and collection ConstValues are not yet supported for direct loading.
            // These will be used by const evaluation to extract computed values.
            ConstValue::Tuple(_)
            | ConstValue::Struct(_)
            | ConstValue::Enum { .. }
            | ConstValue::ResultOk(_)
            | ConstValue::ResultErr(_)
            | ConstValue::Data(_)
            | ConstValue::Error(_)
            | ConstValue::List(_)
            | ConstValue::Set(_)
            | ConstValue::Map(_)
            | ConstValue::Table { .. } => {
                todo!("compile_const for aggregate/collection types: {:?}", value)
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile an Int (bigint) constant.
    ///
    /// Uses the runtime function `dtlv_rti_int_from_limbs` to construct the Int.
    pub(super) fn compile_int_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        limbs: &[u32],
        negative: bool,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Int constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Int constant requires runtime handle".into())
        })?;
        let int_from_limbs_func = self.runtime.as_ref().ok_or_else(|| {
            CraneliftError::Codegen("Int constant requires runtime imports".into())
        })?.int_from_limbs;

        // Get Int TyDesc.
        let tydesc_id = self.tydesc_emitter.get(&datalove_datafun_ir::IrType::Int).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Int".into())
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Get limbs pointer (null for zero, static data otherwise).
        let limbs_ptr = if limbs.is_empty() {
            builder.ins().iconst(PTR_TYPE, 0)
        } else {
            // Emit limbs as static data (little-endian bytes, 4-byte aligned for u32 access).
            let limbs_bytes: Vec<u8> = limbs.iter()
                .flat_map(|&limb| limb.to_le_bytes())
                .collect();
            let limbs_data_id = self.emit_static_bytes_aligned(&limbs_bytes, 4)?;
            let limbs_gv = self.module.declare_data_in_func(limbs_data_id, builder.func);
            builder.ins().global_value(PTR_TYPE, limbs_gv)
        };

        let limb_count = builder.ins().iconst(cl_types::I32, limbs.len() as i64);
        let negative_val = builder.ins().iconst(cl_types::I8, negative as i64);

        // Call dtlv_rti_int_from_limbs(rt, limbs_ptr, limb_count, negative, result_out, result_tydesc).
        let func_ref = self.module.declare_func_in_func(int_from_limbs_func, builder.func);
        builder.ins().call(func_ref, &[rt_handle, limbs_ptr, limb_count, negative_val, base, tydesc_ptr]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a String constant.
    ///
    /// String layout: `{ data: *const u8, size: u32, capacity: u32 }` = 16 bytes.
    pub(super) fn compile_string_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        s: &str,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for String constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("String constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("String constant requires runtime imports".into())
        })?;

        // Get String TyDesc.
        let tydesc_id = self.tydesc_emitter.get(&IrType::String).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for String".into())
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Get bytes pointer and length.
        let (bytes_ptr, len) = if s.is_empty() {
            let null_ptr = builder.ins().iconst(PTR_TYPE, 0);
            let zero_len = builder.ins().iconst(cl_types::I32, 0);
            (null_ptr, zero_len)
        } else {
            let bytes = s.as_bytes();
            let bytes_data_id = self.emit_static_bytes(bytes)?;
            let bytes_gv = self.module.declare_data_in_func(bytes_data_id, builder.func);
            let bytes_ptr = builder.ins().global_value(PTR_TYPE, bytes_gv);
            let len = builder.ins().iconst(cl_types::I32, bytes.len() as i64);
            (bytes_ptr, len)
        };

        // Call dtlv_rti_string_from_bytes(rt, bytes, len, dest, tydesc).
        let from_bytes_ref = self.module.declare_func_in_func(runtime.string_from_bytes, builder.func);
        builder.ins().call(from_bytes_ref, &[rt_handle, bytes_ptr, len, base, tydesc_ptr]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Emit static bytes data and return its DataId.
    pub(super) fn emit_static_bytes(&mut self, bytes: &[u8]) -> Result<cranelift_module::DataId, CraneliftError> {
        self.emit_static_bytes_aligned(bytes, 1)
    }

    /// Emit static bytes data with specified alignment and return its DataId.
    pub(super) fn emit_static_bytes_aligned(&mut self, bytes: &[u8], align: u64) -> Result<cranelift_module::DataId, CraneliftError> {
        use cranelift_module::DataDescription;

        // Generate globally unique name for this data using an atomic counter.
        // This avoids collisions when multiple modules have functions with the same name.
        let global_id = GLOBAL_STATIC_DATA_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("__static_bytes_{}", global_id);

        let data_id = self.module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CraneliftError::Module(format!("declare static bytes: {}", e)))?;

        let mut desc = DataDescription::new();
        desc.define(bytes.to_vec().into_boxed_slice());
        desc.set_align(align);

        self.module
            .define_data(data_id, &desc)
            .map_err(|e| CraneliftError::Module(format!("define static bytes: {}", e)))?;

        Ok(data_id)
    }

    /// Compile an Option None constant.
    ///
    /// Option layout: tag (u8) at offset 0, payload at aligned offset.
    /// None tag = 1.
    fn compile_option_none_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Option constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write None tag (1) at offset 0.
        let tag = builder.ins().iconst(cl_types::I8, 1);
        builder.ins().store(cranelift_codegen::ir::MemFlags::trusted(), tag, base, 0);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile an Option Some constant.
    ///
    /// Option layout: tag (u8) at offset 0, payload at aligned offset.
    /// Some tag = 2.
    fn compile_option_some_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &ConstValue,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Option constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write Some tag (2) at offset 0.
        let tag = builder.ins().iconst(cl_types::I8, 2);
        builder.ins().store(cranelift_codegen::ir::MemFlags::trusted(), tag, base, 0);

        // Compute payload offset based on inner type alignment.
        let inner_align = self.align_of_const_value(inner);
        let payload_offset = datalove_rtdt::layout::option_payload_offset(inner_align);
        let payload_addr = builder.ins().iadd_imm(base, payload_offset as i64);

        // Write the inner value at the payload offset.
        self.write_const_value_to_addr(builder, payload_addr, inner)?;

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Get the alignment of a ConstValue.
    fn align_of_const_value(&self, value: &ConstValue) -> u32 {
        match value {
            ConstValue::Unit => 1,
            ConstValue::Bool(_) => 1,
            ConstValue::U8(_) | ConstValue::I8(_) => 1,
            ConstValue::U16(_) | ConstValue::I16(_) => 2,
            ConstValue::U32(_) | ConstValue::I32(_) | ConstValue::F32(_) => 4,
            ConstValue::U64(_) | ConstValue::I64(_) | ConstValue::F64(_) => 8,
            ConstValue::Usize(_) | ConstValue::Isize(_) => std::mem::size_of::<usize>() as u32,
            // Pointer-sized types.
            ConstValue::Int { .. } | ConstValue::String(_) => 8,
            ConstValue::OptionNone | ConstValue::OptionSome(_) => 1,
            _ => 8, // Default to pointer alignment for other types.
        }
    }

    /// Write a ConstValue to a memory address.
    fn write_const_value_to_addr(
        &mut self,
        builder: &mut FunctionBuilder,
        addr: cranelift_codegen::ir::Value,
        value: &ConstValue,
    ) -> Result<(), CraneliftError> {
        let mem_flags = cranelift_codegen::ir::MemFlags::trusted();
        match value {
            ConstValue::Unit => {
                // Unit is zero-sized, nothing to write.
            }
            ConstValue::Bool(b) => {
                let val = builder.ins().iconst(cl_types::I8, *b as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::U8(v) => {
                let val = builder.ins().iconst(cl_types::I8, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::U16(v) => {
                let val = builder.ins().iconst(cl_types::I16, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::U32(v) => {
                let val = builder.ins().iconst(cl_types::I32, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::U64(v) => {
                let val = builder.ins().iconst(cl_types::I64, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::I8(v) => {
                let val = builder.ins().iconst(cl_types::I8, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::I16(v) => {
                let val = builder.ins().iconst(cl_types::I16, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::I32(v) => {
                let val = builder.ins().iconst(cl_types::I32, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::I64(v) => {
                let val = builder.ins().iconst(cl_types::I64, *v);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(not(feature = "index-64"))]
            ConstValue::Usize(v) => {
                let val = builder.ins().iconst(cl_types::I32, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(feature = "index-64")]
            ConstValue::Usize(v) => {
                let val = builder.ins().iconst(cl_types::I64, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(not(feature = "index-64"))]
            ConstValue::Isize(v) => {
                let val = builder.ins().iconst(cl_types::I32, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(feature = "index-64")]
            ConstValue::Isize(v) => {
                let val = builder.ins().iconst(cl_types::I64, *v);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::F32(v) => {
                let val = builder.ins().f32const(*v);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            ConstValue::F64(v) => {
                let val = builder.ins().f64const(*v);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "write_const_value_to_addr not implemented for: {:?}", value
                )));
            }
        }
        Ok(())
    }
}
