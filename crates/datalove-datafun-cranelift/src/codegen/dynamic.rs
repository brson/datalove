//! Values of a dynamic type: those of a generic's type parameters.
//!
//! A generic is handed one descriptor per type
//! parameter, after its other arguments. On entry it fills its descriptor
//! table from them -- a bare parameter is its descriptor, anything else is
//! instantiated from a template the emitter wrote -- and takes space on the
//! shadow stack for each value and slot of a dynamic type, sized by its
//! entry. A dynamic value is then an aggregate like any other to the rest of
//! codegen: its Cranelift value is the address of its storage. What differs
//! is where that storage is, and that its size and the offsets inside it are
//! read from its descriptor rather than known here. See
//! `botdocs/plan-fat-generics.md`.
//!
//! A reference to a dynamic type is a plain pointer here, and its descriptor
//! is the table entry of what it points at.

use super::*;

use datalove_rt::c::RtHeader;
use datalove_rtdt::TyDesc;

const SHADOW_TOP_OFFSET: i32 = std::mem::offset_of!(RtHeader, shadow_top) as i32;
const SHADOW_LIMIT_OFFSET: i32 = std::mem::offset_of!(RtHeader, shadow_limit) as i32;
const TYDESC_SIZE_OFFSET: i32 = std::mem::offset_of!(TyDesc, size) as i32;
const TYDESC_ALIGN_OFFSET: i32 = std::mem::offset_of!(TyDesc, align) as i32;
const TYDESC_FLAGS_OFFSET: i32 = std::mem::offset_of!(TyDesc, flags) as i32;

