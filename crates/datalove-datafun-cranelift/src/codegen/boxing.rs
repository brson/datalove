//! Boxing instruction compilation (ErrorFrom, DataFrom).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{Operand, ValueId};

use crate::types::PTR_TYPE;
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile an ErrorFrom instruction.
    ///
    /// Creates an Error from any value by moving the value to heap storage.
    pub(super) fn compile_error_from(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &Operand,
    ) -> Result<(), CraneliftError> {
        let error_from_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("ErrorFrom requires runtime imports".into()))?
            .error_from;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ErrorFrom requires runtime handle parameter".into())
        })?;

        // Get the type of the inner operand.
        let inner_ty = self.get_operand_type(inner)?;

        // Get pointer to the inner value.
        let inner_ptr = self.get_operand_ptr(builder, inner)?;

        // Get TyDesc for the inner type.
        let inner_tydesc_id = self.tydesc_emitter.get(&inner_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for inner type {:?}",
                inner_ty
            ))
        })?;
        let inner_tydesc_gv = self.module.declare_data_in_func(inner_tydesc_id, builder.func);
        let inner_tydesc_addr = builder.ins().symbol_value(PTR_TYPE, inner_tydesc_gv);

        // Allocate temp slot for the Error result (16 bytes, 8-byte aligned).
        let slot_data = cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            16,
            0,
        );
        let temp_slot = builder.create_sized_stack_slot(slot_data);
        let dest_ptr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);

        // Call error_from_local(rt_handle, inner_ptr, inner_tydesc, dest_ptr).
        let error_from_ref = self.module.declare_func_in_func(error_from_func_id, builder.func);
        builder.ins().call(error_from_ref, &[rt_handle, inner_ptr, inner_tydesc_addr, dest_ptr]);

        // Store the destination pointer as the value.
        self.values.insert(dest, dest_ptr);

        Ok(())
    }

    /// Compile a DataFrom instruction.
    ///
    /// Creates a Data from any value by moving the value to heap storage.
    pub(super) fn compile_data_from(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &Operand,
    ) -> Result<(), CraneliftError> {
        let data_from_func_id = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("DataFrom requires runtime imports".into()))?
            .data_from;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("DataFrom requires runtime handle parameter".into())
        })?;

        // Get the type of the inner operand.
        let inner_ty = self.get_operand_type(inner)?;

        // Get pointer to the inner value.
        let inner_ptr = self.get_operand_ptr(builder, inner)?;

        // Get TyDesc for the inner type.
        let inner_tydesc_id = self.tydesc_emitter.get(&inner_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for inner type {:?}",
                inner_ty
            ))
        })?;
        let inner_tydesc_gv = self.module.declare_data_in_func(inner_tydesc_id, builder.func);
        let inner_tydesc_addr = builder.ins().symbol_value(PTR_TYPE, inner_tydesc_gv);

        // Allocate temp slot for the Data result (16 bytes, 8-byte aligned).
        let slot_data = cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            16,
            0,
        );
        let temp_slot = builder.create_sized_stack_slot(slot_data);
        let dest_ptr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);

        // Call data_from_local(rt_handle, inner_ptr, inner_tydesc, dest_ptr).
        let data_from_ref = self.module.declare_func_in_func(data_from_func_id, builder.func);
        builder.ins().call(data_from_ref, &[rt_handle, inner_ptr, inner_tydesc_addr, dest_ptr]);

        // Store the destination pointer as the value.
        self.values.insert(dest, dest_ptr);

        Ok(())
    }
}
