//! Slot instruction compilation (SlotStore, SlotLoad).

use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{Operand, SlotDest, SlotId, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a SlotStore instruction.
    pub(super) fn compile_slot_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &SlotDest,
        value: &Operand,
    ) -> Result<(), AotError> {
        let slot_id = match dest {
            SlotDest::Local(id) => *id,
            SlotDest::External { unit, slot } => {
                return Err(AotError::Unsupported(format!(
                    "external slot store (unit={}, slot={:?}) not yet implemented",
                    unit, slot
                )));
            }
        };

        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for slot store".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot_id.0);
        let slot_ty = &self.func.slot_types[slot_id.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                builder.ins().store(MemFlags::new(), val, addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copy bytes from source to slot.
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);

                // Use memcpy for aggregates.
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_ptr, size);
            }
        }

        Ok(())
    }

    /// Compile a SlotLoad instruction.
    pub(super) fn compile_slot_load(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        slot: SlotId,
    ) -> Result<(), AotError> {
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for slot load".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot.0);
        let slot_ty = &self.func.slot_types[slot.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Scalar: load value directly.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                let val = builder.ins().load(cl_ty, MemFlags::new(), addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                // Aggregate: return pointer to slot location.
                // The value stays in place, we just track the pointer.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                self.values.insert(dest, addr);
            }
        }

        Ok(())
    }
}
