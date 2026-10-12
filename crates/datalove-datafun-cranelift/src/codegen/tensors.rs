//! Tensor indexing instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, BlockArg, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::index_types::INDEX_TYPE;
use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a TensorBoundsCheck instruction.
    ///
    /// Loads shape[0] and compares against the index.
    pub(super) fn compile_tensor_bounds_check(
        &mut self,
        builder: &mut FunctionBuilder,
        is_valid: ValueId,
        tensor: &Operand,
        index: &Operand,
    ) -> Result<(), CraneliftError> {
        let tensor_ptr = self.get_operand_ptr(builder, tensor)?;
        let idx = self.get_operand_value(builder, index)?;

        // Load shape pointer from Tensor struct.
        let shape_offset = std::mem::offset_of!(datalove_rtdt::Tensor, shape) as i32;
        let shape_ptr = builder.ins().load(PTR_TYPE, MemFlagsData::new(), tensor_ptr, shape_offset);

        // Load shape[0].
        let dim0 = builder.ins().load(INDEX_TYPE, MemFlagsData::new(), shape_ptr, 0);

        let is_valid_val = builder.ins().icmp(
            cl_ir::condcodes::IntCC::UnsignedLessThan,
            idx,
            dim0,
        );

        self.values.insert(is_valid, is_valid_val);
        Ok(())
    }

    /// How wide a tensor's elements are: a compile-time constant, or for a
    /// dynamic element type, read from its descriptor table entry.
    fn tensor_elem_width(
        &mut self,
        builder: &mut FunctionBuilder,
        elem_ty: &IrType,
    ) -> Result<cl_ir::Value, CraneliftError> {
        if !elem_ty.is_dynamic() {
            let size = types::ir_type_to_cranelift(elem_ty).layout().size;
            return Ok(builder.ins().iconst(INDEX_TYPE, size as i64));
        }
        let elem_desc = self.table_entry(elem_ty)?;
        let size_offset = std::mem::offset_of!(datalove_rtdt::TyDesc, size) as i32;
        let size = builder.ins().load(
            cl_types::I32, MemFlagsData::new(), elem_desc, size_offset);
        let size = if INDEX_TYPE == cl_types::I32 {
            size
        } else {
            builder.ins().uextend(INDEX_TYPE, size)
        };
        Ok(size)
    }

    /// Compute element address in a tensor for rank-1 access.
    ///
    /// Address = ptr_base + (offset + index * strides[0]) * elem_size
    fn compute_tensor_element_addr(
        &self,
        builder: &mut FunctionBuilder,
        tensor_ptr: cl_ir::Value,
        idx: cl_ir::Value,
        elem_size: cl_ir::Value,
    ) -> cl_ir::Value {
        let ptr_base = builder.ins().load(
            PTR_TYPE, MemFlagsData::new(), tensor_ptr,
            std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base) as i32,
        );
        let offset_elems = builder.ins().load(
            INDEX_TYPE, MemFlagsData::new(), tensor_ptr,
            std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems) as i32,
        );
        let strides_ptr = builder.ins().load(
            PTR_TYPE, MemFlagsData::new(), tensor_ptr,
            std::mem::offset_of!(datalove_rtdt::Tensor, strides) as i32,
        );
        let stride0 = builder.ins().load(INDEX_TYPE, MemFlagsData::new(), strides_ptr, 0);

        // linear_offset = offset + index * stride0
        let idx_stride = builder.ins().imul(idx, stride0);
        let linear_offset = builder.ins().iadd(offset_elems, idx_stride);

        // byte_offset = linear_offset * elem_size
        let byte_offset = builder.ins().imul(linear_offset, elem_size);

        // Widen to pointer width if needed.
        let byte_offset_wide = if INDEX_TYPE != PTR_TYPE {
            builder.ins().uextend(PTR_TYPE, byte_offset)
        } else {
            byte_offset
        };

        builder.ins().iadd(ptr_base, byte_offset_wide)
    }

    /// Compile a TensorGet instruction.
    ///
    /// Rank 1: bounds check + clone element. Rank > 1: call hyperplane_clone.
    pub(super) fn compile_tensor_get(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_valid: ValueId,
        tensor: &Operand,
        index: &Operand,
    ) -> Result<(), CraneliftError> {
        let tensor_ty = self.get_operand_type(tensor)?;
        let (elem_ty, rank) = match &tensor_ty {
            IrType::Tensor(e, r) => (e.as_ref().clone(), *r),
            _ => return Err(CraneliftError::Codegen(format!(
                "TensorGet on non-tensor type: {:?}", tensor_ty
            ))),
        };

        let tensor_ptr = self.get_operand_ptr(builder, tensor)?;
        let idx = self.get_operand_value(builder, index)?;

        // Bounds check.
        let shape_offset = std::mem::offset_of!(datalove_rtdt::Tensor, shape) as i32;
        let shape_ptr = builder.ins().load(PTR_TYPE, MemFlagsData::new(), tensor_ptr, shape_offset);
        let dim0 = builder.ins().load(INDEX_TYPE, MemFlagsData::new(), shape_ptr, 0);
        let is_valid_val = builder.ins().icmp(
            cl_ir::condcodes::IntCC::UnsignedLessThan,
            idx,
            dim0,
        );
        self.values.insert(is_valid, is_valid_val);

        let load_block = builder.create_block();
        let skip_block = builder.create_block();
        let merge_block = builder.create_block();

        if rank == 1 {
            let elem_repr = types::ir_type_to_cranelift(&elem_ty);
            let elem_size = self.tensor_elem_width(builder, &elem_ty)?;

            match &elem_repr {
                CraneliftRepr::Scalar(cl_ty) => {
                    let cl_ty = *cl_ty;
                    builder.append_block_param(merge_block, cl_ty);

                    builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                    builder.switch_to_block(load_block);
                    builder.seal_block(load_block);
                    let elem_addr = self.compute_tensor_element_addr(builder, tensor_ptr, idx, elem_size);
                    let val = builder.ins().load(cl_ty, MemFlagsData::new(), elem_addr, 0);
                    builder.ins().jump(merge_block, &[BlockArg::from(val)]);

                    builder.switch_to_block(skip_block);
                    builder.seal_block(skip_block);
                    let zero = Self::emit_scalar_zero(builder, cl_ty);
                    builder.ins().jump(merge_block, &[BlockArg::from(zero)]);

                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                    let phi_val = builder.block_params(merge_block)[0];
                    self.values.insert(dest, phi_val);
                }
                CraneliftRepr::Aggregate(_) => {
                    let dest_addr = self.dest_addr(builder, dest)?;

                    builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                    builder.switch_to_block(load_block);
                    builder.seal_block(load_block);
                    let elem_addr = self.compute_tensor_element_addr(builder, tensor_ptr, idx, elem_size);

                    let runtime = self.runtime;
                    let rt_handle = self.rt_handle_param.ok_or_else(|| {
                        CraneliftError::Codegen("TensorGet aggregate requires runtime handle".into())
                    })?;
                    let tydesc_ptr = self.tydesc_value(builder, &elem_ty)?;

                    let clone_ref = self.module
                        .declare_func_in_func(runtime.clone_local, builder.func);
                    builder.ins().call(clone_ref, &[
                        rt_handle, elem_addr, tydesc_ptr, dest_addr, tydesc_ptr,
                    ]);
                    builder.ins().jump(merge_block, &[]);

                    builder.switch_to_block(skip_block);
                    builder.seal_block(skip_block);
                    builder.ins().jump(merge_block, &[]);

                    builder.switch_to_block(merge_block);
                    builder.seal_block(merge_block);
                    self.values.insert(dest, dest_addr);
                }
            }
        } else {
            // Rank > 1: call hyperplane_clone.
            let dest_addr = self.dest_addr(builder, dest)?;

            builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

            builder.switch_to_block(load_block);
            builder.seal_block(load_block);

            let runtime = self.runtime;
            let rt_handle = self.rt_handle_param.ok_or_else(|| {
                CraneliftError::Codegen("TensorGet hyperplane requires runtime handle".into())
            })?;
            let tydesc_ptr = self.tydesc_value(builder, &tensor_ty)?;

            let hp_ref = self.module.declare_func_in_func(runtime.tensor_hyperplane_clone, builder.func);
            builder.ins().call(hp_ref, &[
                rt_handle,
                tensor_ptr,
                tydesc_ptr,
                idx,
                dest_addr,
            ]);
            builder.ins().jump(merge_block, &[]);

            builder.switch_to_block(skip_block);
            builder.seal_block(skip_block);
            builder.ins().jump(merge_block, &[]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            self.values.insert(dest, dest_addr);
        }

        self.emit_conditional_tracking(builder, dest, is_valid_val);
        Ok(())
    }

    /// Compile a TensorSet instruction, rank 1 only.
    ///
    /// As ListSet: the runtime checks the index and writes, or destroys the
    /// value if the index is past the end. A rank 1 tensor's indices are the
    /// one index, so its address is the indices array.
    pub(super) fn compile_tensor_set(
        &mut self,
        builder: &mut FunctionBuilder,
        is_valid: ValueId,
        tensor: &Operand,
        index: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let set = self.runtime.tensor_set_erased;
        let tensor_ptr = self.get_operand_ptr(builder, tensor)?;
        let tensor_tydesc = self.operand_tydesc(builder, tensor)?;
        let indices = self.get_operand_ptr(builder, index)?;
        self.compile_checked_set(builder, is_valid, set, [tensor_ptr, tensor_tydesc, indices], value)
    }

    /// Compile a TensorIndexRef instruction.
    ///
    /// Rank 1: pointer to element. Rank > 1: construct view Tensor on stack.
    pub(super) fn compile_tensor_index_ref(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        tensor: &Operand,
        index: &Operand,
    ) -> Result<(), CraneliftError> {
        let tensor_ty = self.get_operand_type(tensor)?;
        let (elem_ty, rank) = match &tensor_ty {
            IrType::Tensor(e, r) => (e.as_ref().clone(), *r),
            _ => return Err(CraneliftError::Codegen(format!(
                "TensorIndexRef on non-tensor type: {:?}", tensor_ty
            ))),
        };

        let tensor_ptr = self.get_operand_ptr(builder, tensor)?;
        let idx = self.get_operand_value(builder, index)?;

        if rank == 1 {
            // Rank 1: element pointer.
            let elem_size = self.tensor_elem_width(builder, &elem_ty)?;
            let elem_addr = self.compute_tensor_element_addr(builder, tensor_ptr, idx, elem_size);
            self.values.insert(dest, elem_addr);
        } else {
            // Rank > 1: construct view Tensor on stack.
            // Allocate a stack slot for the Tensor struct.
            let tensor_struct_size = std::mem::size_of::<datalove_rtdt::Tensor>() as u32;
            let ss = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                tensor_struct_size,
                types::align_shift(std::mem::align_of::<datalove_rtdt::Tensor>() as u32),
            ));
            let view_ptr = builder.ins().stack_addr(PTR_TYPE, ss, 0);

            // Load fields from parent tensor.
            let ptr_base = builder.ins().load(
                PTR_TYPE, MemFlagsData::new(), tensor_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base) as i32,
            );
            let offset_elems = builder.ins().load(
                INDEX_TYPE, MemFlagsData::new(), tensor_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems) as i32,
            );
            let strides_ptr = builder.ins().load(
                PTR_TYPE, MemFlagsData::new(), tensor_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, strides) as i32,
            );
            let shape_ptr = builder.ins().load(
                PTR_TYPE, MemFlagsData::new(), tensor_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, shape) as i32,
            );
            let layout = builder.ins().load(
                cl_ir::types::I8, MemFlagsData::new(), tensor_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, layout) as i32,
            );

            // Compute new offset: offset + idx * strides[0].
            let stride0 = builder.ins().load(INDEX_TYPE, MemFlagsData::new(), strides_ptr, 0);
            let idx_stride = builder.ins().imul(idx, stride0);
            let new_offset = builder.ins().iadd(offset_elems, idx_stride);

            // Advance shape and strides pointers by one element.
            let index_size = std::mem::size_of::<datalove_rtdt::Index>() as i64;
            let index_size_val = builder.ins().iconst(PTR_TYPE, index_size);
            let new_shape = builder.ins().iadd(shape_ptr, index_size_val);
            let new_strides = builder.ins().iadd(strides_ptr, index_size_val);

            // capacity_elems = 0 (view sentinel).
            let zero = builder.ins().iconst(INDEX_TYPE, 0);

            // Store all fields into the stack slot.
            builder.ins().store(MemFlagsData::new(), ptr_base, view_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base) as i32);
            builder.ins().store(MemFlagsData::new(), zero, view_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, capacity_elems) as i32);
            builder.ins().store(MemFlagsData::new(), new_offset, view_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems) as i32);
            builder.ins().store(MemFlagsData::new(), new_shape, view_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, shape) as i32);
            builder.ins().store(MemFlagsData::new(), new_strides, view_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, strides) as i32);
            builder.ins().store(MemFlagsData::new(), layout, view_ptr,
                std::mem::offset_of!(datalove_rtdt::Tensor, layout) as i32);

            self.values.insert(dest, view_ptr);
        }

        Ok(())
    }
}
