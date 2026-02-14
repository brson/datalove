//! Core codegen driver for translating IR to Cranelift.
//!
//! Translates [`IrCodeUnit`] to Cranelift IR using FunctionBuilder. The main type is
//! [`FunctionCompiler`], which handles the translation of a single function.
//!
//! # Submodules
//!
//! Instruction compilation is split across submodules by category:
//! - [`ops`]: Binary and unary arithmetic/logic operations.
//! - [`constants`]: Constant value materialization.
//! - [`collections`]: List, Set, Map construction.
//! - [`aggregates`]: Tuple/struct Pack and Unpack.
//! - [`calls`]: Function call compilation.
//! - [`slots`]: Mutable slot load/store.
//! - [`runtime`]: Runtime calls (DebugLog, Drop).
//! - [`terminators`]: Block terminators (Return, Branch, Goto).
//!
//! # Value representation
//!
//! IR values are represented in Cranelift as either:
//! - **Scalar**: Fits in a register (bools, integers, floats).
//! - **Aggregate**: Stored in the stack frame, tracked by pointer.
//!
//! All function parameters are passed by pointer. The implicit `rt_handle`
//! is threaded as the first parameter to all functions.
//!
//! # Block parameters (loop carries/brings)
//!
//! Block parameters implement loop carry/bring values. The representation differs
//! by value type:
//!
//! - **Scalars**: Cranelift block param IS the value. Pure SSA semantics - the
//!   value flows directly through Goto/Branch instructions.
//!
//! - **Aggregates**: Cranelift block param is a pointer to the source data.
//!   On block entry, we memcpy to the value's fixed frame location. This is
//!   necessary to prevent aliasing when the same frame location is both source
//!   and destination (common in loop carry scenarios).

/// Tuple/struct packing and unpacking.
mod aggregates;
/// Function call compilation.
mod calls;
/// Collection type construction.
mod collections;
/// Constant value materialization.
mod constants;
/// Intrinsic function codegen.
mod intrinsics;
/// List indexing operations.
mod lists;
/// Binary and unary operations.
mod ops;
/// Option and Result operations.
mod options;
/// Boxing operations (ErrorFrom, DataFrom).
mod boxing;
/// Runtime calls (DebugLog, Drop).
mod runtime;
/// Mutable slot operations.
mod slots;
/// Block terminators.
mod terminators;

use std::collections::HashMap;

use cranelift_codegen::ir::{
    self as cl_ir,
    types as cl_types,
    InstBuilder,
    MemFlags,
};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module};

use datalove_datafun_ir::{
    BlockId, FunctionContext, FunctionRegistry, IrCodeUnit,
    IrModuleId, IrType, Instruction, Operand, ParamId, SlotDest, SlotId,
    ValueId,
};

use crate::layout::FrameLayout;
use crate::runtime::RuntimeImports;
use crate::tydesc_emit::TyDescEmitter;
use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

/// Build a Cranelift function signature for an IR code unit.
///
/// All functions have an implicit rt_handle as first parameter.
/// For aggregate returns, an sret (structure return) pointer is the second parameter.
/// User-visible parameters follow, all passed by pointer.
///
/// Panics if the code unit is not a function.
pub fn build_signature_for_func(
    func: &IrCodeUnit,
    isa: &dyn TargetIsa,
) -> cl_ir::Signature {
    let func_ctx = func.function_context()
        .expect("build_signature_for_func requires a function code unit");

    let call_conv = isa.default_call_conv();
    let mut sig = cl_ir::Signature::new(call_conv);

    // Implicit rt_handle as first param (pointer to runtime).
    sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));

    // For aggregate returns, add sret pointer as second param.
    // Caller allocates space and passes pointer; callee writes result there.
    let ret_ty = &func_ctx.return_type;
    let has_sret = match ret_ty {
        IrType::Unit => false,
        _ => matches!(types::ir_type_to_cranelift(ret_ty), CraneliftRepr::Aggregate(_)),
    };
    if has_sret {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // User parameters are passed by pointer.
    for _ in &func_ctx.param_types {
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
    }

    // Return type: scalars in register, Unit/aggregates return nothing (aggregates use sret).
    match ret_ty {
        IrType::Unit => {
            // Unit returns nothing.
        }
        _ => match types::ir_type_to_cranelift(ret_ty) {
            CraneliftRepr::Scalar(cl_ty) => {
                sig.returns.push(cl_ir::AbiParam::new(cl_ty));
            }
            CraneliftRepr::Aggregate(_) => {
                // Aggregate uses sret convention - no return value.
            }
        }
    }

    sig
}

/// Check if a return type uses sret (structure return) convention.
pub fn uses_sret(ret_ty: &IrType) -> bool {
    match ret_ty {
        IrType::Unit => false,
        _ => matches!(types::ir_type_to_cranelift(ret_ty), CraneliftRepr::Aggregate(_)),
    }
}

