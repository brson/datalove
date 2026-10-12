//! The debug-build check of everything the run loop takes on trust.

use super::*;

impl BcFunction {
    /// Check, in a debug build, everything the loop takes on trust: that every
    /// place an op names is inside the frame, every jump lands on an op, every
    /// index names an entry of its table, and every fused unwrap has a block
    /// after it to fall through into.
    ///
    /// The loop reads and writes the frame through raw offsets, and this is
    /// what those offsets rest on.
    pub(super) fn verify(&self, func: &IrCodeUnit, layout: &IrLayout) {
        let size = layout.frame_size;
        let n = self.ops.len() as u32;
        let place = |at: usize, loc: Loc, len: u32| {
            let offset = loc.0 & !INDIRECT;
            // An indirect place is read through the pointer the frame holds;
            // what it points at is the caller's or a reference's.
            let len = if loc.0 & INDIRECT != 0 { 8 } else { len };
            assert!(offset + len <= size,
                "{}: op {} ({:?}) names {} bytes at {} in a frame of {}",
                func.name, at, self.ops[at], len, offset, size);
        };
        let target = |at: usize, to: u32| {
            assert!(to < n, "{}: op {} ({:?}) goes to {}, past the last op", func.name, at, self.ops[at], to);
        };
        assert!(self.entry < n, "{}: the entry {} is past the last op", func.name, self.entry);
        for (at, op) in self.ops.iter().enumerate() {
            op.places(&mut |loc, len| place(at, loc, len));
            op.clone().targets_mut(&mut |to| target(at, *to));
            let in_table = |i: u32, len: usize, what: &str| {
                assert!((i as usize) < len, "{}: op {} ({:?}) names {} {} of {}", func.name, at, op, what, i, len);
            };
            match *op {
                Op::Ir { block, index } | Op::Call { block, index } => {
                    in_table(block, func.blocks.len(), "block");
                    in_table(index, func.blocks[block as usize].instructions.len(), "instruction");
                }
                Op::EdgeIr { block, .. } | Op::ReturnIr { block } => in_table(block, func.blocks.len(), "block"),
                Op::CallFast { site } => in_table(site, self.calls.len(), "call site"),
                Op::LoopHead { at } => in_table(at, self.loops.len(), "loop"),
                Op::StaticRef { slot, .. } => in_table(slot, self.statics.len(), "static"),
                Op::ConstPool { at: pool_at, len, .. } | Op::ConstString { at: pool_at, len, .. } => {
                    assert!(pool_at + len <= self.pool.len() as u32, "{}: op {} reads past the pool", func.name, at);
                }
                Op::ListElementRefRt { at: i, .. }
                | Op::CloneRt { at: i } | Op::WidenFixedRt { at: i } | Op::BinOpRt { at: i, .. }
                | Op::OpAssignRt { at: i, .. } | Op::MapGetRt { at: i, .. } => {
                    in_table(i, self.rt.len(), "routine operands");
                    // Unused entries of a triple are padding, with no descriptor.
                    for (loc, desc) in self.rt[i as usize] {
                        if !matches!(desc, Desc::Static(d) if d.is_null()) {
                            place(at, loc, 1);
                        }
                    }
                }
                Op::Switch { table, .. } => {
                    in_table(table, self.switches.len(), "switch table");
                    let t = &self.switches[table as usize];
                    t.cases.iter().for_each(|(_, to)| target(at, *to));
                    target(at, t.default);
                }
                Op::UnwrapOkBr { .. } => assert!((at as u32) + 1 < n,
                    "{}: op {} unwraps with nothing after it to fall through into", func.name, at),
                _ => {}
            }
            match *op {
                Op::ConstString { desc, .. } | Op::Drop { desc, .. } | Op::DropTracked { desc, .. }
                | Op::Widen { src_desc: desc, .. } => {
                    in_table(desc, self.descs.len(), "descriptor");
                }
                _ => {}
            }
            match *op {
                Op::DropTracked { track, .. } | Op::StoreTracked { track, .. } | Op::LoadMoveTracked { track, .. } => {
                    assert!(track < size, "{}: op {} names tracking byte {} in a frame of {}", func.name, at, track, size);
                }
                _ => {}
            }
        }
        for call in &self.calls {
            place(0, call.dest, 1);
            if let Some(resolved) = &call.resolved {
                for &(loc, _) in resolved {
                    place(0, loc, 1);
                }
            }
        }
    }
}
