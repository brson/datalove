//! What each op touches and may be done with, and printing ops.

use super::*;

impl Op {
    /// What the optimization pass may do with the op.
    ///
    /// Exhaustive, so that a new op cannot be added without saying.
    pub(super) fn shape(&self) -> Shape {
        match self {
            Op::Const1 { .. } | Op::Const4 { .. } | Op::ConstPool { .. } | Op::Copy1 { .. } | Op::Copy4 { .. }
            | Op::Copy8 { .. } | Op::CopyN { .. } | Op::CmpU32I { .. } | Op::CmpU8 { .. } | Op::CmpU32 { .. }
            | Op::CmpI32 { .. } | Op::CmpU64 { .. } | Op::CmpI64 { .. } | Op::AddWrapU32 { .. }
            | Op::SubWrapU32 { .. } | Op::MulWrapU32 { .. } | Op::RemU32 { .. } | Op::ShrU32 { .. }
            | Op::ShlU32 { .. } | Op::AndU32 { .. } | Op::AddWrapIndex { .. } | Op::SubWrapIndex { .. }
            | Op::U64ToIndex { .. } | Op::ZextU8U32 { .. } | Op::ZextU8U64 { .. } | Op::ZextU32U64 { .. }
            | Op::NotBool { .. } | Op::XorU32 { .. } | Op::OrU32 { .. } | Op::CmpF32 { .. }
            | Op::CmpF64 { .. } | Op::AddF64 { .. } | Op::SubF64 { .. } | Op::MulF64 { .. }
            | Op::DivF64 { .. } | Op::AddF32 { .. } | Op::SubF32 { .. } | Op::MulF32 { .. }
            | Op::DivF32 { .. } | Op::NegF64 { .. } | Op::NegF32 { .. } | Op::SqrtF64 { .. }
            | Op::SqrtF32 { .. } | Op::U64ToF64 { .. } | Op::I64ToF64 { .. } | Op::F32ToF64 { .. }
            | Op::F64ToF32 { .. } | Op::ListCopyAt { .. } => Shape::ONE_RESULT,
            Op::BrIf { .. } | Op::BrCmpU8 { .. } | Op::BrCmpU32I { .. } | Op::BrCmpU32 { .. }
            | Op::BrCmpI32 { .. } | Op::BrCmpU64 { .. } | Op::BrCmpI64 { .. } | Op::BrCmpF32 { .. }
            | Op::BrCmpF64 { .. } | Op::BrListBounds { .. } | Op::CkU32Br { .. } | Op::CkU32IBr { .. }
            | Op::CkAssignBr { .. } | Op::CkAssignU32IBr { .. } => Shape::TWO_WAY,
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. } | Op::EdgeIr { .. } | Op::UnwrapOkBr { .. }
            | Op::Switch { .. } | Op::Jump { .. } | Op::Return { .. } | Op::ReturnOk { .. } | Op::ReturnUnit
            | Op::ReturnIr { .. } | Op::CopyDyn { .. } | Op::ReturnDyn { .. } => Shape::FIXED,
            Op::LoopHead { .. } | Op::ConstString { .. } | Op::CopyAt { .. } | Op::FieldRef { .. }
            | Op::StaticRef { .. } | Op::MapGetRt { .. } | Op::AddCkU32 { .. } | Op::SubCkU32 { .. }
            | Op::MulCkU32 { .. } | Op::AddCkI32 { .. } | Op::SubCkI32 { .. } | Op::MulCkI32 { .. }
            | Op::DivCkU32 { .. } | Op::DivCkI32 { .. } | Op::DivCkU64 { .. } | Op::DivCkI64 { .. }
            | Op::AddCkU64 { .. } | Op::SubCkU64 { .. } | Op::MulCkU64 { .. } | Op::AddCkI64 { .. }
            | Op::SubCkI64 { .. } | Op::MulCkI64 { .. } | Op::AddCkU32I { .. } | Op::SubCkU32I { .. }
            | Op::MulCkU32I { .. } | Op::ListBoundsCheck { .. } | Op::ListGetCopy { .. }
            | Op::ListSetCopy { .. } | Op::ListStoreAt { .. } | Op::ListElementRef { .. }
            | Op::ListElementRefRt { .. } | Op::CloneRt { .. }
            | Op::WidenFixedRt { .. } | Op::WrapSome { .. } | Op::WrapNone { .. } | Op::WrapOk { .. }
            | Op::UnwrapOption { .. } | Op::UnwrapResult { .. } | Op::Drop { .. } | Op::DropTracked { .. }
            | Op::StoreTracked { .. } | Op::LoadMoveTracked { .. } | Op::Widen { .. } | Op::BinOpRt { .. }
            | Op::OpAssignRt { .. } | Op::CkAssign { .. } | Op::CkAssignU32I { .. } => Shape::OTHER,
        }
    }

    /// Every place in the frame the op reads or writes, with how many bytes it
    /// reads or writes there.
    ///
    /// Exhaustive, so that a new op cannot be added without saying what it
    /// touches; the verifier checks these against the frame.
    pub(super) fn places(&self, f: &mut impl FnMut(Loc, u32)) {
        const I: u32 = rtdt::INDEX_SIZE;
        match *self {
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. } | Op::Jump { .. } | Op::LoopHead { .. }
            | Op::EdgeIr { .. } | Op::ReturnUnit | Op::ReturnIr { .. }
            | Op::ListElementRefRt { .. }
            | Op::CloneRt { .. } | Op::WidenFixedRt { .. } | Op::BinOpRt { .. }
            | Op::OpAssignRt { .. } => {}
            Op::CkAssign { ty, place, b, ovf, .. } => { f(place, ty.size()); f(b, ty.size()); f(ovf, 1) }
            Op::CkAssignU32I { place, ovf, .. } => { f(place, 4); f(ovf, 1) }
            Op::CopyDyn { dst, src, desc } => { f(dst, 8); f(src, 8); f(desc, 8) }
            Op::ReturnDyn { src, desc } => { f(src, 8); f(desc, 8) }
            Op::CkAssignBr { ty, place, b, .. } => { f(place, ty.size()); f(b, ty.size()) }
            Op::CkAssignU32IBr { place, .. } => f(place, 4),
            Op::CopyAt { dst, src, offset, len } => { f(dst, len); f(src, offset + len) }
            Op::FieldRef { dst, src, offset } => { f(dst, 8); f(src, offset) }
            Op::StaticRef { dst, .. } => f(dst, 8),
            Op::MapGetRt { valid, .. } => f(valid, 1),
            Op::Const1 { dst, .. } => f(dst, 1),
            Op::Const4 { dst, .. } => f(dst, 4),
            Op::ConstPool { dst, len, .. } => f(dst, len),
            Op::ConstString { dst, .. } => f(dst, 1),
            Op::Copy1 { dst, src } => { f(dst, 1); f(src, 1) }
            Op::Copy4 { dst, src } => { f(dst, 4); f(src, 4) }
            Op::Copy8 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::CopyN { dst, src, len } => { f(dst, len); f(src, len) }
            Op::AddCkU32 { dst, ovf, a, b } | Op::SubCkU32 { dst, ovf, a, b } | Op::MulCkU32 { dst, ovf, a, b }
            | Op::AddCkI32 { dst, ovf, a, b } | Op::SubCkI32 { dst, ovf, a, b } | Op::MulCkI32 { dst, ovf, a, b }
            | Op::DivCkU32 { dst, ovf, a, b } | Op::DivCkI32 { dst, ovf, a, b } => {
                f(dst, 4); f(ovf, 1); f(a, 4); f(b, 4)
            }
            Op::AddCkU64 { dst, ovf, a, b } | Op::SubCkU64 { dst, ovf, a, b } | Op::MulCkU64 { dst, ovf, a, b }
            | Op::AddCkI64 { dst, ovf, a, b } | Op::SubCkI64 { dst, ovf, a, b } | Op::MulCkI64 { dst, ovf, a, b }
            | Op::DivCkU64 { dst, ovf, a, b } | Op::DivCkI64 { dst, ovf, a, b } => {
                f(dst, 8); f(ovf, 1); f(a, 8); f(b, 8)
            }
            Op::CmpU32I { dst, a, .. } => { f(dst, 1); f(a, 4) }
            Op::AddCkU32I { dst, ovf, a, .. } | Op::SubCkU32I { dst, ovf, a, .. }
            | Op::MulCkU32I { dst, ovf, a, .. } => { f(dst, 4); f(ovf, 1); f(a, 4) }
            Op::CmpU8 { dst, a, b, .. } => { f(dst, 1); f(a, 1); f(b, 1) }
            Op::CmpU32 { dst, a, b, .. } | Op::CmpI32 { dst, a, b, .. } => { f(dst, 1); f(a, 4); f(b, 4) }
            Op::CmpU64 { dst, a, b, .. } | Op::CmpI64 { dst, a, b, .. } => { f(dst, 1); f(a, 8); f(b, 8) }
            Op::AddWrapU32 { dst, a, b } | Op::SubWrapU32 { dst, a, b } | Op::MulWrapU32 { dst, a, b }
            | Op::RemU32 { dst, a, b } | Op::ShrU32 { dst, a, b } | Op::ShlU32 { dst, a, b }
            | Op::AndU32 { dst, a, b } => { f(dst, 4); f(a, 4); f(b, 4) }
            Op::AddWrapIndex { dst, a, b } | Op::SubWrapIndex { dst, a, b } => { f(dst, I); f(a, I); f(b, I) }
            Op::U64ToIndex { dst, src } => { f(dst, I); f(src, 8) }
            Op::ZextU8U32 { dst, src } => { f(dst, 4); f(src, 1) }
            Op::ZextU8U64 { dst, src } => { f(dst, 8); f(src, 1) }
            Op::ZextU32U64 { dst, src } => { f(dst, 8); f(src, 4) }
            Op::NotBool { dst, src } => { f(dst, 1); f(src, 1) }
            Op::XorU32 { dst, a, b } | Op::OrU32 { dst, a, b } => { f(dst, 4); f(a, 4); f(b, 4) }
            Op::CmpF32 { dst, a, b, .. } => { f(dst, 1); f(a, 4); f(b, 4) }
            Op::CmpF64 { dst, a, b, .. } => { f(dst, 1); f(a, 8); f(b, 8) }
            Op::AddF64 { dst, a, b } | Op::SubF64 { dst, a, b }
            | Op::MulF64 { dst, a, b } | Op::DivF64 { dst, a, b } => { f(dst, 8); f(a, 8); f(b, 8) }
            Op::AddF32 { dst, a, b } | Op::SubF32 { dst, a, b }
            | Op::MulF32 { dst, a, b } | Op::DivF32 { dst, a, b } => { f(dst, 4); f(a, 4); f(b, 4) }
            Op::NegF64 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::NegF32 { dst, src } => { f(dst, 4); f(src, 4) }
            Op::SqrtF64 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::SqrtF32 { dst, src } => { f(dst, 4); f(src, 4) }
            Op::U64ToF64 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::I64ToF64 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::F32ToF64 { dst, src } => { f(dst, 8); f(src, 4) }
            Op::F64ToF32 { dst, src } => { f(dst, 4); f(src, 8) }
            Op::ListBoundsCheck { dst, list, index } => {
                f(dst, 1); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I)
            }
            Op::ListGetCopy { dst, valid, list, index, size } => {
                f(dst, size); f(valid, 1); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I)
            }
            Op::ListCopyAt { dst, list, index, len, .. } => {
                f(dst, len as u32); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I)
            }
            Op::ListStoreAt { list, index, src, len, .. } => {
                f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I); f(src, len as u32)
            }
            Op::ListSetCopy { valid, list, index, value, size } => {
                f(valid, 1); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I); f(value, size)
            }
            Op::ListElementRef { dst, list, index, .. } => {
                f(dst, 8); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I)
            }
            Op::WrapSome { dst, src, at, len } | Op::WrapOk { dst, src, at, len } => { f(dst, at + len); f(src, len) }
            Op::WrapNone { dst } => f(dst, 1),
            Op::UnwrapOption { dst, flag, src, at, len } => { f(dst, len); f(flag, 1); f(src, at + len) }
            Op::UnwrapResult { ok, err, flag, src, at, ok_len, err_len } => {
                f(ok, ok_len as u32); f(err, err_len as u32); f(flag, 1);
                f(src, at as u32 + (ok_len.max(err_len)) as u32)
            }
            Op::UnwrapOkBr { ok, err, src, at, ok_len, .. } => {
                let err_len = std::mem::size_of::<rtdt::Error>() as u32;
                f(ok, ok_len as u32); f(err, err_len); f(src, at as u32 + (ok_len as u32).max(err_len))
            }
            Op::Drop { src, .. } | Op::DropTracked { src, .. } => f(src, 1),
            Op::StoreTracked { dst, src, len, .. } | Op::LoadMoveTracked { dst, src, len, .. } => {
                f(dst, len); f(src, len)
            }
            Op::Widen { dst, src, .. } => { f(dst, std::mem::size_of::<rtdt::Int>() as u32); f(src, 1) }
            Op::BrIf { cond, .. } => f(cond, 1),
            Op::BrCmpU8 { a, b, .. } => { f(a, 1); f(b, 1) }
            Op::BrCmpU32I { a, .. } => f(a, 4),
            Op::CkU32Br { dst, a, b, .. } => { f(dst, 4); f(a, 4); f(b, 4) }
            Op::CkU32IBr { dst, a, .. } => { f(dst, 4); f(a, 4) }
            Op::BrCmpU32 { a, b, .. } | Op::BrCmpI32 { a, b, .. } => { f(a, 4); f(b, 4) }
            Op::BrCmpU64 { a, b, .. } | Op::BrCmpI64 { a, b, .. } => { f(a, 8); f(b, 8) }
            Op::BrListBounds { list, index, .. } => { f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I) }
            Op::BrCmpF32 { a, b, .. } => { f(a, 4); f(b, 4) }
            Op::BrCmpF64 { a, b, .. } => { f(a, 8); f(b, 8) }
            Op::Switch { disc, .. } => f(disc, 4),
            Op::Return { src, len } => f(src, len),
            Op::ReturnOk { src, len, .. } => f(src, len),
        }
    }

    /// Every op the op may go to next, other than the one after it.
    ///
    /// Exhaustive, so that a new op that jumps cannot be left out of the
    /// fixups, the compaction or the verifier, which all go through this.
    pub(super) fn targets_mut(&mut self, f: &mut impl FnMut(&mut u32)) {
        match self {
            Op::Jump { to } | Op::EdgeIr { to, .. } => f(to),
            Op::UnwrapOkBr { then, .. } => f(then),
            Op::BrIf { then, els, .. } | Op::BrCmpU8 { then, els, .. } | Op::BrCmpU32I { then, els, .. }
            | Op::BrCmpU32 { then, els, .. } | Op::BrCmpI32 { then, els, .. }
            | Op::BrCmpU64 { then, els, .. } | Op::BrCmpI64 { then, els, .. }
            | Op::BrCmpF32 { then, els, .. } | Op::BrCmpF64 { then, els, .. }
            | Op::BrListBounds { then, els, .. } => { f(then); f(els) }
            Op::CkU32Br { ovf, ok, .. } | Op::CkU32IBr { ovf, ok, .. }
            | Op::CkAssignBr { ovf, ok, .. } | Op::CkAssignU32IBr { ovf, ok, .. } => { f(ovf); f(ok) }
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. }
            | Op::Const1 { .. } | Op::Const4 { .. } | Op::ConstPool { .. } | Op::ConstString { .. }
            | Op::Copy1 { .. } | Op::Copy4 { .. } | Op::Copy8 { .. } | Op::CopyN { .. }
            | Op::CopyAt { .. } | Op::FieldRef { .. } | Op::StaticRef { .. } | Op::MapGetRt { .. }
            | Op::DivCkU32 { .. } | Op::DivCkI32 { .. } | Op::DivCkU64 { .. } | Op::DivCkI64 { .. }
            | Op::AddCkU32 { .. } | Op::SubCkU32 { .. } | Op::MulCkU32 { .. }
            | Op::AddCkI32 { .. } | Op::SubCkI32 { .. } | Op::MulCkI32 { .. }
            | Op::AddCkU64 { .. } | Op::SubCkU64 { .. } | Op::MulCkU64 { .. }
            | Op::AddCkI64 { .. } | Op::SubCkI64 { .. } | Op::MulCkI64 { .. }
            | Op::CmpU32I { .. } | Op::AddCkU32I { .. } | Op::SubCkU32I { .. } | Op::MulCkU32I { .. }
            | Op::CmpU8 { .. } | Op::CmpU32 { .. } | Op::CmpI32 { .. } | Op::CmpU64 { .. } | Op::CmpI64 { .. }
            | Op::AddWrapU32 { .. } | Op::SubWrapU32 { .. } | Op::MulWrapU32 { .. } | Op::RemU32 { .. }
            | Op::ShrU32 { .. } | Op::ShlU32 { .. } | Op::AndU32 { .. }
            | Op::AddWrapIndex { .. } | Op::SubWrapIndex { .. } | Op::U64ToIndex { .. }
            | Op::ZextU8U32 { .. } | Op::ZextU8U64 { .. } | Op::ZextU32U64 { .. } | Op::NotBool { .. }
            | Op::XorU32 { .. } | Op::OrU32 { .. } | Op::CmpF32 { .. } | Op::CmpF64 { .. } | Op::AddF64 { .. } | Op::SubF64 { .. } | Op::MulF64 { .. } | Op::DivF64 { .. } | Op::AddF32 { .. } | Op::SubF32 { .. } | Op::MulF32 { .. } | Op::DivF32 { .. } | Op::NegF64 { .. } | Op::NegF32 { .. } | Op::SqrtF64 { .. } | Op::SqrtF32 { .. } | Op::U64ToF64 { .. } | Op::I64ToF64 { .. } | Op::F32ToF64 { .. } | Op::F64ToF32 { .. }
            | Op::ListBoundsCheck { .. } | Op::ListGetCopy { .. } | Op::ListSetCopy { .. }
            | Op::ListCopyAt { .. } | Op::ListStoreAt { .. }
            | Op::ListElementRef { .. } | Op::ListElementRefRt { .. }
            | Op::CloneRt { .. } | Op::WidenFixedRt { .. }
            | Op::WrapSome { .. } | Op::WrapNone { .. } | Op::WrapOk { .. }
            | Op::UnwrapOption { .. } | Op::UnwrapResult { .. }
            | Op::Drop { .. } | Op::DropTracked { .. } | Op::StoreTracked { .. } | Op::LoadMoveTracked { .. }
            | Op::Widen { .. } | Op::BinOpRt { .. } | Op::OpAssignRt { .. }
            | Op::CkAssign { .. } | Op::CkAssignU32I { .. } | Op::Switch { .. }
            | Op::Return { .. } | Op::ReturnOk { .. } | Op::ReturnUnit | Op::ReturnIr { .. }
            | Op::CopyDyn { .. } | Op::ReturnDyn { .. }
            | Op::LoopHead { .. } => {}
        }
    }
}

impl BcFunction {
    /// The ops, one per line, for reading.
    pub(crate) fn dump(&self, func: &IrCodeUnit) -> String {
        self.ops.iter().enumerate().map(|(i, op)| match op {
            Op::Ir { block, index } | Op::Call { block, index } => format!(
                "  {i:3}: {op:?} {:?}\n", func.blocks[*block as usize].instructions[*index as usize]),
            Op::CallFast { site } => {
                let call = &self.calls[*site as usize];
                format!("  {i:3}: {op:?} {:?}\n", func.blocks[call.block as usize].instructions[call.index as usize])
            }
            _ => format!("  {i:3}: {op:?}\n"),
        }).collect()
    }
}
