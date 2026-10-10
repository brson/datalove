//! Runtime call instruction compilation (DebugLog, Drop).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::PTR_TYPE;
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a DebugLog instruction.
    pub(super) fn compile_debuglog(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<(), CraneliftError> {
        // Need runtime imports for debuglog.
        let debuglog_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("DebugLog requires runtime imports".into()))?
            .debuglog_local;

        // Need runtime handle.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("DebugLog requires runtime handle parameter".into())
        })?;

        // Get the type of the operand.
        let ty = self.get_operand_type(operand)?;

        // Handle Ref types: get the dereferenced pointer and inner type.
        // Refs are produced by GetFieldRef for borrowing field projections.
        // Note: get_operand_ptr already returns the pointer value for Ref types
        // (not a pointer to the pointer), so we just use the inner type for TyDesc.
        let (actual_ty, value_ptr) = if let IrType::Ref(inner) = &ty {
            // For Ref types, get_operand_ptr returns the contained pointer value.
            let actual_ptr = self.get_operand_ptr(builder, operand)?;
            (inner.as_ref().clone(), actual_ptr)
        } else {
            // Non-ref: get pointer to the value directly.
            let value_ptr = self.get_operand_ptr(builder, operand)?;
            (ty, value_ptr)
        };

        // Look up pre-emitted TyDesc for the actual (non-ref) type.
        let tydesc_id = self.tydesc(&actual_ty)?;

        // Get address of tydesc.
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Declare debuglog function in this function.
        let debuglog_ref = self.module.declare_func_in_func(
            debuglog_func_id,
            builder.func,
        );

        // Call debuglog.
        builder.ins().call(debuglog_ref, &[rt_handle, value_ptr, tydesc_addr]);

        Ok(())
    }

    /// Compile a Drop instruction.
    ///
    /// Calls dtlv_rti_any_destroy_local to destroy the value.
    pub(super) fn compile_drop(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<(), CraneliftError> {
        // Need runtime imports for destroy.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("Drop requires runtime imports".into()))?
            .destroy_local;

        // Need runtime handle.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Drop requires runtime handle parameter".into())
        })?;

        // Get the type of the operand.
        let ty = self.get_operand_type(operand)?;

        // Get pointer to the value.
        let value_ptr = self.get_operand_ptr(builder, operand)?;

        // Look up pre-emitted TyDesc.
        let tydesc_id = self.tydesc(&ty)?;

        // Get address of tydesc.
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Declare destroy function in this function.
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);

        // Call dtlv_rti_any_destroy_local(rt, value_ptr, tydesc).
        builder.ins().call(destroy_ref, &[rt_handle, value_ptr, tydesc_addr]);

        Ok(())
    }

    /// Compile a DropTracked instruction.
    ///
    /// Unlike `compile_drop`, this checks the tracking byte and skips if the
    /// value was moved or never initialized. Used for script unit_end drops
    /// where bindings may have been exported or consumed.
    pub(super) fn compile_drop_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<(), CraneliftError> {
        use datalove_datafun_ir::frame_layout::tracking;

        // Get the type of the operand.
        let ty = self.get_operand_type(operand)?;

        // Copy types don't need drops.
        if ty.is_copy() {
            return Ok(());
        }

        // Get the tracking byte offset. DropTracked should only be emitted for tracked operands.
        let track_offset = self.tracking_byte_offset(operand).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "DropTracked on untracked operand {:?} - IR lowering bug",
                operand
            ))
        })?;

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("DropTracked requires frame slot".into())
        })?;

        let track_addr = frame_slot.addr(builder, track_offset as i32);
        let track_byte = builder.ins().load(cl_ir::types::I8, MemFlagsData::new(), track_addr, 0);

        // Check if LIVE (0x01).
        let live_val = builder.ins().iconst(cl_ir::types::I8, tracking::LIVE as i64);
        let is_live = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, track_byte, live_val);

        let do_drop_block = builder.create_block();
        let after_block = builder.create_block();

        // Branch: drop if live, skip otherwise (uninit or moved).
        builder.ins().brif(is_live, do_drop_block, &[], after_block, &[]);

        // do_drop block: call destroy and mark as moved.
        builder.switch_to_block(do_drop_block);
        builder.seal_block(do_drop_block);

        // Get runtime imports.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("DropTracked requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("DropTracked requires runtime handle parameter".into())
        })?;

        // Look up pre-emitted TyDesc.
        let tydesc_id = self.tydesc(&ty)?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        let value_ptr = self.get_operand_ptr(builder, operand)?;

        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, value_ptr, tydesc_addr]);

        // Mark as moved after drop.
        let moved_val = builder.ins().iconst(cl_ir::types::I8, tracking::MOVED as i64);
        builder.ins().store(MemFlagsData::new(), moved_val, track_addr, 0);

        // Jump to after block.
        builder.ins().jump(after_block, &[]);

        // Continue in after block.
        builder.switch_to_block(after_block);
        builder.seal_block(after_block);

        Ok(())
    }

    /// Compile a DropViaRef instruction.
    ///
    /// Destroys the value pointed to by a reference value (from GetFieldRef).
    /// Used to destroy field values before passing a reference as an out param.
    pub(super) fn compile_drop_via_ref(
        &mut self,
        builder: &mut FunctionBuilder,
        ref_value: ValueId,
    ) -> Result<(), CraneliftError> {
        // Get the type of the ref value (should be Ref(inner_type)).
        let ref_ty = &self.func.value_types[ref_value.0 as usize];
        let inner_ty = match ref_ty {
            IrType::Ref(inner) => inner.as_ref(),
            _ => return Err(CraneliftError::Codegen(format!(
                "DropViaRef: expected Ref type, got {:?}", ref_ty
            ))),
        };

        // Copy types don't need drops.
        if inner_ty.is_copy() {
            return Ok(());
        }

        // Get runtime imports.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("DropViaRef requires runtime imports".into()))?
            .destroy_local;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("DropViaRef requires runtime handle parameter".into())
        })?;

        // Get the ref value (pointer to field).
        let ref_ptr = self.get_value_as_ref_ptr(builder, ref_value)?;

        // Get tydesc for inner type.
        let tydesc_id = self.tydesc(inner_ty)?;

        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().symbol_value(PTR_TYPE, tydesc_gv);

        // Declare destroy function and call it.
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, ref_ptr, tydesc_addr]);

        Ok(())
    }
}
