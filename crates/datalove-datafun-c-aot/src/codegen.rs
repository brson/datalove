//! C code generation for IR instructions and terminators.

use std::fmt::Write;

use datalove_datafun_ir::layout as ir_layout;
use datalove_datafun_ir::{
    BinOp, BlockId, CodeRef, ConstValue, FunctionRegistry, IrCodeUnit, IrModuleId,
    IrType, Instruction, Operand, ParamId, SlotDest, SlotId, Terminator, UnaryOp,
    ValueId,
};

use crate::layout::FrameLayout;
use crate::types::{self, CRepr};
use crate::{CAotCompiler, CAotError};

/// Emit a complete function.
pub fn emit_function(
    out: &mut String,
    func_name: &str,
    unit: &IrCodeUnit,
    module_id: Option<IrModuleId>,
    parent_unit: Option<&IrCodeUnit>,
    compiler: &mut CAotCompiler,
    registry: &FunctionRegistry,
) -> Result<(), CAotError> {
    let func_ctx = unit.function_context()
        .ok_or_else(|| CAotError::Codegen("expected function context".into()))?;

    let layout = FrameLayout::compute(
        &func_ctx.param_types,
        &unit.value_types,
        &unit.slot_types,
        &unit.tracked_slots,
        &func_ctx.tracked_params,
    );

    // Build signature.
    let uses_sret = types::uses_sret(&func_ctx.return_type);
    let mut params = String::from("void* rt");
    if uses_sret {
        params.push_str(", void* __sret");
    }
    for (i, _) in func_ctx.param_types.iter().enumerate() {
        write!(&mut params, ", void* p{}", i).unwrap();
    }
    for (i, _) in func_ctx.descriptor_params.iter().enumerate() {
        write!(&mut params, ", const dtlv_tydesc_t* d{}", i).unwrap();
    }
    // Then one for each shape the body builds a collection of, which no value
    // carries.
    for (i, _) in func_ctx.descriptor_shapes.iter().enumerate() {
        write!(&mut params, ", const dtlv_tydesc_t* s{}", i).unwrap();
    }

    let return_type = if func_ctx.return_type == IrType::Unit || uses_sret {
        "void"
    } else {
        &types::ir_type_to_c(&func_ctx.return_type)
    };

    // Module functions are not static (they need to be visible across files).
    // Local functions are static.
    let static_prefix = if module_id.is_some() { "" } else { "static " };
    writeln!(out, "{}{} {}({}) {{", static_prefix, return_type, func_name, params).unwrap();

    // Emit frame allocation.
    if layout.frame_size > 0 {
        writeln!(out, "    _Alignas({}) uint8_t __frame[{}];", layout.frame_align, layout.frame_size).unwrap();
    }

    // Initialize tracking bytes to UNINIT.
    if layout.tracking_count > 0 {
        writeln!(out, "    memset(__frame + {}, TRACK_UNINIT, {});", layout.tracking_offset, layout.tracking_count).unwrap();
    }

    // A reference whose static type does not describe what it points at carries
    // a descriptor beside the pointer. Declared here rather than where it is
    // assigned, so that a `goto` between blocks never jumps over a declaration.
    // Empty outside a generic.
    let ref_descs = datalove_datafun_ir::resolve_ref_descriptors(unit);
    for vid in ref_descs.keys() {
        writeln!(out, "    const dtlv_tydesc_t* __rd{};", vid.0).unwrap();
    }

    // Create function context for codegen.
    let mut ctx = FunctionCodegenContext {
        unit,
        parent_unit,
        layout: &layout,
        compiler,
        registry,
        uses_sret,
        const_scratch: 0,
        ref_descs,
    };

    // Emit blocks.
    for block in &unit.blocks {
        ctx.emit_block(out, block)?;
    }

    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    Ok(())
}

/// Emit the script body function.
pub fn emit_script_body(
    out: &mut String,
    unit: &IrCodeUnit,
    compiler: &mut CAotCompiler,
    registry: &FunctionRegistry,
) -> Result<(), CAotError> {
    let script_ctx = unit.script_context()
        .ok_or_else(|| CAotError::Codegen("expected script context".into()))?;

    let layout = FrameLayout::compute(
        &[],  // No params for script.
        &unit.value_types,
        &unit.slot_types,
        &unit.tracked_slots,
        &[],
    );

    writeln!(out, "static void __script_body(void* rt) {{").unwrap();

    // Emit frame allocation.
    if layout.frame_size > 0 {
        writeln!(out, "    _Alignas({}) uint8_t __frame[{}];", layout.frame_align, layout.frame_size).unwrap();
    }

    // Initialize tracking bytes to UNINIT.
    if layout.tracking_count > 0 {
        writeln!(out, "    memset(__frame + {}, TRACK_UNINIT, {});", layout.tracking_offset, layout.tracking_count).unwrap();
    }

    // Silence unused variable warning if result is unused.
    let _ = script_ctx;

    // Create script context for codegen.
    // Script body uses itself for local function lookup (it contains nested_units).
    // A script unit has no type parameters, so no reference in one carries a
    // descriptor.
    let mut ctx = FunctionCodegenContext {
        unit,
        parent_unit: None,
        layout: &layout,
        compiler,
        registry,
        uses_sret: false,
        const_scratch: 0,
        ref_descs: Default::default(),
    };

    // Emit blocks.
    for block in &unit.blocks {
        ctx.emit_block(out, block)?;
    }

    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    Ok(())
}

/// Context for generating code within a function.
struct FunctionCodegenContext<'a> {
    unit: &'a IrCodeUnit,
    /// Parent unit for local function lookup (script unit for nested functions).
    parent_unit: Option<&'a IrCodeUnit>,
    layout: &'a FrameLayout,
    compiler: &'a mut CAotCompiler,
    registry: &'a FunctionRegistry,
    uses_sret: bool,
    /// How many scratch buffers a constant has asked for.
    ///
    /// A constant holding a collection builds each part in a buffer of its
    /// own, and a collection holding a collection nests those. C scopes by
    /// block, so a buffer named the same as the one outside it hides it: a
    /// list of lists pushed each inner list into its own scratch rather than
    /// into the list being built, and came out empty. The count makes each
    /// name its own.
    const_scratch: u32,
    /// What each reference points at, where its static type does not say.
    ///
    /// Carried in a `__rd{n}` declared in the prologue. See
    /// `datalove_datafun_ir::RefDesc`.
    ref_descs: std::collections::BTreeMap<ValueId, datalove_datafun_ir::RefDesc>,
}

impl<'a> FunctionCodegenContext<'a> {
    /// Emit a basic block.
    fn emit_block(
        &mut self,
        out: &mut String,
        block: &datalove_datafun_ir::IrBlock,
    ) -> Result<(), CAotError> {
        // Emit block label.
        writeln!(out, "__block_{}:", block.id.0).unwrap();

        // Handle block parameters (for loops).
        // Block params are initialized by goto/branch - we just need to mark their locations.
        // The goto/branch copies data to the param locations before jumping.

        // Emit instructions.
        for inst in &block.instructions {
            self.emit_instruction(out, inst)?;
        }

        // Emit terminator.
        self.emit_terminator(out, &block.terminator)?;

        Ok(())
    }

    /// Emit an instruction.
    fn emit_instruction(
        &mut self,
        out: &mut String,
        inst: &Instruction,
    ) -> Result<(), CAotError> {
        match inst {
            Instruction::Const { dest, value } => {
                self.emit_const(out, *dest, value)?;
            }
            Instruction::Copy { dest, src } | Instruction::Move { dest, src } => {
                self.emit_copy(out, *dest, src)?;
            }
            Instruction::Clone { dest, src } => {
                self.emit_clone(out, *dest, src)?;
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                self.emit_binop(out, *dest, *op, lhs, rhs)?;
            }
            Instruction::UnaryOp { dest, op, operand } => {
                self.emit_unaryop(out, *dest, *op, operand)?;
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                self.emit_binop_checked(out, *dest, *overflow, *op, lhs, rhs)?;
            }
            Instruction::UnaryOpChecked { dest, overflow, op, operand } => {
                self.emit_unaryop_checked(out, *dest, *overflow, *op, operand)?;
            }
            Instruction::Widen { dest, src } => {
                self.emit_widen(out, *dest, src)?;
            }
            Instruction::WidenFixed { dest, src } => {
                self.emit_widen_fixed(out, *dest, src)?;
            }
            Instruction::Pack { dest, ty: _, fields } => {
                self.emit_pack(out, *dest, fields)?;
            }
            Instruction::Unpack { dests, src } => {
                self.emit_unpack(out, dests, src)?;
            }
            Instruction::GetField { dest, src, field_index } => {
                self.emit_get_field(out, *dest, src, *field_index)?;
            }
            Instruction::GetFieldRef { dest, src, field_index } => {
                self.emit_get_field_ref(out, *dest, src, *field_index)?;
            }
            Instruction::DataBorrow { dest, src } => {
                self.emit_data_borrow(out, *dest, src)?;
            }
            Instruction::SetField { slot, field_path, value } => {
                self.emit_set_field(out, slot, field_path, value, false)?;
            }
            Instruction::SetFieldTracked { slot, field_path, value } => {
                self.emit_set_field(out, slot, field_path, value, true)?;
            }
            Instruction::WrapSome { dest, inner } => {
                self.emit_wrap_some(out, *dest, inner)?;
            }
            Instruction::WrapNone { dest } => {
                self.emit_wrap_none(out, *dest)?;
            }
            Instruction::WrapOk { dest, inner } => {
                self.emit_wrap_ok(out, *dest, inner)?;
            }
            Instruction::WrapErr { dest, inner } => {
                self.emit_wrap_err(out, *dest, inner)?;
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                self.emit_unwrap_option(out, *dest, *is_some, src)?;
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                self.emit_unwrap_result(out, *ok_dest, *err_dest, *is_ok, src)?;
            }
            Instruction::EnumVariant { dest, variant_index, payload } => {
                self.emit_enum_variant(out, *dest, *variant_index, payload.as_ref())?;
            }
            Instruction::EnumDiscriminant { dest, src } => {
                self.emit_enum_discriminant(out, *dest, src)?;
            }
            Instruction::EnumPayload { dest, src, variant_index } => {
                self.emit_enum_payload(out, *dest, src, *variant_index)?;
            }
            Instruction::ErrorFrom { dest, inner } => {
                self.emit_error_from(out, *dest, inner)?;
            }
            Instruction::DataFrom { dest, inner } => {
                self.emit_data_from(out, *dest, inner)?;
            }
            Instruction::Erase { dest, src } => {
                self.emit_erasure(out, *dest, src, true)?;
            }
            Instruction::EraseTracked { dest, src } => {
                self.emit_erasure_tracked(out, *dest, src)?;
            }
            Instruction::Reify { dest, src } => {
                self.emit_erasure(out, *dest, src, false)?;
            }
            Instruction::ListNew { dest, elements, descriptor } => {
                match descriptor {
                    Some(i) => self.emit_list_new_erased(out, *dest, elements, *i)?,
                    None => self.emit_list_new(out, *dest, elements)?,
                }
            }
            Instruction::SetNew { dest, elements, descriptor } => {
                match descriptor {
                    Some(i) => self.emit_set_new_erased(out, *dest, elements, *i)?,
                    None => self.emit_set_new(out, *dest, elements)?,
                }
            }
            Instruction::MapNew { dest, entries, descriptor } => {
                match descriptor {
                    Some(i) => self.emit_map_new_erased(out, *dest, entries, *i)?,
                    None => self.emit_map_new(out, *dest, entries)?,
                }
            }
            Instruction::TensorNew { dest, shape, elements } => {
                self.emit_tensor_new(out, *dest, shape, elements)?;
            }
            Instruction::TableNew { dest, rows } => {
                self.emit_table_new(out, *dest, rows)?;
            }
            Instruction::Call { dest, func, args, shape_descriptors, .. } => {
                self.emit_call(out, *dest, func, args, shape_descriptors)?;
            }
            // A comptime call is not generic, so it binds nothing.
            Instruction::ComptimeCall { dest, func, args, .. } => {
                self.emit_call(out, *dest, func, args, &[])?;
            }
            Instruction::SlotStoreCopy { dest, value } => {
                self.emit_slot_store(out, dest, value, true, false)?;
            }
            Instruction::SlotStoreCopyTracked { dest, value } => {
                self.emit_slot_store(out, dest, value, true, true)?;
            }
            Instruction::SlotStoreMove { dest, value } => {
                self.emit_slot_store(out, dest, value, false, false)?;
            }
            Instruction::SlotStoreMoveTracked { dest, value } => {
                self.emit_slot_store(out, dest, value, false, true)?;
            }
            Instruction::SlotLoadCopy { dest, slot } => {
                self.emit_slot_load(out, *dest, *slot, true)?;
            }
            Instruction::SlotLoadMove { dest, slot } => {
                self.emit_slot_load(out, *dest, *slot, false)?;
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                self.emit_slot_load(out, *dest, *slot, false)?;
                self.emit_mark_slot_moved(out, *slot)?;
            }
            Instruction::ParamStore { param, value } => {
                self.emit_param_store(out, *param, value, false)?;
            }
            Instruction::ParamStoreTracked { param, value } => {
                self.emit_param_store(out, *param, value, true)?;
            }
            Instruction::ParamSetField { param, field_path, value } => {
                self.emit_param_set_field(out, *param, field_path, value, false)?;
            }
            Instruction::ParamSetFieldTracked { param, field_path, value } => {
                self.emit_param_set_field(out, *param, field_path, value, true)?;
            }
            Instruction::RefStore { dest, value } => {
                self.emit_ref_store(out, dest, value, false)?;
            }
            Instruction::RefStoreTracked { dest, value } => {
                self.emit_ref_store(out, dest, value, true)?;
            }
            Instruction::RefSetField { dest, field_path, value } => {
                self.emit_ref_set_field(out, dest, field_path, value, false)?;
            }
            Instruction::RefSetFieldTracked { dest, field_path, value } => {
                self.emit_ref_set_field(out, dest, field_path, value, true)?;
            }
            Instruction::Drop { operand } => {
                self.emit_drop(out, operand, false)?;
            }
            Instruction::DropTracked { operand } => {
                self.emit_drop(out, operand, true)?;
            }
            Instruction::DropViaRef { ref_value } => {
                self.emit_drop_via_ref(out, *ref_value)?;
            }
            Instruction::UnitEndDrop { operand } => {
                self.emit_drop(out, operand, false)?;
            }
            Instruction::UnitEndDropTracked { operand } => {
                self.emit_drop(out, operand, true)?;
            }
            Instruction::DebugLog { operand } => {
                self.emit_debuglog(out, operand)?;
            }
            Instruction::Intrinsic { dest, intrinsic, args } => {
                self.emit_intrinsic(out, *dest, *intrinsic, args)?;
            }
            Instruction::ListGet { dest, is_valid, list, index } => {
                self.emit_list_get(out, *dest, *is_valid, list, index)?;
            }
            Instruction::ListBoundsCheck { is_valid, list, index } => {
                self.emit_list_bounds_check(out, *is_valid, list, index)?;
            }
            Instruction::ListSet { list, index, value } => {
                self.emit_list_set(out, list, index, value)?;
            }
            Instruction::ListElementRef { dest, list, index } => {
                self.emit_list_element_ref(out, *dest, list, index)?;
            }
            Instruction::MapGet { dest, is_valid, map, key } => {
                self.emit_map_get(out, *dest, *is_valid, map, key)?;
            }
            Instruction::MapContainsKey { is_valid, map, key } => {
                self.emit_map_contains_key(out, *is_valid, map, key)?;
            }
            Instruction::MapSetValue { map, key, value } => {
                self.emit_map_set_value(out, map, key, value)?;
            }
            Instruction::MapValueRef { dest, map, key } => {
                self.emit_map_value_ref(out, *dest, map, key)?;
            }
            Instruction::MapUpsert { map, key, value } => {
                self.emit_map_upsert(out, map, key, value)?;
            }
            Instruction::TensorGet { dest, is_valid, tensor, index } => {
                self.emit_tensor_get(out, *dest, *is_valid, tensor, index)?;
            }
            Instruction::TensorBoundsCheck { is_valid, tensor, index } => {
                self.emit_tensor_bounds_check(out, *is_valid, tensor, index)?;
            }
            Instruction::TensorSet { tensor, index, value } => {
                self.emit_tensor_set(out, tensor, index, value)?;
            }
            Instruction::TensorIndexRef { dest, tensor, index } => {
                self.emit_tensor_index_ref(out, *dest, tensor, index)?;
            }
            Instruction::Nop => {}
        }
        Ok(())
    }

    /// Get the address expression for a value.
    fn value_addr(&self, vid: ValueId) -> String {
        let offset = self.layout.value_offset(vid.0);
        format!("(__frame + {})", offset)
    }

    /// Get the address expression for a slot.
    fn slot_addr(&self, sid: SlotId) -> String {
        let offset = self.layout.slot_offset(sid.0);
        format!("(__frame + {})", offset)
    }

    /// Get the address expression for a param.
    fn param_addr(&self, pid: ParamId) -> String {
        format!("p{}", pid.0)
    }

