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
    /// Compile a ListNew whose element type only a handed-over descriptor says.
    ///
    /// The destination is a `data`, because a list built over a type parameter
    /// erases to one, so the list is made in a stack temporary described by
    /// that descriptor and moved into the destination. That is what an owned
    /// collection of a type parameter looks like everywhere else.
    pub(super) fn compile_list_new_erased(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[Operand],
        shape: u32,
    ) -> Result<(), CraneliftError> {
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("ListNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("ListNew requires runtime handle".into())
        })?;
        let list_tydesc_ptr = *self.shape_descriptor_values.get(shape as usize)
            .ok_or_else(|| CraneliftError::Codegen(format!(
                "shape {} was built with but never passed", shape)))?;

        let temp = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            std::mem::size_of::<datalove_rtdt::List>() as u32,
            align_shift(std::mem::align_of::<datalove_rtdt::List>() as u32),
        ));
        let list_ptr = builder.ins().stack_addr(PTR_TYPE, temp, 0);

        let create_ref = self.module.declare_func_in_func(runtime.list_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, list_ptr, list_tydesc_ptr]);

        // Pushed rather than copied in, and described as what it really is. An
        // element here is in the erased shape -- a `data` for a bare type
        // parameter, a tuple of them for a tuple of parameters -- and the list
        // holds its elements as what they really are, so the push converts.
        let push_ref = self.module.declare_func_in_func(runtime.list_push_erased, builder.func);
        for elem in elements {
            let elem_ptr = self.get_operand_ptr(builder, elem)?;
            let elem_tydesc_ptr = self.operand_tydesc(builder, elem)?;
            builder.ins().call(push_ref, &[
                rt_handle, list_ptr, list_tydesc_ptr, elem_ptr, elem_tydesc_ptr
            ]);
        }

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for ListNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
        let wrap_ref = self.module.declare_func_in_func(runtime.data_from_local, builder.func);
        builder.ins().call(wrap_ref, &[rt_handle, list_ptr, list_tydesc_ptr, dest_ptr]);

        self.values.insert(dest, dest_ptr);
        Ok(())
    }

    pub(super) fn compile_list_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("ListNew requires runtime imports".into())
        })?;
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
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for ListNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let list_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get List TyDesc.
        let list_tydesc_id = self.tydesc_emitter.get(&list_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for {:?}", list_ty))
        })?;
        let list_tydesc_gv = self.module.declare_data_in_func(list_tydesc_id, builder.func);
        let list_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, list_tydesc_gv);

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for element type {:?}", elem_ty))
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, elem_tydesc_gv);

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
    /// Compile a SetNew whose element type only a handed-over descriptor says.
    ///
    /// The same two steps as the list: build in a temporary the descriptor
    /// describes, then move it into the `data` the destination is.
    pub(super) fn compile_set_new_erased(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[Operand],
        shape: u32,
    ) -> Result<(), CraneliftError> {
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("SetNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("SetNew requires runtime handle".into())
        })?;
        let set_tydesc_ptr = *self.shape_descriptor_values.get(shape as usize)
            .ok_or_else(|| CraneliftError::Codegen(format!(
                "shape {} was built with but never passed", shape)))?;

        let temp = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            std::mem::size_of::<datalove_rtdt::Set>() as u32,
            align_shift(std::mem::align_of::<datalove_rtdt::Set>() as u32),
        ));
        let set_ptr = builder.ins().stack_addr(PTR_TYPE, temp, 0);
        let create_ref = self.module.declare_func_in_func(runtime.set_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, set_ptr, set_tydesc_ptr]);

        let bool_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot, 1, align_shift(1)));
        let bool_ptr = builder.ins().stack_addr(PTR_TYPE, bool_slot, 0);
        let insert_ref = self.module.declare_func_in_func(runtime.set_insert_erased, builder.func);
        for elem in elements {
            let elem_ptr = self.get_operand_ptr(builder, elem)?;
            let elem_tydesc_ptr = self.operand_tydesc(builder, elem)?;
            builder.ins().call(insert_ref, &[
                rt_handle, set_ptr, set_tydesc_ptr, elem_ptr, elem_tydesc_ptr, bool_ptr
            ]);
        }

        self.wrap_built_collection(builder, dest, set_ptr, set_tydesc_ptr, runtime)
    }

    /// Compile a MapNew whose key and value types a handed-over descriptor says.
    pub(super) fn compile_map_new_erased(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        entries: &[(Operand, Operand)],
        shape: u32,
    ) -> Result<(), CraneliftError> {
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapNew requires runtime handle".into())
        })?;
        let map_tydesc_ptr = *self.shape_descriptor_values.get(shape as usize)
            .ok_or_else(|| CraneliftError::Codegen(format!(
                "shape {} was built with but never passed", shape)))?;

        let temp = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            std::mem::size_of::<datalove_rtdt::Map>() as u32,
            align_shift(std::mem::align_of::<datalove_rtdt::Map>() as u32),
        ));
        let map_ptr = builder.ins().stack_addr(PTR_TYPE, temp, 0);
        let create_ref = self.module.declare_func_in_func(runtime.map_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, map_ptr, map_tydesc_ptr]);

        let insert_ref = self.module.declare_func_in_func(runtime.map_insert_erased, builder.func);
        for (key, val) in entries {
            let key_ptr = self.get_operand_ptr(builder, key)?;
            let key_tydesc_ptr = self.operand_tydesc(builder, key)?;
            let val_ptr = self.get_operand_ptr(builder, val)?;
            let val_tydesc_ptr = self.operand_tydesc(builder, val)?;
            builder.ins().call(insert_ref, &[
                rt_handle, map_ptr, map_tydesc_ptr,
                key_ptr, key_tydesc_ptr, val_ptr, val_tydesc_ptr
            ]);
        }

        self.wrap_built_collection(builder, dest, map_ptr, map_tydesc_ptr, runtime)
    }

    /// Move a freshly built collection into the `data` its destination is.
    fn wrap_built_collection(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        built_ptr: cl_ir::Value,
        tydesc_ptr: cl_ir::Value,
        runtime: crate::runtime::RuntimeImports,
    ) -> Result<(), CraneliftError> {
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("wrapping requires runtime handle".into())
        })?;
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for a built collection".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
        let wrap_ref = self.module.declare_func_in_func(runtime.data_from_local, builder.func);
        builder.ins().call(wrap_ref, &[rt_handle, built_ptr, tydesc_ptr, dest_ptr]);
        self.values.insert(dest, dest_ptr);
        Ok(())
    }

    pub(super) fn compile_set_new(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        elements: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("SetNew requires runtime imports".into())
        })?;
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
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for SetNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let set_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Set TyDesc.
        let set_tydesc_id = self.tydesc_emitter.get(&set_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for {:?}", set_ty))
        })?;
        let set_tydesc_gv = self.module.declare_data_in_func(set_tydesc_id, builder.func);
        let set_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, set_tydesc_gv);

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for element type {:?}", elem_ty))
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, elem_tydesc_gv);

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
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapNew requires runtime imports".into())
        })?;
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
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for MapNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let map_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Map TyDesc.
        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, map_tydesc_gv);

        // Get key TyDesc.
        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, key_tydesc_gv);

        // Get value TyDesc.
        let val_tydesc_id = self.tydesc_emitter.get(&val_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for value type {:?}", val_ty))
        })?;
        let val_tydesc_gv = self.module.declare_data_in_func(val_tydesc_id, builder.func);
        let val_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, val_tydesc_gv);

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
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("TensorNew requires runtime imports".into())
        })?;
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
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for TensorNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let tensor_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Tensor TyDesc.
        let tensor_tydesc_id = self.tydesc_emitter.get(&tensor_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for {:?}", tensor_ty))
        })?;
        let tensor_tydesc_gv = self.module.declare_data_in_func(tensor_tydesc_id, builder.func);
        let tensor_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, tensor_tydesc_gv);

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for element type {:?}", elem_ty))
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, elem_tydesc_gv);

        // Get element size from the type.
        let elem_size = crate::types::ir_type_size(&elem_ty);
        let element_count = elements.len() as u32;
        let rank = shape.len() as u32;

        // Allocate a temporary stack buffer for element data.
        let data_buffer_size = (element_count as usize) * (elem_size as usize);
        let data_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            data_buffer_size as u32,
            align_shift(ir_type_align(&elem_ty)),
        ));
        let data_ptr = builder.ins().stack_addr(PTR_TYPE, data_slot, 0);

        // Copy each element into the data buffer.
        for (i, elem) in elements.iter().enumerate() {
            let elem_src_ptr = self.get_operand_ptr(builder, elem)?;
            let elem_dest_offset = (i as u32) * elem_size;
            let elem_dest_ptr = builder.ins().stack_addr(PTR_TYPE, data_slot, elem_dest_offset as i32);

            // Use memcpy to copy the element.
            // For small fixed sizes, could use load/store, but memcpy is simpler.
            let size_val = builder.ins().iconst(PTR_TYPE, elem_size as i64);
            builder.call_memcpy(
                self.isa.frontend_config(),
                elem_dest_ptr,
                elem_src_ptr,
                size_val,
            );
        }

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
        use crate::index_types::INDEX_TYPE;

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
        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("TableNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("TableNew requires runtime handle".into())
        })?;

        // Get table type from dest.
        let table_ty = self.func.value_types[dest.0 as usize].clone();
        let columns = match &table_ty {
            IrType::Table(cols) => cols.clone(),
            _ => return Err(CraneliftError::Codegen(format!(
                "TableNew dest has non-table type: {:?}", table_ty
            ))),
        };

        // Build row tuple type from column types.
        let row_ty = IrType::Tuple(columns.iter().map(|(_, t)| (**t).clone()).collect());

        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for TableNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let table_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Table TyDesc.
        let table_tydesc_id = self.tydesc_emitter.get(&table_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for {:?}", table_ty))
        })?;
        let table_tydesc_gv = self.module.declare_data_in_func(table_tydesc_id, builder.func);
        let table_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, table_tydesc_gv);

        // Get row (tuple) TyDesc.
        let row_tydesc_id = self.tydesc_emitter.get(&row_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for row type {:?}", row_ty))
        })?;
        let row_tydesc_gv = self.module.declare_data_in_func(row_tydesc_id, builder.func);
        let row_tydesc_ptr = builder.ins().symbol_value(PTR_TYPE, row_tydesc_gv);

        // Create empty table.
        let create_ref = self.module.declare_func_in_func(runtime.table_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, table_ptr, table_tydesc_ptr]);

        // Push each row.
        let push_ref = self.module.declare_func_in_func(runtime.table_push_row, builder.func);
        for row in rows {
            let row_ptr = self.get_operand_ptr(builder, row)?;
            builder.ins().call(push_ref, &[
                rt_handle, table_ptr, table_tydesc_ptr, row_ptr, row_tydesc_ptr
            ]);
        }

        // Store pointer for this value.
        self.values.insert(dest, table_ptr);
        Ok(())
    }
}
