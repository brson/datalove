//! Boxing instruction compilation (ErrorFrom, DataFrom).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::{align_shift, PTR_TYPE};
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
        let error_from_func_id = self.runtime
            .error_from;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ErrorFrom requires runtime handle parameter".into())
        })?;

        // Get the type of the inner operand.
        let inner_ty = self.get_operand_type(inner)?;

        // Get pointer to the inner value.
        let inner_ptr = self.get_operand_ptr(builder, inner)?;

        // Get TyDesc for the inner type.
        let inner_tydesc_addr = self.tydesc_value(builder, &inner_ty)?;

        // Allocate temp slot for the Error result.
        let error_layout = datalove_datafun_ir::layout::layout_of(&IrType::Error);
        let slot_data = cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            error_layout.size,
            align_shift(error_layout.align),
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
        let data_from_func_id = self.runtime
            .data_from;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("DataFrom requires runtime handle parameter".into())
        })?;

        // Get the type of the inner operand.
        let inner_ty = self.get_operand_type(inner)?;

        // Get pointer to the inner value.
        let inner_ptr = self.get_operand_ptr(builder, inner)?;

        // Get TyDesc for the inner type.
        let inner_tydesc_addr = self.tydesc_value(builder, &inner_ty)?;

        // Allocate temp slot for the Data result.
        let data_layout = datalove_datafun_ir::layout::layout_of(&IrType::Data);
        let slot_data = cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            data_layout.size,
            align_shift(data_layout.align),
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
