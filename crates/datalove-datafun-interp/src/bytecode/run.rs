//! Running ops: the loop, the routines it calls, and the IR walker for what
//! the bytecode left to it.

use super::*;
use datalove_rtdt::{AnyOut, AnyRef};

// =============================================================================
// Execution
// =============================================================================

#[inline(always)]
unsafe fn rd<T: Copy>(base: *mut u8, loc: Loc) -> T {
    unsafe { std::ptr::read_unaligned(loc.at(base) as *const T) }
}

#[inline(always)]
unsafe fn wr<T>(base: *mut u8, loc: Loc, v: T) {
    unsafe { std::ptr::write_unaligned(loc.at(base) as *mut T, v) }
}

/// Checked division, as the IR walker's: by zero, or the least signed value
/// by minus one, sets the flag and gives zero.
#[inline(always)]
unsafe fn div_checked<T: Copy + CheckedIntOps>(base: *mut u8, dst: Loc, ovf: Loc, a: Loc, b: Loc) {
    unsafe {
        let (r, o) = rd::<T>(base, a).checked_div_impl(rd::<T>(base, b));
        wr(base, dst, r);
        wr(base, ovf, o);
    }
}

/// An operand an op hands to an IR walker routine.
#[inline(always)]
unsafe fn rt_value(frame: &Frame, base: *mut u8, (loc, desc): (Loc, Desc)) -> crate::value::Value {
    let tydesc = match desc {
        Desc::Static(tydesc) => tydesc,
        Desc::Table(n) => frame.table_entry(n as usize),
    };
    crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc }
}

#[inline(always)]
unsafe fn rt_dest(frame: &Frame, base: *mut u8, operand: (Loc, Desc)) -> Destination {
    let val = unsafe { rt_value(frame, base, operand) };
    Destination { ptr: val.ptr, tydesc: val.tydesc }
}

/// A checked update of `place` with `b`, written only if it doesn't
/// overflow. Gives whether it did.
#[inline(always)]
unsafe fn ck_assign(base: *mut u8, ty: CkTy, kind: Ck, place: Loc, b: Loc) -> bool {
    unsafe {
        match ty {
            CkTy::U32 => ck_assign_as::<u32>(base, kind, place, rd(base, b)),
            CkTy::I32 => ck_assign_as::<i32>(base, kind, place, rd(base, b)),
            CkTy::U64 => ck_assign_as::<u64>(base, kind, place, rd(base, b)),
            CkTy::I64 => ck_assign_as::<i64>(base, kind, place, rd(base, b)),
        }
    }
}

#[inline(always)]
unsafe fn ck_assign_as<T: Copy + CheckedIntOps>(base: *mut u8, kind: Ck, place: Loc, b: T) -> bool {
    unsafe {
        let a = rd::<T>(base, place);
        let (r, o) = match kind {
            Ck::Add => a.overflowing_add_impl(b),
            Ck::Sub => a.overflowing_sub_impl(b),
            Ck::Mul => a.overflowing_mul_impl(b),
        };
        if !o {
            wr(base, place, r);
        }
        o
    }
}

#[inline(always)]
unsafe fn checked<T: Copy + CheckedIntOps>(
    base: *mut u8,
    dst: Loc,
    ovf: Loc,
    a: Loc,
    b: Loc,
    f: fn(T, T) -> (T, bool),
) {
    unsafe {
        let (r, o) = f(rd::<T>(base, a), rd::<T>(base, b));
        wr(base, dst, r);
        wr(base, ovf, o);
    }
}

