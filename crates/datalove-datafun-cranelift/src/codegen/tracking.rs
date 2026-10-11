//! Tracking bytes: where a tracked value, slot or parameter keeps whether it
//! holds anything, and marking it.

use super::*;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Get the tracking byte offset for an operand, if it's tracked.
    pub(super) fn tracking_byte_offset(&self, operand: &Operand) -> Option<u32> {
        match operand {
            Operand::Value(vid) => self.layout.values[vid.0 as usize].tracking_byte,
            Operand::Slot(sid) => self.layout.slots[sid.0 as usize].tracking_byte,
            Operand::Param(pid) => self.param_tracking_byte_offset(*pid),
            _ => None,
        }
    }

    /// Mark a tracking byte as LIVE.
    pub(super) fn mark_tracking_live(
        &self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) {
        use datalove_datafun_ir::frame_layout::tracking;

        if let Some(track_offset) = self.tracking_byte_offset(operand) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = frame_slot.addr(builder, track_offset as i32);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            builder.ins().store(MemFlagsData::new(), live_val, track_addr, 0);
        }
    }

    /// Mark a tracking byte as MOVED.
    pub(super) fn mark_tracking_moved(
        &self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) {
        use datalove_datafun_ir::frame_layout::tracking;

        if let Some(track_offset) = self.tracking_byte_offset(operand) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = frame_slot.addr(builder, track_offset as i32);
            let moved_val = builder.ins().iconst(cl_types::I8, tracking::MOVED as i64);
            builder.ins().store(MemFlagsData::new(), moved_val, track_addr, 0);
        }
    }

    /// Mark a tracked value as LIVE after initialization.
    pub(super) fn mark_value_live(&self, builder: &mut FunctionBuilder, vid: ValueId) {
        self.mark_tracking_live(builder, &Operand::Value(vid));
    }

    /// Mark a tracked slot as LIVE after store.
    pub(super) fn mark_slot_live(&self, builder: &mut FunctionBuilder, sid: SlotId) {
        self.mark_tracking_live(builder, &Operand::Slot(sid));
    }

    /// Get tracking byte offset for a param, if tracked.
    pub(super) fn param_tracking_byte_offset(&self, pid: ParamId) -> Option<u32> {
        self.layout.param_tracking[pid.0 as usize]
    }

    /// Mark a tracked param as LIVE after store.
    pub(super) fn mark_param_live(&self, builder: &mut FunctionBuilder, pid: ParamId) {
        self.mark_tracking_live(builder, &Operand::Param(pid));
    }

    /// Emit a conditional tracking byte write based on an is_valid flag.
    ///
    /// Writes LIVE if is_valid is true, UNINIT if false.
    pub(super) fn emit_conditional_tracking(
        &self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_valid_val: cl_ir::Value,
    ) {
        if let Some(track_offset) = self.layout.values[dest.0 as usize].tracking_byte {
            use datalove_datafun_ir::frame_layout::tracking;
            let frame_slot = self.frame_slot.expect("tracking requires frame slot");
            let frame_addr = frame_slot.addr(builder, 0);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);
            let track_addr = builder.ins().iadd_imm_s(frame_addr, track_offset as i64);
            let track_val = builder.ins().select(is_valid_val, live_val, uninit_val);
            builder.ins().store(MemFlagsData::new(), track_val, track_addr, 0);
        }
    }
}
