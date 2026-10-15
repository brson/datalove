//! C code generation for IR instructions and terminators.

use std::fmt::Write;
use std::mem::offset_of;
use datalove_rtdt as rtdt;

use datalove_datafun_ir::layout as ir_layout;
use datalove_datafun_ir::{
    BinOp, BlockId, CodeRef, ConstValue, CursorKind, FunctionRegistry, IrCodeUnit, IrModuleId,
    IrType, Instruction, Operand, ParamId, SlotDest, SlotId, Terminator, UnaryOp,
    ValueId,
};

use datalove_datafun_ir::frame_layout::FrameLayout;
use crate::types::{self, CRepr};
use crate::{CAotCompiler, CAotError};

/// Emit a complete function.
/// Emit `__dtlv_statics_init`, which builds each static const into its array
/// before the script runs, and `__dtlv_statics_fini`, which destroys them
/// before the runtime shuts down and looks for leaks.
pub fn emit_static_lifecycle(
    out: &mut String,
    compiler: &mut CAotCompiler,
    registry: &FunctionRegistry,
) -> Result<(), CAotError> {
    let statics = compiler.statics().to_vec();
    // The const writer works within a function; this one has no IR of its own.
    let unit = IrCodeUnit {
        id: datalove_datafun_ir::CodeUnitId(0),
        name: "__dtlv_statics_init".to_string(),
        blocks: Vec::new(),
        value_count: 0,
        slot_count: 0,
        value_types: Vec::new(),
        slot_types: Vec::new(),
        tracked_slots: Vec::new(),
        const_values: Vec::new(),
        symbols: Default::default(),
        context: datalove_datafun_ir::CodeUnitContext::Function(
            datalove_datafun_ir::FunctionContext {
                params: Vec::new(),
                param_modes: Vec::new(),
                param_types: Vec::new(),
                return_type: IrType::Unit,
                tracked_params: Vec::new(),
                type_param_count: 0,
                descriptor_table: Vec::new(),
                inline: false,
            },
        ),
        nested_units: Vec::new(),
    };
    let layout = FrameLayout::compute(&[], &[], &[], &[], &[]);
    let mut ctx = FunctionCodegenContext {
        unit: &unit,
        parent_unit: None,
        layout: &layout,
        compiler,
        registry,
        uses_sret: false,
        const_scratch: 0,
        table: Vec::new(),
        takes_shadow: false,
    };

    writeln!(out, "static void __dtlv_statics_init(void* rt) {{").unwrap();
    for (i, (ty, value)) in statics.iter().enumerate() {
        ctx.emit_const_at(out, &format!("((uint8_t*)__dtlv_static_{})", i), ty, value)?;
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "static void __dtlv_statics_fini(void* rt) {{").unwrap();
    for (i, (ty, _)) in statics.iter().enumerate() {
        let tydesc = ctx.tydesc_name(ty);
        writeln!(out, "    dtlv_rti_any_destroy_local(rt, __dtlv_static_{}, &{});", i, tydesc).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    Ok(())
}

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
    // Then one descriptor per type parameter, last.
    for i in 0..func_ctx.type_param_count {
        write!(&mut params, ", const dtlv_tydesc_t* tp{}", i).unwrap();
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
    writeln!(out, "    DTLV_CHECK_STACK(rt);").unwrap();

    // Emit frame allocation.
    if layout.frame_size > 0 {
        writeln!(out, "    _Alignas({}) uint8_t __frame[{}];", layout.frame_align, layout.frame_size).unwrap();
    }

    // Initialize tracking bytes to UNINIT.
    if layout.tracking_count > 0 {
        writeln!(out, "    memset(__frame + {}, TRACK_UNINIT, {});", layout.tracking_offset, layout.tracking_count).unwrap();
    }

    check_dynamic_support(unit)?;
    let takes_shadow = emit_dynamic_prologue(out, unit, &layout, compiler);

    // Create function context for codegen.
    let mut ctx = FunctionCodegenContext {
        unit,
        parent_unit,
        layout: &layout,
        compiler,
        registry,
        uses_sret,
        const_scratch: 0,
        table: func_ctx.descriptor_table.clone(),
        takes_shadow,
    };

    // Emit blocks.
    for block in &unit.blocks {
        ctx.emit_block(out, block)?;
    }

    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    Ok(())
}

/// In a generic, fill the descriptor table into `__t{i}`
/// locals and take shadow stack space for each dynamic value and slot.
/// Whether any was taken, which every return then gives back.
///
/// The table goes through a one-entry cache, a static array of the last
/// call's type parameter descriptors and what they built: an AOT program has
/// the one runtime handle, so what it built lives as long as the cache.
fn emit_dynamic_prologue(
    out: &mut String,
    unit: &IrCodeUnit,
    layout: &FrameLayout,
    compiler: &mut CAotCompiler,
) -> bool {
    let Some(func_ctx) = unit.function_context() else { return false };
    let table = &func_ctx.descriptor_table;
    if table.is_empty() {
        return false;
    }
    let n = func_ctx.type_param_count as usize;
    let built: Vec<(usize, &IrType)> = table.iter().enumerate()
        .filter(|(_, ty)| !matches!(ty, IrType::Param(_)))
        .collect();
    for (i, ty) in table.iter().enumerate() {
        match ty {
            IrType::Param(p) => writeln!(out, "    const dtlv_tydesc_t* __t{i} = tp{p};").unwrap(),
            _ => writeln!(out, "    const dtlv_tydesc_t* __t{i};").unwrap(),
        }
    }
    if !built.is_empty() {
        let params: Vec<String> = (0..n).map(|i| format!("tp{i}")).collect();
        writeln!(out, "    const dtlv_tydesc_t* __tp[{}] = {{ {} }};", n.max(1), params.join(", ")).unwrap();
        writeln!(out, "    static const dtlv_tydesc_t* __tc[{}];", n + built.len()).unwrap();
        let hit: Vec<String> = (0..n).map(|i| format!("__tc[{i}] == tp{i}")).collect();
        writeln!(out, "    if ({}) {{", if hit.is_empty() { "1".to_string() } else { hit.join(" && ") }).unwrap();
        for (k, (i, _)) in built.iter().enumerate() {
            writeln!(out, "        __t{i} = __tc[{}];", n + k).unwrap();
        }
        writeln!(out, "    }} else {{").unwrap();
        for (k, (i, ty)) in built.iter().enumerate() {
            let template = compiler.get_tydesc_name(ty);
            writeln!(out, "        __t{i} = dtlv_rti_tydesc_instantiate(rt, &{template}, __tp, {n});").unwrap();
            writeln!(out, "        __tc[{}] = __t{i};", n + k).unwrap();
        }
        for i in 0..n {
            writeln!(out, "        __tc[{i}] = tp{i};").unwrap();
        }
        writeln!(out, "    }}").unwrap();
    }
    let places: Vec<(u32, usize)> = unit.value_types.iter().enumerate()
        .filter(|(_, ty)| FunctionCodegenContext::is_dynamic_value(ty))
        .map(|(v, ty)| (layout.value_offset(v as u32), table.iter().position(|t| t == ty).expect("in the table")))
        .chain(unit.slot_types.iter().enumerate()
            .filter(|(_, ty)| FunctionCodegenContext::is_dynamic_value(ty))
            .map(|(s, ty)| (layout.slot_offset(s as u32), table.iter().position(|t| t == ty).expect("in the table"))))
        .collect();
    if places.is_empty() {
        return false;
    }
    writeln!(out, "    uintptr_t __shadow_top = ((dtlv_rt_header_t*)rt)->shadow_top;").unwrap();
    writeln!(out, "    uintptr_t __shadow_limit = ((dtlv_rt_header_t*)rt)->shadow_limit;").unwrap();
    for (offset, entry) in places {
        writeln!(out, "    *(void**)(__frame + {offset}) = dtlv_shadow_bump(rt, __t{entry});").unwrap();
    }
    true
}

/// Refuse a function using an instruction on a dynamic value that this
/// backend does not handle yet, rather than emit it wrong.
fn check_dynamic_support(unit: &IrCodeUnit) -> Result<(), CAotError> {
    if !datalove_datafun_ir::uses_descriptors(unit) {
        return Ok(());
    }
    let ctx = unit.function_context();
    let operand_type = |op: &Operand| -> Option<&IrType> {
        match op {
            Operand::Value(v) => Some(&unit.value_types[v.0 as usize]),
            Operand::ValueRef(v) => match &unit.value_types[v.0 as usize] {
                IrType::Ref(inner) => Some(inner.as_ref()),
                other => Some(other),
            },
            Operand::Slot(s) => Some(&unit.slot_types[s.0 as usize]),
            Operand::Param(p) => ctx.map(|c| &c.param_types[p.0 as usize]),
            _ => None,
        }
    };
    for block in &unit.blocks {
        for instr in &block.instructions {
            let mut touches = instr.dests().into_iter()
                .any(|v| unit.value_types[v.0 as usize].is_dynamic());
            instr.for_each_operand(|op: &Operand| {
                touches |= operand_type(op).is_some_and(|t| t.is_dynamic());
            });
            match instr {
                Instruction::SlotStoreCopy { dest: SlotDest::Local(s), .. }
                | Instruction::SlotStoreMove { dest: SlotDest::Local(s), .. }
                | Instruction::SlotStoreCopyTracked { dest: SlotDest::Local(s), .. }
                | Instruction::SlotStoreMoveTracked { dest: SlotDest::Local(s), .. }
                | Instruction::SetField { slot: SlotDest::Local(s), .. }
                | Instruction::SetFieldTracked { slot: SlotDest::Local(s), .. } => {
                    touches |= unit.slot_types[s.0 as usize].is_dynamic();
                }
                Instruction::ParamStore { param, .. }
                | Instruction::ParamStoreTracked { param, .. }
                | Instruction::ParamSetField { param, .. }
                | Instruction::ParamSetFieldTracked { param, .. } => {
                    touches |= ctx.is_some_and(|c| c.param_types[param.0 as usize].is_dynamic());
                }
                _ => {}
            }
            if touches && !supports_dynamic(instr) {
                let name = format!("{:?}", instr);
                let name = name.split([' ', '{', '(']).next().unwrap_or("an instruction").to_string();
                return Err(CAotError::Unsupported(format!(
                    "{name} on a value of a generic's type parameter, in {}",
                    unit.name)));
            }
        }
    }
    Ok(())
}

/// Whether this backend handles an instruction whose operands or result are
/// dynamic.
fn supports_dynamic(instr: &Instruction) -> bool {
    matches!(instr,
        Instruction::Copy { .. }
        | Instruction::CursorBegin { .. }
        | Instruction::CursorDone { .. }
        | Instruction::CursorElem { .. }
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
        | Instruction::DataFrom { .. }
        | Instruction::ErrorFrom { .. }
        | Instruction::Nop
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
        | Instruction::WrapSome { .. }
        | Instruction::WrapNone { .. }
        | Instruction::WrapOk { .. }
        | Instruction::WrapErr { .. }
        | Instruction::UnwrapOption { .. }
        | Instruction::UnwrapResult { .. }
        | Instruction::EnumVariant { .. }
        | Instruction::EnumDiscriminant { .. }
        | Instruction::EnumPayload { .. }
        | Instruction::DropField { .. }
        | Instruction::ListNew { .. }
        | Instruction::TensorNew { .. }
        | Instruction::TableNew { .. }
        | Instruction::SetNew { .. }
        | Instruction::MapNew { .. }
        | Instruction::ListBoundsCheck { .. }
        | Instruction::ListGet { .. }
        | Instruction::ListSet { .. }
        | Instruction::ListElementRef { .. }
        | Instruction::MapGet { .. }
        | Instruction::MapContainsKey { .. }
        | Instruction::MapSetValue { .. }
        | Instruction::MapValueRef { .. }
        | Instruction::MapUpsert { .. }
        | Instruction::TensorBoundsCheck { .. }
        | Instruction::TensorSet { .. }
        | Instruction::TensorGet { .. }
        | Instruction::TensorIndexRef { .. }
    )
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
        table: Vec::new(),
        takes_shadow: false,
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
    /// In a generic, the descriptor table: entry `i` is in the
    /// local `__t{i}`, filled in the prologue.
    table: Vec<IrType>,
    /// Whether the function takes shadow stack space on entry, which every
    /// return gives back.
    takes_shadow: bool,
}

impl<'a> FunctionCodegenContext<'a> {
    /// The local a dynamic type's descriptor is in.
    fn table_local(&self, ty: &IrType) -> String {
        match self.table.iter().position(|t| t == ty) {
            Some(i) => format!("__t{}", i),
            None => panic!("{ty} is not in the descriptor table of {}", self.unit.name),
        }
    }

    /// The size of a value of `ty`, as a C expression: a constant, or a
    /// dynamic type's descriptor's.
    fn size_expr(&self, ty: &IrType) -> String {
        if ty.is_dynamic() && !matches!(ty, IrType::Ref(_)) {
            return format!("(size_t){}->size", self.table_local(ty));
        }
        types::ir_type_to_crepr(ty).layout().size.to_string()
    }

    /// Each field's offset in a tuple or struct of type `ty` (or one a
    /// reference to `ty` points at), as C expressions: constants, or for a
    /// dynamic type, read from its descriptor.
    fn field_offsets(&self, ty: &IrType, count: usize) -> Vec<String> {
        let ty = match ty {
            IrType::Ref(inner) => inner.as_ref(),
            other => other,
        };
        if ty.is_dynamic() {
            let local = self.table_local(ty);
            return (0..count).map(|i| format!("dtlv_rti_field_offset({local}, {i})")).collect();
        }
        let field_types: Vec<IrType> = match ty {
            IrType::Tuple(tys) => tys.clone(),
            IrType::Struct(fs) => fs.iter().map(|(_, t)| t.clone()).collect(),
            _ => Vec::new(),
        };
        types::compute_tuple_field_offsets(&field_types).iter().map(|o| o.to_string()).collect()
    }

    /// The offset of the payload of an option, result, term or enum (variant
    /// `index`) of type `ty`, as a C expression.
    fn payload_offset(&self, ty: &IrType, index: u32) -> String {
        if ty.is_dynamic() {
            return format!("dtlv_rti_payload_offset({}, {index})", self.table_local(ty));
        }
        match ty {
            IrType::Option(inner) => ir_layout::option_payload_offset(inner),
            IrType::Result(ok) => ir_layout::result_payload_offset(ok),
            IrType::Enum(variants) => match &variants[index as usize].1 {
                Some(payload) => ir_layout::enum_payload_offset(payload),
                None => 0,
            },
            _ => 0,
        }.to_string()
    }

    /// Whether a value of `ty` lives on the shadow stack, its frame slot
    /// holding a pointer to it.
    fn is_dynamic_value(ty: &IrType) -> bool {
        ty.is_dynamic() && !matches!(ty, IrType::Ref(_))
    }

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
            Instruction::StaticRef { dest, value } => {
                // The reference is the address of the array the program
                // built the value into before the script ran.
                let index = self.compiler.static_index(value);
                writeln!(out, "    *(void**){} = (void*)__dtlv_static_{};",
                    self.value_addr(*dest), index).unwrap();
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
            Instruction::OpAssign { place, op, rhs } => {
                self.emit_op_assign(out, place, *op, rhs)?;
            }
            Instruction::OpAssignChecked { overflow, place, op, rhs } => {
                self.emit_op_assign_checked(out, *overflow, place, *op, rhs)?;
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
            Instruction::Call { dest, func, args, bindings, .. } => {
                self.emit_call(out, *dest, func, args, bindings)?;
            }
            Instruction::ComptimeCall { dest, func, args, bindings, .. } => {
                self.emit_call(out, *dest, func, args, bindings)?;
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
            Instruction::DropField { base, field_path } => {
                self.emit_drop_field(out, base, field_path)?;
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
            Instruction::ListSet { is_valid, list, index, value } => {
                self.emit_list_set(out, *is_valid, list, index, value)?;
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
            Instruction::MapSetValue { is_valid, map, key, value } => {
                self.emit_map_set_value(out, *is_valid, map, key, value)?;
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
            Instruction::TensorSet { is_valid, tensor, index, value } => {
                self.emit_tensor_set(out, *is_valid, tensor, index, value)?;
            }
            Instruction::TensorIndexRef { dest, tensor, index } => {
                self.emit_tensor_index_ref(out, *dest, tensor, index)?;
            }
            Instruction::CursorBegin { cursor, coll } => {
                self.emit_cursor_begin(out, *cursor, coll);
            }
            Instruction::CursorDone { dest, cursor } => {
                let dest_addr = self.value_addr(*dest);
                let done = self.cursor_done(*cursor);
                writeln!(out, "    *(bool_t*){dest_addr} = {done};").unwrap();
            }
            Instruction::CursorElem { dest, cursor } => {
                let dest_addr = self.value_addr(*dest);
                let elem = self.cursor_elem(*cursor);
                writeln!(out, "    *(const void**){dest_addr} = {elem};").unwrap();
            }
            Instruction::CursorValue { dest, cursor } => {
                let dest_addr = self.value_addr(*dest);
                let value = self.cursor_value(*cursor);
                writeln!(out, "    *(const void**){dest_addr} = {value};").unwrap();
            }
            Instruction::CursorAdvance { cursor, coll } => {
                self.emit_cursor_advance(out, *cursor, coll);
            }
            Instruction::Nop => {}
        }
        Ok(())
    }

    /// Get the address expression for a value.
    fn value_addr(&self, vid: ValueId) -> String {
        let offset = self.layout.value_offset(vid.0);
        if Self::is_dynamic_value(&self.unit.value_types[vid.0 as usize]) {
            return format!("(*(uint8_t**)(__frame + {}))", offset);
        }
        format!("(__frame + {})", offset)
    }

    /// Get the address expression for a slot.
    fn slot_addr(&self, sid: SlotId) -> String {
        let offset = self.layout.slot_offset(sid.0);
        if Self::is_dynamic_value(&self.unit.slot_types[sid.0 as usize]) {
            return format!("(*(uint8_t**)(__frame + {}))", offset);
        }
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
    ///
    /// A dynamic type's is its table entry, written as what it points at so
    /// that `&` of it, which is how a descriptor is passed, is the pointer.
    fn tydesc_name(&mut self, ty: &IrType) -> String {
        if ty.is_dynamic() && !matches!(ty, IrType::Ref(_)) {
            return format!("(*{})", self.table_local(ty));
        }
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
            // C has no negative literals, only negated positive ones, and the
            // most negative value of a type has no positive to negate. A
            // decimal literal past the signed range is likewise not a value C
            // will take without a suffix that allows it to be unsigned.
            ConstValue::I64(v) => {
                let lit = if *v == i64::MIN { "INT64_MIN".to_string() } else { format!("{v}LL") };
                writeln!(out, "    *(int64_t*){} = {};", dest_addr, lit).unwrap();
            }
            ConstValue::Index(v) => {
                writeln!(out, "    *(index_t*){} = {}u;", dest_addr, v).unwrap();
            }
            ConstValue::Offset(v) => {
                let lit = if *v == datalove_rtdt::OffsetRepr::MIN {
                    "DTLV_OFFSET_MIN".to_string()
                } else {
                    format!("{v}")
                };
                writeln!(out, "    *(offset_t*){} = {};", dest_addr, lit).unwrap();
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
            // A term is laid out as its payload, with nothing to say which
            // variant it is, there being only the one.
            ConstValue::Enum { payload, .. } if matches!(ty, IrType::Term(..)) => {
                let IrType::Term(_, payload_ty) = ty else { unreachable!() };
                let payload = payload.as_deref().ok_or_else(|| CAotError::Codegen(
                    "a term constant has no payload".into()))?;
                self.emit_const_at(out, dest_addr, payload_ty, payload)?;
            }
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
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, self.size_expr(ty)).unwrap();
            }
        }
        Ok(())
    }

    /// Emit a clone (deep copy).
    fn emit_clone(&mut self, out: &mut String, dest: ValueId, src: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let src_addr = self.operand_addr(src);
        let src_tydesc = self.operand_tydesc(src);
        let dest_ty = self.value_type(dest).clone();
        let dest_tydesc = self.tydesc_name(&dest_ty);

        writeln!(out, "    dtlv_rti_clone_local(rt, {}, {}, {}, &{});",
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
        if lhs_ty.is_dynamic() || self.operand_type(rhs).is_dynamic()
        {
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

        let operand_ty = match lhs_ty {
            IrType::Ref(inner) => inner.as_ref(),
            other => other,
        };

        // A type with one value is equal to itself, with nothing to read.
        if matches!(operand_ty, IrType::Unit | IrType::Atom(_)) {
            let result = match op {
                BinOp::Eq => 1,
                BinOp::Ne => 0,
                _ => return Err(CAotError::Unsupported(format!("{:?} on {:?}", op, operand_ty))),
            };
            writeln!(out, "    *(bool_t*){dest_addr} = {result};").unwrap();
            return Ok(());
        }

        // Everything else that is not a scalar the runtime compares by walking
        // its descriptor. An error is two types the typechecker let through as
        // one, so it traps.
        if matches!(
            operand_ty,
            IrType::String
                | IrType::Tuple(_)
                | IrType::Struct(_)
                | IrType::Option(_)
                | IrType::Term(..)
                | IrType::Enum(_)
        ) {
            let test = match op {
                BinOp::Eq => "__eq == EQ_EQUALS",
                BinOp::Ne => "__eq != EQ_EQUALS",
                _ => return Err(CAotError::Unsupported(format!("{:?} on {:?}", op, operand_ty))),
            };
            let lhs_td = self.operand_tydesc(lhs);
            let rhs_td = self.operand_tydesc(rhs);
            writeln!(out, "    {{ uint8_t __eq = dtlv_rti_eq_local(rt, {lhs_addr}, {lhs_td}, {rhs_addr}, {rhs_td}); \
                if (__eq == EQ_ERROR) __builtin_trap(); *(bool_t*){dest_addr} = ({test}); }}").unwrap();
            return Ok(());
        }

        // What is left has to be a scalar the instructions below can read.
        // Anything else would compare whatever word stands for it, which for
        // an aggregate is its address: a wrong answer rather than a failure.
        if !matches!(
            operand_ty,
            IrType::Bool
                | IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64
                | IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64
                | IrType::Index | IrType::Offset
                | IrType::F32 | IrType::F64
        ) {
            panic!("{:?} on {:?}, which has no scalar form to compare", op, operand_ty);
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

        // A type parameter bounded to `float`, whose width only the
        // descriptor says.
        if src_ty.is_dynamic() {
            assert_eq!(op, UnaryOp::Neg, "negation is the one operator a bounded type parameter has");
            let src_td = self.operand_tydesc(operand);
            let dest_ty = self.value_type(dest).clone();
            let dest_td = self.tydesc_name(&dest_ty);
            let code = datalove_rtdt::DynUnOp::Neg as u8;
            writeln!(out, "    dtlv_rti_dyn_unop(rt, {code}, {src_addr}, {src_td}, {dest_addr}, &{dest_td});").unwrap();
            return Ok(());
        }

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
        if lhs_ty.is_dynamic() {
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

        // The type-generic builtins check against the type their result
        // pointer points to, so one name serves every width. The fixed-width
        // ones name C's `int`, `long` and `long long`, which `int64_t` and
        // `index_t` do not reliably match.
        let builtin = match (lhs_ty, op) {
            (IrType::I32 | IrType::I64 | IrType::U32 | IrType::U64
                | IrType::Index | IrType::Offset, BinOp::Add) => "__builtin_add_overflow",
            (IrType::I32 | IrType::I64 | IrType::U32 | IrType::U64
                | IrType::Index | IrType::Offset, BinOp::Sub) => "__builtin_sub_overflow",
            (IrType::I32 | IrType::I64 | IrType::U32 | IrType::U64
                | IrType::Index | IrType::Offset, BinOp::Mul) => "__builtin_mul_overflow",
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

    /// Emit `OpAssign`: update a place with an operator that can't fail.
    ///
    /// An `int` is updated by the runtime, in its own buffer when it has room;
    /// a float in place.
    fn emit_op_assign(&mut self, out: &mut String, place: &Operand, op: BinOp, rhs: &Operand) -> Result<(), CAotError> {
        let place_addr = self.operand_addr(place);
        let rhs_addr = self.operand_addr(rhs);
        let ty = self.operand_type(place).clone();
        if matches!(ty, IrType::Int) {
            let func = match op {
                BinOp::Add => "dtlv_rti_int_add_assign",
                BinOp::Sub => "dtlv_rti_int_sub_assign",
                BinOp::Mul => "dtlv_rti_int_mul_assign",
                _ => panic!("{:?} on an `int` place, which the typechecker refuses", op),
            };
            let tydesc = self.tydesc_name(&IrType::Int);
            writeln!(out, "    {func}(rt, {place_addr}, &{tydesc}, {rhs_addr}, &{tydesc});").unwrap();
            return Ok(());
        }
        let op_str = match (op, &ty) {
            (BinOp::Add, IrType::F32 | IrType::F64) => "+=",
            (BinOp::Sub, IrType::F32 | IrType::F64) => "-=",
            (BinOp::Mul, IrType::F32 | IrType::F64) => "*=",
            (BinOp::Div, IrType::F32 | IrType::F64) => "/=",
            _ => panic!("{:?} on a {:?} place, which lowering doesn't emit as an update", op, ty),
        };
        let c_ty = types::ir_type_to_c(&ty);
        writeln!(out, "    *({c_ty}*){place_addr} {op_str} *({c_ty}*){rhs_addr};").unwrap();
        Ok(())
    }

    /// Emit `OpAssignChecked`: update a place with a checked operator, if it
    /// succeeds, leaving it as it was if not.
    fn emit_op_assign_checked(
        &mut self,
        out: &mut String,
        overflow: ValueId,
        place: &Operand,
        op: BinOp,
        rhs: &Operand,
    ) -> Result<(), CAotError> {
        let overflow_addr = self.value_addr(overflow);
        let place_addr = self.operand_addr(place);
        let rhs_addr = self.operand_addr(rhs);
        let ty = self.operand_type(place).clone();
        if matches!(ty, IrType::Int) {
            assert_eq!(op, BinOp::Div, "checked {:?} on an `int` place, which the typechecker refuses", op);
            let tydesc = self.tydesc_name(&IrType::Int);
            writeln!(out, "    *(bool_t*){overflow_addr} = \
                dtlv_rti_int_div_assign_checked(rt, {place_addr}, &{tydesc}, {rhs_addr}, &{tydesc}) != 1;").unwrap();
            return Ok(());
        }

        let c_ty = types::ir_type_to_c(&ty);
        let (a, b) = (format!("*({c_ty}*){place_addr}"), format!("*({c_ty}*){rhs_addr}"));
        let failed = match op {
            // The builtins check against the type their result points to, at
            // every width.
            BinOp::Add => format!("__builtin_add_overflow({a}, {b}, &__r)"),
            BinOp::Sub => format!("__builtin_sub_overflow({a}, {b}, &__r)"),
            BinOp::Mul => format!("__builtin_mul_overflow({a}, {b}, &__r)"),
            // A zero divisor, and a signed type's least value over -1, have no
            // answer, and have to be caught before the divide, which faults.
            BinOp::Div => {
                let signed_min = match ty {
                    IrType::I8 => Some("INT8_MIN"),
                    IrType::I16 => Some("INT16_MIN"),
                    IrType::I32 => Some("INT32_MIN"),
                    IrType::I64 => Some("INT64_MIN"),
                    IrType::Offset => Some("DTLV_OFFSET_MIN"),
                    _ => None,
                };
                let overflows = match signed_min {
                    Some(min) => format!(" || ({a} == {min} && {b} == -1)"),
                    None => String::new(),
                };
                format!("({b} == 0{overflows} ? 1 : (__r = {a} / {b}, 0))")
            }
            _ => panic!("{:?} as a checked update; only Add, Sub, Mul and Div can fail", op),
        };
        writeln!(out, "    {{ {c_ty} __r; bool_t __o = {failed}; *(bool_t*){overflow_addr} = __o; if (!__o) {a} = __r; }}").unwrap();
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
        if src_ty.is_dynamic() {
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
                // A signed value overflows only at its minimum, and an
                // unsigned one at anything but zero.
                match src_ty {
                    IrType::I8 => {
                        writeln!(out, "    {{ int8_t __v = *(int8_t*){}; *(bool_t*){} = (__v == INT8_MIN); *(int8_t*){} = (int8_t)-__v; }}",
                            src_addr, overflow_addr, dest_addr).unwrap();
                    }
                    IrType::I16 => {
                        writeln!(out, "    {{ int16_t __v = *(int16_t*){}; *(bool_t*){} = (__v == INT16_MIN); *(int16_t*){} = (int16_t)-__v; }}",
                            src_addr, overflow_addr, dest_addr).unwrap();
                    }
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
                    IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64 | IrType::Index => {
                        writeln!(out, "    {{ {c_ty} __v = *({c_ty}*){src_addr}; *(bool_t*){overflow_addr} = (__v != 0); *({c_ty}*){dest_addr} = ({c_ty})0; }}").unwrap();
                    }
                    _ => panic!("checked negation of {:?}, which is not a fixed-width integer", src_ty),
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

        let offsets = self.field_offsets(dest_ty, field_types.len());

        for (i, field_op) in fields.iter().enumerate() {
            let field_offset = &offsets[i];
            let field_ty = &field_types[i];
            let repr = types::ir_type_to_crepr(field_ty);
            let field_addr = format!("({} + {})", dest_addr, field_offset);
            let src_addr = self.operand_addr(field_op);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){field_addr} = *({c_ty}*){src_addr};").unwrap();
                }
                CRepr::Aggregate(_) => {
                    writeln!(out, "    memcpy({}, {}, {});", field_addr, src_addr, self.size_expr(field_ty)).unwrap();
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

        let offsets = self.field_offsets(src_ty, field_types.len());

        for (i, dest_vid) in dests.iter().enumerate() {
            let field_offset = &offsets[i];
            let field_ty = &field_types[i];
            let repr = types::ir_type_to_crepr(field_ty);
            let field_addr = format!("({} + {})", src_addr, field_offset);
            let dest_addr = self.value_addr(*dest_vid);

            match repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){field_addr};").unwrap();
                }
                CRepr::Aggregate(_) => {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, field_addr, self.size_expr(field_ty)).unwrap();
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

        let offsets = self.field_offsets(&src_ty, field_types.len());
        let field_offset = &offsets[field_index as usize];
        let field_ty = &field_types[field_index as usize];
        let repr = types::ir_type_to_crepr(field_ty);
        let field_addr = format!("({} + {})", src_addr, field_offset);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){field_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", dest_addr, field_addr, self.size_expr(field_ty)).unwrap();
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

        let offsets = self.field_offsets(&src_ty, field_types.len());
        let field_offset = &offsets[field_index as usize];

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
            let offsets = self.field_offsets(&current_ty, field_types.len());
            let field_offset = &offsets[idx as usize];
            current_addr = format!("({} + {})", current_addr, field_offset);
            current_ty = field_types[idx as usize].clone();
        }

        // Copy value to field.
        let src_addr = self.operand_addr(value);
        let repr = types::ir_type_to_crepr(&current_ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){current_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", current_addr, src_addr, self.size_expr(&current_ty)).unwrap();
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
        let payload_offset = self.payload_offset(&self.value_type(dest).clone(), 0);

        // Set tag to Some (2).
        writeln!(out, "    *(uint8_t*){} = OPTION_SOME;", dest_addr).unwrap();

        // Move inner value (copy then zero source for non-copy types).
        let payload_addr = format!("({} + {})", dest_addr, payload_offset);
        match &inner_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){payload_addr} = *({c_ty}*){inner_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", payload_addr, inner_addr, self.size_expr(&inner_ty)).unwrap();
            }
        }

        // Zero source for non-copy types to implement move semantics.
        if !inner_ty.is_copy() {
            self.emit_zero(out, &inner_addr, &inner_ty);
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

        if !matches!(dest_ty, IrType::Result(_)) {
            return Err(CAotError::Codegen("wrap_ok requires Result type".into()));
        }

        let payload_offset = self.payload_offset(&dest_ty.clone(), 0);

        // Set tag to Ok (1).
        writeln!(out, "    *(uint8_t*){} = RESULT_OK;", dest_addr).unwrap();

        // Move inner value (copy then zero source for non-copy types).
        let payload_addr = format!("({} + {})", dest_addr, payload_offset);
        let inner_repr = types::ir_type_to_crepr(&inner_ty);
        match &inner_repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){payload_addr} = *({c_ty}*){inner_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", payload_addr, inner_addr, self.size_expr(&inner_ty)).unwrap();
            }
        }

        // Zero source for non-copy types to implement move semantics.
        if !inner_ty.is_copy() {
            self.emit_zero(out, &inner_addr, &inner_ty);
        }
        Ok(())
    }

    /// Emit wrap err.
    fn emit_wrap_err(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let dest_ty = self.value_type(dest);

        if !matches!(dest_ty, IrType::Result(_)) {
            return Err(CAotError::Codegen("wrap_err requires Result type".into()));
        }

        let error_size = std::mem::size_of::<datalove_rtdt::Error>() as u32;
        let payload_offset = self.payload_offset(&dest_ty.clone(), 0);

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
        let payload_offset = self.payload_offset(&self.operand_type(src).clone(), 0);

        // Read tag.
        writeln!(out, "    *(bool_t*){} = (*(uint8_t*){} == OPTION_SOME);", is_some_addr, src_addr).unwrap();

        // Copy payload.
        let payload_addr = format!("({} + {})", src_addr, payload_offset);
        match types::ir_type_to_crepr(dest_ty) {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){payload_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", dest_addr, payload_addr, self.size_expr(dest_ty)).unwrap();
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
        let payload_offset = self.payload_offset(&self.operand_type(src).clone(), 0);

        // Read tag.
        writeln!(out, "    *(bool_t*){} = (*(uint8_t*){} == RESULT_OK);", is_ok_addr, src_addr).unwrap();

        // Copy ok payload.
        let payload_addr = format!("({} + {})", src_addr, payload_offset);
        match types::ir_type_to_crepr(ok_ty) {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){ok_addr} = *({c_ty}*){payload_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", ok_addr, payload_addr, self.size_expr(ok_ty)).unwrap();
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
            let payload_offset = self.payload_offset(&self.value_type(dest).clone(), variant_index);
            let dest_payload_addr = format!("({} + {})", dest_addr, payload_offset);

            match types::ir_type_to_crepr(payload_ty) {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    *({c_ty}*){dest_payload_addr} = *({c_ty}*){payload_addr};").unwrap();
                }
                CRepr::Aggregate(_) => {
                    writeln!(out, "    memcpy({}, {}, {});", dest_payload_addr, payload_addr, self.size_expr(payload_ty)).unwrap();
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
        let payload_offset = self.payload_offset(&src_ty, variant_index);

        match types::ir_type_to_crepr(payload_ty) {
            CRepr::Scalar(c_ty) => {
                let src_payload = format!("({} + {})", src_addr, payload_offset);
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_payload};").unwrap();
            }
            CRepr::Aggregate(layout) => {
                if payload_ty.is_dynamic() || layout.size > 0 {
                    let src_payload = format!("({} + {})", src_addr, payload_offset);
                    writeln!(out, "    memcpy({}, {}, {});",
                        dest_addr, src_payload, self.size_expr(payload_ty)).unwrap();
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

        writeln!(out, "    dtlv_rti_error_from_local(rt, {}, &{}, {});", inner_addr, inner_tydesc, dest_addr).unwrap();

        // Zero source to implement move semantics (prevents double-free/leak).
        self.emit_zero(out, &inner_addr, &inner_ty);
        Ok(())
    }

    /// Zero a value of `ty` at `addr`, which is how a move leaves its source.
    fn emit_zero(&self, out: &mut String, addr: &str, ty: &IrType) {
        if ty.is_dynamic() || types::ir_type_to_crepr(ty).layout().size > 0 {
            writeln!(out, "    memset({}, 0, {});", addr, self.size_expr(ty)).unwrap();
        }
    }

    /// Emit data from.
    fn emit_data_from(&mut self, out: &mut String, dest: ValueId, inner: &Operand) -> Result<(), CAotError> {
        let dest_addr = self.value_addr(dest);
        let inner_addr = self.operand_addr(inner);
        let inner_ty = self.operand_type(inner).clone();
        let inner_tydesc = self.tydesc_name(&inner_ty);

        writeln!(out, "    dtlv_rti_data_from_local(rt, {}, &{}, {});", inner_addr, inner_tydesc, dest_addr).unwrap();

        // Zero source to implement move semantics (prevents double-free/leak).
        self.emit_zero(out, &inner_addr, &inner_ty);
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
            let list_tydesc = self.tydesc_name(&dest_ty);
            writeln!(out, "    dtlv_rti_list_create_local(rt, {}, &{});", dest_addr, list_tydesc).unwrap();
        } else if elem_ty.is_dynamic() {
            // Elements of a size known only at run time are pushed one at a
            // time rather than laid out in a buffer here.
            let list_tydesc = self.tydesc_name(&dest_ty);
            writeln!(out, "    dtlv_rti_list_create_local(rt, {}, &{});", dest_addr, list_tydesc).unwrap();
            for elem_op in elements {
                let elem_addr = self.operand_addr(elem_op);
                writeln!(out, "    dtlv_rti_list_push_local(rt, {}, &{}, {}, &{});",
                    dest_addr, list_tydesc, elem_addr, elem_tydesc).unwrap();
            }
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

        if !matches!(dest_ty, IrType::Set(_)) {
            return Err(CAotError::Codegen("set_new requires Set type".into()));
        }
        let set_tydesc = self.tydesc_name(&dest_ty);

        writeln!(out, "    dtlv_rti_btreeset_create_local(rt, {}, &{});", dest_addr, set_tydesc).unwrap();
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
        let shape_str: Vec<String> = shape.iter().map(|s| s.to_string()).collect();

        // An element whose size only its descriptor says is gathered in a
        // buffer the runtime allocates at that size, and freed once
        // `tensor_init` has moved the elements into the tensor's own.
        if elem_ty.is_dynamic() {
            let n = elements.len();
            writeln!(out, "    {{ uint8_t* __elems = dtlv_rti_mem_alloc_raw_local(rt, {td}.size, {td}.align, {n});",
                td = elem_tydesc).unwrap();
            for (i, elem_op) in elements.iter().enumerate() {
                let elem_addr = self.operand_addr(elem_op);
                writeln!(out, "    memcpy(__elems + {i} * (size_t){td}.size, {}, (size_t){td}.size);",
                    elem_addr, td = elem_tydesc).unwrap();
            }
            writeln!(out, "    static const uint32_t __shape[] = {{ {} }};", shape_str.join(", ")).unwrap();
            writeln!(out, "    dtlv_rti_tensor_init_local(rt, __elems, {n}, &{}, __shape, {}, {}, &{});",
                elem_tydesc, rank, dest_addr, dest_tydesc).unwrap();
            writeln!(out, "    dtlv_rti_mem_free_raw_local(rt, {td}.size, {td}.align, {n}, __elems); }}",
                td = elem_tydesc).unwrap();
            return Ok(());
        }

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
        } else if row_ty.is_dynamic() {
            // Rows of a size known only at run time are pushed one at a time,
            // each with its own descriptor.
            writeln!(out, "    dtlv_rti_table_create_local(rt, {}, &{});", dest_addr, dest_tydesc).unwrap();
            for row_op in rows {
                let row_addr = self.operand_addr(row_op);
                let row_td = self.operand_tydesc(row_op);
                writeln!(out, "    dtlv_rti_table_push_row_local(rt, {}, &{}, {}, {});",
                    dest_addr, dest_tydesc, row_addr, row_td).unwrap();
            }
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
        bindings: &[IrType],
    ) -> Result<(), CAotError> {
        // A rider function is reached through the runtime C ABI rather than
        // this backend's own convention, so it is a different call entirely.
        if let CodeRef::Module { module, id } = func {
            if let Some(unit) = self.registry.get_module_function_as_unit(*module, *id) {
                if let Some(native) = unit.native_context() {
                    let symbol = native.symbol().to_string();
                    // A descriptor for each type parameter no argument
                    // determines, for what this call bound it to.
                    let undetermined = native.undetermined_bindings(bindings);
                    let modes = native.param_modes.clone();
                    self.emit_native_call(out, dest, &symbol, args, &undetermined)?;
                    self.mark_in_arguments_moved(out, &modes, args);
                    return Ok(());
                }
            }
        }


        // For local function lookup, use parent_unit if available (for nested functions).
        let lookup_unit = self.parent_unit.unwrap_or(self.unit);
        let func_name = self.compiler.resolve_func_name_with_registry(func, lookup_unit, self.registry);
        let dest_ty = self.value_type(dest).clone();
        let uses_sret = types::uses_sret(&dest_ty) || self.callee_returns_dynamic(func);

        // Build argument list.
        let mut call_args = String::from("rt");
        if uses_sret {
            write!(&mut call_args, ", {}", self.value_addr(dest)).unwrap();
        }
        for arg in args {
            write!(&mut call_args, ", {}", self.operand_addr(arg)).unwrap();
        }

        // Then one descriptor for each of the callee's type parameters, for
        // what this call bound it to.
        for binding in bindings {
            let name = self.tydesc_name(binding);
            write!(&mut call_args, ", &{}", name).unwrap();
        }

        if uses_sret || dest_ty == IrType::Unit {
            writeln!(out, "    {}({});", func_name, call_args).unwrap();
        } else {
            let dest_addr = self.value_addr(dest);
            let c_ty = types::ir_type_to_c(&dest_ty);
            writeln!(out, "    *({c_ty}*){dest_addr} = {}({});", func_name, call_args).unwrap();
        }
        self.mark_out_destinations_live(out, func, args);
        let modes = self.callee_param_modes(func);
        self.mark_in_arguments_moved(out, &modes, args);
        Ok(())
    }

    /// Record that the call took every owned `in` argument away.
    ///
    /// The callee owns what it is handed by value, and the tracked drop at the
    /// end of the argument's scope must see that it has gone. A callee that
    /// drops its parameter in place zeroes the caller's copy, which hid this,
    /// but one that moves the parameter elsewhere leaves the caller still
    /// holding what the callee frees. Whether a value of a type known only at
    /// run time is given up or copied is its descriptor's to say.
    fn mark_in_arguments_moved(&mut self, out: &mut String, modes: &[datalove_datafun_ir::ParamMode], args: &[Operand]) {
        use datalove_datafun_ir::ParamMode;
        for (i, arg) in args.iter().enumerate() {
            if modes.get(i) != Some(&ParamMode::In) {
                continue;
            }
            let offset = match arg {
                Operand::Slot(id) => self.layout.slot_tracking_byte(id.0),
                Operand::Param(id) => self.layout.param_tracking_byte(id.0),
                _ => None,
            };
            let Some(offset) = offset else { continue };
            let ty = self.operand_type(arg).clone();
            if ty.is_dynamic() {
                writeln!(out, "    if (!({}.flags & {})) __frame[{}] = TRACK_MOVED;",
                    self.tydesc_name(&ty), datalove_rtdt::TyDesc::PLAIN, offset).unwrap();
            } else if !ty.is_copy() {
                writeln!(out, "    __frame[{}] = TRACK_MOVED;", offset).unwrap();
            }
        }
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

    /// How `func` takes each of its parameters.
    /// Whether the callee returns a dynamic type, which it writes through an
    /// sret pointer whatever the caller makes of the result.
    fn callee_returns_dynamic(&self, func: &CodeRef) -> bool {
        let unit = match func {
            CodeRef::Module { module, id } => self.registry.get_module_function_as_unit(*module, *id),
            CodeRef::Local(id) => {
                let lookup_unit = self.parent_unit.unwrap_or(self.unit);
                lookup_unit.nested_units.iter().find(|nested| nested.id == *id)
            }
            CodeRef::External { .. } => None,
        };
        unit.and_then(|u| u.function_context()).is_some_and(|c| c.return_type.is_dynamic())
    }

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

    /// The descriptor for an operand, as a C expression of pointer type: its
    /// own type's, a static descriptor or a descriptor table entry.
    fn operand_tydesc(&mut self, operand: &Operand) -> String {
        let ty = self.operand_type(operand).clone();
        format!("&{}", self.tydesc_name(&ty))
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
    /// after the out parameter, in the order the native declared them.
    fn emit_native_call(
        &mut self,
        out: &mut String,
        dest: ValueId,
        symbol: &str,
        args: &[Operand],
        undetermined: &[IrType],
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

        for ty in undetermined {
            let name = self.tydesc_name(ty);
            write!(&mut call_args, ", &{}", name).unwrap();
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
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, self.size_expr(&ty)).unwrap();
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
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", dest_addr, slot_addr, self.size_expr(ty)).unwrap();
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

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){param_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", param_addr, src_addr, self.size_expr(ty)).unwrap();
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
            let offsets = self.field_offsets(&current_ty, field_types.len());
            let field_offset = &offsets[idx as usize];
            current_addr = format!("({} + {})", current_addr, field_offset);
            current_ty = field_types[idx as usize].clone();
        }

        let src_addr = self.operand_addr(value);
        let repr = types::ir_type_to_crepr(&current_ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){current_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", current_addr, src_addr, self.size_expr(&current_ty)).unwrap();
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

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){dest_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, self.size_expr(&ty)).unwrap();
            }
        }

        if tracked {
            // This only records that something is there now. A reference
            // into a place this frame does not own has no tracking byte here,
            // and then there is nothing to record.
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
            let offsets = self.field_offsets(&current_ty, field_types.len());
            let field_offset = &offsets[idx as usize];
            current_addr = format!("({} + {})", current_addr, field_offset);
            current_ty = field_types[idx as usize].clone();
        }

        let src_addr = self.operand_addr(value);
        let repr = types::ir_type_to_crepr(&current_ty);

        match repr {
            CRepr::Scalar(c_ty) => {
                writeln!(out, "    *({c_ty}*){current_addr} = *({c_ty}*){src_addr};").unwrap();
            }
            CRepr::Aggregate(_) => {
                writeln!(out, "    memcpy({}, {}, {});", current_addr, src_addr, self.size_expr(&current_ty)).unwrap();
            }
        }

        if tracked {
            todo!("RefSetFieldTracked tracking byte write not yet implemented in C AOT");
        }
        Ok(())
    }

    /// Emit drop.
    fn emit_drop(&mut self, out: &mut String, operand: &Operand, tracked: bool) -> Result<(), CAotError> {
        let addr = self.operand_addr(operand);
        let ty = self.operand_type(operand).clone();
        // A copy type owns nothing to destroy.
        if ty.is_copy() {
            return Ok(());
        }
        let tydesc = format!("&{}", self.tydesc_name(&ty));

        if tracked {
            // Check tracking byte before dropping.
            let track_offset = match operand {
                Operand::Slot(sid) => self.layout.slot_tracking_byte(sid.0),
                Operand::Value(vid) => self.layout.value_tracking_byte(vid.0),
                Operand::Param(pid) => self.layout.param_tracking_byte(pid.0),
                _ => None,
            };

            if let Some(offset) = track_offset {
                writeln!(out, "    if (__frame[{}] == TRACK_LIVE) {{ dtlv_rti_any_destroy_local(rt, {}, {}); __frame[{}] = TRACK_MOVED; }}",
                    offset, addr, tydesc, offset).unwrap();
            } else {
                writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, {});", addr, tydesc).unwrap();
            }
        } else {
            writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, {});", addr, tydesc).unwrap();
        }
        Ok(())
    }

    /// Emit drop field: destroy a field in place, where its type owns
    /// something.
    fn emit_drop_field(&mut self, out: &mut String, base: &Operand, field_path: &[u32]) -> Result<(), CAotError> {
        // A reference value names what it points at.
        let (addr, base_ty) = match self.operand_type(base) {
            IrType::Ref(inner) => (format!("(*(void**){})", self.operand_addr(base)), inner.as_ref().clone()),
            ty => (self.operand_addr(base), ty.clone()),
        };
        let mut addr = addr;
        let mut ty = base_ty;
        for &idx in field_path {
            let field_types: Vec<IrType> = match &ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(fs) => fs.iter().map(|(_, t)| t.clone()).collect(),
                other => return Err(CAotError::Codegen(format!(
                    "field path steps through a {:?}, which has no fields", other))),
            };
            let offsets = self.field_offsets(&ty, field_types.len());
            addr = format!("({} + {})", addr, offsets[idx as usize]);
            ty = field_types[idx as usize].clone();
        }
        if !ty.is_copy() {
            writeln!(out, "    dtlv_rti_any_destroy_local(rt, {}, &{});", addr, self.tydesc_name(&ty)).unwrap();
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
        let elem_size = self.size_expr(&elem_ty);

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
        is_valid: ValueId,
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
        // An element type known here that owns nothing is a bounds check and
        // a store: nothing to destroy, in bounds or out.
        if elem_ty.is_copy() {
            let is_valid_addr = self.value_addr(is_valid);
            let size_offset = std::mem::offset_of!(datalove_rtdt::List, size);
            writeln!(out, "    *(bool_t*){} = (*(index_t*){} < *(index_t*)({} + {}));",
                is_valid_addr, index_addr, list_addr, size_offset).unwrap();
            let elem_repr = types::ir_type_to_crepr(&elem_ty);
            let elem_addr = format!("(*(uint8_t**){} + (size_t)*(index_t*){} * {})",
                list_addr, index_addr, elem_repr.layout().size);
            match elem_repr {
                CRepr::Scalar(c_ty) => {
                    writeln!(out, "    if (*(bool_t*){is_valid_addr}) *({c_ty}*){elem_addr} = *({c_ty}*){value_addr};").unwrap();
                }
                CRepr::Aggregate(layout) => {
                    if layout.size > 0 {
                        writeln!(out, "    if (*(bool_t*){}) memcpy({}, {}, {});",
                            is_valid_addr, elem_addr, value_addr, layout.size).unwrap();
                    }
                }
            }
            return Ok(());
        }

        // The runtime checks the index, destroys what was there and stores the
        // value; past the end it destroys the value instead.
        let list_tydesc = self.operand_tydesc(list);
        let value_tydesc = self.operand_tydesc(value);
        writeln!(out, "    *(bool_t*){} = dtlv_rti_list_set_local(rt, {}, {}, *(index_t*){}, {}, {}) == 1;",
            self.value_addr(is_valid), list_addr, list_tydesc, index_addr, value_addr, value_tydesc).unwrap();
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

        let elem_size = self.size_expr(&elem_ty);

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

        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);

        writeln!(out, "    dtlv_rti_btreemap_contains_key_local(rt, {}, {}, {}, &{}, (bool_t*){});",
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

        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);
        let value_tydesc = self.tydesc_name(&value_ty);

        // Get pointer to value (returns null on miss).
        writeln!(out, "    {{").unwrap();
        writeln!(out, "        void* __vptr;").unwrap();
        writeln!(out, "        dtlv_rti_btreemap_get_value_ref_local(rt, {}, {}, {}, &{}, &__vptr);",
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
        is_valid: ValueId,
        map: &Operand,
        key: &Operand,
        value: &Operand,
    ) -> Result<(), CAotError> {
        let map_addr = self.operand_addr(map);
        let key_addr = self.operand_addr(key);
        let value_addr = self.operand_addr(value);

        let map_ty = self.operand_type(map).clone();
        let key_ty = match &map_ty {
            IrType::Map(k, _) => k.as_ref().clone(),
            _ => return Err(CAotError::Codegen(format!(
                "MapSetValue on non-map type: {:?}", map_ty
            ))),
        };

        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);
        let value_tydesc = self.operand_tydesc(value);

        // The runtime takes the value whether or not the key is there.
        writeln!(out, "    *(bool_t*){} = dtlv_rti_btreemap_set_value_local(rt, {}, {}, {}, &{}, {}, {}) == 1;",
            self.value_addr(is_valid), map_addr, map_tydesc, key_addr, key_tydesc, value_addr, value_tydesc).unwrap();
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

    /// Which cursor a slot holds.
    fn cursor_kind(&self, cursor: SlotId) -> CursorKind {
        match self.slot_type(cursor) {
            IrType::Cursor(kind) => *kind,
            other => panic!("a cursor instruction on a slot of {other}"),
        }
    }

    // A cursor begins and advances through the runtime's protocol,
    // `dtlv_rti_*_cursor_*`, and is read through its `rtdt` fields. Most of
    // the protocol is open-coded on those layouts too, keeping the runtime's
    // meanings field for field. What stays a
    // call is starting a walk over a tensor or a tree, once a walk, and a
    // tree's advance from one leaf to the next.

    /// Whether a walk is over: no elements remaining, or no leaf.
    fn cursor_done(&self, cursor: SlotId) -> String {
        let at = self.slot_addr(cursor);
        match self.cursor_kind(cursor) {
            CursorKind::Stride => format!("(*(index_t*)({at} + {}) == 0)", offset_of!(rtdt::StrideCursor, remaining)),
            CursorKind::Row => format!("(*(index_t*)({at} + {}) == 0)", offset_of!(rtdt::RowCursor, remaining)),
            CursorKind::Tree => format!("(*(void**)({at} + {}) == NULL)", offset_of!(rtdt::TreeCursor, leaf)),
        }
    }

    /// The current element: the cursor's pointer, or for a row, the view
    /// inside the cursor.
    fn cursor_elem(&self, cursor: SlotId) -> String {
        let at = self.slot_addr(cursor);
        match self.cursor_kind(cursor) {
            CursorKind::Stride => format!("*(const void**)({at} + {})", offset_of!(rtdt::StrideCursor, elem)),
            CursorKind::Row => format!("(const void*)({at} + {})", offset_of!(rtdt::RowCursor, view)),
            CursorKind::Tree => format!("*(const void**)({at} + {})", offset_of!(rtdt::TreeCursor, elem)),
        }
    }

    /// A map's current value.
    fn cursor_value(&self, cursor: SlotId) -> String {
        format!("*(const void**)({} + {})", self.slot_addr(cursor), offset_of!(rtdt::TreeCursor, value))
    }

    /// Start a walk. A list's is open-coded, being three loads.
    fn emit_cursor_begin(&mut self, out: &mut String, cursor: SlotId, coll: &Operand) {
        let coll_addr = self.operand_addr(coll);
        let cursor_addr = self.slot_addr(cursor);
        let func = match (self.cursor_kind(cursor), self.operand_type(coll).clone()) {
            (CursorKind::Stride, IrType::List(elem)) => {
                let stride = self.size_expr(&elem);
                writeln!(out, "    {{ uint8_t* __l = (uint8_t*){coll_addr}; uint8_t* __c = (uint8_t*){cursor_addr};").unwrap();
                writeln!(out, "      *(void**)(__c + {}) = *(void**)(__l + {});",
                    offset_of!(rtdt::StrideCursor, elem), offset_of!(rtdt::List, data)).unwrap();
                writeln!(out, "      *(uintptr_t*)(__c + {}) = (uintptr_t){stride};",
                    offset_of!(rtdt::StrideCursor, stride)).unwrap();
                writeln!(out, "      *(index_t*)(__c + {}) = *(index_t*)(__l + {}); }}",
                    offset_of!(rtdt::StrideCursor, remaining), offset_of!(rtdt::List, size)).unwrap();
                return;
            }
            (CursorKind::Stride, IrType::Tensor(..)) => "dtlv_rti_tensor_cursor_begin_local",
            (CursorKind::Row, IrType::Tensor(..)) => "dtlv_rti_tensor_row_cursor_begin_local",
            (CursorKind::Tree, IrType::Set(_)) => "dtlv_rti_btreeset_cursor_begin_local",
            (CursorKind::Tree, IrType::Map(..)) => "dtlv_rti_btreemap_cursor_begin_local",
            (kind, ty) => panic!("a {kind:?} cursor over a {ty}"),
        };
        let coll_tydesc = self.operand_tydesc(coll);
        writeln!(out, "    {func}(rt, {coll_addr}, {coll_tydesc}, {cursor_addr});").unwrap();
    }

    /// Move a walk on. A list's or a tensor's is a pointer or an offset and a
    /// count; a tree's moves along its leaf inline and calls the runtime only
    /// to cross to the next one.
    fn emit_cursor_advance(&mut self, out: &mut String, cursor: SlotId, coll: &Operand) {
        let at = self.slot_addr(cursor);
        match self.cursor_kind(cursor) {
            CursorKind::Stride => {
                let (elem, stride, remaining) = (
                    offset_of!(rtdt::StrideCursor, elem),
                    offset_of!(rtdt::StrideCursor, stride),
                    offset_of!(rtdt::StrideCursor, remaining),
                );
                // On the integer, since after the last element it points past the
                // buffer, where C does not define pointer arithmetic.
                writeln!(out, "    *(uintptr_t*)({at} + {elem}) += *(uintptr_t*)({at} + {stride});").unwrap();
                writeln!(out, "    *(index_t*)({at} + {remaining}) -= 1;").unwrap();
            }
            CursorKind::Row => {
                let offset = offset_of!(rtdt::RowCursor, view) + offset_of!(rtdt::Tensor, offset_elems);
                let (stride, remaining) = (offset_of!(rtdt::RowCursor, stride), offset_of!(rtdt::RowCursor, remaining));
                writeln!(out, "    *(index_t*)({at} + {offset}) += *(index_t*)({at} + {stride});").unwrap();
                writeln!(out, "    *(index_t*)({at} + {remaining}) -= 1;").unwrap();
            }
            CursorKind::Tree => {
                let coll_ty = self.operand_type(coll).clone();
                let (func, key_ty, value_ty) = match &coll_ty {
                    IrType::Set(elem) => ("dtlv_rti_btreeset_cursor_advance_local", (**elem).clone(), None),
                    IrType::Map(key, value) => ("dtlv_rti_btreemap_cursor_advance_local", (**key).clone(), Some((**value).clone())),
                    ty => panic!("a tree cursor over a {ty}"),
                };
                let (leaf, elem, value, slot) = (
                    offset_of!(rtdt::TreeCursor, leaf),
                    offset_of!(rtdt::TreeCursor, elem),
                    offset_of!(rtdt::TreeCursor, value),
                    offset_of!(rtdt::TreeCursor, slot),
                );
                let len = offset_of!(rtdt::SetNode, len);
                let key_size = self.size_expr(&key_ty);
                writeln!(out, "    {{ uint8_t* __c = (uint8_t*){at}; uint32_t __next = *(uint32_t*)(__c + {slot}) + 1;").unwrap();
                writeln!(out, "      if (__next < *(uint32_t*)(*(uint8_t**)(__c + {leaf}) + {len})) {{").unwrap();
                writeln!(out, "        *(uint32_t*)(__c + {slot}) = __next;").unwrap();
                writeln!(out, "        *(uintptr_t*)(__c + {elem}) += {key_size};").unwrap();
                if let Some(value_ty) = value_ty {
                    let value_size = self.size_expr(&value_ty);
                    writeln!(out, "        *(uintptr_t*)(__c + {value}) += {value_size};").unwrap();
                }
                let coll_tydesc = self.operand_tydesc(coll);
                writeln!(out, "      }} else {{").unwrap();
                writeln!(out, "        {func}(rt, __c, {coll_tydesc});").unwrap();
                writeln!(out, "      }} }}").unwrap();
            }
        }
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

        let map_tydesc = self.operand_tydesc(map);
        let key_tydesc = self.tydesc_name(&key_ty);

        writeln!(out, "    dtlv_rti_btreemap_get_value_ref_local(rt, {}, {}, {}, &{}, (void**){});",
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
                writeln!(out, "    *(float*){} = nearbyintf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::TruncF32 => {
                writeln!(out, "    *(float*){} = truncf(*(float*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::CopysignF32 => {
                writeln!(out, "    *(float*){} = copysignf(*(float*){}, *(float*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MinF32 => {
                writeln!(out, "    *(float*){} = dtlv_minf(*(float*){}, *(float*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MaxF32 => {
                writeln!(out, "    *(float*){} = dtlv_maxf(*(float*){}, *(float*){});", dest_addr, arg0(), arg1()).unwrap();
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
            IntrinsicId::U64ToF64 => {
                writeln!(out, "    *(double*){} = (double)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToF64 => {
                writeln!(out, "    *(double*){} = (double)*(int64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U64ToF32 => {
                writeln!(out, "    *(float*){} = (float)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToF32 => {
                writeln!(out, "    *(float*){} = (float)*(int64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U16ToU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)*(uint16_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U32ToU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)*(uint32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U32ToU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)*(uint32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U64ToU8 => {
                writeln!(out, "    *(uint8_t*){} = (uint8_t)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U64ToU16 => {
                writeln!(out, "    *(uint16_t*){} = (uint16_t)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U64ToU32 => {
                writeln!(out, "    *(uint32_t*){} = (uint32_t)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I16ToI8 => {
                writeln!(out, "    *(int8_t*){} = (int8_t)*(int16_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I32ToI8 => {
                writeln!(out, "    *(int8_t*){} = (int8_t)*(int32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I32ToI16 => {
                writeln!(out, "    *(int16_t*){} = (int16_t)*(int32_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToI8 => {
                writeln!(out, "    *(int8_t*){} = (int8_t)*(int64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToI16 => {
                writeln!(out, "    *(int16_t*){} = (int16_t)*(int64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToI32 => {
                writeln!(out, "    *(int32_t*){} = (int32_t)*(int64_t*){};", dest_addr, arg0()).unwrap();
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
                writeln!(out, "    *(double*){} = nearbyint(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::TruncF64 => {
                writeln!(out, "    *(double*){} = trunc(*(double*){});", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::CopysignF64 => {
                writeln!(out, "    *(double*){} = copysign(*(double*){}, *(double*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MinF64 => {
                writeln!(out, "    *(double*){} = dtlv_min(*(double*){}, *(double*){});", dest_addr, arg0(), arg1()).unwrap();
            }
            IntrinsicId::MaxF64 => {
                writeln!(out, "    *(double*){} = dtlv_max(*(double*){}, *(double*){});", dest_addr, arg0(), arg1()).unwrap();
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
            IntrinsicId::IndexToU64 => {
                writeln!(out, "    *(uint64_t*){} = (uint64_t)*(index_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::U64ToIndex => {
                writeln!(out, "    *(index_t*){} = (index_t)*(uint64_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::OffsetToI64 => {
                writeln!(out, "    *(int64_t*){} = (int64_t)*(offset_t*){};", dest_addr, arg0()).unwrap();
            }
            IntrinsicId::I64ToOffset => {
                writeln!(out, "    *(offset_t*){} = (offset_t)*(int64_t*){};", dest_addr, arg0()).unwrap();
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
                CRepr::Aggregate(_) => {
                    writeln!(out, "    memcpy({}, {}, {});", dest_addr, src_addr, self.size_expr(ty)).unwrap();
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
                CRepr::Aggregate(_) => {
                    writeln!(out, "        memcpy({}, {}, {});", dest_addr, src_addr, self.size_expr(ty)).unwrap();
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
                CRepr::Aggregate(_) => {
                    writeln!(out, "        memcpy({}, {}, {});", dest_addr, src_addr, self.size_expr(ty)).unwrap();
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
                writeln!(out, "    memcpy(__sret, {}, {});", src_addr, self.size_expr(ret_ty)).unwrap();
                self.emit_give_back_shadow(out);
                writeln!(out, "    return;").unwrap();
            } else {
                let src_addr = self.operand_addr(val);
                let c_ty = types::ir_type_to_c(ret_ty);
                self.emit_give_back_shadow(out);
                writeln!(out, "    return *({c_ty}*){src_addr};").unwrap();
            }
        } else {
            self.emit_give_back_shadow(out);
            writeln!(out, "    return;").unwrap();
        }
        Ok(())
    }

    /// Give back the shadow stack space the prologue took, before a return.
    fn emit_give_back_shadow(&self, out: &mut String) {
        if self.takes_shadow {
            writeln!(out, "    ((dtlv_rt_header_t*)rt)->shadow_top = __shadow_top;").unwrap();
            writeln!(out, "    ((dtlv_rt_header_t*)rt)->shadow_limit = __shadow_limit;").unwrap();
        }
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

            let elem_size = self.size_expr(&elem_ty);

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

    /// Emit a TensorSet instruction, rank 1 only.
    ///
    /// As ListSet: the runtime checks the index and writes, or destroys the
    /// value if the index is past the end. A rank 1 tensor's indices are the
    /// one index, so its address is the indices array.
    fn emit_tensor_set(
        &mut self,
        out: &mut String,
        is_valid: ValueId,
        tensor: &Operand,
        index: &Operand,
        value: &Operand,
    ) -> Result<(), CAotError> {
        let tensor_addr = self.operand_addr(tensor);
        let index_addr = self.operand_addr(index);
        let value_addr = self.operand_addr(value);
        let tensor_tydesc = self.operand_tydesc(tensor);
        let value_tydesc = self.operand_tydesc(value);
        writeln!(out, "    *(bool_t*){} = dtlv_rti_tensor_set_local(rt, {}, {}, (const index_t*){}, {}, {}) == 1;",
            self.value_addr(is_valid), tensor_addr, tensor_tydesc, index_addr, value_addr, value_tydesc).unwrap();
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
            let elem_size = self.size_expr(&elem_ty);

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