impl IrInterpreter {
    /// Run a lowered function body in a frame `Frame::enter` has made ready.
    ///
    /// Calls the fast path makes to other bytecode bodies run here too, without
    /// recursing: the call pushes the callee's frame, saves the caller's
    /// registers -- the body, `pc`, the frame, where its result goes -- in an
    /// `Activation`, and carries on in the callee; its return pops the frame
    /// and restores them. So a datalove call costs no Rust stack, and
    /// recursion that stays in bytecode is bounded by the frame stack. Calls
    /// the general path makes, and an instruction run on the IR walker that
    /// calls, still nest, starting a loop of their own.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_bytecode<'r>(
        &mut self,
        bc: &'r BcFunction,
        func: &'r IrCodeUnit,
        frame: &mut Frame,
        ret_dest: Destination,
        ctx: &ExecutionContext<'r>,
        registry: &'r FunctionRegistry,
        frames: &mut FrameStore,
        code_ref: Option<&'r CodeRef>,
    ) -> Result<(), InterpError> {
        let mut regs = Regs {
            bc,
            bc_keep: None,
            func,
            code_ref,
            ctx: *ctx,
            frame: *frame,
            ret_dest,
            stack: Vec::new(),
            registry,
            frames,
        };
        // SAFETY: the entry is an op of the body.
        let result = unsafe { self.run_body(&mut regs, bc.entry as usize) };
        if result.is_err() {
            // Every frame the loop pushed is popped before the error goes.
            self.unwind(&mut regs);
        }
        result
    }

    /// The loop: run from `pc` in the body `regs` says, through every call
    /// it makes to another and back, until the body the loop was entered with
    /// returns or something fails.
    ///
    /// The ops, the frame's base and `pc` are locals, which the compiler keeps
    /// in machine registers, and everything else `regs`, behind a pointer;
    /// a call or return reloads the three from it.
    #[inline(never)]
    unsafe fn run_body<'r>(&mut self, regs: &mut Regs<'r>, mut pc: usize) -> Result<(), InterpError> {
        // SAFETY: kept alive by `regs.bc_keep`, or by whoever entered the loop.
        let mut bc: &BcFunction = unsafe { &*regs.bc };
        let mut ops = bc.ops.as_ptr();
        let mut base = regs.frame.base_ptr();

        macro_rules! tri {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                }
            };
        }
        // A return goes back to the caller this loop saved, or out of the loop
        // if there is none.
        macro_rules! ret {
            () => {
                match self.leave(regs) {
                    Some(resume) => {
                        bc = &*regs.bc;
                        ops = bc.ops.as_ptr();
                        base = regs.frame.base_ptr();
                        pc = resume;
                        continue;
                    }
                    None => return Ok(()),
                }
            };
        }

        // SAFETY, for every frame access below: the ops were lowered against
        // the running frame's layout, so every offset is inside it, and `base`
        // stays the frame's data while it runs -- an instruction run on the IR
        // walker uses the same frame, and frames never move.
        unsafe {
            loop {
                // Every block ends in a jump or a return, and every target is
                // an op of this body, so `pc` never runs off the end.
                debug_assert!(pc < bc.ops.len());
                match *ops.add(pc) {
                    Op::Ir { block, index } => tri!(self.run_ir_op(regs, block, index)),
                    Op::Call { block, index } => tri!(self.run_general_call(regs, block, index)),
                    Op::CallFast { site } => {
                        let call = &bc.calls[site as usize];
                        match self.valid_plan(call) {
                            Some(Planned::Body(plan)) => {
                                pc = tri!(self.enter_planned(regs, call, plan, base, pc.wrapping_add(1)));
                                bc = &*regs.bc;
                                ops = bc.ops.as_ptr();
                                base = regs.frame.base_ptr();
                                continue;
                            }
                            // Outside the generation it was found in, the
                            // function may be another's, which the slow path
                            // finds again.
                            Some(Planned::Native(plan)) if plan.generation == self.native_table.generation() => {
                                self.call_planned_native(regs, call, plan, base);
                                pc = pc.wrapping_add(1);
                                continue;
                            }
                            Some(Planned::Jit(plan)) => {
                                tri!(self.call_planned_jit(regs, call, plan, base));
                                pc = pc.wrapping_add(1);
                                continue;
                            }
                            Some(Planned::Offer(weight)) => {
                                self.call_weight = weight;
                                let result = self.run_general_call(regs, call.block, call.index);
                                self.call_weight = 1;
                                tri!(result);
                                pc = pc.wrapping_add(1);
                                continue;
                            }
                            _ => {}
                        }
                        if let Some(resume) = tri!(self.run_fast_call(regs, call, base, pc)) {
                            pc = resume;
                            bc = &*regs.bc;
                            ops = bc.ops.as_ptr();
                            base = regs.frame.base_ptr();
                            continue;
                        }
                    }
                    Op::Const1 { dst, imm } => wr(base, dst, imm),
                    Op::Const4 { dst, imm } => wr(base, dst, imm),
                    Op::ConstPool { dst, at, len } => {
                        copy_bytes(bc.pool.as_ptr().add(at as usize), dst.at(base), len as usize)
                    }
                    Op::Copy1 { dst, src } => wr(base, dst, rd::<u8>(base, src)),
                    Op::Copy4 { dst, src } => wr(base, dst, rd::<u32>(base, src)),
                    Op::Copy8 { dst, src } => wr(base, dst, rd::<u64>(base, src)),
                    Op::CopyN { dst, src, len } => {
                        std::ptr::copy_nonoverlapping(src.at(base), dst.at(base), len as usize)
                    }

                    Op::AddCkU32 { dst, ovf, a, b } => checked::<u32>(base, dst, ovf, a, b, u32::overflowing_add_impl),
                    Op::SubCkU32 { dst, ovf, a, b } => checked::<u32>(base, dst, ovf, a, b, u32::overflowing_sub_impl),
                    Op::MulCkU32 { dst, ovf, a, b } => checked::<u32>(base, dst, ovf, a, b, u32::overflowing_mul_impl),
                    Op::AddCkI32 { dst, ovf, a, b } => checked::<i32>(base, dst, ovf, a, b, i32::overflowing_add_impl),
                    Op::SubCkI32 { dst, ovf, a, b } => checked::<i32>(base, dst, ovf, a, b, i32::overflowing_sub_impl),
                    Op::MulCkI32 { dst, ovf, a, b } => checked::<i32>(base, dst, ovf, a, b, i32::overflowing_mul_impl),
                    Op::AddCkU64 { dst, ovf, a, b } => checked::<u64>(base, dst, ovf, a, b, u64::overflowing_add_impl),
                    Op::SubCkU64 { dst, ovf, a, b } => checked::<u64>(base, dst, ovf, a, b, u64::overflowing_sub_impl),
                    Op::MulCkU64 { dst, ovf, a, b } => checked::<u64>(base, dst, ovf, a, b, u64::overflowing_mul_impl),
                    Op::AddCkI64 { dst, ovf, a, b } => checked::<i64>(base, dst, ovf, a, b, i64::overflowing_add_impl),
                    Op::SubCkI64 { dst, ovf, a, b } => checked::<i64>(base, dst, ovf, a, b, i64::overflowing_sub_impl),
                    Op::MulCkI64 { dst, ovf, a, b } => checked::<i64>(base, dst, ovf, a, b, i64::overflowing_mul_impl),

                    Op::CmpU8 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<u8>(base, a), rd::<u8>(base, b))),
                    Op::CmpI8 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<i8>(base, a), rd::<i8>(base, b))),
                    Op::CmpU32 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<u32>(base, a), rd::<u32>(base, b))),
                    Op::CmpU32I { cmp, dst, a, imm } => wr(base, dst, cmp.apply(rd::<u32>(base, a), imm)),
                    Op::AddCkU32I { dst, ovf, a, imm } => {
                        let (r, o) = rd::<u32>(base, a).overflowing_add(imm);
                        wr(base, dst, r);
                        wr(base, ovf, o);
                    }
                    Op::SubCkU32I { dst, ovf, a, imm } => {
                        let (r, o) = rd::<u32>(base, a).overflowing_sub(imm);
                        wr(base, dst, r);
                        wr(base, ovf, o);
                    }
                    Op::MulCkU32I { dst, ovf, a, imm } => {
                        let (r, o) = rd::<u32>(base, a).overflowing_mul(imm);
                        wr(base, dst, r);
                        wr(base, ovf, o);
                    }
                    Op::CmpI32 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<i32>(base, a), rd::<i32>(base, b))),
                    Op::CmpU64 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<u64>(base, a), rd::<u64>(base, b))),
                    Op::CmpI64 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<i64>(base, a), rd::<i64>(base, b))),

                    Op::AddWrapU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_add(rd(base, b))),
                    Op::SubWrapU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_sub(rd(base, b))),
                    Op::MulWrapU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_mul(rd(base, b))),
                    Op::RemU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a) % rd::<u32>(base, b)),
                    Op::ShrU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_shr(rd(base, b))),
                    Op::ShlU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_shl(rd(base, b))),
                    Op::AndU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a) & rd::<u32>(base, b)),
                    Op::AddWrapIndex { dst, a, b } => {
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, a).wrapping_add(rd(base, b)))
                    }
                    Op::SubWrapIndex { dst, a, b } => {
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, a).wrapping_sub(rd(base, b)))
                    }
                    Op::MulWrapIndex { dst, a, b } => {
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, a).wrapping_mul(rd(base, b)))
                    }
                    Op::ShrIndex { dst, a, b } => {
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, a).wrapping_shr(rd::<u32>(base, b)))
                    }
                    Op::U64ToIndex { dst, src } => wr(base, dst, rd::<u64>(base, src) as rtdt::IndexRepr),
                    Op::IndexToU64 { dst, src } => wr(base, dst, rd::<rtdt::IndexRepr>(base, src) as u64),
                    Op::U64ToU32 { dst, src } => wr(base, dst, rd::<u64>(base, src) as u32),
                    Op::ZextU8U32 { dst, src } => wr(base, dst, rd::<u8>(base, src) as u32),
                    Op::ZextU8U64 { dst, src } => wr(base, dst, rd::<u8>(base, src) as u64),
                    Op::ZextU32U64 { dst, src } => wr(base, dst, rd::<u32>(base, src) as u64),
                    Op::NotBool { dst, src } => wr(base, dst, rd::<u8>(base, src) == 0),
                    Op::XorU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a) ^ rd::<u32>(base, b)),
                    Op::OrU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a) | rd::<u32>(base, b)),
                    Op::CmpF32 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<f32>(base, a), rd::<f32>(base, b))),
                    Op::CmpF64 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<f64>(base, a), rd::<f64>(base, b))),
                    Op::AddF64 { dst, a, b } => wr(base, dst, rd::<f64>(base, a) + rd::<f64>(base, b)),
                    Op::SubF64 { dst, a, b } => wr(base, dst, rd::<f64>(base, a) - rd::<f64>(base, b)),
                    Op::MulF64 { dst, a, b } => wr(base, dst, rd::<f64>(base, a) * rd::<f64>(base, b)),
                    Op::DivF64 { dst, a, b } => wr(base, dst, rd::<f64>(base, a) / rd::<f64>(base, b)),
                    Op::AddF32 { dst, a, b } => wr(base, dst, rd::<f32>(base, a) + rd::<f32>(base, b)),
                    Op::SubF32 { dst, a, b } => wr(base, dst, rd::<f32>(base, a) - rd::<f32>(base, b)),
                    Op::MulF32 { dst, a, b } => wr(base, dst, rd::<f32>(base, a) * rd::<f32>(base, b)),
                    Op::DivF32 { dst, a, b } => wr(base, dst, rd::<f32>(base, a) / rd::<f32>(base, b)),
                    Op::NegF64 { dst, src } => wr(base, dst, -rd::<f64>(base, src)),
                    Op::NegF32 { dst, src } => wr(base, dst, -rd::<f32>(base, src)),
                    Op::SqrtF64 { dst, src } => wr(base, dst, rd::<f64>(base, src).sqrt()),
                    Op::SqrtF32 { dst, src } => wr(base, dst, rd::<f32>(base, src).sqrt()),
                    Op::U64ToF64 { dst, src } => wr(base, dst, rd::<u64>(base, src) as f64),
                    Op::I64ToF64 { dst, src } => wr(base, dst, rd::<i64>(base, src) as f64),
                    Op::F32ToF64 { dst, src } => wr(base, dst, rd::<f32>(base, src) as f64),
                    Op::F64ToF32 { dst, src } => wr(base, dst, rd::<f64>(base, src) as f32),

                    Op::ListBoundsCheck { dst, list, index } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, index) < list.size.0);
                    }
                    Op::ListCopyAt { dst, list, index, size, offset, len } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        let i = rd::<rtdt::IndexRepr>(base, index) as usize;
                        let at = (list.data as *const u8).add(i * size as usize + offset as usize);
                        copy_bytes(at, dst.at(base), len as usize);
                    }
                    Op::ListStoreAt { list, index, src, size, offset, len } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        let i = rd::<rtdt::IndexRepr>(base, index) as usize;
                        let at = (list.data as *mut u8).add(i * size as usize + offset as usize);
                        copy_bytes(src.at(base), at, len as usize);
                    }
                    Op::ListGetCopy { dst, valid, list, index, size } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        let i = rd::<rtdt::IndexRepr>(base, index);
                        let in_bounds = i < list.size.0;
                        if in_bounds {
                            let elem = (list.data as *const u8).add(i as usize * size as usize);
                            copy_bytes(elem, dst.at(base), size as usize);
                        }
                        wr(base, valid, in_bounds);
                    }
                    Op::ListSetCopy { valid, list, index, value, size } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        let i = rd::<rtdt::IndexRepr>(base, index);
                        let in_bounds = i < list.size.0;
                        if in_bounds {
                            let elem = (list.data as *mut u8).add(i as usize * size as usize);
                            copy_bytes(value.at(base), elem, size as usize);
                        }
                        wr(base, valid, in_bounds);
                    }
                    Op::CursorDone { dst, cursor, kind } => {
                        wr(base, dst, crate::collections::cursor_done(kind, cursor.at(base)));
                    }
                    Op::CursorElem { dst, cursor, kind } => {
                        wr(base, dst, crate::collections::cursor_elem(kind, cursor.at(base)));
                    }
                    Op::CursorIndex { dst, cursor, kind } => {
                        wr(base, dst, crate::collections::cursor_index(kind, cursor.at(base)));
                    }
                    Op::CursorValue { dst, cursor } => {
                        wr(base, dst, crate::collections::cursor_value(cursor.at(base)));
                    }
                    Op::CursorAdvance { cursor, kind } => {
                        let (rt, at) = (self.runtime.handle(), cursor.at(base));
                        let status = match kind {
                            CursorKind::Stride => datalove_rt::c::dtlv_rti_stride_cursor_advance_local(rt, at as *mut rtdt::StrideCursor),
                            CursorKind::Row => datalove_rt::c::dtlv_rti_row_cursor_advance_local(rt, at as *mut rtdt::RowCursor),
                            CursorKind::Tree => datalove_rt::c::dtlv_rti_tree_cursor_advance_local(rt, at as *mut rtdt::TreeCursor),
                        };
                        debug_assert_eq!(status, datalove_rt::c::RtStatus::Ok, "advancing a cursor cannot fail");
                    }
                    Op::ListElementRef { dst, list, index, size } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        let i = rd::<rtdt::IndexRepr>(base, index) as usize;
                        wr(base, dst, (list.data as *mut u8).add(i * size as usize));
                    }
                    Op::CopyAt { dst, src, offset, len } => {
                        copy_bytes(src.at(base).add(offset as usize), dst.at(base), len as usize);
                    }
                    Op::FieldRef { dst, src, offset } => wr(base, dst, src.at(base).add(offset as usize)),
                    Op::StaticRef { dst, slot } => {
                        let slot = &bc.statics[slot as usize];
                        let mut ptr = slot.resolved.get();
                        if ptr.is_null() {
                            ptr = self.static_const_with_tydesc(&slot.value, slot.pointee);
                            slot.resolved.set(ptr);
                        }
                        wr(base, dst, ptr);
                    }
                    Op::DivCkU32 { dst, ovf, a, b } => div_checked::<u32>(base, dst, ovf, a, b),
                    Op::DivCkI32 { dst, ovf, a, b } => div_checked::<i32>(base, dst, ovf, a, b),
                    Op::DivCkU64 { dst, ovf, a, b } => div_checked::<u64>(base, dst, ovf, a, b),
                    Op::DivCkI64 { dst, ovf, a, b } => div_checked::<i64>(base, dst, ovf, a, b),

                    Op::WrapSome { dst, src, at, len } => {
                        let d = dst.at(base);
                        *d = rtdt::OptionTag::Some as u8;
                        copy_bytes(src.at(base), d.add(at as usize), len as usize);
                    }
                    Op::WrapNone { dst } => *dst.at(base) = rtdt::OptionTag::None as u8,
                    Op::WrapOk { dst, src, at, len } => {
                        let d = dst.at(base);
                        *d = rtdt::ResultTag::Ok as u8;
                        copy_bytes(src.at(base), d.add(at as usize), len as usize);
                    }
                    Op::UnwrapOption { dst, flag, src, at, len } => {
                        let s = src.at(base);
                        let some = *s != rtdt::OptionTag::None as u8;
                        wr(base, flag, some);
                        if some {
                            copy_bytes(s.add(at as usize), dst.at(base), len as usize);
                        }
                    }
                    Op::UnwrapResult { ok, err, flag, src, at, ok_len, err_len } => {
                        let s = src.at(base);
                        let is_ok = *s == rtdt::ResultTag::Ok as u8;
                        wr(base, flag, is_ok);
                        if is_ok {
                            copy_bytes(s.add(at as usize), ok.at(base), ok_len as usize);
                        } else {
                            copy_bytes(s.add(at as usize), err.at(base), err_len as usize);
                        }
                    }

                    Op::Drop { src, desc } => {
                        let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[desc as usize] };
                        self.execute_drop(&val);
                    }
                    Op::DropTracked { src, track, desc } => {
                        let byte = base.add(track as usize);
                        if *byte == tracking::LIVE {
                            let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[desc as usize] };
                            self.execute_drop(&val);
                            *byte = tracking::MOVED;
                        }
                    }
                    Op::StoreTracked { dst, track, src, len } => {
                        copy_bytes(src.at(base), dst.at(base), len as usize);
                        *base.add(track as usize) = tracking::LIVE;
                    }
                    Op::LoadMoveTracked { dst, src, track, len } => {
                        copy_bytes(src.at(base), dst.at(base), len as usize);
                        *base.add(track as usize) = tracking::MOVED;
                    }
                    Op::ConstString { .. } | Op::ListElementRefRt { .. }
                    | Op::CloneRt { .. } | Op::WidenFixedRt { .. }
                    | Op::Widen { .. } | Op::BinOpRt { .. } | Op::OpAssignRt { .. } | Op::MapGetRt { .. }
                    | Op::WrapSomeRt { .. } | Op::WrapNoneRt { .. } | Op::WrapOkRt { .. } | Op::WrapErrRt { .. }
                    | Op::UnwrapOptionRt { .. } | Op::UnwrapResultRt { .. } | Op::ErrorFromRt { .. } => {
                        self.run_routine_op(*ops.add(pc), bc, &mut regs.frame, base)
                    }
                    Op::CkAssign { ty, kind, place, b, ovf } => wr(base, ovf, ck_assign(base, ty, kind, place, b)),
                    Op::CkAssignU32I { kind, place, imm, ovf } => {
                        wr(base, ovf, ck_assign_as::<u32>(base, kind, place, imm))
                    }
                    Op::CkAssignBr { ty, kind, place, b, ovf, ok } => {
                        pc = if ck_assign(base, ty, kind, place, b) { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::LoopHead { at } => {
                        let counter = &bc.loops[at as usize];
                        if counter.countdown.tick() && tri!(self.loop_hot(regs, counter, base)) {
                            // Compiled code ran the rest of the call.
                            ret!();
                        }
                    }
                    Op::CkAssignU32IBr { kind, place, imm, ovf, ok } => {
                        pc = if ck_assign_as::<u32>(base, kind, place, imm) { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::Jump { to } => {
                        pc = to as usize;
                        continue;
                    }
                    Op::BrIf { cond, then, els } => {
                        pc = if rd::<u8>(base, cond) != 0 { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpU8 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<u8>(base, a), rd::<u8>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpI8 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<i8>(base, a), rd::<i8>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpU32I { cmp, a, imm, then, els } => {
                        pc = if cmp.apply(rd::<u32>(base, a), imm) { then } else { els } as usize;
                        continue;
                    }
                    Op::CkU32Br { kind, dst, a, b, ovf, ok } => {
                        let (r, o) = kind.apply(rd::<u32>(base, a), rd::<u32>(base, b));
                        wr(base, dst, r);
                        pc = if o { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::CkU32IBr { kind, dst, a, imm, ovf, ok } => {
                        let (r, o) = kind.apply(rd::<u32>(base, a), imm);
                        wr(base, dst, r);
                        pc = if o { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::UnwrapOkBr { ok, err, src, at, ok_len, then } => {
                        let s = src.at(base).add(at as usize);
                        if *src.at(base) == rtdt::ResultTag::Ok as u8 {
                            copy_bytes(s, ok.at(base), ok_len as usize);
                            pc = then as usize;
                            continue;
                        }
                        copy_bytes(s, err.at(base), std::mem::size_of::<rtdt::Error>());
                    }
                    Op::BrCmpU32 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<u32>(base, a), rd::<u32>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpI32 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<i32>(base, a), rd::<i32>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpU64 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<u64>(base, a), rd::<u64>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpI64 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<i64>(base, a), rd::<i64>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrListBounds { list, index, then, els } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        pc = if rd::<rtdt::IndexRepr>(base, index) < list.size.0 { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpF32 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<f32>(base, a), rd::<f32>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpF64 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<f64>(base, a), rd::<f64>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::EdgeIr { block, edge, to } => {
                        tri!(self.run_edge(regs, block, edge));
                        pc = to as usize;
                        continue;
                    }
                    Op::Switch { disc, table } => {
                        let value = rd::<u32>(base, disc);
                        let table = &bc.switches[table as usize];
                        pc = table.cases.iter()
                            .find(|(v, _)| *v == value)
                            .map_or(table.default, |(_, to)| *to) as usize;
                        continue;
                    }
                    Op::Return { src, len } => {
                        copy_bytes(src.at(base), regs.ret_dest.ptr, len as usize);
                        ret!();
                    }
                    Op::ReturnOk { src, at, len } => {
                        let d = regs.ret_dest.ptr;
                        *d = rtdt::ResultTag::Ok as u8;
                        copy_bytes(src.at(base), d.add(at as usize), len as usize);
                        ret!();
                    }
                    Op::CopyDyn { dst, src, desc } => {
                        let tydesc: *const rtdt::TyDesc = rd(base, desc);
                        copy_bytes(rd(base, src), rd(base, dst), (*tydesc).size as usize);
                    }
                    Op::ReturnDyn { src, desc } => {
                        let tydesc: *const rtdt::TyDesc = rd(base, desc);
                        copy_bytes(rd(base, src), regs.ret_dest.ptr, (*tydesc).size as usize);
                        ret!();
                    }
                    Op::ReturnUnit => ret!(),
                    Op::ReturnIr { block } => {
                        self.run_return_ir(regs, block);
                        ret!();
                    }
                }
                // Wrapping, so that it is not checked: release builds check
                // overflow, and `pc` is an op index, which cannot overflow.
                // The check and its panic path cost the loop about an eighth
                // of its instructions.
                pc = pc.wrapping_add(1);
            }
        }
    }
}

impl IrInterpreter {
    // The loop's rarer ops, out of it, so that the loop's own code is what
    // the common ops need: every value live across a call in an arm is one
    // the loop has to keep somewhere, and with these inline it kept its
    // registers on the stack.

    /// Run an instruction on the IR walker.
    #[inline(never)]
    fn run_ir_op(&mut self, regs: &mut Regs<'_>, block: u32, index: u32) -> Result<(), InterpError> {
        let instr = &regs.func.blocks[block as usize].instructions[index as usize];
        if self.bc_stats.counting {
            self.bc_stats.count(|| variant(instr));
        }
        let frames = &mut *regs.frames;
        if !self.execute_hot(instr, &mut regs.frame, frames)
            && !self.execute_warm(instr, &mut regs.frame, frames)
        {
            self.execute_instruction(instr, &mut regs.frame, &regs.ctx, regs.registry, frames)?;
        }
        Ok(())
    }

    /// Make the call that is instruction `index` of block `block` by the
    /// general path.
    #[inline(never)]
    fn run_general_call(&mut self, regs: &mut Regs<'_>, block: u32, index: u32) -> Result<(), InterpError> {
        if self.bc_stats.counting {
            self.bc_stats.count(|| "(general call)".into());
        }
        let (func_ref, args, bindings, dest) =
            match &regs.func.blocks[block as usize].instructions[index as usize] {
                Instruction::Call { dest, func: f, args, bindings, .. }
                | Instruction::ComptimeCall { dest, func: f, args, bindings, .. } => {
                    (f, args, bindings, *dest)
                }
                i => unreachable!("Call op on {:?}", i),
            };
        let frames = &mut *regs.frames;
        self.execute_call(func_ref, args, bindings, dest, &mut regs.frame, &regs.ctx, regs.registry, frames)
    }

    /// Make a fast call without a plan: a native, a forwarder, a bytecode
    /// body the loop goes on in -- then where it starts -- or the general
    /// path.
    #[inline(never)]
    unsafe fn run_fast_call<'r>(
        &mut self,
        regs: &mut Regs<'r>,
        call: &'r FastCall,
        base: *mut u8,
        pc: usize,
    ) -> Result<Option<usize>, InterpError> {
        let frames = &mut *regs.frames;
        // SAFETY: lowered against the running frame, which `base` is.
        match unsafe { self.fast_call(call, base, &mut regs.frame, &regs.ctx, regs.registry, frames, regs.func) }? {
            Called::Done => Ok(None),
            Called::Enter(callee) => Ok(Some(self.switch_to(regs, callee, pc.wrapping_add(1)))),
            Called::General => {
                if self.bc_stats.counting {
                    self.bc_stats.count(|| "(fast call fell back)".into());
                }
                let Instruction::Call { .. } = &regs.func.blocks[call.block as usize].instructions[call.index as usize] else {
                    unreachable!("a fast call is a call")
                };
                self.run_general_call(regs, call.block, call.index).map(|()| None)
            }
        }
    }

    /// Pass edge `edge` of block `block`'s terminator's arguments on the IR
    /// walker.
    #[inline(never)]
    fn run_edge(&mut self, regs: &mut Regs<'_>, block: u32, edge: u32) -> Result<(), InterpError> {
        if self.bc_stats.counting {
            self.bc_stats.count(|| "(edge)".into());
        }
        let (target, args) = match &regs.func.blocks[block as usize].terminator {
            Terminator::Goto { target, args } => (*target, args),
            Terminator::Branch { then_block, then_args, .. } if edge == 0 => (*then_block, then_args),
            Terminator::Branch { else_block, else_args, .. } => (*else_block, else_args),
            t => unreachable!("an edge from {:?}", t),
        };
        let frames = &mut *regs.frames;
        self.pass_block_args(&regs.func.blocks, target, args, &mut regs.frame, frames)
    }

    /// Return block `block`'s value on the IR walker.
    #[inline(never)]
    fn run_return_ir(&mut self, regs: &mut Regs<'_>, block: u32) {
        if self.bc_stats.counting {
            self.bc_stats.count(|| "(return)".into());
        }
        let Terminator::Return { value: Some(op) } = &regs.func.blocks[block as usize].terminator else {
            unreachable!("ReturnIr on a block that does not return a value")
        };
        let frames = &mut *regs.frames;
        let val = self.read_operand(op, &regs.frame, frames);
        // SAFETY: the value read is the operand the IR returns, and the
        // destination the caller gave for it.
        unsafe { self.move_value(&val, regs.ret_dest) };
        Self::mark_source_dropped_all(op, &mut regs.frame, frames);
    }
}

impl IrInterpreter {
    /// Run an op that hands its operands to an IR walker routine.
    ///
    /// Out of the loop, since the routines do the work and the arms for
    /// them made the loop's own code worse: primes, which runs none of
    /// them, was 7% slower with them inline.
    #[inline(never)]
    unsafe fn run_routine_op(&mut self, op: Op, bc: &BcFunction, frame: &mut Frame, base: *mut u8) {
        unsafe {
            match op {
                Op::ConstString { dst, at, len, desc } => {
                    let bytes = if len == 0 { std::ptr::null() } else { bc.pool.as_ptr().add(at as usize) };
                    datalove_rt::c::dtlv_rti_string_from_bytes(
                        self.runtime.handle(), bytes, len as rtdt::IndexRepr, AnyOut::new(dst.at(base)), &*(bc.descs[desc as usize]));
                }
                Op::ListElementRefRt { dest, at } => {
                    let [(dst, _), l, (index, _)] = bc.rt[at as usize];
                    let list_val = rt_value(frame, base, l);
                    let (list, element_tydesc, size) = crate::list_element_info(&list_val);
                    let i = rd::<rtdt::IndexRepr>(base, index) as usize;
                    wr(base, dst, (list.data as *mut u8).add(i * size));
                    frame.check_referent(ValueId(dest), element_tydesc);
                }
                Op::CloneRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    let (src, dest) = (rt_value(frame, base, s), rt_dest(frame, base, d));
                    datalove_rt::c::dtlv_rti_clone_local(
                        self.runtime.handle(), AnyRef::new(src.ptr), &*src.tydesc, AnyOut::new(dest.ptr), &*dest.tydesc);
                }
                Op::WidenFixedRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.widen_fixed(&rt_value(frame, base, s), &rt_dest(frame, base, d));
                }
                Op::Widen { dst, src, src_desc } => {
                    let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[src_desc as usize] };
                    self.widen_to_int(&val, &mut *(dst.at(base) as *mut rtdt::Int));
                }
                Op::MapGetRt { at, valid } => {
                    let [d, m, k] = bc.rt[at as usize];
                    let found = self.map_get(&rt_value(frame, base, m), &rt_value(frame, base, k), rt_dest(frame, base, d));
                    wr(base, valid, found);
                }
                Op::BinOpRt { op, at } => {
                    let [d, a, b] = bc.rt[at as usize];
                    let (lhs, rhs) = (rt_value(frame, base, a), rt_value(frame, base, b));
                    self.execute_binop(op, &lhs, &rhs, rt_dest(frame, base, d));
                }
                Op::OpAssignRt { op, at } => {
                    let [p, r, _] = bc.rt[at as usize];
                    let (place, rhs) = (rt_value(frame, base, p), rt_value(frame, base, r));
                    self.execute_op_assign(op, &place, &rhs);
                }
                Op::WrapSomeRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.execute_wrap_some(&rt_value(frame, base, s), rt_dest(frame, base, d));
                }
                Op::WrapNoneRt { at } => {
                    let [d, _, _] = bc.rt[at as usize];
                    self.execute_wrap_none(rt_dest(frame, base, d));
                }
                Op::WrapOkRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.execute_wrap_ok(&rt_value(frame, base, s), rt_dest(frame, base, d));
                }
                Op::WrapErrRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.execute_wrap_err(&rt_value(frame, base, s), rt_dest(frame, base, d));
                }
                Op::ErrorFromRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.execute_error_from(&rt_value(frame, base, s), rt_dest(frame, base, d));
                }
                Op::UnwrapOptionRt { at, flag } => {
                    let [d, s, _] = bc.rt[at as usize];
                    let flag = Destination { ptr: flag.at(base), tydesc: std::ptr::null() };
                    self.execute_unwrap_option(&rt_value(frame, base, s), rt_dest(frame, base, d), flag);
                }
                Op::UnwrapResultRt { at, flag } => {
                    let [ok, err, s] = bc.rt[at as usize];
                    let flag = Destination { ptr: flag.at(base), tydesc: std::ptr::null() };
                    self.execute_unwrap_result(
                        &rt_value(frame, base, s), rt_dest(frame, base, ok), rt_dest(frame, base, err), flag);
                }
                op => unreachable!("{:?} is not a routine op", op),
            }
        }
    }
}
