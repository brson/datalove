//! Slot instruction compilation (SlotStore, SlotLoad).

use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{Operand, ParamId, SlotDest, SlotId, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a SlotStore instruction.
    pub(super) fn compile_slot_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &SlotDest,
        value: &Operand,
        is_copy: bool,
    ) -> Result<(), AotError> {
        let slot_id = match dest {
            SlotDest::Local(id) => *id,
            SlotDest::External { unit, slot } => {
                return Err(AotError::Unsupported(format!(
                    "external slot store (unit={}, slot={:?}) not yet implemented",
                    unit, slot
                )));
            }
        };

        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for slot store".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot_id.0);
        let slot_ty = &self.func.slot_types[slot_id.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                builder.ins().store(MemFlags::new(), val, addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: destroy old value in dest, copy from source.
                // For move semantics (!is_copy), also zero the source.
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);

                // Destroy the old value in the slot before overwriting.
                let destroy_func_id = self.runtime.as_ref()
                    .ok_or_else(|| AotError::Codegen("SlotStore aggregate requires runtime imports".into()))?
                    .destroy_local;

                let rt_handle = self.rt_handle_param.ok_or_else(|| {
                    AotError::Codegen("SlotStore aggregate requires runtime handle parameter".into())
                })?;

                // Get TyDesc for the slot type.
                let tydesc_id = self.tydesc_emitter.get(slot_ty).ok_or_else(|| {
                    AotError::Codegen(format!(
                        "TyDesc not found for slot type {:?}",
                        slot_ty
                    ))
                })?;

                let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                let tydesc_addr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

                // Call destroy_local(rt_handle, slot_ptr, tydesc) to free old value.
                let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
                builder.ins().call(destroy_ref, &[rt_handle, dest_addr, tydesc_addr]);

                // Copy bytes from source to destination.
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_ptr, size);

                if !is_copy {
                    // Zero out the source to complete the move (prevents double-free).
                    // Both Slot and Value sources need to be zeroed since they can hold
                    // allocations that would otherwise be freed when their frame storage
                    // is destroyed at function cleanup.
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
    ) -> Result<(), AotError> {
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for slot load".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot.0);
        let slot_ty = &self.func.slot_types[slot.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Scalar: load value directly.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                let val = builder.ins().load(cl_ty, MemFlags::new(), addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                // Aggregate: return pointer to slot location.
                // The value stays in place, we just track the pointer.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                self.values.insert(dest, addr);
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
    ) -> Result<(), AotError> {
        // Get the param pointer (points to caller's data).
        let param_ptr = self.param_values.get(&param).copied().ok_or_else(|| {
            AotError::Codegen(format!("undefined param: {:?}", param))
        })?;

        let param_ty = &self.func.param_types[param.0 as usize];
        let repr = types::ir_type_to_cranelift(param_ty);

        // For mut params, destroy the old value first.
        // (Out params would be uninitialized, but we don't track that at compile time.)
        // Call destroy_local on the existing value.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| AotError::Codegen("ParamStore requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("ParamStore requires runtime handle parameter".into())
        })?;

        // Get TyDesc for the param type.
        let tydesc_id = self.tydesc_emitter.get(param_ty).ok_or_else(|| {
            AotError::Codegen(format!(
                "TyDesc not found for param type {:?}",
                param_ty
            ))
        })?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Call destroy_local(rt_handle, value_ptr, tydesc).
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, param_ptr, tydesc_addr]);

        // Now store the new value.
        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlags::new(), val, param_ptr, 0);
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
}
