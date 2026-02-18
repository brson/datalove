//! Map indexing instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, BlockArg, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a MapContainsKey instruction.
    ///
    /// Calls the runtime contains_key function and stores the boolean result.
    pub(super) fn compile_map_contains_key(
        &mut self,
        builder: &mut FunctionBuilder,
        is_valid: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CraneliftError> {
        let map_ty = self.get_operand_type(map)?;
        let (key_ty, _value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "MapContainsKey on non-map type: {:?}", map_ty
            ))),
        };

        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapContainsKey requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapContainsKey requires runtime handle".into())
        })?;

        let map_ptr = self.get_operand_ptr(builder, map)?;
        let key_ptr = self.get_operand_ptr(builder, key)?;

        // Get map tydesc.
        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for map type {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().global_value(PTR_TYPE, map_tydesc_gv);

        // Get key tydesc.
        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        // Allocate stack slot for bool result.
        let result_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            1,
            0,
        ));
        let result_addr = builder.ins().stack_addr(PTR_TYPE, result_slot, 0);

        let func_ref = self.module.declare_func_in_func(runtime.map_contains_key, builder.func);
        builder.ins().call(func_ref, &[
            rt_handle,
            map_ptr,
            map_tydesc_ptr,
            key_ptr,
            key_tydesc_ptr,
            result_addr,
        ]);

        // Load the bool result.
        let result_val = builder.ins().load(cl_types::I8, MemFlags::new(), result_addr, 0);
        let is_valid_val = builder.ins().icmp_imm(
            cl_ir::condcodes::IntCC::NotEqual,
            result_val,
            0,
        );
        self.values.insert(is_valid, is_valid_val);
        Ok(())
    }

    /// Compile a MapGet instruction.
    ///
    /// Calls contains_key, then conditionally calls get_value_ref + clone.
    pub(super) fn compile_map_get(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_valid: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CraneliftError> {
        let map_ty = self.get_operand_type(map)?;
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "MapGet on non-map type: {:?}", map_ty
            ))),
        };
        let value_repr = types::ir_type_to_cranelift(&value_ty);

        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapGet requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapGet requires runtime handle".into())
        })?;

        let map_ptr = self.get_operand_ptr(builder, map)?;
        let key_ptr = self.get_operand_ptr(builder, key)?;

        // Get tydescs.
        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for map type {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().global_value(PTR_TYPE, map_tydesc_gv);

        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        let value_tydesc_id = self.tydesc_emitter.get(&value_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for value type {:?}", value_ty))
        })?;
        let value_tydesc_gv = self.module.declare_data_in_func(value_tydesc_id, builder.func);
        let value_tydesc_ptr = builder.ins().global_value(PTR_TYPE, value_tydesc_gv);

        // Check if key exists.
        let result_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            1,
            0,
        ));
        let result_addr = builder.ins().stack_addr(PTR_TYPE, result_slot, 0);

        let contains_ref = self.module.declare_func_in_func(runtime.map_contains_key, builder.func);
        builder.ins().call(contains_ref, &[
            rt_handle,
            map_ptr,
            map_tydesc_ptr,
            key_ptr,
            key_tydesc_ptr,
            result_addr,
        ]);

        let result_val = builder.ins().load(cl_types::I8, MemFlags::new(), result_addr, 0);
        let is_valid_val = builder.ins().icmp_imm(
            cl_ir::condcodes::IntCC::NotEqual,
            result_val,
            0,
        );
        self.values.insert(is_valid, is_valid_val);

        let load_block = builder.create_block();
        let skip_block = builder.create_block();
        let merge_block = builder.create_block();

        match &value_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                let cl_ty = *cl_ty;
                builder.append_block_param(merge_block, cl_ty);

                builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                // Load block: get value ref, load scalar value.
                builder.switch_to_block(load_block);
                builder.seal_block(load_block);

                // Get pointer to value.
                let vref_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                    cl_ir::StackSlotKind::ExplicitSlot,
                    std::mem::size_of::<*mut u8>() as u32,
                    0,
                ));
                let vref_addr = builder.ins().stack_addr(PTR_TYPE, vref_slot, 0);

                let getref_ref = self.module.declare_func_in_func(runtime.map_get_value_ref, builder.func);
                builder.ins().call(getref_ref, &[
                    rt_handle,
                    map_ptr,
                    map_tydesc_ptr,
                    key_ptr,
                    key_tydesc_ptr,
                    vref_addr,
                ]);

                let value_ptr = builder.ins().load(PTR_TYPE, MemFlags::new(), vref_addr, 0);
                let val = builder.ins().load(cl_ty, MemFlags::new(), value_ptr, 0);
                builder.ins().jump(merge_block, &[BlockArg::from(val)]);

                // Skip block: dummy value.
                builder.switch_to_block(skip_block);
                builder.seal_block(skip_block);
                let zero = if cl_ty == cl_types::F32 {
                    builder.ins().f32const(0.0f32)
                } else if cl_ty == cl_types::F64 {
                    builder.ins().f64const(0.0f64)
                } else {
                    builder.ins().iconst(cl_ty, 0)
                };
                builder.ins().jump(merge_block, &[BlockArg::from(zero)]);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
                let phi_val = builder.block_params(merge_block)[0];
                self.values.insert(dest, phi_val);
            }
            CraneliftRepr::Aggregate(_) => {
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for MapGet aggregate dest".into())
                })?;
                let dest_offset = self.layout.value_offset(dest.0);
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                // Load block: get value ref, clone to dest.
                builder.switch_to_block(load_block);
                builder.seal_block(load_block);

                let vref_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
                    cl_ir::StackSlotKind::ExplicitSlot,
                    std::mem::size_of::<*mut u8>() as u32,
                    0,
                ));
                let vref_addr = builder.ins().stack_addr(PTR_TYPE, vref_slot, 0);

                let getref_ref = self.module.declare_func_in_func(runtime.map_get_value_ref, builder.func);
                builder.ins().call(getref_ref, &[
                    rt_handle,
                    map_ptr,
                    map_tydesc_ptr,
                    key_ptr,
                    key_tydesc_ptr,
                    vref_addr,
                ]);

                let value_ptr = builder.ins().load(PTR_TYPE, MemFlags::new(), vref_addr, 0);

                let clone_ref = self.module.declare_func_in_func(runtime.clone_local, builder.func);
                builder.ins().call(clone_ref, &[
                    rt_handle,
                    value_ptr,
                    value_tydesc_ptr,
                    dest_addr,
                    value_tydesc_ptr,
                ]);
                builder.ins().jump(merge_block, &[]);

                // Skip block.
                builder.switch_to_block(skip_block);
                builder.seal_block(skip_block);
                builder.ins().jump(merge_block, &[]);

                builder.switch_to_block(merge_block);
                builder.seal_block(merge_block);
                self.values.insert(dest, dest_addr);
            }
        }

        // Conditional tracking byte.
        if let Some(track_offset) = self.layout.values[dest.0 as usize].tracking_byte {
            use crate::layout::tracking;
            let frame_slot = self.frame_slot.expect("tracking requires frame slot");
            let frame_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, 0);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);
            let track_addr = builder.ins().iadd_imm(frame_addr, track_offset as i64);
            let track_val = builder.ins().select(is_valid_val, live_val, uninit_val);
            builder.ins().store(MemFlags::new(), track_val, track_addr, 0);
        }

        Ok(())
    }

    /// Compile a MapSetValue instruction.
    ///
    /// Key must exist (caller checks). Calls runtime to destroy old value and
    /// store new value.
    pub(super) fn compile_map_set_value(
        &mut self,
        builder: &mut FunctionBuilder,
        map: &Operand,
        key: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let map_ty = self.get_operand_type(map)?;
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "MapSetValue on non-map type: {:?}", map_ty
            ))),
        };

        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapSetValue requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapSetValue requires runtime handle".into())
        })?;

        let map_ptr = self.get_operand_ptr(builder, map)?;
        let key_ptr = self.get_operand_ptr(builder, key)?;
        let value_ptr = self.get_operand_ptr(builder, value)?;

        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for map type {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().global_value(PTR_TYPE, map_tydesc_gv);

        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        let value_tydesc_id = self.tydesc_emitter.get(&value_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for value type {:?}", value_ty))
        })?;
        let value_tydesc_gv = self.module.declare_data_in_func(value_tydesc_id, builder.func);
        let value_tydesc_ptr = builder.ins().global_value(PTR_TYPE, value_tydesc_gv);

        let func_ref = self.module.declare_func_in_func(runtime.map_set_value, builder.func);
        builder.ins().call(func_ref, &[
            rt_handle,
            map_ptr,
            map_tydesc_ptr,
            key_ptr,
            key_tydesc_ptr,
            value_ptr,
            value_tydesc_ptr,
        ]);

        Ok(())
    }

    /// Compile a MapValueRef instruction.
    ///
    /// Key must exist (caller checks). Calls runtime to get pointer to value
    /// in the map's B-tree leaf.
    pub(super) fn compile_map_value_ref(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CraneliftError> {
        let map_ty = self.get_operand_type(map)?;
        let (key_ty, _value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "MapValueRef on non-map type: {:?}", map_ty
            ))),
        };

        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapValueRef requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapValueRef requires runtime handle".into())
        })?;

        let map_ptr = self.get_operand_ptr(builder, map)?;
        let key_ptr = self.get_operand_ptr(builder, key)?;

        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for map type {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().global_value(PTR_TYPE, map_tydesc_gv);

        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        // Allocate stack slot for the value pointer result.
        let vref_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            std::mem::size_of::<*mut u8>() as u32,
            0,
        ));
        let vref_addr = builder.ins().stack_addr(PTR_TYPE, vref_slot, 0);

        let func_ref = self.module.declare_func_in_func(runtime.map_get_value_ref, builder.func);
        builder.ins().call(func_ref, &[
            rt_handle,
            map_ptr,
            map_tydesc_ptr,
            key_ptr,
            key_tydesc_ptr,
            vref_addr,
        ]);

        let value_ptr = builder.ins().load(PTR_TYPE, MemFlags::new(), vref_addr, 0);
        self.values.insert(dest, value_ptr);
        Ok(())
    }

    /// Compile a MapUpsert instruction.
    ///
    /// Insert key-value if absent, overwrite value if present. Calls
    /// `dtlv_rti_btreemap_insert_local` which has full upsert semantics.
    pub(super) fn compile_map_upsert(
        &mut self,
        builder: &mut FunctionBuilder,
        map: &Operand,
        key: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let map_ty = self.get_operand_type(map)?;
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "MapUpsert on non-map type: {:?}", map_ty
            ))),
        };

        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen("MapUpsert requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("MapUpsert requires runtime handle".into())
        })?;

        let map_ptr = self.get_operand_ptr(builder, map)?;
        let key_ptr = self.get_operand_ptr(builder, key)?;
        let value_ptr = self.get_operand_ptr(builder, value)?;

        let map_tydesc_id = self.tydesc_emitter.get(&map_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for map type {:?}", map_ty))
        })?;
        let map_tydesc_gv = self.module.declare_data_in_func(map_tydesc_id, builder.func);
        let map_tydesc_ptr = builder.ins().global_value(PTR_TYPE, map_tydesc_gv);

        let key_tydesc_id = self.tydesc_emitter.get(&key_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for key type {:?}", key_ty))
        })?;
        let key_tydesc_gv = self.module.declare_data_in_func(key_tydesc_id, builder.func);
        let key_tydesc_ptr = builder.ins().global_value(PTR_TYPE, key_tydesc_gv);

        let value_tydesc_id = self.tydesc_emitter.get(&value_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for value type {:?}", value_ty))
        })?;
        let value_tydesc_gv = self.module.declare_data_in_func(value_tydesc_id, builder.func);
        let value_tydesc_ptr = builder.ins().global_value(PTR_TYPE, value_tydesc_gv);

        let func_ref = self.module.declare_func_in_func(runtime.map_insert, builder.func);
        builder.ins().call(func_ref, &[
            rt_handle,
            map_ptr,
            map_tydesc_ptr,
            key_ptr,
            key_tydesc_ptr,
            value_ptr,
            value_tydesc_ptr,
        ]);

        Ok(())
    }
}
