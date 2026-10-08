//! Aggregate type instruction compilation (Pack, Unpack, Copy).

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ParamId, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
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
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for aggregate".into())
                })?;

                let dest_offset = self.layout.value_offset(dest.0);
                let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                // Get field offsets.
                let field_types: Vec<_> = match dest_ty {
                    IrType::Unit => Vec::new(),  // Unit is empty tuple.
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        panic!("`pack` into {:?}, which is neither a tuple nor a struct", dest_ty);
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, field_op) in fields.iter().enumerate() {
                    let field_val = self.get_operand_value(builder, field_op)?;
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(_) => {
                            let addr = builder.ins().iadd_imm_s(base, offsets[i] as i64);
                            builder.ins().store(MemFlagsData::new(), field_val, addr, 0);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Move aggregate bytes from source to destination.
                            let dst_addr = builder.ins().iadd_imm_s(base, offsets[i] as i64);

                            // Look up pre-emitted TyDesc.
                            let tydesc_id = self.tydesc(field_ty)?;
                            let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                            let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

                            // Call move_value runtime function.
                            let move_func_id = self.runtime.as_ref()
                                .ok_or_else(|| CraneliftError::Codegen("Pack aggregate requires runtime imports".into()))?
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

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, &dest) in dests.iter().enumerate() {
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(field_cl_ty) => {
                            let addr = builder.ins().iadd_imm_s(base, offsets[i] as i64);
                            let val = builder.ins().load(field_cl_ty, MemFlagsData::new(), addr, 0);
                            self.values.insert(dest, val);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Return pointer to field.
                            let addr = builder.ins().iadd_imm_s(base, offsets[i] as i64);
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

        // A base whose static type does not describe it has the field's offset
        // and the field's own type read from the descriptor of what arrived.
        // The runtime does both, and decides there whether the value wants
        // packing on the way out; see `dtlv_rti_field_read_local`.
        if let Some(base_desc) = self.operand_ref_desc(src) {
            return self.compile_get_field_dynamic(builder, dest, src, base_desc, field_index);
        }

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

                let offsets = types::compute_tuple_field_offsets(&field_types);
                let field_ty = &field_types[field_index as usize];
                let field_repr = types::ir_type_to_cranelift(field_ty);

                match field_repr {
                    CraneliftRepr::Scalar(field_cl_ty) => {
                        let addr = builder.ins().iadd_imm_s(base, offsets[field_index as usize] as i64);
                        let val = builder.ins().load(field_cl_ty, MemFlagsData::new(), addr, 0);
                        self.values.insert(dest, val);
                    }
                    CraneliftRepr::Aggregate(layout) => {
                        // Copy the field into the dest's frame location.
                        // Only copy types are allowed for field projections.
                        let src_addr = builder.ins().iadd_imm_s(base, offsets[field_index as usize] as i64);

                        // Get dest's frame location.
                        let frame_slot = self.frame_slot.ok_or_else(|| {
                            CraneliftError::Codegen("GetField aggregate requires frame slot".into())
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

    /// Read a field of a base whose layout only its descriptor says.
    ///
    /// A scalar destination is one whose static type is the truth -- `data` is
    /// two words and never a register -- so only the offset was in doubt and a
    /// load off the dynamic address is the whole of it. Anything else goes
    /// through the runtime, which is where the decision between copying the
    /// field and packing it belongs.
    fn compile_get_field_dynamic(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
        base_desc: cl_ir::Value,
        field_index: u32,
    ) -> Result<(), CraneliftError> {
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("a generic field read requires runtime imports".into())
        })?;
        let base = self.get_operand_value(builder, src)?;
        let dest_ty = self.func.value_types[dest.0 as usize].clone();
        let index = builder.ins().iconst(cl_types::I32, field_index as i64);

        if let CraneliftRepr::Scalar(dest_cl_ty) = types::ir_type_to_cranelift(&dest_ty) {
            let offset_fn = self.module.declare_func_in_func(runtime.field_offset, builder.func);
            let call = builder.ins().call(offset_fn, &[base_desc, index]);
            let offset = builder.inst_results(call)[0];
            let offset = builder.ins().uextend(PTR_TYPE, offset);
            let addr = builder.ins().iadd(base, offset);
            let val = builder.ins().load(dest_cl_ty, MemFlagsData::new(), addr, 0);
            self.values.insert(dest, val);
            return Ok(());
        }

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("a generic field read requires a runtime handle".into())
        })?;
        let dest_tydesc = self.static_tydesc(builder, &dest_ty)?;
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("a generic field read requires a frame slot".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        let read_fn = self.module.declare_func_in_func(runtime.field_read, builder.func);
        builder.ins().call(read_fn, &[rt_handle, dest_addr, dest_tydesc, base, base_desc, index]);
        self.record_runtime_result(builder, dest, dest_addr);
        Ok(())
    }

    /// Compile a data_borrow instruction: point at the container a `data`
    /// holds, and carry the descriptor it holds it under.
    ///
    /// A container is never packed into the two words -- `can_inline` admits
    /// none -- so the wrapper always has something to point at and the scratch
    /// `data_borrow` wants for an inline value goes unused.
    pub(super) fn compile_data_borrow(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), CraneliftError> {
        let runtime = self.runtime.ok_or_else(|| CraneliftError::Codegen(
            "DataBorrow requires runtime imports".into()))?;
        let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot, 3 * types::PTR_SIZE,
            types::align_shift(types::PTR_ALIGN)));
        let value_out = builder.ins().stack_addr(PTR_TYPE, slot, 0);
        let tydesc_out = builder.ins().stack_addr(PTR_TYPE, slot, 8);
        let scratch = builder.ins().stack_addr(PTR_TYPE, slot, 16);

        let data_ptr = self.get_operand_ptr(builder, src)?;
        let borrow_ref = self.module.declare_func_in_func(runtime.data_borrow, builder.func);
        builder.ins().call(borrow_ref, &[data_ptr, scratch, value_out, tydesc_out]);

        let value = builder.ins().load(PTR_TYPE, MemFlagsData::new(), value_out, 0);
        let tydesc = builder.ins().load(PTR_TYPE, MemFlagsData::new(), tydesc_out, 0);
        self.values.insert(dest, value);
        self.ref_desc_values.insert(dest, tydesc);
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

        // A reference whose static type says `data` where a type parameter
        // stood lies about the layout, so the offset comes from the descriptor
        // of what really arrived, and the field's own descriptor goes on to
        // describe this reference. Both are in there already: building a
        // descriptor is what settled the offsets.
        if let Some(base_desc) = self.operand_ref_desc(src) {
            let runtime = self.runtime.ok_or_else(|| {
                CraneliftError::Codegen("a generic field borrow requires runtime imports".into())
            })?;
            let base = self.get_operand_value(builder, src)?;
            let index = builder.ins().iconst(cl_types::I32, field_index as i64);

            let offset_fn = self.module.declare_func_in_func(runtime.field_offset, builder.func);
            let call = builder.ins().call(offset_fn, &[base_desc, index]);
            let offset = builder.inst_results(call)[0];
            let offset = builder.ins().uextend(PTR_TYPE, offset);
            let field_addr = builder.ins().iadd(base, offset);

            let tydesc_fn = self.module.declare_func_in_func(runtime.field_tydesc, builder.func);
            let call = builder.ins().call(tydesc_fn, &[base_desc, index]);
            let field_desc = builder.inst_results(call)[0];

            self.values.insert(dest, field_addr);
            self.ref_desc_values.insert(dest, field_desc);
            return Ok(());
        }

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

                let offsets = types::compute_tuple_field_offsets(&field_types);
                let field_addr = builder.ins().iadd_imm_s(base, offsets[field_index as usize] as i64);

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
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for set_field".into())
                })?;
                let slot_offset = self.layout.slot_offset(slot_id.0);
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
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

            let offsets = types::compute_tuple_field_offsets(&field_types);
            current_addr = builder.ins().iadd_imm_s(current_addr, offsets[field_idx as usize] as i64);
            current_ty = field_types[field_idx as usize].clone();
        }

        // Destroy old field value before overwriting (handles move types).
        // Get TyDesc for the field type.
        let tydesc_id = self.tydesc(&current_ty)?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Call destroy_local on old field value.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("SetField requires runtime imports".into()))?
            .destroy_local;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("SetField requires runtime handle parameter".into())
        })?;
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, current_addr, tydesc_ptr]);

        // Store new value at target address.
        let val = self.get_operand_value(builder, value)?;
        let field_repr = types::ir_type_to_cranelift(&current_ty);

        match field_repr {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => {
                // Call move_value runtime function.
                let move_func_id = self.runtime.as_ref()
                    .ok_or_else(|| CraneliftError::Codegen("SetField aggregate requires runtime imports".into()))?
                    .move_value;
                let move_ref = self.module.declare_func_in_func(move_func_id, builder.func);
                builder.ins().call(move_ref, &[rt_handle, val, tydesc_ptr, current_addr]);
            }
        }

        Ok(())
    }

    /// Compile a ParamSetField instruction.
    pub(super) fn compile_param_set_field(
        &mut self,
        builder: &mut FunctionBuilder,
        param: &datalove_datafun_ir::ParamId,
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

        // Navigate field path to find target. A parameter our caller described
        // is one whose static type says `data` where a type parameter stood, so
        // its offsets are wrong by whatever the difference in width is. Writing
        // at one of those is the severe half of this: it puts the value past
        // the end of what the caller owns.
        let dynamic_base = self.operand_ref_desc(&Operand::Param(*param));
        let mut current_addr = addr;
        let mut current_ty = ty;
        let mut current_desc = dynamic_base;

        for &field_idx in field_path.iter() {
            let field_types: Vec<_> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => {
                    return Err(CraneliftError::Codegen(format!(
                        "param_set_field path through non-aggregate type: {:?}",
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

            match current_desc {
                Some(desc) => {
                    let runtime = self.runtime.ok_or_else(|| CraneliftError::Codegen(
                        "a generic field write requires runtime imports".into()))?;
                    let index = builder.ins().iconst(cl_types::I32, field_idx as i64);
                    let offset_fn = self.module
                        .declare_func_in_func(runtime.field_offset, builder.func);
                    let call = builder.ins().call(offset_fn, &[desc, index]);
                    let offset = builder.inst_results(call)[0];
                    let offset = builder.ins().uextend(PTR_TYPE, offset);
                    current_addr = builder.ins().iadd(current_addr, offset);

                    let tydesc_fn = self.module
                        .declare_func_in_func(runtime.field_tydesc, builder.func);
                    let call = builder.ins().call(tydesc_fn, &[desc, index]);
                    current_desc = Some(builder.inst_results(call)[0]);
                }
                None => {
                    let offsets = types::compute_tuple_field_offsets(&field_types);
                    current_addr = builder.ins()
                        .iadd_imm_s(current_addr, offsets[field_idx as usize] as i64);
                }
            }
            current_ty = field_types[field_idx as usize].clone();
        }


        let runtime = self.runtime.ok_or_else(|| CraneliftError::Codegen(
            "ParamSetField requires runtime imports".into()))?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ParamSetField requires runtime handle parameter".into())
        })?;

        // The descriptor that says what is really at the target. Inside a
        // generic that is the one narrowed down the path above; elsewhere the
        // static type is the truth.
        let tydesc_ptr = match current_desc {
            Some(desc) => desc,
            None => {
                let tydesc_id = self.tydesc(&current_ty)?;
                let gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                builder.ins().symbol_value(PTR_TYPE, gv)
            }
        };

        // Destroy old field value before overwriting.
        let destroy_ref = self.module.declare_func_in_func(runtime.destroy_local, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, current_addr, tydesc_ptr]);

        let val = self.get_operand_value(builder, value)?;

        // Inside a generic what this function holds is in the erased shape and
        // the field is in the real one, so the value is moved back out of its
        // wrapping on the way in. `reify_local` walks the two descriptors and
        // converts wherever one says `data`.
        if current_desc.is_some() {
            let src_tydesc = self.operand_tydesc(builder, value)?;
            let src_ptr = self.get_operand_ptr(builder, value)?;
            let reify_ref = self.module.declare_func_in_func(runtime.reify, builder.func);
            builder.ins().call(reify_ref, &[rt_handle, src_ptr, src_tydesc, current_addr, tydesc_ptr]);
            return Ok(());
        }

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
    /// For Out params: caller destroys before call, so first write sees
    /// uninitialized memory. Check tracking byte before destroying.
    pub(super) fn compile_param_set_field_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        param: &ParamId,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        use datalove_datafun_ir::frame_layout::tracking;
        use cranelift_codegen::ir::types as cl_types;

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

            let offsets = types::compute_tuple_field_offsets(&field_types);
            current_addr = builder.ins().iadd_imm_s(current_addr, offsets[field_idx as usize] as i64);
            current_ty = field_types[field_idx as usize].clone();
        }

        // Get tracking byte offset (should exist for tracked params).
        let track_offset = self.param_tracking_byte_offset(*param)
            .ok_or_else(|| CraneliftError::Codegen(format!(
                "ParamSetFieldTracked: param {:?} has no tracking byte", param
            )))?;

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("ParamSetFieldTracked requires frame slot".into())
        })?;

        // Load tracking byte.
        let track_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, track_offset as i32);
        let track_val = builder.ins().load(cl_types::I8, MemFlagsData::new(), track_addr, 0);

        // Check if initialized (LIVE).
        let live_const = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
        let is_init = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            track_val,
            live_const,
        );

        // Create blocks for conditional destroy.
        let destroy_block = builder.create_block();
        let store_block = builder.create_block();

        builder.ins().brif(is_init, destroy_block, &[], store_block, &[]);

        // Destroy block: destroy old field value, then jump to store.
        builder.switch_to_block(destroy_block);
        builder.seal_block(destroy_block);

        let tydesc_id = self.tydesc(&current_ty)?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("ParamSetFieldTracked requires runtime imports".into()))?
            .destroy_local;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ParamSetFieldTracked requires runtime handle parameter".into())
        })?;
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, current_addr, tydesc_ptr]);

        builder.ins().jump(store_block, &[]);

        // Store block: store new value at target address.
        builder.switch_to_block(store_block);
        builder.seal_block(store_block);

        let val = self.get_operand_value(builder, value)?;
        let field_repr = types::ir_type_to_cranelift(&current_ty);

        match field_repr {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => {
                let move_func_id = self.runtime.as_ref()
                    .ok_or_else(|| CraneliftError::Codegen("ParamSetFieldTracked aggregate requires runtime imports".into()))?
                    .move_value;
                let move_ref = self.module.declare_func_in_func(move_func_id, builder.func);
                builder.ins().call(move_ref, &[rt_handle, val, tydesc_ptr, current_addr]);
            }
        }

        // Mark param as LIVE.
        self.mark_param_live(builder, *param);

        Ok(())
    }
}
