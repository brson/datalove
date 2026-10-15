//! Cursors: a `for` loop's walk over a collection.
//!
//! The runtime's `dtlv_rti_*_cursor_*` functions begin and advance a walk, and
//! the interpreter runs exactly them; whether it is done and its current
//! element are the cursor's fields. Here most of the protocol is open-coded on
//! the runtime's own layouts, read off the `rtdt` structs, and keeps the
//! runtime's meanings field for field: OSR continues in compiled code a walk the
//! interpreter began. What stays a call is starting a walk over a tensor or a
//! tree, once a walk, and a tree's advance from one leaf to the next, once
//! every few elements.

use std::mem::offset_of;
use cranelift_codegen::ir::{self as cl_ir, condcodes::IntCC, types as cl_types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};
use datalove_datafun_ir::{CursorKind, IrType, Operand, SlotId, ValueId};
use datalove_rtdt as rtdt;
use crate::index_types::INDEX_TYPE;
use crate::types::PTR_TYPE;
use crate::CraneliftError;
use super::FunctionCompiler;

const LIST_DATA: i32 = offset_of!(rtdt::List, data) as i32;
const LIST_SIZE: i32 = offset_of!(rtdt::List, size) as i32;

const STRIDE_ELEM: i32 = offset_of!(rtdt::StrideCursor, elem) as i32;
const STRIDE_STRIDE: i32 = offset_of!(rtdt::StrideCursor, stride) as i32;
const STRIDE_INDEX: i32 = offset_of!(rtdt::StrideCursor, index) as i32;
const STRIDE_LEN: i32 = offset_of!(rtdt::StrideCursor, len) as i32;

const ROW_VIEW: i32 = offset_of!(rtdt::RowCursor, view) as i32;
const ROW_OFFSET: i32 = ROW_VIEW + offset_of!(rtdt::Tensor, offset_elems) as i32;
const ROW_STRIDE: i32 = offset_of!(rtdt::RowCursor, stride) as i32;
const ROW_INDEX: i32 = offset_of!(rtdt::RowCursor, index) as i32;
const ROW_LEN: i32 = offset_of!(rtdt::RowCursor, len) as i32;

const TREE_LEAF: i32 = offset_of!(rtdt::TreeCursor, leaf) as i32;
const TREE_ELEM: i32 = offset_of!(rtdt::TreeCursor, elem) as i32;
const TREE_VALUE: i32 = offset_of!(rtdt::TreeCursor, value) as i32;
const TREE_SLOT: i32 = offset_of!(rtdt::TreeCursor, slot) as i32;

