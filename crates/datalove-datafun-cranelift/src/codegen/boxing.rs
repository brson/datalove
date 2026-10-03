//! Boxing instruction compilation (ErrorFrom, DataFrom).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
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

    /// Compile an Erase or Reify instruction.
    ///
    /// Both convert between a value and the erased shape a generic callee was
    /// compiled for, in opposite directions, and both need the tydescs of each
    /// side because the shapes differ only where one has `data`. The result
    /// goes in the destination's frame slot, and a scalar is loaded back into a
    /// register from there because that is how scalars are held.
    pub(super) fn compile_erasure(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
        erasing: bool,
    ) -> Result<(), CraneliftError> {
        self.compile_erasure_inner(builder, dest, src, erasing, false)
    }

    /// Compile an EraseTracked instruction.
    ///
    /// The source is the destination of an erased `out` parameter and may never
    /// have been written, in which case there is nothing to move across and
    /// reading it would read the frame's own poison. Its tracking byte says
    /// which, so the erase happens under a branch and the other way zeroes the
    /// destination: an empty `data`, which the call destroys as a no-op and the
    /// callee overwrites.
    pub(super) fn compile_erasure_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), CraneliftError> {
        self.compile_erasure_inner(builder, dest, src, true, true)
    }

    fn compile_erasure_inner(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
        erasing: bool,
        tracked: bool,
    ) -> Result<(), CraneliftError> {
        let runtime = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen("erasure requires runtime imports".into()))?;
        let func_id = if erasing { runtime.erase } else { runtime.reify };

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("erasure requires runtime handle parameter".into())
        })?;

        let src_ptr = self.get_operand_ptr(builder, src)?;
        let src_ty = self.get_operand_type(src)?;
        let src_tydesc_id = self.tydesc_emitter.get(&src_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for type {:?}", src_ty))
        })?;
        let src_tydesc_gv = self.module.declare_data_in_func(src_tydesc_id, builder.func);
        let src_tydesc_addr = builder.ins().symbol_value(PTR_TYPE, src_tydesc_gv);

        let dest_ty = self.func.value_types[dest.0 as usize].clone();
        let dest_tydesc_id = self.tydesc_emitter.get(&dest_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for type {:?}", dest_ty))
        })?;
        let dest_tydesc_gv = self.module.declare_data_in_func(dest_tydesc_id, builder.func);
        let dest_tydesc_addr = builder.ins().symbol_value(PTR_TYPE, dest_tydesc_gv);

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for erasure result".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Whether the source is a place this frame tracks, which is what says
        // it may be holding nothing.
        let guard = if tracked {
            match src {
                Operand::Param(param) => self.param_tracking_byte_offset(*param),
                other => self.tracking_byte_offset(other),
            }
        } else {
            None
        };

        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        match guard {
            None => {
                builder.ins().call(
                    func_ref,
                    &[rt_handle, src_ptr, src_tydesc_addr, dest_ptr, dest_tydesc_addr],
                );
            }
            Some(track_offset) => {
                use datalove_datafun_ir::frame_layout::tracking;
                use cranelift_codegen::ir::types as cl_types;

                let frame = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("tracked erasure requires frame slot".into())
                })?;
                let track_addr = builder.ins().stack_addr(PTR_TYPE, frame, track_offset as i32);
                let track_val = builder.ins().load(cl_types::I8, MemFlagsData::new(), track_addr, 0);
                let live_const = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
                let is_live = builder.ins().icmp(
                    cranelift_codegen::ir::condcodes::IntCC::Equal, track_val, live_const);

                let erase_block = builder.create_block();
                let empty_block = builder.create_block();
                let merge_block = builder.create_block();
                builder.ins().brif(is_live, erase_block, &[], empty_block, &[]);

                builder.switch_to_block(erase_block);
                builder.seal_block(erase_block);
                builder.ins().call(
                    func_ref,
                    &[rt_handle, src_ptr, src_tydesc_addr, dest_ptr, dest_tydesc_addr],
                );
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(empty_block);
                builder.seal_block(empty_block);
                let dest_size = crate::types::ir_type_to_cranelift(&dest_ty).layout().size;
                let zero = builder.ins().iconst(cl_types::I8, 0);
                let size = builder.ins().iconst(PTR_TYPE, dest_size as i64);
                builder.call_memset(self.isa.frontend_config(), dest_ptr, zero, size);
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
            }
        }

        match crate::types::ir_type_to_cranelift(&dest_ty) {
            crate::types::CraneliftRepr::Scalar(cl_ty) => {
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), dest_ptr, 0);
                self.values.insert(dest, val);
            }
            crate::types::CraneliftRepr::Aggregate(_) => {
                self.values.insert(dest, dest_ptr);
            }
        }

        Ok(())
    }
}
