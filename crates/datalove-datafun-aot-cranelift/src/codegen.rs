//! Core codegen driver for translating IR to Cranelift.
//!
//! Translates IrFunction to Cranelift IR using FunctionBuilder.

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
    BinOp, BlockId, ConstValue, FuncRef, IrFunction, IrType, Instruction,
    Operand, ParamId, SlotDest, SlotId, Terminator, UnaryOp, ValueId,
};

use crate::layout::FrameLayout;
use crate::runtime::RuntimeImports;
use crate::tydesc_emit::TyDescEmitter;
use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::AotError;

/// Compiles a single IR function to Cranelift IR.
pub struct FunctionCompiler<'a, M: Module> {
    /// The IR function being compiled.
    func: &'a IrFunction,
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
    local_funcs: HashMap<datalove_datafun_ir::FuncId, FuncId>,
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
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Create a new function compiler.
    pub fn new(
        func: &'a IrFunction,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
    ) -> Self {
        let layout = FrameLayout::compute(
            &func.param_types,
            &func.value_types,
            &func.slot_types,
        );

        Self {
            func,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            local_funcs: HashMap::new(),
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: None,
            tydesc_emitter: TyDescEmitter::new(),
            rt_handle_param: None,
        }
    }

    /// Create a new function compiler with runtime imports.
    ///
    /// Use this for functions that may need runtime calls (like DebugLog).
    pub fn new_with_runtime(
        func: &'a IrFunction,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
        runtime: RuntimeImports,
    ) -> Self {
        let layout = FrameLayout::compute(
            &func.param_types,
            &func.value_types,
            &func.slot_types,
        );

        Self {
            func,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            local_funcs: HashMap::new(),
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: Some(runtime),
            tydesc_emitter: TyDescEmitter::new(),
            rt_handle_param: None,
        }
    }

    /// Create a new function compiler with runtime imports and pre-populated TyDescs.
    ///
    /// Use this when TyDescs have been emitted upfront (whole-world compilation).
    pub fn new_with_runtime_and_tydescs(
        func: &'a IrFunction,
        isa: &'a dyn TargetIsa,
        module: &'a mut M,
        runtime: RuntimeImports,
        tydesc_emitter: TyDescEmitter,
    ) -> Self {
        let layout = FrameLayout::compute(
            &func.param_types,
            &func.value_types,
            &func.slot_types,
        );

        Self {
            func,
            layout,
            isa,
            module,
            values: HashMap::new(),
            blocks: HashMap::new(),
            param_values: HashMap::new(),
            local_funcs: HashMap::new(),
            slot_vars: HashMap::new(),
            frame_slot: None,
            next_var: 0,
            runtime: Some(runtime),
            tydesc_emitter,
            rt_handle_param: None,
        }
    }

    /// Compile the function and return the Cranelift FuncId.
    pub fn compile(mut self) -> Result<FuncId, AotError> {
        // Build function signature.
        let sig = self.build_signature();

        // Declare function in module.
        let func_id = self.module
            .declare_function(&self.func.name, Linkage::Export, &sig)
            .map_err(|e| AotError::Module(format!("declare function: {}", e)))?;

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

        // Create blocks.
        for block in &self.func.blocks {
            let cl_block = builder.create_block();
            self.blocks.insert(block.id, cl_block);
        }

        // Set up entry block with parameters.
        let entry_block = self.blocks[&BlockId(0)];
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Extract block parameters.
        // Layout: [rt_handle, user_param_0, user_param_1, ...]
        let param_values: Vec<_> = builder.block_params(entry_block).to_vec();

        // First param is always rt_handle (implicit).
        self.rt_handle_param = Some(param_values[0]);

        // User params start at index 1.
        // Store them for lookup by ParamId.
        for (i, &val) in param_values[1..].iter().enumerate() {
            let param_id = ParamId(i as u32);
            // Track param values for get_operand_value.
            self.param_values.insert(param_id, val);
        }

        // Compile each block.
        for ir_block in &self.func.blocks {
            let cl_block = self.blocks[&ir_block.id];

            // Switch to block (entry already switched).
            if ir_block.id != BlockId(0) {
                builder.switch_to_block(cl_block);
                // Seal after all predecessors are known (for now, seal immediately).
                builder.seal_block(cl_block);
            }

            // Compile instructions.
            for inst in &ir_block.instructions {
                self.compile_instruction(&mut builder, inst)?;
            }

            // Compile terminator.
            self.compile_terminator(&mut builder, &ir_block.terminator)?;
        }

        // Finalize function.
        builder.finalize();

        // Define function in module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| AotError::Codegen(format!("define function: {}", e)))?;

        Ok(func_id)
    }

    /// Build the Cranelift function signature.
    ///
    /// All functions have an implicit rt_handle as first parameter.
    /// User-visible parameters follow.
    fn build_signature(&self) -> cl_ir::Signature {
        let call_conv = self.isa.default_call_conv();
        let mut sig = cl_ir::Signature::new(call_conv);

        // Implicit rt_handle as first param (pointer to runtime).
        sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));

        // User parameters are passed by pointer.
        for _ in &self.func.param_types {
            sig.params.push(cl_ir::AbiParam::new(PTR_TYPE));
        }

        // Return slot pointer (caller-allocated).
        // For now, simple scalar returns go in a register.
        let ret_ty = self.func.infer_return_type();
        match types::ir_type_to_cranelift(&ret_ty) {
            CraneliftRepr::Scalar(cl_ty) => {
                sig.returns.push(cl_ir::AbiParam::new(cl_ty));
            }
            CraneliftRepr::Aggregate(_) => {
                // Aggregate returns via pointer (first param is return slot).
                // For now, we'll handle this in later phases.
            }
        }

        sig
    }

    /// Compile a single instruction.
    fn compile_instruction(
        &mut self,
        builder: &mut FunctionBuilder,
        inst: &Instruction,
    ) -> Result<(), AotError> {
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
            Instruction::Copy { dest, src } => {
                self.compile_copy(builder, *dest, src)?;
            }
            Instruction::Move { dest, src } => {
                // Move is same as copy for now (ownership tracking is semantic).
                self.compile_copy(builder, *dest, src)?;
            }
            Instruction::Pack { dest, ty: _, fields } => {
                self.compile_pack(builder, *dest, fields)?;
            }
            Instruction::Unpack { dests, src } => {
                self.compile_unpack(builder, dests, src)?;
            }
            Instruction::Nop => {}
            Instruction::DebugLog { operand } => {
                self.compile_debuglog(builder, operand)?;
            }
            Instruction::Call { dest, func, args } => {
                self.compile_call(builder, *dest, func, args)?;
            }
            Instruction::SlotStore { dest, value } => {
                self.compile_slot_store(builder, dest, value)?;
            }
            Instruction::SlotLoad { dest, slot } => {
                self.compile_slot_load(builder, *dest, *slot)?;
            }

            // TODO: More instructions in later phases.
            _ => {
                return Err(AotError::Unsupported(format!(
                    "instruction not yet implemented: {:?}",
                    inst
                )));
            }
        }
        Ok(())
    }

    /// Compile a DebugLog instruction.
    fn compile_debuglog(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<(), AotError> {
        // Need runtime imports for debuglog.
        let debuglog_func_id = self.runtime.as_ref()
            .ok_or_else(|| AotError::Codegen("DebugLog requires runtime imports".into()))?
            .debuglog_local;

        // Need runtime handle.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("DebugLog requires runtime handle parameter".into())
        })?;

        // Get the type of the operand.
        let ty = self.get_operand_type(operand)?;

        // Get pointer to the value. For scalars, we need to spill to memory first.
        let value_ptr = self.get_operand_ptr(builder, operand)?;

        // Look up pre-emitted TyDesc (whole-world compilation guarantees it exists).
        let tydesc_id = self.tydesc_emitter.get(&ty).ok_or_else(|| {
            AotError::Codegen(format!(
                "TyDesc not found for type {:?} - should have been emitted upfront",
                ty
            ))
        })?;

        // Get address of tydesc.
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        let tydesc_addr = builder.ins().global_value(PTR_TYPE, tydesc_gv);

        // Declare debuglog function in this function.
        let debuglog_ref = self.module.declare_func_in_func(
            debuglog_func_id,
            builder.func,
        );

        // Call debuglog.
        builder.ins().call(debuglog_ref, &[rt_handle, value_ptr, tydesc_addr]);

        Ok(())
    }

    /// Compile a Call instruction.
    ///
    /// Threads rt_handle as implicit first argument to callee.
    fn compile_call(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        func_ref: &FuncRef,
        args: &[Operand],
    ) -> Result<(), AotError> {
        // Get rt_handle for threading.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("Call requires runtime handle".into())
        })?;

        // Look up or declare the callee.
        let callee_func_id = self.resolve_func_ref(func_ref)?;

        // Build call arguments: [rt_handle, user_args...]
        let mut call_args = Vec::with_capacity(1 + args.len());
        call_args.push(rt_handle);
        for arg in args {
            let arg_val = self.get_operand_ptr(builder, arg)?;
            call_args.push(arg_val);
        }

        // Declare callee in this function.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);

        // Emit call.
        let call_inst = builder.ins().call(callee_ref, &call_args);

        // Get return value (if any).
        let results = builder.inst_results(call_inst);
        if !results.is_empty() {
            self.values.insert(dest, results[0]);
        } else {
            // Void return - use dummy value.
            let dummy = builder.ins().iconst(cl_types::I8, 0);
            self.values.insert(dest, dummy);
        }

        Ok(())
    }

    /// Resolve a FuncRef to a Cranelift FuncId.
    fn resolve_func_ref(&mut self, func_ref: &FuncRef) -> Result<FuncId, AotError> {
        match func_ref {
            FuncRef::Local(ir_func_id) => {
                // Look up in local_funcs or declare.
                if let Some(&func_id) = self.local_funcs.get(ir_func_id) {
                    return Ok(func_id);
                }

                // For now, assume local functions aren't pre-declared.
                // This requires the callee to be compiled before the caller,
                // or a two-pass approach (declare all, then define all).
                Err(AotError::Unsupported(format!(
                    "local function {:?} not yet declared - needs two-pass compilation",
                    ir_func_id
                )))
            }
            FuncRef::External { unit, func } => {
                Err(AotError::Unsupported(format!(
                    "external function call (unit={}, func={:?}) not yet implemented",
                    unit, func
                )))
            }
            FuncRef::Module { module, func } => {
                Err(AotError::Unsupported(format!(
                    "module function call (module={:?}, func={:?}) not yet implemented",
                    module, func
                )))
            }
        }
    }

    /// Register a local function that has been declared.
    ///
    /// Call this for each local function before compiling any function bodies
    /// that may call them.
    pub fn register_local_func(
        &mut self,
        ir_func_id: datalove_datafun_ir::FuncId,
        cl_func_id: FuncId,
    ) {
        self.local_funcs.insert(ir_func_id, cl_func_id);
    }

    /// Compile a SlotStore instruction.
    fn compile_slot_store(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: &SlotDest,
        value: &Operand,
    ) -> Result<(), AotError> {
        let slot_id = match dest {
            SlotDest::Local(id) => *id,
            SlotDest::External { unit, slot } => {
                return Err(AotError::Unsupported(format!(
                    "external slot store (unit={}, slot={:?}) not yet implemented",
                    unit, slot
                )));
            }
        };

        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for slot store".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot_id.0);
        let slot_ty = &self.func.slot_types[slot_id.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(_cl_ty) => {
                // Scalar: store value directly.
                let val = self.get_operand_value(builder, value)?;
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                builder.ins().store(MemFlags::new(), val, addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                // Aggregate: copy bytes from source to slot.
                let src_ptr = self.get_operand_ptr(builder, value)?;
                let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);

                // Use memcpy for aggregates.
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), dest_addr, src_ptr, size);
            }
        }

        Ok(())
    }

    /// Compile a SlotLoad instruction.
    fn compile_slot_load(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        slot: SlotId,
    ) -> Result<(), AotError> {
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for slot load".into())
        })?;

        let slot_offset = self.layout.slot_offset(slot.0);
        let slot_ty = &self.func.slot_types[slot.0 as usize];
        let repr = types::ir_type_to_cranelift(slot_ty);

        match repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Scalar: load value directly.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                let val = builder.ins().load(cl_ty, MemFlags::new(), addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                // Aggregate: return pointer to slot location.
                // The value stays in place, we just track the pointer.
                let addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, slot_offset as i32);
                self.values.insert(dest, addr);
            }
        }

        Ok(())
    }

    /// Get a pointer to an operand's value.
    ///
    /// For aggregates already in memory, returns the pointer directly.
    /// For scalars in registers, spills to a temporary stack location.
    fn get_operand_ptr(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<cl_ir::Value, AotError> {
        let ty = self.get_operand_type(operand)?;
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
                        AotError::Codegen("no frame slot for value spill".into())
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

    /// Compile a constant instruction.
    fn compile_const(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        value: &ConstValue,
    ) -> Result<(), AotError> {
        let cl_val = match value {
            ConstValue::Unit => {
                // Unit is zero-sized, no actual value needed.
                // We'll use a dummy i8 value.
                builder.ins().iconst(cl_types::I8, 0)
            }
            ConstValue::Bool(b) => {
                builder.ins().iconst(cl_types::I8, *b as i64)
            }
            ConstValue::U8(v) => {
                builder.ins().iconst(cl_types::I8, *v as i64)
            }
            ConstValue::U16(v) => {
                builder.ins().iconst(cl_types::I16, *v as i64)
            }
            ConstValue::U32(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            ConstValue::U64(v) => {
                builder.ins().iconst(cl_types::I64, *v as i64)
            }
            ConstValue::I8(v) => {
                builder.ins().iconst(cl_types::I8, *v as i64)
            }
            ConstValue::I16(v) => {
                builder.ins().iconst(cl_types::I16, *v as i64)
            }
            ConstValue::I32(v) => {
                builder.ins().iconst(cl_types::I32, *v as i64)
            }
            ConstValue::I64(v) => {
                builder.ins().iconst(cl_types::I64, *v)
            }
            ConstValue::F32(v) => {
                builder.ins().f32const(*v)
            }
            ConstValue::Int { .. } | ConstValue::String(_) => {
                // Runtime types need runtime calls - phase 6.
                return Err(AotError::Unsupported(format!(
                    "runtime constant not yet implemented: {:?}",
                    value
                )));
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile a binary operation.
    fn compile_binop(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<(), AotError> {
        // Get the type of the result to determine how to compile.
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Check for runtime types that need special handling.
        if matches!(dest_ty, IrType::Int) {
            return Err(AotError::Unsupported(format!(
                "BinOp with Int (bigint) result type - requires runtime call: {:?}",
                op
            )));
        }

        let lhs_val = self.get_operand_value(builder, lhs)?;
        let rhs_val = self.get_operand_value(builder, rhs)?;

        let is_signed = matches!(dest_ty, IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64);
        let is_float = matches!(dest_ty, IrType::F32);

        let cl_val = if is_float {
            match op {
                BinOp::Add => builder.ins().fadd(lhs_val, rhs_val),
                BinOp::Sub => builder.ins().fsub(lhs_val, rhs_val),
                BinOp::Mul => builder.ins().fmul(lhs_val, rhs_val),
                BinOp::Div => builder.ins().fdiv(lhs_val, rhs_val),
                BinOp::Eq => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::Equal, lhs_val, rhs_val)
                }
                BinOp::Ne => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::NotEqual, lhs_val, rhs_val)
                }
                BinOp::Lt => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::LessThan, lhs_val, rhs_val)
                }
                BinOp::Le => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::LessThanOrEqual, lhs_val, rhs_val)
                }
                BinOp::Gt => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::GreaterThan, lhs_val, rhs_val)
                }
                BinOp::Ge => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::GreaterThanOrEqual, lhs_val, rhs_val)
                }
                _ => {
                    return Err(AotError::Unsupported(format!(
                        "float binop not supported: {:?}",
                        op
                    )));
                }
            }
        } else {
            match op {
                BinOp::Add => builder.ins().iadd(lhs_val, rhs_val),
                BinOp::Sub => builder.ins().isub(lhs_val, rhs_val),
                BinOp::Mul => builder.ins().imul(lhs_val, rhs_val),
                BinOp::Div => {
                    if is_signed {
                        builder.ins().sdiv(lhs_val, rhs_val)
                    } else {
                        builder.ins().udiv(lhs_val, rhs_val)
                    }
                }
                BinOp::Mod => {
                    if is_signed {
                        builder.ins().srem(lhs_val, rhs_val)
                    } else {
                        builder.ins().urem(lhs_val, rhs_val)
                    }
                }
                BinOp::Eq => {
                    builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, lhs_val, rhs_val)
                }
                BinOp::Ne => {
                    builder.ins().icmp(cl_ir::condcodes::IntCC::NotEqual, lhs_val, rhs_val)
                }
                BinOp::Lt => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedLessThan
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedLessThan
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::Le => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedLessThanOrEqual
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedLessThanOrEqual
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::Gt => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedGreaterThan
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedGreaterThan
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::Ge => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedGreaterThanOrEqual
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedGreaterThanOrEqual
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::And => {
                    // Logical AND - both operands are booleans.
                    builder.ins().band(lhs_val, rhs_val)
                }
                BinOp::Or => {
                    // Logical OR.
                    builder.ins().bor(lhs_val, rhs_val)
                }
                BinOp::BitAnd => builder.ins().band(lhs_val, rhs_val),
                BinOp::BitOr => builder.ins().bor(lhs_val, rhs_val),
                BinOp::BitXor => builder.ins().bxor(lhs_val, rhs_val),
                BinOp::Shl => builder.ins().ishl(lhs_val, rhs_val),
                BinOp::Shr => {
                    if is_signed {
                        builder.ins().sshr(lhs_val, rhs_val)
                    } else {
                        builder.ins().ushr(lhs_val, rhs_val)
                    }
                }
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile a unary operation.
    fn compile_unaryop(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        op: UnaryOp,
        operand: &Operand,
    ) -> Result<(), AotError> {
        let val = self.get_operand_value(builder, operand)?;
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let is_float = matches!(dest_ty, IrType::F32);

        let cl_val = match op {
            UnaryOp::Neg => {
                if is_float {
                    builder.ins().fneg(val)
                } else {
                    builder.ins().ineg(val)
                }
            }
            UnaryOp::Not => {
                // Logical NOT on boolean (i8).
                let one = builder.ins().iconst(cl_types::I8, 1);
                builder.ins().bxor(val, one)
            }
            UnaryOp::BitNot => {
                builder.ins().bnot(val)
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile a copy instruction.
    fn compile_copy(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), AotError> {
        let val = self.get_operand_value(builder, src)?;
        self.values.insert(dest, val);
        Ok(())
    }

    /// Compile a pack instruction (tuple/struct creation).
    fn compile_pack(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        fields: &[Operand],
    ) -> Result<(), AotError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let repr = types::ir_type_to_cranelift(dest_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple that fits in a register.
                if fields.len() == 1 {
                    let val = self.get_operand_value(builder, &fields[0])?;
                    self.values.insert(dest, val);
                } else {
                    return Err(AotError::Unsupported(
                        "scalar pack with multiple fields".into()
                    ));
                }
            }
            CraneliftRepr::Aggregate(_layout) => {
                // Allocate in frame and store fields.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    AotError::Codegen("no frame slot for aggregate".into())
                })?;

                let dest_offset = self.layout.value_offset(dest.0);
                let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                // Get field offsets.
                let field_types: Vec<_> = match dest_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "pack for non-tuple/struct: {:?}",
                            dest_ty
                        )));
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, field_op) in fields.iter().enumerate() {
                    let field_val = self.get_operand_value(builder, field_op)?;
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(_) => {
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            builder.ins().store(MemFlags::new(), field_val, addr, 0);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // TODO: memcpy for aggregate fields.
                            return Err(AotError::Unsupported(
                                "aggregate field in pack".into()
                            ));
                        }
                    }
                }

                // For aggregates, we store a pointer to the frame location.
                self.values.insert(dest, base);
            }
        }

        Ok(())
    }

    /// Compile an unpack instruction (tuple/struct destructuring).
    fn compile_unpack(
        &mut self,
        builder: &mut FunctionBuilder,
        dests: &[ValueId],
        src: &Operand,
    ) -> Result<(), AotError> {
        // Get source type from first dest's expected type.
        // Actually we need the source operand's type.
        let src_ty = self.get_operand_type(src)?;
        let repr = types::ir_type_to_cranelift(&src_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple.
                if dests.len() == 1 {
                    let val = self.get_operand_value(builder, src)?;
                    self.values.insert(dests[0], val);
                } else {
                    return Err(AotError::Unsupported(
                        "scalar unpack with multiple dests".into()
                    ));
                }
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; load fields.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "unpack from non-tuple/struct: {:?}",
                            src_ty
                        )));
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, &dest) in dests.iter().enumerate() {
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(field_cl_ty) => {
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            let val = builder.ins().load(field_cl_ty, MemFlags::new(), addr, 0);
                            self.values.insert(dest, val);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Return pointer to field.
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            self.values.insert(dest, addr);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Compile a terminator.
    fn compile_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
    ) -> Result<(), AotError> {
        match term {
            Terminator::Goto(target) => {
                let block = self.blocks[target];
                builder.ins().jump(block, &[]);
            }
            Terminator::Branch { cond, then_block, else_block } => {
                let cond_val = self.get_operand_value(builder, cond)?;
                let then_blk = self.blocks[then_block];
                let else_blk = self.blocks[else_block];
                builder.ins().brif(cond_val, then_blk, &[], else_blk, &[]);
            }
            Terminator::Return { value } => {
                if let Some(val_op) = value {
                    let val = self.get_operand_value(builder, val_op)?;
                    builder.ins().return_(&[val]);
                } else {
                    builder.ins().return_(&[]);
                }
            }
            Terminator::TryReturn { .. } | Terminator::UnitEnd { .. } | Terminator::UnitEarlyReturn { .. } => {
                return Err(AotError::Unsupported(format!(
                    "terminator not yet implemented: {:?}",
                    term
                )));
            }
        }
        Ok(())
    }

    /// Get a Cranelift value for an operand.
    fn get_operand_value(
        &self,
        builder: &mut FunctionBuilder,
        op: &Operand,
    ) -> Result<cl_ir::Value, AotError> {
        match op {
            Operand::Value(vid) => {
                self.values.get(vid).copied().ok_or_else(|| {
                    AotError::Codegen(format!("undefined value: {:?}", vid))
                })
            }
            Operand::Param(pid) => {
                // User params are stored in param_values (rt_handle is separate).
                self.param_values.get(pid).copied().ok_or_else(|| {
                    AotError::Codegen(format!("undefined param: {:?}", pid))
                })
            }
            Operand::Slot(slot_id) => {
                // Load value from slot in frame.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    AotError::Codegen("no frame slot for slot operand".into())
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
                Err(AotError::Unsupported(format!(
                    "operand type not yet implemented: {:?}",
                    op
                )))
            }
        }
    }

    /// Get the type of an operand.
    fn get_operand_type(&self, op: &Operand) -> Result<IrType, AotError> {
        match op {
            Operand::Value(vid) => {
                Ok(self.func.value_types[vid.0 as usize].clone())
            }
            Operand::Param(pid) => {
                Ok(self.func.param_types[pid.0 as usize].clone())
            }
            Operand::Slot(sid) => {
                Ok(self.func.slot_types[sid.0 as usize].clone())
            }
            _ => Err(AotError::Unsupported(format!(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_object::{ObjectBuilder, ObjectModule};
    use datalove_datafun_ir::{IrBlock, IrFunction, FuncId as IrFuncId};

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

    #[test]
    fn test_compile_const_i32() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { 42 }
        let func = IrFunction {
            id: IrFuncId(0),
            name: "test_const".into(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
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
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I32],
            slot_types: vec![],
        };

        let compiler = FunctionCompiler::new(&func, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_binop_add() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { 10 + 32 }
        let func = IrFunction {
            id: IrFuncId(0),
            name: "test_add".into(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
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
                            op: BinOp::Add,
                            lhs: Operand::Value(ValueId(0)),
                            rhs: Operand::Value(ValueId(1)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(2))),
                    },
                },
            ],
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I32, IrType::I32, IrType::I32],
            slot_types: vec![],
        };

        let compiler = FunctionCompiler::new(&func, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_comparison() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> bool { 10 < 32 }
        let func = IrFunction {
            id: IrFuncId(0),
            name: "test_cmp".into(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
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
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I32, IrType::I32, IrType::Bool],
            slot_types: vec![],
        };

        let compiler = FunctionCompiler::new(&func, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }

    #[test]
    fn test_compile_unary_neg() {
        let isa = create_test_isa();
        let mut module = create_test_module(isa.clone());

        // Create a function: fn foo() -> i32 { -42 }
        let func = IrFunction {
            id: IrFuncId(0),
            name: "test_neg".into(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::I32(42),
                        },
                        Instruction::UnaryOp {
                            dest: ValueId(1),
                            op: UnaryOp::Neg,
                            operand: Operand::Value(ValueId(0)),
                        },
                    ],
                    terminator: Terminator::Return {
                        value: Some(Operand::Value(ValueId(1))),
                    },
                },
            ],
            value_count: 2,
            slot_count: 0,
            value_types: vec![IrType::I32, IrType::I32],
            slot_types: vec![],
        };

        let compiler = FunctionCompiler::new(&func, isa.as_ref(), &mut module);
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
        let func = IrFunction {
            id: IrFuncId(0),
            name: "test_branch".into(),
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            blocks: vec![
                IrBlock {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::Const {
                            dest: ValueId(0),
                            value: ConstValue::Bool(true),
                        },
                    ],
                    terminator: Terminator::Branch {
                        cond: Operand::Value(ValueId(0)),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                IrBlock {
                    id: BlockId(1),
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
            value_types: vec![IrType::Bool, IrType::I32, IrType::I32],
            slot_types: vec![],
        };

        let compiler = FunctionCompiler::new(&func, isa.as_ref(), &mut module);
        let result = compiler.compile();
        assert!(result.is_ok(), "compile failed: {:?}", result.err());
    }
}
