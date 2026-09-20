//! Slot instruction compilation (SlotStore, SlotLoad).

use cranelift_codegen::ir::{InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{Operand, ParamId, SlotDest, SlotId, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a SlotStore instruction.
    pub(super) fn compile_slot_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &SlotDest,
        value: &Operand,
        is_copy: bool,
    ) -> Result<(), CraneliftError> {
        let slot_id = match dest {
            SlotDest::Local(id) => *id,
            SlotDest::External { unit, slot } => {
                return Err(CraneliftError::Unsupported(format!(
                    "writing slot {:?} of script unit {}, which finished earlier",
                    slot, unit
                )));
            }
        };

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for slot store".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot_id.0);
        let slot_ty = &self.func.slot_types[slot_id.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                builder.ins().store(MemFlagsData::new(), val, addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copy bytes from source to destination.
                // The old value (if any) is destroyed by explicit drop.tracked in the IR
                // before this store instruction.
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_ptr, size);

                if !is_copy {
                    // Zero out the source to complete the move (prevents double-free).
                    let zero = builder.ins().iconst(cranelift_codegen::ir::types::I8, 0);
                    builder.call_memset(self.isa.frontend_config(), src_ptr, zero, size);
                }
            }
        }

        Ok(())
    }

    /// Compile a SlotLoad instruction.
    ///
    /// For aggregates, this returns a pointer to the slot. The `is_copy` parameter
    /// is available for future optimization but not currently used here - move
    /// semantics for aggregates are handled when the value is consumed elsewhere.
    pub(super) fn compile_slot_load(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        slot: SlotId,
        _is_copy: bool,
    ) -> Result<(), CraneliftError> {
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for slot load".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot.0);
        let slot_ty = &self.func.slot_types[slot.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Scalar: load value directly.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copied into the destination's own room rather
                // than pointed at where it lies.
                //
                // Pointing at the slot would be cheaper and is wrong: a load
                // takes the value out, and what is taken has to survive the
                // slot being written again. A loop that reads a slot, stores
                // a fresh value into it and only then drops what it took --
                // which is what `set x = some elem` after reading `x` lowers
                // to -- would otherwise drop the fresh value and leave the
                // old one, freeing one thing twice. The other backends copy
                // here for the same reason.
                let src = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                let dest_offset = self.layout.value_offset(dest.0);
                let dst = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
                if layout.size > 0 {
                    let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                    builder.call_memcpy(self.isa.frontend_config(), dst, src, size);
                }
                self.values.insert(dest, dst);
            }
        }

        Ok(())
    }

    /// Compile a ParamStore instruction.
    ///
    /// Stores a value to a mut/out param (writes to caller's memory).
    /// For mut params, destroys the old value first.
    pub(super) fn compile_param_store(
        &mut self,
        builder: &mut FunctionBuilder,
        param: ParamId,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        // Get the param pointer (points to caller's data).
        let param_ptr = self.param_values.get(&param).copied().ok_or_else(|| {
            CraneliftError::Codegen(format!("undefined param: {:?}", param))
        })?;

        let param_ty = &self.func_ctx.param_types[param.0 as usize];
        let repr = types::ir_type_to_cranelift(param_ty);

        // For mut params, destroy the old value first.
        // (Out params would be uninitialized, but we don't track that at compile time.)
        // Call destroy_local on the existing value.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("ParamStore requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ParamStore requires runtime handle parameter".into())
        })?;

        // Get TyDesc for the param type.
        let tydesc_id = self.tydesc_emitter.get(param_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for param type {:?}",
                param_ty
            ))
        })?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Call destroy_local(rt_handle, value_ptr, tydesc).
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, param_ptr, tydesc_addr]);

        // Now store the new value.
        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, param_ptr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copy bytes from source to param.
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), param_ptr, src_ptr, size);
            }
        }

        Ok(())
    }

    /// Compile a ParamStoreTracked instruction.
    ///
    /// For Out params: caller destroys before call, so first write sees
    /// uninitialized memory. Check tracking byte before destroying.
    pub(super) fn compile_param_store_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        param: ParamId,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        use crate::layout::tracking;
        use cranelift_codegen::ir::types as cl_types;

        // Get the param pointer (points to caller's data).
        let param_ptr = self.param_values.get(&param).copied().ok_or_else(|| {
            CraneliftError::Codegen(format!("undefined param: {:?}", param))
        })?;

        let param_ty = &self.func_ctx.param_types[param.0 as usize];
        let repr = types::ir_type_to_cranelift(param_ty);

        // Get tracking byte offset (should exist for tracked params).
        let track_offset = self.param_tracking_byte_offset(param)
            .ok_or_else(|| CraneliftError::Codegen(format!(
                "ParamStoreTracked: param {:?} has no tracking byte", param
            )))?;

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("ParamStoreTracked requires frame slot".into())
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

        // Destroy block: destroy old value, then jump to store.
        builder.switch_to_block(destroy_block);
        builder.seal_block(destroy_block);

        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("ParamStoreTracked requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ParamStoreTracked requires runtime handle parameter".into())
        })?;

        let tydesc_id = self.tydesc_emitter.get(param_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for param type {:?}",
                param_ty
            ))
        })?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, param_ptr, tydesc_addr]);

        builder.ins().jump(store_block, &[]);

        // Store block: store new value, mark as LIVE.
        builder.switch_to_block(store_block);
        builder.seal_block(store_block);

        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, param_ptr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), param_ptr, src_ptr, size);
            }
        }

        // Mark param as LIVE.
        self.mark_param_live(builder, param);

        Ok(())
    }

    /// Compile a RefStore instruction.
    ///
    /// Stores a value through a reference operand (Slot, ValueRef, etc).
    /// Used after inlining mut params.
    pub(super) fn compile_ref_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        // Get the destination pointer.
        let dest_ptr = self.get_operand_ptr(builder, dest)?;

        // Get the type of the destination.
        let dest_ty = self.get_operand_type(dest)?;
        let repr = types::ir_type_to_cranelift(&dest_ty);

        // Destroy the old value first.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("RefStore requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("RefStore requires runtime handle parameter".into())
        })?;

        let tydesc_id = self.tydesc_emitter.get(&dest_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for ref store type {:?}",
                dest_ty
            ))
        })?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Call destroy_local(rt_handle, value_ptr, tydesc).
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, dest_ptr, tydesc_addr]);

        // Now store the new value.
        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, dest_ptr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_ptr, src_ptr, size);
            }
        }

        Ok(())
    }

    /// Compile a RefSetField instruction.
    ///
    /// Stores a value to a field through a reference operand.
    /// Used after inlining mut params that have field access.
    pub(super) fn compile_ref_set_field(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        use datalove_datafun_ir::IrType;

        // Get the base pointer and type.
        let mut current_addr = self.get_operand_ptr(builder, dest)?;
        let mut current_ty = self.get_operand_type(dest)?;

        // Navigate field path to find target.
        for &field_idx in field_path.iter() {
            let field_types: Vec<_> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => {
                    return Err(CraneliftError::Codegen(format!(
                        "ref_set_field path through non-aggregate type: {:?}",
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

        let repr = types::ir_type_to_cranelift(&current_ty);

        // Destroy the old value first.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("RefSetField requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("RefSetField requires runtime handle parameter".into())
        })?;

        let tydesc_id = self.tydesc_emitter.get(&current_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for ref set field type {:?}",
                current_ty
            ))
        })?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Call destroy_local(rt_handle, field_ptr, tydesc).
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, current_addr, tydesc_addr]);

        // Now store the new value.
        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), current_addr, src_ptr, size);
            }
        }

        Ok(())
    }

    /// Compile a RefStoreTracked instruction.
    ///
    /// Stores a value through a reference operand, without destroying old value.
    /// Used after inlining out params (destination was uninitialized).
    pub(super) fn compile_ref_store_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        // Get the destination pointer.
        let dest_ptr = self.get_operand_ptr(builder, dest)?;

        // Get the type of the destination.
        let dest_ty = self.get_operand_type(dest)?;
        let repr = types::ir_type_to_cranelift(&dest_ty);

        // Store the value (no destroy since destination was uninitialized).
        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, dest_ptr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_ptr, src_ptr, size);
            }
        }

        // Mark the destination as live.
        self.mark_tracking_live(builder, dest);

        Ok(())
    }

    /// Compile a RefSetFieldTracked instruction.
    ///
    /// Stores a value to a field through a reference operand, without destroying old value.
    /// Used after inlining out params with field access.
    pub(super) fn compile_ref_set_field_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        use datalove_datafun_ir::IrType;

        // Get the base pointer and type.
        let mut current_addr = self.get_operand_ptr(builder, dest)?;
        let mut current_ty = self.get_operand_type(dest)?;

        // Navigate field path to find target.
        for &field_idx in field_path.iter() {
            let field_types: Vec<_> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => {
                    return Err(CraneliftError::Codegen(format!(
                        "ref_set_field_tracked path through non-aggregate type: {:?}",
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

        let repr = types::ir_type_to_cranelift(&current_ty);

        // Store the value (no destroy since destination was uninitialized).
        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, current_addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), current_addr, src_ptr, size);
            }
        }

        // Mark the destination as live.
        self.mark_tracking_live(builder, dest);

        Ok(())
    }
}
