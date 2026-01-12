//! Runtime call instruction compilation (DebugLog, Drop).

use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand};

use crate::types::PTR_TYPE;
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a DebugLog instruction.
    pub(super) fn compile_debuglog(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<(), AotError> {
        // Need runtime imports for debuglog.
        let debuglog_func_id = self.runtime.as_ref()
            .ok_or_else(|| AotError::Codegen("DebugLog requires runtime imports".into()))?
            .debuglog_local;

        // Need runtime handle.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("DebugLog requires runtime handle parameter".into())
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
            AotError::Codegen(format!(
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
    ) -> Result<(), AotError> {
        // Need runtime imports for destroy.
        let destroy_func_id = self.runtime.as_ref()
            .ok_or_else(|| AotError::Codegen("Drop requires runtime imports".into()))?
            .destroy_local;

        // Need runtime handle.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("Drop requires runtime handle parameter".into())
        })?;

        // Get the type of the operand.
        let ty = self.get_operand_type(operand)?;

        // Get pointer to the value.
        let value_ptr = self.get_operand_ptr(builder, operand)?;

        // Look up pre-emitted TyDesc.
        let tydesc_id = self.tydesc_emitter.get(&ty).ok_or_else(|| {
            AotError::Codegen(format!(
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
}
