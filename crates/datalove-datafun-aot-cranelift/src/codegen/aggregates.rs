//! Aggregate type instruction compilation (Pack, Unpack, Copy).

use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a copy instruction.
    pub(super) fn compile_copy(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), AotError> {
        let val = self.get_operand_value(builder, src)?;
        self.values.insert(dest, val);
        Ok(())
    }

    /// Compile a pack instruction (tuple/struct creation).
    pub(super) fn compile_pack(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        fields: &[Operand],
    ) -> Result<(), AotError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let repr = types::ir_type_to_cranelift(dest_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple that fits in a register.
                if fields.len() == 1 {
                    let val = self.get_operand_value(builder, &fields[0])?;
                    self.values.insert(dest, val);
                } else {
                    return Err(AotError::Unsupported(
                        "scalar pack with multiple fields".into()
                    ));
                }
            }
            CraneliftRepr::Aggregate(_layout) => {
                // Allocate in frame and store fields.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    AotError::Codegen("no frame slot for aggregate".into())
                })?;

                let dest_offset = self.layout.value_offset(dest.0);
                let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                // Get field offsets.
                let field_types: Vec<_> = match dest_ty {
                    IrType::Unit => Vec::new(),  // Unit is empty tuple.
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "pack for non-tuple/struct: {:?}",
                            dest_ty
                        )));
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, field_op) in fields.iter().enumerate() {
                    let field_val = self.get_operand_value(builder, field_op)?;
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(_) => {
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            builder.ins().store(MemFlags::new(), field_val, addr, 0);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Move aggregate bytes from source to destination.
                            let dst_addr = builder.ins().iadd_imm(base, offsets[i] as i64);

                            // Look up pre-emitted TyDesc.
                            let tydesc_id = self.tydesc_emitter.get(field_ty).ok_or_else(|| {
                                AotError::Codegen(format!(
                                    "TyDesc not found for type {:?} - should have been emitted upfront",
                                    field_ty
                                ))
                            })?;
                            let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                            let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

                            // Call move_value runtime function.
                            let move_func_id = self.runtime.as_ref()
                                .ok_or_else(|| AotError::Codegen("Pack aggregate requires runtime imports".into()))?
                                .move_value;
                            let rt_handle = self.rt_handle_param.ok_or_else(|| {
                                AotError::Codegen("Pack aggregate requires runtime handle parameter".into())
                            })?;
                            let move_ref = self.module.declare_func_in_func(move_func_id, builder.func);
                            builder.ins().call(move_ref, &[rt_handle, field_val, tydesc_ptr, dst_addr]);
                        }
                    }
                }

                // For aggregates, we store a pointer to the frame location.
                self.values.insert(dest, base);
            }
        }

        Ok(())
    }

    /// Compile an unpack instruction (tuple/struct destructuring).
    pub(super) fn compile_unpack(
        &mut self,
        builder: &mut FunctionBuilder,
        dests: &[ValueId],
        src: &Operand,
    ) -> Result<(), AotError> {
        // Get source type from first dest's expected type.
        // Actually we need the source operand's type.
        let src_ty = self.get_operand_type(src)?;
        let repr = types::ir_type_to_cranelift(&src_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple.
                if dests.len() == 1 {
                    let val = self.get_operand_value(builder, src)?;
                    self.values.insert(dests[0], val);
                } else {
                    return Err(AotError::Unsupported(
                        "scalar unpack with multiple dests".into()
                    ));
                }
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; load fields.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "unpack from non-tuple/struct: {:?}",
                            src_ty
                        )));
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, &dest) in dests.iter().enumerate() {
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(field_cl_ty) => {
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            let val = builder.ins().load(field_cl_ty, MemFlags::new(), addr, 0);
                            self.values.insert(dest, val);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Return pointer to field.
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            self.values.insert(dest, addr);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Compile a get_field instruction (extract single field from tuple/struct).
    pub(super) fn compile_get_field(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
        field_index: u32,
    ) -> Result<(), AotError> {
        let src_ty = self.get_operand_type(src)?;
        let repr = types::ir_type_to_cranelift(&src_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple that fits in a register.
                if field_index == 0 {
                    let val = self.get_operand_value(builder, src)?;
                    self.values.insert(dest, val);
                } else {
                    return Err(AotError::Unsupported(
                        format!("scalar get_field with field_index {} (max 0)", field_index)
                    ));
                }
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; load the field.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "get_field from non-tuple/struct: {:?}",
                            src_ty
                        )));
                    }
                };

                if field_index as usize >= field_types.len() {
                    return Err(AotError::Codegen(format!(
                        "field index {} out of bounds for type with {} fields",
                        field_index,
                        field_types.len()
                    )));
                }

                let offsets = types::compute_tuple_field_offsets(&field_types);
                let field_ty = &field_types[field_index as usize];
                let field_repr = types::ir_type_to_cranelift(field_ty);

                match field_repr {
                    CraneliftRepr::Scalar(field_cl_ty) => {
                        let addr = builder.ins().iadd_imm(base, offsets[field_index as usize] as i64);
                        let val = builder.ins().load(field_cl_ty, MemFlags::new(), addr, 0);
                        self.values.insert(dest, val);
                    }
                    CraneliftRepr::Aggregate(layout) => {
                        // Copy the field into the dest's frame location.
                        // Only copy types are allowed for field projections.
                        let src_addr = builder.ins().iadd_imm(base, offsets[field_index as usize] as i64);

                        // Get dest's frame location.
                        let frame_slot = self.frame_slot.ok_or_else(|| {
                            AotError::Codegen("GetField aggregate requires frame slot".into())
                        })?;
                        let dest_offset = self.layout.value_offset(dest.0);
                        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                        // Emit memcpy for the field.
                        let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                        builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_addr, size);

                        self.values.insert(dest, dest_addr);
                    }
                }
            }
        }

        Ok(())
    }

    /// Compile a set_field instruction (set field in slot).
    pub(super) fn compile_set_field(
        &mut self,
        builder: &mut FunctionBuilder,
        slot: &datalove_datafun_ir::SlotDest,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), AotError> {
        use datalove_datafun_ir::SlotDest;

        // Get base address of the slot.
        let base = match slot {
            SlotDest::Local(slot_id) => {
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    AotError::Codegen("no frame slot for set_field".into())
                })?;
                let slot_offset = self.layout.slot_offset(slot_id.0);
                builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32)
            }
            SlotDest::External { unit: _, slot: _ } => {
                return Err(AotError::Unsupported(
                    "set_field on external slot".into()
                ));
            }
        };

        // Get slot type.
        let slot_ty = match slot {
            SlotDest::Local(slot_id) => {
                self.func.slot_types.get(slot_id.0 as usize)
                    .cloned()
                    .ok_or_else(|| AotError::Codegen(format!("slot {:?} type not found", slot_id)))?
            }
            SlotDest::External { .. } => {
                return Err(AotError::Unsupported("external slot".into()));
            }
        };

        // Navigate field path to find target.
        let mut current_addr = base;
        let mut current_ty = slot_ty;

        for &field_idx in field_path.iter() {
            let field_types: Vec<_> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => {
                    return Err(AotError::Codegen(format!(
                        "set_field path through non-aggregate type: {:?}",
                        current_ty
                    )));
                }
            };

            if field_idx as usize >= field_types.len() {
                return Err(AotError::Codegen(format!(
                    "field index {} out of bounds",
                    field_idx
                )));
            }

            let offsets = types::compute_tuple_field_offsets(&field_types);
            current_addr = builder.ins().iadd_imm(current_addr, offsets[field_idx as usize] as i64);
            current_ty = field_types[field_idx as usize].clone();
        }

        // Destroy old field value before overwriting (handles move types).
        // Get TyDesc for the field type.
        let tydesc_id = self.tydesc_emitter.get(&current_ty).ok_or_else(|| {
            AotError::Codegen(format!(
                "TyDesc not found for type {:?}",
                current_ty
            ))
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Call destroy_local on old field value.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| AotError::Codegen("SetField requires runtime imports".into()))?
            .destroy_local;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("SetField requires runtime handle parameter".into())
        })?;
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, current_addr, tydesc_ptr]);

        // Store new value at target address.
        let val = self.get_operand_value(builder, value)?;
        let field_repr = types::ir_type_to_cranelift(&current_ty);

        match field_repr {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlags::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => {
                // Call move_value runtime function.
                let move_func_id = self.runtime.as_ref()
                    .ok_or_else(|| AotError::Codegen("SetField aggregate requires runtime imports".into()))?
                    .move_value;
                let move_ref = self.module.declare_func_in_func(move_func_id, builder.func);
                builder.ins().call(move_ref, &[rt_handle, val, tydesc_ptr, current_addr]);
            }
        }

        Ok(())
    }
}
