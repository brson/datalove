//! Natives the compiler knows the meaning of, done in place of the call.
//!
//! A call to a native passes each argument as a pointer and a descriptor,
//! and the native works out from the descriptor what a push or a swap moves.
//! Where the types are static all of that is known here, and the work is a
//! few loads and stores. The native stays the definition, and is called where
//! a type is dynamic.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_intrinsics::NativeIntrinsic;
use datalove_datafun_ir::{IrType, Operand, ParamMode, ValueId};
use datalove_rtdt::OptionTag;

use crate::index_types::INDEX_TYPE;
use crate::types::{self, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

const LIST_DATA_OFFSET: i32 = std::mem::offset_of!(datalove_rtdt::List, data) as i32;
const LIST_SIZE_OFFSET: i32 = std::mem::offset_of!(datalove_rtdt::List, size) as i32;
const LIST_CAPACITY_OFFSET: i32 = std::mem::offset_of!(datalove_rtdt::List, capacity) as i32;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Do the work of a call to intrinsic native `intrinsic` in place, if its
    /// types are static; false if not, for the caller to call the native.
    ///
    /// `modes` are the native's, checked against the intrinsic's shape, since
    /// what is emitted here relies on it.
    pub(super) fn compile_native_intrinsic(
        &mut self,
        builder: &mut FunctionBuilder,
        intrinsic: NativeIntrinsic,
        modes: &[ParamMode],
        dest: ValueId,
        args: &[Operand],
    ) -> Result<bool, CraneliftError> {
        use ParamMode::{In, Mut};
        let shape: &[ParamMode] = match intrinsic {
            NativeIntrinsic::ListPush => &[Mut, In],
            NativeIntrinsic::ListPop => &[Mut],
            NativeIntrinsic::MemSwap => &[Mut, Mut],
            NativeIntrinsic::MemReplace => &[Mut, In],
        };
        assert_eq!(modes, shape, "intrinsic native {intrinsic:?} declared with the wrong modes");

        let first_ty = self.get_operand_type(&args[0])?;
        if first_ty.is_dynamic() {
            return Ok(false);
        }
        match intrinsic {
            NativeIntrinsic::ListPush => self.open_list_push(builder, &first_ty, args)?,
            NativeIntrinsic::ListPop => return self.open_list_pop(builder, &first_ty, dest, args).map(|()| true),
            NativeIntrinsic::MemSwap => {
                let a = self.get_operand_ptr(builder, &args[0])?;
                let b = self.get_operand_ptr(builder, &args[1])?;
                self.swap_bytes(builder, a, b, types::ir_type_size(&first_ty) as u64);
            }
            NativeIntrinsic::MemReplace => {
                // The old value goes out, then `src` moves in; as the native
                // does, `src`'s own place is left as it was.
                let place = self.get_operand_ptr(builder, &args[0])?;
                let size = types::ir_type_size(&first_ty) as u64;
                if self.holds_words(dest) {
                    let old = Self::load_words(builder, place, &first_ty);
                    self.put_value(builder, place, &first_ty, &args[1])?;
                    self.words.insert(dest, old);
                    return Ok(true);
                }
                let out = self.dest_addr(builder, dest)?;
                self.copy_bytes(builder, out, place, size);
                self.put_value(builder, place, &first_ty, &args[1])?;
            }
        }
        let dest_ptr = self.dest_addr(builder, dest)?;
        self.bind_written_dest(builder, dest, dest_ptr);
        Ok(true)
    }

    /// `list_push`: the element stored after the last if there is room, and
    /// the runtime's push, which grows the list, if not.
    fn open_list_push(
        &mut self,
        builder: &mut FunctionBuilder,
        list_ty: &IrType,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
        let IrType::List(elem_ty) = list_ty else {
            panic!("list_push's first parameter is a list, not {list_ty}");
        };
        let elem_size = types::ir_type_size(elem_ty) as i64;
        let list = self.get_operand_ptr(builder, &args[0])?;
        // An element held in registers is stored from them, and has an address
        // made for it only on the way to the runtime.
        let in_registers = self.words_of(&args[1]).is_some()
            || matches!(types::ir_type_to_cranelift(elem_ty), types::CraneliftRepr::Scalar(_));
        let elem = if in_registers { None } else { Some(self.get_operand_ptr(builder, &args[1])?) };
        let flags = MemFlagsData::new();
        let size = builder.ins().load(INDEX_TYPE, flags, list, LIST_SIZE_OFFSET);
        let capacity = builder.ins().load(INDEX_TYPE, flags, list, LIST_CAPACITY_OFFSET);
        let has_room = builder.ins().icmp(cl_ir::condcodes::IntCC::UnsignedLessThan, size, capacity);

        let store = builder.create_block();
        let grow = builder.create_block();
        let done = builder.create_block();
        builder.ins().brif(has_room, store, &[], grow, &[]);

        builder.switch_to_block(store);
        builder.seal_block(store);
        let data = builder.ins().load(PTR_TYPE, flags, list, LIST_DATA_OFFSET);
        let at = self.element_offset(builder, size, elem_size);
        let slot = builder.ins().iadd(data, at);
        match elem {
            Some(elem) => self.copy_bytes(builder, slot, elem, elem_size as u64),
            None => self.put_value(builder, slot, elem_ty, &args[1])?,
        }
        let grown = builder.ins().iadd_imm_s(size, 1);
        builder.ins().store(flags, grown, list, LIST_SIZE_OFFSET);
        builder.ins().jump(done, &[]);

        builder.set_cold_block(grow);
        builder.switch_to_block(grow);
        builder.seal_block(grow);
        let rt = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("list_push requires the runtime handle".into())
        })?;
        let list_desc = self.static_tydesc(builder, list_ty)?;
        let elem_desc = self.static_tydesc(builder, elem_ty)?;
        let elem = match elem {
            Some(elem) => elem,
            None => self.get_operand_ptr(builder, &args[1])?,
        };
        let push = self.module.declare_func_in_func(self.runtime.list_push, builder.func);
        builder.ins().call(push, &[rt, list, list_desc, elem, elem_desc]);
        builder.ins().jump(done, &[]);

        builder.switch_to_block(done);
        builder.seal_block(done);
        Ok(())
    }

    /// `list_pop`: the last element moved into the option, or none for an
    /// empty list.
    fn open_list_pop(
        &mut self,
        builder: &mut FunctionBuilder,
        list_ty: &IrType,
        dest: ValueId,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
        let IrType::List(elem_ty) = list_ty else {
            panic!("list_pop's parameter is a list, not {list_ty}");
        };
        let elem_size = types::ir_type_size(elem_ty) as i64;
        let option_ty = self.func.value_types[dest.0 as usize].clone();
        let list = self.get_operand_ptr(builder, &args[0])?;
        let flags = MemFlagsData::new();
        let size = builder.ins().load(INDEX_TYPE, flags, list, LIST_SIZE_OFFSET);

        // Held as words, the option is built in registers on both arms and
        // meets at the end as block parameters.
        let payload_at = super::dynamic::static_payload_offset(&option_ty, 0)?;
        let elem_cut = super::words::word_pieces(elem_ty).unwrap_or_default();
        let elem_parts: Vec<(u32, u32)> = elem_cut.iter().map(|&(at, t)| (payload_at + at as u32, t.bytes())).collect();
        let as_words = self.holds_words(dest)
            && (elem_size == 0 || !elem_cut.is_empty())
            && super::words::parts_fit(&option_ty, &[&[(0, 1)][..], &elem_parts].concat());
        if as_words {
            let take = builder.create_block();
            let empty = builder.create_block();
            let done = builder.create_block();
            let word_tys: Vec<_> = super::words::word_pieces(&option_ty).expect("held as words").into_iter().map(|(_, t)| t).collect();
            for &t in &word_tys {
                builder.append_block_param(done, t);
            }
            builder.ins().brif(size, take, &[], empty, &[]);

            builder.switch_to_block(take);
            builder.seal_block(take);
            let last = builder.ins().iadd_imm_s(size, -1);
            builder.ins().store(flags, last, list, LIST_SIZE_OFFSET);
            let data = builder.ins().load(PTR_TYPE, flags, list, LIST_DATA_OFFSET);
            let at = self.element_offset(builder, last, elem_size);
            let slot = builder.ins().iadd(data, at);
            let some = builder.ins().iconst(cl_types::I8, OptionTag::Some as i64);
            let mut parts = vec![(0, some)];
            for &(at, t) in &elem_cut {
                let w = builder.ins().load(t, flags, slot, at);
                parts.push((payload_at + at as u32, w));
            }
            let words = Self::compose_words(builder, &option_ty, &parts).expect("checked to fit");
            let args: Vec<cl_ir::BlockArg> = words.into_iter().map(Into::into).collect();
            builder.ins().jump(done, &args);

            builder.switch_to_block(empty);
            builder.seal_block(empty);
            let none = builder.ins().iconst(cl_types::I8, OptionTag::None as i64);
            let words = Self::compose_words(builder, &option_ty, &[(0, none)]).expect("a tag fits");
            let args: Vec<cl_ir::BlockArg> = words.into_iter().map(Into::into).collect();
            builder.ins().jump(done, &args);

            builder.switch_to_block(done);
            builder.seal_block(done);
            let words = builder.block_params(done).to_vec();
            self.words.insert(dest, words);
            return Ok(());
        }

        let out = self.dest_addr(builder, dest)?;

        let take = builder.create_block();
        let empty = builder.create_block();
        let done = builder.create_block();
        builder.ins().brif(size, take, &[], empty, &[]);

        builder.switch_to_block(take);
        builder.seal_block(take);
        let last = builder.ins().iadd_imm_s(size, -1);
        builder.ins().store(flags, last, list, LIST_SIZE_OFFSET);
        let data = builder.ins().load(PTR_TYPE, flags, list, LIST_DATA_OFFSET);
        let at = self.element_offset(builder, last, elem_size);
        let slot = builder.ins().iadd(data, at);
        let payload = self.payload_addr(builder, out, &option_ty, 0)?;
        self.copy_bytes(builder, payload, slot, elem_size as u64);
        let some = builder.ins().iconst(cl_types::I8, OptionTag::Some as i64);
        builder.ins().store(flags, some, out, 0);
        builder.ins().jump(done, &[]);

        builder.switch_to_block(empty);
        builder.seal_block(empty);
        let none = builder.ins().iconst(cl_types::I8, OptionTag::None as i64);
        builder.ins().store(flags, none, out, 0);
        builder.ins().jump(done, &[]);

        builder.switch_to_block(done);
        builder.seal_block(done);
        self.bind_written_dest(builder, dest, out);
        Ok(())
    }

    /// Where element `index` of a list starts, from its data, as a
    /// pointer-width offset.
    fn element_offset(&self, builder: &mut FunctionBuilder, index: cl_ir::Value, elem_size: i64) -> cl_ir::Value {
        let index = if INDEX_TYPE != PTR_TYPE { builder.ins().uextend(PTR_TYPE, index) } else { index };
        builder.ins().imul_imm_s(index, elem_size)
    }
}
