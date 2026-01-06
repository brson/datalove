//! Constant instruction compilation.

use cranelift_codegen::ir::{types as cl_types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{Linkage, Module};

use datalove_datafun_ir::{ConstValue, IrType, ValueId};

use crate::types::PTR_TYPE;
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a constant instruction.
    pub(super) fn compile_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        value: &ConstValue,
    ) -> Result<(), AotError> {
        let cl_val = match value {
            ConstValue::Unit => {
                // Unit is zero-sized, but we need a valid address for debuglog.
                // Store the frame address like other aggregates.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    AotError::Codegen("no frame slot for Unit constant".into())
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
            ConstValue::F32(v) => {
                builder.ins().f32const(*v)
            }
            ConstValue::Int { limbs, negative } => {
                // Int is an aggregate type - write directly to frame.
                return self.compile_int_const(builder, dest, limbs, *negative);
            }
            ConstValue::String(s) => {
                // String needs runtime calls.
                return self.compile_string_const(builder, dest, s);
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile an Int (bigint) constant.
    ///
    /// Int layout: `{ data: *const u32, size_and_sign: i32, capacity: u32 }` = 16 bytes.
    pub(super) fn compile_int_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        limbs: &[u32],
        negative: bool,
    ) -> Result<(), AotError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for Int constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        if limbs.is_empty() {
            // Zero: null data, size=0, capacity=0.
            let null = builder.ins().iconst(cl_types::I64, 0);
            let zero32 = builder.ins().iconst(cl_types::I32, 0);
            builder.ins().store(MemFlags::new(), null, base, 0);     // data
            builder.ins().store(MemFlags::new(), zero32, base, 8);   // size_and_sign
            builder.ins().store(MemFlags::new(), zero32, base, 12);  // capacity
        } else {
            // Need runtime handle for memory allocation.
            let rt_handle = self.rt_handle_param.ok_or_else(|| {
                AotError::Codegen("Int constant requires runtime handle".into())
            })?;
            let runtime = self.runtime.as_ref().ok_or_else(|| {
                AotError::Codegen("Int constant requires runtime imports".into())
            })?;

            // Allocate limbs: 4 bytes each, 4-byte aligned.
            let alloc_ref = self.module.declare_func_in_func(runtime.mem_alloc_raw, builder.func);
            let size = builder.ins().iconst(cl_types::I32, 4);   // size of u32
            let align = builder.ins().iconst(cl_types::I32, 4);  // align of u32
            let count = builder.ins().iconst(cl_types::I32, limbs.len() as i64);
            let call = builder.ins().call(alloc_ref, &[rt_handle, size, align, count]);
            let limbs_ptr = builder.inst_results(call)[0];

            // Write limbs to allocated memory.
            for (i, &limb) in limbs.iter().enumerate() {
                let limb_val = builder.ins().iconst(cl_types::I32, limb as i64);
                let offset = (i * 4) as i32;
                builder.ins().store(MemFlags::new(), limb_val, limbs_ptr, offset);
            }

            // Write Int struct fields.
            builder.ins().store(MemFlags::new(), limbs_ptr, base, 0);  // data

            let size_and_sign = if negative {
                -(limbs.len() as i32)
            } else {
                limbs.len() as i32
            };
            let size_val = builder.ins().iconst(cl_types::I32, size_and_sign as i64);
            builder.ins().store(MemFlags::new(), size_val, base, 8);   // size_and_sign

            let cap_val = builder.ins().iconst(cl_types::I32, limbs.len() as i64);
            builder.ins().store(MemFlags::new(), cap_val, base, 12);   // capacity
        }

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
    ) -> Result<(), AotError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for String constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("String constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("String constant requires runtime imports".into())
        })?;

        // Get String TyDesc.
        let tydesc_id = self.tydesc_emitter.get(&IrType::String).ok_or_else(|| {
            AotError::Codegen("TyDesc not found for String".into())
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Call dtlv_rti_string_create_local(rt, dest, tydesc).
        let create_ref = self.module.declare_func_in_func(runtime.string_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, base, tydesc_ptr]);

        if !s.is_empty() {
            // Emit string bytes as static data.
            let bytes = s.as_bytes();
            let bytes_data_id = self.emit_static_bytes(bytes)?;
            let bytes_gv = self.module.declare_data_in_func(bytes_data_id, builder.func);
            let bytes_ptr = builder.ins().global_value(PTR_TYPE, bytes_gv);
            let len = builder.ins().iconst(cl_types::I32, bytes.len() as i64);

            // Call dtlv_rti_string_push_bytes_local(rt, dest, tydesc, bytes, len).
            let push_ref = self.module.declare_func_in_func(runtime.string_push_bytes, builder.func);
            builder.ins().call(push_ref, &[rt_handle, base, tydesc_ptr, bytes_ptr, len]);
        }

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Emit static bytes data and return its DataId.
    pub(super) fn emit_static_bytes(&mut self, bytes: &[u8]) -> Result<cranelift_module::DataId, AotError> {
        use cranelift_module::DataDescription;

        // Generate unique name for this data, including function name for uniqueness.
        let id = self.static_data_counter;
        self.static_data_counter += 1;
        let name = format!("__string_bytes_{}_{}", self.func.name, id);

        let data_id = self.module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| AotError::Module(format!("declare string bytes: {}", e)))?;

        let mut desc = DataDescription::new();
        desc.define(bytes.to_vec().into_boxed_slice());

        self.module
            .define_data(data_id, &desc)
            .map_err(|e| AotError::Module(format!("define string bytes: {}", e)))?;

        Ok(data_id)
    }
}