/// Whether a type is held as a dynamic value: dynamic, and not a reference.
pub(super) fn is_dynamic_value(ty: &IrType) -> bool {
    ty.is_dynamic() && !matches!(ty, IrType::Ref(_))
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Fill the descriptor table and give each dynamic value and slot its
    /// storage, in the entry block.
    pub(super) fn enter_dynamic(&mut self, builder: &mut FunctionBuilder) -> Result<(), CraneliftError> {
        if self.func_ctx.descriptor_table.is_empty() {
            return Ok(());
        }
        let rt = self.rt_handle_param.expect("the entry block has the runtime handle");

        // The type parameters' descriptors, as an array the runtime reads.
        // It has a slot even when there are none, since the runtime takes
        // the pointer as a slice's, which is never null.
        let count = self.type_param_values.len() as u32;
        let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot, count.max(1) * types::PTR_SIZE,
            types::align_shift(types::PTR_ALIGN)));
        for (i, &desc) in self.type_param_values.iter().enumerate() {
            let at = builder.ins().stack_addr(PTR_TYPE, slot, (i as u32 * types::PTR_SIZE) as i32);
            builder.ins().store(MemFlagsData::new(), desc, at, 0);
        }
        let params = builder.ins().stack_addr(PTR_TYPE, slot, 0);
        let num_params = builder.ins().iconst(cl_types::I32, count as i64);

        // The entries built from templates, by their table index.
        let table = self.func_ctx.descriptor_table.clone();
        let built: Vec<(usize, IrType)> = table.iter().enumerate()
            .filter(|(_, ty)| !matches!(ty, IrType::Param(_)))
            .map(|(i, ty)| (i, ty.clone()))
            .collect();
        let built_values = if built.is_empty() {
            Vec::new()
        } else {
            self.fill_cached(builder, rt, params, num_params, &built)?
        };
        let mut built_values = built_values.into_iter();
        for ty in &table {
            let desc = match ty {
                IrType::Param(i) => self.type_param_values[*i as usize],
                _ => built_values.next().expect("one built value per template entry"),
            };
            self.table_values.push(desc);
        }

        // Storage on the shadow stack, given back on every return.
        let values: Vec<(ValueId, IrType)> = self.func.value_types.iter().enumerate()
            .filter(|(_, ty)| is_dynamic_value(ty))
            .map(|(i, ty)| (ValueId(i as u32), ty.clone()))
            .collect();
        let slots: Vec<(SlotId, IrType)> = self.func.slot_types.iter().enumerate()
            .filter(|(_, ty)| is_dynamic_value(ty))
            .map(|(i, ty)| (SlotId(i as u32), ty.clone()))
            .collect();
        if values.is_empty() && slots.is_empty() {
            return Ok(());
        }
        let top = builder.ins().load(PTR_TYPE, MemFlagsData::new(), rt, SHADOW_TOP_OFFSET);
        let limit = builder.ins().load(PTR_TYPE, MemFlagsData::new(), rt, SHADOW_LIMIT_OFFSET);
        self.shadow_mark = Some((top, limit));
        for (id, ty) in values {
            let ptr = self.shadow_alloc(builder, &ty)?;
            self.dyn_storage.insert(id, ptr);
        }
        for (id, ty) in slots {
            let ptr = self.shadow_alloc(builder, &ty)?;
            self.dyn_slot_storage.insert(id, ptr);
        }
        Ok(())
    }

    /// The descriptors the table's template entries `built` instantiate to,
    /// through a cache of one: the type parameter descriptors of the last
    /// call and what they built.
    ///
    /// A call at the binding the last one had -- nearly every call, at a
    /// given call site -- then costs a compare per type parameter rather than
    /// a runtime lookup per entry. The cache is a data object of this
    /// function's own, `[params..., built...]`, zero to begin with, which no
    /// descriptor pointer is. It holds the descriptors a runtime handle built,
    /// and so lives no longer than code running on one does: the JIT's code
    /// goes with its interpreter, and an AOT program has the one handle.
    fn fill_cached(
        &mut self,
        builder: &mut FunctionBuilder,
        rt: cl_ir::Value,
        params: cl_ir::Value,
        num_params: cl_ir::Value,
        built: &[(usize, IrType)],
    ) -> Result<Vec<cl_ir::Value>, CraneliftError> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static CACHES: AtomicU32 = AtomicU32::new(0);

        let word = types::PTR_SIZE as i32;
        let count = self.type_param_values.len();
        let name = format!("__dtlv_table_cache_{}", CACHES.fetch_add(1, Ordering::Relaxed));
        let data = self.module
            .declare_data(&name, Linkage::Local, true, false)
            .map_err(|e| CraneliftError::Module(format!("declare table cache: {}", e)))?;
        let mut description = cranelift_module::DataDescription::new();
        description.define_zeroinit((count + built.len()) * types::PTR_SIZE as usize);
        description.set_align(types::PTR_ALIGN as u64);
        self.module.define_data(data, &description)
            .map_err(|e| CraneliftError::Module(format!("define table cache: {}", e)))?;
        let gv = self.module.declare_data_in_func(data, builder.func);
        let cache = builder.ins().symbol_value(PTR_TYPE, gv);

        let hit_block = builder.create_block();
        let miss_block = builder.create_block();
        let join = builder.create_block();
        for _ in built {
            builder.append_block_param(join, PTR_TYPE);
        }

        let mut hit = builder.ins().iconst(cl_types::I8, 1);
        for (i, &param) in self.type_param_values.clone().iter().enumerate() {
            let cached = builder.ins().load(PTR_TYPE, MemFlagsData::new(), cache, i as i32 * word);
            let same = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, cached, param);
            hit = builder.ins().band(hit, same);
        }
        builder.ins().brif(hit, hit_block, &[], miss_block, &[]);

        builder.switch_to_block(hit_block);
        builder.seal_block(hit_block);
        let cached: Vec<cl_ir::BlockArg> = (0..built.len())
            .map(|k| builder.ins().load(PTR_TYPE, MemFlagsData::new(), cache, (count + k) as i32 * word).into())
            .collect();
        builder.ins().jump(join, &cached);

        builder.switch_to_block(miss_block);
        builder.seal_block(miss_block);
        builder.set_cold_block(miss_block);
        let mut fresh = Vec::with_capacity(built.len());
        for (k, (_, ty)) in built.iter().enumerate() {
            let template = self.tydesc_emitter.emit(self.module, ty)?;
            let gv = self.module.declare_data_in_func(template, builder.func);
            let template = builder.ins().symbol_value(PTR_TYPE, gv);
            let instantiate = self.module.declare_func_in_func(self.runtime.tydesc_instantiate, builder.func);
            let call = builder.ins().call(instantiate, &[rt, template, params, num_params]);
            let desc = builder.inst_results(call)[0];
            builder.ins().store(MemFlagsData::new(), desc, cache, (count + k) as i32 * word);
            fresh.push(desc.into());
        }
        for (i, &param) in self.type_param_values.clone().iter().enumerate() {
            builder.ins().store(MemFlagsData::new(), param, cache, i as i32 * word);
        }
        builder.ins().jump(join, &fresh);

        builder.switch_to_block(join);
        builder.seal_block(join);
        Ok(builder.block_params(join).to_vec())
    }

    /// Space on the shadow stack for a value of dynamic type `ty`.
    ///
    /// Bumped inline, as `ShadowStack::alloc` would; the runtime is called
    /// only when the current chunk has no room.
    fn shadow_alloc(&mut self, builder: &mut FunctionBuilder, ty: &IrType) -> Result<cl_ir::Value, CraneliftError> {
        let rt = self.rt_handle_param.expect("the entry block has the runtime handle");
        let desc = self.static_tydesc(builder, ty)?;
        let size = builder.ins().load(cl_types::I32, MemFlagsData::new(), desc, TYDESC_SIZE_OFFSET);
        let align = builder.ins().load(cl_types::I32, MemFlagsData::new(), desc, TYDESC_ALIGN_OFFSET);
        let size_wide = builder.ins().uextend(PTR_TYPE, size);
        let align_wide = builder.ins().uextend(PTR_TYPE, align);

        // start = (top + align - 1) & !(align - 1); fits if start + size <= limit.
        let top = builder.ins().load(PTR_TYPE, MemFlagsData::new(), rt, SHADOW_TOP_OFFSET);
        let limit = builder.ins().load(PTR_TYPE, MemFlagsData::new(), rt, SHADOW_LIMIT_OFFSET);
        let mask = builder.ins().iadd_imm_s(align_wide, -1);
        let bumped = builder.ins().iadd(top, mask);
        let not_mask = builder.ins().bnot(mask);
        let start = builder.ins().band(bumped, not_mask);
        let end = builder.ins().iadd(start, size_wide);
        let fits = builder.ins().icmp(cl_ir::condcodes::IntCC::UnsignedLessThanOrEqual, end, limit);

        let fast = builder.create_block();
        let slow = builder.create_block();
        let join = builder.create_block();
        builder.append_block_param(join, PTR_TYPE);
        builder.ins().brif(fits, fast, &[], slow, &[]);

        builder.switch_to_block(fast);
        builder.seal_block(fast);
        builder.ins().store(MemFlagsData::new(), end, rt, SHADOW_TOP_OFFSET);
        builder.ins().jump(join, &[start.into()]);

        builder.switch_to_block(slow);
        builder.seal_block(slow);
        builder.set_cold_block(slow);
        let alloc = self.module.declare_func_in_func(self.runtime.shadow_alloc, builder.func);
        let call = builder.ins().call(alloc, &[rt, size, align]);
        let ptr = builder.inst_results(call)[0];
        builder.ins().jump(join, &[ptr.into()]);

        builder.switch_to_block(join);
        builder.seal_block(join);
        Ok(builder.block_params(join)[0])
    }

    /// Give back the shadow stack space taken on entry, before a return.
    pub(super) fn leave_dynamic(&mut self, builder: &mut FunctionBuilder) {
        if let Some((top, limit)) = self.shadow_mark {
            let rt = self.rt_handle_param.expect("a function has the runtime handle");
            builder.ins().store(MemFlagsData::new(), top, rt, SHADOW_TOP_OFFSET);
            builder.ins().store(MemFlagsData::new(), limit, rt, SHADOW_LIMIT_OFFSET);
        }
    }

    /// The descriptor for `ty`, as a runtime pointer: its static symbol, or
    /// for a dynamic type its descriptor table entry.
    ///
    /// Unlike `static_tydesc` this does not read through a reference: it is
    /// the descriptor of `ty` itself.
    pub(super) fn tydesc_value(&mut self, builder: &mut FunctionBuilder, ty: &IrType) -> Result<cl_ir::Value, CraneliftError> {
        if ty.is_dynamic() && !matches!(ty, IrType::Ref(_)) {
            return self.table_entry(ty);
        }
        let id = self.tydesc(ty)?;
        let gv = self.module.declare_data_in_func(id, builder.func);
        Ok(builder.ins().symbol_value(PTR_TYPE, gv))
    }

    /// The descriptor table entry for dynamic type `ty`.
    pub(super) fn table_entry(&self, ty: &IrType) -> Result<cl_ir::Value, CraneliftError> {
        match self.func_ctx.descriptor_table.iter().position(|t| t == ty) {
            Some(i) => self.table_values.get(i).copied().ok_or_else(|| CraneliftError::Codegen(
                format!("descriptor table entry {i} read before the table was filled"))),
            None => Err(CraneliftError::Codegen(format!("{ty} is not in the descriptor table"))),
        }
    }

    /// The size of a value of `ty`, as a pointer-width Cranelift value: a
    /// constant, or for a dynamic type, read from its descriptor.
    pub(super) fn value_size(&mut self, builder: &mut FunctionBuilder, ty: &IrType) -> Result<cl_ir::Value, CraneliftError> {
        if is_dynamic_value(ty) {
            let desc = self.static_tydesc(builder, ty)?;
            let size = builder.ins().load(cl_types::I32, MemFlagsData::new(), desc, TYDESC_SIZE_OFFSET);
            return Ok(builder.ins().uextend(PTR_TYPE, size));
        }
        Ok(builder.ins().iconst(PTR_TYPE, types::ir_type_size(ty) as i64))
    }

    /// How many bytes of a moved-from value of dynamic `ty` to zero: its size,
    /// or none for a plain one.
    ///
    /// Zeroing is what keeps a moved value from being freed twice, and a plain
    /// value owns nothing to free. Its source may also be the caller's own copy
    /// value, handed to a `T` by pointer and still the caller's to read.
    pub(super) fn moved_size(&mut self, builder: &mut FunctionBuilder, ty: &IrType) -> Result<cl_ir::Value, CraneliftError> {
        let size = self.value_size(builder, ty)?;
        let plain = self.is_plain(builder, ty)?;
        let zero = builder.ins().iconst(PTR_TYPE, 0);
        Ok(builder.ins().select(plain, zero, size))
    }

    /// Whether a value of dynamic `ty` owns nothing, as its descriptor says:
    /// nonzero if so.
    pub(super) fn is_plain(&mut self, builder: &mut FunctionBuilder, ty: &IrType) -> Result<cl_ir::Value, CraneliftError> {
        let desc = self.static_tydesc(builder, ty)?;
        let flags = builder.ins().load(cl_types::I8, MemFlagsData::new(), desc, TYDESC_FLAGS_OFFSET);
        Ok(builder.ins().band_imm_u(flags, TyDesc::PLAIN as i64))
    }

    /// The size and alignment of a value of dynamic `ty`, as 32-bit values
    /// read from its descriptor.
    pub(super) fn size_and_align(&mut self, builder: &mut FunctionBuilder, ty: &IrType) -> Result<(cl_ir::Value, cl_ir::Value), CraneliftError> {
        let desc = self.static_tydesc(builder, ty)?;
        let size = builder.ins().load(cl_types::I32, MemFlagsData::new(), desc, TYDESC_SIZE_OFFSET);
        let align = builder.ins().load(cl_types::I32, MemFlagsData::new(), desc, TYDESC_ALIGN_OFFSET);
        Ok((size, align))
    }

    /// Copy a value of `ty` from `src` to `dst`, as many bytes as it has.
    pub(super) fn copy_value_bytes(
        &mut self,
        builder: &mut FunctionBuilder,
        dst: cl_ir::Value,
        src: cl_ir::Value,
        ty: &IrType,
    ) -> Result<(), CraneliftError> {
        let size = self.value_size(builder, ty)?;
        builder.call_memcpy(self.isa.frontend_config(), dst, src, size);
        Ok(())
    }

    /// The address of the payload of `base`, an option, result, term or
    /// enum of type `ty`: of variant `index`, for an enum.
    ///
    /// A static type's offset is a constant here. A dynamic one's is the
    /// runtime's to say (`dtlv_rti_payload_offset`), since the payload's
    /// alignment is only in its descriptor, and no one rule covers every
    /// form.
    pub(super) fn payload_addr(
        &mut self,
        builder: &mut FunctionBuilder,
        base: cl_ir::Value,
        ty: &IrType,
        index: u32,
    ) -> Result<cl_ir::Value, CraneliftError> {
        use datalove_datafun_ir::layout as ir_layout;
        if !ty.is_dynamic() {
            let offset = match ty {
                IrType::Option(inner) => ir_layout::option_payload_offset(inner),
                IrType::Result(ok) => ir_layout::result_payload_offset(ok),
                IrType::Term(..) => 0,
                IrType::Enum(variants) => match &variants[index as usize].1 {
                    Some(payload) => ir_layout::enum_payload_offset(payload),
                    None => 0,
                },
                other => return Err(CraneliftError::Codegen(format!("a payload of {other}"))),
            };
            return Ok(builder.ins().iadd_imm_s(base, offset as i64));
        }
        let desc = self.static_tydesc(builder, ty)?;
        let index = builder.ins().iconst(cl_types::I32, index as i64);
        let call_ref = self.module.declare_func_in_func(self.runtime.payload_offset, builder.func);
        let call = builder.ins().call(call_ref, &[desc, index]);
        let offset = builder.inst_results(call)[0];
        let offset = builder.ins().uextend(PTR_TYPE, offset);
        Ok(builder.ins().iadd(base, offset))
    }

    /// The address of field `index` of `base`, a tuple or struct of type `ty`.
    ///
    /// A static type's offset is a constant here; a dynamic one's is read
    /// from its descriptor (`dtlv_rti_field_offset`).
    pub(super) fn field_addr(
        &mut self,
        builder: &mut FunctionBuilder,
        base: cl_ir::Value,
        ty: &IrType,
        index: u32,
    ) -> Result<cl_ir::Value, CraneliftError> {
        if !ty.is_dynamic() {
            let field_types: Vec<IrType> = match ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, t)| t.clone()).collect(),
                other => return Err(CraneliftError::Codegen(format!("a field of {other}"))),
            };
            let offset = types::compute_tuple_field_offsets(&field_types)[index as usize];
            return Ok(builder.ins().iadd_imm_s(base, offset as i64));
        }
        let desc = self.static_tydesc(builder, ty)?;
        let index = builder.ins().iconst(cl_types::I32, index as i64);
        let call_ref = self.module.declare_func_in_func(self.runtime.field_offset, builder.func);
        let call = builder.ins().call(call_ref, &[desc, index]);
        let offset = builder.inst_results(call)[0];
        let offset = builder.ins().uextend(PTR_TYPE, offset);
        Ok(builder.ins().iadd(base, offset))
    }

    /// Store `value`, of type `ty`, at `addr`: a scalar by a store, anything
    /// else by copying as many bytes as it has.
    pub(super) fn put_value(
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
                let src = self.get_operand_ptr(builder, value)?;
                self.copy_value_bytes(builder, addr, src, ty)?;
            }
        }
        Ok(())
    }

    /// Where `dest` is to be written: its storage on the shadow stack if it
    /// is dynamic, its place in the frame otherwise.
    pub(super) fn dest_addr(&mut self, builder: &mut FunctionBuilder, dest: ValueId) -> Result<cl_ir::Value, CraneliftError> {
        if let Some(&ptr) = self.dyn_storage.get(&dest) {
            return Ok(ptr);
        }
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen(format!("no frame slot for {:?}", dest))
        })?;
        Ok(frame_slot.addr(builder, self.layout.value_offset(dest.0) as i32))
    }

    /// Refuse a function using an instruction on a dynamic value that the
    /// codegen does not handle yet, rather than compile it wrong.
    ///
    /// A refusal leaves the function to the interpreter under the JIT, and
    /// fails the AOT build.
    pub(super) fn check_dynamic_support(&self) -> Result<(), CraneliftError> {
        if !datalove_datafun_ir::uses_descriptors(self.func) {
            return Ok(());
        }

        for block in &self.func.blocks {
            for instr in &block.instructions {
                let mut touches = instr.dests().into_iter()
                    .any(|v| self.func.value_types[v.0 as usize].is_dynamic());
                instr.for_each_operand(|op: &Operand| {
                    touches |= self.get_operand_type(op).is_ok_and(|t| t.is_dynamic());
                });
                // What is written into, which is neither an operand nor a
                // result.
                match instr {
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(s), .. }
                    | Instruction::SlotStoreMove { dest: SlotDest::Local(s), .. }
                    | Instruction::SlotStoreCopyTracked { dest: SlotDest::Local(s), .. }
                    | Instruction::SlotStoreMoveTracked { dest: SlotDest::Local(s), .. }
                    | Instruction::SetField { slot: SlotDest::Local(s), .. }
                    | Instruction::SetFieldTracked { slot: SlotDest::Local(s), .. } => {
                        touches |= self.func.slot_types[s.0 as usize].is_dynamic();
                    }
                    Instruction::ParamStore { param, .. }
                    | Instruction::ParamStoreTracked { param, .. }
                    | Instruction::ParamSetField { param, .. }
                    | Instruction::ParamSetFieldTracked { param, .. } => {
                        touches |= self.func_ctx.param_types[param.0 as usize].is_dynamic();
                    }
                    _ => {}
                }
                if touches && !supports_dynamic(instr) {
                    return Err(CraneliftError::Unsupported(format!(
                        "{} on a value of a generic's type parameter",
                        instruction_name(instr))));
                }
            }
        }
        Ok(())
    }
}

