//! Cursors: a `for` loop's walk over a collection.
//!
//! Every operation calls the runtime's protocol, as the interpreter does.
//! Open-coding the strided cursors and the trees' field reads is an
//! optimization that has to keep the cursor's layout and meaning, since OSR
//! continues a walk the interpreter began.

use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};
use datalove_datafun_ir::{CursorKind, IrType, Operand, SlotId, ValueId};
use crate::CraneliftError;
use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Which cursor a slot holds.
    fn cursor_kind(&self, cursor: SlotId) -> CursorKind {
        match &self.func.slot_types[cursor.0 as usize] {
            IrType::Cursor(kind) => *kind,
            other => panic!("a cursor instruction on a slot of {other}"),
        }
    }

    fn rt_handle(&self, op_name: &str) -> Result<cranelift_codegen::ir::Value, CraneliftError> {
        self.rt_handle_param.ok_or_else(|| CraneliftError::Codegen(format!("{op_name} requires runtime handle")))
    }

    /// Call a function of the protocol that takes the handle, the cursor and
    /// the collection's descriptor: begin, or a tree's advance.
    fn call_cursor_with_coll(
        &mut self,
        builder: &mut FunctionBuilder,
        func: FuncId,
        cursor: SlotId,
        coll: &Operand,
        coll_first: bool,
        op_name: &str,
    ) -> Result<(), CraneliftError> {
        let coll_ty = self.get_operand_type(coll)?;
        let rt = self.rt_handle(op_name)?;
        let cursor_ptr = self.get_operand_ptr(builder, &Operand::Slot(cursor))?;
        let coll_td = self.tydesc_value(builder, &coll_ty)?;
        let func_ref = self.module.declare_func_in_func(func, builder.func);
        if coll_first {
            let coll_ptr = self.get_operand_ptr(builder, coll)?;
            builder.ins().call(func_ref, &[rt, coll_ptr, coll_td, cursor_ptr]);
        } else {
            builder.ins().call(func_ref, &[rt, cursor_ptr, coll_td]);
        }
        Ok(())
    }

    pub(super) fn compile_cursor_begin(
        &mut self,
        builder: &mut FunctionBuilder,
        cursor: SlotId,
        coll: &Operand,
    ) -> Result<(), CraneliftError> {
        let coll_ty = self.get_operand_type(coll)?;
        let func = match (self.cursor_kind(cursor), &coll_ty) {
            (CursorKind::Stride, IrType::List(_)) => self.runtime.list_cursor_begin,
            (CursorKind::Stride, IrType::Tensor(..)) => self.runtime.tensor_cursor_begin,
            (CursorKind::Row, IrType::Tensor(..)) => self.runtime.tensor_row_cursor_begin,
            (CursorKind::Tree, IrType::Set(_)) => self.runtime.set_cursor_begin,
            (CursorKind::Tree, IrType::Map(..)) => self.runtime.map_cursor_begin,
            (kind, ty) => panic!("a {kind:?} cursor over a {ty}"),
        };
        self.call_cursor_with_coll(builder, func, cursor, coll, true, "CursorBegin")
    }

    pub(super) fn compile_cursor_advance(
        &mut self,
        builder: &mut FunctionBuilder,
        cursor: SlotId,
        coll: &Operand,
    ) -> Result<(), CraneliftError> {
        let func = match self.cursor_kind(cursor) {
            CursorKind::Stride => self.runtime.stride_cursor_advance,
            CursorKind::Row => self.runtime.row_cursor_advance,
            CursorKind::Tree => {
                let func = match self.get_operand_type(coll)? {
                    IrType::Set(_) => self.runtime.set_cursor_advance,
                    IrType::Map(..) => self.runtime.map_cursor_advance,
                    ty => panic!("a tree cursor over a {ty}"),
                };
                return self.call_cursor_with_coll(builder, func, cursor, coll, false, "CursorAdvance");
            }
        };
        let cursor_ptr = self.get_operand_ptr(builder, &Operand::Slot(cursor))?;
        let func_ref = self.module.declare_func_in_func(func, builder.func);
        builder.ins().call(func_ref, &[cursor_ptr]);
        Ok(())
    }

    /// Done, elem and value: a call on the cursor alone, whose one result is
    /// `dest`.
    pub(super) fn compile_cursor_read(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        cursor: SlotId,
        func: FuncId,
    ) -> Result<(), CraneliftError> {
        let cursor_ptr = self.get_operand_ptr(builder, &Operand::Slot(cursor))?;
        let func_ref = self.module.declare_func_in_func(func, builder.func);
        let call = builder.ins().call(func_ref, &[cursor_ptr]);
        let result = builder.inst_results(call)[0];
        self.values.insert(dest, result);
        Ok(())
    }

    pub(super) fn cursor_done_func(&self, cursor: SlotId) -> FuncId {
        match self.cursor_kind(cursor) {
            CursorKind::Stride => self.runtime.stride_cursor_done,
            CursorKind::Row => self.runtime.row_cursor_done,
            CursorKind::Tree => self.runtime.tree_cursor_done,
        }
    }

    pub(super) fn cursor_elem_func(&self, cursor: SlotId) -> FuncId {
        match self.cursor_kind(cursor) {
            CursorKind::Stride => self.runtime.stride_cursor_elem,
            CursorKind::Row => self.runtime.row_cursor_elem,
            CursorKind::Tree => self.runtime.tree_cursor_elem,
        }
    }
}