    /// Get the address expression for an operand.
    fn operand_addr(&self, op: &Operand) -> String {
        match op {
            Operand::Value(vid) => self.value_addr(*vid),
            Operand::Param(pid) => self.param_addr(*pid),
            Operand::Slot(sid) => self.slot_addr(*sid),
            Operand::ValueRef(vid) => {
                // ValueRef: the value itself contains a pointer.
                format!("(*(void**){})", self.value_addr(*vid))
            }
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                "NULL /* external */".into()
            }
        }
    }

    /// Get the type of an operand (the type after any auto-dereference).
    ///
    /// For `ValueRef`, this returns the inner type (what the pointer points to),
    /// since ValueRef operands are auto-dereferenced when used.
    fn operand_type(&self, op: &Operand) -> &IrType {
        match op {
            Operand::Value(vid) => {
                &self.unit.value_types[vid.0 as usize]
            }
            Operand::ValueRef(vid) => {
                // ValueRef is auto-dereferenced, so return the inner type.
                let ref_ty = &self.unit.value_types[vid.0 as usize];
                match ref_ty {
                    IrType::Ref(inner) => inner.as_ref(),
                    _ => ref_ty, // Shouldn't happen, but fallback to the type itself.
                }
            }
            Operand::Param(pid) => {
                let func_ctx = self.unit.function_context().unwrap();
                &func_ctx.param_types[pid.0 as usize]
            }
            Operand::Slot(sid) => {
                &self.unit.slot_types[sid.0 as usize]
            }
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                &IrType::Unit
            }
        }
    }

    /// Get the type of a value.
    fn value_type(&self, vid: ValueId) -> &IrType {
        &self.unit.value_types[vid.0 as usize]
    }

    /// Get the type of a slot.
    fn slot_type(&self, sid: SlotId) -> &IrType {
        &self.unit.slot_types[sid.0 as usize]
    }

    /// Get the tydesc name for a type.
    fn tydesc_name(&mut self, ty: &IrType) -> String {
        self.compiler.get_tydesc_name(ty)
    }

    /// Emit a constant into a value.
    fn emit_const(&mut self, out: &mut String, dest: ValueId, value: &ConstValue) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let ty = self.value_type(dest).clone();
        self.emit_const_at(out, &dest_addr, &ty, value)
    }

    /// A scratch buffer name nothing else is using.
    fn next_const_scratch(&mut self) -> String {
        self.const_scratch += 1;
        format!("__cs{}", self.const_scratch)
    }

    /// Emit a constant into an address.
    ///
    /// Written against an address rather than a value so that a constant made
    /// of constants can put its parts somewhere: a list's elements go into a
    /// scratch buffer one at a time on their way into the list.
    fn emit_const_at(
        &mut self,
        out: &mut String,
        dest_addr: &str,
        ty: &IrType,
        value: &ConstValue,
    ) -> Result<(), CAotError> {
        let ty = ty.clone();
        let ty = &ty;

        match value {
            ConstValue::Unit => {
                // Nothing to store for Unit.
            }
            ConstValue::Bool(b) => {
                writeln!(out, "    *(bool_t*){} = {};", dest_addr, if *b { 1 } else { 0 }).unwrap();
            }
            ConstValue::U8(v) => {
                writeln!(out, "    *(uint8_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::I8(v) => {
                writeln!(out, "    *(int8_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::U16(v) => {
                writeln!(out, "    *(uint16_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::I16(v) => {
                writeln!(out, "    *(int16_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::U32(v) => {
                writeln!(out, "    *(uint32_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::I32(v) => {
                writeln!(out, "    *(int32_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::U64(v) => {
                writeln!(out, "    *(uint64_t*){} = {}ULL;", dest_addr, v).unwrap();
            }
            ConstValue::I64(v) => {
                writeln!(out, "    *(int64_t*){} = {}LL;", dest_addr, v).unwrap();
            }
            ConstValue::Index(v) => {
                writeln!(out, "    *(index_t*){} = {};", dest_addr, v).unwrap();
            }
            ConstValue::Offset(v) => {
                writeln!(out, "    *(offset_t*){} = {};", dest_addr, v).unwrap();
            }
            // A float goes across as its bits rather than as a number written
            // out and read back. Decimal loses nothing for most values but
            // says nothing about the rest: a literal is allowed to name a
            // particular NaN by its bit pattern, and `0.0/0.0` produces
            // whichever one the target likes, which on x86_64 has the sign bit
            // set where the literal did not.
            ConstValue::F32(v) => {
                writeln!(out, "    {{ uint32_t __bits = {:#010x}u; memcpy({}, &__bits, 4); }}",
                    v.0.to_bits(), dest_addr).unwrap();
            }
            ConstValue::F64(v) => {
                writeln!(out, "    {{ uint64_t __bits = {:#018x}ull; memcpy({}, &__bits, 8); }}",
                    v.0.to_bits(), dest_addr).unwrap();
            }
            ConstValue::Int { limbs, negative } => {
                let ty = ty.clone();
                let tydesc = self.tydesc_name(&ty);
                if limbs.is_empty() {
                    // Zero.
                    writeln!(out, "    dtlv_rti_int_from_limbs(rt, NULL, 0, 0, {}, &{});", dest_addr, tydesc).unwrap();
                } else {
                    // Emit limbs as static array.
                    let limbs_str: Vec<String> = limbs.iter().map(|l| format!("0x{:08x}", l)).collect();
                    writeln!(out, "    {{ static const uint32_t __limbs[] = {{ {} }}; dtlv_rti_int_from_limbs(rt, __limbs, {}, {}, {}, &{}); }}",
                        limbs_str.join(", "), limbs.len(), if *negative { 1 } else { 0 }, dest_addr, tydesc).unwrap();
                }
            }
            ConstValue::String(s) => {
                let ty = ty.clone();
                let tydesc = self.tydesc_name(&ty);
                let bytes = s.as_bytes();
                if bytes.is_empty() {
                    writeln!(out, "    dtlv_rti_string_from_bytes(rt, NULL, 0, {}, &{});", dest_addr, tydesc).unwrap();
                } else {
                    // Emit string bytes.
                    let hex_bytes: Vec<String> = bytes.iter().map(|b| format!("0x{:02x}", b)).collect();
                    writeln!(out, "    {{ static const uint8_t __str[] = {{ {} }}; dtlv_rti_string_from_bytes(rt, __str, {}, {}, &{}); }}",
                        hex_bytes.join(", "), bytes.len(), dest_addr, tydesc).unwrap();
                }
            }
            ConstValue::List(elements) => {
                let IrType::List(elem_ty) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a list constant wants a list type, not {:?}", ty)));
                };
                let elem_ty = (**elem_ty).clone();
                let list_tydesc = self.tydesc_name(ty);
                let elem_tydesc = self.tydesc_name(&elem_ty);
                let elem_size = types::ir_type_to_crepr(&elem_ty).layout().size.max(1);

                writeln!(out, "    dtlv_rti_list_create_local(rt, {}, &{});",
                    dest_addr, list_tydesc).unwrap();
                for element in elements {
                    // Each element is built in a scratch buffer and pushed,
                    // which moves it into the list. The buffer goes out of
                    // scope with nothing left in it to release.
                    let scratch = self.next_const_scratch();
                    writeln!(out, "    {{ _Alignas(8) uint8_t {}[{}] = {{0}};", scratch, elem_size).unwrap();
                    self.emit_const_at(out, &scratch, &elem_ty, element)?;
                    writeln!(out, "    dtlv_rti_list_push_local(rt, {}, &{}, {}, &{}); }}",
                        dest_addr, list_tydesc, scratch, elem_tydesc).unwrap();
                }
            }
            // An atom is one value of a type that has only that value, so it
            // occupies nothing and there is nothing to write.
            ConstValue::Enum { .. } if matches!(ty, IrType::Atom(_)) => {}
            ConstValue::Enum { variant, payload } => {
                let IrType::Enum(variants) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "an enum constant wants an enum type, not {:?}", ty)));
                };
                let index = variants.iter().position(|(name, _)| name == variant)
                    .ok_or_else(|| CAotError::Codegen(format!(
                        "enum constant names variant '{}', which {:?} does not have",
                        variant, ty)))?;
                writeln!(out, "    *(uint32_t*){} = {};", dest_addr, index).unwrap();

                if let Some(payload) = payload {
                    let payload_ty = variants[index].1.clone()
                        .ok_or_else(|| CAotError::Codegen(format!(
                            "enum constant gives variant '{}' a payload it does not take",
                            variant)))?;
                    let offset = ir_layout::enum_payload_offset(&payload_ty);
                    let payload_addr = format!("({} + {})", dest_addr, offset);
                    self.emit_const_at(out, &payload_addr, &payload_ty, payload)?;
                }
            }
            ConstValue::Tuple(elements) => {
                let IrType::Tuple(field_types) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a tuple constant wants a tuple type, not {:?}", ty)));
                };
                let offsets = ir_layout::aggregate_field_offsets(field_types);
                for (i, element) in elements.iter().enumerate() {
                    let field_ty = field_types[i].clone();
                    let field_addr = format!("({} + {})", dest_addr, offsets[i]);
                    self.emit_const_at(out, &field_addr, &field_ty, element)?;
                }
            }
            ConstValue::Struct(fields) => {
                let IrType::Struct(field_types) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a struct constant wants a struct type, not {:?}", ty)));
                };
                // The type's fields are in the order the layout uses, which is
                // what the offsets are computed from; the value's are matched
                // to them by name.
                let ordered: Vec<IrType> =
                    field_types.iter().map(|(_, ty)| ty.clone()).collect();
                let offsets = ir_layout::aggregate_field_offsets(&ordered);
                for (name, field_value) in fields {
                    let index = field_types.iter().position(|(n, _)| n == name)
                        .ok_or_else(|| CAotError::Codegen(format!(
                            "struct constant names field '{}', which {:?} does not have",
                            name, ty)))?;
                    let field_addr = format!("({} + {})", dest_addr, offsets[index]);
                    self.emit_const_at(out, &field_addr, &ordered[index], field_value)?;
                }
            }
            ConstValue::OptionNone => {
                writeln!(out, "    *(uint8_t*){} = 1;", dest_addr).unwrap();
            }
            ConstValue::OptionSome(inner) => {
                let IrType::Option(inner_ty) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "an option constant wants an option type, not {:?}", ty)));
                };
                writeln!(out, "    *(uint8_t*){} = 2;", dest_addr).unwrap();
                let offset = ir_layout::option_payload_offset(inner_ty);
                let payload_addr = format!("({} + {})", dest_addr, offset);
                self.emit_const_at(out, &payload_addr, inner_ty, inner)?;
            }
            ConstValue::ResultOk(inner) => {
                let IrType::Result(ok_ty) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a result constant wants a result type, not {:?}", ty)));
                };
                writeln!(out, "    *(uint8_t*){} = 1;", dest_addr).unwrap();
                let offset = ir_layout::result_payload_offset(ok_ty);
                let payload_addr = format!("({} + {})", dest_addr, offset);
                self.emit_const_at(out, &payload_addr, ok_ty, inner)?;
            }
            ConstValue::ResultErr(inner) => {
                let IrType::Result(ok_ty) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a result constant wants a result type, not {:?}", ty)));
                };
                writeln!(out, "    *(uint8_t*){} = 2;", dest_addr).unwrap();
                let offset = ir_layout::result_payload_offset(ok_ty);
                let payload_addr = format!("({} + {})", dest_addr, offset);
                // The error side is an `error` whatever the ok side is.
                self.emit_const_at(out, &payload_addr, &IrType::Error, inner)?;
            }
            // A `data` and an `error` are built from what they hold, which
            // says its own type: the value goes into a scratch buffer and the
            // runtime packs it, taking it from there.
            ConstValue::Data { payload_type, value: inner }
            | ConstValue::Error { payload_type, value: inner } => {
                // The type the value was read back as, rather than one worked
                // out from the value: an empty collection cannot say what it
                // holds, and the descriptor a `data` carries has to be one
                // this unit emitted.
                let inner_ty = (**payload_type).clone();
                let inner_tydesc = self.tydesc_name(&inner_ty);
                let inner_size = types::ir_type_to_crepr(&inner_ty).layout().size.max(1);
                let pack = match value {
                    ConstValue::Data { .. } => "dtlv_rti_data_from_local",
                    _ => "dtlv_rti_error_from_local",
                };
                let scratch = self.next_const_scratch();
                writeln!(out, "    {{ _Alignas(8) uint8_t {}[{}] = {{0}};", scratch, inner_size).unwrap();
                self.emit_const_at(out, &scratch, &inner_ty, inner)?;
                writeln!(out, "    {}(rt, {}, &{}, {}); }}", pack, scratch, inner_tydesc, dest_addr).unwrap();
            }
            ConstValue::Set(elements) => {
                let IrType::Set(elem_ty) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a set constant wants a set type, not {:?}", ty)));
                };
                let elem_ty = (**elem_ty).clone();
                let set_tydesc = self.tydesc_name(ty);
                let elem_tydesc = self.tydesc_name(&elem_ty);
                let elem_size = types::ir_type_to_crepr(&elem_ty).layout().size.max(1);

                writeln!(out, "    dtlv_rti_btreeset_create_local(rt, {}, &{});",
                    dest_addr, set_tydesc).unwrap();
                for element in elements {
                    // Inserted one at a time, as a literal is, so that the set
                    // holds each element once and lets go of a duplicate.
                    let scratch = self.next_const_scratch();
                    writeln!(out, "    {{ _Alignas(8) uint8_t {}[{}] = {{0}}; uint8_t {}_added = 0;",
                        scratch, elem_size, scratch).unwrap();
                    self.emit_const_at(out, &scratch, &elem_ty, element)?;
                    writeln!(out, "    dtlv_rti_btreeset_insert_local(rt, {}, &{}, {}, &{}, &{}_added); }}",
                        dest_addr, set_tydesc, scratch, elem_tydesc, scratch).unwrap();
                }
            }
            ConstValue::Map(entries) => {
                let IrType::Map(key_ty, value_ty) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a map constant wants a map type, not {:?}", ty)));
                };
                let key_ty = (**key_ty).clone();
                let value_ty = (**value_ty).clone();
                let map_tydesc = self.tydesc_name(ty);
                let key_tydesc = self.tydesc_name(&key_ty);
                let value_tydesc = self.tydesc_name(&value_ty);
                let key_size = types::ir_type_to_crepr(&key_ty).layout().size.max(1);
                let value_size = types::ir_type_to_crepr(&value_ty).layout().size.max(1);

                writeln!(out, "    dtlv_rti_btreemap_create_local(rt, {}, &{});",
                    dest_addr, map_tydesc).unwrap();
                for (key, entry_value) in entries {
                    let key_scratch = self.next_const_scratch();
                    let value_scratch = self.next_const_scratch();
                    writeln!(out, "    {{ _Alignas(8) uint8_t {}[{}] = {{0}};", key_scratch, key_size).unwrap();
                    self.emit_const_at(out, &key_scratch, &key_ty, key)?;
                    writeln!(out, "    _Alignas(8) uint8_t {}[{}] = {{0}};", value_scratch, value_size).unwrap();
                    self.emit_const_at(out, &value_scratch, &value_ty, entry_value)?;
                    writeln!(out, "    dtlv_rti_btreemap_insert_local(rt, {}, &{}, {}, &{}, {}, &{}); }}",
                        dest_addr, map_tydesc, key_scratch, key_tydesc, value_scratch, value_tydesc).unwrap();
                }
            }

            ConstValue::Tensor { shape, elements } => {
                let IrType::Tensor(elem_ty, rank) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a tensor constant wants a tensor type, not {:?}", ty)));
                };
                let elem_ty = (**elem_ty).clone();
                let rank = *rank;
                if shape.len() as u32 != rank {
                    return Err(CAotError::Codegen(format!(
                        "a tensor constant of rank {} was given a shape of {}",
                        rank, shape.len())));
                }
                let tensor_tydesc = self.tydesc_name(ty);
                let elem_tydesc = self.tydesc_name(&elem_ty);
                let elem_size = types::ir_type_to_crepr(&elem_ty).layout().size.max(1);

                // The elements go into one run and the runtime takes them from
                // there, along with the shape saying how they group. The same
                // call a tensor literal makes.
                let scratch = self.next_const_scratch();
                writeln!(out, "    {{ _Alignas(8) uint8_t {}[{}] = {{0}};",
                    scratch, (elem_size * elements.len() as u32).max(1)).unwrap();
                for (i, element) in elements.iter().enumerate() {
                    let elem_addr = format!("({} + {})", scratch, i as u32 * elem_size);
                    self.emit_const_at(out, &elem_addr, &elem_ty, element)?;
                }
                // An extent is a `u32` in the IR and the runtime reads it as
                // one, whatever width an index happens to be.
                let extents: Vec<String> = shape.iter().map(|e| e.to_string()).collect();
                writeln!(out, "    static const uint32_t {}_shape[] = {{ {} }};",
                    scratch, if extents.is_empty() { "0".to_string() } else { extents.join(", ") }).unwrap();
                writeln!(out, "    dtlv_rti_tensor_init_local(rt, {}, {}, &{}, {}_shape, {}, {}, &{}); }}",
                    scratch, elements.len(), elem_tydesc, scratch, rank, dest_addr, tensor_tydesc).unwrap();
            }

            ConstValue::Table { rows, .. } => {
                let IrType::Table(columns) = ty else {
                    return Err(CAotError::Codegen(format!(
                        "a table constant wants a table type, not {:?}", ty)));
                };
                let table_tydesc = self.tydesc_name(ty);

                // A row is a tuple of the column types, laid out as one, and
                // the runtime is handed a descriptor for it along with the run
                // of rows. Cells go in the order the columns were written,
                // which is the order every layer keeps them in.
                let col_types: Vec<IrType> =
                    columns.iter().map(|(_, ty)| (**ty).clone()).collect();
                let row_ty = IrType::Tuple(col_types.clone());
                let row_tydesc = self.tydesc_name(&row_ty);
                let offsets = ir_layout::aggregate_field_offsets(&col_types);
                let row_size = types::ir_type_to_crepr(&row_ty).layout().size;

                if rows.is_empty() {
                    writeln!(out, "    dtlv_rti_table_create_local(rt, {}, &{});",
                        dest_addr, table_tydesc).unwrap();
                } else {
                    let scratch = self.next_const_scratch();
                    writeln!(out, "    {{ _Alignas(8) uint8_t {}[{}] = {{0}};",
                        scratch, (row_size * rows.len() as u32).max(1)).unwrap();
                    for (row_index, row) in rows.iter().enumerate() {
                        let row_base = row_index as u32 * row_size;
                        for (column, cell) in row.iter().enumerate() {
                            let cell_addr =
                                format!("({} + {})", scratch, row_base + offsets[column]);
                            self.emit_const_at(out, &cell_addr, &col_types[column], cell)?;
                        }
                    }
                    writeln!(out, "    dtlv_rti_table_build_from_rows_local(rt, {}, &{}, {}, &{}, {}); }}",
                        dest_addr, table_tydesc, scratch, row_tydesc, rows.len()).unwrap();
                }
            }
        }
        Ok(())
    }

    /// Emit a copy/move.
    fn emit_copy(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        let ty = self.operand_type(src);
        let repr = types::ir_type_to_crepr(ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, layout.size).unwrap();
                }
            }
        }
        Ok(())
    }

    /// Emit a clone (deep copy).
    fn emit_clone(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        // The source's descriptor says what is really there, which inside a
        // generic its static type does not, and the destination's says what
        // shape the clone has to arrive in. Where the two differ the value is
        // wrapped on the way, which the runtime decides rather than this.
        let src_tydesc = self.operand_tydesc(src);
        let dest_ty = self.value_type(dest).clone();
        let dest_tydesc = self.tydesc_name(&dest_ty);

        writeln!(out, "    dtlv_rti_clone_erased_local(rt, {}, {}, {}, &{});",
            src_addr, src_tydesc, dest_addr, dest_tydesc).unwrap();
        Ok(())
    }

    /// Emit a binary operation.
    fn emit_binop(&mut self, out: &mut String, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let lhs_addr = self.operand_addr(lhs);
        let rhs_addr = self.operand_addr(rhs);
        let lhs_ty = self.operand_type(lhs);
        let dest_ty = self.value_type(dest);

        // Check if this is an Int operation.
        if matches!(lhs_ty, IrType::Int) {
            return self.emit_int_binop(out, dest, op, lhs, rhs);
        }

        // An operand whose type only a descriptor says: a type parameter
        // bounded to `float`, fixed at one width or the other by the call site.
        // This body was compiled once and cannot hold both, so the runtime
        // reads the descriptor and picks.
        if matches!(lhs_ty, IrType::Data) || matches!(self.operand_type(rhs), IrType::Data) {
            let code = datalove_datafun_ir::dyn_op_code(op).ok_or_else(|| {
                CAotError::Codegen(format!("no dynamic form of {:?}", op))
            })? as u8;
            let lhs_td = self.operand_tydesc(lhs);
            let rhs_td = self.operand_tydesc(rhs);
            let dest_ty = self.value_type(dest).clone();
            let dest_td = self.tydesc_name(&dest_ty);
            writeln!(out, "    dtlv_rti_dyn_binop(rt, {code}, {lhs_addr}, {lhs_td}, \
                {rhs_addr}, {rhs_td}, {dest_addr}, &{dest_td});").unwrap();
            return Ok(());
        }

        let lhs_c_ty = types::ir_type_to_c(lhs_ty);
        let dest_c_ty = types::ir_type_to_c(dest_ty);

        let op_str = match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And | BinOp::LogicAnd => "&&",
            BinOp::Or | BinOp::LogicOr => "||",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor | BinOp::LogicXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        };

        writeln!(out, "    *({dest_c_ty}*){dest_addr} = (*({lhs_c_ty}*){lhs_addr}) {op_str} (*({lhs_c_ty}*){rhs_addr});").unwrap();
        Ok(())
    }

    /// Emit Int (bigint) binary operation.
    fn emit_int_binop(&mut self, out: &mut String, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let lhs_addr = self.operand_addr(lhs);
        let rhs_addr = self.operand_addr(rhs);
        let lhs_ty = self.operand_type(lhs).clone();
        let dest_ty = self.value_type(dest).clone();
        let lhs_tydesc = self.tydesc_name(&lhs_ty);
        let dest_tydesc = self.tydesc_name(&dest_ty);

        let func = match op {
            BinOp::Add => "dtlv_rti_int_add",
            BinOp::Sub => "dtlv_rti_int_sub",
            BinOp::Mul => "dtlv_rti_int_mul",
            BinOp::Div => "dtlv_rti_int_div_checked",
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
                // Comparison - use cmp and then test the RtOrdering tag.
                // RtOrdering is Less=1, Equal=2, Greater=3, so it must be
                // compared against those tags, not against zero.
                let test = match op {
                    BinOp::Lt => "__ord == ORD_LESS",
                    BinOp::Le => "__ord == ORD_LESS || __ord == ORD_EQUAL",
                    BinOp::Gt => "__ord == ORD_GREATER",
                    BinOp::Ge => "__ord == ORD_GREATER || __ord == ORD_EQUAL",
                    BinOp::Eq => "__ord == ORD_EQUAL",
                    BinOp::Ne => "__ord != ORD_EQUAL",
                    _ => unreachable!(),
                };
                writeln!(out, "    {{ int8_t __ord = dtlv_rti_cmp_local(rt, {}, &{}, {}, &{}); *(bool_t*){} = ({}); }}",
                    lhs_addr, lhs_tydesc, rhs_addr, lhs_tydesc, dest_addr, test).unwrap();
                return Ok(());
            }
            _ => return Err(CAotError::Unsupported(format!("Int binop {:?}", op))),
        };

        writeln!(out, "    {}(rt, {}, &{}, {}, &{}, {}, &{});",
            func, lhs_addr, lhs_tydesc, rhs_addr, lhs_tydesc, dest_addr, dest_tydesc).unwrap();
        Ok(())
    }

    /// Emit a unary operation.
    fn emit_unaryop(&mut self, out: &mut String, dest: ValueId, op: UnaryOp, operand: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(operand);
        let src_ty = self.operand_type(operand).clone();

        // Check if this is an Int operation.
        if matches!(src_ty, IrType::Int) {
            let dest_ty = self.value_type(dest).clone();
            let src_tydesc = self.tydesc_name(&src_ty);
            let dest_tydesc = self.tydesc_name(&dest_ty);
            match op {
                UnaryOp::Neg => {
                    writeln!(out, "    dtlv_rti_int_neg(rt, {}, &{}, {}, &{});",
                        src_addr, src_tydesc, dest_addr, dest_tydesc).unwrap();
                }
                _ => return Err(CAotError::Unsupported(format!("Int unaryop {:?}", op))),
            }
            return Ok(());
        }

        let c_ty = types::ir_type_to_c(&src_ty);

        let op_str = match op {
            UnaryOp::Neg => "-",
            UnaryOp::Not | UnaryOp::LogicNot => "!",
            UnaryOp::BitNot => "~",
        };

        writeln!(out, "    *({c_ty}*){dest_addr} = {op_str}(*({c_ty}*){src_addr});").unwrap();
        Ok(())
    }

    /// Emit a checked binary operation.
    fn emit_binop_checked(
        &mut self,
        out: &mut String,
        dest: ValueId,
        overflow: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let overflow_addr = self.value_addr(overflow);
        let lhs_addr = self.operand_addr(lhs);
        let rhs_addr = self.operand_addr(rhs);
        let lhs_ty = self.operand_type(lhs).clone();

        // A bigint has no width to overflow, so the only checked operation
        // over one is division, and what it reports is a zero divisor. The
        // runtime says so with its status, the same way it does for the
        // cranelift backends.
        if matches!(lhs_ty, IrType::Int) {
            if op != BinOp::Div {
                return Err(CAotError::Unsupported(format!(
                    "checked bigint binop only supports Div, got {:?}", op)));
            }
            let tydesc = self.tydesc_name(&IrType::Int);
            writeln!(out,
                "    *(bool_t*){} = dtlv_rti_int_div_checked(rt, {}, &{}, {}, &{}, {}, &{}) != 1;",
                overflow_addr, lhs_addr, tydesc, rhs_addr, tydesc, dest_addr, tydesc).unwrap();
            return Ok(());
        }

        // A type parameter bounded to `fixedint`, whose width and signedness
        // only the descriptor says. This body is emitted once and cannot hold
        // the arithmetic for all ten, so the runtime reads and picks.
        if matches!(lhs_ty, IrType::Data) {
            let code = datalove_datafun_ir::dyn_op_code(op).ok_or_else(|| {
                CAotError::Codegen(format!("no dynamic form of {:?}", op))
            })? as u8;
            let lhs_td = self.operand_tydesc(lhs);
            let rhs_td = self.operand_tydesc(rhs);
            let dest_ty = self.value_type(dest).clone();
            let dest_td = self.tydesc_name(&dest_ty);
            writeln!(out, "    dtlv_rti_dyn_binop_checked(rt, {code}, {lhs_addr}, {lhs_td}, \
                {rhs_addr}, {rhs_td}, {dest_addr}, &{dest_td}, (bool_t*){overflow_addr});").unwrap();
            return Ok(());
        }

        let c_ty = types::ir_type_to_c(&lhs_ty);

        // Use GCC/Clang builtins for overflow checking.
        // Index and Offset are platform-dependent sizes.
        #[cfg(not(feature = "index-64"))]
        let (index_add, index_sub, index_mul) = (
            "__builtin_uadd_overflow",
            "__builtin_usub_overflow",
            "__builtin_umul_overflow",
        );
        #[cfg(feature = "index-64")]
        let (index_add, index_sub, index_mul) = (
            "__builtin_uaddll_overflow",
            "__builtin_usubll_overflow",
            "__builtin_umulll_overflow",
        );
        #[cfg(not(feature = "index-64"))]
        let (offset_add, offset_sub, offset_mul) = (
            "__builtin_sadd_overflow",
            "__builtin_ssub_overflow",
            "__builtin_smul_overflow",
        );
        #[cfg(feature = "index-64")]
        let (offset_add, offset_sub, offset_mul) = (
            "__builtin_saddll_overflow",
            "__builtin_ssubll_overflow",
            "__builtin_smulll_overflow",
        );

        // Handle small integer types by widening to 32-bit.
        let (min_val, max_val): (Option<i64>, Option<i64>) = match lhs_ty {
            IrType::I8 => (Some(-128), Some(127)),
            IrType::I16 => (Some(-32768), Some(32767)),
            IrType::U8 => (Some(0), Some(255)),
            IrType::U16 => (Some(0), Some(65535)),
            _ => (None, None),
        };

        // Division and modulo are checked the same way at every width, and
        // every check has to come before the divide rather than after. This
        // is ahead of the widening below
        // because a narrow type would otherwise reach the fallback there and
        // divide unchecked, which faults rather than reporting.
        if matches!(op, BinOp::Div | BinOp::Mod) {
            // A signed type has a second divisor with no answer: its most
            // negative value over -1 is one past the top of the range. On x86
            // the divide instruction faults on it rather than wrapping, so it
            // has to be caught before the divide, the same as a zero divisor.
            // At eight and sixteen bits it does not fault, because both sides
            // promote to `int` first, but it does give the wrong answer.
            //
            // The remainder there is zero rather than an overflow, which is
            // what the interpreter and both cranelift backends report.
            let signed_min = match lhs_ty {
                IrType::I8 => Some("INT8_MIN"),
                IrType::I16 => Some("INT16_MIN"),
                IrType::I32 => Some("INT32_MIN"),
                IrType::I64 => Some("INT64_MIN"),
                IrType::Offset => Some("DTLV_OFFSET_MIN"),
                _ => None,
            };
            writeln!(out, "    if (*({c_ty}*){rhs_addr} == 0) {{").unwrap();
            writeln!(out, "        *(bool_t*){overflow_addr} = 1;").unwrap();
            writeln!(out, "        *({c_ty}*){dest_addr} = 0;").unwrap();
            if let Some(min) = signed_min {
                writeln!(out, "    }} else if (*({c_ty}*){lhs_addr} == {min} && *({c_ty}*){rhs_addr} == -1) {{").unwrap();
                writeln!(out, "        *(bool_t*){overflow_addr} = {};",
                    if op == BinOp::Div { 1 } else { 0 }).unwrap();
                writeln!(out, "        *({c_ty}*){dest_addr} = 0;").unwrap();
            }
            writeln!(out, "    }} else {{").unwrap();
            writeln!(out, "        *(bool_t*){overflow_addr} = 0;").unwrap();
            writeln!(out, "        *({c_ty}*){dest_addr} = *({c_ty}*){lhs_addr} {} *({c_ty}*){rhs_addr};",
                if op == BinOp::Div { "/" } else { "%" }).unwrap();
            writeln!(out, "    }}").unwrap();
            return Ok(());
        }

        if let (Some(min_val), Some(max_val)) = (min_val, max_val) {
            // Widen to 32-bit, do operation, check range.
            let op_str = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                _ => {
                    // Fall back to unchecked for other ops.
                    self.emit_binop(out, dest, op, lhs, rhs)?;
                    writeln!(out, "    *(bool_t*){} = 0;", overflow_addr).unwrap();
                    return Ok(());
                }
            };
            let is_signed = matches!(lhs_ty, IrType::I8 | IrType::I16);
            let wide_ty = if is_signed { "int32_t" } else { "uint32_t" };
            writeln!(out, "    {{ {wide_ty} __tmp = ({wide_ty})*({c_ty}*){lhs_addr} {op_str} ({wide_ty})*({c_ty}*){rhs_addr}; *(bool_t*){overflow_addr} = (__tmp < {min_val} || __tmp > {max_val}); *({c_ty}*){dest_addr} = ({c_ty})__tmp; }}").unwrap();
            return Ok(());
        }

        let builtin = match (lhs_ty, op) {
            (IrType::I32, BinOp::Add) => "__builtin_sadd_overflow",
            (IrType::I32, BinOp::Sub) => "__builtin_ssub_overflow",
            (IrType::I32, BinOp::Mul) => "__builtin_smul_overflow",
            (IrType::I64, BinOp::Add) => "__builtin_saddll_overflow",
            (IrType::I64, BinOp::Sub) => "__builtin_ssubll_overflow",
            (IrType::I64, BinOp::Mul) => "__builtin_smulll_overflow",
            (IrType::U32, BinOp::Add) => "__builtin_uadd_overflow",
            (IrType::U32, BinOp::Sub) => "__builtin_usub_overflow",
            (IrType::U32, BinOp::Mul) => "__builtin_umul_overflow",
            (IrType::U64, BinOp::Add) => "__builtin_uaddll_overflow",
            (IrType::U64, BinOp::Sub) => "__builtin_usubll_overflow",
            (IrType::U64, BinOp::Mul) => "__builtin_umulll_overflow",
            (IrType::Index, BinOp::Add) => index_add,
            (IrType::Index, BinOp::Sub) => index_sub,
            (IrType::Index, BinOp::Mul) => index_mul,
            (IrType::Offset, BinOp::Add) => offset_add,
            (IrType::Offset, BinOp::Sub) => offset_sub,
            (IrType::Offset, BinOp::Mul) => offset_mul,
            _ => {
                // Fall back to unchecked operation.
                self.emit_binop(out, dest, op, lhs, rhs)?;
                writeln!(out, "    *(bool_t*){} = 0;", overflow_addr).unwrap();
                return Ok(());
            }
        };

        writeln!(out, "    *(bool_t*){} = {}(*({c_ty}*){}, *({c_ty}*){}, ({c_ty}*){});",
            overflow_addr, builtin, lhs_addr, rhs_addr, dest_addr).unwrap();
        Ok(())
    }

    /// Emit a checked unary operation.
    fn emit_unaryop_checked(
        &mut self,
        out: &mut String,
        dest: ValueId,
        overflow: ValueId,
        op: UnaryOp,
        operand: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let overflow_addr = self.value_addr(overflow);
        let src_addr = self.operand_addr(operand);
        let src_ty = self.operand_type(operand).clone();

        // A type parameter bounded to `fixedint`, which may turn out to be
        // unsigned; the runtime reads the descriptor and decides.
        if matches!(src_ty, IrType::Data) {
            if op != UnaryOp::Neg {
                return Err(CAotError::Unsupported(format!(
                    "no dynamic form of checked {:?}", op)));
            }
            let src_td = self.operand_tydesc(operand);
            let dest_ty = self.value_type(dest).clone();
            let dest_td = self.tydesc_name(&dest_ty);
            writeln!(out, "    dtlv_rti_dyn_neg_checked(rt, {src_addr}, {src_td}, \
                {dest_addr}, &{dest_td}, (bool_t*){overflow_addr});").unwrap();
            return Ok(());
        }

        let c_ty = types::ir_type_to_c(&src_ty);
        let src_ty = &src_ty;

        match op {
            UnaryOp::Neg => {
                // Check for overflow on negation (only INT_MIN for signed types).
                match src_ty {
                    IrType::I32 => {
                        writeln!(out, "    {{ int32_t __v = *(int32_t*){}; *(bool_t*){} = (__v == INT32_MIN); *(int32_t*){} = -__v; }}",
                            src_addr, overflow_addr, dest_addr).unwrap();
                    }
                    IrType::I64 => {
                        writeln!(out, "    {{ int64_t __v = *(int64_t*){}; *(bool_t*){} = (__v == INT64_MIN); *(int64_t*){} = -__v; }}",
                            src_addr, overflow_addr, dest_addr).unwrap();
                    }
                    IrType::Offset => {
                        writeln!(out, "    {{ offset_t __v = *(offset_t*){}; *(bool_t*){} = (__v == DTLV_OFFSET_MIN); *(offset_t*){} = -__v; }}",
                            src_addr, overflow_addr, dest_addr).unwrap();
                    }
                    _ => {
                        // Unsigned negation doesn't overflow in the same way.
                        writeln!(out, "    *({c_ty}*){dest_addr} = -(*({c_ty}*){src_addr});").unwrap();
                        writeln!(out, "    *(bool_t*){} = 0;", overflow_addr).unwrap();
                    }
                }
            }
            _ => {
                // Other unary ops don't overflow.
                self.emit_unaryop(out, dest, op, operand)?;
                writeln!(out, "    *(bool_t*){} = 0;", overflow_addr).unwrap();
            }
        }
        Ok(())
    }

    /// Emit widen (fixed to Int).
    fn emit_widen(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        let src_ty = self.operand_type(src).clone();
        let dest_ty = self.value_type(dest).clone();
        let src_tydesc = self.tydesc_name(&src_ty);
        let dest_tydesc = self.tydesc_name(&dest_ty);

        writeln!(out, "    dtlv_rti_int_from_fixed(rt, {}, &{}, {}, &{});",
            src_addr, src_tydesc, dest_addr, dest_tydesc).unwrap();
        Ok(())
    }

    /// Emit widen fixed (fixed to larger fixed).
    fn emit_widen_fixed(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        let src_ty = self.operand_type(src).clone();
        let dest_ty = self.value_type(dest).clone();
        let src_c_ty = types::ir_type_to_c(&src_ty);
        let dest_c_ty = types::ir_type_to_c(&dest_ty);

        // Simple cast - C handles sign/zero extension.
        writeln!(out, "    *({dest_c_ty}*){dest_addr} = ({dest_c_ty})(*({src_c_ty}*){src_addr});").unwrap();
        Ok(())
    }

    /// Emit pack (tuple/struct construction).
    fn emit_pack(&mut self, out: &mut String, dest: ValueId, fields: &[Operand]) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest);

        // Unit is a zero-size type - nothing to pack.
        if matches!(dest_ty, IrType::Unit) {
            return Ok(());
        }

        let field_types: Vec<IrType> = match dest_ty {
            IrType::Tuple(tys) => tys.clone(),
            IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
            _ => return Err(CAotError::Codegen(format!("pack requires tuple/struct type, got {:?}", dest_ty))),
        };

        let offsets = types::compute_tuple_field_offsets(&field_types);

        for (i, field_op) in fields.iter().enumerate() {
            let field_offset = offsets[i];
            let field_ty = &field_types[i];
            let repr = types::ir_type_to_crepr(field_ty);
            let field_addr = format!("({} + {})", dest_addr, field_offset);
            let src_addr = self.operand_addr(field_op);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){field_addr} = *({c_ty}*){src_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "    memcpy({}, {}, {});", field_addr, src_addr, layout.size).unwrap();
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit unpack (tuple/struct destruction).
    fn emit_unpack(&mut self, out: &mut String, dests: &[ValueId], src: &Operand) -> Result<(), CAotError> {
        let src_addr = self.operand_addr(src);
        let src_ty = self.operand_type(src);

        let field_types: Vec<IrType> = match src_ty {
            IrType::Tuple(tys) => tys.clone(),
            IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
            _ => return Err(CAotError::Codegen("unpack requires tuple/struct type".into())),
        };

        let offsets = types::compute_tuple_field_offsets(&field_types);

        for (i, dest_vid) in dests.iter().enumerate() {
            let field_offset = offsets[i];
            let field_ty = &field_types[i];
            let repr = types::ir_type_to_crepr(field_ty);
            let field_addr = format!("({} + {})", src_addr, field_offset);
            let dest_addr = self.value_addr(*dest_vid);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){field_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "    memcpy({}, {}, {});", dest_addr, field_addr, layout.size).unwrap();
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit get field (move field out of aggregate).
    fn emit_get_field(&mut self, out: &mut String, dest: ValueId, src: &Operand, field_index: u32) -> Result<(), CAotError> {
        let src_addr = self.operand_addr(src);
        let src_ty = self.operand_type(src);
        let dest_addr = self.value_addr(dest);

        // A base whose static type does not describe it has the field's offset
        // and the field's own type read from the descriptor of what arrived.
        // The runtime does both, and decides there whether the value wants
        // packing on the way out; see `dtlv_rti_field_read_local`.
        if let Some(base_desc) = self.operand_ref_desc(src) {
            let dest_ty = self.value_type(dest).clone();
            let dest_tydesc = self.tydesc_name(&dest_ty);
            writeln!(out, "    dtlv_rti_field_read_local(rt, {}, &{}, {}, {}, {});",
                dest_addr, dest_tydesc, src_addr, base_desc, field_index).unwrap();
            return Ok(());
        }

        let field_types: Vec<IrType> = match src_ty {
            IrType::Tuple(tys) => tys.clone(),
            IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
            _ => return Err(CAotError::Codegen("get_field requires tuple/struct type".into())),
        };

        let offsets = types::compute_tuple_field_offsets(&field_types);
        let field_offset = offsets[field_index as usize];
        let field_ty = &field_types[field_index as usize];
        let repr = types::ir_type_to_crepr(field_ty);
        let field_addr = format!("({} + {})", src_addr, field_offset);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){field_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, field_addr, layout.size).unwrap();
                }
            }
        }
        Ok(())
    }

    /// Emit a data borrow: point at the container a `data` holds, and carry
    /// the descriptor it holds it under.
    ///
    /// A container is never packed into the two words -- `can_inline` admits
    /// none -- so the wrapper always has something to point at and the scratch
    /// `data_borrow` wants for an inline value goes unused.
    fn emit_data_borrow(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        let name = format!("__db{}", dest.0);
        writeln!(out, "    {{ const void* {name}_v; uint64_t {name}_s;").unwrap();
        writeln!(out, "    dtlv_rti_data_borrow({}, &{name}_s, &{name}_v, &__rd{});",
            src_addr, dest.0).unwrap();
        writeln!(out, "    *(void**){} = (void*){name}_v; }}", dest_addr).unwrap();
        Ok(())
    }

    /// Emit get field ref (get pointer to field).
    fn emit_get_field_ref(&mut self, out: &mut String, dest: ValueId, src: &Operand, field_index: u32) -> Result<(), CAotError> {
        let src_addr = self.operand_addr(src);
        let src_ty = self.operand_type(src);
        let dest_addr = self.value_addr(dest);

        // A reference whose static type says `data` where a type parameter
        // stood lies about the layout, so the offset comes from the descriptor
        // of what really arrived, and the field's own descriptor goes on to
        // describe this reference. Both are in there already: building a
        // descriptor is what settled the offsets.
        if let Some(base_desc) = self.operand_ref_desc(src) {
            writeln!(out, "    __rd{} = dtlv_rti_field_tydesc({}, {});",
                dest.0, base_desc, field_index).unwrap();
            writeln!(out, "    *(void**){} = (uint8_t*){} + dtlv_rti_field_offset({}, {});",
                dest_addr, src_addr, base_desc, field_index).unwrap();
            return Ok(());
        }

        let field_types: Vec<IrType> = match src_ty {
            IrType::Tuple(tys) => tys.clone(),
            IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
            IrType::Ref(inner) => match inner.as_ref() {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => return Err(CAotError::Codegen("get_field_ref requires tuple/struct type".into())),
            },
            _ => return Err(CAotError::Codegen("get_field_ref requires tuple/struct type".into())),
        };

        let offsets = types::compute_tuple_field_offsets(&field_types);
        let field_offset = offsets[field_index as usize];

        // Store pointer to field.
        writeln!(out, "    *(void**){} = {} + {};", dest_addr, src_addr, field_offset).unwrap();
        Ok(())
    }

    /// Emit set field.
    fn emit_set_field(
        &mut self,
        out: &mut String,
        slot: &SlotDest,
        field_path: &[u32],
        value: &Operand,
        tracked: bool,
    ) -> Result<(), CAotError> {
        let (slot_addr, slot_id) = match slot {
            SlotDest::Local(sid) => (self.slot_addr(*sid), Some(*sid)),
            SlotDest::External { .. } => {
                return Err(CAotError::Unsupported("external slot".into()));
            }
        };

        // Navigate to the target field.
        let mut current_addr = slot_addr.clone();
        let mut current_ty = match slot {
            SlotDest::Local(sid) => self.slot_type(*sid).clone(),
            SlotDest::External { .. } => IrType::Unit,
        };

        for &idx in field_path {
            let field_types: Vec<IrType> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => return Err(CAotError::Codegen("set_field requires tuple/struct type".into())),
            };
            let offsets = types::compute_tuple_field_offsets(&field_types);
            let field_offset = offsets[idx as usize];
            current_addr = format!("({} + {})", current_addr, field_offset);
            current_ty = field_types[idx as usize].clone();
        }

        // Copy value to field.
        let src_addr = self.operand_addr(value);
        let repr = types::ir_type_to_crepr(&current_ty);

        // The field holds a live value, so destroy it before overwriting.
        // This applies to the tracked form too: the tracking byte covers the
        // whole slot, which is already live whenever a field of it is assigned.
        if !current_ty.is_copy() {
            let tydesc = self.tydesc_name(&current_ty);
            writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", current_addr, tydesc).unwrap();
        }

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){current_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", current_addr, src_addr, layout.size).unwrap();
                }
            }
        }

        // Mark tracking byte if tracked.
        if tracked {
            if let Some(sid) = slot_id {
                self.emit_mark_slot_live(out, sid)?;
            }
        }

        Ok(())
    }

    /// Emit wrap some.
    fn emit_wrap_some(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let inner_ty = self.operand_type(inner).clone();
        let inner_repr = types::ir_type_to_crepr(&inner_ty);
        let inner_layout = inner_repr.layout();
        let payload_offset = ir_layout::option_payload_offset(&inner_ty);

        // Set tag to Some (2).
        writeln!(out, "    *(uint8_t*){} = OPTION_SOME;", dest_addr).unwrap();

        // Move inner value (copy then zero source for non-copy types).
        let payload_addr = format!("({} + {})", dest_addr, payload_offset);
        match &inner_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){payload_addr} = *({c_ty}*){inner_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", payload_addr, inner_addr, layout.size).unwrap();
                }
            }
        }

        // Zero source for non-copy types to implement move semantics.
        if !inner_ty.is_copy() {
            let size = inner_layout.size;
            if size > 0 {
                writeln!(out, "    memset({}, 0, {});", inner_addr, size).unwrap();
            }
        }
        Ok(())
    }

    /// Emit wrap none.
    fn emit_wrap_none(&mut self, out: &mut String, dest: ValueId) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        // Set tag to None (1).
        writeln!(out, "    *(uint8_t*){} = OPTION_NONE;", dest_addr).unwrap();
        Ok(())
    }

    /// Emit wrap ok.
    fn emit_wrap_ok(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let inner_ty = self.operand_type(inner).clone();
        let dest_ty = self.value_type(dest);

        let ok_ty = match dest_ty {
            IrType::Result(ok) => ok.as_ref(),
            _ => return Err(CAotError::Codegen("wrap_ok requires Result type".into())),
        };

        let payload_offset = ir_layout::result_payload_offset(ok_ty);

        // Set tag to Ok (1).
        writeln!(out, "    *(uint8_t*){} = RESULT_OK;", dest_addr).unwrap();

        // Move inner value (copy then zero source for non-copy types).
        let payload_addr = format!("({} + {})", dest_addr, payload_offset);
        let inner_repr = types::ir_type_to_crepr(&inner_ty);
        match &inner_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){payload_addr} = *({c_ty}*){inner_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", payload_addr, inner_addr, layout.size).unwrap();
                }
            }
        }

        // Zero source for non-copy types to implement move semantics.
        if !inner_ty.is_copy() {
            let size = inner_repr.layout().size;
            if size > 0 {
                writeln!(out, "    memset({}, 0, {});", inner_addr, size).unwrap();
            }
        }
        Ok(())
    }

    /// Emit wrap err.
    fn emit_wrap_err(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let dest_ty = self.value_type(dest);

        let ok_ty = match dest_ty {
            IrType::Result(ok) => ok.as_ref(),
            _ => return Err(CAotError::Codegen("wrap_err requires Result type".into())),
        };

        let error_size = std::mem::size_of::<datalove_rtdt::Error>() as u32;
        let payload_offset = ir_layout::result_payload_offset(ok_ty);

        // Set tag to Err (2).
        writeln!(out, "    *(uint8_t*){} = RESULT_ERR;", dest_addr).unwrap();

        // Move error value (copy then zero source to prevent double-free).
        let payload_addr = format!("({} + {})", dest_addr, payload_offset);
        writeln!(out, "    memcpy({}, {}, {});", payload_addr, inner_addr, error_size).unwrap();
        writeln!(out, "    memset({}, 0, {});", inner_addr, error_size).unwrap();
        Ok(())
    }

    /// Emit unwrap option.
    fn emit_unwrap_option(&mut self, out: &mut String, dest: ValueId, is_some: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let is_some_addr = self.value_addr(is_some);
        let src_addr = self.operand_addr(src);
        let dest_ty = self.value_type(dest);
        let payload_offset = ir_layout::option_payload_offset(dest_ty);

        // Read tag.
        writeln!(out, "    *(bool_t*){} = (*(uint8_t*){} == OPTION_SOME);", is_some_addr, src_addr).unwrap();

        // Copy payload.
        let payload_addr = format!("({} + {})", src_addr, payload_offset);
        match types::ir_type_to_crepr(dest_ty) {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){payload_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, payload_addr, layout.size).unwrap();
                }
            }
        }
        Ok(())
    }

    /// Emit unwrap result.
    fn emit_unwrap_result(
        &mut self,
        out: &mut String,
        ok_dest: ValueId,
        err_dest: ValueId,
        is_ok: ValueId,
        src: &Operand,
    ) -> Result<(), CAotError> {
        let ok_addr = self.value_addr(ok_dest);
        let err_addr = self.value_addr(err_dest);
        let is_ok_addr = self.value_addr(is_ok);
        let src_addr = self.operand_addr(src);
        let ok_ty = self.value_type(ok_dest);

        let error_size = std::mem::size_of::<datalove_rtdt::Error>() as u32;
        let payload_offset = ir_layout::result_payload_offset(ok_ty);

        // Read tag.
        writeln!(out, "    *(bool_t*){} = (*(uint8_t*){} == RESULT_OK);", is_ok_addr, src_addr).unwrap();

        // Copy ok payload.
        let payload_addr = format!("({} + {})", src_addr, payload_offset);
        match types::ir_type_to_crepr(ok_ty) {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){ok_addr} = *({c_ty}*){payload_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", ok_addr, payload_addr, layout.size).unwrap();
                }
            }
        }

        // Copy err payload.
        writeln!(out, "    memcpy({}, {}, {});", err_addr, payload_addr, error_size).unwrap();
        Ok(())
    }

    /// Emit enum variant.
    fn emit_enum_variant(
        &mut self,
        out: &mut String,
        dest: ValueId,
        variant_index: u32,
        payload: Option<&Operand>,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);

        // Set discriminant.
        writeln!(out, "    *(uint32_t*){} = {};", dest_addr, variant_index).unwrap();

        // Copy payload if present.
        if let Some(payload_op) = payload {
            let payload_addr = self.operand_addr(payload_op);
            let payload_ty = self.operand_type(payload_op);
            let payload_offset = ir_layout::enum_payload_offset(payload_ty);
            let dest_payload_addr = format!("({} + {})", dest_addr, payload_offset);

            match types::ir_type_to_crepr(payload_ty) {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){dest_payload_addr} = *({c_ty}*){payload_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "    memcpy({}, {}, {});", dest_payload_addr, payload_addr, layout.size).unwrap();
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit enum discriminant read.
    fn emit_enum_discriminant(
        &mut self,
        out: &mut String,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        writeln!(out, "    *(uint32_t*){} = *(uint32_t*){};", dest_addr, src_addr).unwrap();
        Ok(())
    }

    /// Emit enum payload extraction.
    fn emit_enum_payload(
        &mut self,
        out: &mut String,
        dest: ValueId,
        src: &Operand,
        variant_index: u32,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);

        // Get the enum type from src to find payload offset.
        let src_ty = self.operand_type(src).clone();
        let variants = src_ty.enum_variants()
            .expect("EnumPayload src must be enum-like type");
        let payload_ty = variants[variant_index as usize].1.as_ref()
            .unwrap_or_else(|| panic!("EnumPayload variant has no payload type"));
        let payload_offset = ir_layout::enum_payload_offset(payload_ty);

        match types::ir_type_to_crepr(payload_ty) {
            CRepr::Scalar(c_ty) => {
                let src_payload = format!("({} + {})", src_addr, payload_offset);
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_payload};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    let src_payload = format!("({} + {})", src_addr, payload_offset);
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_payload, layout.size).unwrap();
                }
            }
        }
        Ok(())
    }

    /// Emit error from.
    fn emit_error_from(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let inner_ty = self.operand_type(inner).clone();
        let inner_tydesc = self.tydesc_name(&inner_ty);
        let inner_size = types::ir_type_to_crepr(&inner_ty).layout().size;

        writeln!(out, "    dtlv_rti_error_from_local(rt, {}, &{}, {});", inner_addr, inner_tydesc, dest_addr).unwrap();

        // Zero source to implement move semantics (prevents double-free/leak).
        if inner_size > 0 {
            writeln!(out, "    memset({}, 0, {});", inner_addr, inner_size).unwrap();
        }
        Ok(())
    }

    /// Emit data from.
    fn emit_data_from(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let inner_ty = self.operand_type(inner).clone();
        let inner_tydesc = self.tydesc_name(&inner_ty);
        let inner_size = types::ir_type_to_crepr(&inner_ty).layout().size;

        writeln!(out, "    dtlv_rti_data_from_local(rt, {}, &{}, {});", inner_addr, inner_tydesc, dest_addr).unwrap();

        // Zero source to implement move semantics (prevents double-free/leak).
        if inner_size > 0 {
            writeln!(out, "    memset({}, 0, {});", inner_addr, inner_size).unwrap();
        }
        Ok(())
    }

    /// Emit an erasure conversion, in either direction.
    ///
    /// The two tydescs differ only where the erased shape has a data, and the
    /// runtime walks them together. The source is zeroed after, the way the
    /// wrap zeroes what it consumed, since the value now belongs to the
    /// destination.
    fn emit_erasure(&mut self, out: &mut String, dest: ValueId, src: &Operand, erasing: bool) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        let dest_ty = self.value_type(dest).clone();
        let src_ty = self.operand_type(src).clone();
        let dest_tydesc = self.tydesc_name(&dest_ty);
        let src_tydesc = self.tydesc_name(&src_ty);
        let func = if erasing { "dtlv_rti_erase_local" } else { "dtlv_rti_reify_local" };

        // The source is left as it stands. Zeroing it here would say the value
        // has gone, which is the drop schedule's to decide and not true of a
        // copy type: a `u32` erased into two calls is read twice from the same
        // place, and the second read would find the zero the first wrote.
        writeln!(out, "    {}(rt, {}, &{}, {}, &{});", func, src_addr, src_tydesc, dest_addr, dest_tydesc).unwrap();
        Ok(())
    }

    /// Emit an erasure of a source that may be holding nothing.
    ///
    /// The source is the destination of an erased `out` parameter, and one that
    /// has never been written holds whatever the C stack left in `__frame`.
    /// Its tracking byte says which, and with nothing there the destination
    /// gets two zero words instead: an empty `data`, which the call destroys as
    /// a no-op and the callee overwrites.
    fn emit_erasure_tracked(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let guard = match src {
            Operand::Slot(id) => self.layout.slot_tracking_byte(id.0),
            Operand::Value(id) | Operand::ValueRef(id) => self.layout.value_tracking_byte(id.0),
            Operand::Param(id) => self.layout.param_tracking_byte(id.0),
            Operand::ExternalSlot { .. } | Operand::ExternalValue { .. } => None,
        };
        let Some(offset) = guard else {
            return self.emit_erasure(out, dest, src, true);
        };

        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest).clone();
        let dest_size = types::ir_type_to_crepr(&dest_ty).layout().size;

        writeln!(out, "    if (__frame[{}] == TRACK_LIVE) {{", offset).unwrap();
        self.emit_erasure(out, dest, src, true)?;
        writeln!(out, "    }} else {{").unwrap();
        writeln!(out, "    memset({}, 0, {});", dest_addr, dest_size).unwrap();
        writeln!(out, "    }}").unwrap();
        Ok(())
    }

    /// Emit list new.
    fn emit_list_new(&mut self, out: &mut String, dest: ValueId, elements: &[Operand]) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest).clone();

        let elem_ty = match &dest_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CAotError::Codegen("list_new requires List type".into())),
        };

        let elem_tydesc = self.tydesc_name(&elem_ty);
        let elem_layout = types::ir_type_to_crepr(&elem_ty).layout();

        if elements.is_empty() {
            writeln!(out, "    dtlv_rti_list_create_local(rt, {}, &{});", dest_addr, elem_tydesc).unwrap();
        } else {
            // Allocate temp buffer for elements.
            let total_size = elem_layout.size * elements.len() as u32;
            writeln!(out, "    {{ uint8_t __elems[{}];", total_size).unwrap();

            for (i, elem_op) in elements.iter().enumerate() {
                let elem_addr = self.operand_addr(elem_op);
                let offset = i as u32 * elem_layout.size;

                match types::ir_type_to_crepr(&elem_ty) {
                    CRepr::Scalar(c_ty) => {
                        writeln!(out, "    *({}*)(__elems + {}) = *({}*){};", c_ty, offset, c_ty, elem_addr).unwrap();
                    }
                    CRepr::Aggregate(layout) => {
                        if layout.size > 0 {
                            writeln!(out, "    memcpy(__elems + {}, {}, {});", offset, elem_addr, layout.size).unwrap();
                        }
                    }
                }
            }

            writeln!(out, "    dtlv_rti_list_build_from_slice_local(rt, {}, &{}, __elems, {}); }}",
                dest_addr, elem_tydesc, elements.len()).unwrap();
        }
        Ok(())
    }

    /// Emit set new.
    fn emit_set_new(&mut self, out: &mut String, dest: ValueId, elements: &[Operand]) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest).clone();

        let elem_ty = match &dest_ty {
            IrType::Set(e) => e.as_ref().clone(),
            _ => return Err(CAotError::Codegen("set_new requires Set type".into())),
        };
        let elem_tydesc = self.tydesc_name(&elem_ty);
        let set_tydesc = self.tydesc_name(&dest_ty);

        writeln!(out, "    dtlv_rti_btreeset_create_local(rt, {}, &{});", dest_addr, elem_tydesc).unwrap();
        if elements.is_empty() {
            return Ok(());
        }

        // Inserted one at a time. Building from a slice takes the elements for
        // sorted, and a literal is written in whatever order the author liked:
        // `#{"c", "a", "b"}` built that way is a tree whose keys are out of
        // order, and every lookup afterwards binary-searches into the wrong
        // place. `contains` said false for an element that was there.
        let added = format!("__sa{}", dest.0);
        writeln!(out, "    bool_t {added};").unwrap();
        for elem_op in elements {
            let elem_addr = self.operand_addr(elem_op);
            let elem_td = self.operand_tydesc(elem_op);
            writeln!(out, "    dtlv_rti_btreeset_insert_local(rt, {}, &{}, {}, {}, &{added});",
                dest_addr, set_tydesc, elem_addr, elem_td).unwrap();
        }
        Ok(())
    }

    /// Emit map new.
    fn emit_map_new(&mut self, out: &mut String, dest: ValueId, entries: &[(Operand, Operand)]) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest).clone();

        let (key_ty, val_ty) = match &dest_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CAotError::Codegen("map_new requires Map type".into())),
        };
        let key_tydesc = self.tydesc_name(&key_ty);
        let val_tydesc = self.tydesc_name(&val_ty);
        let map_tydesc = self.tydesc_name(&dest_ty);

        writeln!(out, "    dtlv_rti_btreemap_create_local(rt, {}, &{});", dest_addr, map_tydesc).unwrap();
        if entries.is_empty() {
            return Ok(());
        }

        // Inserted one at a time; see `emit_set_new`. Building from slices
        // takes the entries for sorted by key, and a literal is not.
        let _ = (&key_tydesc, &val_tydesc);
        for (key_op, val_op) in entries {
            let key_addr = self.operand_addr(key_op);
            let key_td = self.operand_tydesc(key_op);
            let val_addr = self.operand_addr(val_op);
            let val_td = self.operand_tydesc(val_op);
            writeln!(out, "    dtlv_rti_btreemap_insert_local(rt, {}, &{}, {}, {}, {}, {});",
                dest_addr, map_tydesc, key_addr, key_td, val_addr, val_td).unwrap();
        }
        Ok(())
    }

    /// Emit tensor new.
    fn emit_tensor_new(&mut self, out: &mut String, dest: ValueId, shape: &[u32], elements: &[Operand]) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest).clone();

        let (elem_ty, rank) = match &dest_ty {
            IrType::Tensor(e, r) => (e.as_ref().clone(), *r),
            _ => return Err(CAotError::Codegen("tensor_new requires Tensor type".into())),
        };

        let elem_tydesc = self.tydesc_name(&elem_ty);
        let dest_tydesc = self.tydesc_name(&dest_ty);
        let elem_layout = types::ir_type_to_crepr(&elem_ty).layout();

        // Allocate temp buffer for elements.
        let total_size = elem_layout.size * elements.len() as u32;

        writeln!(out, "    {{ uint8_t __elems[{}];", total_size.max(1)).unwrap();

        for (i, elem_op) in elements.iter().enumerate() {
            let elem_addr = self.operand_addr(elem_op);
            let offset = i as u32 * elem_layout.size;

            match types::ir_type_to_crepr(&elem_ty) {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({}*)(__elems + {}) = *({}*){};", c_ty, offset, c_ty, elem_addr).unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "    memcpy(__elems + {}, {}, {});", offset, elem_addr, layout.size).unwrap();
                    }
                }
            }
        }

        // Emit shape array. An extent is a `u32` in the IR and the runtime
        // reads it as one, whatever width an index happens to be.
        let shape_str: Vec<String> = shape.iter().map(|s| s.to_string()).collect();
        writeln!(out, "    static const uint32_t __shape[] = {{ {} }};", shape_str.join(", ")).unwrap();

        writeln!(out, "    dtlv_rti_tensor_init_local(rt, __elems, {}, &{}, __shape, {}, {}, &{}); }}",
            elements.len(), elem_tydesc, rank, dest_addr, dest_tydesc).unwrap();
        Ok(())
    }

    /// Emit table new.
    fn emit_table_new(&mut self, out: &mut String, dest: ValueId, rows: &[Operand]) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let dest_ty = self.value_type(dest).clone();

        let columns = match &dest_ty {
            IrType::Table(cols) => cols.clone(),
            _ => return Err(CAotError::Codegen("table_new requires Table type".into())),
        };

        let dest_tydesc = self.tydesc_name(&dest_ty);

        // Compute row type (tuple of column types).
        let row_field_types: Vec<IrType> = columns.iter().map(|(_, ty)| (**ty).clone()).collect();
        let row_ty = IrType::Tuple(row_field_types);
        let row_tydesc = self.tydesc_name(&row_ty);
        let row_layout = types::ir_type_to_crepr(&row_ty).layout();

        if rows.is_empty() {
            writeln!(out, "    dtlv_rti_table_create_local(rt, {}, &{});", dest_addr, dest_tydesc).unwrap();
        } else {
            let total_size = row_layout.size * rows.len() as u32;
            // Aligned, because the runtime reads each row through the tuple
            // descriptor's offsets and those are computed from the columns'
            // own alignments.
            writeln!(out, "    {{ _Alignas(8) uint8_t __rows[{}];", total_size.max(1)).unwrap();

            for (i, row_op) in rows.iter().enumerate() {
                let row_addr = self.operand_addr(row_op);
                let offset = i as u32 * row_layout.size;

                if row_layout.size > 0 {
                    writeln!(out, "    memcpy(__rows + {}, {}, {});", offset, row_addr, row_layout.size).unwrap();
                }
            }

            writeln!(out, "    dtlv_rti_table_build_from_rows_local(rt, {}, &{}, __rows, &{}, {}); }}",
                dest_addr, dest_tydesc, row_tydesc, rows.len()).unwrap();
        }
        Ok(())
    }

    /// Emit function call.
    fn emit_call(
        &mut self,
        out: &mut String,
        dest: ValueId,
        func: &CodeRef,
        args: &[Operand],
        shape_descriptors: &[datalove_datafun_ir::DescriptorRef],
    ) -> Result<(), CAotError> {
        // A rider function is reached through the runtime C ABI rather than
        // this backend's own convention, so it is a different call entirely.
        if let CodeRef::Module { module, id } = func {
            if let Some(unit) = self.registry.get_module_function_as_unit(*module, *id) {
                if let Some(native) = unit.native_context() {
                    let symbol = native.symbol.clone();
                    let param_modes = native.param_modes.clone();
                    return self.emit_native_call(
                        out, dest, &symbol, args, &param_modes, shape_descriptors);
                }
            }
        }

        self.destroy_out_destinations(out, func, args)?;

        // An argument that arrives wrapped where the callee wants it borrowed
        // is read through first, and the descriptor that comes with it is used
        // for that parameter below. Both come out of the wrapper together.
        let modes = self.callee_param_modes(func);
        let mut borrowed: Vec<Option<String>> = vec![None; args.len()];
        for (i, arg) in args.iter().enumerate() {
            let wants_borrow = matches!(
                modes.get(i),
                Some(datalove_datafun_ir::ParamMode::Ref) | Some(datalove_datafun_ir::ParamMode::Mut));
            if !wants_borrow || *self.operand_type(arg) != IrType::Data {
                continue;
            }
            // Anything that carries its own descriptor is already a pointer at
            // the value, never a wrapper around it, even though its type reads
            // `data`. That is one of our own borrowed parameters, and any
            // reference projected or indexed out of one. Reading through it
            // would take the first bytes of the value for a wrapper's two
            // pointers.
            if self.operand_ref_desc(arg).is_some() {
                continue;
            }
            // Named for the call's destination as well as the argument, since
            // two calls in one block would otherwise declare the same locals.
            let name = format!("__bw{}_{}", dest.0, i);
            writeln!(out, "    const void* {name}_v; const dtlv_tydesc_t* {name}_t; uint64_t {name}_s;").unwrap();
            writeln!(out, "    dtlv_rti_data_borrow({}, &{name}_s, &{name}_v, &{name}_t);",
                self.operand_addr(arg)).unwrap();
            borrowed[i] = Some(name);
        }

        // For local function lookup, use parent_unit if available (for nested functions).
        let lookup_unit = self.parent_unit.unwrap_or(self.unit);
        let func_name = self.compiler.resolve_func_name_with_registry(func, lookup_unit, self.registry);
        let dest_ty = self.value_type(dest).clone();
        let uses_sret = types::uses_sret(&dest_ty);

        // Build argument list.
        let mut call_args = String::from("rt");
        if uses_sret {
            write!(&mut call_args, ", {}", self.value_addr(dest)).unwrap();
        }
        for (i, arg) in args.iter().enumerate() {
            match &borrowed[i] {
                Some(name) => write!(&mut call_args, ", (void*){name}_v").unwrap(),
                None => write!(&mut call_args, ", {}", self.operand_addr(arg)).unwrap(),
            }
        }

        // Then a descriptor for each parameter whose own type does not say what
        // arrives at it. This is the place that knows: the argument here either
        // has a concrete type, was read out of a wrapper just above, or is a
        // parameter our own caller described.
        for param_id in self.callee_descriptor_params(func) {
            let index = param_id.0 as usize;
            let arg = args.get(index).ok_or_else(|| CAotError::Codegen(format!(
                "callee wants a descriptor for parameter {} but got {} arguments",
                param_id.0, args.len(),
            )))?;
            match borrowed.get(index).and_then(|b| b.as_ref()) {
                Some(name) => write!(&mut call_args, ", {name}_t").unwrap(),
                None => write!(&mut call_args, ", {}", self.operand_tydesc(arg)).unwrap(),
            }
        }

        // Then one for each shape the callee builds a collection of, as worked
        // out when the shape sets settled. Read rather than derived, so this
        // side and the callee's signature cannot disagree.
        for r in shape_descriptors {
            match r {
                datalove_datafun_ir::DescriptorRef::Static(ty) => {
                    let name = self.tydesc_name(ty);
                    write!(&mut call_args, ", &{}", name).unwrap()
                }
                datalove_datafun_ir::DescriptorRef::Own(i) => {
                    write!(&mut call_args, ", s{}", i).unwrap()
                }
            }
        }

        if uses_sret || dest_ty == IrType::Unit {
            writeln!(out, "    {}({});", func_name, call_args).unwrap();
        } else {
            let dest_addr = self.value_addr(dest);
            let c_ty = types::ir_type_to_c(&dest_ty);
            writeln!(out, "    *({c_ty}*){dest_addr} = {}({});", func_name, call_args).unwrap();
        }
        self.mark_out_destinations_live(out, func, args);
        Ok(())
    }

    /// Record that the call wrote every `out` argument's destination.
    ///
    /// The callee promises to write one before it returns, so whatever drops
    /// the destination afterwards has to know it holds something. Without this
    /// an uninitialized `var` passed as `out` came back still reading
    /// uninitialized and the drop at the end of its scope skipped what the call
    /// had put there. A destination that was live before the call dropped
    /// correctly either way, which is why this went unseen.
    fn mark_out_destinations_live(&mut self, out: &mut String, func: &CodeRef, args: &[Operand]) {
        use datalove_datafun_ir::ParamMode;
        let modes = self.callee_param_modes(func);
        for (i, arg) in args.iter().enumerate() {
            if modes.get(i) == Some(&ParamMode::Out) {
                self.mark_tracking_live(out, arg);
            }
        }
    }

    /// Build a set described by a handed-over descriptor, then wrap it.
    fn emit_set_new_erased(
        &mut self,
        out: &mut String,
        dest: ValueId,
        elements: &[Operand],
        shape: u32,
    ) -> Result<(), CAotError> {
        let name = format!("__ns{}", dest.0);
        writeln!(out, "    dtlv_set_t {name}; bool_t {name}_added;").unwrap();
        writeln!(out, "    dtlv_rti_btreeset_create_local(rt, &{name}, s{shape});").unwrap();
        for elem in elements {
            let elem_addr = self.operand_addr(elem);
            let elem_td = self.operand_tydesc(elem);
            writeln!(out, "    dtlv_rti_btreeset_insert_erased_local(rt, &{name}, s{shape}, {elem_addr}, {elem_td}, &{name}_added);").unwrap();
        }
        let dest_addr = self.value_addr(dest);
        writeln!(out, "    dtlv_rti_data_from_local(rt, &{name}, s{shape}, {dest_addr});").unwrap();
        Ok(())
    }

    /// Build a map described by a handed-over descriptor, then wrap it.
    fn emit_map_new_erased(
        &mut self,
        out: &mut String,
        dest: ValueId,
        entries: &[(Operand, Operand)],
        shape: u32,
    ) -> Result<(), CAotError> {
        let name = format!("__nm{}", dest.0);
        writeln!(out, "    dtlv_map_t {name};").unwrap();
        writeln!(out, "    dtlv_rti_btreemap_create_local(rt, &{name}, s{shape});").unwrap();
        for (key, val) in entries {
            let key_addr = self.operand_addr(key);
            let key_td = self.operand_tydesc(key);
            let val_addr = self.operand_addr(val);
            let val_td = self.operand_tydesc(val);
            writeln!(out, "    dtlv_rti_btreemap_insert_erased_local(rt, &{name}, s{shape}, {key_addr}, {key_td}, {val_addr}, {val_td});").unwrap();
        }
        let dest_addr = self.value_addr(dest);
        writeln!(out, "    dtlv_rti_data_from_local(rt, &{name}, s{shape}, {dest_addr});").unwrap();
        Ok(())
    }

    /// The shapes this function itself declared.
    fn unit_descriptor_shapes(&self) -> Vec<datalove_datafun_ir::DescriptorShape> {
        self.unit.function_context()
            .map(|c| c.descriptor_shapes.clone())
            .unwrap_or_default()
    }

    /// Build a list described by a handed-over descriptor, then wrap it.
    ///
    /// The destination is a `data`, because a list built over a type parameter
    /// erases to one. The elements are `data` too, which is what a `T` is
    /// anywhere.
    fn emit_list_new_erased(
        &mut self,
        out: &mut String,
        dest: ValueId,
        elements: &[Operand],
        shape: u32,
    ) -> Result<(), CAotError> {
        let name = format!("__nl{}", dest.0);
        writeln!(out, "    dtlv_list_t {name};").unwrap();
        writeln!(out, "    dtlv_rti_list_create_local(rt, &{name}, s{shape});").unwrap();
        // Described as what it really is: an element here is in the erased
        // shape, and the list holds its elements as what they really are, so
        // the push converts.
        for elem in elements {
            let elem_addr = self.operand_addr(elem);
            let elem_td = self.operand_tydesc(elem);
            writeln!(out, "    dtlv_rti_list_push_erased_local(rt, &{name}, s{shape}, {elem_addr}, {elem_td});").unwrap();
        }
        let dest_addr = self.value_addr(dest);
        writeln!(out, "    dtlv_rti_data_from_local(rt, &{name}, s{shape}, {dest_addr});").unwrap();
        Ok(())
    }

    /// The parameters of `func` whose descriptor this call site supplies.
    ///
    /// Has to agree with what `build_signature` put in the callee's signature,
    /// so both read the same `descriptor_params`.
    fn callee_descriptor_params(&self, func: &CodeRef) -> Vec<ParamId> {
        let unit = match func {
            CodeRef::Module { module, id } => {
                self.registry.get_module_function_as_unit(*module, *id)
            }
            CodeRef::Local(id) => {
                let lookup_unit = self.parent_unit.unwrap_or(self.unit);
                lookup_unit.nested_units.iter().find(|nested| nested.id == *id)
            }
            CodeRef::External { .. } => None,
        };
        unit.and_then(|u| u.function_context())
            .map(|ctx| ctx.descriptor_params.clone())
            .unwrap_or_default()
    }

    /// How `func` takes each of its parameters.
    fn callee_param_modes(&self, func: &CodeRef) -> Vec<datalove_datafun_ir::ParamMode> {
        let unit = match func {
            CodeRef::Module { module, id } => {
                self.registry.get_module_function_as_unit(*module, *id)
            }
            CodeRef::Local(id) => {
                let lookup_unit = self.parent_unit.unwrap_or(self.unit);
                lookup_unit.nested_units.iter().find(|nested| nested.id == *id)
            }
            CodeRef::External { .. } => None,
        };
        unit.and_then(|u| u.function_context())
            .map(|ctx| ctx.param_modes.clone())
            .unwrap_or_default()
    }

    /// Destroy whatever an `out` argument's destination holds now.
    ///
    /// The callee writes a fresh value there and its tracking byte starts
    /// uninitialized, so its first store destroys nothing. Something has to,
    /// or the old value is dropped on the floor.
    fn destroy_out_destinations(
        &mut self,
        out: &mut String,
        func: &CodeRef,
        args: &[Operand],
    ) -> Result<(), CAotError> {
        use datalove_datafun_ir::ParamMode;
        let modes = self.callee_param_modes(func);
        for (i, arg) in args.iter().enumerate() {
            if modes.get(i) != Some(&ParamMode::Out) {
                continue;
            }
            let addr = self.operand_addr(arg);
            let tydesc = self.operand_tydesc(arg);

            // A destination that is tracked says for itself whether it holds
            // anything. This function's own out parameter, passed straight on,
            // was cleared by whoever called this one; an uninitialized `var`
            // has never held anything at all, and `__frame` is whatever the C
            // stack left there, so clearing it would free that. Freeing what
            // was never allocated is worse than leaking.
            let guard = match arg {
                Operand::Param(param) => self.layout.param_tracking_byte(param.0),
                Operand::Slot(id) => self.layout.slot_tracking_byte(id.0),
                Operand::Value(id) | Operand::ValueRef(id) => self.layout.value_tracking_byte(id.0),
                Operand::ExternalSlot { .. } | Operand::ExternalValue { .. } => None,
            };
            match guard {
                Some(offset) => writeln!(out,
                    "    if (__frame[{}] == TRACK_LIVE) dtlv_rti_any_destroy_local(rt, {}, {});",
                    offset, addr, tydesc).unwrap(),
                None => writeln!(out,
                    "    dtlv_rti_any_destroy_local(rt, {}, {});", addr, tydesc).unwrap(),
            }
        }
        Ok(())
    }

    /// The descriptor for an operand, as a C expression of pointer type.
    ///
    /// A parameter our own caller described uses that descriptor: this
    /// function's static type for it says `data` where a type parameter stood,
    /// so one built from that type would misdescribe the value. Everything
    /// else is described by its own type, which is a static descriptor.
    fn operand_tydesc(&mut self, operand: &Operand) -> String {
        if let Some(desc) = self.operand_ref_desc(operand) {
            return desc;
        }
        let ty = self.operand_type(operand).clone();
        format!("&{}", self.tydesc_name(&ty))
    }

    /// Walk a field path from a base whose layout only its descriptor says.
    ///
    /// Returns the address of the field and the static type the erased
    /// signature gives it. The offsets are read from the descriptor at each
    /// step, and the descriptor is narrowed alongside, because a path reaches
    /// further in one field at a time and each step's offsets live in the
    /// previous step's descriptor.
    ///
    /// The static type comes back too, so the caller can tell a copy type --
    /// which is a shallow store -- from one that owns what it holds.
    fn walk_dynamic_field_path(
        &self,
        base_addr: String,
        base_desc: String,
        base_ty: &IrType,
        field_path: &[u32],
    ) -> Result<(String, String, IrType), CAotError> {
        let mut addr = base_addr;
        let mut desc = base_desc;
        let mut ty = base_ty.clone();
        for &idx in field_path {
            let field_types: Vec<IrType> = match &ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(fs) => fs.iter().map(|(_, t)| t.clone()).collect(),
                other => return Err(CAotError::Codegen(format!(
                    "field path steps through a {:?}, which has no fields", other))),
            };
            addr = format!("((uint8_t*){} + dtlv_rti_field_offset({}, {}))", addr, desc, idx);
            desc = format!("dtlv_rti_field_tydesc({}, {})", desc, idx);
            ty = field_types[idx as usize].clone();
        }
        Ok((addr, desc, ty))
    }

    /// The descriptor for what an operand names, where its static type does not
    /// describe it, as a C expression of pointer type.
    ///
    /// That is a borrowed parameter our own caller described, and any reference
    /// projected out of one. `None` everywhere else, meaning the static type is
    /// the truth and a constant offset is right.
    fn operand_ref_desc(&self, operand: &Operand) -> Option<String> {
        match operand {
            Operand::Param(param_id) => self.func_descriptor_index(*param_id)
                .map(|i| format!("d{}", i)),
            Operand::Value(vid) | Operand::ValueRef(vid) => self.ref_descs.contains_key(vid)
                .then(|| format!("__rd{}", vid.0)),
            _ => None,
        }
    }

    /// Where this function's own supplied descriptor for `param` arrives, if
    /// its signature asks for one.
    fn func_descriptor_index(&self, param: ParamId) -> Option<usize> {
        self.unit.function_context()?
            .descriptor_params
            .iter()
            .position(|p| *p == param)
    }

    /// Emit a call to a native rider function.
    ///
    /// The runtime C ABI, which is the same one the interpreter and the
    /// cranelift backends use: every argument is a pointer and a descriptor
    /// saying what is behind it, the result is written through an out
    /// parameter given the same way, and the status the function returns says
    /// whether it wrote one.
    ///
    /// A type parameter no argument determines has its descriptor handed over
    /// after the out parameter, in the order the native declared its shapes.
    fn emit_native_call(
        &mut self,
        out: &mut String,
        dest: ValueId,
        symbol: &str,
        args: &[Operand],
        native_param_modes: &[datalove_datafun_ir::ParamMode],
        shape_descriptors: &[datalove_datafun_ir::DescriptorRef],
    ) -> Result<(), CAotError> {
        let mut call_args = String::from("rt");
        // A collection of a type parameter is wrapped once it is owned, and a
        // rider works on the collection itself with a descriptor beside it.
        // An argument that arrives wrapped where the native wants the thing
        // itself is read through first; see the same step in the cranelift
        // backend.
        for (i, arg) in args.iter().enumerate() {
            // Decided by the mode, not by the native's own parameter type: a
            // generic native's types are erased too, so `mut self: [T]` reads
            // `data` there just as the argument does. What tells them apart is
            // that the native borrows its collection and takes its element, and
            // only a borrow is passed through a wrapper.
            // Anything carrying its own descriptor is already a pointer at the
            // value, never a wrapper around it, even though its type reads
            // `data`. Same as at a call to a module function.
            let forwarded = self.operand_ref_desc(arg).is_some();
            let wrapped = !forwarded
                && *self.operand_type(arg) == IrType::Data
                && matches!(native_param_modes.get(i),
                    Some(datalove_datafun_ir::ParamMode::Ref)
                    | Some(datalove_datafun_ir::ParamMode::Mut));
            if wrapped {
                let name = format!("__nb{}_{}", dest.0, i);
                writeln!(out, "    const void* {name}_v; const dtlv_tydesc_t* {name}_t; uint64_t {name}_s;").unwrap();
                writeln!(out, "    dtlv_rti_data_borrow({}, &{name}_s, &{name}_v, &{name}_t);",
                    self.operand_addr(arg)).unwrap();
                write!(&mut call_args, ", (void*){name}_v, {name}_t").unwrap();
                continue;
            }
            let addr = self.operand_addr(arg);
            let tydesc = self.operand_tydesc(arg);
            write!(&mut call_args, ", {}, {}", addr, tydesc).unwrap();
        }

        let dest_ty = self.value_type(dest).clone();
        let dest_addr = self.value_addr(dest);
        let dest_tydesc = self.tydesc_name(&dest_ty);
        write!(&mut call_args, ", {}, &{}", dest_addr, dest_tydesc).unwrap();

        for r in shape_descriptors {
            match r {
                datalove_datafun_ir::DescriptorRef::Static(ty) => {
                    let name = self.tydesc_name(ty);
                    write!(&mut call_args, ", &{}", name).unwrap()
                }
                datalove_datafun_ir::DescriptorRef::Own(i) => {
                    write!(&mut call_args, ", s{}", i).unwrap()
                }
            }
        }

        writeln!(out, "    {}({});", symbol, call_args).unwrap();
        Ok(())
    }

    /// Emit slot store.
    fn emit_slot_store(
        &mut self,
        out: &mut String,
        dest: &SlotDest,
        value: &Operand,
        _is_copy: bool,
        tracked: bool,
    ) -> Result<(), CAotError> {
        let (dest_addr, slot_id) = match dest {
            SlotDest::Local(sid) => (self.slot_addr(*sid), Some(*sid)),
            SlotDest::External { .. } => {
                return Err(CAotError::Unsupported("external slot".into()));
            }
        };
        let src_addr = self.operand_addr(value);
        let ty = self.operand_type(value).clone();
        let repr = types::ir_type_to_crepr(&ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, layout.size).unwrap();
                }
            }
        }

        if tracked {
            if let Some(sid) = slot_id {
                self.emit_mark_slot_live(out, sid)?;
            }
        }
        Ok(())
    }

    /// Emit slot load.
    fn emit_slot_load(&mut self, out: &mut String, dest: ValueId, slot: SlotId, _is_copy: bool) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let slot_addr = self.slot_addr(slot);
        let ty = self.slot_type(slot);
        let repr = types::ir_type_to_crepr(ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){slot_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, slot_addr, layout.size).unwrap();
                }
            }
        }
        Ok(())
    }

    /// Emit param store.
    fn emit_param_store(&mut self, out: &mut String, param: ParamId, value: &Operand, tracked: bool) -> Result<(), CAotError> {
        let param_addr = self.param_addr(param);
        let src_addr = self.operand_addr(value);
        let func_ctx = self.unit.function_context().unwrap();
        let ty = &func_ctx.param_types[param.0 as usize];
        let repr = types::ir_type_to_crepr(ty);

        // For non-copy types, destroy the old value before overwriting.
        // - For tracked params (Out), check tracking byte first.
        // - For non-tracked mut params, always destroy (param is always initialized).
        if !ty.is_copy() {
            if tracked {
                if let Some(track_offset) = self.layout.param_tracking_byte(param.0) {
                    let tydesc = self.tydesc_name(ty);
                    writeln!(out, "    if (__frame[{}] == TRACK_LIVE) dtlv_rti_any_destroy_local(rt, {}, &{});",
                        track_offset, param_addr, tydesc).unwrap();
                }
            } else {
                // Mut param - always has a valid value that needs destruction.
                let tydesc = self.tydesc_name(ty);
                writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", param_addr, tydesc).unwrap();
            }
        }

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){param_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", param_addr, src_addr, layout.size).unwrap();
                }
            }
        }

        if tracked {
            if let Some(track_offset) = self.layout.param_tracking_byte(param.0) {
                writeln!(out, "    __frame[{}] = TRACK_LIVE;", track_offset).unwrap();
            }
        }
        Ok(())
    }

    /// Emit param set field.
    fn emit_param_set_field(
        &mut self,
        out: &mut String,
        param: ParamId,
        field_path: &[u32],
        value: &Operand,
        tracked: bool,
    ) -> Result<(), CAotError> {
        let param_addr = self.param_addr(param);
        let func_ctx = self.unit.function_context().unwrap();
        let mut current_ty = func_ctx.param_types[param.0 as usize].clone();
        let mut current_addr = param_addr;

        // A parameter our caller described is one whose static type says `data`
        // where a type parameter stood, so the offsets it gives are wrong by
        // whatever the difference in width is. Writing at one of those is the
        // severe half of this: it puts the value past the end of what the
        // caller owns.
        if let Some(base_desc) = self.operand_ref_desc(&Operand::Param(param)) {
            let (addr, field_desc, field_ty) = self.walk_dynamic_field_path(
                current_addr, base_desc, &current_ty, field_path)?;
            let src_addr = self.operand_addr(value);
            let src_tydesc = self.operand_tydesc(value);

            // The field holds a live value, destroyed against the descriptor
            // that says what is really there rather than the erased type.
            if !field_ty.is_copy() {
                writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, {});", addr, field_desc).unwrap();
            }

            // What this function holds is in the erased shape and the field is
            // in the real one, so the value is moved back out of its wrapping
            // on the way in. `reify_local` walks the two descriptors and
            // converts wherever one of them says `data`, which for a field that
            // was never erased is a copy of the same width.
            writeln!(out, "    dtlv_rti_reify_local(rt, {}, {}, {}, {});",
                src_addr, src_tydesc, addr, field_desc).unwrap();
            if tracked {
                if let Some(track_offset) = self.layout.param_tracking_byte(param.0) {
                    writeln!(out, "    __frame[{}] = TRACK_LIVE;", track_offset).unwrap();
                }
            }
            return Ok(());
        }

        for &idx in field_path {
            let field_types: Vec<IrType> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => return Err(CAotError::Codegen("param_set_field requires tuple/struct type".into())),
            };
            let offsets = types::compute_tuple_field_offsets(&field_types);
            let field_offset = offsets[idx as usize];
            current_addr = format!("({} + {})", current_addr, field_offset);
            current_ty = field_types[idx as usize].clone();
        }

        let src_addr = self.operand_addr(value);
        let repr = types::ir_type_to_crepr(&current_ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){current_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", current_addr, src_addr, layout.size).unwrap();
                }
            }
        }

        if tracked {
            if let Some(track_offset) = self.layout.param_tracking_byte(param.0) {
                writeln!(out, "    __frame[{}] = TRACK_LIVE;", track_offset).unwrap();
            }
        }
        Ok(())
    }

    /// Emit ref store.
    fn emit_ref_store(&mut self, out: &mut String, dest: &Operand, value: &Operand, tracked: bool) -> Result<(), CAotError> {
        let dest_addr = self.operand_addr(dest);
        let src_addr = self.operand_addr(value);
        let ty = self.operand_type(value).clone();
        let repr = types::ir_type_to_crepr(&ty);

        if !tracked && !ty.is_copy() {
            // Non-tracked: destination has a valid value, destroy it first.
            let tydesc = self.tydesc_name(&ty);
            writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", dest_addr, tydesc).unwrap();
        }

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, layout.size).unwrap();
                }
            }
        }

        if tracked {
            // The destination was uninitialized, so nothing was destroyed
            // above and this only records that something is there now. A
            // reference into a place this frame does not own has no tracking
            // byte here, and then there is nothing to record.
            self.mark_tracking_live(out, dest);
        }
        Ok(())
    }

    /// Note that an operand's storage holds a value again.
    ///
    /// Only a place this frame tracks has a byte to write. A reference
    /// pointing into a caller's frame does not, and its liveness is that
    /// frame's to know.
    fn mark_tracking_live(&self, out: &mut String, operand: &Operand) {
        let offset = match operand {
            Operand::Slot(id) => self.layout.slot_tracking_byte(id.0),
            Operand::Value(id) | Operand::ValueRef(id) => self.layout.value_tracking_byte(id.0),
            Operand::Param(id) => self.layout.param_tracking_byte(id.0),
            Operand::ExternalSlot { .. } | Operand::ExternalValue { .. } => None,
        };
        if let Some(offset) = offset {
            writeln!(out, "    __frame[{}] = TRACK_LIVE;", offset).unwrap();
        }
    }

    /// Emit ref set field.
    fn emit_ref_set_field(
        &mut self,
        out: &mut String,
        dest: &Operand,
        field_path: &[u32],
        value: &Operand,
        tracked: bool,
    ) -> Result<(), CAotError> {
        let mut current_addr = self.operand_addr(dest);
        let mut current_ty = self.operand_type(dest).clone();

        // Dereference if it's a Ref type.
        if let IrType::Ref(inner) = &current_ty {
            current_ty = inner.as_ref().clone();
        }

        for &idx in field_path {
            let field_types: Vec<IrType> = match &current_ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(fs) => fs.iter().map(|(_, ty)| ty.clone()).collect(),
                _ => return Err(CAotError::Codegen("ref_set_field requires tuple/struct type".into())),
            };
            let offsets = types::compute_tuple_field_offsets(&field_types);
            let field_offset = offsets[idx as usize];
            current_addr = format!("({} + {})", current_addr, field_offset);
            current_ty = field_types[idx as usize].clone();
        }

        let src_addr = self.operand_addr(value);
        let repr = types::ir_type_to_crepr(&current_ty);

        if !tracked && !current_ty.is_copy() {
            // Non-tracked: field has a valid value, destroy it first.
            let tydesc = self.tydesc_name(&current_ty);
            writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", current_addr, tydesc).unwrap();
        }

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){current_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    memcpy({}, {}, {});", current_addr, src_addr, layout.size).unwrap();
                }
            }
        }

        if tracked {
            // Tracked (RefSetFieldTracked): destination was uninitialized, no destroy needed.
            // Mark tracking byte as LIVE.
            todo!("RefSetFieldTracked tracking byte write not yet implemented in C AOT");
        }
        Ok(())
    }

    /// Emit drop.
    fn emit_drop(&mut self, out: &mut String, operand: &Operand, tracked: bool) -> Result<(), CAotError> {
        let addr = self.operand_addr(operand);
        let ty = self.operand_type(operand).clone();
        let tydesc = self.tydesc_name(&ty);

        if tracked {
            // Check tracking byte before dropping.
            let track_offset = match operand {
                Operand::Slot(sid) => self.layout.slot_tracking_byte(sid.0),
                Operand::Value(vid) => self.layout.value_tracking_byte(vid.0),
                _ => None,
            };

            if let Some(offset) = track_offset {
                writeln!(out, "    if (__frame[{}] == TRACK_LIVE) {{ dtlv_rti_any_destroy_local(rt, {}, &{}); __frame[{}] = TRACK_MOVED; }}",
                    offset, addr, tydesc, offset).unwrap();
            } else {
                writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", addr, tydesc).unwrap();
            }
        } else {
            writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", addr, tydesc).unwrap();
        }
        Ok(())
    }

    /// Emit drop via ref.
    fn emit_drop_via_ref(&mut self, out: &mut String, ref_value: ValueId) -> Result<(), CAotError> {
        let ref_ty = self.value_type(ref_value).clone();

        let inner_ty = match &ref_ty {
            IrType::Ref(inner) => (*inner).clone(),
            _ => return Err(CAotError::Codegen("drop_via_ref requires Ref type".into())),
        };

        // Copy types don't need drops.
        if inner_ty.is_copy() {
            return Ok(());
        }

        let ref_addr = self.value_addr(ref_value);
        let inner_tydesc = self.tydesc_name(&inner_ty);

        // The ref_addr contains a pointer - dereference and destroy.
        writeln!(out, "    dtlv_rti_any_destroy_local(rt, *(void**){}, &{});", ref_addr, inner_tydesc).unwrap();
        Ok(())
    }

    /// Emit debug log.
    fn emit_debuglog(&mut self, out: &mut String, operand: &Operand) -> Result<(), CAotError> {
        let addr = self.operand_addr(operand);
        let ty = self.operand_type(operand).clone();
        let tydesc = self.tydesc_name(&ty);

        writeln!(out, "    dtlv_rti_debuglog_local(rt, {}, &{});", addr, tydesc).unwrap();
        Ok(())
    }

    /// Emit a ListBoundsCheck instruction.
    fn emit_list_bounds_check(
        &mut self,
        out: &mut String,
        is_valid: ValueId,
        list: &Operand,
        index: &Operand,
    ) -> Result<(), CAotError> {
        let is_valid_addr = self.value_addr(is_valid);
        let list_addr = self.operand_addr(list);
        let index_addr = self.operand_addr(index);
        let size_offset = std::mem::offset_of!(datalove_rtdt::List, size);

        writeln!(out, "    *(bool_t*){} = (*(index_t*){} < *(index_t*)({} + {}));",
            is_valid_addr, index_addr, list_addr, size_offset).unwrap();
        Ok(())
    }

    /// Emit a ListGet instruction.
    fn emit_list_get(
        &mut self,
        out: &mut String,
        dest: ValueId,
        is_valid: ValueId,
        list: &Operand,
        index: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let is_valid_addr = self.value_addr(is_valid);
        let list_addr = self.operand_addr(list);
        let index_addr = self.operand_addr(index);
        let size_offset = std::mem::offset_of!(datalove_rtdt::List, size);

        // Get element type.
        let list_ty = self.operand_type(list).clone();
        let elem_ty = match &list_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "ListGet on non-list type: {:?}", list_ty
            ))),
        };
        // An element the callee cannot name is read through the list's
        // descriptor rather than at a stride taken from the static type, which
        // inside a generic is a `data`'s and lands between elements. The
        // runtime decides whether the element wants packing on the way out,
        // since a list whose elements really are `data` does not.
        if *self.value_type(dest) == IrType::Data {
            let option_ty = IrType::Option(Box::new(IrType::Data));
            let option_layout = ir_layout::layout_of(&option_ty);
            let option_tydesc = self.tydesc_name(&option_ty);
            let list_tydesc = self.operand_tydesc(list);
            let name = format!("__ix{}", dest.0);
            writeln!(out, "    _Alignas(8) uint8_t {name}[{}] = {{0}};", option_layout.size).unwrap();
            writeln!(out, "    dtlv_rti_list_get_erased_local(rt, {}, {}, *(index_t*){}, {name}, &{});",
                list_addr, list_tydesc, index_addr, option_tydesc).unwrap();
            writeln!(out, "    *(bool_t*){} = ({name}[0] == OPTION_SOME);", is_valid_addr).unwrap();
            writeln!(out, "    memcpy({}, {name} + {}, {});",
                dest_addr, std::mem::align_of::<datalove_rtdt::Data>(),
                std::mem::size_of::<datalove_rtdt::Data>()).unwrap();
            return Ok(());
        }

        let elem_repr = types::ir_type_to_crepr(&elem_ty);
        let elem_size = elem_repr.layout().size;

        // Bounds check.
        writeln!(out, "    *(bool_t*){} = (*(index_t*){} < *(index_t*)({} + {}));",
            is_valid_addr, index_addr, list_addr, size_offset).unwrap();

        // Conditional load.
        writeln!(out, "    if (*(bool_t*){}) {{", is_valid_addr).unwrap();

        // Compute element address: data_ptr + index * elem_size.
        writeln!(out, "        void* __elem = *(void**){} + (size_t)*(index_t*){} * {};",
            list_addr, index_addr, elem_size).unwrap();

        // Clone element to dest.
        match &elem_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "        *({c_ty}*){dest_addr} = *({c_ty}*)__elem;").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    let tydesc = self.tydesc_name(&elem_ty);
                    writeln!(out, "        dtlv_rti_clone_local(rt, __elem, &{}, {}, &{});",
                        tydesc, dest_addr, tydesc).unwrap();
                }
            }
        }

        writeln!(out, "    }}").unwrap();

        // Conditional tracking byte.
        if let Some(track_offset) = self.layout.value_tracking_byte(dest.0) {
            writeln!(out, "    __frame[{}] = *(bool_t*){} ? TRACK_LIVE : TRACK_UNINIT;",
                track_offset, is_valid_addr).unwrap();
        }

        Ok(())
    }

    /// Emit a ListSet instruction.
    fn emit_list_set(
        &mut self,
        out: &mut String,
        list: &Operand,
        index: &Operand,
        value: &Operand,
    ) -> Result<(), CAotError> {
        let list_addr = self.operand_addr(list);
        let index_addr = self.operand_addr(index);
        let value_addr = self.operand_addr(value);

        // Get element type.
        let list_ty = self.operand_type(list).clone();
        let elem_ty = match &list_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "ListSet on non-list type: {:?}", list_ty
            ))),
        };
        let elem_repr = types::ir_type_to_crepr(&elem_ty);
        let elem_size = elem_repr.layout().size;
        let elem_tydesc = self.tydesc_name(&elem_ty);

        // Compute element address.
        writeln!(out, "    {{ void* __elem = *(void**){} + (size_t)*(index_t*){} * {};",
            list_addr, index_addr, elem_size).unwrap();

        // Destroy old element.
        writeln!(out, "    dtlv_rti_any_destroy_local(rt, __elem, &{});", elem_tydesc).unwrap();

        // Store new value.
        match &elem_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*)__elem = *({c_ty}*){value_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    dtlv_rti_move_value_local(rt, {}, &{}, __elem);",
                        value_addr, elem_tydesc).unwrap();
                }
            }
        }

        writeln!(out, "    }}").unwrap();
        Ok(())
    }

    /// Emit a ListElementRef instruction.
    fn emit_list_element_ref(
        &mut self,
        out: &mut String,
        dest: ValueId,
        list: &Operand,
        index: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let list_addr = self.operand_addr(list);
        let index_addr = self.operand_addr(index);

        let list_ty = self.operand_type(list).clone();
        let elem_ty = match &list_ty {
            IrType::List(e) => e.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "ListElementRef on non-list type: {:?}", list_ty
            ))),
        };

        // Inside a generic the static element type is a `data`, and a `data` is
        // a different width from whatever the caller's list really holds, so a
        // stride taken from it lands between elements. The list's own
        // descriptor says, and the element's descriptor goes on to describe
        // this reference.
        if let Some(list_desc) = self.operand_ref_desc(list) {
            writeln!(out, "    __rd{} = dtlv_rti_element_tydesc({});",
                dest.0, list_desc).unwrap();
            writeln!(out, "    *(void**){} = *(void**){} + (size_t)*(index_t*){} * __rd{}->size;",
                dest_addr, list_addr, index_addr, dest.0).unwrap();
            return Ok(());
        }

        let elem_repr = types::ir_type_to_crepr(&elem_ty);
        let elem_size = elem_repr.layout().size;

        // Store pointer to element: dest = &list.data[index].
        writeln!(out, "    *(void**){} = *(void**){} + (size_t)*(index_t*){} * {};",
            dest_addr, list_addr, index_addr, elem_size).unwrap();
        Ok(())
    }

    /// Emit a MapContainsKey instruction.
    fn emit_map_contains_key(
        &mut self,
        out: &mut String,
        is_valid: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CAotError> {
        let is_valid_addr = self.value_addr(is_valid);
        let map_addr = self.operand_addr(map);
        let key_addr = self.operand_addr(key);

        let map_ty = self.operand_type(map).clone();
        let key_ty = match &map_ty {
            IrType::Map(k, _) => k.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "MapContainsKey on non-map type: {:?}", map_ty
            ))),
        };

        // The map's descriptor is whichever one tells the truth: inside a
        // generic the static type describes a `%{data = data}`, and looking a
        // `string` up against that ordering finds nothing. The key may have
        // arrived packed, which the runtime decides rather than this.
        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);

        writeln!(out, "    dtlv_rti_btreemap_contains_key_erased_local(rt, {}, {}, {}, &{}, (bool_t*){});",
            map_addr, map_tydesc, key_addr, key_tydesc, is_valid_addr).unwrap();
        Ok(())
    }

    /// Emit a MapGet instruction.
    fn emit_map_get(
        &mut self,
        out: &mut String,
        dest: ValueId,
        is_valid: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let is_valid_addr = self.value_addr(is_valid);
        let map_addr = self.operand_addr(map);
        let key_addr = self.operand_addr(key);

        let map_ty = self.operand_type(map).clone();
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CAotError::Codegen(format!(
                "MapGet on non-map type: {:?}", map_ty
            ))),
        };
        let value_repr = types::ir_type_to_crepr(&value_ty);

        // The truthful map descriptor, and a key the runtime unpacks if it
        // arrived packed. See the containment check above.
        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);
        let value_tydesc = self.tydesc_name(&value_ty);
        let erased = self.operand_ref_desc(map).is_some();

        // Get pointer to value (returns null on miss).
        writeln!(out, "    {{").unwrap();
        writeln!(out, "        void* __vptr;").unwrap();
        writeln!(out, "        dtlv_rti_btreemap_get_value_ref_erased_local(rt, {}, {}, {}, &{}, &__vptr);",
            map_addr, map_tydesc, key_addr, key_tydesc).unwrap();
        writeln!(out, "        *(bool_t*){} = __vptr != NULL;", is_valid_addr).unwrap();

        // Conditionally clone value.
        writeln!(out, "        if (__vptr != NULL) {{").unwrap();

        // Clone value to dest.
        if erased {
            // What is in the map is the real value type and the destination is
            // whatever this function's static type says, so the clone may want
            // wrapping on the way -- the same decision `list_get_erased` makes,
            // and the runtime's for the same reason.
            writeln!(out, "            dtlv_rti_clone_erased_local(rt, __vptr, dtlv_rti_element_tydesc({}), {}, &{});",
                map_tydesc, dest_addr, value_tydesc).unwrap();
        } else {
            match &value_repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "            *({c_ty}*){dest_addr} = *({c_ty}*)__vptr;").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "            dtlv_rti_clone_local(rt, __vptr, &{}, {}, &{});",
                            value_tydesc, dest_addr, value_tydesc).unwrap();
                    }
                }
            }
        }

        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();

        // Conditional tracking byte.
        if let Some(track_offset) = self.layout.value_tracking_byte(dest.0) {
            writeln!(out, "    __frame[{}] = *(bool_t*){} ? TRACK_LIVE : TRACK_UNINIT;",
                track_offset, is_valid_addr).unwrap();
        }

        Ok(())
    }

    /// Emit a MapSetValue instruction.
    fn emit_map_set_value(
        &mut self,
        out: &mut String,
        map: &Operand,
        key: &Operand,
        value: &Operand,
    ) -> Result<(), CAotError> {
        let map_addr = self.operand_addr(map);
        let key_addr = self.operand_addr(key);
        let value_addr = self.operand_addr(value);

        let map_ty = self.operand_type(map).clone();
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CAotError::Codegen(format!(
                "MapSetValue on non-map type: {:?}", map_ty
            ))),
        };

        let map_tydesc = self.tydesc_name(&map_ty);
        let key_tydesc = self.tydesc_name(&key_ty);
        let value_tydesc = self.tydesc_name(&value_ty);

        writeln!(out, "    dtlv_rti_btreemap_set_value_local(rt, {}, &{}, {}, &{}, {}, &{});",
            map_addr, map_tydesc, key_addr, key_tydesc, value_addr, value_tydesc).unwrap();
        Ok(())
    }

    /// Emit a MapUpsert instruction.
    ///
    /// Calls `dtlv_rti_btreemap_insert_local` which inserts if absent or
    /// overwrites if present.
    fn emit_map_upsert(
        &mut self,
        out: &mut String,
        map: &Operand,
        key: &Operand,
        value: &Operand,
    ) -> Result<(), CAotError> {
        let map_addr = self.operand_addr(map);
        let key_addr = self.operand_addr(key);
        let value_addr = self.operand_addr(value);

        let map_ty = self.operand_type(map).clone();
        let (key_ty, value_ty) = match &map_ty {
            IrType::Map(k, v) => (k.as_ref().clone(), v.as_ref().clone()),
            _ => return Err(CAotError::Codegen(format!(
                "MapUpsert on non-map type: {:?}", map_ty
            ))),
        };

        let map_tydesc = self.tydesc_name(&map_ty);
        let key_tydesc = self.tydesc_name(&key_ty);
        let value_tydesc = self.tydesc_name(&value_ty);

        writeln!(out, "    dtlv_rti_btreemap_insert_local(rt, {}, &{}, {}, &{}, {}, &{});",
            map_addr, map_tydesc, key_addr, key_tydesc, value_addr, value_tydesc).unwrap();
        Ok(())
    }

    /// Emit a MapValueRef instruction.
    fn emit_map_value_ref(
        &mut self,
        out: &mut String,
        dest: ValueId,
        map: &Operand,
        key: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let map_addr = self.operand_addr(map);
        let key_addr = self.operand_addr(key);

        let map_ty = self.operand_type(map).clone();
        let key_ty = match &map_ty {
            IrType::Map(k, _) => k.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "MapValueRef on non-map type: {:?}", map_ty
            ))),
        };

        // As at the containment check: the truthful map descriptor, and a key
        // the runtime unpacks if it arrived packed. What the pointer reaches is
        // the map's real value type rather than the `data` this function's
        // static type claims, so the reference carries that descriptor on.
        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);

        writeln!(out, "    dtlv_rti_btreemap_get_value_ref_erased_local(rt, {}, {}, {}, &{}, (void**){});",
            map_addr, map_tydesc, key_addr, key_tydesc, dest_addr).unwrap();
        if self.ref_descs.contains_key(&dest) {
            writeln!(out, "    __rd{} = dtlv_rti_element_tydesc({});", dest.0, map_tydesc).unwrap();
        }
        Ok(())
    }

    /// Emit intrinsic.
    fn emit_intrinsic(
        &mut self,
        out: &mut String,
        dest: ValueId,
        intrinsic: datalove_datafun_intrinsics::IntrinsicId,
        args: &[Operand],
    ) -> Result<(), CAotError> {
        use datalove_datafun_intrinsics::IntrinsicId;

        let dest_addr = self.value_addr(dest);
        let _dest_ty = self.value_type(dest);

        // Helper to get arg addresses.
        let arg0 = || self.operand_addr(&args[0]);
        let arg1 = || self.operand_addr(&args[1]);

        match intrinsic {
            // U32 bitwise operations.
            IntrinsicId::BitnotU32 => {
                writeln!(out, "    *(uint32_t*){} = ~*(uint32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitandU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} & *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitorU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} | *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitxorU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} ^ *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShlU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} << *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU32 => {
                writeln!(out, "    *(uint32_t*){} = __builtin_popcount(*(uint32_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} ? __builtin_clz(*(uint32_t*){}) : 32;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} ? __builtin_ctz(*(uint32_t*){}) : 32;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::SwapBytesU32 => {
                writeln!(out, "    *(uint32_t*){} = __builtin_bswap32(*(uint32_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ReverseBitsU32 => {
                writeln!(out, "    {{ uint32_t __v = *(uint32_t*){}; __v = ((__v >> 1) & 0x55555555u) | ((__v & 0x55555555u) << 1); __v = ((__v >> 2) & 0x33333333u) | ((__v & 0x33333333u) << 2); __v = ((__v >> 4) & 0x0F0F0F0Fu) | ((__v & 0x0F0F0F0Fu) << 4); *(uint32_t*){} = __builtin_bswap32(__v); }}", arg0(), dest_addr).unwrap();
            }
            IntrinsicId::AddWrappingU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} + *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SubWrappingU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} - *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MulWrappingU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} * *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::RemU32 => {
                writeln!(out, "    *(uint32_t*){} = *(uint32_t*){} % *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::U32ToI32 => {
                writeln!(out, "    *(int32_t*){} = (int32_t)*(uint32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I32ToU32 => {
                writeln!(out, "    *(uint32_t*){} = (uint32_t)*(int32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::NegWrappingI32 => {
                writeln!(out, "    *(int32_t*){} = -*(int32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SshrI32 => {
                writeln!(out, "    *(int32_t*){} = *(int32_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SremI32 => {
                writeln!(out, "    *(int32_t*){} = *(int32_t*){} % *(int32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }

            // Platform query.
            IntrinsicId::IsBigEndian => {
                writeln!(out, "    *(bool_t*){} = 0; // Assuming little-endian", dest_addr).unwrap();
            }

            // F32 operations.
            IntrinsicId::IsNanF32 => {
                writeln!(out, "    *(bool_t*){} = isnan(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::IsInfiniteF32 => {
                writeln!(out, "    *(bool_t*){} = isinf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::F32ToBits => {
                writeln!(out, "    memcpy({}, {}, 4);", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitsToF32 => {
                writeln!(out, "    memcpy({}, {}, 4);", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::AbsF32 => {
                writeln!(out, "    *(float*){} = fabsf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SqrtF32 => {
                writeln!(out, "    *(float*){} = sqrtf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::FloorF32 => {
                writeln!(out, "    *(float*){} = floorf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::CeilF32 => {
                writeln!(out, "    *(float*){} = ceilf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::RoundF32 => {
                writeln!(out, "    *(float*){} = roundf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::TruncF32 => {
                writeln!(out, "    *(float*){} = truncf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::CopysignF32 => {
                writeln!(out, "    *(float*){} = copysignf(*(float*){}, *(float*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MinF32 => {
                writeln!(out, "    *(float*){} = fminf(*(float*){}, *(float*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MaxF32 => {
                writeln!(out, "    *(float*){} = fmaxf(*(float*){}, *(float*){});", dest_addr, arg0(), arg1()).unwrap();
            }

            // F64 operations.
            IntrinsicId::IsNanF64 => {
                writeln!(out, "    *(bool_t*){} = isnan(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::IsInfiniteF64 => {
                writeln!(out, "    *(bool_t*){} = isinf(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::F32ToF64 => {
                writeln!(out, "    *(double*){} = (double)*(float*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::F64ToF32 => {
                writeln!(out, "    *(float*){} = (float)*(double*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::F64ToBits => {
                writeln!(out, "    memcpy({}, {}, 8);", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitsToF64 => {
                writeln!(out, "    memcpy({}, {}, 8);", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::AbsF64 => {
                writeln!(out, "    *(double*){} = fabs(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SqrtF64 => {
                writeln!(out, "    *(double*){} = sqrt(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::FloorF64 => {
                writeln!(out, "    *(double*){} = floor(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::CeilF64 => {
                writeln!(out, "    *(double*){} = ceil(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::RoundF64 => {
                writeln!(out, "    *(double*){} = round(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::TruncF64 => {
                writeln!(out, "    *(double*){} = trunc(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::CopysignF64 => {
                writeln!(out, "    *(double*){} = copysign(*(double*){}, *(double*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MinF64 => {
                writeln!(out, "    *(double*){} = fmin(*(double*){}, *(double*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MaxF64 => {
                writeln!(out, "    *(double*){} = fmax(*(double*){}, *(double*){});", dest_addr, arg0(), arg1()).unwrap();
            }

            // U64 operations.
            IntrinsicId::BitnotU64 => {
                writeln!(out, "    *(uint64_t*){} = ~*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitandU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} & *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitorU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} | *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitxorU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} ^ *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShlU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} << *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU64 => {
                writeln!(out, "    *(uint32_t*){} = (uint32_t)__builtin_popcountll(*(uint64_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU64 => {
                writeln!(out, "    *(uint32_t*){} = *(uint64_t*){} ? (uint32_t)__builtin_clzll(*(uint64_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU64 => {
                writeln!(out, "    *(uint32_t*){} = *(uint64_t*){} ? (uint32_t)__builtin_ctzll(*(uint64_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::SwapBytesU64 => {
                writeln!(out, "    *(uint64_t*){} = __builtin_bswap64(*(uint64_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ReverseBitsU64 => {
                writeln!(out, "    {{ uint64_t __v = *(uint64_t*){}; __v = ((__v >> 1) & 0x5555555555555555ull) | ((__v & 0x5555555555555555ull) << 1); __v = ((__v >> 2) & 0x3333333333333333ull) | ((__v & 0x3333333333333333ull) << 2); __v = ((__v >> 4) & 0x0F0F0F0F0F0F0F0Full) | ((__v & 0x0F0F0F0F0F0F0F0Full) << 4); *(uint64_t*){} = __builtin_bswap64(__v); }}", arg0(), dest_addr).unwrap();
            }
            IntrinsicId::AddWrappingU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} + *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SubWrappingU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} - *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MulWrappingU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} * *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::RemU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} % *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::U64ToI64 => {
                writeln!(out, "    *(int64_t*){} = (int64_t)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToU64 => {
                writeln!(out, "    *(uint64_t*){} = (uint64_t)*(int64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::NegWrappingI64 => {
                writeln!(out, "    *(int64_t*){} = -*(int64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SshrI64 => {
                writeln!(out, "    *(int64_t*){} = *(int64_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SremI64 => {
                writeln!(out, "    *(int64_t*){} = *(int64_t*){} % *(int64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }

            // U8 operations.
            IntrinsicId::BitnotU8 => {
                writeln!(out, "    *(uint8_t*){} = ~*(uint8_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitandU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} & *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitorU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} | *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitxorU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} ^ *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShlU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} << *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU8 => {
                writeln!(out, "    *(uint32_t*){} = __builtin_popcount(*(uint8_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU8 => {
                writeln!(out, "    *(uint32_t*){} = *(uint8_t*){} ? (uint32_t)(__builtin_clz(*(uint8_t*){}) - 24) : 8;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU8 => {
                writeln!(out, "    *(uint32_t*){} = *(uint8_t*){} ? (uint32_t)__builtin_ctz(*(uint8_t*){}) : 8;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::ReverseBitsU8 => {
                writeln!(out, "    {{ uint8_t __v = *(uint8_t*){}; __v = ((__v >> 1) & 0x55) | ((__v & 0x55) << 1); __v = ((__v >> 2) & 0x33) | ((__v & 0x33) << 2); *(uint8_t*){} = (__v >> 4) | (__v << 4); }}", arg0(), dest_addr).unwrap();
            }
            IntrinsicId::AddWrappingU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)(*(uint8_t*){} + *(uint8_t*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SubWrappingU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)(*(uint8_t*){} - *(uint8_t*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MulWrappingU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)(*(uint8_t*){} * *(uint8_t*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::RemU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} % *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::U8ToI8 => {
                writeln!(out, "    *(int8_t*){} = (int8_t)*(uint8_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I8ToU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)*(int8_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::NegWrappingI8 => {
                writeln!(out, "    *(int8_t*){} = -*(int8_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SshrI8 => {
                writeln!(out, "    *(int8_t*){} = *(int8_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SremI8 => {
                writeln!(out, "    *(int8_t*){} = *(int8_t*){} % *(int8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }

            // U16 operations.
            IntrinsicId::BitnotU16 => {
                writeln!(out, "    *(uint16_t*){} = ~*(uint16_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitandU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} & *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitorU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} | *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitxorU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} ^ *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShlU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} << *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU16 => {
                writeln!(out, "    *(uint32_t*){} = __builtin_popcount(*(uint16_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU16 => {
                writeln!(out, "    *(uint32_t*){} = *(uint16_t*){} ? (uint32_t)(__builtin_clz(*(uint16_t*){}) - 16) : 16;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU16 => {
                writeln!(out, "    *(uint32_t*){} = *(uint16_t*){} ? (uint32_t)__builtin_ctz(*(uint16_t*){}) : 16;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::SwapBytesU16 => {
                writeln!(out, "    *(uint16_t*){} = __builtin_bswap16(*(uint16_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ReverseBitsU16 => {
                writeln!(out, "    {{ uint16_t __v = *(uint16_t*){}; __v = ((__v >> 1) & 0x5555) | ((__v & 0x5555) << 1); __v = ((__v >> 2) & 0x3333) | ((__v & 0x3333) << 2); __v = ((__v >> 4) & 0x0F0F) | ((__v & 0x0F0F) << 4); *(uint16_t*){} = __builtin_bswap16(__v); }}", arg0(), dest_addr).unwrap();
            }
            IntrinsicId::AddWrappingU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)(*(uint16_t*){} + *(uint16_t*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SubWrappingU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)(*(uint16_t*){} - *(uint16_t*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MulWrappingU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)(*(uint16_t*){} * *(uint16_t*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::RemU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} % *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::U16ToI16 => {
                writeln!(out, "    *(int16_t*){} = (int16_t)*(uint16_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I16ToU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)*(int16_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::NegWrappingI16 => {
                writeln!(out, "    *(int16_t*){} = -*(int16_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SshrI16 => {
                writeln!(out, "    *(int16_t*){} = *(int16_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SremI16 => {
                writeln!(out, "    *(int16_t*){} = *(int16_t*){} % *(int16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }

            // Index operations.
            IntrinsicId::BitnotIndex => {
                writeln!(out, "    *(index_t*){} = ~*(index_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::BitandIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} & *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitorIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} | *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::BitxorIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} ^ *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShlIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} << *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(uint32_t*){} = __builtin_popcount(*(index_t*){});", dest_addr, arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(uint32_t*){} = (uint32_t)__builtin_popcountll(*(index_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(uint32_t*){} = *(index_t*){} ? __builtin_clz(*(index_t*){}) : 32;", dest_addr, arg0(), arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(uint32_t*){} = *(index_t*){} ? __builtin_clzll(*(index_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(uint32_t*){} = *(index_t*){} ? __builtin_ctz(*(index_t*){}) : 32;", dest_addr, arg0(), arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(uint32_t*){} = *(index_t*){} ? __builtin_ctzll(*(index_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::SwapBytesIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(index_t*){} = __builtin_bswap32(*(index_t*){});", dest_addr, arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(index_t*){} = __builtin_bswap64(*(index_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ReverseBitsIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    {{ index_t __v = *(index_t*){}; __v = ((__v >> 1) & 0x55555555u) | ((__v & 0x55555555u) << 1); __v = ((__v >> 2) & 0x33333333u) | ((__v & 0x33333333u) << 2); __v = ((__v >> 4) & 0x0F0F0F0Fu) | ((__v & 0x0F0F0F0Fu) << 4); *(index_t*){} = __builtin_bswap32(__v); }}", arg0(), dest_addr).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    {{ index_t __v = *(index_t*){}; __v = ((__v >> 1) & 0x5555555555555555ull) | ((__v & 0x5555555555555555ull) << 1); __v = ((__v >> 2) & 0x3333333333333333ull) | ((__v & 0x3333333333333333ull) << 2); __v = ((__v >> 4) & 0x0F0F0F0F0F0F0F0Full) | ((__v & 0x0F0F0F0F0F0F0F0Full) << 4); *(index_t*){} = __builtin_bswap64(__v); }}", arg0(), dest_addr).unwrap();
            }
            IntrinsicId::AddWrappingIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} + *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SubWrappingIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} - *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MulWrappingIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} * *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::RemIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} % *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::IndexToOffset => {
                writeln!(out, "    *(offset_t*){} = (offset_t)*(index_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::IndexBits => {
                writeln!(out, "    *(uint32_t*){} = DTLV_INDEX_BITS;", dest_addr).unwrap();
            }

            // Offset operations.
            IntrinsicId::OffsetToIndex => {
                writeln!(out, "    *(index_t*){} = (index_t)*(offset_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::NegWrappingOffset => {
                writeln!(out, "    *(offset_t*){} = -*(offset_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SshrOffset => {
                writeln!(out, "    *(offset_t*){} = *(offset_t*){} >> *(uint32_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::SremOffset => {
                writeln!(out, "    *(offset_t*){} = *(offset_t*){} % *(offset_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
        }
        Ok(())
    }

    /// Mark a slot as live.
    fn emit_mark_slot_live(&self, out: &mut String, sid: SlotId) -> Result<(), CAotError> {
        if let Some(track_offset) = self.layout.slot_tracking_byte(sid.0) {
            writeln!(out, "    __frame[{}] = TRACK_LIVE;", track_offset).unwrap();
        }
        Ok(())
    }

    /// Mark a slot as moved.
    fn emit_mark_slot_moved(&self, out: &mut String, sid: SlotId) -> Result<(), CAotError> {
        if let Some(track_offset) = self.layout.slot_tracking_byte(sid.0) {
            writeln!(out, "    __frame[{}] = TRACK_MOVED;", track_offset).unwrap();
        }
        Ok(())
    }

    /// Emit a terminator.
    fn emit_terminator(&mut self, out: &mut String, term: &Terminator) -> Result<(), CAotError> {
        match term {
            Terminator::Goto { target, args } => {
                self.emit_goto(out, *target, args)?;
            }
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                self.emit_branch(out, cond, *then_block, then_args, *else_block, else_args)?;
            }
            Terminator::Return { value } => {
                self.emit_return(out, value.as_ref())?;
            }
            Terminator::UnitEnd { result } => {
                self.emit_unit_end(out, result.as_ref())?;
            }
            Terminator::UnitEarlyReturn { value } => {
                // Early return - debuglog the value (to match interpreter behavior), then destroy and return.
                self.emit_debuglog(out, value)?;

                // Destroy the value if it's non-copy to prevent leaks.
                let ty = self.operand_type(value).clone();
                if !ty.is_copy() {
                    let addr = self.operand_addr(value);
                    let tydesc = self.tydesc_name(&ty);
                    writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", addr, tydesc).unwrap();
                }

                writeln!(out, "    return;").unwrap();
            }
            Terminator::Switch { discriminant, cases, default } => {
                let disc_addr = self.operand_addr(discriminant);
                writeln!(out, "    switch (*(uint32_t*){}) {{", disc_addr).unwrap();
                for (val, target) in cases {
                    writeln!(out, "    case {}: goto __block_{};", val, target.0).unwrap();
                }
                writeln!(out, "    default: goto __block_{};", default.0).unwrap();
                writeln!(out, "    }}").unwrap();
            }
        }
        Ok(())
    }

    /// Emit goto.
    fn emit_goto(&mut self, out: &mut String, target: BlockId, args: &[Operand]) -> Result<(), CAotError> {
        // Copy args to target block's param locations.
        let target_block = self.unit.blocks.iter()
            .find(|b| b.id == target)
            .ok_or_else(|| CAotError::Codegen(format!("block {:?} not found", target)))?;

        for (param_vid, arg_op) in target_block.params.iter().zip(args.iter()) {
            let dest_addr = self.value_addr(*param_vid);
            let src_addr = self.operand_addr(arg_op);
            let ty = self.operand_type(arg_op);
            let repr = types::ir_type_to_crepr(ty);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, layout.size).unwrap();
                    }
                }
            }
        }

        writeln!(out, "    goto __block_{};", target.0).unwrap();
        Ok(())
    }

    /// Emit branch.
    fn emit_branch(
        &mut self,
        out: &mut String,
        cond: &Operand,
        then_block: BlockId,
        then_args: &[Operand],
        else_block: BlockId,
        else_args: &[Operand],
    ) -> Result<(), CAotError> {
        let cond_addr = self.operand_addr(cond);

        writeln!(out, "    if (*(bool_t*){}) {{", cond_addr).unwrap();

        // Copy then args.
        let then_blk = self.unit.blocks.iter()
            .find(|b| b.id == then_block)
            .ok_or_else(|| CAotError::Codegen(format!("block {:?} not found", then_block)))?;

        for (param_vid, arg_op) in then_blk.params.iter().zip(then_args.iter()) {
            let dest_addr = self.value_addr(*param_vid);
            let src_addr = self.operand_addr(arg_op);
            let ty = self.operand_type(arg_op);
            let repr = types::ir_type_to_crepr(ty);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "        *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "        memcpy({}, {}, {});", dest_addr, src_addr, layout.size).unwrap();
                    }
                }
            }
        }

        writeln!(out, "        goto __block_{};", then_block.0).unwrap();
        writeln!(out, "    }} else {{").unwrap();

        // Copy else args.
        let else_blk = self.unit.blocks.iter()
            .find(|b| b.id == else_block)
            .ok_or_else(|| CAotError::Codegen(format!("block {:?} not found", else_block)))?;

        for (param_vid, arg_op) in else_blk.params.iter().zip(else_args.iter()) {
            let dest_addr = self.value_addr(*param_vid);
            let src_addr = self.operand_addr(arg_op);
            let ty = self.operand_type(arg_op);
            let repr = types::ir_type_to_crepr(ty);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "        *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "        memcpy({}, {}, {});", dest_addr, src_addr, layout.size).unwrap();
                    }
                }
            }
        }

        writeln!(out, "        goto __block_{};", else_block.0).unwrap();
        writeln!(out, "    }}").unwrap();
        Ok(())
    }

    /// Emit return.
    fn emit_return(&mut self, out: &mut String, value: Option<&Operand>) -> Result<(), CAotError> {
        if let Some(val) = value {
            let func_ctx = self.unit.function_context().unwrap();
            let ret_ty = &func_ctx.return_type;

            if self.uses_sret {
                // Copy to sret pointer.
                let src_addr = self.operand_addr(val);
                let layout = types::ir_type_to_crepr(ret_ty).layout();
                if layout.size > 0 {
                    writeln!(out, "    memcpy(__sret, {}, {});", src_addr, layout.size).unwrap();
                }
                writeln!(out, "    return;").unwrap();
            } else {
                let src_addr = self.operand_addr(val);
                let c_ty = types::ir_type_to_c(ret_ty);
                writeln!(out, "    return *({c_ty}*){src_addr};").unwrap();
            }
        } else {
            writeln!(out, "    return;").unwrap();
        }
        Ok(())
    }

    /// Emit unit end.
    fn emit_unit_end(&mut self, out: &mut String, _result: Option<&Operand>) -> Result<(), CAotError> {
        // Script ends here.
        writeln!(out, "    return;").unwrap();
        Ok(())
    }

    // ========================================================================
    // Tensor Indexing
    // ========================================================================

    /// Emit a TensorBoundsCheck instruction.
    fn emit_tensor_bounds_check(
        &mut self,
        out: &mut String,
        is_valid: ValueId,
        tensor: &Operand,
        index: &Operand,
    ) -> Result<(), CAotError> {
        let is_valid_addr = self.value_addr(is_valid);
        let tensor_addr = self.operand_addr(tensor);
        let index_addr = self.operand_addr(index);
        let shape_offset = std::mem::offset_of!(datalove_rtdt::Tensor, shape);

        // shape_ptr = tensor.shape; dim0 = *shape_ptr; valid = index < dim0
        writeln!(out, "    {{ index_t* __shape = *(index_t**)({} + {});", tensor_addr, shape_offset).unwrap();
        writeln!(out, "    *(bool_t*){} = (*(index_t*){} < *__shape); }}", is_valid_addr, index_addr).unwrap();
        Ok(())
    }

    /// Emit a TensorGet instruction.
    fn emit_tensor_get(
        &mut self,
        out: &mut String,
        dest: ValueId,
        is_valid: ValueId,
        tensor: &Operand,
        index: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let is_valid_addr = self.value_addr(is_valid);
        let tensor_addr = self.operand_addr(tensor);
        let index_addr = self.operand_addr(index);

        let tensor_ty = self.operand_type(tensor).clone();
        let (elem_ty, rank) = match &tensor_ty {
            IrType::Tensor(e, r) => (e.as_ref().clone(), *r),
            _ => return Err(CAotError::Codegen(format!(
                "TensorGet on non-tensor type: {:?}", tensor_ty
            ))),
        };

        let shape_offset = std::mem::offset_of!(datalove_rtdt::Tensor, shape);

        // Bounds check.
        writeln!(out, "    {{ index_t* __shape = *(index_t**)({} + {});", tensor_addr, shape_offset).unwrap();
        writeln!(out, "    *(bool_t*){} = (*(index_t*){} < *__shape);", is_valid_addr, index_addr).unwrap();
        writeln!(out, "    if (*(bool_t*){}) {{", is_valid_addr).unwrap();

        if rank == 1 {
            let elem_repr = types::ir_type_to_crepr(&elem_ty);
            let ptr_base_offset = std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base);
            let offset_elems_offset = std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems);
            let strides_offset = std::mem::offset_of!(datalove_rtdt::Tensor, strides);

            // Inside a generic the static element type is a `data`, whose width
            // is not the width of what the caller's tensor really holds, so the
            // stride comes from the tensor's own descriptor.
            let erased_desc = self.operand_ref_desc(tensor)
                .map(|d| format!("dtlv_rti_element_tydesc({})", d));
            let elem_size = match &erased_desc {
                Some(elem) => format!("{}->size", elem),
                None => elem_repr.layout().size.to_string(),
            };

            // elem_addr = ptr_base + (offset + index * strides[0]) * elem_size
            writeln!(out, "        void* __elem = *(void**)({t} + {pb}) + (size_t)(*(index_t*)({t} + {oe}) + *(index_t*){idx} * **(index_t**)({t} + {st})) * {es};",
                t = tensor_addr, pb = ptr_base_offset, oe = offset_elems_offset,
                idx = index_addr, st = strides_offset, es = elem_size).unwrap();

            match (&erased_desc, &elem_repr) {
                // What is in the tensor is the real element type and the
                // destination is whatever this function's static type says, so
                // the clone may want wrapping on the way.
                (Some(elem), _) => {
                    let dest_tydesc = self.tydesc_name(&elem_ty);
                    writeln!(out, "        dtlv_rti_clone_erased_local(rt, __elem, {}, {}, &{});",
                        elem, dest_addr, dest_tydesc).unwrap();
                }
                (None, CRepr::Scalar(c_ty)) => {
                    writeln!(out, "        *({c_ty}*){dest_addr} = *({c_ty}*)__elem;").unwrap();
                }
                (None, CRepr::Aggregate(layout)) => {
                    if layout.size > 0 {
                        let tydesc = self.tydesc_name(&elem_ty);
                        writeln!(out, "        dtlv_rti_clone_local(rt, __elem, &{}, {}, &{});",
                            tydesc, dest_addr, tydesc).unwrap();
                    }
                }
            }
        } else {
            // Rank > 1: call hyperplane_clone.
            let tensor_tydesc = self.tydesc_name(&tensor_ty);
            writeln!(out, "        dtlv_rti_tensor_hyperplane_clone_local(rt, {}, &{}, *(index_t*){}, {});",
                tensor_addr, tensor_tydesc, index_addr, dest_addr).unwrap();
        }

        writeln!(out, "    }} }}").unwrap();

        // Conditional tracking byte.
        if let Some(track_offset) = self.layout.value_tracking_byte(dest.0) {
            writeln!(out, "    __frame[{}] = *(bool_t*){} ? TRACK_LIVE : TRACK_UNINIT;",
                track_offset, is_valid_addr).unwrap();
        }

        Ok(())
    }

    /// Emit a TensorSet instruction.
    fn emit_tensor_set(
        &mut self,
        out: &mut String,
        tensor: &Operand,
        index: &Operand,
        value: &Operand,
    ) -> Result<(), CAotError> {
        let tensor_addr = self.operand_addr(tensor);
        let index_addr = self.operand_addr(index);
        let value_addr = self.operand_addr(value);

        let tensor_ty = self.operand_type(tensor).clone();
        let elem_ty = match &tensor_ty {
            IrType::Tensor(e, _) => e.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "TensorSet on non-tensor type: {:?}", tensor_ty
            ))),
        };
        let elem_repr = types::ir_type_to_crepr(&elem_ty);
        let elem_size = elem_repr.layout().size;
        let elem_tydesc = self.tydesc_name(&elem_ty);

        let ptr_base_offset = std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base);
        let offset_elems_offset = std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems);
        let strides_offset = std::mem::offset_of!(datalove_rtdt::Tensor, strides);

        writeln!(out, "    {{ void* __elem = *(void**)({t} + {pb}) + (size_t)(*(index_t*)({t} + {oe}) + *(index_t*){idx} * **(index_t**)({t} + {st})) * {es};",
            t = tensor_addr, pb = ptr_base_offset, oe = offset_elems_offset,
            idx = index_addr, st = strides_offset, es = elem_size).unwrap();

        // Destroy old element.
        writeln!(out, "    dtlv_rti_any_destroy_local(rt, __elem, &{});", elem_tydesc).unwrap();

        // Store new value.
        match &elem_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*)__elem = *({c_ty}*){value_addr};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if layout.size > 0 {
                    writeln!(out, "    dtlv_rti_move_value_local(rt, {}, &{}, __elem);",
                        value_addr, elem_tydesc).unwrap();
                }
            }
        }

        writeln!(out, "    }}").unwrap();
        Ok(())
    }

    /// Emit a TensorIndexRef instruction.
    fn emit_tensor_index_ref(
        &mut self,
        out: &mut String,
        dest: ValueId,
        tensor: &Operand,
        index: &Operand,
    ) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let tensor_addr = self.operand_addr(tensor);
        let index_addr = self.operand_addr(index);

        let tensor_ty = self.operand_type(tensor).clone();
        let (elem_ty, rank) = match &tensor_ty {
            IrType::Tensor(e, r) => (e.as_ref().clone(), *r),
            _ => return Err(CAotError::Codegen(format!(
                "TensorIndexRef on non-tensor type: {:?}", tensor_ty
            ))),
        };

        let ptr_base_offset = std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base);
        let offset_elems_offset = std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems);
        let strides_offset = std::mem::offset_of!(datalove_rtdt::Tensor, strides);

        if rank == 1 {
            // As at TensorGet: inside a generic the stride comes from the
            // tensor's own descriptor, and the element's descriptor goes on to
            // describe this reference.
            let elem_size = match self.operand_ref_desc(tensor) {
                Some(desc) => {
                    writeln!(out, "    __rd{} = dtlv_rti_element_tydesc({});", dest.0, desc).unwrap();
                    format!("__rd{}->size", dest.0)
                }
                None => types::ir_type_to_crepr(&elem_ty).layout().size.to_string(),
            };

            // Element pointer.
            writeln!(out, "    *(void**){} = *(void**)({t} + {pb}) + (size_t)(*(index_t*)({t} + {oe}) + *(index_t*){idx} * **(index_t**)({t} + {st})) * {es};",
                dest_addr,
                t = tensor_addr, pb = ptr_base_offset, oe = offset_elems_offset,
                idx = index_addr, st = strides_offset, es = elem_size).unwrap();
        } else {
            // Rank > 1: construct view tensor at dest.
            // For C AOT, the dest is a Ref (pointer), so we need stack storage for the view.
            let shape_offset = std::mem::offset_of!(datalove_rtdt::Tensor, shape);
            let layout_offset = std::mem::offset_of!(datalove_rtdt::Tensor, layout);
            let capacity_offset = std::mem::offset_of!(datalove_rtdt::Tensor, capacity_elems);
            let index_size = std::mem::size_of::<datalove_rtdt::Index>();
            let tensor_size = std::mem::size_of::<datalove_rtdt::Tensor>();

            writeln!(out, "    {{ /* TensorIndexRef rank > 1: construct view */").unwrap();
            // Allocate view tensor on C stack as a local variable.
            writeln!(out, "    static _Alignas(8) uint8_t __view[{}];", tensor_size).unwrap();
            // Copy ptr_base.
            writeln!(out, "    *(void**)(__view + {}) = *(void**)({} + {});",
                ptr_base_offset, tensor_addr, ptr_base_offset).unwrap();
            // Set capacity_elems = 0.
            writeln!(out, "    *(index_t*)(__view + {}) = 0;", capacity_offset).unwrap();
            // new_offset = offset + idx * strides[0].
            writeln!(out, "    *(index_t*)(__view + {oe}) = *(index_t*)({t} + {oe}) + *(index_t*){idx} * **(index_t**)({t} + {st});",
                oe = offset_elems_offset, t = tensor_addr, idx = index_addr, st = strides_offset).unwrap();
            // shape = parent.shape + 1.
            writeln!(out, "    *(void**)(__view + {}) = (void*)(*(index_t**)({} + {}) + 1);",
                shape_offset, tensor_addr, shape_offset).unwrap();
            // strides = parent.strides + 1.
            writeln!(out, "    *(void**)(__view + {}) = (void*)(*(index_t**)({} + {}) + 1);",
                strides_offset, tensor_addr, strides_offset).unwrap();
            let _ = index_size; // Pointer arithmetic on index_t* already handles element size.
            // layout = parent.layout.
            writeln!(out, "    *(uint8_t*)(__view + {}) = *(uint8_t*)({} + {});",
                layout_offset, tensor_addr, layout_offset).unwrap();
            // Store pointer to view as the ref.
            writeln!(out, "    *(void**){} = __view; }}", dest_addr).unwrap();
        }

        Ok(())
    }
}
