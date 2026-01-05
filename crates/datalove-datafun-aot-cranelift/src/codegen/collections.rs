//! Collection type instruction compilation (List, Set, Map).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::PTR_TYPE;
use crate::AotError;

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
    ) -> Result<(), AotError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("ListNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("ListNew requires runtime handle".into())
        })?;

        // Get list type from dest.
        let list_ty = self.func.value_types[dest.0 as usize].clone();
        let elem_ty = match &list_ty {
            IrType::List(elem) => elem.as_ref().clone(),
            _ => return Err(AotError::Codegen(format!(
                "ListNew dest has non-list type: {:?}", list_ty
            ))),
        };

        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for ListNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let list_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get List TyDesc.
        let list_tydesc_id = self.tydesc_emitter.get(&list_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for {:?}", list_ty))
        })?;
        let list_tydesc_gv = self.module.declare_data_in_func(list_tydesc_id, builder.func);
        let list_tydesc_ptr = builder.ins().global_value(PTR_TYPE, list_tydesc_gv);

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for element type {:?}", elem_ty))
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().global_value(PTR_TYPE, elem_tydesc_gv);

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
    ) -> Result<(), AotError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("SetNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("SetNew requires runtime handle".into())
        })?;

        // Get set type from dest.
        let set_ty = self.func.value_types[dest.0 as usize].clone();
        let elem_ty = match &set_ty {
            IrType::Set(elem) => elem.as_ref().clone(),
            _ => return Err(AotError::Codegen(format!(
                "SetNew dest has non-set type: {:?}", set_ty
            ))),
        };

        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for SetNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let set_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Set TyDesc.
        let set_tydesc_id = self.tydesc_emitter.get(&set_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for {:?}", set_ty))
        })?;
        let set_tydesc_gv = self.module.declare_data_in_func(set_tydesc_id, builder.func);
        let set_tydesc_ptr = builder.ins().global_value(PTR_TYPE, set_tydesc_gv);

        // Get element TyDesc.
        let elem_tydesc_id = self.tydesc_emitter.get(&elem_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for element type {:?}", elem_ty))
        })?;
        let elem_tydesc_gv = self.module.declare_data_in_func(elem_tydesc_id, builder.func);
        let elem_tydesc_ptr = builder.ins().global_value(PTR_TYPE, elem_tydesc_gv);

        // Create empty set.
        let create_ref = self.module.declare_func_in_func(runtime.set_create, builder.func);
        builder.ins().call(create_ref, &[rt_handle, set_ptr, set_tydesc_ptr]);

        // Insert each element.
        // btreeset_insert_local needs a bool_out pointer for the result.
        // Allocate a temp stack slot for this.
        let bool_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            1,
            0,
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
    ) -> Result<(), AotError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("MapNew requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("MapNew requires runtime handle".into())
        })?;

        // Get map type from dest.
        let map_ty = self.func.value_types[dest.0 as usize].clone();
        let (key_ty, val_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(AotError::Codegen(format!(
                "MapNew dest has non-map type: {:?}", map_ty
            ))),
        };

        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for MapNew".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let map_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Map TyDesc.
        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().global_value(PTR_TYPE, map_tydesc_gv);

        // Get key TyDesc.
        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        // Get value TyDesc.
        let val_tydesc_id = self.tydesc_emitter.get(&val_ty).ok_or_else(|| {
            AotError::Codegen(format!("TyDesc not found for value type {:?}", val_ty))
        })?;
        let val_tydesc_gv = self.module.declare_data_in_func(val_tydesc_id, builder.func);
        let val_tydesc_ptr = builder.ins().global_value(PTR_TYPE, val_tydesc_gv);

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
}
