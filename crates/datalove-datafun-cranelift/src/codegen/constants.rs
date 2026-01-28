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
            ConstValue::Tuple(fields) => {
                // Tuple: write each field at computed offset.
                return self.compile_tuple_const(builder, dest, fields);
            }
            ConstValue::Struct(fields) => {
                // Struct: write each named field at computed offset.
                return self.compile_struct_const(builder, dest, fields);
            }
            ConstValue::Enum { variant, payload } => {
                // Enum: write discriminant and optional payload.
                return self.compile_enum_const(builder, dest, variant, payload.as_deref());
            }
            ConstValue::ResultOk(inner) => {
                // Result Ok: write tag=1 and inner value.
                return self.compile_result_ok_const(builder, dest, inner);
            }
            ConstValue::List(elements) => {
                // List: create list and push elements.
                return self.compile_list_const(builder, dest, elements);
            }
            ConstValue::Set(elements) => {
                // Set: create set and insert elements.
                return self.compile_set_const(builder, dest, elements);
            }
            ConstValue::Map(entries) => {
                // Map: create map and insert entries.
                return self.compile_map_const(builder, dest, entries);
            }
            ConstValue::Table { rows, .. } => {
                // Table: create table and push rows.
                return self.compile_table_const(builder, dest, rows);
            }
            // Aggregate and collection ConstValues are not yet supported for direct loading.
            ConstValue::ResultErr(_)
            | ConstValue::Data(_)
            | ConstValue::Error(_) => {
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

    /// Compile a Tuple constant.
    ///
    /// Tuple layout: fields at computed offsets based on alignment.
    fn compile_tuple_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        fields: &[ConstValue],
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Tuple constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Compute field offsets.
        let field_offsets = self.compute_tuple_field_offsets(fields);

        // Write each field at its offset.
        for (i, field_value) in fields.iter().enumerate() {
            let field_offset = field_offsets[i];
            let field_addr = builder.ins().iadd_imm(base, field_offset as i64);
            self.write_const_value_to_addr(builder, field_addr, field_value)?;
        }

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Struct constant.
    ///
    /// Struct layout: same as tuple, fields at computed offsets based on alignment.
    fn compile_struct_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        fields: &[(String, ConstValue)],
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Struct constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Compute field offsets (same algorithm as tuple).
        let field_values: Vec<_> = fields.iter().map(|(_, v)| v.clone()).collect();
        let field_offsets = self.compute_tuple_field_offsets(&field_values);

        // Write each field at its offset.
        for (i, (_, field_value)) in fields.iter().enumerate() {
            let field_offset = field_offsets[i];
            let field_addr = builder.ins().iadd_imm(base, field_offset as i64);
            self.write_const_value_to_addr(builder, field_addr, field_value)?;
        }

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile an Enum constant.
    ///
    /// Enum layout: discriminant (u32) at offset 0, payload at aligned offset.
    fn compile_enum_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        variant: &str,
        payload: Option<&ConstValue>,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Enum constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get enum type from function to find variant index.
        let ir_type = self.func.value_types.get(dest.0 as usize).ok_or_else(|| {
            CraneliftError::Codegen("no type for Enum constant".into())
        })?;
        let variants = match ir_type {
            IrType::Enum(v) => v,
            _ => return Err(CraneliftError::Codegen("expected Enum type".into())),
        };

        // Find variant index.
        let variant_index = variants.iter().position(|(name, _)| name == variant)
            .ok_or_else(|| CraneliftError::Codegen(format!("enum variant '{}' not found", variant)))?;

        // Write discriminant (u32) at offset 0.
        let discriminant = builder.ins().iconst(cl_types::I32, variant_index as i64);
        builder.ins().store(cranelift_codegen::ir::MemFlags::trusted(), discriminant, base, 0);

        // Write payload if present.
        if let Some(payload_value) = payload {
            let payload_align = self.align_of_const_value(payload_value);
            let payload_offset = datalove_rtdt::layout::align_up(4, payload_align);
            let payload_addr = builder.ins().iadd_imm(base, payload_offset as i64);
            self.write_const_value_to_addr(builder, payload_addr, payload_value)?;
        }

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Result Ok constant.
    ///
    /// Result layout: tag (u8) at offset 0, payload at aligned offset.
    /// Ok tag = 1.
    fn compile_result_ok_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &ConstValue,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Result constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write Ok tag (1) at offset 0.
        let tag = builder.ins().iconst(cl_types::I8, 1);
        builder.ins().store(cranelift_codegen::ir::MemFlags::trusted(), tag, base, 0);

        // Compute payload offset based on inner type alignment.
        let inner_align = self.align_of_const_value(inner);
        let error_align = 8u32;
        let max_align = inner_align.max(error_align);
        let payload_offset = datalove_rtdt::layout::align_up(1, max_align);
        let payload_addr = builder.ins().iadd_imm(base, payload_offset as i64);

        // Write the inner value at the payload offset.
        self.write_const_value_to_addr(builder, payload_addr, inner)?;

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a List constant.
    ///
    /// Builds list from a slice of elements using bulk-build runtime function.
    fn compile_list_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[ConstValue],
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for List constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("List constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("List constant requires runtime imports".into())
        })?;

        // Get List type from function.
        let ir_type = self.func.value_types.get(dest.0 as usize).ok_or_else(|| {
            CraneliftError::Codegen("no type for List constant".into())
        })?;
        let element_type = match ir_type {
            IrType::List(elem) => (**elem).clone(),
            _ => return Err(CraneliftError::Codegen("expected List type".into())),
        };

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&element_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for List element".into())
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().global_value(PTR_TYPE, elem_tydesc_gv);

        // Get element size/alignment for buffer allocation.
        let elem_layout = crate::types::ir_type_to_cranelift(&element_type).layout();
        let elem_size = elem_layout.size;
        let elem_align = elem_layout.align;
        let elem_stride = datalove_rtdt::layout::align_up(elem_size, elem_align);
        let elem_stride = if elem_stride == 0 { 1 } else { elem_stride };

        // Allocate stack buffer for ALL elements.
        let num_elements = elements.len() as u32;
        let buffer_size = (elem_stride * num_elements).max(8);
        let elements_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            buffer_size,
            elem_align as u8,
        ));
        let elements_addr = builder.ins().stack_addr(PTR_TYPE, elements_slot, 0);

        // Write each element at its offset in the buffer.
        for (i, element_value) in elements.iter().enumerate() {
            let offset = (i as u32) * elem_stride;
            let elem_addr = builder.ins().iadd_imm(elements_addr, offset as i64);
            self.write_const_value_to_addr(builder, elem_addr, element_value)?;
        }

        // Build list from slice.
        let num_elements_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_elements as i64);
        let list_build_ref = self.module.declare_func_in_func(runtime.list_build_from_slice, builder.func);
        builder.ins().call(list_build_ref, &[rt_handle, base, elem_tydesc_ptr, elements_addr, num_elements_val]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Set constant.
    ///
    /// Builds set from a sorted slice of elements using bulk-build runtime function.
    fn compile_set_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[ConstValue],
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Set constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Set constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Set constant requires runtime imports".into())
        })?;

        // Get Set type from function.
        let ir_type = self.func.value_types.get(dest.0 as usize).ok_or_else(|| {
            CraneliftError::Codegen("no type for Set constant".into())
        })?;
        let element_type = match ir_type {
            IrType::Set(elem) => (**elem).clone(),
            _ => return Err(CraneliftError::Codegen("expected Set type".into())),
        };

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&element_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Set element".into())
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().global_value(PTR_TYPE, elem_tydesc_gv);

        // Get element size/alignment for buffer allocation.
        let elem_layout = crate::types::ir_type_to_cranelift(&element_type).layout();
        let elem_size = elem_layout.size;
        let elem_align = elem_layout.align;
        let elem_stride = datalove_rtdt::layout::align_up(elem_size, elem_align);
        let elem_stride = if elem_stride == 0 { 1 } else { elem_stride };

        // Allocate stack buffer for ALL elements.
        let num_elements = elements.len() as u32;
        let buffer_size = elem_stride * num_elements;
        let buffer_size = buffer_size.max(8); // Minimum size for alignment
        let elements_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            buffer_size,
            elem_align as u8,
        ));
        let elements_addr = builder.ins().stack_addr(PTR_TYPE, elements_slot, 0);

        // Write each element at its offset in the buffer.
        for (i, element_value) in elements.iter().enumerate() {
            let offset = (i as u32) * elem_stride;
            let elem_addr = builder.ins().iadd_imm(elements_addr, offset as i64);
            self.write_const_value_to_addr(builder, elem_addr, element_value)?;
        }

        // Build set from sorted slice.
        let num_elements_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_elements as i64);
        let set_build_ref = self.module.declare_func_in_func(runtime.set_build_from_sorted, builder.func);
        builder.ins().call(set_build_ref, &[rt_handle, base, elem_tydesc_ptr, elements_addr, num_elements_val]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Map constant.
    ///
    /// Builds map from sorted slices of keys and values using bulk-build runtime function.
    fn compile_map_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        entries: &[(ConstValue, ConstValue)],
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Map constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Map constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Map constant requires runtime imports".into())
        })?;

        // Get Map type from function.
        let ir_type = self.func.value_types.get(dest.0 as usize).ok_or_else(|| {
            CraneliftError::Codegen("no type for Map constant".into())
        })?;
        let (key_type, value_type) = match ir_type {
            IrType::Map(k, v) => ((**k).clone(), (**v).clone()),
            _ => return Err(CraneliftError::Codegen("expected Map type".into())),
        };

        // Get key TyDesc.
        let key_tydesc_id = self.tydesc_emitter.get(&key_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Map key".into())
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        // Get value TyDesc.
        let val_tydesc_id = self.tydesc_emitter.get(&value_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Map value".into())
        })?;
        let val_tydesc_gv = self.module.declare_data_in_func(val_tydesc_id, builder.func);
        let val_tydesc_ptr = builder.ins().global_value(PTR_TYPE, val_tydesc_gv);

        // Get key size/alignment for buffer allocation.
        let key_layout = crate::types::ir_type_to_cranelift(&key_type).layout();
        let key_size = key_layout.size;
        let key_align = key_layout.align;
        let key_stride = datalove_rtdt::layout::align_up(key_size, key_align);
        let key_stride = if key_stride == 0 { 1 } else { key_stride };

        // Get value size/alignment.
        let val_layout = crate::types::ir_type_to_cranelift(&value_type).layout();
        let val_size = val_layout.size;
        let val_align = val_layout.align;
        let val_stride = datalove_rtdt::layout::align_up(val_size, val_align);
        let val_stride = if val_stride == 0 { 1 } else { val_stride };

        // Allocate stack buffers for ALL keys and ALL values.
        let num_entries = entries.len() as u32;
        let keys_buffer_size = (key_stride * num_entries).max(8);
        let vals_buffer_size = (val_stride * num_entries).max(8);

        let keys_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            keys_buffer_size,
            key_align as u8,
        ));
        let vals_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            vals_buffer_size,
            val_align as u8,
        ));
        let keys_addr = builder.ins().stack_addr(PTR_TYPE, keys_slot, 0);
        let vals_addr = builder.ins().stack_addr(PTR_TYPE, vals_slot, 0);

        // Write each key and value at their offsets in the buffers.
        for (i, (key_value, val_value)) in entries.iter().enumerate() {
            let key_offset = (i as u32) * key_stride;
            let val_offset = (i as u32) * val_stride;
            let key_addr = builder.ins().iadd_imm(keys_addr, key_offset as i64);
            let val_addr = builder.ins().iadd_imm(vals_addr, val_offset as i64);
            self.write_const_value_to_addr(builder, key_addr, key_value)?;
            self.write_const_value_to_addr(builder, val_addr, val_value)?;
        }

        // Build map from sorted slices.
        let num_entries_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_entries as i64);
        let map_build_ref = self.module.declare_func_in_func(runtime.map_build_from_sorted, builder.func);
        builder.ins().call(map_build_ref, &[rt_handle, base, key_tydesc_ptr, val_tydesc_ptr, keys_addr, vals_addr, num_entries_val]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Table constant.
    ///
    /// Builds table from rows using bulk-build runtime function.
    fn compile_table_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        rows: &[Vec<ConstValue>],
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Table constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Table constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Table constant requires runtime imports".into())
        })?;

        // Get Table type from function.
        let ir_type = self.func.value_types.get(dest.0 as usize).ok_or_else(|| {
            CraneliftError::Codegen("no type for Table constant".into())
        })?;
        let columns = match ir_type {
            IrType::Table(cols) => cols.clone(),
            _ => return Err(CraneliftError::Codegen("expected Table type".into())),
        };

        // Get Table TyDesc.
        let table_tydesc_id = self.tydesc_emitter.get(ir_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Table".into())
        })?;
        let table_tydesc_gv = self.module.declare_data_in_func(table_tydesc_id, builder.func);
        let table_tydesc_ptr = builder.ins().global_value(PTR_TYPE, table_tydesc_gv);

        // Compute row tuple layout.
        let col_types: Vec<_> = columns.iter().map(|(_, ty)| (**ty).clone()).collect();
        let mut row_offset = 0u32;
        let mut row_max_align = 1u32;
        let mut field_offsets = Vec::with_capacity(col_types.len());
        let mut col_tydescs = Vec::with_capacity(col_types.len());

        for col_type in col_types.iter() {
            let col_layout = crate::types::ir_type_to_cranelift(col_type).layout();
            let field_align = col_layout.align;
            let field_size = col_layout.size;
            row_max_align = row_max_align.max(field_align);
            row_offset = datalove_rtdt::layout::align_up(row_offset, field_align);
            field_offsets.push(row_offset);
            row_offset += field_size;

            let col_tydesc_id = self.tydesc_emitter.get(col_type).ok_or_else(|| {
                CraneliftError::Codegen("TyDesc not found for Table column".into())
            })?;
            col_tydescs.push(col_tydesc_id);
        }
        let row_size = datalove_rtdt::layout::align_up(row_offset, row_max_align);
        let row_stride = if row_size == 0 { 1 } else { row_size };

        // Build row tuple TyDesc on stack.
        // TyInfoTupleField is { offset: u32, _pad: u32, tydesc: *const TyDesc }
        let tuple_fields_size = columns.len() as u32 * 16; // Each field is 16 bytes
        let tuple_fields_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            tuple_fields_size.max(8),
            8,
        ));
        let tuple_fields_addr = builder.ins().stack_addr(PTR_TYPE, tuple_fields_slot, 0);

        // Fill in the tuple fields.
        let mem_flags = cranelift_codegen::ir::MemFlags::trusted();
        for (i, &col_tydesc_id) in col_tydescs.iter().enumerate() {
            let field_base = builder.ins().iadd_imm(tuple_fields_addr, (i * 16) as i64);
            let offset_val = builder.ins().iconst(cl_types::I32, field_offsets[i] as i64);
            builder.ins().store(mem_flags, offset_val, field_base, 0);

            let col_tydesc_gv = self.module.declare_data_in_func(col_tydesc_id, builder.func);
            let col_tydesc_ptr = builder.ins().global_value(PTR_TYPE, col_tydesc_gv);
            builder.ins().store(mem_flags, col_tydesc_ptr, field_base, 8);
        }

        // Build the tuple TyDesc on stack.
        // TyDesc = { type_tag: u8, _pad: [u8; 3], size: u32, align: u32, _pad2: u32, type_info: TyInfo }
        // TyInfo for Tuple = { num_fields: u32, _pad: u32, fields: *const TyInfoTupleField }
        // Total size: 1 + 3 + 4 + 4 + 4 + 4 + 4 + 8 = 32 bytes
        let row_tydesc_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            32,
            8,
        ));
        let row_tydesc_addr = builder.ins().stack_addr(PTR_TYPE, row_tydesc_slot, 0);

        // type_tag = Tuple (7)
        let tag_val = builder.ins().iconst(cl_types::I8, 7);
        builder.ins().store(mem_flags, tag_val, row_tydesc_addr, 0);
        // size
        let size_val = builder.ins().iconst(cl_types::I32, row_size as i64);
        builder.ins().store(mem_flags, size_val, row_tydesc_addr, 4);
        // align
        let align_val = builder.ins().iconst(cl_types::I32, row_max_align as i64);
        builder.ins().store(mem_flags, align_val, row_tydesc_addr, 8);
        // num_fields
        let num_fields_val = builder.ins().iconst(cl_types::I32, columns.len() as i64);
        builder.ins().store(mem_flags, num_fields_val, row_tydesc_addr, 16);
        // fields pointer
        builder.ins().store(mem_flags, tuple_fields_addr, row_tydesc_addr, 24);

        // Allocate stack buffer for ALL rows.
        let num_rows = rows.len() as u32;
        let buffer_size = (row_stride * num_rows).max(8);
        let rows_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            buffer_size,
            row_max_align as u8,
        ));
        let rows_addr = builder.ins().stack_addr(PTR_TYPE, rows_slot, 0);

        // Write each row at its offset in the buffer.
        for (row_idx, row_values) in rows.iter().enumerate() {
            let row_base_offset = (row_idx as u32) * row_stride;
            for (col_idx, col_value) in row_values.iter().enumerate() {
                let col_offset = row_base_offset + field_offsets[col_idx];
                let col_addr = builder.ins().iadd_imm(rows_addr, col_offset as i64);
                self.write_const_value_to_addr(builder, col_addr, col_value)?;
            }
        }

        // Build table from rows.
        let num_rows_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_rows as i64);
        let table_build_ref = self.module.declare_func_in_func(runtime.table_build_from_rows, builder.func);
        builder.ins().call(table_build_ref, &[rt_handle, base, table_tydesc_ptr, rows_addr, row_tydesc_addr, num_rows_val]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compute field offsets for a tuple of ConstValues.
    fn compute_tuple_field_offsets(&self, fields: &[ConstValue]) -> Vec<u32> {
        let mut offset = 0u32;
        let mut offsets = Vec::with_capacity(fields.len());

        for field in fields {
            let field_align = self.align_of_const_value(field);
            let field_size = self.size_of_const_value(field);

            // Align offset to field alignment.
            offset = datalove_rtdt::layout::align_up(offset, field_align);
            offsets.push(offset);

            // Advance offset by field size.
            offset += field_size;
        }

        offsets
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
            ConstValue::OptionNone => 1,
            ConstValue::OptionSome(inner) => {
                self.align_of_const_value(inner).max(1)
            }
            ConstValue::Tuple(fields) => {
                fields.iter().map(|f| self.align_of_const_value(f)).max().unwrap_or(1)
            }
            ConstValue::Struct(fields) => {
                fields.iter().map(|(_, v)| self.align_of_const_value(v)).max().unwrap_or(1)
            }
            ConstValue::Enum { payload, .. } => {
                // Enum alignment is max of discriminant (4) and payload alignment.
                let payload_align = payload.as_ref().map(|p| self.align_of_const_value(p)).unwrap_or(1);
                4u32.max(payload_align)
            }
            ConstValue::ResultOk(inner) | ConstValue::ResultErr(inner) => {
                // Result alignment is max of inner and Error (8).
                self.align_of_const_value(inner).max(8)
            }
            ConstValue::List(_) => 8, // List is pointer-aligned
            _ => 8, // Default to pointer alignment for other types.
        }
    }

    /// Get the size of a ConstValue.
    fn size_of_const_value(&self, value: &ConstValue) -> u32 {
        match value {
            ConstValue::Unit => 0,
            ConstValue::Bool(_) => 1,
            ConstValue::U8(_) | ConstValue::I8(_) => 1,
            ConstValue::U16(_) | ConstValue::I16(_) => 2,
            ConstValue::U32(_) | ConstValue::I32(_) | ConstValue::F32(_) => 4,
            ConstValue::U64(_) | ConstValue::I64(_) | ConstValue::F64(_) => 8,
            ConstValue::Usize(_) | ConstValue::Isize(_) => std::mem::size_of::<usize>() as u32,
            ConstValue::Int { .. } => std::mem::size_of::<datalove_rtdt::Int>() as u32,
            ConstValue::String(_) => 16, // ptr + size + capacity
            ConstValue::OptionNone => 1, // Just the tag
            ConstValue::OptionSome(inner) => {
                let inner_align = self.align_of_const_value(inner);
                let payload_offset = datalove_rtdt::layout::option_payload_offset(inner_align);
                let inner_size = self.size_of_const_value(inner);
                datalove_rtdt::layout::align_up(payload_offset + inner_size, inner_align.max(1))
            }
            ConstValue::Tuple(fields) => {
                let offsets = self.compute_tuple_field_offsets(fields);
                if fields.is_empty() {
                    0
                } else {
                    let last_offset = offsets[fields.len() - 1];
                    let last_size = self.size_of_const_value(&fields[fields.len() - 1]);
                    let max_align = fields.iter().map(|f| self.align_of_const_value(f)).max().unwrap_or(1);
                    datalove_rtdt::layout::align_up(last_offset + last_size, max_align)
                }
            }
            ConstValue::Struct(fields) => {
                let field_values: Vec<_> = fields.iter().map(|(_, v)| v.clone()).collect();
                let offsets = self.compute_tuple_field_offsets(&field_values);
                if fields.is_empty() {
                    0
                } else {
                    let last_offset = offsets[fields.len() - 1];
                    let last_size = self.size_of_const_value(&fields[fields.len() - 1].1);
                    let max_align = fields.iter().map(|(_, v)| self.align_of_const_value(v)).max().unwrap_or(1);
                    datalove_rtdt::layout::align_up(last_offset + last_size, max_align)
                }
            }
            ConstValue::Enum { payload, .. } => {
                // Enum size: discriminant (4) + aligned payload.
                let payload_size = payload.as_ref().map(|p| self.size_of_const_value(p)).unwrap_or(0);
                let payload_align = payload.as_ref().map(|p| self.align_of_const_value(p)).unwrap_or(1);
                let payload_offset = datalove_rtdt::layout::align_up(4, payload_align);
                let max_align = 4u32.max(payload_align);
                datalove_rtdt::layout::align_up(payload_offset + payload_size, max_align)
            }
            ConstValue::ResultOk(inner) | ConstValue::ResultErr(inner) => {
                // Result size: tag (1) + padding + max(inner, Error).
                let inner_size = self.size_of_const_value(inner);
                let error_size = 16u32; // Error is two pointers
                let max_payload = inner_size.max(error_size);
                let inner_align = self.align_of_const_value(inner);
                let error_align = 8u32;
                let max_align = inner_align.max(error_align);
                let payload_offset = datalove_rtdt::layout::align_up(1, max_align);
                datalove_rtdt::layout::align_up(payload_offset + max_payload, max_align)
            }
            ConstValue::List(_) => 24, // ptr + size + capacity (with padding)
            _ => 8, // Default for other types
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
            ConstValue::Int { limbs, negative } => {
                // Int needs runtime call - allocate inline.
                let rt_handle = self.rt_handle_param.ok_or_else(|| {
                    CraneliftError::Codegen("Int constant requires runtime handle".into())
                })?;
                let int_from_limbs_func = self.runtime.as_ref().ok_or_else(|| {
                    CraneliftError::Codegen("Int constant requires runtime imports".into())
                })?.int_from_limbs;

                let tydesc_id = self.tydesc_emitter.get(&IrType::Int).ok_or_else(|| {
                    CraneliftError::Codegen("TyDesc not found for Int".into())
                })?;
                let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

                let limbs_ptr = if limbs.is_empty() {
                    builder.ins().iconst(PTR_TYPE, 0)
                } else {
                    let limbs_bytes: Vec<u8> = limbs.iter()
                        .flat_map(|&limb| limb.to_le_bytes())
                        .collect();
                    let limbs_data_id = self.emit_static_bytes_aligned(&limbs_bytes, 4)?;
                    let limbs_gv = self.module.declare_data_in_func(limbs_data_id, builder.func);
                    builder.ins().global_value(PTR_TYPE, limbs_gv)
                };

                let limb_count = builder.ins().iconst(cl_types::I32, limbs.len() as i64);
                let negative_val = builder.ins().iconst(cl_types::I8, *negative as i64);

                let func_ref = self.module.declare_func_in_func(int_from_limbs_func, builder.func);
                builder.ins().call(func_ref, &[rt_handle, limbs_ptr, limb_count, negative_val, addr, tydesc_ptr]);
            }
            ConstValue::String(s) => {
                // String needs runtime call.
                let rt_handle = self.rt_handle_param.ok_or_else(|| {
                    CraneliftError::Codegen("String constant requires runtime handle".into())
                })?;
                let runtime = self.runtime.ok_or_else(|| {
                    CraneliftError::Codegen("String constant requires runtime imports".into())
                })?;

                let tydesc_id = self.tydesc_emitter.get(&IrType::String).ok_or_else(|| {
                    CraneliftError::Codegen("TyDesc not found for String".into())
                })?;
                let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

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

                let from_bytes_ref = self.module.declare_func_in_func(runtime.string_from_bytes, builder.func);
                builder.ins().call(from_bytes_ref, &[rt_handle, bytes_ptr, len, addr, tydesc_ptr]);
            }
            ConstValue::OptionNone => {
                // Write None tag (1) at offset 0.
                let tag = builder.ins().iconst(cl_types::I8, 1);
                builder.ins().store(mem_flags, tag, addr, 0);
            }
            ConstValue::OptionSome(inner) => {
                // Write Some tag (2) and payload.
                let tag = builder.ins().iconst(cl_types::I8, 2);
                builder.ins().store(mem_flags, tag, addr, 0);

                let inner_align = self.align_of_const_value(inner);
                let payload_offset = datalove_rtdt::layout::option_payload_offset(inner_align);
                let payload_addr = builder.ins().iadd_imm(addr, payload_offset as i64);
                self.write_const_value_to_addr(builder, payload_addr, inner)?;
            }
            ConstValue::Tuple(fields) => {
                let field_offsets = self.compute_tuple_field_offsets(fields);
                for (i, field_value) in fields.iter().enumerate() {
                    let field_offset = field_offsets[i];
                    let field_addr = builder.ins().iadd_imm(addr, field_offset as i64);
                    self.write_const_value_to_addr(builder, field_addr, field_value)?;
                }
            }
            ConstValue::Struct(fields) => {
                let field_values: Vec<_> = fields.iter().map(|(_, v)| v.clone()).collect();
                let field_offsets = self.compute_tuple_field_offsets(&field_values);
                for (i, (_, field_value)) in fields.iter().enumerate() {
                    let field_offset = field_offsets[i];
                    let field_addr = builder.ins().iadd_imm(addr, field_offset as i64);
                    self.write_const_value_to_addr(builder, field_addr, field_value)?;
                }
            }
            ConstValue::ResultOk(inner) => {
                // Write Ok tag (1) at offset 0.
                let tag = builder.ins().iconst(cl_types::I8, 1);
                builder.ins().store(mem_flags, tag, addr, 0);

                let inner_align = self.align_of_const_value(inner);
                let error_align = 8u32;
                let max_align = inner_align.max(error_align);
                let payload_offset = datalove_rtdt::layout::align_up(1, max_align);
                let payload_addr = builder.ins().iadd_imm(addr, payload_offset as i64);
                self.write_const_value_to_addr(builder, payload_addr, inner)?;
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
