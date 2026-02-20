//! Map indexing instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, BlockArg, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use crate::runtime::RuntimeImports;

use super::FunctionCompiler;

/// Pre-computed values shared across map instruction compilation.
struct MapSetup {
    map_ptr: cl_ir::Value,
    key_ptr: cl_ir::Value,
    map_tydesc_ptr: cl_ir::Value,
    key_tydesc_ptr: cl_ir::Value,
    value_ty: IrType,
    runtime: RuntimeImports,
    rt_handle: cl_ir::Value,
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Prepare common values needed by all map operations.
    ///
    /// Extracts key/value types, runtime imports, operand pointers, and tydesc
    /// global values. Each `compile_map_*` method calls this then uses the result.
    fn prepare_map_op(
        &mut self,
        builder: &mut FunctionBuilder,
        op_name: &str,
        map: &Operand,
        key: &Operand,
    ) -> Result<MapSetup, CraneliftError> {
        let map_ty = self.get_operand_type(map)?;
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CraneliftError::Codegen(format!(
                "{} on non-map type: {:?}", op_name, map_ty
            ))),
        };

        let runtime = self.runtime.ok_or_else(|| {
            CraneliftError::Codegen(format!("{} requires runtime imports", op_name))
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen(format!("{} requires runtime handle", op_name))
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

        Ok(MapSetup {
            map_ptr,
            key_ptr,
            map_tydesc_ptr,
            key_tydesc_ptr,
            value_ty,
            runtime,
            rt_handle,
        })
    }

    /// Resolve a value tydesc global value from an IrType.
    fn resolve_tydesc_ptr(
        &mut self,
        builder: &mut FunctionBuilder,
        ty: &IrType,
    ) -> Result<cl_ir::Value, CraneliftError> {
        let tydesc_id = self.tydesc_emitter.get(ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for type {:?}", ty))
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        Ok(builder.ins().global_value(PTR_TYPE, tydesc_gv))
    }

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
        let s = self.prepare_map_op(builder, "MapContainsKey", map, key)?;

        // Allocate stack slot for bool result.
        let result_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            1,
            0,
        ));
        let result_addr = builder.ins().stack_addr(PTR_TYPE, result_slot, 0);

        let func_ref = self.module.declare_func_in_func(s.runtime.map_contains_key, builder.func);
        builder.ins().call(func_ref, &[
            s.rt_handle,
            s.map_ptr,
            s.map_tydesc_ptr,
            s.key_ptr,
            s.key_tydesc_ptr,
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
    /// Calls get_value_ref unconditionally, derives is_valid from null check.
    pub(super) fn compile_map_get(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_valid: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CraneliftError> {
        let s = self.prepare_map_op(builder, "MapGet", map, key)?;
        let value_repr = types::ir_type_to_cranelift(&s.value_ty);
        let value_tydesc_ptr = self.resolve_tydesc_ptr(builder, &s.value_ty)?;

        // Get pointer to value (returns null on miss).
        let vref_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            std::mem::size_of::<*mut u8>() as u32,
            0,
        ));
        let vref_addr = builder.ins().stack_addr(PTR_TYPE, vref_slot, 0);

        let getref_ref = self.module.declare_func_in_func(s.runtime.map_get_value_ref, builder.func);
        builder.ins().call(getref_ref, &[
            s.rt_handle,
            s.map_ptr,
            s.map_tydesc_ptr,
            s.key_ptr,
            s.key_tydesc_ptr,
            vref_addr,
        ]);

        let value_ptr = builder.ins().load(PTR_TYPE, MemFlags::new(), vref_addr, 0);
        let null_ptr = builder.ins().iconst(PTR_TYPE, 0);
        let is_valid_val = builder.ins().icmp(
            cl_ir::condcodes::IntCC::NotEqual,
            value_ptr,
            null_ptr,
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

                // Load block: load scalar value from already-retrieved pointer.
                builder.switch_to_block(load_block);
                builder.seal_block(load_block);

                let val = builder.ins().load(cl_ty, MemFlags::new(), value_ptr, 0);
                builder.ins().jump(merge_block, &[BlockArg::from(val)]);

                // Skip block: dummy value.
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
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for MapGet aggregate dest".into())
                })?;
                let dest_offset = self.layout.value_offset(dest.0);
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                builder.ins().brif(is_valid_val, load_block, &[], skip_block, &[]);

                // Load block: clone from already-retrieved pointer to dest.
                builder.switch_to_block(load_block);
                builder.seal_block(load_block);

                let clone_ref = self.module.declare_func_in_func(s.runtime.clone_local, builder.func);
                builder.ins().call(clone_ref, &[
                    s.rt_handle,
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
        self.emit_conditional_tracking(builder, dest, is_valid_val);

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
        let s = self.prepare_map_op(builder, "MapSetValue", map, key)?;
        let value_ptr = self.get_operand_ptr(builder, value)?;
        let value_tydesc_ptr = self.resolve_tydesc_ptr(builder, &s.value_ty)?;

        let func_ref = self.module.declare_func_in_func(s.runtime.map_set_value, builder.func);
        builder.ins().call(func_ref, &[
            s.rt_handle,
            s.map_ptr,
            s.map_tydesc_ptr,
            s.key_ptr,
            s.key_tydesc_ptr,
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
        let s = self.prepare_map_op(builder, "MapValueRef", map, key)?;

        // Allocate stack slot for the value pointer result.
        let vref_slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            std::mem::size_of::<*mut u8>() as u32,
            0,
        ));
        let vref_addr = builder.ins().stack_addr(PTR_TYPE, vref_slot, 0);

        let func_ref = self.module.declare_func_in_func(s.runtime.map_get_value_ref, builder.func);
        builder.ins().call(func_ref, &[
            s.rt_handle,
            s.map_ptr,
            s.map_tydesc_ptr,
            s.key_ptr,
            s.key_tydesc_ptr,
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
        let s = self.prepare_map_op(builder, "MapUpsert", map, key)?;
        let value_ptr = self.get_operand_ptr(builder, value)?;
        let value_tydesc_ptr = self.resolve_tydesc_ptr(builder, &s.value_ty)?;

        let func_ref = self.module.declare_func_in_func(s.runtime.map_insert, builder.func);
        builder.ins().call(func_ref, &[
            s.rt_handle,
            s.map_ptr,
            s.map_tydesc_ptr,
            s.key_ptr,
            s.key_tydesc_ptr,
            value_ptr,
            value_tydesc_ptr,
        ]);

        Ok(())
    }
}
