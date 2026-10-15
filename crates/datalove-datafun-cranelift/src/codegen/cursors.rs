//! Cursors: a `for` loop's walk over a collection.
//!
//! The runtime's `dtlv_rti_*_cursor_*` functions begin and advance a walk, and
//! the interpreter runs exactly them; whether it is done and its current
//! element are the cursor's fields. Begin is a call, once a walk: it reads the
//! collection and its descriptor and fills in everything the walk needs. The
//! rest is open-coded on the cursor's fields alone, read off the `rtdt`
//! structs, keeping the runtime's meanings field for field: OSR continues in
//! compiled code a walk the interpreter began.

use std::mem::offset_of;
use cranelift_codegen::ir::{self as cl_ir, condcodes::IntCC, types as cl_types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use datalove_datafun_ir::{CursorKind, IrType, Operand, SlotId, ValueId};
use datalove_rtdt as rtdt;
use crate::index_types::INDEX_TYPE;
use crate::types::PTR_TYPE;
use crate::CraneliftError;
use super::FunctionCompiler;

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
const TREE_ELEM_STRIDE: i32 = offset_of!(rtdt::TreeCursor, elem_stride) as i32;
const TREE_VALUE_STRIDE: i32 = offset_of!(rtdt::TreeCursor, value_stride) as i32;
const TREE_SLOT: i32 = offset_of!(rtdt::TreeCursor, slot) as i32;
const TREE_LEAF_LEN: i32 = offset_of!(rtdt::TreeCursor, leaf_len) as i32;
const TREE_NEXT_LEAF_OFFSET: i32 = offset_of!(rtdt::TreeCursor, next_leaf_offset) as i32;
const TREE_KEYS_OFFSET: i32 = offset_of!(rtdt::TreeCursor, keys_offset) as i32;
const TREE_VALUES_OFFSET: i32 = offset_of!(rtdt::TreeCursor, values_offset) as i32;

/// Where a node keeps how many keys it holds, a set's and a map's alike.
const NODE_LEN: i32 = rtdt::TREE_NODE_LEN_OFFSET as i32;

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

    /// Start a walk, with a call to the collection's begin.
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
        let rt = self.rt_handle("CursorBegin")?;
        let coll_ptr = self.get_operand_ptr(builder, coll)?;
        let coll_td = self.tydesc_value(builder, &coll_ty)?;
        let cursor_ptr = self.cursor_ptr(builder, cursor)?;
        let func_ref = self.module.declare_func_in_func(func, builder.func);
        builder.ins().call(func_ref, &[rt, coll_ptr, coll_td, cursor_ptr]);
        Ok(())
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

    /// Move a walk on, as the runtime's advance does, on the cursor's fields
    /// alone. A list's or a tensor's is a pointer or an offset and a count; a
    /// tree's steps along its leaf by the strides, and at the end of one
    /// follows the chain to the next.
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
                let is_map = match self.get_operand_type(coll)? {
                    IrType::Set(_) => false,
                    IrType::Map(..) => true,
                    ty => panic!("a tree cursor over a {ty}"),
                };
                let within = builder.create_block();
                let across = builder.create_block();
                let chained = builder.create_block();
                let ended = builder.create_block();
                let after = builder.create_block();

                let leaf = builder.ins().load(PTR_TYPE, flags, at, TREE_LEAF);
                let slot = builder.ins().load(cl_types::I32, flags, at, TREE_SLOT);
                let next_slot = builder.ins().iadd_imm_s(slot, 1);
                let len = builder.ins().load(cl_types::I32, flags, at, TREE_LEAF_LEN);
                let in_leaf = builder.ins().icmp(IntCC::UnsignedLessThan, next_slot, len);
                builder.ins().brif(in_leaf, within, &[], across, &[]);

                builder.switch_to_block(within);
                builder.ins().store(flags, next_slot, at, TREE_SLOT);
                let elem = builder.ins().load(PTR_TYPE, flags, at, TREE_ELEM);
                let elem_stride = builder.ins().load(PTR_TYPE, flags, at, TREE_ELEM_STRIDE);
                let next_elem = builder.ins().iadd(elem, elem_stride);
                builder.ins().store(flags, next_elem, at, TREE_ELEM);
                if is_map {
                    let value = builder.ins().load(PTR_TYPE, flags, at, TREE_VALUE);
                    let value_stride = builder.ins().load(PTR_TYPE, flags, at, TREE_VALUE_STRIDE);
                    let next_value = builder.ins().iadd(value, value_stride);
                    builder.ins().store(flags, next_value, at, TREE_VALUE);
                }
                builder.ins().jump(after, &[]);

                builder.switch_to_block(across);
                let next_leaf_offset = builder.ins().load(cl_types::I32, flags, at, TREE_NEXT_LEAF_OFFSET);
                let next_leaf_offset = builder.ins().uextend(PTR_TYPE, next_leaf_offset);
                let next_leaf_at = builder.ins().iadd(leaf, next_leaf_offset);
                let next = builder.ins().load(PTR_TYPE, flags, next_leaf_at, 0);
                builder.ins().store(flags, next, at, TREE_LEAF);
                let zero = builder.ins().iconst(cl_types::I32, 0);
                builder.ins().store(flags, zero, at, TREE_SLOT);
                builder.ins().brif(next, chained, &[], ended, &[]);

                builder.switch_to_block(chained);
                let next_len = builder.ins().load(cl_types::I32, flags, next, NODE_LEN);
                builder.ins().store(flags, next_len, at, TREE_LEAF_LEN);
                let keys_offset = builder.ins().load(cl_types::I32, flags, at, TREE_KEYS_OFFSET);
                let keys_offset = builder.ins().uextend(PTR_TYPE, keys_offset);
                let first_key = builder.ins().iadd(next, keys_offset);
                builder.ins().store(flags, first_key, at, TREE_ELEM);
                if is_map {
                    let values_offset = builder.ins().load(cl_types::I32, flags, at, TREE_VALUES_OFFSET);
                    let values_offset = builder.ins().uextend(PTR_TYPE, values_offset);
                    let first_value = builder.ins().iadd(next, values_offset);
                    builder.ins().store(flags, first_value, at, TREE_VALUE);
                }
                builder.ins().jump(after, &[]);

                builder.switch_to_block(ended);
                let null = builder.ins().iconst(PTR_TYPE, 0);
                builder.ins().store(flags, null, at, TREE_ELEM);
                if is_map {
                    builder.ins().store(flags, null, at, TREE_VALUE);
                }
                builder.ins().store(flags, zero, at, TREE_LEAF_LEN);
                builder.ins().jump(after, &[]);

                builder.switch_to_block(after);
            }
        }
        Ok(())
    }
}
