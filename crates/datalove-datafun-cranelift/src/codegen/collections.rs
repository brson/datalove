//! Collection type instruction compilation (List, Set, Map, Tensor, Table).

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::{align_shift, ir_type_align, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a ListNew instruction.
    ///
    /// Creates an empty list, then pushes each element.
    pub(super) fn compile_list_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ListNew requires runtime handle".into())
        })?;

        // Get list type from dest.
        let list_ty = self.func.value_types[dest.0 as usize].clone();
        let elem_ty = match &list_ty {
            IrType::List(elem) => elem.as_ref().clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "ListNew dest has non-list type: {:?}", list_ty
            ))),
        };

        // Get frame slot and destination address.
        let list_ptr = self.dest_addr(builder, dest)?;

        // Get List TyDesc.
        let list_tydesc_ptr = self.tydesc_value(builder, &list_ty)?;

        // Get element TyDesc.
        let elem_tydesc_ptr = self.tydesc_value(builder, &elem_ty)?;

        // Create empty list.
        let create_ref = self.module.declare_func_in_func(runtime.list_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, list_ptr, list_tydesc_ptr]);

        // Push each element.
        let push_ref = self.module.declare_func_in_func(runtime.list_push, builder.func);
        for elem in elements {
            let elem_ptr = self.get_operand_ptr(builder, elem)?;
            builder.ins().call(push_ref, &[
                rt_handle, list_ptr, list_tydesc_ptr, elem_ptr, elem_tydesc_ptr
            ]);
        }

        // Store pointer for this value.
        self.values.insert(dest, list_ptr);
        Ok(())
    }

    /// Compile a SetNew instruction.
    ///
    /// Creates an empty set, then inserts each element.
    pub(super) fn compile_set_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("SetNew requires runtime handle".into())
        })?;

        // Get set type from dest.
        let set_ty = self.func.value_types[dest.0 as usize].clone();
        let elem_ty = match &set_ty {
            IrType::Set(elem) => elem.as_ref().clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "SetNew dest has non-set type: {:?}", set_ty
            ))),
        };

        // Get frame slot and destination address.
        let set_ptr = self.dest_addr(builder, dest)?;

        // Get Set TyDesc.
        let set_tydesc_ptr = self.tydesc_value(builder, &set_ty)?;

        // Get element TyDesc.
        let elem_tydesc_ptr = self.tydesc_value(builder, &elem_ty)?;

        // Create empty set.
        let create_ref = self.module.declare_func_in_func(runtime.set_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, set_ptr, set_tydesc_ptr]);

        // Insert each element.
        // btreeset_insert_local needs a bool_out pointer for the result.
        // Allocate a temp stack slot for this.
        let bool_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            1,
            align_shift(1),
        ));
        let bool_ptr = builder.ins().stack_addr(PTR_TYPE, bool_slot, 0);

        let insert_ref = self.module.declare_func_in_func(runtime.set_insert, builder.func);
        for elem in elements {
            let elem_ptr = self.get_operand_ptr(builder, elem)?;
            builder.ins().call(insert_ref, &[
                rt_handle, set_ptr, set_tydesc_ptr, elem_ptr, elem_tydesc_ptr, bool_ptr
            ]);
        }

        // Store pointer for this value.
        self.values.insert(dest, set_ptr);
        Ok(())
    }

    /// Compile a MapNew instruction.
    ///
    /// Creates an empty map, then inserts each key-value pair.
    pub(super) fn compile_map_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        entries: &[(Operand, Operand)],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapNew requires runtime handle".into())
        })?;

        // Get map type from dest.
        let map_ty = self.func.value_types[dest.0 as usize].clone();
        let (key_ty, val_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "MapNew dest has non-map type: {:?}", map_ty
            ))),
        };

        // Get frame slot and destination address.
        let map_ptr = self.dest_addr(builder, dest)?;

        // Get Map TyDesc.
        let map_tydesc_ptr = self.tydesc_value(builder, &map_ty)?;

        // Get key TyDesc.
        let key_tydesc_ptr = self.tydesc_value(builder, &key_ty)?;

        // Get value TyDesc.
        let val_tydesc_ptr = self.tydesc_value(builder, &val_ty)?;

        // Create empty map.
        let create_ref = self.module.declare_func_in_func(runtime.map_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, map_ptr, map_tydesc_ptr]);

        // Insert each entry.
        let insert_ref = self.module.declare_func_in_func(runtime.map_insert, builder.func);
        for (key, val) in entries {
            let key_ptr = self.get_operand_ptr(builder, key)?;
            let val_ptr = self.get_operand_ptr(builder, val)?;
            builder.ins().call(insert_ref, &[
                rt_handle, map_ptr, map_tydesc_ptr,
                key_ptr, key_tydesc_ptr,
                val_ptr, val_tydesc_ptr
            ]);
        }

        // Store pointer for this value.
        self.values.insert(dest, map_ptr);
        Ok(())
    }

    /// Compile a TensorNew instruction.
    ///
    /// Creates a tensor from the given shape and elements.
    pub(super) fn compile_tensor_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        shape: &[u32],
        elements: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("TensorNew requires runtime handle".into())
        })?;

        // Get tensor type from dest.
        let tensor_ty = self.func.value_types[dest.0 as usize].clone();
        let elem_ty = match &tensor_ty {
            IrType::Tensor(elem, _rank) => elem.as_ref().clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "TensorNew dest has non-tensor type: {:?}", tensor_ty
            ))),
        };

        // Get frame slot and destination address.
        let tensor_ptr = self.dest_addr(builder, dest)?;

        // Get Tensor TyDesc.
        let tensor_tydesc_ptr = self.tydesc_value(builder, &tensor_ty)?;

        // Get element TyDesc.
        let elem_tydesc_ptr = self.tydesc_value(builder, &elem_ty)?;

        let element_count = elements.len() as u32;
        let rank = shape.len() as u32;
        use crate::index_types::INDEX_TYPE;

        // The elements are gathered in a buffer `tensor_init` moves them out
        // of. One whose size only its descriptor says is gathered in a buffer
        // the runtime allocates at that size, freed afterwards; any other in a
        // stack slot.
        let mut dynamic_buffer = None;
        let data_ptr = if elem_ty.is_dynamic() {
            let (size, align) = self.size_and_align(builder, &elem_ty)?;
            let count = builder.ins().iconst(INDEX_TYPE, element_count as i64);
            let alloc_ref = self.module.declare_func_in_func(runtime.mem_alloc_raw, builder.func);
            let call = builder.ins().call(alloc_ref, &[rt_handle, size, align, count]);
            let buffer = builder.inst_results(call)[0];
            let stride = builder.ins().uextend(PTR_TYPE, size);
            for (i, elem) in elements.iter().enumerate() {
                let src = self.get_operand_ptr(builder, elem)?;
                let offset = builder.ins().imul_imm_u(stride, i as i64);
                let dst = builder.ins().iadd(buffer, offset);
                self.copy_value_bytes(builder, dst, src, &elem_ty)?;
            }
            dynamic_buffer = Some((buffer, size, align, count));
            buffer
        } else {
            let elem_size = crate::types::ir_type_size(&elem_ty);
            let data_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                element_count * elem_size,
                align_shift(ir_type_align(&elem_ty)),
            ));
            for (i, elem) in elements.iter().enumerate() {
                let src = self.get_operand_ptr(builder, elem)?;
                let dst = builder.ins().stack_addr(PTR_TYPE, data_slot, (i as u32 * elem_size) as i32);
                let size_val = builder.ins().iconst(PTR_TYPE, elem_size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dst, src, size_val);
            }
            builder.ins().stack_addr(PTR_TYPE, data_slot, 0)
        };

        // Allocate stack buffer for shape array and fill it.
        let shape_buffer_size = (rank as usize) * std::mem::size_of::<u32>();
        let shape_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            shape_buffer_size as u32,
            align_shift(std::mem::align_of::<u32>() as u32),
        ));
        let shape_ptr = builder.ins().stack_addr(PTR_TYPE, shape_slot, 0);

        // Store each shape dimension.
        for (i, &dim) in shape.iter().enumerate() {
            let offset = (i * std::mem::size_of::<u32>()) as i32;
            let dim_val = builder.ins().iconst(cl_types::I32, dim as i64);
            builder.ins().store(cl_ir::MemFlagsData::new(), dim_val, shape_ptr, offset);
        }

        // Call tensor_init runtime function.
        // element_count is IndexRepr type (I32 or I64 depending on index-64 feature).
        let init_ref = self.module.declare_func_in_func(runtime.tensor_init, builder.func);
        let element_count_val = builder.ins().iconst(INDEX_TYPE, element_count as i64);
        let rank_val = builder.ins().iconst(cl_types::I32, rank as i64);
        builder.ins().call(init_ref, &[
            rt_handle,
            data_ptr,
            element_count_val,
            elem_tydesc_ptr,
            shape_ptr,
            rank_val,
            tensor_ptr,
            tensor_tydesc_ptr,
        ]);
        if let Some((buffer, size, align, count)) = dynamic_buffer {
            let free_ref = self.module.declare_func_in_func(runtime.mem_free_raw, builder.func);
            builder.ins().call(free_ref, &[rt_handle, size, align, count, buffer]);
        }

        // Store pointer for this value.
        self.values.insert(dest, tensor_ptr);
        Ok(())
    }

    /// Compile a TableNew instruction.
    ///
    /// Creates an empty table, then pushes each row (a tuple).
    pub(super) fn compile_table_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        rows: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("TableNew requires runtime handle".into())
        })?;

        // Get table type from dest.
        let table_ty = self.func.value_types[dest.0 as usize].clone();
        if !matches!(table_ty, IrType::Table(_)) {
            return Err(CraneliftError::Codegen(format!(
                "TableNew dest has non-table type: {:?}", table_ty
            )));
        }

        // Get frame slot and destination address.
        let table_ptr = self.dest_addr(builder, dest)?;

        // Get Table TyDesc.
        let table_tydesc_ptr = self.tydesc_value(builder, &table_ty)?;

        // Create empty table.
        let create_ref = self.module.declare_func_in_func(runtime.table_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, table_ptr, table_tydesc_ptr]);

        // Push each row.
        let push_ref = self.module.declare_func_in_func(runtime.table_push_row, builder.func);
        for row in rows {
            let row_ptr = self.get_operand_ptr(builder, row)?;
            let row_tydesc_ptr = self.operand_tydesc(builder, row)?;
            builder.ins().call(push_ref, &[
                rt_handle, table_ptr, table_tydesc_ptr, row_ptr, row_tydesc_ptr
            ]);
        }

        // Store pointer for this value.
        self.values.insert(dest, table_ptr);
        Ok(())
    }
}
