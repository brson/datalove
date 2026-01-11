//! IR interpreter with frame-based execution.
//!
//! # Architecture
//!
//! The interpreter executes IR instructions using a frame-based memory model:
//!
//! - **Frame**: Flat byte buffer (`Vec<u8>`) holding all values and slots for a
//!   function call or script unit. Layout computed from type information.
//!
//! - **Value/Destination**: Pointer + type descriptor pairs. `Value` for reading,
//!   `Destination` for writing. Type descriptors (`TyDesc`) provide size, alignment,
//!   and type-specific metadata for runtime operations.
//!
//! - **Linear semantics**: Non-copy types (Int, String, List, etc.) use move semantics.
//!   Moved values are marked dropped to prevent double-free. Copy types use shallow copy.
//!
//! # Execution Modes
//!
//! **Functions**: Called via `call_with_env()` or `call_in_context()`. Arguments moved
//! into parameter slots, return value moved to caller-provided destination. Frame
//! destroyed after return.
//!
//! **Script units**: Executed via `execute_script_unit_in_env()`. Frames persist in
//! `FrameStore` for subsequent units to access via `ExternalValue`/`ExternalSlot`
//! operands. Supports early return via `!` and `?` operators.
//!
//! # Environment
//!
//! - `FunctionRegistry`: Stores functions from modules and previous script units.
//! - `FrameStore`: Stores frames from previous units for external value access.
//! - `ScriptEnvironment`: Combines registry and frame store for script execution.
//! - `ExecutionContext`: Local functions available during execution.
//!
//! # Runtime Integration
//!
//! All memory operations go through `datalove-rt`: allocation, deallocation, deep
//! copy, comparison, and pretty-printing. Type descriptors are constructed by
//! `IrTyDescTable` from `IrType` definitions.

mod error;
mod value;
mod layout;
mod tydesc;
mod frame;
mod env;
mod ops;
mod types;
mod collections;
mod dispatch;

#[cfg(test)]
mod tests;

pub use error::InterpError;
pub use value::{Value, Destination};
pub use layout::IrLayout;
pub use tydesc::IrTyDescTable;
pub use frame::{Frame, FrameStore};
pub use env::{FunctionRegistry, ScriptEnvironment, ExecutionContext};
pub use dispatch::{CallDispatcher, DispatchCallContext, DispatchResult};
pub use datalove_rt::c::DebugOutputMode;

use std::cell::RefCell;

use datalove_rt::rtdt;
use datalove_datafun_ir::{
    IrFunction, IrScriptUnit, IrBlock, IrType, Instruction, Terminator,
    BlockId, Operand, SlotDest, ConstValue, ParamMode,
};

/// Result of executing a script unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCompletion {
    /// Normal completion (fragment: no value; expr: value in expr_dest).
    Normal,
    /// Early return via `!` or `?` operator (Result<(), Error> written to ret_dest).
    EarlyReturn,
}

/// IR function interpreter.
pub struct IrInterpreter {
    runtime: datalove_rt::rust::Runtime,
    tydesc_table: IrTyDescTable,
    /// Optional call dispatcher for JIT integration.
    /// Uses RefCell to allow passing &mut self to dispatch_call.
    call_dispatcher: RefCell<Option<Box<dyn CallDispatcher>>>,
}

impl IrInterpreter {
    /// Create a new interpreter with default settings (debug output disabled).
    pub fn new() -> Self {
        Self::new_with_debug_mode(datalove_rt::c::DebugOutputMode::Disabled)
    }

    /// Create a new interpreter with the specified debug output mode.
    pub fn new_with_debug_mode(debug_mode: datalove_rt::c::DebugOutputMode) -> Self {
        Self {
            runtime: datalove_rt::rust::Runtime::new_with_debug_mode(debug_mode),
            tydesc_table: IrTyDescTable::new(),
            call_dispatcher: RefCell::new(None),
        }
    }

    /// Set a call dispatcher for intercepting function calls.
    ///
    /// Use this to integrate JIT compilation or other call dispatch mechanisms.
    pub fn set_call_dispatcher(&mut self, dispatcher: Box<dyn CallDispatcher>) {
        *self.call_dispatcher.borrow_mut() = Some(dispatcher);
    }

    /// Remove the call dispatcher.
    pub fn clear_call_dispatcher(&mut self) {
        *self.call_dispatcher.borrow_mut() = None;
    }

    /// Get the runtime handle for memory management.
    pub fn runtime_handle(&self) -> datalove_rt::c::LocalRtHandle {
        self.runtime.handle()
    }

    /// Get mutable access to the type descriptor table.
    pub fn tydesc_table_mut(&mut self) -> &mut IrTyDescTable {
        &mut self.tydesc_table
    }

    /// Get the contents of the debug buffer.
    ///
    /// Returns the accumulated debug output as a string.
    pub fn get_debug_buffer(&self) -> String {
        unsafe {
            let mut ptr: *const u8 = std::ptr::null();
            let mut len: usize = 0;
            let status = datalove_rt::c::dtlv_rti_get_debug_buffer(
                self.runtime.handle(),
                &mut ptr,
                &mut len,
            );
            if status != datalove_rt::c::RtStatus::Ok || ptr.is_null() || len == 0 {
                return String::new();
            }
            let bytes = std::slice::from_raw_parts(ptr, len);
            String::from_utf8_lossy(bytes).to_string()
        }
    }

