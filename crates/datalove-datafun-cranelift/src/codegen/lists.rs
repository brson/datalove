//! List indexing instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, BlockArg, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::index_types::INDEX_TYPE;
use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a ListBoundsCheck instruction.
    ///
    /// Loads the list size and compares against the index.
    pub(super) fn compile_list_bounds_check(
        &mut self,
        builder: &mut FunctionBuilder,
        is_valid: ValueId,
        list: &Operand,
        index: &Operand,
    ) -> Result<(), CraneliftError> {
        let list_ptr = self.get_operand_ptr(builder, list)?;
        let idx = self.get_operand_value(builder, index)?;

        let size_offset = std::mem::offset_of!(datalove_rtdt::List, size) as i32;
        let list_size = builder.ins().load(INDEX_TYPE, MemFlags::new(), list_ptr, size_offset);

        let is_valid_val = builder.ins().icmp(
            cl_ir::condcodes::IntCC::UnsignedLessThan,
            idx,
            list_size,
        );

        self.values.insert(is_valid, is_valid_val);
        Ok(())
    }

    /// Compute the address of an element in a list's data buffer.
    ///
    /// `list_ptr` points to the List struct. Element address is
    /// `list.data + index * elem_size`.
    fn compute_element_addr(
        &self,
        builder: &mut FunctionBuilder,
        list_ptr: cl_ir::Value,
        idx: cl_ir::Value,
        elem_size: u32,
    ) -> cl_ir::Value {
        let data_ptr = builder.ins().load(PTR_TYPE, MemFlags::new(), list_ptr, 0);
        let elem_size_val = builder.ins().iconst(PTR_TYPE, elem_size as i64);

        // Widen index to pointer width if needed.
        let idx_wide = if INDEX_TYPE != PTR_TYPE {
            builder.ins().uextend(PTR_TYPE, idx)
        } else {
            idx
        };

        let offset = builder.ins().imul(idx_wide, elem_size_val);
        builder.ins().iadd(data_ptr, offset)
    }

    /// Compile a ListGet instruction.
    ///
    /// Performs bounds check and conditionally loads the element. Uses branching
    /// to avoid loading from out-of-bounds memory.
    pub(super) fn compile_list_get(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_valid: ValueId,
        list: &Operand,
        index: &Operand,
    ) -> Result<(), CraneliftError> {
        let list_ty = self.get_operand_type(list)?;
        let elem_ty = match &list_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "ListGet on non-list type: {:?}", list_ty
            ))),
        };
        let elem_repr = types::ir_type_to_cranelift(&elem_ty);
        let elem_size = elem_repr.layout().size;

        let list_ptr = self.get_operand_ptr(builder, list)?;
        let idx = self.get_operand_value(builder, index)?;

        // Bounds check.
        let size_offset = std::mem::offset_of!(datalove_rtdt::List, size) as i32;
        let list_size = builder.ins().load(INDEX_TYPE, MemFlags::new(), list_ptr, size_offset);
        let is_valid_val = builder.ins().icmp(
            cl_ir::condcodes::IntCC::UnsignedLessThan,
            idx,
            list_size,
        );
        self.values.insert(is_valid, is_valid_val);

        let load_block = builder.create_block();
        let skip_block = builder.create_block();
        let merge_block = builder.create_block();

        match &elem_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                let cl_ty = *cl_ty;
                builder.append_block_param(merge_block, cl_ty);

                builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                // Load block: load element value, jump to merge.
                builder.switch_to_block(load_block);
                builder.seal_block(load_block);
                let elem_addr = self.compute_element_addr(builder, list_ptr, idx, elem_size);
                let val = builder.ins().load(cl_ty, MemFlags::new(), elem_addr, 0);
                builder.ins().jump(merge_block, &[BlockArg::from(val)]);

                // Skip block: provide dummy value, jump to merge.
                builder.switch_to_block(skip_block);
                builder.seal_block(skip_block);
                let zero = Self::emit_scalar_zero(builder, cl_ty);
                builder.ins().jump(merge_block, &[BlockArg::from(zero)]);

                // Merge block: phi value is the result.
                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
                let phi_val = builder.block_params(merge_block)[0];
                self.values.insert(dest, phi_val);
            }
            CraneliftRepr::Aggregate(_) => {
                // Allocate dest in frame.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for ListGet aggregate dest".into())
                })?;
                let dest_offset = self.layout.value_offset(dest.0);
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                // Load block: clone element to dest.
                builder.switch_to_block(load_block);
                builder.seal_block(load_block);
                let elem_addr = self.compute_element_addr(builder, list_ptr, idx, elem_size);

                let runtime = self.runtime.ok_or_else(|| {
                    CraneliftError::Codegen("ListGet aggregate requires runtime imports".into())
                })?;
                let rt_handle = self.rt_handle_param.ok_or_else(|| {
                    CraneliftError::Codegen("ListGet aggregate requires runtime handle".into())
                })?;
                let tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
                    CraneliftError::Codegen(format!(
                        "TyDesc not found for element type {:?}", elem_ty
                    ))
                })?;
                let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
                let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

                let clone_ref = self.module.declare_func_in_func(runtime.clone_local, builder.func);
                builder.ins().call(clone_ref, &[
                    rt_handle,
                    elem_addr,
                    tydesc_ptr,
                    dest_addr,
                    tydesc_ptr,
                ]);
                builder.ins().jump(merge_block, &[]);

                // Skip block: dest is undefined per IR semantics.
                builder.switch_to_block(skip_block);
                builder.seal_block(skip_block);
                builder.ins().jump(merge_block, &[]);

                // Merge block.
                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
                self.values.insert(dest, dest_addr);
            }
        }

        // Conditionally mark dest tracking byte based on is_valid.
        self.emit_conditional_tracking(builder, dest, is_valid_val);

        Ok(())
    }

    /// Compile a ListSet instruction.
    ///
    /// Bounds are assumed already checked. Destroys the old element and stores
    /// the new value.
    pub(super) fn compile_list_set(
        &mut self,
        builder: &mut FunctionBuilder,
        list: &Operand,
        index: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let list_ty = self.get_operand_type(list)?;
        let elem_ty = match &list_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "ListSet on non-list type: {:?}", list_ty
            ))),
        };
        let elem_repr = types::ir_type_to_cranelift(&elem_ty);
        let elem_size = elem_repr.layout().size;

        let list_ptr = self.get_operand_ptr(builder, list)?;
        let idx = self.get_operand_value(builder, index)?;

        // Compute element address.
        let elem_addr = self.compute_element_addr(builder, list_ptr, idx, elem_size);

        // Get runtime imports and tydesc.
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("ListSet requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ListSet requires runtime handle".into())
        })?;
        let tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for element type {:?}", elem_ty
            ))
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_ptr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Destroy old element.
        let destroy_ref = self.module.declare_func_in_func(runtime.destroy_local, builder.func);
        builder.ins().call(destroy_ref, &[rt_handle, elem_addr, tydesc_ptr]);

        // Store new value.
        let val = self.get_operand_value(builder, value)?;
        match elem_repr {
            CraneliftRepr::Scalar(_) => {
                builder.ins().store(MemFlags::new(), val, elem_addr, 0);
            }
            CraneliftRepr::Aggregate(_) => {
                let move_ref = self.module.declare_func_in_func(runtime.move_value, builder.func);
                builder.ins().call(move_ref, &[rt_handle, val, tydesc_ptr, elem_addr]);
            }
        }

        Ok(())
    }

    /// Compile a ListElementRef instruction.
    ///
    /// Computes a pointer to the element at the given index. Bounds must
    /// already be checked. Stores the pointer directly as the dest value.
    pub(super) fn compile_list_element_ref(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        list: &Operand,
        index: &Operand,
    ) -> Result<(), CraneliftError> {
        let list_ty = self.get_operand_type(list)?;
        let elem_ty = match &list_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "ListElementRef on non-list type: {:?}", list_ty
            ))),
        };
        let elem_repr = types::ir_type_to_cranelift(&elem_ty);
        let elem_size = elem_repr.layout().size;

        let list_ptr = self.get_operand_ptr(builder, list)?;
        let idx = self.get_operand_value(builder, index)?;

        let elem_addr = self.compute_element_addr(builder, list_ptr, idx, elem_size);
        self.values.insert(dest, elem_addr);
        Ok(())
    }
}
