//! Aggregate type instruction compilation (Pack, Unpack, Copy).

use cranelift_codegen::ir::{InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ParamId, ValueId};

use crate::types::{self, CraneliftRepr};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a copy instruction.
    pub(super) fn compile_copy(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), CraneliftError> {
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
    ) -> Result<(), CraneliftError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let repr = types::ir_type_to_cranelift(dest_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple that fits in a register.
                if fields.len() == 1 {
                    let val = self.get_operand_value(builder, &fields[0])?;
                    self.values.insert(dest, val);
                } else {
                    panic!("a scalar `pack` of {} fields; lowering packs one field \
                            into a scalar and more only into an aggregate", fields.len());
                }
            }
            CraneliftRepr::Aggregate(_layout) => {
                // Allocate in frame and store fields.

                let base = self.dest_addr(builder, dest)?;

                // Get field offsets.
                let field_types: Vec<_> = match dest_ty {
                    IrType::Unit => Vec::new(),  // Unit is empty tuple.
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        panic!("`pack` into {:?}, which is neither a tuple nor a struct", dest_ty);
                    }
                };

                let dest_ty = dest_ty.clone();
                for (i, field_op) in fields.iter().enumerate() {
                    let field_val = self.get_operand_value(builder, field_op)?;
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);
                    let addr = self.field_addr(builder, base, &dest_ty, i as u32)?;

                    match field_repr {
                        CraneliftRepr::Scalar(_) => {
                            builder.ins().store(MemFlagsData::new(), field_val, addr, 0);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Move aggregate bytes from source to destination.
                            let dst_addr = addr;

                            // Look up pre-emitted TyDesc.
                            let tydesc_ptr = self.tydesc_value(builder, field_ty)?;

                            // Call move_value runtime function.
                            let move_func_id = self.runtime
                                .move_value;
                            let rt_handle = self.rt_handle_param.ok_or_else(|| {
                                CraneliftError::Codegen("Pack aggregate requires runtime handle parameter".into())
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
    ) -> Result<(), CraneliftError> {
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
                    panic!("a scalar `unpack` into {} destinations; lowering unpacks a \
                            scalar into one and an aggregate into the rest", dests.len());
                }
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; load fields.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        panic!("`unpack` of {:?}, which is neither a tuple nor a struct", src_ty);
                    }
                };

                for (i, &dest) in dests.iter().enumerate() {
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);
                    let addr = self.field_addr(builder, base, &src_ty, i as u32)?;

                    match field_repr {
                        CraneliftRepr::Scalar(field_cl_ty) => {
                            let val = builder.ins().load(field_cl_ty, MemFlagsData::new(), addr, 0);
                            self.values.insert(dest, val);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Return pointer to field.
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
    ) -> Result<(), CraneliftError> {
        let src_ty = self.get_operand_type(src)?;
        let repr = types::ir_type_to_cranelift(&src_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple that fits in a register.
                if field_index == 0 {
                    let val = self.get_operand_value(builder, src)?;
                    self.values.insert(dest, val);
                } else {
                    panic!("field {} of a scalar, which has only field 0", field_index);
                }
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; load the field.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        panic!("field of {:?}, which is neither a tuple nor a struct", src_ty);
                    }
                };

                if field_index as usize >= field_types.len() {
                    return Err(CraneliftError::Codegen(format!(
                        "field index {} out of bounds for type with {} fields",
                        field_index,
                        field_types.len()
                    )));
                }

                let field_ty = field_types[field_index as usize].clone();
                let field_repr = types::ir_type_to_cranelift(&field_ty);
                let field_addr = self.field_addr(builder, base, &src_ty, field_index)?;

                match field_repr {
                    CraneliftRepr::Scalar(field_cl_ty) => {
                        let val = builder.ins().load(field_cl_ty, MemFlagsData::new(), field_addr, 0);
                        self.values.insert(dest, val);
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // Copy the field into the dest's own place.
                        // Only copy types are allowed for field projections.
                        let dest_addr = self.dest_addr(builder, dest)?;
                        self.copy_value_bytes(builder, dest_addr, field_addr, &field_ty)?;
                        self.values.insert(dest, dest_addr);
                    }
                }
            }
        }

        Ok(())
    }

    /// Compile a get_field_ref instruction (get pointer to field).
    ///
    /// Unlike get_field which copies the field value, this returns a pointer
    /// to the field. Used when passing field projections to ref/mut/out params.
    pub(super) fn compile_get_field_ref(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
        field_index: u32,
    ) -> Result<(), CraneliftError> {
        let src_ty = self.get_operand_type(src)?;
        let repr = types::ir_type_to_cranelift(&src_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple. Need to get address of the value.
                // This requires spilling the scalar to memory first.
                if field_index != 0 {
                    panic!("a reference to field {} of a scalar, which has only field 0",
                        field_index);
                }
                // Get pointer to the source operand.
                let ptr = self.get_operand_ptr(builder, src)?;
                self.values.insert(dest, ptr);
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; compute field address.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        panic!("a reference to a field of {:?}, which is neither a tuple \
                                nor a struct", src_ty);
                    }
                };

                if field_index as usize >= field_types.len() {
                    return Err(CraneliftError::Codegen(format!(
                        "field index {} out of bounds for type with {} fields",
                        field_index,
                        field_types.len()
                    )));
                }

                let field_addr = self.field_addr(builder, base, &src_ty, field_index)?;

                // The ref value IS the pointer to the field.
                self.values.insert(dest, field_addr);
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
    ) -> Result<(), CraneliftError> {
        use datalove_datafun_ir::SlotDest;

        // Get base address and type.
        let (base, slot_ty) = match slot {
            SlotDest::Local(slot_id) => {
                let addr = self.get_operand_ptr(builder, &Operand::Slot(*slot_id))?;
                let ty = self.func.slot_types.get(slot_id.0 as usize)
                    .cloned()
                    .ok_or_else(|| CraneliftError::Codegen(format!("slot {:?} type not found", slot_id)))?;
                (addr, ty)
            }
            SlotDest::External { unit: _, slot: _ } => {
                return Err(CraneliftError::Unsupported(
                    "setting a field of a slot in an earlier script unit".into()
                ));
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
                    return Err(CraneliftError::Codegen(format!(
                        "set_field path through non-aggregate type: {:?}",
                        current_ty
                    )));
                }
            };

            if field_idx as usize >= field_types.len() {
                return Err(CraneliftError::Codegen(format!(
                    "field index {} out of bounds",
                    field_idx
                )));
            }

            current_addr = self.field_addr(builder, current_addr, &current_ty, field_idx)?;
            current_ty = field_types[field_idx as usize].clone();
        }


        // Store new value at target address.
        let val = self.get_operand_value(builder, value)?;
        let field_repr = types::ir_type_to_cranelift(&current_ty);

        match field_repr {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => self.move_into(builder, val, &current_ty, current_addr)?,
        }

        Ok(())
    }

    /// The address of the field `field_path` names within `base`, and its
    /// type.
    pub(super) fn field_place(
        &mut self,
        builder: &mut FunctionBuilder,
        base: &Operand,
        field_path: &[u32],
    ) -> Result<(cranelift_codegen::ir::Value, IrType), CraneliftError> {
        let mut current_addr = self.get_operand_ptr(builder, base)?;
        let mut current_ty = match self.get_operand_type(base)? {
            IrType::Ref(inner) => *inner,
            ty => ty,
        };

        for &field_idx in field_path.iter() {
            let field_types: Vec<_> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => {
                    return Err(CraneliftError::Codegen(format!(
                        "field path through non-aggregate type: {:?}",
                        current_ty
                    )));
                }
            };

            if field_idx as usize >= field_types.len() {
                return Err(CraneliftError::Codegen(format!(
                    "field index {} out of bounds",
                    field_idx
                )));
            }

            current_addr = self.field_addr(builder, current_addr, &current_ty, field_idx)?;
            current_ty = field_types[field_idx as usize].clone();
        }
        Ok((current_addr, current_ty))
    }

    /// Compile a DropField instruction: destroy a field in place, where its
    /// type owns something.
    pub(super) fn compile_drop_field(
        &mut self,
        builder: &mut FunctionBuilder,
        base: &Operand,
        field_path: &[u32],
    ) -> Result<(), CraneliftError> {
        let (addr, ty) = self.field_place(builder, base, field_path)?;
        self.destroy_value(builder, addr, &ty, None, None)
    }

    /// Compile a ParamSetField instruction.
    pub(super) fn compile_param_set_field(
        &mut self,
        builder: &mut FunctionBuilder,
        param: &datalove_datafun_ir::ParamId,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let (current_addr, current_ty) =
            self.field_place(builder, &Operand::Param(*param), field_path)?;

        let runtime = self.runtime;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ParamSetField requires runtime handle parameter".into())
        })?;

        let tydesc_ptr = self.tydesc_value(builder, &current_ty)?;


        let val = self.get_operand_value(builder, value)?;

        // Store new value at target address.
        match types::ir_type_to_cranelift(&current_ty) {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => {
                let move_ref = self.module
                    .declare_func_in_func(runtime.move_value, builder.func);
                builder.ins().call(move_ref, &[rt_handle, val, tydesc_ptr, current_addr]);
            }
        }

        Ok(())
    }

    /// Compile a ParamSetFieldTracked instruction.
    ///
    /// Writes the field and marks the param live; what the field held, a
    /// `drop.field` in front destroyed.
    pub(super) fn compile_param_set_field_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        param: &ParamId,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        // Get base address and type from param.
        let addr = self.param_values.get(param).copied().ok_or_else(|| {
            CraneliftError::Codegen(format!("param {:?} not found in param_values", param))
        })?;
        let ty = self.func_ctx.param_types.get(param.0 as usize)
            .cloned()
            .ok_or_else(|| CraneliftError::Codegen(format!("param {:?} type not found", param)))?;

        // Navigate field path to find target.
        let mut current_addr = addr;
        let mut current_ty = ty;

        for &field_idx in field_path.iter() {
            let field_types: Vec<_> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => {
                    return Err(CraneliftError::Codegen(format!(
                        "param_set_field_tracked path through non-aggregate type: {:?}",
                        current_ty
                    )));
                }
            };

            if field_idx as usize >= field_types.len() {
                return Err(CraneliftError::Codegen(format!(
                    "field index {} out of bounds",
                    field_idx
                )));
            }

            current_addr = self.field_addr(builder, current_addr, &current_ty, field_idx)?;
            current_ty = field_types[field_idx as usize].clone();
        }


        let val = self.get_operand_value(builder, value)?;
        let field_repr = types::ir_type_to_cranelift(&current_ty);

        match field_repr {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => self.move_into(builder, val, &current_ty, current_addr)?,
        }

        // Mark param as LIVE.
        self.mark_param_live(builder, *param);

        Ok(())
    }

    /// Move the aggregate of type `ty` at `src` into `dest`, through the
    /// runtime, which knows how to move each type.
    fn move_into(
        &mut self,
        builder: &mut FunctionBuilder,
        src: cranelift_codegen::ir::Value,
        ty: &IrType,
        dest: cranelift_codegen::ir::Value,
    ) -> Result<(), CraneliftError> {
        let move_value = self.runtime
            .move_value;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("moving an aggregate requires the runtime handle".into())
        })?;
        let tydesc = self.tydesc_value(builder, ty)?;
        let move_ref = self.module.declare_func_in_func(move_value, builder.func);
        builder.ins().call(move_ref, &[rt_handle, src, tydesc, dest]);
        Ok(())
    }
}