    /// Clear the debug buffer.
    pub fn clear_debug_buffer(&self) {
        unsafe {
            datalove_rt::c::dtlv_rti_clear_debug_buffer(self.runtime.handle());
        }
    }

    /// Pretty-print a value using the runtime's pretty printer.
    pub fn pretty_print_value(&mut self, value: &Value) -> Result<String, InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let string_tydesc = self.tydesc_table.get_or_create(&IrType::String);

        unsafe {
            // Create output string.
            let mut output_string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let status = datalove_rt::c::dtlv_rti_string_create_local(
                rt_handle,
                output_string.as_mut_ptr() as *mut u8,
                string_tydesc,
            );

            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to create output string".to_string(),
                ));
            }

            let mut output_string = output_string.assume_init();

            // Pretty-print the value.
            let status = datalove_rt::c::dtlv_rti_pretty_print_local(
                rt_handle,
                value.ptr,
                value.tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                string_tydesc,
            );

            if status != RtStatus::Ok {
                datalove_rt::c::dtlv_rti_string_destroy_local(
                    rt_handle,
                    &mut output_string as *mut rtdt::String as *mut u8,
                    string_tydesc,
                );
                return Err(InterpError::RuntimeError(
                    "Failed to pretty-print value".to_string(),
                ));
            }

            // Extract string contents.
            let result = if output_string.data.is_null() || output_string.size == 0 {
                String::new()
            } else {
                let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
                String::from_utf8_lossy(bytes).to_string()
            };

            // Destroy the output string.
            datalove_rt::c::dtlv_rti_string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                string_tydesc,
            );

            Ok(result)
        }
    }

    /// Destroy a value, freeing any associated allocations.
    pub fn destroy_value(&mut self, value: &Value) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();

        unsafe {
            let status = datalove_rt::c::dtlv_rti_any_destroy_local(
                rt_handle,
                value.ptr,
                value.tydesc,
            );

            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to destroy value".to_string(),
                ));
            }

            Ok(())
        }
    }

    /// Execute a function with arguments using a script environment.
    ///
    /// This allows the function to call other functions registered in the environment.
    pub fn call_with_env(
        &mut self,
        func: &IrFunction,
        args: Vec<Value>,
        ret_dest: Destination,
        env: &ScriptEnvironment,
    ) -> Result<(), InterpError> {
        // Create an empty context (module functions are resolved via registry, not local context).
        let ctx = ExecutionContext::new(&[]);
        // Use the environment's registry but create fresh frames (function execution
        // doesn't persist frames like script units do).
        let mut frames = FrameStore::new();
        self.call_in_context(func, args, ret_dest, &ctx, &env.registry, &mut frames)
    }

    /// Execute a function with arguments in a context with available functions.
    pub fn call_in_context(
        &mut self,
        func: &IrFunction,
        args: Vec<Value>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        self.call_in_context_impl(func, args, ret_dest, ctx, registry, frames, false)
    }

    /// Execute a function called from JIT code via the trampoline.
    ///
    /// All arguments are treated as borrowed because the JIT caller is responsible
    /// for its own frame values. The JIT calling convention doesn't have the same
    /// ownership transfer semantics as interpreter-to-interpreter calls.
    pub fn call_in_context_jit(
        &mut self,
        func: &IrFunction,
        args: Vec<Value>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        self.call_in_context_impl(func, args, ret_dest, ctx, registry, frames, true)
    }

    /// Implementation for call_in_context and call_in_context_jit.
    fn call_in_context_impl(
        &mut self,
        func: &IrFunction,
        args: Vec<Value>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        jit_caller: bool,
    ) -> Result<(), InterpError> {
        // Compute layout.
        let layout = IrLayout::compute(
            &func.value_types,
            &func.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame with param storage.
        let mut frame = Frame::new(layout, func.params.len());

        // Set up parameters as pointers to caller's data.
        // All params store pointers - mode determines ownership semantics.
        for (i, &param_id) in func.params.iter().enumerate() {
            if i < args.len() {
                let src = &args[i];
                let mode = func.param_modes.get(i).copied().unwrap_or(ParamMode::In);

                // Get tydesc for this param from param_types.
                let param_type = &func.param_types[i];
                let tydesc = self.tydesc_table.get_or_create(param_type);

                // Borrowed semantics (caller retains ownership, callee doesn't destroy):
                // - Ref/Mut/Out modes: caller retains ownership
                // - Copy types with In mode: callee makes a copy, caller retains original
                // - JIT caller: all params borrowed (JIT handles its own frame)
                // Non-borrowed (callee destroys):
                // - Non-Copy types with In mode: ownership transfers to callee
                let borrowed = jit_caller
                    || matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out)
                    || param_type.is_copy();

                // Initialized: true for In/Ref/Mut (data exists), false for Out (callee writes first).
                let initialized = !matches!(mode, ParamMode::Out);

                frame.set_param(param_id, src.ptr, tydesc, borrowed, initialized);
            }
        }

        // Execute blocks, writing return value directly to ret_dest.
        // Functions use ret_dest for Return, not expr_dest.
        let result = self.execute_blocks(&func.blocks, &mut frame, ret_dest, None, ctx, registry, frames);

        // Destroy remaining values in frame.
        // This destroys In params (ownership transferred from caller).
        // For JIT callers, params are borrowed so they won't be destroyed here.
        frame.destroy_all(self.runtime.handle());

        // Convert UnitCompletion to () - functions always complete normally.
        result.map(|_| ())
    }

    /// Execute a script unit with access to previous units' values.
    ///
    /// After execution, the unit's frame and functions are added to the environment
    /// for subsequent units to reference.
    ///
    /// Returns `UnitCompletion::Normal` for regular completion, or
    /// `UnitCompletion::EarlyReturn` if `!` or `?` triggered early return.
    ///
    /// - `ret_dest`: Destination for early return (always `Result<(), Error>` type)
    /// - `expr_dest`: Destination for expression result (for expr units, `None` for fragments)
    pub fn execute_script_unit_in_env(
        &mut self,
        unit: &IrScriptUnit,
        env: &mut ScriptEnvironment,
        ret_dest: Destination,
        expr_dest: Option<Destination>,
    ) -> Result<UnitCompletion, InterpError> {
        // Compute layout.
        let layout = IrLayout::compute(
            &unit.value_types,
            &unit.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame (script units have no function params).
        let mut frame = Frame::new(layout, 0);

        // Create execution context with local functions.
        let ctx = ExecutionContext::new(&unit.functions);

        // Execute blocks with registry for function lookups and frames for slot access.
        let result = self.execute_blocks(
            &unit.blocks,
            &mut frame,
            ret_dest,
            expr_dest,
            &ctx,
            &env.registry,
            &mut env.frames,
        );

        // On error, destroy the frame and propagate the error.
        if let Err(e) = result {
            frame.destroy_all(self.runtime.handle());
            return Err(e);
        }

        // Add this unit's frame and functions to the environment for future units.
        env.add_unit(frame, unit.functions.clone());

        result
    }

    fn execute_blocks(
        &mut self,
        blocks: &[IrBlock],
        frame: &mut Frame,
        ret_dest: Destination,
        expr_dest: Option<Destination>,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<UnitCompletion, InterpError> {
        let mut current_block = BlockId(0);

        loop {
            let block = blocks.iter()
                .find(|b| b.id == current_block)
                .ok_or(InterpError::BlockNotFound(current_block))?;

            // Execute instructions.
            for instr in &block.instructions {
                self.execute_instruction(instr, frame, ret_dest, ctx, registry, frames)?;
            }

            // Handle terminator.
            match &block.terminator {
                Terminator::Goto { target, args } => {
                    // Pass block arguments to target block.
                    self.pass_block_args(blocks, *target, args, frame, frames)?;
                    current_block = *target;
                }
                Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                    let cond_val = self.read_operand(cond, frame, frames)?;
                    let cond_bool = unsafe { *(cond_val.ptr as *const bool) };
                    if cond_bool {
                        self.pass_block_args(blocks, *then_block, then_args, frame, frames)?;
                        current_block = *then_block;
                    } else {
                        self.pass_block_args(blocks, *else_block, else_args, frame, frames)?;
                        current_block = *else_block;
                    }
                }
                Terminator::Return { value } => {
                    if let Some(op) = value {
                        let val = self.read_operand(op, frame, frames)?;
                        // Use move_value (shallow copy). The frame will be
                        // destroyed by call_in_context, so we must transfer
                        // ownership to avoid double-free.
                        unsafe { self.move_value(&val, ret_dest)?; }
                        // Mark source as dropped to prevent destroy in frame.destroy_all().
                        match op {
                            Operand::Value(id) => frame.mark_value_dropped(*id),
                            Operand::Slot(id) => frame.mark_slot_dropped(*id),
                            Operand::Param(id) => frame.mark_param_dropped(*id),
                            Operand::ExternalValue { unit, value } => {
                                frames.mark_external_value_dropped(*unit, *value);
                            }
                            Operand::ExternalSlot { unit, slot } => {
                                frames.mark_external_slot_dropped(*unit, *slot);
                            }
                        }
                    }
                    return Ok(UnitCompletion::Normal);
                }
                Terminator::UnitEnd { result } => {
                    if let Some(op) = result {
                        let val = self.read_operand(op, frame, frames)?;
                        // Write to expr_dest (not ret_dest) for expression results.
                        let dest = expr_dest.expect("UnitEnd with result requires expr_dest");
                        unsafe { self.move_value(&val, dest)?; }
                        match op {
                            Operand::Value(id) => frame.mark_value_dropped(*id),
                            Operand::Slot(id) => frame.mark_slot_dropped(*id),
                            Operand::Param(id) => frame.mark_param_dropped(*id),
                            Operand::ExternalValue { unit, value } => {
                                frames.mark_external_value_dropped(*unit, *value);
                            }
                            Operand::ExternalSlot { unit, slot } => {
                                frames.mark_external_slot_dropped(*unit, *slot);
                            }
                        }
                    }
                    return Ok(UnitCompletion::Normal);
                }
                Terminator::UnitEarlyReturn { value } => {
                    let val = self.read_operand(value, frame, frames)?;
                    // Debuglog the value (borrow, not consume).
                    let rt_handle = self.runtime.handle();
                    unsafe {
                        let status = datalove_rt::c::dtlv_rti_debuglog_local(
                            rt_handle,
                            val.ptr,
                            val.tydesc,
                        );
                        if status != datalove_rt::c::RtStatus::Ok {
                            return Err(InterpError::RuntimeError(
                                "debuglog failed".to_string(),
                            ));
                        }
                    }
                    // Write to ret_dest (Result<(), Error> type).
                    unsafe { self.move_value(&val, ret_dest)?; }
                    match value {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { unit, value } => {
                            frames.mark_external_value_dropped(*unit, *value);
                        }
                        Operand::ExternalSlot { unit, slot } => {
                            frames.mark_external_slot_dropped(*unit, *slot);
                        }
                    }
                    return Ok(UnitCompletion::EarlyReturn);
                }
            }
        }
    }

    /// Pass block arguments to the target block's parameters.
    ///
    /// Implements move semantics for block parameters:
    /// 1. Read source operand value
    /// 2. Copy data into the target block param's fixed frame location
    /// 3. Mark source as dropped (ownership transferred)
    ///
    /// Each block param has a pre-allocated frame location. This function
    /// moves values INTO those locations - the previous contents are overwritten.
    fn pass_block_args(
        &mut self,
        blocks: &[IrBlock],
        target: BlockId,
        args: &[Operand],
        frame: &mut Frame,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        if args.is_empty() {
            return Ok(());
        }

        // Find the target block to get its parameters.
        let target_block = blocks.iter()
            .find(|b| b.id == target)
            .ok_or(InterpError::BlockNotFound(target))?;

        // Pass each argument to the corresponding block parameter.
        for (param_id, arg) in target_block.params.iter().zip(args.iter()) {
            let src_val = self.read_operand(arg, frame, frames)?;
            let dest_slot = frame.value_dest(*param_id)?;
            // Block args use move semantics.
            unsafe { self.move_value(&src_val, dest_slot)?; }
            frame.mark_value_initialized(*param_id);

            // Mark source as dropped.
            match arg {
                Operand::Value(id) => frame.mark_value_dropped(*id),
                Operand::Slot(id) => frame.mark_slot_dropped(*id),
                Operand::Param(id) => frame.mark_param_dropped(*id),
                Operand::ExternalValue { unit, value } => {
                    frames.mark_external_value_dropped(*unit, *value);
                }
                Operand::ExternalSlot { unit, slot } => {
                    frames.mark_external_slot_dropped(*unit, *slot);
                }
            }
        }
        Ok(())
    }

    fn execute_instruction(
        &mut self,
        instr: &Instruction,
        frame: &mut Frame,
        _ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        match instr {
            Instruction::Const { dest, value } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.write_const(value, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::Copy { dest, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                unsafe { self.copy_value(&src_val, dest_slot)?; }
                frame.mark_value_initialized(*dest);
            }
            Instruction::Move { dest, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                unsafe { self.move_value(&src_val, dest_slot)?; }
                frame.mark_value_initialized(*dest);
                // Mark source as dropped to prevent double-free.
                match src {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { unit, value } => {
                        frames.mark_external_value_dropped(*unit, *value);
                    }
                    Operand::ExternalSlot { unit, slot } => {
                        frames.mark_external_slot_dropped(*unit, *slot);
                    }
                }
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                let lhs_val = self.read_operand(lhs, frame, frames)?;
                let rhs_val = self.read_operand(rhs, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_binop(*op, &lhs_val, &rhs_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::UnaryOp { dest, op, operand } => {
                let src_val = self.read_operand(operand, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_unaryop(*op, &src_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::SlotStore { dest, value } => {
                let src_val = self.read_operand(value, frame, frames)?;
                match dest {
                    SlotDest::Local(slot_id) => {
                        // Destroy old value if slot was already initialized.
                        if frame.is_slot_initialized(*slot_id) {
                            let old_val = frame.slot(*slot_id)?;
                            unsafe {
                                datalove_rt::c::dtlv_rti_any_destroy_local(
                                    self.runtime.handle(),
                                    old_val.ptr,
                                    old_val.tydesc,
                                );
                            }
                        }
                        let dest_slot = frame.slot_dest(*slot_id)?;
                        // Move value into slot (consumes source).
                        unsafe { self.move_value(&src_val, dest_slot)?; }
                        frame.mark_slot_initialized(*slot_id);
                        // Mark source as dropped.
                        match value {
                            Operand::Value(id) => frame.mark_value_dropped(*id),
                            Operand::Slot(id) => frame.mark_slot_dropped(*id),
                            Operand::Param(id) => frame.mark_param_dropped(*id),
                            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                        }
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            self.runtime.handle(),
                            *unit,
                            *slot,
                            &src_val,
                        )?;
                        // Mark source as dropped for external store too.
                        match value {
                            Operand::Value(id) => frame.mark_value_dropped(*id),
                            Operand::Slot(id) => frame.mark_slot_dropped(*id),
                            Operand::Param(id) => frame.mark_param_dropped(*id),
                            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                        }
                    }
                }
            }
            Instruction::ParamStore { param, value } => {
                let src_val = self.read_operand(value, frame, frames)?;
                // Get destination pointer from param (points to caller's data).
                let dest_ptr = frame.param_dest(*param)?;
                // Destroy old value at destination only if initialized.
                // (Out params start uninitialized - first write doesn't destroy.)
                if frame.is_param_initialized(*param) {
                    unsafe {
                        datalove_rt::c::dtlv_rti_any_destroy_local(
                            self.runtime.handle(),
                            dest_ptr.ptr,
                            dest_ptr.tydesc,
                        );
                    }
                }
                // Move new value into destination.
                unsafe { self.move_value(&src_val, dest_ptr)?; }
                // Mark param as initialized (important for Out params).
                frame.mark_param_initialized(*param);
                // Mark source as dropped.
                match value {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::SlotLoad { dest, slot } => {
                let slot_val = frame.slot(*slot)?;
                let dest_slot = frame.value_dest(*dest)?;
                // Check if type is Copy (primitive scalars).
                let tag = unsafe { (*slot_val.tydesc).type_tag };
                let is_copy = matches!(
                    tag,
                    rtdt::TyTag::Bool
                        | rtdt::TyTag::U8
                        | rtdt::TyTag::I8
                        | rtdt::TyTag::U16
                        | rtdt::TyTag::I16
                        | rtdt::TyTag::U32
                        | rtdt::TyTag::I32
                        | rtdt::TyTag::U64
                        | rtdt::TyTag::I64
                        | rtdt::TyTag::F32
                        | rtdt::TyTag::F64
                );
                if is_copy {
                    // Copy types: just copy the bytes, slot remains valid.
                    unsafe { self.copy_value(&slot_val, dest_slot)?; }
                } else {
                    // Non-copy types: move value out, slot becomes invalid.
                    unsafe { self.move_value(&slot_val, dest_slot)?; }
                    frame.mark_slot_dropped(*slot);
                }
                frame.mark_value_initialized(*dest);
            }
            Instruction::Pack { dest, ty: _, fields } => {
                let field_vals: Vec<Value> = fields.iter()
                    .map(|op| self.read_operand(op, frame, frames))
                    .collect::<Result<_, _>>()?;
                let dest_slot = frame.value_dest(*dest)?;
                // Check type tag to determine if tuple or struct.
                let tag = unsafe { (*dest_slot.tydesc).type_tag };
                match tag {
                    rtdt::TyTag::Tuple => self.execute_pack_tuple(&field_vals, dest_slot)?,
                    rtdt::TyTag::Struct => self.execute_pack_struct(&field_vals, dest_slot)?,
                    _ => return Err(InterpError::TypeMismatch(
                        format!("Pack requires tuple or struct type, got {:?}", tag)
                    )),
                }
                frame.mark_value_initialized(*dest);
                // Mark source fields as moved (linear semantics - consumes fields).
                for field in fields {
                    match field {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }
            }
            Instruction::Unpack { dests, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let tag = unsafe { (*src_val.tydesc).type_tag };
                match tag {
                    rtdt::TyTag::Tuple => {
                        let tuple_info = unsafe { (*src_val.tydesc).type_info.tuple };
                        for (i, &dest_id) in dests.iter().enumerate() {
                            let dest_slot = frame.value_dest(dest_id)?;
                            let field_info = unsafe { &*tuple_info.fields.add(i) };
                            let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                            let size = unsafe { (*field_info.tydesc).size as usize };
                            unsafe { std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, size); }
                            frame.mark_value_initialized(dest_id);
                        }
                    }
                    rtdt::TyTag::Struct => {
                        let struct_info = unsafe { (*src_val.tydesc).type_info.struct_ };
                        for (i, &dest_id) in dests.iter().enumerate() {
                            let dest_slot = frame.value_dest(dest_id)?;
                            let field_info = unsafe { &*struct_info.fields.add(i) };
                            let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                            let size = unsafe { (*field_info.tydesc).size as usize };
                            unsafe { std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, size); }
                            frame.mark_value_initialized(dest_id);
                        }
                    }
                    _ => return Err(InterpError::TypeMismatch(
                        format!("Unpack requires tuple or struct type, got {:?}", tag)
                    )),
                }
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                // Execute checked arithmetic and set overflow flag.
                let lhs_val = self.read_operand(lhs, frame, frames)?;
                let rhs_val = self.read_operand(rhs, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let overflow_slot = frame.value_dest(*overflow)?;
                self.execute_binop_checked(*op, &lhs_val, &rhs_val, dest_slot, overflow_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*overflow);
            }
            Instruction::UnaryOpChecked { dest, overflow, op, operand } => {
                // Execute checked unary op and set overflow flag.
                let operand_val = self.read_operand(operand, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let overflow_slot = frame.value_dest(*overflow)?;
                self.execute_unaryop_checked(*op, &operand_val, dest_slot, overflow_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*overflow);
            }
            Instruction::WrapSome { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_some(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
                // Mark source as moved (linear semantics - consumes inner value).
                match inner {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::WrapNone { dest } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_none(dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::EnumVariant { dest, variant_index, payload } => {
                let payload_val = payload.as_ref()
                    .map(|p| self.read_operand(p, frame, frames))
                    .transpose()?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_enum_variant(*variant_index, payload_val.as_ref(), dest_slot)?;
                frame.mark_value_initialized(*dest);
                // Mark payload source as moved if present.
                if let Some(p) = payload {
                    match p {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let is_some_slot = frame.value_dest(*is_some)?;
                self.execute_unwrap_option(&src_val, dest_slot, is_some_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*is_some);
                // Mark source as consumed only if inner type is non-copy.
                // Option<copy_type> is itself copy, so unwrapping doesn't consume it.
                let inner_is_copy = unsafe {
                    let option_info = (*src_val.tydesc).type_info.option;
                    Self::is_copy_type_tag((*option_info.inner_tydesc).type_tag)
                };
                if !inner_is_copy {
                    match src {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { unit, value } => {
                            frames.mark_external_value_dropped(*unit, *value);
                        }
                        Operand::ExternalSlot { unit, slot } => {
                            frames.mark_external_slot_dropped(*unit, *slot);
                        }
                    }
                }
            }
            Instruction::WrapOk { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_ok(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
                // Mark source as moved (linear semantics - consumes inner value).
                match inner {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::WrapErr { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_err(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
                // Mark source as moved (linear semantics - consumes inner Error).
                match inner {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let ok_slot = frame.value_dest(*ok_dest)?;
                let err_slot = frame.value_dest(*err_dest)?;
                let is_ok_slot = frame.value_dest(*is_ok)?;
                self.execute_unwrap_result(&src_val, ok_slot, err_slot, is_ok_slot)?;
                frame.mark_value_initialized(*ok_dest);
                frame.mark_value_initialized(*err_dest);
                frame.mark_value_initialized(*is_ok);
                // Mark source as consumed - Result is destructured.
                match src {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { unit, value } => {
                        frames.mark_external_value_dropped(*unit, *value);
                    }
                    Operand::ExternalSlot { unit, slot } => {
                        frames.mark_external_slot_dropped(*unit, *slot);
                    }
                }
            }
            Instruction::ErrorFrom { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_error_from(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
                // Mark source as moved (linear semantics - consumes inner).
                match inner {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::DataFrom { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_data_from(&inner_val, dest_slot)?;
                frame.mark_value_initialized(*dest);
                // Mark source as moved (linear semantics - consumes inner).
                match inner {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::Call { dest, func, args } => {
                // Look up the function and determine the correct context for the callee.
                let (callee, callee_unit) = ctx.get_function_with_context(func, registry)?;

                // Evaluate arguments.
                // For Out params: get pointer to uninitialized slot (callee will write to it).
                // For other params: read the value as before.
                let mut arg_vals: Vec<Value> = Vec::with_capacity(args.len());
                for (i, op) in args.iter().enumerate() {
                    let mode = callee.param_modes.get(i).copied().unwrap_or(ParamMode::In);
                    if mode == ParamMode::Out {
                        // Out param: get destination pointer without reading value.
                        let val = self.get_operand_dest(op, frame)?;
                        arg_vals.push(val);
                    } else {
                        // Other modes: read the value.
                        arg_vals.push(self.read_operand(op, frame, frames)?);
                    }
                }

                // Get destination for return value.
                let dest_slot = frame.value_dest(*dest)?;

                // Mark arg sources as dropped based on param mode and type.
                // With reference passing:
                // - Ref/Mut/Out params: caller retains ownership (borrowed)
                // - In params with Copy types: callee makes a copy, caller retains original
                // - In params with non-Copy types: ownership transfers to callee
                for (i, arg) in args.iter().enumerate() {
                    let mode = callee.param_modes.get(i).copied().unwrap_or(ParamMode::In);
                    if matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out) {
                        // Borrowed param: caller retains ownership. Don't mark dropped.
                        continue;
                    }
                    // For Copy types, caller retains ownership (callee makes a copy).
                    if let Some(param_type) = callee.param_types.get(i) {
                        if param_type.is_copy() {
                            continue;
                        }
                    }
                    // Non-Copy In mode: ownership transfers to callee, mark source dropped.
                    match arg {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }

                // Try dispatcher first (for JIT integration).
                let mut call_handled = false;
                {
                    // Take the dispatcher temporarily to avoid borrow conflicts.
                    let dispatcher_opt = self.call_dispatcher.borrow_mut().take();
                    if let Some(mut dispatcher) = dispatcher_opt {
                        let rt_handle = self.runtime.handle();

                        // Create dispatch context for mixed-mode execution.
                        let call_ctx = dispatch::DispatchCallContext {
                            exec_ctx: ctx,
                            registry,
                            frames,
                            interp: self,
                        };

                        match dispatcher.dispatch_call(func, callee, &arg_vals, dest_slot, rt_handle, call_ctx) {
                            dispatch::DispatchResult::Handled(result) => {
                                *self.call_dispatcher.borrow_mut() = Some(dispatcher);
                                result?;
                                call_handled = true;
                            }
                            dispatch::DispatchResult::NotHandled => {
                                *self.call_dispatcher.borrow_mut() = Some(dispatcher);
                            }
                        }
                    }
                }

                // Fall through to interpreter if not handled by dispatcher.
                if !call_handled {
                    // Call the function with appropriate context.
                    // For external functions, use the callee's unit's context.
                    // For local/module functions, use the current context.
                    if let Some(unit) = callee_unit {
                        // External function - create context with callee's unit functions.
                        let unit_funcs = registry.unit_functions(unit)
                            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
                        let callee_ctx = ExecutionContext::new(unit_funcs);
                        self.call_in_context(callee, arg_vals, dest_slot, &callee_ctx, registry, frames)?;
                    } else {
                        // Local or module function - use current context.
                        self.call_in_context(callee, arg_vals, dest_slot, ctx, registry, frames)?;
                    }
                }

                // After call returns, Out param slots are now initialized.
                for (i, arg) in args.iter().enumerate() {
                    let mode = callee.param_modes.get(i).copied().unwrap_or(ParamMode::In);
                    if mode == ParamMode::Out {
                        match arg {
                            Operand::Slot(id) => frame.mark_slot_initialized(*id),
                            // Value destinations don't need marking - they're SSA.
                            _ => {}
                        }
                    }
                }

                frame.mark_value_initialized(*dest);
            }
            Instruction::ListNew { dest, elements } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_list_new(elements, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
                // Mark source elements as moved (linear semantics - consumes elements).
                for elem in elements {
                    match elem {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }
            }
            Instruction::SetNew { dest, elements } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_set_new(elements, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
                // Mark source elements as moved (linear semantics - consumes elements).
                for elem in elements {
                    match elem {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }
            }
            Instruction::MapNew { dest, entries } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_map_new(entries, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
                // Mark source entries as moved (linear semantics - consumes entries).
                for (key, val) in entries {
                    match key {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                    match val {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }
            }
            Instruction::TensorNew { dest, shape, elements } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_tensor_new(shape, elements, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
                // Mark source elements as moved (linear semantics - consumes elements).
                for elem in elements {
                    match elem {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::Param(id) => frame.mark_param_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }
            }
            Instruction::Drop { operand } => {
                // Skip drop if already dropped (can happen with move semantics).
                let val = match self.read_operand(operand, frame, frames) {
                    Ok(v) => v,
                    Err(InterpError::UninitializedValue(_)) => {
                        // Already dropped, skip.
                        return Ok(());
                    }
                    Err(InterpError::UninitializedSlot(_)) => {
                        // Already dropped, skip.
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                };
                self.execute_drop(&val)?;
                // Mark as dropped to prevent double-destroy in destroy_all.
                match operand {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::Param(id) => frame.mark_param_dropped(*id),
                    // External values/slots are in other frames, handled separately.
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::DebugLog { operand } => {
                let val = self.read_operand(operand, frame, frames)?;
                let rt_handle = self.runtime.handle();
                unsafe {
                    let status = datalove_rt::c::dtlv_rti_debuglog_local(
                        rt_handle,
                        val.ptr,
                        val.tydesc,
                    );
                    if status != datalove_rt::c::RtStatus::Ok {
                        return Err(InterpError::RuntimeError(
                            "debuglog failed".to_string(),
                        ));
                    }
                }
                // Note: no mark_dropped - we're borrowing, not consuming.
            }
            Instruction::Widen { dest, src } => {
                // Widen a fixed-width integer to Int.
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                // Cast dest to Int buffer and call widen_to_int.
                unsafe {
                    let int_buf = &mut *(dest_slot.ptr as *mut datalove_rt::rtdt::Int);
                    self.widen_to_int(&src_val, int_buf)?;
                }
                frame.mark_value_initialized(*dest);
                // Source is borrowed (read), not consumed.
            }
            Instruction::Nop => {}
        }
        Ok(())
    }

    pub(crate) fn read_operand(
        &self,
        op: &Operand,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<Value, InterpError> {
        match op {
            Operand::Value(id) => frame.value(*id),
            Operand::Slot(id) => frame.slot(*id),
            Operand::Param(id) => frame.param(*id),
            Operand::ExternalValue { unit, value } => {
                frames.external_value(*unit, *value)
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.external_slot(*unit, *slot)
            }
        }
    }

    /// Get pointer to operand's destination without checking initialization.
    ///
    /// Used for Out params where we need to pass a pointer to an uninitialized slot.
    fn get_operand_dest(&mut self, op: &Operand, frame: &mut Frame) -> Result<Value, InterpError> {
        match op {
            Operand::Slot(id) => {
                let dest = frame.slot_dest(*id)?;
                Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc })
            }
            Operand::Value(id) => {
                let dest = frame.value_dest(*id)?;
                Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc })
            }
            _ => Err(InterpError::InvalidOutParamArg),
        }
    }

    fn write_const(&mut self, value: &ConstValue, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            match value {
                ConstValue::Unit => {}
                ConstValue::Bool(b) => {
                    *(dest.ptr as *mut bool) = *b;
                }
                ConstValue::U8(n) => {
                    *(dest.ptr as *mut u8) = *n;
                }
                ConstValue::U16(n) => {
                    *(dest.ptr as *mut u16) = *n;
                }
                ConstValue::U32(n) => {
                    *(dest.ptr as *mut u32) = *n;
                }
                ConstValue::U64(n) => {
                    *(dest.ptr as *mut u64) = *n;
                }
                ConstValue::I8(n) => {
                    *(dest.ptr as *mut i8) = *n;
                }
                ConstValue::I16(n) => {
                    *(dest.ptr as *mut i16) = *n;
                }
                ConstValue::I32(n) => {
                    *(dest.ptr as *mut i32) = *n;
                }
                ConstValue::I64(n) => {
                    *(dest.ptr as *mut i64) = *n;
                }
                ConstValue::Int { limbs, negative } => {
                    let int_ptr = dest.ptr as *mut rtdt::Int;
                    if limbs.is_empty() {
                        (*int_ptr).data = std::ptr::null();
                        (*int_ptr).size_and_sign = 0;
                        (*int_ptr).capacity = 0;
                    } else {
                        // Allocate limbs in runtime memory.
                        // Must use size=4, count=num_limbs to match the destroy code.
                        let rt_handle = self.runtime.handle();
                        let num_limbs = limbs.len() as u32;
                        let limbs_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                            rt_handle,
                            4,          // size of one limb
                            4,          // align
                            num_limbs,  // count
                        ) as *mut u32;
                        if limbs_ptr.is_null() {
                            return Err(InterpError::RuntimeError(
                                "Failed to allocate bigint limbs".to_string()
                            ));
                        }
                        for (i, &limb) in limbs.iter().enumerate() {
                            *limbs_ptr.add(i) = limb;
                        }
                        (*int_ptr).data = limbs_ptr as *const u32;
                        (*int_ptr).size_and_sign = if *negative {
                            -(limbs.len() as i32)
                        } else {
                            limbs.len() as i32
                        };
                        (*int_ptr).capacity = num_limbs;
                    }
                }
                ConstValue::F32(n) => {
                    *(dest.ptr as *mut f32) = *n;
                }
                ConstValue::F64(n) => {
                    *(dest.ptr as *mut f64) = *n;
                }
                ConstValue::String(s) => {
                    let rt_handle = self.runtime.handle();
                    let status = datalove_rt::c::dtlv_rti_string_create_local(
                        rt_handle,
                        dest.ptr,
                        dest.tydesc,
                    );
                    if status != datalove_rt::c::RtStatus::Ok {
                        return Err(InterpError::RuntimeError(
                            "Failed to create string".to_string()
                        ));
                    }
                    if !s.is_empty() {
                        let status = datalove_rt::c::dtlv_rti_string_push_bytes_local(
                            rt_handle,
                            dest.ptr,
                            dest.tydesc,
                            s.as_ptr(),
                            s.len() as u32,
                        );
                        if status != datalove_rt::c::RtStatus::Ok {
                            return Err(InterpError::RuntimeError(
                                "Failed to push string bytes".to_string()
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    unsafe fn copy_value(&self, src: &Value, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            let tag = (*src.tydesc).type_tag;

            // Assert this is a copy type - not a heap-allocated type.
            // In our linear type system, non-copy types must use move_value.
            assert!(
                !matches!(
                    tag,
                    rtdt::TyTag::Int
                        | rtdt::TyTag::String
                        | rtdt::TyTag::Data
                        | rtdt::TyTag::Error
                        | rtdt::TyTag::List
                        | rtdt::TyTag::Set
                        | rtdt::TyTag::Map
                        | rtdt::TyTag::Tensor
                ),
                "copy_value called on non-copy type: {:?}",
                tag
            );

            // Shallow copy for copyable types.
            let size = (*src.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(src.ptr, dest.ptr, size);
        }
        Ok(())
    }

    unsafe fn move_value(&self, src: &Value, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            // Move is always a shallow copy - ownership transfers to dest.
            // The source should be marked as dropped so it won't be destroyed.
            let size = (*src.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(src.ptr, dest.ptr, size);
        }
        Ok(())
    }

    /// Execute Drop: run destructor for a value.
    fn execute_drop(&mut self, val: &Value) -> Result<(), InterpError> {
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.runtime.handle(),
                val.ptr,
                val.tydesc,
            );
        }
        Ok(())
    }
}

impl Default for IrInterpreter {
    fn default() -> Self {
        Self::new()
    }
}
