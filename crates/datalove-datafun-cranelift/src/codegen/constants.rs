//! Constant instruction compilation.

use std::sync::atomic::{AtomicU64, Ordering};

use cranelift_codegen::ir::{types as cl_types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{Linkage, Module};

use datalove_datafun_ir::{layout as ir_layout, ConstValue, IrType, ValueId};

use crate::types::{align_shift, PTR_TYPE};
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
            ConstValue::Index(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            #[cfg(feature = "index-64")]
            ConstValue::Index(v) => {
                builder.ins().iconst(cl_types::I64, *v as i64)
            }
            #[cfg(not(feature = "index-64"))]
            ConstValue::Offset(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            #[cfg(feature = "index-64")]
            ConstValue::Offset(v) => {
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
            ConstValue::ResultErr(inner) => {
                // Result Err: write tag=2 and Error payload.
                return self.compile_result_err_const(builder, dest, inner);
            }
            ConstValue::Data(inner) => {
                // Data: box inner value.
                return self.compile_data_const(builder, dest, inner);
            }
            ConstValue::Error(inner) => {
                // Error: box inner value.
                return self.compile_error_const(builder, dest, inner);
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
        let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

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
            builder.ins().symbol_value(PTR_TYPE, limbs_gv)
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
        let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Get bytes pointer and length.
        let (bytes_ptr, len) = if s.is_empty() {
            let null_ptr = builder.ins().iconst(PTR_TYPE, 0);
            let zero_len = builder.ins().iconst(cl_types::I32, 0);
            (null_ptr, zero_len)
        } else {
            let bytes = s.as_bytes();
            let bytes_data_id = self.emit_static_bytes(bytes)?;
            let bytes_gv = self.module.declare_data_in_func(bytes_data_id, builder.func);
            let bytes_ptr = builder.ins().symbol_value(PTR_TYPE, bytes_gv);
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
        builder.ins().store(cranelift_codegen::ir::MemFlagsData::trusted(), tag, base, 0);

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

        let base = self.const_dest_addr(builder, dest, "Option")?;
        let ty = self.value_type_of(dest)?;
        let value = ConstValue::OptionSome(Box::new(inner.clone()));
        self.write_const_value_to_addr(builder, base, &ty, &value)?;
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

        let base = self.const_dest_addr(builder, dest, "Tuple")?;
        let ty = self.value_type_of(dest)?;
        let value = ConstValue::Tuple(fields.to_vec());
        self.write_const_value_to_addr(builder, base, &ty, &value)?;
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

        let base = self.const_dest_addr(builder, dest, "Struct")?;
        let ty = self.value_type_of(dest)?;
        let value = ConstValue::Struct(fields.to_vec());
        self.write_const_value_to_addr(builder, base, &ty, &value)?;
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile an Enum constant.
    ///
    /// Enum layout: discriminant (u32) at offset 0, payload at aligned offset.
    /// For Atom types: zero-sized, nothing to write.
    /// For Term types: same layout as payload.
    fn compile_enum_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        variant: &str,
        payload: Option<&ConstValue>,
    ) -> Result<(), CraneliftError> {
        let ir_type = self.func.value_types.get(dest.0 as usize).ok_or_else(|| {
            CraneliftError::Codegen("no type for Enum constant".into())
        })?;

        match ir_type {
            IrType::Atom(_) => {
                // Atom is zero-sized. Allocate address for debuglog.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for Atom constant".into())
                })?;
                let dest_offset = self.layout.value_offset(dest.0);
                let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
                self.values.insert(dest, base);
                Ok(())
            }
            IrType::Term(_, payload_ty) => {
                // A term is laid out as its payload, so what goes at the
                // address is that payload, as the type the term names.
                let payload_ty = (**payload_ty).clone();
                let base = self.const_dest_addr(builder, dest, "Term")?;
                if let Some(payload_value) = payload {
                    self.write_const_value_to_addr(builder, base, &payload_ty, payload_value)?;
                }
                self.values.insert(dest, base);
                Ok(())
            }
            _ => {
                let base = self.const_dest_addr(builder, dest, "Enum")?;
                let ty = self.value_type_of(dest)?;
                let value = ConstValue::Enum {
                    variant: variant.to_string(),
                    payload: payload.map(|p| Box::new(p.clone())),
                };
                self.write_const_value_to_addr(builder, base, &ty, &value)?;
                self.values.insert(dest, base);
                Ok(())
            }
        }
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

        let base = self.const_dest_addr(builder, dest, "Result")?;
        let ty = self.value_type_of(dest)?;
        let value = ConstValue::ResultOk(Box::new(inner.clone()));
        self.write_const_value_to_addr(builder, base, &ty, &value)?;
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Result Err constant.
    ///
    /// Result layout: tag (u8) at offset 0, Error payload at aligned offset.
    /// Err tag = 2.
    fn compile_result_err_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &ConstValue,
    ) -> Result<(), CraneliftError> {

        let base = self.const_dest_addr(builder, dest, "Result")?;
        let ty = self.value_type_of(dest)?;
        let value = ConstValue::ResultErr(Box::new(inner.clone()));
        self.write_const_value_to_addr(builder, base, &ty, &value)?;
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile an Error constant.
    ///
    /// Creates an Error by boxing an inner value.
    fn compile_error_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &ConstValue,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Error constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Error constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Error constant requires runtime imports".into())
        })?;

        // Infer inner type and get TyDesc.
        let inner_ir_type = datalove_datafun_ir::ir_type_of_const_value(inner);
        let inner_tydesc_id = self.tydesc_emitter.get(&inner_ir_type).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for inner type {:?}", inner_ir_type))
        })?;
        let inner_tydesc_gv = self.module.declare_data_in_func(inner_tydesc_id, builder.func);
        let inner_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, inner_tydesc_gv);

        // Get inner size and alignment.
        // Sized by the type the descriptor beside it names, which is the same
        // type the runtime will read it back as.
        let inner_layout = ir_layout::layout_of(&inner_ir_type);
        let inner_size = inner_layout.size.max(1);
        let inner_align = inner_layout.align.max(1);

        // Allocate temp slot for the inner value.
        let slot_data = cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            inner_size,
            align_shift(inner_align),
        );
        let temp_slot = builder.create_sized_stack_slot(slot_data);
        let inner_ptr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);

        // Write inner value to temp slot.
        self.write_const_value_to_addr(builder, inner_ptr, &inner_ir_type, inner)?;

        // Call error_from_local to box the inner value into an Error.
        let error_from_ref = self.module.declare_func_in_func(runtime.error_from, builder.func);
        builder.ins().call(error_from_ref, &[rt_handle, inner_ptr, inner_tydesc_ptr, base]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a Data constant.
    ///
    /// Creates a Data by boxing an inner value.
    fn compile_data_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &ConstValue,
    ) -> Result<(), CraneliftError> {
        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for Data constant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Data constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Data constant requires runtime imports".into())
        })?;

        // Infer inner type and get TyDesc.
        let inner_ir_type = datalove_datafun_ir::ir_type_of_const_value(inner);
        let inner_tydesc_id = self.tydesc_emitter.get(&inner_ir_type).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for inner type {:?}", inner_ir_type))
        })?;
        let inner_tydesc_gv = self.module.declare_data_in_func(inner_tydesc_id, builder.func);
        let inner_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, inner_tydesc_gv);

        // Get inner size and alignment.
        // Sized by the type the descriptor beside it names, which is the same
        // type the runtime will read it back as.
        let inner_layout = ir_layout::layout_of(&inner_ir_type);
        let inner_size = inner_layout.size.max(1);
        let inner_align = inner_layout.align.max(1);

        // Allocate temp slot for the inner value.
        let slot_data = cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            inner_size,
            align_shift(inner_align),
        );
        let temp_slot = builder.create_sized_stack_slot(slot_data);
        let inner_ptr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);

        // Write inner value to temp slot.
        self.write_const_value_to_addr(builder, inner_ptr, &inner_ir_type, inner)?;

        // Call data_from_local to box the inner value into a Data.
        let data_from_ref = self.module.declare_func_in_func(runtime.data_from, builder.func);
        builder.ins().call(data_from_ref, &[rt_handle, inner_ptr, inner_tydesc_ptr, base]);

        // Store base pointer for this value.
        self.values.insert(dest, base);
        Ok(())
    }

    /// Compile a List constant.
    ///
    /// Builds list from a slice of elements using bulk-build runtime function.
    /// The address a constant with an id is written to.
    fn const_dest_addr(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        what: &str,
    ) -> Result<cranelift_codegen::ir::Value, CraneliftError> {
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen(format!("no frame slot for {} constant", what))
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        Ok(builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32))
    }

    /// The declared type of a value, which is what a constant is written as.
    fn value_type_of(&self, dest: ValueId) -> Result<IrType, CraneliftError> {
        self.func.value_types.get(dest.0 as usize).cloned().ok_or_else(|| {
            CraneliftError::Codegen(format!("no type for value {:?}", dest))
        })
    }

    fn compile_list_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[ConstValue],
    ) -> Result<(), CraneliftError> {
        let base = self.const_dest_addr(builder, dest, "List")?;
        let element_type = match self.value_type_of(dest)? {
            IrType::List(elem) => (*elem).clone(),
            other => return Err(CraneliftError::Codegen(format!(
                "a list constant wants a list type, not {:?}", other))),
        };
        self.build_list_const_at(builder, base, &element_type, elements)?;
        self.values.insert(dest, base);
        Ok(())
    }

    /// Build a list at an address, from a declared element type.
    ///
    /// Apart from the entry point above, which knows a `ValueId`, this is
    /// reached from a constant that holds a list somewhere inside it, where
    /// there is no id and the element type comes from the type being written.
    fn build_list_const_at(
        &mut self,
        builder: &mut FunctionBuilder,
        base: cranelift_codegen::ir::Value,
        element_type: &IrType,
        elements: &[ConstValue],
    ) -> Result<(), CraneliftError> {
        let element_type = element_type.clone();
        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("List constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("List constant requires runtime imports".into())
        })?;

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&element_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for List element".into())
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, elem_tydesc_gv);

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
            align_shift(elem_align),
        ));
        let elements_addr = builder.ins().stack_addr(PTR_TYPE, elements_slot, 0);

        // Write each element at its offset in the buffer.
        for (i, element_value) in elements.iter().enumerate() {
            let offset = (i as u32) * elem_stride;
            let elem_addr = builder.ins().iadd_imm_s(elements_addr, offset as i64);
            self.write_const_value_to_addr(builder, elem_addr, &element_type, element_value)?;
        }

        // Build list from slice.
        let num_elements_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_elements as i64);
        let list_build_ref = self.module.declare_func_in_func(runtime.list_build_from_slice, builder.func);
        builder.ins().call(list_build_ref, &[rt_handle, base, elem_tydesc_ptr, elements_addr, num_elements_val]);
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
        let base = self.const_dest_addr(builder, dest, "Set")?;
        let element_type = match self.value_type_of(dest)? {
            IrType::Set(elem) => (*elem).clone(),
            other => return Err(CraneliftError::Codegen(format!(
                "a set constant wants a set type, not {:?}", other))),
        };
        self.build_set_const_at(builder, base, &element_type, elements)?;
        self.values.insert(dest, base);
        Ok(())
    }

    /// Build a set at an address, from a declared element type. See
    /// `build_list_const_at`.
    fn build_set_const_at(
        &mut self,
        builder: &mut FunctionBuilder,
        base: cranelift_codegen::ir::Value,
        element_type: &IrType,
        elements: &[ConstValue],
    ) -> Result<(), CraneliftError> {
        let element_type = element_type.clone();
        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Set constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Set constant requires runtime imports".into())
        })?;


        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&element_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Set element".into())
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, elem_tydesc_gv);

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
            align_shift(elem_align),
        ));
        let elements_addr = builder.ins().stack_addr(PTR_TYPE, elements_slot, 0);

        // Write each element at its offset in the buffer.
        for (i, element_value) in elements.iter().enumerate() {
            let offset = (i as u32) * elem_stride;
            let elem_addr = builder.ins().iadd_imm_s(elements_addr, offset as i64);
            self.write_const_value_to_addr(builder, elem_addr, &element_type, element_value)?;
        }

        // Build set from sorted slice.
        let num_elements_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_elements as i64);
        let set_build_ref = self.module.declare_func_in_func(runtime.set_build_from_sorted, builder.func);
        builder.ins().call(set_build_ref, &[rt_handle, base, elem_tydesc_ptr, elements_addr, num_elements_val]);
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
        let base = self.const_dest_addr(builder, dest, "Map")?;
        let (key_type, value_type) = match self.value_type_of(dest)? {
            IrType::Map(k, v) => ((*k).clone(), (*v).clone()),
            other => return Err(CraneliftError::Codegen(format!(
                "a map constant wants a map type, not {:?}", other))),
        };
        self.build_map_const_at(builder, base, &key_type, &value_type, entries)?;
        self.values.insert(dest, base);
        Ok(())
    }

    /// Build a map at an address, from declared key and value types. See
    /// `build_list_const_at`.
    fn build_map_const_at(
        &mut self,
        builder: &mut FunctionBuilder,
        base: cranelift_codegen::ir::Value,
        key_type: &IrType,
        value_type: &IrType,
        entries: &[(ConstValue, ConstValue)],
    ) -> Result<(), CraneliftError> {
        let key_type = key_type.clone();
        let value_type = value_type.clone();
        // Need runtime handle and imports.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Map constant requires runtime handle".into())
        })?;
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("Map constant requires runtime imports".into())
        })?;

        // Get key TyDesc.
        let key_tydesc_id = self.tydesc_emitter.get(&key_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Map key".into())
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, key_tydesc_gv);

        // Get value TyDesc.
        let val_tydesc_id = self.tydesc_emitter.get(&value_type).ok_or_else(|| {
            CraneliftError::Codegen("TyDesc not found for Map value".into())
        })?;
        let val_tydesc_gv = self.module.declare_data_in_func(val_tydesc_id, builder.func);
        let val_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, val_tydesc_gv);

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
            align_shift(key_align),
        ));
        let vals_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            vals_buffer_size,
            align_shift(val_align),
        ));
        let keys_addr = builder.ins().stack_addr(PTR_TYPE, keys_slot, 0);
        let vals_addr = builder.ins().stack_addr(PTR_TYPE, vals_slot, 0);

        // Write each key and value at their offsets in the buffers.
        for (i, (key_value, val_value)) in entries.iter().enumerate() {
            let key_offset = (i as u32) * key_stride;
            let val_offset = (i as u32) * val_stride;
            let key_addr = builder.ins().iadd_imm_s(keys_addr, key_offset as i64);
            let val_addr = builder.ins().iadd_imm_s(vals_addr, val_offset as i64);
            self.write_const_value_to_addr(builder, key_addr, &key_type, key_value)?;
            self.write_const_value_to_addr(builder, val_addr, &value_type, val_value)?;
        }

        // Build map from sorted slices.
        let num_entries_val = builder.ins().iconst(crate::index_types::INDEX_TYPE, num_entries as i64);
        let map_build_ref = self.module.declare_func_in_func(runtime.map_build_from_sorted, builder.func);
        builder.ins().call(map_build_ref, &[rt_handle, base, key_tydesc_ptr, val_tydesc_ptr, keys_addr, vals_addr, num_entries_val]);
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
        let table_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, table_tydesc_gv);

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
            align_shift(8),
        ));
        let tuple_fields_addr = builder.ins().stack_addr(PTR_TYPE, tuple_fields_slot, 0);

        // Fill in the tuple fields.
        let mem_flags = cranelift_codegen::ir::MemFlagsData::trusted();
        for (i, &col_tydesc_id) in col_tydescs.iter().enumerate() {
            let field_base = builder.ins().iadd_imm_s(tuple_fields_addr, (i * 16) as i64);
            let offset_val = builder.ins().iconst(cl_types::I32, field_offsets[i] as i64);
            builder.ins().store(mem_flags, offset_val, field_base, 0);

            let col_tydesc_gv = self.module.declare_data_in_func(col_tydesc_id, builder.func);
            let col_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, col_tydesc_gv);
            builder.ins().store(mem_flags, col_tydesc_ptr, field_base, 8);
        }

        // Build the tuple TyDesc on stack.
        // TyDesc = { type_tag: u8, _pad: [u8; 3], size: u32, align: u32, _pad2: u32, type_info: TyInfo }
        // TyInfo for Tuple = { num_fields: u32, _pad: u32, fields: *const TyInfoTupleField }
        // Total size: 1 + 3 + 4 + 4 + 4 + 4 + 4 + 8 = 32 bytes
        let row_tydesc_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            32,
            align_shift(8),
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
            align_shift(row_max_align),
        ));
        let rows_addr = builder.ins().stack_addr(PTR_TYPE, rows_slot, 0);

        // Write each row at its offset in the buffer.
        for (row_idx, row_values) in rows.iter().enumerate() {
            let row_base_offset = (row_idx as u32) * row_stride;
            for (col_idx, col_value) in row_values.iter().enumerate() {
                let col_offset = row_base_offset + field_offsets[col_idx];
                let col_addr = builder.ins().iadd_imm_s(rows_addr, col_offset as i64);
                self.write_const_value_to_addr(builder, col_addr, &col_types[col_idx], col_value)?;
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
    /// Write a ConstValue to a memory address.
    /// Write a constant into an address, as a value of `ty`.
    ///
    /// The type comes from the caller rather than from the value. A value can
    /// only say the part of a type it holds -- an empty list says nothing
    /// about its elements, a `none` says nothing about what it would have held
    /// -- and both the offsets to write at and the descriptors a collection is
    /// built against come from the whole of it. Reading them off the value is
    /// what this did, and it is why a collection could not be written at all
    /// from here: there was nothing to name its elements.
    fn write_const_value_to_addr(
        &mut self,
        builder: &mut FunctionBuilder,
        addr: cranelift_codegen::ir::Value,
        ty: &IrType,
        value: &ConstValue,
    ) -> Result<(), CraneliftError> {
        let ty = ty.clone();
        let ty = &ty;
        let mem_flags = cranelift_codegen::ir::MemFlagsData::trusted();
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
            ConstValue::Index(v) => {
                let val = builder.ins().iconst(cl_types::I32, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(feature = "index-64")]
            ConstValue::Index(v) => {
                let val = builder.ins().iconst(cl_types::I64, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(not(feature = "index-64"))]
            ConstValue::Offset(v) => {
                let val = builder.ins().iconst(cl_types::I32, *v as i64);
                builder.ins().store(mem_flags, val, addr, 0);
            }
            #[cfg(feature = "index-64")]
            ConstValue::Offset(v) => {
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
                let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

                let limbs_ptr = if limbs.is_empty() {
                    builder.ins().iconst(PTR_TYPE, 0)
                } else {
                    let limbs_bytes: Vec<u8> = limbs.iter()
                        .flat_map(|&limb| limb.to_le_bytes())
                        .collect();
                    let limbs_data_id = self.emit_static_bytes_aligned(&limbs_bytes, 4)?;
                    let limbs_gv = self.module.declare_data_in_func(limbs_data_id, builder.func);
                    builder.ins().symbol_value(PTR_TYPE, limbs_gv)
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
                let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

                let (bytes_ptr, len) = if s.is_empty() {
                    let null_ptr = builder.ins().iconst(PTR_TYPE, 0);
                    let zero_len = builder.ins().iconst(cl_types::I32, 0);
                    (null_ptr, zero_len)
                } else {
                    let bytes = s.as_bytes();
                    let bytes_data_id = self.emit_static_bytes(bytes)?;
                    let bytes_gv = self.module.declare_data_in_func(bytes_data_id, builder.func);
                    let bytes_ptr = builder.ins().symbol_value(PTR_TYPE, bytes_gv);
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
                let IrType::Option(inner_ty) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "an option constant wants an option type, not {:?}", ty)));
                };
                // Write Some tag (2) and payload.
                let tag = builder.ins().iconst(cl_types::I8, 2);
                builder.ins().store(mem_flags, tag, addr, 0);

                let payload_offset = ir_layout::option_payload_offset(inner_ty);
                let payload_addr = builder.ins().iadd_imm_s(addr, payload_offset as i64);
                let inner_ty = (**inner_ty).clone();
                self.write_const_value_to_addr(builder, payload_addr, &inner_ty, inner)?;
            }
            ConstValue::Tuple(fields) => {
                let IrType::Tuple(field_types) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a tuple constant wants a tuple type, not {:?}", ty)));
                };
                let field_types = field_types.clone();
                let field_offsets = ir_layout::aggregate_field_offsets(&field_types);
                for (i, field_value) in fields.iter().enumerate() {
                    let field_addr = builder.ins().iadd_imm_s(addr, field_offsets[i] as i64);
                    self.write_const_value_to_addr(
                        builder, field_addr, &field_types[i], field_value)?;
                }
            }
            ConstValue::Struct(fields) => {
                let IrType::Struct(field_types) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a struct constant wants a struct type, not {:?}", ty)));
                };
                // The type's fields are in the order the layout uses, which is
                // what the offsets are computed from; the value's are matched
                // to them by name.
                let field_types = field_types.clone();
                let ordered: Vec<IrType> =
                    field_types.iter().map(|(_, ty)| ty.clone()).collect();
                let field_offsets = ir_layout::aggregate_field_offsets(&ordered);
                for (name, field_value) in fields {
                    let index = field_types.iter().position(|(n, _)| n == name)
                        .ok_or_else(|| CraneliftError::Codegen(format!(
                            "struct constant names field '{}', which {:?} does not have",
                            name, ty)))?;
                    let field_addr = builder.ins().iadd_imm_s(addr, field_offsets[index] as i64);
                    self.write_const_value_to_addr(
                        builder, field_addr, &ordered[index], field_value)?;
                }
            }
            ConstValue::ResultOk(inner) => {
                let IrType::Result(ok_ty) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a result constant wants a result type, not {:?}", ty)));
                };
                // Write Ok tag (1) at offset 0.
                let tag = builder.ins().iconst(cl_types::I8, 1);
                builder.ins().store(mem_flags, tag, addr, 0);

                let payload_offset = ir_layout::result_payload_offset(ok_ty);
                let payload_addr = builder.ins().iadd_imm_s(addr, payload_offset as i64);
                let ok_ty = (**ok_ty).clone();
                self.write_const_value_to_addr(builder, payload_addr, &ok_ty, inner)?;
            }
            ConstValue::ResultErr(inner) => {
                let IrType::Result(ok_ty) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a result constant wants a result type, not {:?}", ty)));
                };
                // Write Err tag (2) at offset 0.
                let tag = builder.ins().iconst(cl_types::I8, 2);
                builder.ins().store(mem_flags, tag, addr, 0);

                // The payload sits where the ok side would, since the two
                // share the space, and the error side is an `error` whatever
                // the ok side is.
                let payload_offset = ir_layout::result_payload_offset(ok_ty);
                let payload_addr = builder.ins().iadd_imm_s(addr, payload_offset as i64);
                self.write_const_value_to_addr(
                    builder, payload_addr, &IrType::Error, inner)?;
            }
            ConstValue::Error(inner) => {
                // Error requires runtime call - write inner value then box it.
                let rt_handle = self.rt_handle_param.ok_or_else(|| {
                    CraneliftError::Codegen("Error constant requires runtime handle".into())
                })?;
                let runtime = self.runtime.ok_or_else(|| {
                    CraneliftError::Codegen("Error constant requires runtime imports".into())
                })?;

                // Infer inner type and get TyDesc.
                let inner_ir_type = datalove_datafun_ir::ir_type_of_const_value(inner);
                let inner_tydesc_id = self.tydesc_emitter.get(&inner_ir_type).ok_or_else(|| {
                    CraneliftError::Codegen(format!("TyDesc not found for inner type {:?}", inner_ir_type))
                })?;
                let inner_tydesc_gv = self.module.declare_data_in_func(inner_tydesc_id, builder.func);
                let inner_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, inner_tydesc_gv);

                // Get inner size and alignment.
                let inner_layout = ir_layout::layout_of(&inner_ir_type);
                let inner_size = inner_layout.size.max(1);
                let inner_align = inner_layout.align.max(1);

                // Allocate temp slot for the inner value.
                let slot_data = cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    inner_size,
                    align_shift(inner_align),
                );
                let temp_slot = builder.create_sized_stack_slot(slot_data);
                let inner_ptr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);

                // Write inner value to temp slot.
                self.write_const_value_to_addr(builder, inner_ptr, &inner_ir_type, inner)?;

                // Call error_from_local to box the inner value into an Error at addr.
                let error_from_ref = self.module.declare_func_in_func(runtime.error_from, builder.func);
                builder.ins().call(error_from_ref, &[rt_handle, inner_ptr, inner_tydesc_ptr, addr]);
            }
            ConstValue::Data(inner) => {
                // Data requires runtime call - write inner value then box it.
                let rt_handle = self.rt_handle_param.ok_or_else(|| {
                    CraneliftError::Codegen("Data constant requires runtime handle".into())
                })?;
                let runtime = self.runtime.ok_or_else(|| {
                    CraneliftError::Codegen("Data constant requires runtime imports".into())
                })?;

                // Infer inner type and get TyDesc.
                let inner_ir_type = datalove_datafun_ir::ir_type_of_const_value(inner);
                let inner_tydesc_id = self.tydesc_emitter.get(&inner_ir_type).ok_or_else(|| {
                    CraneliftError::Codegen(format!("TyDesc not found for inner type {:?}", inner_ir_type))
                })?;
                let inner_tydesc_gv = self.module.declare_data_in_func(inner_tydesc_id, builder.func);
                let inner_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, inner_tydesc_gv);

                // Get inner size and alignment.
                let inner_layout = ir_layout::layout_of(&inner_ir_type);
                let inner_size = inner_layout.size.max(1);
                let inner_align = inner_layout.align.max(1);

                // Allocate temp slot for the inner value.
                let slot_data = cranelift_codegen::ir::StackSlotData::new(
                    cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                    inner_size,
                    align_shift(inner_align),
                );
                let temp_slot = builder.create_sized_stack_slot(slot_data);
                let inner_ptr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);

                // Write inner value to temp slot.
                self.write_const_value_to_addr(builder, inner_ptr, &inner_ir_type, inner)?;

                // Call data_from_local to box the inner value into a Data at addr.
                let data_from_ref = self.module.declare_func_in_func(runtime.data_from, builder.func);
                builder.ins().call(data_from_ref, &[rt_handle, inner_ptr, inner_tydesc_ptr, addr]);
            }
            // An atom is one value of a type that has only that value, so it
            // occupies nothing and there is nothing to write.
            ConstValue::Enum { .. } if matches!(ty, IrType::Atom(_)) => {}
            ConstValue::Enum { variant, payload } => {
                let IrType::Enum(variants) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "an enum constant wants an enum type, not {:?}", ty)));
                };
                let variants = variants.clone();
                let index = variants.iter().position(|(name, _)| name == variant)
                    .ok_or_else(|| CraneliftError::Codegen(format!(
                        "enum constant names variant '{}', which {:?} does not have",
                        variant, ty)))?;
                let discriminant = builder.ins().iconst(cl_types::I32, index as i64);
                builder.ins().store(mem_flags, discriminant, addr, 0);

                if let Some(payload) = payload {
                    let payload_ty = variants[index].1.clone()
                        .ok_or_else(|| CraneliftError::Codegen(format!(
                            "enum constant gives variant '{}' a payload it does not take",
                            variant)))?;
                    let offset = ir_layout::enum_payload_offset(&payload_ty);
                    let payload_addr = builder.ins().iadd_imm_s(addr, offset as i64);
                    self.write_const_value_to_addr(
                        builder, payload_addr, &payload_ty, payload)?;
                }
            }

            // A collection is built rather than written, by the same cores the
            // ones with an id of their own use. What they want and this had no
            // way to give them is the element type, which now comes with the
            // address.
            ConstValue::List(elements) => {
                let IrType::List(element_type) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a list constant wants a list type, not {:?}", ty)));
                };
                let element_type = (**element_type).clone();
                self.build_list_const_at(builder, addr, &element_type, elements)?;
            }
            ConstValue::Set(elements) => {
                let IrType::Set(element_type) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a set constant wants a set type, not {:?}", ty)));
                };
                let element_type = (**element_type).clone();
                self.build_set_const_at(builder, addr, &element_type, elements)?;
            }
            ConstValue::Map(entries) => {
                let IrType::Map(key_type, value_type) = ty else {
                    return Err(CraneliftError::Codegen(format!(
                        "a map constant wants a map type, not {:?}", ty)));
                };
                let key_type = (**key_type).clone();
                let value_type = (**value_type).clone();
                self.build_map_const_at(builder, addr, &key_type, &value_type, entries)?;
            }

            // A table is the one shape left, and nothing writes one yet.
            ConstValue::Table { .. } => {
                return Err(CraneliftError::Codegen(format!(
                    "a table constant cannot be written at an address yet: {:?}", value
                )));
            }
        }
        Ok(())
    }
}

