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

    // Create function context for codegen.
    let mut ctx = FunctionCodegenContext {
        unit,
        parent_unit,
        layout: &layout,
        compiler,
        registry,
        uses_sret,
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
    let mut ctx = FunctionCodegenContext {
        unit,
        parent_unit: None,
        layout: &layout,
        compiler,
        registry,
        uses_sret: false,
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
            Instruction::Reify { dest, src } => {
                self.emit_erasure(out, *dest, src, false)?;
            }
            Instruction::ListNew { dest, elements } => {
                self.emit_list_new(out, *dest, elements)?;
            }
            Instruction::SetNew { dest, elements } => {
                self.emit_set_new(out, *dest, elements)?;
            }
            Instruction::MapNew { dest, entries } => {
                self.emit_map_new(out, *dest, entries)?;
            }
            Instruction::TensorNew { dest, shape, elements } => {
                self.emit_tensor_new(out, *dest, shape, elements)?;
            }
            Instruction::TableNew { dest, rows } => {
                self.emit_table_new(out, *dest, rows)?;
            }
            Instruction::Call { dest, func, args, .. } |
            Instruction::ComptimeCall { dest, func, args, .. } => {
                self.emit_call(out, *dest, func, args)?;
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

    /// Emit a constant.
    fn emit_const(&mut self, out: &mut String, dest: ValueId, value: &ConstValue) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let ty = self.value_type(dest);

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
            ConstValue::F32(v) => {
                if v.is_nan() {
                    writeln!(out, "    *(float*){} = (0.0f/0.0f);", dest_addr).unwrap();
                } else if v.is_infinite() {
                    if *v > 0.0 {
                        writeln!(out, "    *(float*){} = (1.0f/0.0f);", dest_addr).unwrap();
                    } else {
                        writeln!(out, "    *(float*){} = (-1.0f/0.0f);", dest_addr).unwrap();
                    }
                } else {
                    // Use {:e} to ensure scientific notation with decimal point.
                    writeln!(out, "    *(float*){} = {:e}f;", dest_addr, v).unwrap();
                }
            }
            ConstValue::F64(v) => {
                if v.is_nan() {
                    writeln!(out, "    *(double*){} = (0.0/0.0);", dest_addr).unwrap();
                } else if v.is_infinite() {
                    if *v > 0.0 {
                        writeln!(out, "    *(double*){} = (1.0/0.0);", dest_addr).unwrap();
                    } else {
                        writeln!(out, "    *(double*){} = (-1.0/0.0);", dest_addr).unwrap();
                    }
                } else {
                    // Use {:e} to ensure scientific notation with decimal point.
                    writeln!(out, "    *(double*){} = {:e};", dest_addr, v).unwrap();
                }
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
            // Handle other const values as TODO for now.
            _ => {
                writeln!(out, "    /* TODO: const value {:?} */", value).unwrap();
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
        let ty = self.operand_type(src).clone();
        let tydesc = self.tydesc_name(&ty);

        writeln!(out, "    dtlv_rti_clone_local(rt, {}, &{}, {}, &{});", src_addr, tydesc, dest_addr, tydesc).unwrap();
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
            // Division and modulo need explicit zero-check.
            (_, BinOp::Div) | (_, BinOp::Mod) => {
                // Check for division by zero.
                writeln!(out, "    if (*({c_ty}*){rhs_addr} == 0) {{").unwrap();
                writeln!(out, "        *(bool_t*){overflow_addr} = 1;").unwrap();
                writeln!(out, "        *({c_ty}*){dest_addr} = 0;").unwrap();
                writeln!(out, "    }} else {{").unwrap();
                writeln!(out, "        *(bool_t*){overflow_addr} = 0;").unwrap();
                writeln!(out, "        *({c_ty}*){dest_addr} = *({c_ty}*){lhs_addr} {} *({c_ty}*){rhs_addr};",
                    if op == BinOp::Div { "/" } else { "%" }).unwrap();
                writeln!(out, "    }}").unwrap();
                return Ok(());
            }
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
        let src_ty = self.operand_type(operand);
        let c_ty = types::ir_type_to_c(src_ty);

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

    /// Emit get field ref (get pointer to field).
    fn emit_get_field_ref(&mut self, out: &mut String, dest: ValueId, src: &Operand, field_index: u32) -> Result<(), CAotError> {
        let src_addr = self.operand_addr(src);
        let src_ty = self.operand_type(src);
        let dest_addr = self.value_addr(dest);

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
        let src_size = types::ir_type_to_crepr(&src_ty).layout().size;
        let func = if erasing { "dtlv_rti_erase_local" } else { "dtlv_rti_reify_local" };

        writeln!(out, "    {}(rt, {}, &{}, {}, &{});", func, src_addr, src_tydesc, dest_addr, dest_tydesc).unwrap();

        if src_size > 0 {
            writeln!(out, "    memset({}, 0, {});", src_addr, src_size).unwrap();
        }
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
        let elem_layout = types::ir_type_to_crepr(&elem_ty).layout();

        if elements.is_empty() {
            writeln!(out, "    dtlv_rti_btreeset_create_local(rt, {}, &{});", dest_addr, elem_tydesc).unwrap();
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

            writeln!(out, "    dtlv_rti_btreeset_build_from_sorted_slice_local(rt, {}, &{}, __elems, {}); }}",
                dest_addr, elem_tydesc, elements.len()).unwrap();
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
        let key_layout = types::ir_type_to_crepr(&key_ty).layout();
        let val_layout = types::ir_type_to_crepr(&val_ty).layout();

        if entries.is_empty() {
            writeln!(out, "    dtlv_rti_btreemap_create_local(rt, {}, &{});", dest_addr, key_tydesc).unwrap();
        } else {
            let keys_size = key_layout.size * entries.len() as u32;
            let vals_size = val_layout.size * entries.len() as u32;

            writeln!(out, "    {{ uint8_t __keys[{}]; uint8_t __vals[{}];", keys_size, vals_size).unwrap();

            for (i, (key_op, val_op)) in entries.iter().enumerate() {
                let key_addr = self.operand_addr(key_op);
                let val_addr = self.operand_addr(val_op);
                let key_offset = i as u32 * key_layout.size;
                let val_offset = i as u32 * val_layout.size;

                match types::ir_type_to_crepr(&key_ty) {
                    CRepr::Scalar(c_ty) => {
                        writeln!(out, "    *({}*)(__keys + {}) = *({}*){};", c_ty, key_offset, c_ty, key_addr).unwrap();
                    }
                    CRepr::Aggregate(layout) => {
                        if layout.size > 0 {
                            writeln!(out, "    memcpy(__keys + {}, {}, {});", key_offset, key_addr, layout.size).unwrap();
                        }
                    }
                }

                match types::ir_type_to_crepr(&val_ty) {
                    CRepr::Scalar(c_ty) => {
                        writeln!(out, "    *({}*)(__vals + {}) = *({}*){};", c_ty, val_offset, c_ty, val_addr).unwrap();
                    }
                    CRepr::Aggregate(layout) => {
                        if layout.size > 0 {
                            writeln!(out, "    memcpy(__vals + {}, {}, {});", val_offset, val_addr, layout.size).unwrap();
                        }
                    }
                }
            }

            writeln!(out, "    dtlv_rti_btreemap_build_from_sorted_slices_local(rt, {}, &{}, &{}, __keys, __vals, {}); }}",
                dest_addr, key_tydesc, val_tydesc, entries.len()).unwrap();
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

        // Emit shape array.
        let shape_str: Vec<String> = shape.iter().map(|s| s.to_string()).collect();
        writeln!(out, "    static const index_t __shape[] = {{ {} }};", shape_str.join(", ")).unwrap();

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
            writeln!(out, "    {{ uint8_t __rows[{}];", total_size.max(1)).unwrap();

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
    fn emit_call(&mut self, out: &mut String, dest: ValueId, func: &CodeRef, args: &[Operand]) -> Result<(), CAotError> {
        // A rider function is reached through the runtime C ABI rather than
        // this backend's own convention, so it is a different call entirely.
        if let CodeRef::Module { module, id } = func {
            if let Some(unit) = self.registry.get_module_function_as_unit(*module, *id) {
                if let Some(native) = unit.native_context() {
                    let symbol = native.symbol.clone();
                    return self.emit_native_call(out, dest, &symbol, args);
                }
            }
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
        for arg in args {
            write!(&mut call_args, ", {}", self.operand_addr(arg)).unwrap();
        }

        // Then a descriptor for each parameter whose own type does not say what
        // arrives at it. This is the place that knows: the argument here either
        // has a concrete type, or is a parameter our own caller described.
        for param_id in self.callee_descriptor_params(func) {
            let arg = args.get(param_id.0 as usize).ok_or_else(|| CAotError::Codegen(format!(
                "callee wants a descriptor for parameter {} but got {} arguments",
                param_id.0, args.len(),
            )))?;
            write!(&mut call_args, ", {}", self.operand_tydesc(arg)).unwrap();
        }

        if uses_sret || dest_ty == IrType::Unit {
            writeln!(out, "    {}({});", func_name, call_args).unwrap();
        } else {
            let dest_addr = self.value_addr(dest);
            let c_ty = types::ir_type_to_c(&dest_ty);
            writeln!(out, "    *({c_ty}*){dest_addr} = {}({});", func_name, call_args).unwrap();
        }
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

    /// The descriptor for an operand, as a C expression of pointer type.
    ///
    /// A parameter our own caller described uses that descriptor: this
    /// function's static type for it says `data` where a type parameter stood,
    /// so one built from that type would misdescribe the value. Everything
    /// else is described by its own type, which is a static descriptor.
    fn operand_tydesc(&mut self, operand: &Operand) -> String {
        if let Operand::Param(param_id) = operand {
            if let Some(i) = self.func_descriptor_index(*param_id) {
                return format!("d{}", i);
            }
        }
        let ty = self.operand_type(operand).clone();
        format!("&{}", self.tydesc_name(&ty))
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
    /// A descriptor comes from the argument's own type, which is what it is
    /// here because a caller-supplied one never reaches this backend: a
    /// function that takes one is refused in `emit_function`.
    fn emit_native_call(
        &mut self,
        out: &mut String,
        dest: ValueId,
        symbol: &str,
        args: &[Operand],
    ) -> Result<(), CAotError> {
        let mut call_args = String::from("rt");
        for arg in args {
            let addr = self.operand_addr(arg);
            let tydesc = self.operand_tydesc(arg);
            write!(&mut call_args, ", {}, {}", addr, tydesc).unwrap();
        }

        let dest_ty = self.value_type(dest).clone();
        let dest_addr = self.value_addr(dest);
        let dest_tydesc = self.tydesc_name(&dest_ty);
        write!(&mut call_args, ", {}, &{}", dest_addr, dest_tydesc).unwrap();

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
            // Tracked (RefStoreTracked): destination was uninitialized, no destroy needed.
            // Mark tracking byte as LIVE.
            todo!("RefStoreTracked tracking byte write not yet implemented in C AOT");
        }
        Ok(())
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

        let map_tydesc = self.tydesc_name(&map_ty);
        let key_tydesc = self.tydesc_name(&key_ty);

        writeln!(out, "    dtlv_rti_btreemap_contains_key_local(rt, {}, &{}, {}, &{}, (bool_t*){});",
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

        let map_tydesc = self.tydesc_name(&map_ty);
        let key_tydesc = self.tydesc_name(&key_ty);
        let value_tydesc = self.tydesc_name(&value_ty);

        // Get pointer to value (returns null on miss).
        writeln!(out, "    {{").unwrap();
        writeln!(out, "        void* __vptr;").unwrap();
        writeln!(out, "        dtlv_rti_btreemap_get_value_ref_local(rt, {}, &{}, {}, &{}, &__vptr);",
            map_addr, map_tydesc, key_addr, key_tydesc).unwrap();
        writeln!(out, "        *(bool_t*){} = __vptr != NULL;", is_valid_addr).unwrap();

        // Conditionally clone value.
        writeln!(out, "        if (__vptr != NULL) {{").unwrap();

        // Clone value to dest.
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

        let map_tydesc = self.tydesc_name(&map_ty);
        let key_tydesc = self.tydesc_name(&key_ty);

        writeln!(out, "    dtlv_rti_btreemap_get_value_ref_local(rt, {}, &{}, {}, &{}, (void**){});",
            map_addr, map_tydesc, key_addr, key_tydesc, dest_addr).unwrap();
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
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} << *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} >> *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU64 => {
                writeln!(out, "    *(uint64_t*){} = __builtin_popcountll(*(uint64_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} ? __builtin_clzll(*(uint64_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU64 => {
                writeln!(out, "    *(uint64_t*){} = *(uint64_t*){} ? __builtin_ctzll(*(uint64_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
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
                writeln!(out, "    *(int64_t*){} = *(int64_t*){} >> *(uint64_t*){};", dest_addr, arg0(), arg1()).unwrap();
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
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} << *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} >> *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)__builtin_popcount(*(uint8_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} ? (uint8_t)(__builtin_clz(*(uint8_t*){}) - 24) : 8;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU8 => {
                writeln!(out, "    *(uint8_t*){} = *(uint8_t*){} ? (uint8_t)__builtin_ctz(*(uint8_t*){}) : 8;", dest_addr, arg0(), arg0()).unwrap();
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
                writeln!(out, "    *(int8_t*){} = *(int8_t*){} >> *(uint8_t*){};", dest_addr, arg0(), arg1()).unwrap();
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
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} << *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} >> *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)__builtin_popcount(*(uint16_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} ? (uint16_t)(__builtin_clz(*(uint16_t*){}) - 16) : 16;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzU16 => {
                writeln!(out, "    *(uint16_t*){} = *(uint16_t*){} ? (uint16_t)__builtin_ctz(*(uint16_t*){}) : 16;", dest_addr, arg0(), arg0()).unwrap();
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
                writeln!(out, "    *(int16_t*){} = *(int16_t*){} >> *(uint16_t*){};", dest_addr, arg0(), arg1()).unwrap();
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
                writeln!(out, "    *(index_t*){} = *(index_t*){} << *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::ShrIndex => {
                writeln!(out, "    *(index_t*){} = *(index_t*){} >> *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::PopcountIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(index_t*){} = __builtin_popcount(*(index_t*){});", dest_addr, arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(index_t*){} = __builtin_popcountll(*(index_t*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::ClzIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(index_t*){} = *(index_t*){} ? __builtin_clz(*(index_t*){}) : 32;", dest_addr, arg0(), arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(index_t*){} = *(index_t*){} ? __builtin_clzll(*(index_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
            }
            IntrinsicId::CtzIndex => {
                #[cfg(not(feature = "index-64"))]
                writeln!(out, "    *(index_t*){} = *(index_t*){} ? __builtin_ctz(*(index_t*){}) : 32;", dest_addr, arg0(), arg0()).unwrap();
                #[cfg(feature = "index-64")]
                writeln!(out, "    *(index_t*){} = *(index_t*){} ? __builtin_ctzll(*(index_t*){}) : 64;", dest_addr, arg0(), arg0()).unwrap();
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

            // Offset operations.
            IntrinsicId::OffsetToIndex => {
                writeln!(out, "    *(index_t*){} = (index_t)*(offset_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::NegWrappingOffset => {
                writeln!(out, "    *(offset_t*){} = -*(offset_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::SshrOffset => {
                writeln!(out, "    *(offset_t*){} = *(offset_t*){} >> *(index_t*){};", dest_addr, arg0(), arg1()).unwrap();
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
            let elem_size = elem_repr.layout().size;
            let ptr_base_offset = std::mem::offset_of!(datalove_rtdt::Tensor, ptr_base);
            let offset_elems_offset = std::mem::offset_of!(datalove_rtdt::Tensor, offset_elems);
            let strides_offset = std::mem::offset_of!(datalove_rtdt::Tensor, strides);

            // elem_addr = ptr_base + (offset + index * strides[0]) * elem_size
            writeln!(out, "        void* __elem = *(void**)({t} + {pb}) + (size_t)(*(index_t*)({t} + {oe}) + *(index_t*){idx} * **(index_t**)({t} + {st})) * {es};",
                t = tensor_addr, pb = ptr_base_offset, oe = offset_elems_offset,
                idx = index_addr, st = strides_offset, es = elem_size).unwrap();

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
            let elem_repr = types::ir_type_to_crepr(&elem_ty);
            let elem_size = elem_repr.layout().size;

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
