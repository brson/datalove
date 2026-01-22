//! Runtime call instruction compilation (DebugLog, Drop).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
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
        let tydesc_id = self.tydesc_emitter.get(&actual_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for type {:?} - should have been emitted upfront",
                actual_ty
            ))
        })?;

        // Get address of tydesc.
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

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
        let tydesc_id = self.tydesc_emitter.get(&ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for type {:?} - should have been emitted upfront",
                ty
            ))
        })?;

        // Get address of tydesc.
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Declare destroy function in this function.
        let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);

        // Call dtlv_rti_any_destroy_local(rt, value_ptr, tydesc).
        builder.ins().call(destroy_ref, &[rt_handle, value_ptr, tydesc_addr]);

        Ok(())
    }

    /// Compile a DropTracked instruction.
    ///
    /// Unlike `compile_drop`, this checks initialization state and skips if the
    /// value was moved. Used for script unit_end drops where bindings may have
    /// been exported or consumed.
    ///
    /// For aggregates: checks if the first pointer-sized bytes are zero (frame
    /// is zero-initialized, so moved/uninitialized values will have null ptrs).
    /// For scalars: skips unconditionally (scalars are Copy, don't need drops).
    pub(super) fn compile_drop_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<(), CraneliftError> {
        // Get the type of the operand.
        let ty = self.get_operand_type(operand)?;

        // Copy types don't need drops.
        if ty.is_copy() {
            return Ok(());
        }

        let repr = types::ir_type_to_cranelift(&ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Scalar non-copy types (shouldn't exist in practice, but handle gracefully).
                // Just call drop unconditionally.
                self.compile_drop(builder, operand)?;
            }
            CraneliftRepr::Aggregate(_) => {
                // Aggregate: check if first 8 bytes (pointer) are null.
                // Frame is zero-initialized, so moved values will have null pointers.
                let value_ptr = self.get_operand_ptr(builder, operand)?;

                // Load the first pointer-sized value.
                let first_ptr = builder.ins().load(PTR_TYPE, MemFlags::new(), value_ptr, 0);

                // Create blocks for the conditional.
                let do_drop_block = builder.create_block();
                let after_block = builder.create_block();

                // Check if null (zero).
                let zero = builder.ins().iconst(PTR_TYPE, 0);
                let is_null = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, first_ptr, zero);

                // Branch: skip drop if null (was moved), otherwise drop.
                builder.ins().brif(is_null, after_block, &[], do_drop_block, &[]);

                // do_drop block: call destroy.
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
                let tydesc_id = self.tydesc_emitter.get(&ty).ok_or_else(|| {
                    CraneliftError::Codegen(format!(
                        "TyDesc not found for type {:?} - should have been emitted upfront",
                        ty
                    ))
                })?;

                let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                let tydesc_addr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

                let destroy_ref = self.module.declare_func_in_func(destroy_func_id, builder.func);
                builder.ins().call(destroy_ref, &[rt_handle, value_ptr, tydesc_addr]);

                // Jump to after block.
                builder.ins().jump(after_block, &[]);

                // Continue in after block.
                builder.switch_to_block(after_block);
                builder.seal_block(after_block);
            }
        }

        Ok(())
    }
}