/// Compiles a single IR function to Cranelift IR.
pub struct FunctionCompiler<'a, M: Module> {
    /// The IR code unit being compiled.
    func: &'a IrCodeUnit,
    /// Function-specific context (always present for compiled code units).
    func_ctx: &'a FunctionContext,
    /// Frame layout for values and slots.
    layout: FrameLayout,
    /// Target ISA for pointer size etc.
    isa: &'a dyn TargetIsa,
    /// Module for declaring functions.
    module: &'a mut M,
    /// Mapping from IR ValueId to Cranelift Value.
    values: HashMap<ValueId, cl_ir::Value>,
    /// Mapping from IR BlockId to Cranelift Block.
    blocks: HashMap<BlockId, cl_ir::Block>,
    /// Mapping from IR ParamId to Cranelift Value (user params, not rt_handle).
    param_values: HashMap<ParamId, cl_ir::Value>,
    /// Mapping from local IR FuncId to Cranelift FuncId.
    local_funcs: HashMap<datalove_datafun_ir::CodeUnitId, FuncId>,
    /// Mapping from module function (IrModuleId, FuncId) to Cranelift FuncId.
    module_funcs: HashMap<(IrModuleId, datalove_datafun_ir::CodeUnitId), FuncId>,
    /// Function registry for looking up module functions.
    #[allow(dead_code)]
    registry: Option<&'a FunctionRegistry>,
    /// Cranelift variables for mutable slots (SlotId).
    #[allow(dead_code)]
    slot_vars: HashMap<SlotId, Variable>,
    /// Stack slot for frame data (aggregates, spilled values).
    frame_slot: Option<cl_ir::StackSlot>,
    /// Next variable index for Cranelift.
    #[allow(dead_code)]
    next_var: u32,
    /// Runtime function imports (optional, for functions that need runtime calls).
    runtime: Option<RuntimeImports>,
    /// TyDesc emitter for runtime type info.
    tydesc_emitter: TyDescEmitter,
    /// Runtime handle (implicit first parameter to all functions).
    rt_handle_param: Option<cl_ir::Value>,
    /// Sret pointer (implicit second parameter for aggregate returns).
    sret_param: Option<cl_ir::Value>,
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Create a new function compiler.
    ///
    /// Panics if the code unit is not a function.
    pub fn new(
        func: &'a IrCodeUnit,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
    ) -> Self {
        let func_ctx = func.function_context()
            .expect("FunctionCompiler requires a function code unit");

        let layout = FrameLayout::compute(
            &func_ctx.param_types,
            &func.value_types,
            &func.slot_types,
            &func.tracked_slots,
            &func_ctx.tracked_params,
        );

        Self {
            func,
            func_ctx,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            local_funcs: HashMap::new(),
            module_funcs: HashMap::new(),
            registry: None,
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: None,
            tydesc_emitter: TyDescEmitter::new(),
            rt_handle_param: None,
            sret_param: None,
        }
    }

    /// Create a new function compiler with runtime imports.
    ///
    /// Use this for functions that may need runtime calls (like DebugLog).
    ///
    /// Panics if the code unit is not a function.
    pub fn new_with_runtime(
        func: &'a IrCodeUnit,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
        runtime: RuntimeImports,
    ) -> Self {
        let func_ctx = func.function_context()
            .expect("FunctionCompiler requires a function code unit");

        let layout = FrameLayout::compute(
            &func_ctx.param_types,
            &func.value_types,
            &func.slot_types,
            &func.tracked_slots,
            &func_ctx.tracked_params,
        );

        Self {
            func,
            func_ctx,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            local_funcs: HashMap::new(),
            module_funcs: HashMap::new(),
            registry: None,
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: Some(runtime),
            tydesc_emitter: TyDescEmitter::new(),
            rt_handle_param: None,
            sret_param: None,
        }
    }

    /// Create a new function compiler with runtime imports and pre-populated TyDescs.
    ///
    /// Use this when TyDescs have been emitted upfront (whole-world compilation).
    ///
    /// Panics if the code unit is not a function.
    pub fn new_with_runtime_and_tydescs(
        func: &'a IrCodeUnit,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
        runtime: RuntimeImports,
        tydesc_emitter: TyDescEmitter,
        registry: Option<&'a FunctionRegistry>,
    ) -> Self {
        let func_ctx = func.function_context()
            .expect("FunctionCompiler requires a function code unit");

        let layout = FrameLayout::compute(
            &func_ctx.param_types,
            &func.value_types,
            &func.slot_types,
            &func.tracked_slots,
            &func_ctx.tracked_params,
        );

        Self {
            func,
            func_ctx,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            local_funcs: HashMap::new(),
            module_funcs: HashMap::new(),
            registry,
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: Some(runtime),
            tydesc_emitter,
            rt_handle_param: None,
            sret_param: None,
        }
    }

    /// Compile the function and return the Cranelift FuncId.
    ///
    /// This declares the function with Export linkage and then defines it.
    /// Use `compile_predeclared` for functions that have already been declared.
    pub fn compile(mut self) -> Result<FuncId, CraneliftError> {
        // Build function signature.
        let sig = self.build_signature();

        // Declare function in module.
        let func_id = self.module
            .declare_function(&self.func.name, Linkage::Export, &sig)
            .map_err(|e| CraneliftError::Module(format!("declare function: {}", e)))?;

        self.compile_body(func_id, sig)
    }

    /// Compile a function that has already been declared.
    ///
    /// Use this for two-pass compilation where functions are declared first.
    pub fn compile_predeclared(mut self, func_id: FuncId) -> Result<FuncId, CraneliftError> {
        let sig = self.build_signature();
        self.compile_body(func_id, sig)
    }

    /// Compile the function body using the given FuncId and signature.
    fn compile_body(&mut self, func_id: FuncId, sig: cl_ir::Signature) -> Result<FuncId, CraneliftError> {
        // Create Cranelift function.
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        // Create function builder context.
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

        // Create frame stack slot if needed.
        if self.layout.frame_size > 0 {
            let slot_data = cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                self.layout.frame_size,
                self.layout.frame_align.try_into().unwrap_or(0),
            );
            self.frame_slot = Some(builder.create_sized_stack_slot(slot_data));
        }

        // Create blocks with their parameters.
        for block in &self.func.blocks {
            let cl_block = builder.create_block();

            // Add block params for IR block params.
            for &param_value_id in &block.params {
                let param_ty = self.func.value_types.get(param_value_id.0 as usize)
                    .cloned()
                    .unwrap_or(IrType::Unit);
                let cl_ty = match types::ir_type_to_cranelift(&param_ty) {
                    CraneliftRepr::Scalar(t) => t,
                    CraneliftRepr::Aggregate(_) => PTR_TYPE,
                };
                builder.append_block_param(cl_block, cl_ty);
            }

            self.blocks.insert(block.id, cl_block);
        }

        // Set up entry block with parameters.
        let entry_block = self.blocks[&BlockId(0)];
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        // Don't seal yet - wait until all blocks are compiled for loop back-edges.

        // Extract block parameters.
        // Layout: [rt_handle, sret? (if aggregate return), user_param_0, user_param_1, ...]
        let param_values: Vec<_> = builder.block_params(entry_block).to_vec();

        // First param is always rt_handle (implicit).
        self.rt_handle_param = Some(param_values[0]);

        // Check if this function uses sret.
        let has_sret = uses_sret(&self.func_ctx.return_type);
        let user_param_start = if has_sret {
            // Second param is sret pointer.
            self.sret_param = Some(param_values[1]);
            2
        } else {
            1
        };

        // User params start after implicit params.
        // Store them for lookup by ParamId.
        for (i, &val) in param_values[user_param_start..].iter().enumerate() {
            let param_id = ParamId(i as u32);
            // Track param values for get_operand_value.
            self.param_values.insert(param_id, val);
        }

        // Initialize aggregate slots and tracking bytes region.
        // - Aggregate slots: 0xFF poison makes uninitialized reads obvious
        // - Tracking bytes: 0x00 (UNINIT), so DropTracked skips them
        if let Some(frame_slot) = self.frame_slot {
            // Fill aggregate slots with 0xFF poison pattern.
            for (slot_idx, slot_ty) in self.func.slot_types.iter().enumerate() {
                let repr = types::ir_type_to_cranelift(slot_ty);
                if let CraneliftRepr::Aggregate(layout) = repr {
                    let slot_offset = self.layout.slot_offset(slot_idx as u32);
                    let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                    let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                    let poison = builder.ins().iconst(cl_types::I8, 0xFF_u8 as i64);
                    builder.call_memset(self.isa.frontend_config(), addr, poison, size);
                }
            }

            // Zero-init tracking bytes.
            if self.layout.tracking_count > 0 {
                let track_addr = builder.ins().stack_addr(
                    PTR_TYPE,
                    frame_slot,
                    self.layout.tracking_offset as i32,
                );
                let size = builder.ins().iconst(PTR_TYPE, self.layout.tracking_count as i64);
                let zero = builder.ins().iconst(cl_types::I8, 0);
                builder.call_memset(self.isa.frontend_config(), track_addr, zero, size);
            }
        }

        // Compile each block.
        for ir_block in &self.func.blocks {
            let cl_block = self.blocks[&ir_block.id];

            // Switch to block (entry already switched).
            if ir_block.id != BlockId(0) {
                builder.switch_to_block(cl_block);

                // Map block params to IR ValueIds.
                //
                // Block params implement loop carries/brings. The IR semantics specify
                // that Goto/Branch MOVE their args INTO the target block's param locations.
                // Each block param conceptually gets a "fresh" value each time the block
                // is entered.
                //
                // Scalars: Cranelift block param IS the value - pure SSA semantics.
                //
                // Aggregates: Cranelift block param is a POINTER to the source data.
                // We must memcpy to a local frame location to:
                // 1. Ensure value semantics (each iteration sees independent data)
                // 2. Prevent aliasing when source and dest overlap (loop carry case)
                let cl_params = builder.block_params(cl_block).to_vec();
                for (ir_value_id, &cl_param) in ir_block.params.iter().zip(cl_params.iter()) {
                    let param_ty = self.func.value_types.get(ir_value_id.0 as usize)
                        .cloned()
                        .unwrap_or(IrType::Unit);
                    let repr = types::ir_type_to_cranelift(&param_ty);

                    match repr {
                        CraneliftRepr::Scalar(_) => {
                            // Scalar: block param IS the value (pure SSA).
                            self.values.insert(*ir_value_id, cl_param);
                        }
                        CraneliftRepr::Aggregate(layout) => {
                            // Aggregate: block param is PTR to source. Copy to local frame.
                            let frame_slot = self.frame_slot.expect("aggregate block param requires frame slot");
                            let dest_offset = self.layout.value_offset(ir_value_id.0);
                            let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                            // memcpy from incoming pointer to local frame location.
                            let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                            builder.call_memcpy(self.isa.frontend_config(), dest_addr, cl_param, size);

                            // Use local address for this value.
                            self.values.insert(*ir_value_id, dest_addr);

                            // Mark as live if tracked.
                            self.mark_value_live(&mut builder, *ir_value_id);
                        }
                    }
                }

                // Don't seal yet - wait until all blocks are compiled for loop back-edges.
            }

            // Compile instructions.
            for inst in &ir_block.instructions {
                self.compile_instruction(&mut builder, inst)?;
            }

            // Compile terminator.
            self.compile_terminator(&mut builder, &ir_block.terminator)?;
        }

        // Seal all blocks now that all predecessors are known (required for loops).
        builder.seal_all_blocks();

        // Finalize function.
        builder.finalize();

        // Define function in module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| CraneliftError::Codegen(format!("define function: {}", e)))?;

        Ok(func_id)
    }

    /// Build the Cranelift function signature.
    ///
    /// All functions have an implicit rt_handle as first parameter.
    /// For aggregate returns, an sret pointer is the second parameter.
    /// User-visible parameters follow.
    fn build_signature(&self) -> cl_ir::Signature {
        // Use the public function to keep consistency.
        build_signature_for_func(self.func, self.isa)
    }

    /// Compile a single instruction.
    fn compile_instruction(
        &mut self,
        builder: &mut FunctionBuilder,
        inst: &Instruction,
    ) -> Result<(), CraneliftError> {
        match inst {
            Instruction::Const { dest, value } => {
                self.compile_const(builder, *dest, value)?;
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                self.compile_binop(builder, *dest, *op, lhs, rhs)?;
            }
            Instruction::UnaryOp { dest, op, operand } => {
                self.compile_unaryop(builder, *dest, *op, operand)?;
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                self.compile_binop_checked(builder, *dest, *overflow, *op, lhs, rhs)?;
            }
            Instruction::UnaryOpChecked { dest, overflow, op, operand } => {
                self.compile_unaryop_checked(builder, *dest, *overflow, *op, operand)?;
            }
            Instruction::Widen { dest, src } => {
                self.compile_widen(builder, *dest, src)?;
            }
            Instruction::WidenFixed { dest, src } => {
                self.compile_widen_fixed(builder, *dest, src)?;
            }
            Instruction::Clone { dest, src } => {
                self.compile_clone(builder, *dest, src)?;
            }
            Instruction::Copy { dest, src } => {
                self.compile_copy(builder, *dest, src)?;
            }
            Instruction::Move { dest, src } => {
                self.compile_copy(builder, *dest, src)?;
            }
            Instruction::Pack { dest, ty: _, fields } => {
                self.compile_pack(builder, *dest, fields)?;
            }
            Instruction::Unpack { dests, src } => {
                self.compile_unpack(builder, dests, src)?;
            }
            Instruction::GetField { dest, src, field_index } => {
                self.compile_get_field(builder, *dest, src, *field_index)?;
            }
            Instruction::GetFieldRef { dest, src, field_index } => {
                self.compile_get_field_ref(builder, *dest, src, *field_index)?;
            }
            Instruction::SetField { slot, field_path, value } => {
                self.compile_set_field(builder, slot, field_path, value)?;
            }
            Instruction::Nop => {}
            Instruction::ListGet { dest, is_valid, list, index } => {
                self.compile_list_get(builder, *dest, *is_valid, list, index)?;
            }
            Instruction::ListBoundsCheck { is_valid, list, index } => {
                self.compile_list_bounds_check(builder, *is_valid, list, index)?;
            }
            Instruction::ListSet { list, index, value } => {
                self.compile_list_set(builder, list, index, value)?;
            }
            Instruction::DebugLog { operand } => {
                self.compile_debuglog(builder, operand)?;
            }
            Instruction::Call { dest, func, args, .. } => {
                self.compile_call(builder, *dest, func, args)?;
            }
            // ComptimeCall behaves exactly like Call - the specialization metadata is
            // only used by the specialization pass. Without specialization, this calls
            // the original function with original args.
            Instruction::ComptimeCall { dest, func, args, .. } => {
                self.compile_call(builder, *dest, func, args)?;
            }
            Instruction::SlotStoreCopy { dest, value } => {
                self.compile_slot_store(builder, dest, value, true)?;
            }
            Instruction::SlotStoreMove { dest, value } => {
                self.compile_slot_store(builder, dest, value, false)?;
            }
            Instruction::SlotLoadCopy { dest, slot } => {
                self.compile_slot_load(builder, *dest, *slot, true)?;
            }
            Instruction::SlotLoadMove { dest, slot } => {
                // Precise slot load: compiler guarantees slot is occupied.
                self.compile_slot_load(builder, *dest, *slot, false)?;
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                // Tracked slot load: copy then mark slot as moved.
                self.compile_slot_load(builder, *dest, *slot, false)?;
                self.mark_tracking_moved(builder, &Operand::Slot(*slot));
            }
            Instruction::ParamStore { param, value } => {
                self.compile_param_store(builder, *param, value)?;
            }
            Instruction::ParamStoreTracked { param, value } => {
                self.compile_param_store_tracked(builder, *param, value)?;
            }
            Instruction::ParamSetField { param, field_path, value } => {
                self.compile_param_set_field(builder, param, field_path, value)?;
            }
            Instruction::ParamSetFieldTracked { param, field_path, value } => {
                self.compile_param_set_field_tracked(builder, param, field_path, value)?;
            }
            Instruction::RefStore { dest, value } => {
                self.compile_ref_store(builder, dest, value)?;
            }
            Instruction::RefStoreTracked { dest, value } => {
                self.compile_ref_store_tracked(builder, dest, value)?;
            }
            Instruction::RefSetField { dest, field_path, value } => {
                self.compile_ref_set_field(builder, dest, field_path, value)?;
            }
            Instruction::RefSetFieldTracked { dest, field_path, value } => {
                self.compile_ref_set_field_tracked(builder, dest, field_path, value)?;
            }
            Instruction::Drop { operand } => {
                self.compile_drop(builder, operand)?;
            }
            Instruction::DropTracked { operand } => {
                self.compile_drop_tracked(builder, operand)?;
            }
            Instruction::DropViaRef { ref_value } => {
                self.compile_drop_via_ref(builder, *ref_value)?;
            }
            Instruction::UnitEndDrop { operand } => {
                // Precise binding: unconditional drop.
                self.compile_drop(builder, operand)?;
            }
            Instruction::UnitEndDropTracked { operand } => {
                // Tracked binding: conditional drop (checks tracking byte).
                self.compile_drop_tracked(builder, operand)?;
            }
            Instruction::ListNew { dest, elements } => {
                self.compile_list_new(builder, *dest, elements)?;
            }
            Instruction::SetNew { dest, elements } => {
                self.compile_set_new(builder, *dest, elements)?;
            }
            Instruction::MapNew { dest, entries } => {
                self.compile_map_new(builder, *dest, entries)?;
            }
            Instruction::TensorNew { dest, shape, elements } => {
                self.compile_tensor_new(builder, *dest, shape, elements)?;
            }
            Instruction::TableNew { dest, rows } => {
                self.compile_table_new(builder, *dest, rows)?;
            }

            // Option/Result instructions.
            Instruction::WrapSome { dest, inner } => {
                self.compile_wrap_some(builder, *dest, inner)?;
            }
            Instruction::WrapNone { dest } => {
                self.compile_wrap_none(builder, *dest)?;
            }
            Instruction::WrapOk { dest, inner } => {
                self.compile_wrap_ok(builder, *dest, inner)?;
            }
            Instruction::WrapErr { dest, inner } => {
                self.compile_wrap_err(builder, *dest, inner)?;
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                self.compile_unwrap_option(builder, *dest, *is_some, src)?;
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                self.compile_unwrap_result(builder, *ok_dest, *err_dest, *is_ok, src)?;
            }
            Instruction::EnumVariant { dest, variant_index, payload } => {
                self.compile_enum_variant(builder, *dest, *variant_index, payload.as_ref())?;
            }
            Instruction::EnumDiscriminant { dest, src } => {
                self.compile_enum_discriminant(builder, *dest, src)?;
            }
            Instruction::EnumPayload { dest, src, variant_index } => {
                self.compile_enum_payload(builder, *dest, src, *variant_index)?;
            }
            Instruction::ErrorFrom { dest, inner } => {
                self.compile_error_from(builder, *dest, inner)?;
            }
            Instruction::DataFrom { dest, inner } => {
                self.compile_data_from(builder, *dest, inner)?;
            }
            Instruction::Intrinsic { dest, intrinsic, args } => {
                self.compile_intrinsic(builder, *dest, *intrinsic, args)?;
            }

            // ================================================================
            // Slot tracking variants - write slot tracking bytes
            // ================================================================
            Instruction::SlotStoreCopyTracked { dest, value } => {
                self.compile_slot_store(builder, dest, value, true)?;
                if let SlotDest::Local(sid) = dest {
                    self.mark_slot_live(builder, *sid);
                }
            }
            Instruction::SlotStoreMoveTracked { dest, value } => {
                self.compile_slot_store(builder, dest, value, false)?;
                if let SlotDest::Local(sid) = dest {
                    self.mark_slot_live(builder, *sid);
                }
            }
            Instruction::SetFieldTracked { slot, field_path, value } => {
                self.compile_set_field(builder, slot, field_path, value)?;
                if let SlotDest::Local(sid) = slot {
                    self.mark_slot_live(builder, *sid);
                }
            }
        }
        Ok(())
    }

    /// Register a local function that has been declared.
    ///
    /// Call this for each local function before compiling any function bodies
    /// that may call them.
    pub fn register_local_func(
        &mut self,
        ir_func_id: datalove_datafun_ir::CodeUnitId,
        cl_func_id: FuncId,
    ) {
        self.local_funcs.insert(ir_func_id, cl_func_id);
    }

    /// Set all local function mappings at once.
    ///
    /// Use this for two-pass compilation where all functions are declared first.
    pub fn set_local_funcs(&mut self, local_funcs: HashMap<datalove_datafun_ir::CodeUnitId, FuncId>) {
        self.local_funcs = local_funcs;
    }

    /// Set all module function mappings at once.
    ///
    /// Use this for three-pass compilation where all module functions are declared first.
    pub fn set_module_funcs(&mut self, module_funcs: HashMap<(IrModuleId, datalove_datafun_ir::CodeUnitId), FuncId>) {
        self.module_funcs = module_funcs;
    }

    /// Get a pointer to an operand's value.
    ///
    /// For aggregates already in memory, returns the pointer directly.
    /// For scalars in registers, spills to a temporary stack location.
    fn get_operand_ptr(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
        // Params are already passed by pointer - just return the pointer.
        if let Operand::Param(pid) = operand {
            return self.param_values.get(pid).copied().ok_or_else(|| {
                CraneliftError::Codegen(format!("undefined param: {:?}", pid))
            });
        }

        // Slots are in the frame - return the address directly.
        // This is important for mut params: we pass the slot address so writes
        // go to the original slot, not a copy.
        if let Operand::Slot(slot_id) = operand {
            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("no frame slot for slot operand".into())
            })?;
            let slot_offset = self.layout.slot_offset(slot_id.0);
            let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
            return Ok(addr);
        }

        // ValueRef: the stored value is a pointer - return it directly.
        // This is used for ref/mut/out params to get the dereferenced location.
        if let Operand::ValueRef(vid) = operand {
            return self.values.get(vid).copied().ok_or_else(|| {
                CraneliftError::Codegen(format!("undefined value: {:?}", vid))
            });
        }

        let ty = self.get_operand_type(operand)?;

        // Ref types store a pointer value - return it directly without spilling.
        if matches!(&ty, IrType::Ref(_)) {
            return self.get_operand_value(builder, operand);
        }

        let repr = types::ir_type_to_cranelift(&ty);

        match repr {
            CraneliftRepr::Aggregate(_) => {
                // Already a pointer.
                self.get_operand_value(builder, operand)
            }
            CraneliftRepr::Scalar(cl_ty) => {
                // Need to spill to memory.
                let val = self.get_operand_value(builder, operand)?;

                // Use the value's frame offset if available.
                if let Operand::Value(vid) = operand {
                    let offset = self.layout.value_offset(vid.0);
                    let frame_slot = self.frame_slot.ok_or_else(|| {
                        CraneliftError::Codegen("no frame slot for value spill".into())
                    })?;
                    let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, offset as i32);
                    builder.ins().store(MemFlags::new(), val, addr, 0);
                    return Ok(addr);
                }

                // For other operand types, create a temporary slot.
                // This is a simple approach - we create a new stack slot for each spill.
                let size = cl_ty.bytes();
                let slot_data = cl_ir::StackSlotData::new(
                    cl_ir::StackSlotKind::ExplicitSlot,
                    size,
                    0,
                );
                let temp_slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);
                builder.ins().store(MemFlags::new(), val, addr, 0);
                Ok(addr)
            }
        }
    }

    /// Get a Cranelift value for an operand.
    fn get_operand_value(
        &self,
        builder: &mut FunctionBuilder,
        op: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
        match op {
            Operand::Value(vid) => {
                self.values.get(vid).copied().ok_or_else(|| {
                    CraneliftError::Codegen(format!("undefined value: {:?}", vid))
                })
            }
            Operand::ValueRef(vid) => {
                // ValueRef: the stored value is a pointer. Dereference it.
                let ptr = self.values.get(vid).copied().ok_or_else(|| {
                    CraneliftError::Codegen(format!("undefined value: {:?}", vid))
                })?;

                // Get the inner type (the type being pointed to).
                let ref_ty = &self.func.value_types[vid.0 as usize];
                let inner_ty = match ref_ty {
                    IrType::Ref(inner) => inner.as_ref(),
                    _ => return Err(CraneliftError::Codegen(format!(
                        "ValueRef on non-Ref type: {:?}", ref_ty
                    ))),
                };
                let repr = types::ir_type_to_cranelift(inner_ty);

                match repr {
                    CraneliftRepr::Scalar(cl_ty) => {
                        // Load scalar value from the pointer.
                        Ok(builder.ins().load(cl_ty, MemFlags::new(), ptr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // For aggregates, return the pointer itself.
                        Ok(ptr)
                    }
                }
            }
            Operand::Param(pid) => {
                // Params are passed by pointer. Load the value from the pointer.
                let param_ptr = self.param_values.get(pid).copied().ok_or_else(|| {
                    CraneliftError::Codegen(format!("undefined param: {:?}", pid))
                })?;

                let param_ty = &self.func_ctx.param_types[pid.0 as usize];
                let repr = types::ir_type_to_cranelift(param_ty);

                match repr {
                    CraneliftRepr::Scalar(cl_ty) => {
                        // Load scalar value from param pointer.
                        Ok(builder.ins().load(cl_ty, MemFlags::new(), param_ptr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // For aggregates, return the pointer itself.
                        Ok(param_ptr)
                    }
                }
            }
            Operand::Slot(slot_id) => {
                // Load value from slot in frame.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    CraneliftError::Codegen("no frame slot for slot operand".into())
                })?;

                let slot_offset = self.layout.slot_offset(slot_id.0);
                let slot_ty = &self.func.slot_types[slot_id.0 as usize];
                let repr = types::ir_type_to_cranelift(slot_ty);

                match repr {
                    CraneliftRepr::Scalar(cl_ty) => {
                        let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                        Ok(builder.ins().load(cl_ty, MemFlags::new(), addr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // Aggregate: return pointer to slot location.
                        Ok(builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32))
                    }
                }
            }
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                Err(CraneliftError::Unsupported(format!(
                    "operand type not yet implemented: {:?}",
                    op
                )))
            }
        }
    }

    /// Get the type of an operand.
    fn get_operand_type(&self, op: &Operand) -> Result<IrType, CraneliftError> {
        match op {
            Operand::Value(vid) => {
                Ok(self.func.value_types[vid.0 as usize].clone())
            }
            Operand::ValueRef(vid) => {
                // ValueRef dereferences, so return the inner type.
                let ref_ty = &self.func.value_types[vid.0 as usize];
                match ref_ty {
                    IrType::Ref(inner) => Ok(inner.as_ref().clone()),
                    _ => Err(CraneliftError::Codegen(format!(
                        "ValueRef on non-Ref type: {:?}", ref_ty
                    ))),
                }
            }
            Operand::Param(pid) => {
                Ok(self.func_ctx.param_types[pid.0 as usize].clone())
            }
            Operand::Slot(sid) => {
                Ok(self.func.slot_types[sid.0 as usize].clone())
            }
            _ => Err(CraneliftError::Unsupported(format!(
                "get_operand_type for {:?}",
                op
            ))),
        }
    }

    /// Allocate a new Cranelift variable.
    #[allow(dead_code)]
    fn alloc_var(&mut self) -> Variable {
        let var = Variable::from_u32(self.next_var);
        self.next_var += 1;
        var
    }

    /// Get the tracking byte offset for an operand, if it's tracked.
    fn tracking_byte_offset(&self, operand: &Operand) -> Option<u32> {
        match operand {
            Operand::Value(vid) => self.layout.values[vid.0 as usize].tracking_byte,
            Operand::Slot(sid) => self.layout.slots[sid.0 as usize].tracking_byte,
            _ => None,
        }
    }

    /// Mark a tracking byte as LIVE.
    fn mark_tracking_live(
        &self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) {
        use crate::layout::tracking;

        if let Some(track_offset) = self.tracking_byte_offset(operand) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, track_offset as i32);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            builder.ins().store(MemFlags::new(), live_val, track_addr, 0);
        }
    }

    /// Mark a tracking byte as MOVED.
    fn mark_tracking_moved(
        &self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) {
        use crate::layout::tracking;

        if let Some(track_offset) = self.tracking_byte_offset(operand) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, track_offset as i32);
            let moved_val = builder.ins().iconst(cl_types::I8, tracking::MOVED as i64);
            builder.ins().store(MemFlags::new(), moved_val, track_addr, 0);
        }
    }

    /// Mark a tracked value as LIVE after initialization.
    fn mark_value_live(&self, builder: &mut FunctionBuilder, vid: ValueId) {
        self.mark_tracking_live(builder, &Operand::Value(vid));
    }

    /// Mark a tracked slot as LIVE after store.
    fn mark_slot_live(&self, builder: &mut FunctionBuilder, sid: SlotId) {
        self.mark_tracking_live(builder, &Operand::Slot(sid));
    }

    /// Get tracking byte offset for a param, if tracked.
    fn param_tracking_byte_offset(&self, pid: ParamId) -> Option<u32> {
        self.layout.params[pid.0 as usize].tracking_byte
    }

    /// Mark a tracked param as LIVE after store.
    fn mark_param_live(&self, builder: &mut FunctionBuilder, pid: ParamId) {
        use crate::layout::tracking;

        if let Some(track_offset) = self.param_tracking_byte_offset(pid) {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let track_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, track_offset as i32);
            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            builder.ins().store(MemFlags::new(), live_val, track_addr, 0);
        }
    }

    /// Get the pointer stored in a ref value (from GetFieldRef).
    ///
    /// For ref values, the stored cranelift value IS the pointer.
    fn get_value_as_ref_ptr(
        &self,
        _builder: &mut FunctionBuilder,
        vid: ValueId,
    ) -> Result<cl_ir::Value, CraneliftError> {
        // GetFieldRef stores the pointer directly in self.values.
        self.values.get(&vid).copied().ok_or_else(|| {
            CraneliftError::Codegen(format!("ref value {:?} not found", vid))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_object::{ObjectBuilder, ObjectModule};
    use datalove_datafun_ir::{BinOp, IrBlock, IrCodeUnit, CodeUnitId, CodeUnitContext, FunctionContext, ConstValue, Terminator, SymbolTable};

    fn create_test_isa() -> std::sync::Arc<dyn TargetIsa> {
        use cranelift_codegen::isa;
        use cranelift_codegen::settings::{self, Configurable};
        use target_lexicon::Triple;

        let mut settings_builder = settings::builder();
        settings_builder.set("opt_level", "speed").unwrap();
        let flags = settings::Flags::new(settings_builder);

        isa::lookup(Triple::host())
            .unwrap()
            .finish(flags)
            .unwrap()
    }

    fn create_test_module(isa: std::sync::Arc<dyn TargetIsa>) -> ObjectModule {
        let obj_builder = ObjectBuilder::new(
            isa,
            "test",
            cranelift_module::default_libcall_names(),
        ).unwrap();
        ObjectModule::new(obj_builder)
    }

    /// Helper to create a function code unit for tests.
    fn make_func_unit(
        name: &str,
        return_type: IrType,
        blocks: Vec<IrBlock>,
        value_types: Vec<IrType>,
    ) -> IrCodeUnit {
        IrCodeUnit {
            id: CodeUnitId(0),
            name: name.into(),
            blocks,
            value_count: value_types.len() as u32,
            slot_count: 0,
            call_site_count: 0,
            value_types,
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![],
            symbols: SymbolTable::default(),
            context: CodeUnitContext::Function(FunctionContext {
                params: vec![],
                param_modes: vec![],
                param_types: vec![],
                return_type,
                tracked_params: vec![],
            }),
            nested_units: vec![],
        }
    }

    #[test]
    fn test_compile_const_i32() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { 42 }
        let code_unit = make_func_unit(
            "test_const",
            IrType::I32,
            vec![
                IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(42),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(0))),
                    },
                },
            ],
            vec![IrType::I32],
        );

        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    // test_compile_binop_add removed: i32 + i32 -> i32 is invalid IR.
    // Fixed-width integer arithmetic uses widening (-> Int) or checked ops (BinOpChecked).

    #[test]
    fn test_compile_comparison() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> bool { 10 < 32 }
        let code_unit = make_func_unit(
            "test_cmp",
            IrType::Bool,
            vec![
                IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(10),
                        },
                        Instruction::Const {
                            dest: ValueId(1),
                            value: ConstValue::I32(32),
                        },
                        Instruction::BinOp {
                            dest: ValueId(2),
                            op: BinOp::Lt,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            vec![IrType::I32, IrType::I32, IrType::Bool],
        );

        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_unary_neg() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { -42 }
        let code_unit = make_func_unit(
            "test_neg",
            IrType::I32,
            vec![
                IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(42),
                        },
                        Instruction::UnaryOp {
                            dest: ValueId(1),
                            op: datalove_datafun_ir::UnaryOp::Neg,
                            operand: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
            ],
            vec![IrType::I32, IrType::I32],
        );

        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_branch() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function with a branch:
        // fn foo() -> i32 {
        //     if true { 1 } else { 2 }
        // }
        let code_unit = IrCodeUnit {
            id: CodeUnitId(0),
            name: "test_branch".into(),
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::Bool(true),
                        },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(0)),
                        then_block: BlockId(1),
                        then_args: vec![],
                        else_block: BlockId(2),
                        else_args: vec![],
                    },
                },
                IrBlock {
                    id: BlockId(1),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(1),
                            value: ConstValue::I32(1),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
                IrBlock {
                    id: BlockId(2),
                    params: vec![],
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(2),
                            value: ConstValue::I32(2),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            value_count: 3,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![IrType::Bool, IrType::I32, IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![],
            symbols: SymbolTable::default(),
            context: CodeUnitContext::Function(FunctionContext {
                params: vec![],
                param_modes: vec![],
                param_types: vec![],
                return_type: IrType::I32,
                tracked_params: vec![],
            }),
            nested_units: vec![],
        };

        let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }
}
