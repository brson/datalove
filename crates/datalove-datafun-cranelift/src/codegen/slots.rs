//! Slot instruction compilation (SlotStore, SlotLoad).

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ParamId, SlotDest, SlotId, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a SlotStore instruction.
    pub(super) fn compile_slot_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &SlotDest,
        value: &Operand,
        is_copy: bool,
    ) -> Result<(), CraneliftError> {
        let slot_id = match dest {
            SlotDest::Local(id) => *id,
            SlotDest::External { unit, slot } => {
                return Err(CraneliftError::Unsupported(format!(
                    "writing slot {:?} of script unit {}, which finished earlier",
                    slot, unit
                )));
            }
        };

        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for slot store".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot_id.0);
        let slot_ty = &self.func.slot_types[slot_id.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        // A dynamic slot is its storage on the shadow stack, and is copied as
        // many bytes as its descriptor says.
        if let Some(&dest_addr) = self.dyn_slot_storage.get(&slot_id) {
            let slot_ty = slot_ty.clone();
            let src_ptr = self.get_operand_ptr(builder, value)?;
            let size = self.value_size(builder, &slot_ty)?;
            builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_ptr, size);
            if !is_copy {
                let moved = self.moved_size(builder, &slot_ty)?;
                let zero = builder.ins().iconst(cranelift_codegen::ir::types::I8, 0);
                builder.call_memset(self.isa.frontend_config(), src_ptr, zero, moved);
            }
            return Ok(());
        }

        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                let addr = frame_slot.addr(builder, slot_offset as i32);
                builder.ins().store(MemFlagsData::new(), val, addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copy bytes from source to destination.
                // The old value (if any) is destroyed by explicit drop.tracked in the IR
                // before this store instruction.
                let dest_addr = frame_slot.addr(builder, slot_offset as i32);
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_ptr, size);

                if !is_copy {
                    // Zero out the source to complete the move (prevents double-free).
                    let zero = builder.ins().iconst(cranelift_codegen::ir::types::I8, 0);
                    builder.call_memset(self.isa.frontend_config(), src_ptr, zero, size);
                }
            }
        }

        Ok(())
    }

    /// Compile a SlotLoad instruction.
    ///
    /// For aggregates, this returns a pointer to the slot. The `is_copy` parameter
    /// is available for future optimization but not currently used here - move
    /// semantics for aggregates are handled when the value is consumed elsewhere.
    pub(super) fn compile_slot_load(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        slot: SlotId,
        _is_copy: bool,
    ) -> Result<(), CraneliftError> {
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for slot load".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot.0);
        let slot_ty = &self.func.slot_types[slot.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        // Copied out of a dynamic slot's storage into the value's, for the
        // reason the aggregate case below gives.
        if let Some(&src) = self.dyn_slot_storage.get(&slot) {
            let slot_ty = slot_ty.clone();
            let dst = self.dest_addr(builder, dest)?;
            self.copy_value_bytes(builder, dst, src, &slot_ty)?;
            self.values.insert(dest, dst);
            return Ok(());
        }

        match repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Scalar: load value directly.
                let addr = frame_slot.addr(builder, slot_offset as i32);
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copied into the destination's own room rather
                // than pointed at where it lies.
                //
                // Pointing at the slot would be cheaper and is wrong: a load
                // takes the value out, and what is taken has to survive the
                // slot being written again. A loop that reads a slot, stores
                // a fresh value into it and only then drops what it took --
                // which is what `set x = some elem` after reading `x` lowers
                // to -- would otherwise drop the fresh value and leave the
                // old one, freeing one thing twice. The other backends copy
                // here for the same reason.
                let src = frame_slot.addr(builder, slot_offset as i32);
                let dest_offset = self.layout.value_offset(dest.0);
                let dst = frame_slot.addr(builder, dest_offset as i32);
                if layout.size > 0 {
                    let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                    builder.call_memcpy(self.isa.frontend_config(), dst, src, size);
                }
                self.values.insert(dest, dst);
            }
        }

        Ok(())
    }

    /// Compile a ParamStore instruction.
    ///
    /// Stores a value to a mut/out param, in the caller's memory. The old
    /// value, if any, was destroyed by an earlier drop.
    pub(super) fn compile_param_store(
        &mut self,
        builder: &mut FunctionBuilder,
        param: ParamId,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let param_ptr = self.param_values.get(&param).copied().ok_or_else(|| {
            CraneliftError::Codegen(format!("undefined param: {:?}", param))
        })?;
        let param_ty = self.func_ctx.param_types[param.0 as usize].clone();
        self.store_at(builder, param_ptr, &param_ty, value)
    }

    /// Compile a ParamStoreTracked instruction.
    ///
    /// A ParamStore that also marks the param live.
    pub(super) fn compile_param_store_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        param: ParamId,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        self.compile_param_store(builder, param, value)?;
        self.mark_param_live(builder, param);
        Ok(())
    }

    /// Compile a RefStore instruction: store a value where a reference operand
    /// points.
    pub(super) fn compile_ref_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let dest_ptr = self.get_operand_ptr(builder, dest)?;
        let dest_ty = self.get_operand_type(dest)?;
        self.store_at(builder, dest_ptr, &dest_ty, value)
    }

    /// Compile a RefSetField instruction: store a value to a field of what a
    /// reference operand points at.
    pub(super) fn compile_ref_set_field(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        let (addr, ty) = self.static_field_place(builder, dest, field_path)?;
        self.store_at(builder, addr, &ty, value)
    }

    /// Compile a RefStoreTracked instruction: a RefStore that marks the place
    /// live.
    pub(super) fn compile_ref_store_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        self.compile_ref_store(builder, dest, value)?;
        self.mark_tracking_live(builder, dest);
        Ok(())
    }

    /// Compile a RefSetFieldTracked instruction: a RefSetField that marks the
    /// place live.
    pub(super) fn compile_ref_set_field_tracked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &Operand,
        field_path: &[u32],
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        self.compile_ref_set_field(builder, dest, field_path, value)?;
        self.mark_tracking_live(builder, dest);
        Ok(())
    }

    /// Store `value`, of type `ty`, at `addr`: a scalar by a store, an
    /// aggregate by copying its bytes from where the operand is.
    fn store_at(
        &mut self,
        builder: &mut FunctionBuilder,
        addr: cl_ir::Value,
        ty: &IrType,
        value: &Operand,
    ) -> Result<(), CraneliftError> {
        match types::ir_type_to_cranelift(ty) {
            CraneliftRepr::Scalar(_) => {
                let val = self.get_operand_value(builder, value)?;
                builder.ins().store(MemFlagsData::new(), val, addr, 0);
            }
            CraneliftRepr::Aggregate(_) => {
                let src_ptr = self.get_operand_ptr(builder, value)?;
                self.copy_value_bytes(builder, addr, src_ptr, ty)?;
            }
        }
        Ok(())
    }

    /// The address and type of the field `field_path` names within what the
    /// reference operand `base` points at, by its static type.
    fn static_field_place(
        &mut self,
        builder: &mut FunctionBuilder,
        base: &Operand,
        field_path: &[u32],
    ) -> Result<(cl_ir::Value, IrType), CraneliftError> {
        let mut addr = self.get_operand_ptr(builder, base)?;
        let mut ty = self.get_operand_type(base)?;
        for &field_idx in field_path {
            let field_types: Vec<_> = match &ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => return Err(CraneliftError::Codegen(format!(
                    "field path through non-aggregate type: {:?}", ty))),
            };
            if field_idx as usize >= field_types.len() {
                return Err(CraneliftError::Codegen(format!("field index {} out of bounds", field_idx)));
            }
            addr = self.field_addr(builder, addr, &ty, field_idx)?;
            ty = field_types[field_idx as usize].clone();
        }
        Ok((addr, ty))
    }
}