/// Where a node keeps how many keys it holds, a set's and a map's alike.
const NODE_LEN: i32 = offset_of!(rtdt::SetNode, len) as i32;
const _: () = assert!(offset_of!(rtdt::SetNode, len) == offset_of!(rtdt::MapNode, len));

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Which cursor a slot holds.
    fn cursor_kind(&self, cursor: SlotId) -> CursorKind {
        match &self.func.slot_types[cursor.0 as usize] {
            IrType::Cursor(kind) => *kind,
            other => panic!("a cursor instruction on a slot of {other}"),
        }
    }

    fn cursor_ptr(&mut self, builder: &mut FunctionBuilder, cursor: SlotId) -> Result<cl_ir::Value, CraneliftError> {
        self.get_operand_ptr(builder, &Operand::Slot(cursor))
    }

    fn rt_handle(&self, op_name: &str) -> Result<cl_ir::Value, CraneliftError> {
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
        let cursor_ptr = self.cursor_ptr(builder, cursor)?;
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

    /// Start a walk. A list's is open-coded, being three loads and a zero; a
    /// tensor's or a tree's is a call, once a walk.
    pub(super) fn compile_cursor_begin(
        &mut self,
        builder: &mut FunctionBuilder,
        cursor: SlotId,
        coll: &Operand,
    ) -> Result<(), CraneliftError> {
        let coll_ty = self.get_operand_type(coll)?;
        let func = match (self.cursor_kind(cursor), &coll_ty) {
            (CursorKind::Stride, IrType::List(elem)) => {
                let flags = MemFlagsData::new();
                let list = self.get_operand_ptr(builder, coll)?;
                let data = builder.ins().load(PTR_TYPE, flags, list, LIST_DATA);
                let size = builder.ins().load(INDEX_TYPE, flags, list, LIST_SIZE);
                let stride = self.value_size(builder, elem)?;
                let at = self.cursor_ptr(builder, cursor)?;
                builder.ins().store(flags, data, at, STRIDE_ELEM);
                builder.ins().store(flags, stride, at, STRIDE_STRIDE);
                let zero = builder.ins().iconst(INDEX_TYPE, 0);
                builder.ins().store(flags, zero, at, STRIDE_INDEX);
                builder.ins().store(flags, size, at, STRIDE_LEN);
                return Ok(());
            }
            (CursorKind::Stride, IrType::Tensor(..)) => self.runtime.tensor_cursor_begin,
            (CursorKind::Row, IrType::Tensor(..)) => self.runtime.tensor_row_cursor_begin,
            (CursorKind::Tree, IrType::Set(_)) => self.runtime.set_cursor_begin,
            (CursorKind::Tree, IrType::Map(..)) => self.runtime.map_cursor_begin,
            (kind, ty) => panic!("a {kind:?} cursor over a {ty}"),
        };
        self.call_cursor_with_coll(builder, func, cursor, coll, true, "CursorBegin")
    }

    /// Whether a walk is over: the index at the length, or no leaf.
    pub(super) fn compile_cursor_done(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        cursor: SlotId,
    ) -> Result<(), CraneliftError> {
        let flags = MemFlagsData::new();
        let at = self.cursor_ptr(builder, cursor)?;
        let done = match self.cursor_kind(cursor) {
            CursorKind::Stride | CursorKind::Row => {
                let (index_at, len_at) = match self.cursor_kind(cursor) {
                    CursorKind::Stride => (STRIDE_INDEX, STRIDE_LEN),
                    _ => (ROW_INDEX, ROW_LEN),
                };
                let index = builder.ins().load(INDEX_TYPE, flags, at, index_at);
                let len = builder.ins().load(INDEX_TYPE, flags, at, len_at);
                builder.ins().icmp(IntCC::Equal, index, len)
            }
            CursorKind::Tree => {
                let leaf = builder.ins().load(PTR_TYPE, flags, at, TREE_LEAF);
                builder.ins().icmp_imm_u(IntCC::Equal, leaf, 0)
            }
        };
        self.values.insert(dest, done);
        Ok(())
    }

    /// A reference to the current element: the cursor's pointer, or for a
    /// row, the view inside the cursor.
    pub(super) fn compile_cursor_elem(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        cursor: SlotId,
    ) -> Result<(), CraneliftError> {
        let flags = MemFlagsData::new();
        let at = self.cursor_ptr(builder, cursor)?;
        let elem = match self.cursor_kind(cursor) {
            CursorKind::Stride => builder.ins().load(PTR_TYPE, flags, at, STRIDE_ELEM),
            CursorKind::Row => builder.ins().iadd_imm_s(at, ROW_VIEW as i64),
            CursorKind::Tree => builder.ins().load(PTR_TYPE, flags, at, TREE_ELEM),
        };
        self.values.insert(dest, elem);
        Ok(())
    }

    /// The current element's position in a walk over a list or a tensor.
    pub(super) fn compile_cursor_index(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        cursor: SlotId,
    ) -> Result<(), CraneliftError> {
        let index_at = match self.cursor_kind(cursor) {
            CursorKind::Stride => STRIDE_INDEX,
            CursorKind::Row => ROW_INDEX,
            CursorKind::Tree => panic!("a tree cursor has no index"),
        };
        let at = self.cursor_ptr(builder, cursor)?;
        let index = builder.ins().load(INDEX_TYPE, MemFlagsData::new(), at, index_at);
        self.values.insert(dest, index);
        Ok(())
    }

    /// A reference to a map's current value.
    pub(super) fn compile_cursor_value(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        cursor: SlotId,
    ) -> Result<(), CraneliftError> {
        let at = self.cursor_ptr(builder, cursor)?;
        let value = builder.ins().load(PTR_TYPE, MemFlagsData::new(), at, TREE_VALUE);
        self.values.insert(dest, value);
        Ok(())
    }

    /// Move a walk on. A list's or a tensor's is a pointer or an offset and a
    /// count; a tree's moves along its leaf inline and calls the runtime only
    /// to cross to the next one.
    pub(super) fn compile_cursor_advance(
        &mut self,
        builder: &mut FunctionBuilder,
        cursor: SlotId,
        coll: &Operand,
    ) -> Result<(), CraneliftError> {
        let flags = MemFlagsData::new();
        let kind = self.cursor_kind(cursor);
        let at = self.cursor_ptr(builder, cursor)?;
        match kind {
            CursorKind::Stride => {
                let elem = builder.ins().load(PTR_TYPE, flags, at, STRIDE_ELEM);
                let stride = builder.ins().load(PTR_TYPE, flags, at, STRIDE_STRIDE);
                let next = builder.ins().iadd(elem, stride);
                builder.ins().store(flags, next, at, STRIDE_ELEM);
                let index = builder.ins().load(INDEX_TYPE, flags, at, STRIDE_INDEX);
                let following = builder.ins().iadd_imm_s(index, 1);
                builder.ins().store(flags, following, at, STRIDE_INDEX);
            }
            CursorKind::Row => {
                let offset = builder.ins().load(INDEX_TYPE, flags, at, ROW_OFFSET);
                let stride = builder.ins().load(INDEX_TYPE, flags, at, ROW_STRIDE);
                let next = builder.ins().iadd(offset, stride);
                builder.ins().store(flags, next, at, ROW_OFFSET);
                let index = builder.ins().load(INDEX_TYPE, flags, at, ROW_INDEX);
                let following = builder.ins().iadd_imm_s(index, 1);
                builder.ins().store(flags, following, at, ROW_INDEX);
            }
            CursorKind::Tree => {
                let coll_ty = self.get_operand_type(coll)?;
                let (func, key_ty, value_ty) = match &coll_ty {
                    IrType::Set(elem) => (self.runtime.set_cursor_advance, (**elem).clone(), None),
                    IrType::Map(key, value) => (self.runtime.map_cursor_advance, (**key).clone(), Some((**value).clone())),
                    ty => panic!("a tree cursor over a {ty}"),
                };
                let within = builder.create_block();
                let across = builder.create_block();
                let after = builder.create_block();

                let leaf = builder.ins().load(PTR_TYPE, flags, at, TREE_LEAF);
                let slot = builder.ins().load(cl_types::I32, flags, at, TREE_SLOT);
                let next_slot = builder.ins().iadd_imm_s(slot, 1);
                let len = builder.ins().load(cl_types::I32, flags, leaf, NODE_LEN);
                let in_leaf = builder.ins().icmp(IntCC::UnsignedLessThan, next_slot, len);
                builder.ins().brif(in_leaf, within, &[], across, &[]);

                builder.switch_to_block(within);
                builder.ins().store(flags, next_slot, at, TREE_SLOT);
                let key_size = self.value_size(builder, &key_ty)?;
                let elem = builder.ins().load(PTR_TYPE, flags, at, TREE_ELEM);
                let next_elem = builder.ins().iadd(elem, key_size);
                builder.ins().store(flags, next_elem, at, TREE_ELEM);
                if let Some(value_ty) = value_ty {
                    let value_size = self.value_size(builder, &value_ty)?;
                    let value = builder.ins().load(PTR_TYPE, flags, at, TREE_VALUE);
                    let next_value = builder.ins().iadd(value, value_size);
                    builder.ins().store(flags, next_value, at, TREE_VALUE);
                }
                builder.ins().jump(after, &[]);

                builder.switch_to_block(across);
                self.call_cursor_with_coll(builder, func, cursor, coll, false, "CursorAdvance")?;
                builder.ins().jump(after, &[]);

                builder.switch_to_block(after);
            }
        }
        Ok(())
    }
}