/// Whether the codegen handles an instruction whose operands or result are
/// dynamic.
fn supports_dynamic(instr: &Instruction) -> bool {
    matches!(instr,
        Instruction::Copy { .. }
        | Instruction::CursorBegin { .. }
        | Instruction::CursorDone { .. }
        | Instruction::CursorElem { .. }
        | Instruction::CursorIndex { .. }
        | Instruction::CursorValue { .. }
        | Instruction::CursorAdvance { .. }
        | Instruction::Move { .. }
        | Instruction::Clone { .. }
        | Instruction::Call { .. }
        | Instruction::ComptimeCall { .. }
        | Instruction::SlotStoreCopy { .. }
        | Instruction::SlotStoreMove { .. }
        | Instruction::SlotStoreCopyTracked { .. }
        | Instruction::SlotStoreMoveTracked { .. }
        | Instruction::SlotLoadCopy { .. }
        | Instruction::SlotLoadMove { .. }
        | Instruction::SlotLoadMoveTracked { .. }
        | Instruction::ParamStore { .. }
        | Instruction::ParamStoreTracked { .. }
        | Instruction::RefStore { .. }
        | Instruction::RefStoreTracked { .. }
        | Instruction::Drop { .. }
        | Instruction::DropTracked { .. }
        | Instruction::DropViaRef { .. }
        | Instruction::UnitEndDrop { .. }
        | Instruction::UnitEndDropTracked { .. }
        | Instruction::DebugLog { .. }
        | Instruction::Nop
        | Instruction::WrapSome { .. }
        | Instruction::WrapNone { .. }
        | Instruction::WrapOk { .. }
        | Instruction::WrapErr { .. }
        | Instruction::UnwrapOption { .. }
        | Instruction::UnwrapResult { .. }
        | Instruction::EnumVariant { .. }
        | Instruction::EnumDiscriminant { .. }
        | Instruction::EnumPayload { .. }
        | Instruction::ListNew { .. }
        | Instruction::TensorNew { .. }
        | Instruction::TableNew { .. }
        | Instruction::SetNew { .. }
        | Instruction::MapNew { .. }
        | Instruction::ListBoundsCheck { .. }
        | Instruction::ListGet { .. }
        | Instruction::ListSet { .. }
        | Instruction::ListElementRef { .. }
        | Instruction::BinOp { .. }
        | Instruction::BinOpChecked { .. }
        | Instruction::UnaryOp { .. }
        | Instruction::UnaryOpChecked { .. }
        | Instruction::Pack { .. }
        | Instruction::Unpack { .. }
        | Instruction::GetField { .. }
        | Instruction::GetFieldRef { .. }
        | Instruction::SetField { .. }
        | Instruction::SetFieldTracked { .. }
        | Instruction::ParamSetField { .. }
        | Instruction::ParamSetFieldTracked { .. }
        | Instruction::RefSetField { .. }
        | Instruction::RefSetFieldTracked { .. }
        | Instruction::DropField { .. }
        | Instruction::MapGet { .. }
        | Instruction::MapContainsKey { .. }
        | Instruction::MapSetValue { .. }
        | Instruction::MapValueRef { .. }
        | Instruction::MapUpsert { .. }
        | Instruction::TensorBoundsCheck { .. }
        | Instruction::TensorSet { .. }
        | Instruction::TensorGet { .. }
        | Instruction::TensorIndexRef { .. }
        | Instruction::DataFrom { .. }
        | Instruction::ErrorFrom { .. }
    )
}

/// An instruction's name, for a refusal to say what was refused.
fn instruction_name(instr: &Instruction) -> String {
    let text = format!("{:?}", instr);
    text.split([' ', '{', '(']).next().unwrap_or("an instruction").to_string()
}
