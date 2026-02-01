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
mod dynamic;
mod intrinsics;
mod ctfe;

#[cfg(test)]
mod tests;

pub use error::InterpError;
pub use value::{Value, Destination};
pub use layout::IrLayout;
pub use tydesc::IrTyDescTable;
pub use frame::{Frame, FrameStore};
pub use env::{FunctionRegistry, ModuleFunctionRegistry, UnitFunctionRegistry, ScriptEnvironment, ExecutionContext};
pub use dispatch::{CallDispatcher, CallSiteInfo, DispatchCallContext, DispatchResult};
pub use dynamic::{DynamicInliner, DynamicInlinerConfig, InlinerStats};
pub use ctfe::InterpCtfeEvaluator;
pub use datalove_rt::c::DebugOutputMode;

use std::cell::RefCell;

use datalove_rtdt as rtdt;
use datalove_datafun_ir::{
    IrBlock, IrType, Instruction, Terminator,
    BlockId, Operand, SlotDest, ConstValue, ParamMode, FuncRef,
    IrCodeUnit,
};

/// Get param mode for argument at index, defaulting to In.
fn param_mode(callee: &IrCodeUnit, i: usize) -> ParamMode {
    callee.function_context()
        .and_then(|ctx| ctx.param_modes.get(i).copied())
        .unwrap_or(ParamMode::In)
}

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
        Self::new_with_options(datalove_rt::c::DebugOutputMode::Disabled, None)
    }

    /// Create a new interpreter with the specified debug output mode.
    pub fn new_with_debug_mode(debug_mode: datalove_rt::c::DebugOutputMode) -> Self {
        Self::new_with_options(debug_mode, None)
    }

    /// Create a new interpreter with all configuration options.
    pub fn new_with_options(
        debug_mode: datalove_rt::c::DebugOutputMode,
        call_dispatcher: Option<Box<dyn CallDispatcher>>,
    ) -> Self {
        Self {
            runtime: datalove_rt::rust::Runtime::new_with_debug_mode(debug_mode),
            tydesc_table: IrTyDescTable::new(),
            call_dispatcher: RefCell::new(call_dispatcher),
        }
    }

    /// Get the runtime handle for memory management.
    pub fn runtime_handle(&self) -> datalove_rt::c::LocalRtHandle {
        self.runtime.handle()
    }

    /// Take the call dispatcher out of the interpreter.
    ///
    /// Returns the dispatcher if one was set, leaving None in its place.
    /// Useful for inspecting dispatcher state (like inliner stats) after execution.
    pub fn take_dispatcher(&self) -> Option<Box<dyn CallDispatcher>> {
        self.call_dispatcher.borrow_mut().take()
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
            let result = if output_string.data.is_null() || output_string.size == rtdt::Index::ZERO {
                String::new()
            } else {
                let bytes = std::slice::from_raw_parts(output_string.data, output_string.size.as_usize());
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
    pub fn destroy_value(&mut self, value: &Value) {
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.runtime.handle(),
                value.ptr,
                value.tydesc,
            );
        }
    }

    /// Execute a function with arguments using a script environment.
    ///
    /// This allows the function to call other functions registered in the environment.
    pub fn call_with_env(
        &mut self,
        func: &IrCodeUnit,
        args: Vec<Value>,
        ret_dest: Destination,
        env: &ScriptEnvironment,
    ) -> Result<(), InterpError> {
        // Create an empty context (module functions are resolved via registry, not local context).
        let ctx = ExecutionContext::new(&[]);
        // Use the environment's registry but create fresh frames (function execution
        // doesn't persist frames like script units do).
        let mut frames = FrameStore::new();
        self.call_in_context(func, None, args, ret_dest, &ctx, &env.registry, &mut frames)
    }

    /// Execute a function with arguments in a context with available functions.
    ///
    /// `func_ref` identifies the function being executed (for call site tracking).
    pub fn call_in_context(
        &mut self,
        func: &IrCodeUnit,
        func_ref: Option<FuncRef>,
        args: Vec<Value>,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        let func_ctx = func.function_context()
            .expect("call_in_context requires a function code unit");

        // Compute layout.
        let layout = IrLayout::compute(
            &func.value_types,
            &func.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame with param storage (no live value tracking for functions).
        let mut frame = Frame::new_function(layout, func_ctx.params.len());

        // Set up parameters as pointers to caller's data.
        // All params store pointers - mode determines ownership semantics.
        for (i, &param_id) in func_ctx.params.iter().enumerate() {
            if i < args.len() {
                let src = &args[i];
                let mode = func_ctx.param_modes.get(i).copied().unwrap_or(ParamMode::In);

                // Get tydesc for this param from param_types.
                let param_type = &func_ctx.param_types[i];
                let tydesc = self.tydesc_table.get_or_create(param_type);

                // Initialized: true for In/Ref/Mut (data exists), false for Out (callee writes first).
                let initialized = !matches!(mode, ParamMode::Out);

                frame.set_param(param_id, src.ptr, tydesc, initialized);
            }
        }

        // Execute blocks, writing return value directly to ret_dest.
        // Functions use ret_dest for Return, not expr_dest.
        let result = self.execute_blocks(&func.blocks, &mut frame, ret_dest, None, ctx, registry, frames, func_ref.as_ref());

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
        unit: &IrCodeUnit,
        env: &mut ScriptEnvironment,
        ret_dest: Destination,
        expr_dest: Option<Destination>,
    ) -> Result<UnitCompletion, InterpError> {
        let script_ctx = unit.script_context()
            .expect("execute_script_unit_in_env requires a script code unit");

        // Compute layout.
        let layout = IrLayout::compute(
            &unit.value_types,
            &unit.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame with live value tracking for script cleanup.
        let mut frame = Frame::new_script(layout);

        // Create execution context with local functions.
        let ctx = ExecutionContext::new(&unit.nested_units);

        // Execute blocks with registry for function lookups and frames for slot access.
        // Script units don't have a single function ID, so pass None.
        let result = self.execute_blocks(
            &unit.blocks,
            &mut frame,
            ret_dest,
            expr_dest,
            &ctx,
            &env.registry,
            &mut env.frames,
            None,
        );

        // On error, destroy the frame and propagate the error.
        if let Err(e) = result {
            frame.destroy_on_error(
                self.runtime.handle(),
                &script_ctx.unit_end_values,
                &script_ctx.unit_end_slots,
            );
            return Err(e);
        }

        // Add this unit's frame and code units to the environment for future units.
        env.add_unit(
            frame,
            unit.nested_units.clone(),
            script_ctx.unit_end_values.clone(),
            script_ctx.unit_end_slots.clone(),
        );

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
        current_func: Option<&FuncRef>,
    ) -> Result<UnitCompletion, InterpError> {
        let mut current_block = BlockId(0);

        loop {
            // Direct indexing: blocks are renumbered during lowering so blocks[i].id.0 == i.
            let block = &blocks[current_block.0 as usize];

            // Execute instructions.
            for instr in &block.instructions {
                self.execute_instruction(instr, frame, ctx, registry, frames, current_func)?;
            }

            // Handle terminator.
            match &block.terminator {
                Terminator::Goto { target, args } => {
                    // Pass block arguments to target block.
                    self.pass_block_args(blocks, *target, args, frame, frames)?;
                    current_block = *target;
                }
                Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                    let cond_val = self.read_operand(cond, frame, frames);
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
                        let val = self.read_operand(op, frame, frames);
                        // Use move_value (shallow copy). The frame will be
                        // destroyed by call_in_context, so we must transfer
                        // ownership to avoid double-free.
                        unsafe { self.move_value(&val, ret_dest); }
                        Self::mark_source_dropped_all(op, frame, frames);
                    }
                    return Ok(UnitCompletion::Normal);
                }
                Terminator::UnitEnd { result } => {
                    if let Some(op) = result {
                        let val = self.read_operand(op, frame, frames);
                        // Write to expr_dest (not ret_dest) for expression results.
                        let dest = expr_dest.expect("UnitEnd with result requires expr_dest");
                        unsafe { self.move_value(&val, dest); }
                        Self::mark_source_dropped_all(op, frame, frames);
                    }
                    return Ok(UnitCompletion::Normal);
                }
                Terminator::UnitEarlyReturn { value } => {
                    let val = self.read_operand(value, frame, frames);
                    // Debuglog the value (borrow, not consume).
                    let rt_handle = self.runtime.handle();
                    unsafe {
                        datalove_rt::c::dtlv_rti_debuglog_local(
                            rt_handle,
                            val.ptr,
                            val.tydesc,
                        );
                    }
                    // Write to ret_dest (Result<(), Error> type).
                    unsafe { self.move_value(&val, ret_dest); }
                    Self::mark_source_dropped_all(value, frame, frames);
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

        // Direct indexing: blocks are renumbered so blocks[i].id.0 == i.
        let target_block = &blocks[target.0 as usize];

        // Pass each argument to the corresponding block parameter.
        for (param_id, arg) in target_block.params.iter().zip(args.iter()) {
            let src_val = self.read_operand(arg, frame, frames);
            let dest_slot = frame.value_dest(*param_id);
            // Block args use move semantics.
            unsafe { self.move_value(&src_val, dest_slot); }
            frame.mark_value_live(*param_id);
            Self::mark_source_dropped_all(arg, frame, frames);
        }
        Ok(())
    }

    fn execute_instruction(
        &mut self,
        instr: &Instruction,
        frame: &mut Frame,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        current_func: Option<&FuncRef>,
    ) -> Result<(), InterpError> {
        match instr {
            Instruction::Const { dest, value } => {
                let dest_slot = frame.value_dest(*dest);
                self.write_const(value, dest_slot);
                frame.mark_value_live(*dest);
            }
            Instruction::Copy { dest, src } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.copy_value(&src_val, dest_slot); }
                frame.mark_value_live(*dest);
            }
            Instruction::Move { dest, src } => {
                // Precise move: ownership analysis guarantees source exists.
                // Mark external sources dropped so destroy_live_values skips them.
                // (Local sources don't need marking - they're not in unit_end_values.)
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.move_value(&src_val, dest_slot); }
                frame.mark_value_live(*dest);
                if let Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } = src {
                    Self::mark_source_dropped_all(src, frame, frames);
                }
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                let lhs_val = self.read_operand(lhs, frame, frames);
                let rhs_val = self.read_operand(rhs, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_binop(*op, &lhs_val, &rhs_val, dest_slot);
                frame.mark_value_live(*dest);
            }
            Instruction::UnaryOp { dest, op, operand } => {
                let src_val = self.read_operand(operand, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_unaryop(*op, &src_val, dest_slot);
                frame.mark_value_live(*dest);
            }
            Instruction::SlotStoreCopy { dest, value } => {
                let src_val = self.read_operand(value, frame, frames);
                match dest {
                    SlotDest::Local(slot_id) => {
                        // Destroy old value if slot was already initialized.
                        // This happens when reassigning copy types (no Drop emitted for them).
                        if frame.is_slot_initialized(*slot_id) {
                            let old_val = frame.slot(*slot_id).unwrap();
                            unsafe {
                                datalove_rt::c::dtlv_rti_any_destroy_local(
                                    self.runtime.handle(),
                                    old_val.ptr,
                                    old_val.tydesc,
                                );
                            }
                        }
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.copy_value(&src_val, dest_slot); }
                        frame.mark_slot_initialized(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            self.runtime.handle(),
                            *unit,
                            *slot,
                            &src_val,
                        );
                    }
                }
            }
            Instruction::SlotStoreMove { dest, value } => {
                let src_val = self.read_operand(value, frame, frames);
                match dest {
                    SlotDest::Local(slot_id) => {
                        // For move types, the compiler emits Drop before SlotStoreMove,
                        // so the slot should not be initialized. But check defensively.
                        if frame.is_slot_initialized(*slot_id) {
                            let old_val = frame.slot(*slot_id).unwrap();
                            unsafe {
                                datalove_rt::c::dtlv_rti_any_destroy_local(
                                    self.runtime.handle(),
                                    old_val.ptr,
                                    old_val.tydesc,
                                );
                            }
                        }
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.move_value(&src_val, dest_slot); }
                        Self::mark_source_dropped_local(value, frame);
                        frame.mark_slot_initialized(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            self.runtime.handle(),
                            *unit,
                            *slot,
                            &src_val,
                        );
                        Self::mark_source_dropped_local(value, frame);
                    }
                }
            }
            Instruction::ParamStore { param, value } => {
                // Mut params are always initialized - always destroy old value.
                let src_val = self.read_operand(value, frame, frames);
                let dest_ptr = frame.param_dest(*param);
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        dest_ptr.ptr,
                        dest_ptr.tydesc,
                    );
                }
                unsafe { self.move_value(&src_val, dest_ptr); }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::ParamStoreTracked { param, value } => {
                // Out params: caller destroys before call, so first write sees
                // uninitialized memory. Check tracking byte before destroying.
                let src_val = self.read_operand(value, frame, frames);
                let dest_ptr = frame.param_dest(*param);
                if frame.is_param_initialized(*param) {
                    unsafe {
                        datalove_rt::c::dtlv_rti_any_destroy_local(
                            self.runtime.handle(),
                            dest_ptr.ptr,
                            dest_ptr.tydesc,
                        );
                    }
                }
                unsafe { self.move_value(&src_val, dest_ptr); }
                frame.mark_param_initialized(*param);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::RefStore { dest, value } => {
                // Store through a reference operand. Used after inlining mut params.
                // The destination is always precise (initialized), so always destroy old value.
                let src_val = self.read_operand(value, frame, frames);
                let dest_val = self.get_operand_dest(dest, frame);
                let dest_ptr = Destination { ptr: dest_val.ptr, tydesc: dest_val.tydesc };
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        dest_ptr.ptr,
                        dest_ptr.tydesc,
                    );
                }
                unsafe { self.move_value(&src_val, dest_ptr); }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::RefSetField { dest, field_path, value } => {
                // Store to a field through a reference operand. Used after inlining mut params.
                let value_val = self.read_operand(value, frame, frames);
                let dest_ptr = self.get_operand_dest(dest, frame);
                // Navigate to the field.
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    dest_ptr.ptr, dest_ptr.tydesc, field_path
                );
                // Destroy old value and store new value.
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        current_ptr,
                        current_tydesc,
                    );
                }
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(value_val.ptr, current_ptr, size);
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::RefStoreTracked { dest, value } => {
                // Store through a reference operand with tracking. Used after inlining out params.
                // The destination was uninitialized, so don't destroy old value.
                let src_val = self.read_operand(value, frame, frames);
                let dest_val = self.get_operand_dest(dest, frame);
                let dest_ptr = Destination { ptr: dest_val.ptr, tydesc: dest_val.tydesc };
                unsafe { self.move_value(&src_val, dest_ptr); }
                // Mark the destination as initialized.
                match dest {
                    Operand::Slot(slot) => frame.mark_slot_initialized(*slot),
                    Operand::Param(param) => frame.mark_param_initialized(*param),
                    // Other operand types don't have tracking in the same way.
                    _ => {}
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::RefSetFieldTracked { dest, field_path, value } => {
                // Store to a field through a reference operand with tracking. Used after inlining out params.
                let value_val = self.read_operand(value, frame, frames);
                let dest_ptr = self.get_operand_dest(dest, frame);
                // Navigate to the field.
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    dest_ptr.ptr, dest_ptr.tydesc, field_path
                );
                // Don't destroy old value (was uninitialized), just store new value.
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(value_val.ptr, current_ptr, size);
                }
                // Mark the destination as initialized.
                match dest {
                    Operand::Slot(slot) => frame.mark_slot_initialized(*slot),
                    Operand::Param(param) => frame.mark_param_initialized(*param),
                    // Other operand types don't have tracking in the same way.
                    _ => {}
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::SlotLoadCopy { dest, slot } => {
                let slot_val = frame.slot(*slot).unwrap();
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.copy_value(&slot_val, dest_slot); }
                frame.mark_value_live(*dest);
            }
            Instruction::SlotLoadMove { dest, slot } => {
                // Precise slot load: ownership analysis guarantees slot is occupied.
                // Slot is not marked dropped - destroy_live_values skips untracked slots,
                // and precise slots are explicitly dropped via Drop instructions.
                let slot_val = frame.slot(*slot).unwrap();
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.move_value(&slot_val, dest_slot); }
                frame.mark_value_live(*dest);
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                // Tracked slot load: slot may have been moved, updates tracking.
                // Mark slot dropped so destroy_live_values skips it.
                let slot_val = frame.slot(*slot).unwrap();
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.move_value(&slot_val, dest_slot); }
                frame.mark_slot_dropped(*slot);
                frame.mark_value_live(*dest);
            }
            Instruction::Pack { dest, ty: _, fields } => {
                let field_vals: Vec<Value> = fields.iter()
                    .map(|op| self.read_operand(op, frame, frames))
                    .collect();
                let dest_slot = frame.value_dest(*dest);
                // Check type tag to determine if tuple or struct.
                let tag = unsafe { (*dest_slot.tydesc).type_tag };
                match tag {
                    rtdt::TyTag::Tuple => self.execute_pack_tuple(&field_vals, dest_slot),
                    rtdt::TyTag::Struct => self.execute_pack_struct(&field_vals, dest_slot),
                    _ => unreachable!("Pack requires tuple or struct type, got {:?}", tag),
                }
                frame.mark_value_live(*dest);
                // Mark source fields as moved (linear semantics - consumes fields).
                for field in fields {
                    Self::mark_source_dropped_local(field, frame);
                }
            }
            Instruction::Unpack { dests, src } => {
                let src_val = self.read_operand(src, frame, frames);
                let tag = unsafe { (*src_val.tydesc).type_tag };
                match tag {
                    rtdt::TyTag::Tuple => {
                        let tuple_info = unsafe { (*src_val.tydesc).type_info.tuple };
                        for (i, &dest_id) in dests.iter().enumerate() {
                            let dest_slot = frame.value_dest(dest_id);
                            let field_info = unsafe { &*tuple_info.fields.add(i) };
                            let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                            let size = unsafe { (*field_info.tydesc).size as usize };
                            unsafe { std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, size); }
                            frame.mark_value_live(dest_id);
                        }
                    }
                    rtdt::TyTag::Struct => {
                        let struct_info = unsafe { (*src_val.tydesc).type_info.struct_ };
                        for (i, &dest_id) in dests.iter().enumerate() {
                            let dest_slot = frame.value_dest(dest_id);
                            let field_info = unsafe { &*struct_info.fields.add(i) };
                            let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                            let size = unsafe { (*field_info.tydesc).size as usize };
                            unsafe { std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, size); }
                            frame.mark_value_live(dest_id);
                        }
                    }
                    _ => unreachable!("Unpack requires tuple or struct type, got {:?}", tag),
                }
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                // Execute checked arithmetic and set overflow flag.
                let lhs_val = self.read_operand(lhs, frame, frames);
                let rhs_val = self.read_operand(rhs, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                let overflow_slot = frame.value_dest(*overflow);
                self.execute_binop_checked(*op, &lhs_val, &rhs_val, dest_slot, overflow_slot);
                frame.mark_value_live(*dest);
                frame.mark_value_live(*overflow);
            }
            Instruction::UnaryOpChecked { dest, overflow, op, operand } => {
                // Execute checked unary op and set overflow flag.
                let operand_val = self.read_operand(operand, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                let overflow_slot = frame.value_dest(*overflow);
                self.execute_unaryop_checked(*op, &operand_val, dest_slot, overflow_slot);
                frame.mark_value_live(*dest);
                frame.mark_value_live(*overflow);
            }
            Instruction::WrapSome { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_some(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
            }
            Instruction::WrapNone { dest } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_none(dest_slot);
                frame.mark_value_live(*dest);
            }
            Instruction::EnumVariant { dest, variant_index, payload } => {
                let payload_val = payload.as_ref()
                    .map(|p| self.read_operand(p, frame, frames));
                let dest_slot = frame.value_dest(*dest);
                self.execute_enum_variant(*variant_index, payload_val.as_ref(), dest_slot);
                frame.mark_value_live(*dest);
                // Mark payload source as moved if present.
                if let Some(p) = payload {
                    Self::mark_source_dropped_local(p, frame);
                }
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                let is_some_slot = frame.value_dest(*is_some);
                self.execute_unwrap_option(&src_val, dest_slot, is_some_slot);
                frame.mark_value_live(*dest);
                frame.mark_value_live(*is_some);
            }
            Instruction::WrapOk { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_ok(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
            }
            Instruction::WrapErr { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_err(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                let src_val = self.read_operand(src, frame, frames);
                let ok_slot = frame.value_dest(*ok_dest);
                let err_slot = frame.value_dest(*err_dest);
                let is_ok_slot = frame.value_dest(*is_ok);
                self.execute_unwrap_result(&src_val, ok_slot, err_slot, is_ok_slot);
                frame.mark_value_live(*ok_dest);
                frame.mark_value_live(*err_dest);
                frame.mark_value_live(*is_ok);
            }
            Instruction::ErrorFrom { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_error_from(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
            }
            Instruction::DataFrom { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_data_from(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
            }
            Instruction::Call { site_id, dest, func, args } => {
                let callee = ctx.get_function(func, registry);
                let arg_vals = self.prepare_call_args(callee, args, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                Self::mark_consumed_call_args(callee, args, frame);

                // Build call site info for dispatcher if we have caller context.
                let call_site_info = current_func.map(|caller| {
                    dispatch::CallSiteInfo {
                        caller: caller.clone(),
                        call_site_id: *site_id,
                    }
                });

                // Try dispatcher first, fall back to interpreter.
                let call_result = if let Some(result) = self.try_dispatch_call(
                    func, callee, &arg_vals, dest_slot, ctx, registry, frames, call_site_info
                ) {
                    result
                } else {
                    self.execute_call(callee, func, arg_vals, dest_slot, ctx, registry, frames)
                };
                call_result?;

                frame.mark_value_live(*dest);
                Self::mark_out_params_initialized(callee, args, frame);
            }
            // ComptimeCall behaves exactly like Call - the specialization metadata is
            // only used by the specialization pass. Without specialization, this calls
            // the original function with original args.
            Instruction::ComptimeCall { dest, func, args, .. } => {
                let callee = ctx.get_function(func, registry);
                let arg_vals = self.prepare_call_args(callee, args, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                Self::mark_consumed_call_args(callee, args, frame);

                // Try dispatcher first, fall back to interpreter.
                // ComptimeCall doesn't have site_id, so no call_site_info.
                let call_result = if let Some(result) = self.try_dispatch_call(
                    func, callee, &arg_vals, dest_slot, ctx, registry, frames, None
                ) {
                    result
                } else {
                    self.execute_call(callee, func, arg_vals, dest_slot, ctx, registry, frames)
                };
                call_result?;

                frame.mark_value_live(*dest);
                Self::mark_out_params_initialized(callee, args, frame);
            }
            Instruction::ListNew { dest, elements } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_list_new(elements, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
                // Mark source elements as moved (linear semantics - consumes elements).
                for elem in elements {
                    Self::mark_source_dropped_local(elem, frame);
                }
            }
            Instruction::SetNew { dest, elements } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_set_new(elements, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
                // Mark source elements as moved (linear semantics - consumes elements).
                for elem in elements {
                    Self::mark_source_dropped_local(elem, frame);
                }
            }
            Instruction::MapNew { dest, entries } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_map_new(entries, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
                // Mark source entries as moved (linear semantics - consumes entries).
                for (key, val) in entries {
                    Self::mark_source_dropped_local(key, frame);
                    Self::mark_source_dropped_local(val, frame);
                }
            }
            Instruction::TensorNew { dest, shape, elements } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_tensor_new(shape, elements, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
                // Mark source elements as moved (linear semantics - consumes elements).
                for elem in elements {
                    Self::mark_source_dropped_local(elem, frame);
                }
            }
            Instruction::TableNew { dest, rows } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_table_new(rows, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
                // Mark source rows as moved (linear semantics - consumes rows).
                for row in rows {
                    Self::mark_source_dropped_local(row, frame);
                }
            }
            Instruction::Drop { operand } => {
                // Precise drop: ownership analysis guarantees value exists.
                let val = self.read_operand(operand, frame, frames);
                self.execute_drop(&val);
                Self::mark_source_dropped_local(operand, frame);
            }
            Instruction::DropTracked { operand } => {
                // Tracked drop: check initialization first, skip if not present.
                // Only emitted for Tracked bindings (slots, Out params).
                // Values are Precise and use Drop instead.
                let is_initialized = match operand {
                    Operand::Slot(id) => frame.is_slot_initialized(*id),
                    Operand::Param(id) => frame.is_param_initialized(*id),
                    Operand::ExternalSlot { unit, slot } => {
                        frames.is_external_slot_initialized(*unit, *slot)
                    }
                    // Values are Precise, never Tracked.
                    Operand::Value(_) | Operand::ValueRef(_) | Operand::ExternalValue { .. } => {
                        unreachable!("DropTracked emitted for Precise binding")
                    }
                };
                if !is_initialized {
                    // Already dropped or moved, skip.
                    return Ok(());
                }
                let val = self.read_operand(operand, frame, frames);
                self.execute_drop(&val);
                Self::mark_source_dropped_local(operand, frame);
            }
            Instruction::DropViaRef { ref_value } => {
                // Drop through a reference value (e.g., from GetFieldRef).
                // The reference value contains a pointer to what we want to destroy.
                let val = frame.value_deref(*ref_value);
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        val.ptr,
                        val.tydesc,
                    );
                }
                // Note: we don't mark the ref_value as dropped - it's just a reference.
                // The underlying storage still exists but is now uninitialized.
            }
            Instruction::UnitEndDrop { operand: _ } => {
                // No-op: script-level bindings persist for subsequent REPL units.
                // AOT backend handles this as unconditional drop.
            }
            Instruction::UnitEndDropTracked { operand: _ } => {
                // No-op: script-level bindings persist for subsequent REPL units.
                // AOT backend handles this as conditional drop (checks tracking byte).
            }
            Instruction::DebugLog { operand } => {
                let val = self.read_operand(operand, frame, frames);
                let rt_handle = self.runtime.handle();
                unsafe {
                    datalove_rt::c::dtlv_rti_debuglog_local(
                        rt_handle,
                        val.ptr,
                        val.tydesc,
                    );
                }
                // Note: no mark_dropped - we're borrowing, not consuming.
            }
            Instruction::Widen { dest, src } => {
                // Widen a fixed-width integer to Int.
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                // Cast dest to Int buffer and call widen_to_int.
                unsafe {
                    let int_buf = &mut *(dest_slot.ptr as *mut datalove_rtdt::Int);
                    self.widen_to_int(&src_val, int_buf);
                }
                frame.mark_value_live(*dest);
                // Source is borrowed (read), not consumed.
            }
            Instruction::WidenFixed { dest, src } => {
                // Widen a fixed-width integer to a larger fixed-width integer.
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                unsafe {
                    self.widen_fixed(&src_val, &dest_slot);
                }
                frame.mark_value_live(*dest);
                // Source is borrowed (read), not consumed.
            }
            Instruction::Clone { dest, src } => {
                // Clone a linear value (deep copy for @ operator).
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                // Use runtime clone function.
                unsafe {
                    datalove_rt::c::dtlv_rti_clone_local(
                        self.runtime.handle(),
                        src_val.ptr,
                        src_val.tydesc,
                        dest_slot.ptr,
                        src_val.tydesc, // Same type for clone
                    );
                }
                frame.mark_value_live(*dest);
                // Source is borrowed (read), not consumed.
            }
            Instruction::Nop => {}
            Instruction::GetField { dest, src, field_index } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                let tag = unsafe { (*src_val.tydesc).type_tag };

                match tag {
                    rtdt::TyTag::Tuple => {
                        let tuple_info = unsafe { (*src_val.tydesc).type_info.tuple };
                        let field_info = unsafe { &*tuple_info.fields.add(*field_index as usize) };
                        let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                        let field_size = unsafe { (*field_info.tydesc).size as usize };
                        // Copy the field value. Only copy types are allowed for field projections.
                        unsafe {
                            std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, field_size);
                        }
                    }
                    rtdt::TyTag::Struct => {
                        let struct_info = unsafe { (*src_val.tydesc).type_info.struct_ };
                        let field_info = unsafe { &*struct_info.fields.add(*field_index as usize) };
                        let field_ptr = unsafe { src_val.ptr.add(field_info.offset as usize) };
                        let field_size = unsafe { (*field_info.tydesc).size as usize };
                        // Copy the field value. Only copy types are allowed for field projections.
                        unsafe {
                            std::ptr::copy_nonoverlapping(field_ptr, dest_slot.ptr, field_size);
                        }
                    }
                    _ => unreachable!("GetField requires tuple or struct type, got {:?}", tag),
                }
                frame.mark_value_live(*dest);
            }
            Instruction::GetFieldRef { dest, src, field_index } => {
                // Get a reference (pointer) to a field within an aggregate.
                // Unlike GetField, this stores the field pointer instead of copying.
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                let tag = unsafe { (*src_val.tydesc).type_tag };

                let field_ptr = match tag {
                    rtdt::TyTag::Tuple => {
                        let tuple_info = unsafe { (*src_val.tydesc).type_info.tuple };
                        let field_info = unsafe { &*tuple_info.fields.add(*field_index as usize) };
                        unsafe { src_val.ptr.add(field_info.offset as usize) }
                    }
                    rtdt::TyTag::Struct => {
                        let struct_info = unsafe { (*src_val.tydesc).type_info.struct_ };
                        let field_info = unsafe { &*struct_info.fields.add(*field_index as usize) };
                        unsafe { src_val.ptr.add(field_info.offset as usize) }
                    }
                    _ => unreachable!("GetFieldRef requires tuple or struct type, got {:?}", tag),
                };

                // Store the field pointer in dest (ref value stores pointer, not data).
                unsafe {
                    *(dest_slot.ptr as *mut *mut u8) = field_ptr;
                }
                frame.mark_value_live(*dest);
            }
            Instruction::SetField { slot, field_path, value } => {
                let value_val = self.read_operand(value, frame, frames);

                // Get the slot's base pointer and tydesc.
                let slot_info = match slot {
                    SlotDest::Local(id) => frame.slot_dest(*id),
                    SlotDest::External { unit, slot: ext_slot } => {
                        unreachable!(
                            "SetField on external slot unit={} slot={:?} not supported",
                            unit, ext_slot
                        );
                    }
                };

                // Navigate field path to find target field.
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );

                // Destroy old field value before overwriting (handles move types).
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        current_ptr,
                        current_tydesc,
                    );
                }

                // Copy new value to target field.
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        value_val.ptr,
                        current_ptr,
                        size,
                    );
                }
            }
            Instruction::ParamSetField { param, field_path, value } => {
                // Mut params are always initialized - always destroy old field.
                let value_val = self.read_operand(value, frame, frames);
                let slot_info = frame.param_dest(*param);
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        current_ptr,
                        current_tydesc,
                    );
                }
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(value_val.ptr, current_ptr, size);
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::ParamSetFieldTracked { param, field_path, value } => {
                // Out params: caller destroys before call, so first write sees
                // uninitialized memory. Check tracking byte before destroying.
                let value_val = self.read_operand(value, frame, frames);
                let slot_info = frame.param_dest(*param);
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );
                if frame.is_param_initialized(*param) {
                    unsafe {
                        datalove_rt::c::dtlv_rti_any_destroy_local(
                            self.runtime.handle(),
                            current_ptr,
                            current_tydesc,
                        );
                    }
                }
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(value_val.ptr, current_ptr, size);
                }
                frame.mark_param_initialized(*param);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::Intrinsic { dest, intrinsic, args } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_intrinsic(*intrinsic, args, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
            }
            // Slot tracking variants - these track SLOT state, not value state.
            Instruction::SlotStoreCopyTracked { dest, value } => {
                let src_val = self.read_operand(value, frame, frames);
                match dest {
                    SlotDest::Local(slot_id) => {
                        if frame.is_slot_initialized(*slot_id) {
                            let old_val = frame.slot(*slot_id).unwrap();
                            unsafe {
                                datalove_rt::c::dtlv_rti_any_destroy_local(
                                    self.runtime.handle(),
                                    old_val.ptr,
                                    old_val.tydesc,
                                );
                            }
                        }
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.copy_value(&src_val, dest_slot); }
                        frame.mark_slot_initialized(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            self.runtime.handle(),
                            *unit,
                            *slot,
                            &src_val,
                        );
                    }
                }
            }
            Instruction::SlotStoreMoveTracked { dest, value } => {
                let src_val = self.read_operand(value, frame, frames);
                match dest {
                    SlotDest::Local(slot_id) => {
                        if frame.is_slot_initialized(*slot_id) {
                            let old_val = frame.slot(*slot_id).unwrap();
                            unsafe {
                                datalove_rt::c::dtlv_rti_any_destroy_local(
                                    self.runtime.handle(),
                                    old_val.ptr,
                                    old_val.tydesc,
                                );
                            }
                        }
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.move_value(&src_val, dest_slot); }
                        Self::mark_source_dropped_local(value, frame);
                        frame.mark_slot_initialized(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            self.runtime.handle(),
                            *unit,
                            *slot,
                            &src_val,
                        );
                        Self::mark_source_dropped_local(value, frame);
                    }
                }
            }
            Instruction::SetFieldTracked { slot, field_path, value } => {
                let value_val = self.read_operand(value, frame, frames);
                let slot_info = match slot {
                    SlotDest::Local(id) => frame.slot_dest(*id),
                    SlotDest::External { unit, slot: ext_slot } => {
                        unreachable!(
                            "SetFieldTracked on external slot unit={} slot={:?} not supported",
                            unit, ext_slot
                        );
                    }
                };
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        current_ptr,
                        current_tydesc,
                    );
                }
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        value_val.ptr,
                        current_ptr,
                        size,
                    );
                }
            }
        }
        Ok(())
    }

    /// Mark a local operand as dropped after a move.
    ///
    /// Handles Value, Slot, and Param operands in the current frame.
    /// External operands are ignored since they belong to other frames and are
    /// handled separately (typically in terminators via `mark_source_dropped_all`).
    fn mark_source_dropped_local(operand: &Operand, frame: &mut Frame) {
        match operand {
            Operand::Value(id) | Operand::ValueRef(id) => frame.mark_value_dropped(*id),
            Operand::Slot(id) => frame.mark_slot_dropped(*id),
            Operand::Param(id) => frame.mark_param_dropped(*id),
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
        }
    }

    /// Mark any operand as dropped after a move, including external operands.
    ///
    /// Used for terminators (Return, UnitEnd, UnitEarlyReturn) and the Move
    /// instruction where external values/slots may be consumed.
    fn mark_source_dropped_all(operand: &Operand, frame: &mut Frame, frames: &mut FrameStore) {
        match operand {
            Operand::Value(id) | Operand::ValueRef(id) => frame.mark_value_dropped(*id),
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

    pub(crate) fn read_operand(
        &self,
        op: &Operand,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Value {
        match op {
            Operand::Value(id) => frame.value(*id),
            Operand::ValueRef(id) => frame.value_deref(*id),
            Operand::Slot(id) => frame.slot(*id).unwrap(),
            Operand::Param(id) => frame.param(*id).unwrap(),
            Operand::ExternalValue { unit, value } => {
                frames.external_value(*unit, *value).unwrap()
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.external_slot(*unit, *slot).unwrap()
            }
        }
    }

    /// Get pointer to operand's destination without checking initialization.
    ///
    /// Used for Out params where we need to pass a pointer to an uninitialized slot.
    /// For ValueRef operands, dereferences to get the actual destination.
    ///
    /// Panics if operand is not a Slot, Value, or ValueRef (compiler bug).
    fn get_operand_dest(&self, op: &Operand, frame: &mut Frame) -> Value {
        match op {
            Operand::Slot(id) => {
                let dest = frame.slot_dest(*id);
                Value { ptr: dest.ptr, tydesc: dest.tydesc }
            }
            Operand::Value(id) => {
                // Normal value - return the value storage as destination.
                let dest = frame.value_dest(*id);
                Value { ptr: dest.ptr, tydesc: dest.tydesc }
            }
            Operand::ValueRef(id) => {
                // Dereference to get the pointed-to destination.
                frame.value_deref(*id)
            }
            _ => panic!("get_operand_dest: invalid operand {:?} for out param", op),
        }
    }

    // -------------------------------------------------------------------------
    // Call instruction helpers
    // -------------------------------------------------------------------------

    /// Prepare arguments for a function call.
    ///
    /// For Out params, gets the destination pointer and destroys any existing value
    /// (since the callee treats the storage as uninitialized).
    /// For other params, reads the value normally.
    fn prepare_call_args(
        &self,
        callee: &IrCodeUnit,
        args: &[Operand],
        frame: &mut Frame,
        frames: &FrameStore,
    ) -> Vec<Value> {
        let mut arg_vals = Vec::with_capacity(args.len());
        for (i, op) in args.iter().enumerate() {
            let mode = param_mode(callee, i);
            if mode == ParamMode::Out {
                // Out param: get destination pointer, destroy existing value.
                let val = self.get_operand_dest(op, frame);
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        val.ptr,
                        val.tydesc,
                    );
                }
                arg_vals.push(val);
            } else {
                arg_vals.push(self.read_operand(op, frame, frames));
            }
        }
        arg_vals
    }

    /// Mark consumed arguments as dropped after preparing a call.
    ///
    /// Ownership rules:
    /// - Ref/Mut/Out params: borrowed, caller retains ownership
    /// - In params with Copy types: copied, caller retains ownership
    /// - In params with non-Copy types: moved, mark as dropped
    fn mark_consumed_call_args(callee: &IrCodeUnit, args: &[Operand], frame: &mut Frame) {
        let param_types = callee.function_context().map(|c| &c.param_types[..]).unwrap_or(&[]);
        for (i, arg) in args.iter().enumerate() {
            let mode = param_mode(callee, i);
            if matches!(mode, ParamMode::Ref | ParamMode::Mut | ParamMode::Out) {
                continue; // Borrowed, not consumed.
            }
            if let Some(param_type) = param_types.get(i) {
                if param_type.is_copy() {
                    continue; // Copied, not consumed.
                }
            }
            Self::mark_source_dropped_local(arg, frame);
        }
    }

    /// Try to dispatch a call via the JIT dispatcher.
    ///
    /// Returns `Some(result)` if the dispatcher handled the call,
    /// `None` if it should fall through to the interpreter.
    fn try_dispatch_call(
        &mut self,
        func: &FuncRef,
        callee: &IrCodeUnit,
        arg_vals: &[Value],
        dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        call_site_info: Option<dispatch::CallSiteInfo>,
    ) -> Option<Result<(), InterpError>> {
        // Take dispatcher temporarily to avoid borrow conflicts.
        let mut dispatcher = self.call_dispatcher.borrow_mut().take()?;

        // Capture rt_handle before borrowing self for the context.
        let rt_handle = self.runtime.handle();

        let call_ctx = dispatch::DispatchCallContext {
            exec_ctx: ctx,
            registry,
            frames,
            interp: self,
            call_site_info,
        };

        let result = match dispatcher.dispatch_call(func, callee, arg_vals, dest, rt_handle, call_ctx) {
            dispatch::DispatchResult::Handled(result) => Some(result),
            dispatch::DispatchResult::NotHandled => None,
        };

        // Restore dispatcher.
        *self.call_dispatcher.borrow_mut() = Some(dispatcher);
        result
    }

    /// Get an optimized version of a code unit from the dispatcher if available.
    ///
    /// Returns a cloned code unit to avoid lifetime issues with the dispatcher borrow.
    fn get_optimized_function(&self, func_ref: &FuncRef) -> Option<IrCodeUnit> {
        let dispatcher = self.call_dispatcher.borrow();
        dispatcher.as_ref().and_then(|d| d.get_optimized_function(func_ref).cloned())
    }

    /// Mark Out param destinations as initialized after a call returns.
    fn mark_out_params_initialized(callee: &IrCodeUnit, args: &[Operand], frame: &mut Frame) {
        for (i, arg) in args.iter().enumerate() {
            if param_mode(callee, i) == ParamMode::Out {
                match arg {
                    Operand::Slot(id) => frame.mark_slot_initialized(*id),
                    Operand::Value(id) => frame.mark_value_live(*id),
                    _ => {}
                }
            }
        }
    }

    /// Execute a function call via the interpreter.
    ///
    /// Uses `func_ref` to look up optimized versions and to identify the function
    /// for call site tracking. External functions need a context with that unit's
    /// local functions; local and module functions use the current context.
    fn execute_call(
        &mut self,
        callee: &IrCodeUnit,
        func_ref: &FuncRef,
        arg_vals: Vec<Value>,
        dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        // Check if there's an optimized (inlined) version of this function.
        let optimized = self.get_optimized_function(func_ref);
        let func_to_use = optimized.as_ref().unwrap_or(callee);

        if let FuncRef::External { unit, .. } = func_ref {
            let unit_funcs = registry.unit_functions(*unit)
                .unwrap_or_else(|| panic!("external unit {} not found", unit));
            let callee_ctx = ExecutionContext::new(unit_funcs);
            self.call_in_context(func_to_use, Some(func_ref.clone()), arg_vals, dest, &callee_ctx, registry, frames)
        } else {
            self.call_in_context(func_to_use, Some(func_ref.clone()), arg_vals, dest, ctx, registry, frames)
        }
    }

    fn write_const(&mut self, value: &ConstValue, dest: Destination) {
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
                ConstValue::Index(n) => {
                    *(dest.ptr as *mut rtdt::IndexRepr) = *n;
                }
                ConstValue::Offset(n) => {
                    *(dest.ptr as *mut rtdt::OffsetRepr) = *n;
                }
                ConstValue::Int { limbs, negative } => {
                    datalove_rt::c::dtlv_rti_int_from_limbs(
                        self.runtime.handle(),
                        if limbs.is_empty() { std::ptr::null() } else { limbs.as_ptr() },
                        limbs.len() as u32,
                        *negative,
                        dest.ptr,
                        dest.tydesc,
                    );
                }
                ConstValue::F32(n) => {
                    *(dest.ptr as *mut f32) = *n;
                }
                ConstValue::F64(n) => {
                    *(dest.ptr as *mut f64) = *n;
                }
                ConstValue::String(s) => {
                    let bytes_ptr = if s.is_empty() { std::ptr::null() } else { s.as_ptr() };
                    datalove_rt::c::dtlv_rti_string_from_bytes(
                        self.runtime.handle(),
                        bytes_ptr,
                        s.len() as rtdt::IndexRepr,
                        dest.ptr,
                        dest.tydesc,
                    );
                }
                ConstValue::OptionNone => {
                    // Option layout: tag at offset 0.
                    // None tag = 1.
                    *(dest.ptr as *mut u8) = 1;
                }
                ConstValue::OptionSome(inner) => {
                    // Option layout: tag at offset 0, payload at aligned offset.
                    // Some tag = 2.
                    *(dest.ptr as *mut u8) = 2;

                    // Get inner type from tydesc.
                    let inner_tydesc = (*dest.tydesc).type_info.option.inner_tydesc;
                    let inner_align = (*inner_tydesc).align;
                    let payload_offset = rtdt::layout::option_payload_offset(inner_align);
                    let payload_ptr = dest.ptr.add(payload_offset as usize);
                    let payload_dest = Destination {
                        ptr: payload_ptr,
                        tydesc: inner_tydesc,
                    };
                    self.write_const(inner, payload_dest);
                }
                ConstValue::Tuple(fields) => {
                    // Tuple layout: fields at computed offsets.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let layout = rtdt::layout::compute_tuple_layout(tydesc_ref);
                    let tuple_info = tydesc_ref.tuple_info();
                    for (i, field_value) in fields.iter().enumerate() {
                        let field_offset = layout.field_offsets[i];
                        let field_ref = tuple_info.field(i).expect("tuple field out of bounds");
                        let field_tydesc = field_ref.tydesc().as_ptr();
                        let field_ptr = dest.ptr.add(field_offset as usize);
                        let field_dest = Destination {
                            ptr: field_ptr,
                            tydesc: field_tydesc,
                        };
                        self.write_const(field_value, field_dest);
                    }
                }
                ConstValue::Struct(fields) => {
                    // Struct layout: fields at computed offsets (same as tuple).
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let layout = rtdt::layout::compute_struct_layout(tydesc_ref);
                    let struct_info = tydesc_ref.struct_info();
                    for (i, (_, field_value)) in fields.iter().enumerate() {
                        let field_offset = layout.field_offsets[i];
                        let field_ref = struct_info.field(i).expect("struct field out of bounds");
                        let field_tydesc = field_ref.tydesc().as_ptr();
                        let field_ptr = dest.ptr.add(field_offset as usize);
                        let field_dest = Destination {
                            ptr: field_ptr,
                            tydesc: field_tydesc,
                        };
                        self.write_const(field_value, field_dest);
                    }
                }
                ConstValue::Enum { variant, payload } => {
                    // Enum layout: discriminant (u32) at offset 0, payload at variant offset.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let enum_info = tydesc_ref.enum_info();

                    // Find the variant index by name.
                    let mut variant_index = None;
                    for (i, v) in tydesc_ref.iter_enum_variants().enumerate() {
                        if v.name() == variant {
                            variant_index = Some(i);
                            break;
                        }
                    }
                    let variant_idx = variant_index.expect("enum variant not found");

                    // Write discriminant.
                    *(dest.ptr as *mut u32) = variant_idx as u32;

                    // Write payload if present.
                    if let Some(payload_value) = payload {
                        let variant_ref = enum_info.variant(variant_idx).expect("variant out of bounds");
                        let payload_tydesc = variant_ref.payload().expect("variant has no payload");
                        let payload_offset = variant_ref.offset();
                        let payload_ptr = dest.ptr.add(payload_offset as usize);
                        let payload_dest = Destination {
                            ptr: payload_ptr,
                            tydesc: payload_tydesc.as_ptr(),
                        };
                        self.write_const(payload_value, payload_dest);
                    }
                }
                ConstValue::ResultOk(inner) => {
                    // Result layout: tag (u8) at offset 0, payload at aligned offset.
                    // Ok tag = 1.
                    *(dest.ptr as *mut u8) = 1;

                    // Get inner type from tydesc.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let ok_tydesc = tydesc_ref.result_ok_ty();
                    let inner_align = ok_tydesc.align();
                    let error_align = 8u32;
                    let max_align = inner_align.max(error_align);
                    let payload_offset = rtdt::layout::align_up(1, max_align);
                    let payload_ptr = dest.ptr.add(payload_offset as usize);
                    let payload_dest = Destination {
                        ptr: payload_ptr,
                        tydesc: ok_tydesc.as_ptr(),
                    };
                    self.write_const(inner, payload_dest);
                }
                ConstValue::List(elements) => {
                    // Build list from slice of elements.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let element_tydesc = tydesc_ref.list_element_ty().as_ptr();
                    let element_size = (*element_tydesc).size as usize;
                    let element_align = (*element_tydesc).align;

                    // Compute element stride.
                    let stride = rtdt::layout::align_up(element_size as u32, element_align) as usize;
                    let stride = if stride == 0 { 1 } else { stride };

                    // Allocate buffer for ALL elements.
                    let num_elements = elements.len();
                    let mut elements_buffer = vec![0u8; (stride * num_elements).max(8)];

                    // Write each element at its offset in the buffer.
                    for (i, element_value) in elements.iter().enumerate() {
                        let element_dest = Destination {
                            ptr: elements_buffer.as_mut_ptr().add(i * stride),
                            tydesc: element_tydesc,
                        };
                        self.write_const(element_value, element_dest);
                    }

                    // Build list from slice.
                    datalove_rt::c::dtlv_rti_list_build_from_slice_local(
                        self.runtime.handle(),
                        dest.ptr,
                        element_tydesc,
                        elements_buffer.as_mut_ptr(),
                        num_elements as rtdt::IndexRepr,
                    );
                }
                ConstValue::Set(elements) => {
                    // Build set from sorted slice of elements.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let element_tydesc = tydesc_ref.set_element_ty().as_ptr();
                    let element_size = (*element_tydesc).size as usize;
                    let element_align = (*element_tydesc).align;

                    // Compute element stride.
                    let stride = rtdt::layout::align_up(element_size as u32, element_align) as usize;
                    let stride = if stride == 0 { 1 } else { stride };

                    // Allocate buffer for ALL elements.
                    let num_elements = elements.len();
                    let mut elements_buffer = vec![0u8; (stride * num_elements).max(8)];

                    // Write each element at its offset in the buffer.
                    for (i, element_value) in elements.iter().enumerate() {
                        let element_dest = Destination {
                            ptr: elements_buffer.as_mut_ptr().add(i * stride),
                            tydesc: element_tydesc,
                        };
                        self.write_const(element_value, element_dest);
                    }

                    // Build set from sorted slice.
                    datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
                        self.runtime.handle(),
                        dest.ptr,
                        element_tydesc,
                        elements_buffer.as_mut_ptr(),
                        num_elements as rtdt::IndexRepr,
                    );
                }
                ConstValue::Map(entries) => {
                    // Build map from sorted slices of keys and values.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let key_tydesc = tydesc_ref.map_key_ty().as_ptr();
                    let value_tydesc = tydesc_ref.map_value_ty().as_ptr();
                    let key_size = (*key_tydesc).size as usize;
                    let key_align = (*key_tydesc).align;
                    let value_size = (*value_tydesc).size as usize;
                    let value_align = (*value_tydesc).align;

                    // Compute key and value strides.
                    let key_stride = rtdt::layout::align_up(key_size as u32, key_align) as usize;
                    let key_stride = if key_stride == 0 { 1 } else { key_stride };
                    let value_stride = rtdt::layout::align_up(value_size as u32, value_align) as usize;
                    let value_stride = if value_stride == 0 { 1 } else { value_stride };

                    // Allocate buffers for ALL keys and ALL values.
                    let num_entries = entries.len();
                    let mut keys_buffer = vec![0u8; (key_stride * num_entries).max(8)];
                    let mut values_buffer = vec![0u8; (value_stride * num_entries).max(8)];

                    // Write each key and value at their offsets in the buffers.
                    for (i, (key_value, val_value)) in entries.iter().enumerate() {
                        let key_dest = Destination {
                            ptr: keys_buffer.as_mut_ptr().add(i * key_stride),
                            tydesc: key_tydesc,
                        };
                        self.write_const(key_value, key_dest);

                        let val_dest = Destination {
                            ptr: values_buffer.as_mut_ptr().add(i * value_stride),
                            tydesc: value_tydesc,
                        };
                        self.write_const(val_value, val_dest);
                    }

                    // Build map from sorted slices.
                    datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
                        self.runtime.handle(),
                        dest.ptr,
                        key_tydesc,
                        value_tydesc,
                        keys_buffer.as_mut_ptr(),
                        values_buffer.as_mut_ptr(),
                        num_entries as rtdt::IndexRepr,
                    );
                }
                ConstValue::Table { columns: _, rows } => {
                    // Build table from rows.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);

                    // Collect column type descriptors.
                    let col_tydescs: Vec<*const rtdt::TyDesc> = tydesc_ref
                        .table_column_tydescs()
                        .map(|col| col.tydesc().as_ptr())
                        .collect();
                    let num_cols = col_tydescs.len();

                    // Compute row tuple layout manually.
                    let mut row_offset = 0u32;
                    let mut row_max_align = 1u32;
                    let mut field_offsets = Vec::with_capacity(num_cols);
                    for &col_tydesc in col_tydescs.iter() {
                        let field_align = (*col_tydesc).align;
                        let field_size = (*col_tydesc).size;
                        row_max_align = row_max_align.max(field_align);
                        row_offset = rtdt::layout::align_up(row_offset, field_align);
                        field_offsets.push(row_offset);
                        row_offset += field_size;
                    }
                    let row_size = rtdt::layout::align_up(row_offset, row_max_align);
                    let row_stride = if row_size == 0 { 1 } else { row_size };

                    // Create row tuple type descriptor.
                    let mut row_tuple_fields: Vec<rtdt::TyInfoTupleField> = Vec::with_capacity(num_cols);
                    for (i, &col_tydesc) in col_tydescs.iter().enumerate() {
                        row_tuple_fields.push(rtdt::TyInfoTupleField {
                            offset: field_offsets[i],
                            tydesc: col_tydesc,
                        });
                    }
                    let row_tydesc = rtdt::TyDesc {
                        type_tag: rtdt::TyTag::Tuple,
                        size: row_size,
                        align: row_max_align,
                        type_info: rtdt::TyInfo {
                            tuple: rtdt::TyInfoTuple {
                                num_fields: num_cols as u32,
                                fields: row_tuple_fields.as_ptr(),
                            },
                        },
                    };

                    // Allocate buffer for ALL rows.
                    let num_rows = rows.len();
                    let mut rows_buffer = vec![0u8; (row_stride as usize * num_rows).max(8)];

                    // Write each row at its offset in the buffer.
                    for (row_idx, row_values) in rows.iter().enumerate() {
                        let row_base_offset = row_idx * row_stride as usize;
                        for (col_idx, col_value) in row_values.iter().enumerate() {
                            let col_offset = row_base_offset + field_offsets[col_idx] as usize;
                            let col_dest = Destination {
                                ptr: rows_buffer.as_mut_ptr().add(col_offset),
                                tydesc: col_tydescs[col_idx],
                            };
                            self.write_const(col_value, col_dest);
                        }
                    }

                    // Build table from rows.
                    datalove_rt::c::dtlv_rti_table_build_from_rows_local(
                        self.runtime.handle(),
                        dest.ptr,
                        dest.tydesc,
                        rows_buffer.as_mut_ptr(),
                        &row_tydesc,
                        num_rows as rtdt::IndexRepr,
                    );
                }
                ConstValue::ResultErr(inner) => {
                    // Result::Err layout: tag (u8) at offset 0, Error payload at aligned offset.
                    // Err tag = 2.
                    *(dest.ptr as *mut u8) = 2;

                    // Get the Ok type from the Result tydesc to compute payload offset.
                    let tydesc_ref = rtdt::TyDescRef::from_ptr(dest.tydesc);
                    let ok_tydesc = tydesc_ref.result_ok_ty();
                    let inner_align = ok_tydesc.align();
                    let error_align = 8u32;
                    let max_align = inner_align.max(error_align);
                    let payload_offset = rtdt::layout::align_up(1, max_align);
                    let payload_ptr = dest.ptr.add(payload_offset as usize);

                    // The payload is an Error value. Create the Error tydesc and destination.
                    let error_tydesc = self.tydesc_table.get_or_create(&IrType::Error);
                    let payload_dest = Destination {
                        ptr: payload_ptr,
                        tydesc: error_tydesc,
                    };
                    self.write_const(inner, payload_dest);
                }
                ConstValue::Error(inner) => {
                    // Error is a boxed wrapper around any value.
                    let inner_ir_type = ir_type_of_const_value(inner);
                    let inner_tydesc = self.tydesc_table.get_or_create(&inner_ir_type);
                    let inner_size = (*inner_tydesc).size as usize;
                    let inner_align = (*inner_tydesc).align as usize;

                    // Allocate temp buffer with proper alignment.
                    let layout = std::alloc::Layout::from_size_align(inner_size.max(1), inner_align.max(1))
                        .expect("invalid layout for Error inner");
                    let inner_buffer = std::alloc::alloc_zeroed(layout);

                    // Write the inner value to the temp buffer.
                    let inner_dest = Destination {
                        ptr: inner_buffer,
                        tydesc: inner_tydesc,
                    };
                    self.write_const(inner, inner_dest);

                    // Now box the inner value into an Error at the destination.
                    datalove_rt::c::dtlv_rti_error_from_local(
                        self.runtime.handle(),
                        inner_buffer,
                        inner_tydesc,
                        dest.ptr,
                    );

                    // Deallocate temp buffer.
                    std::alloc::dealloc(inner_buffer, layout);
                }
                ConstValue::Data(inner) => {
                    // Data is a boxed wrapper around any value.
                    let inner_ir_type = ir_type_of_const_value(inner);
                    let inner_tydesc = self.tydesc_table.get_or_create(&inner_ir_type);
                    let inner_size = (*inner_tydesc).size as usize;
                    let inner_align = (*inner_tydesc).align as usize;

                    // Allocate temp buffer with proper alignment.
                    let layout = std::alloc::Layout::from_size_align(inner_size.max(1), inner_align.max(1))
                        .expect("invalid layout for Data inner");
                    let inner_buffer = std::alloc::alloc_zeroed(layout);

                    // Write the inner value to the temp buffer.
                    let inner_dest = Destination {
                        ptr: inner_buffer,
                        tydesc: inner_tydesc,
                    };
                    self.write_const(inner, inner_dest);

                    // Now box the inner value into a Data at the destination.
                    datalove_rt::c::dtlv_rti_data_from_local(
                        self.runtime.handle(),
                        inner_buffer,
                        inner_tydesc,
                        dest.ptr,
                    );

                    // Deallocate temp buffer.
                    std::alloc::dealloc(inner_buffer, layout);
                }
            }
        }
    }

    /// Navigate a field path to get the pointer and tydesc for a nested field.
    fn navigate_field_path(
        &self,
        base_ptr: *mut u8,
        base_tydesc: *const rtdt::TyDesc,
        field_path: &[u32],
    ) -> (*mut u8, *const rtdt::TyDesc) {
        let mut current_ptr = base_ptr;
        let mut current_tydesc = base_tydesc;

        for &field_idx in field_path {
            let tag = unsafe { (*current_tydesc).type_tag };
            match tag {
                rtdt::TyTag::Tuple => {
                    let tuple_info = unsafe { (*current_tydesc).type_info.tuple };
                    let field_info = unsafe { &*tuple_info.fields.add(field_idx as usize) };
                    current_ptr = unsafe { current_ptr.add(field_info.offset as usize) };
                    current_tydesc = field_info.tydesc;
                }
                rtdt::TyTag::Struct => {
                    let struct_info = unsafe { (*current_tydesc).type_info.struct_ };
                    let field_info = unsafe { &*struct_info.fields.add(field_idx as usize) };
                    current_ptr = unsafe { current_ptr.add(field_info.offset as usize) };
                    current_tydesc = field_info.tydesc;
                }
                _ => unreachable!("field path element requires tuple or struct type, got {:?}", tag),
            }
        }

        (current_ptr, current_tydesc)
    }

    unsafe fn copy_value(&self, src: &Value, dest: Destination) {
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
    }

    unsafe fn move_value(&self, src: &Value, dest: Destination) {
        unsafe {
            // Move is always a shallow copy - ownership transfers to dest.
            // The source should be marked as dropped so it won't be destroyed.
            let size = (*src.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(src.ptr, dest.ptr, size);
        }
    }

    /// Execute Drop: run destructor for a value.
    fn execute_drop(&mut self, val: &Value) {
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.runtime.handle(),
                val.ptr,
                val.tydesc,
            );
        }
    }
}

impl Default for IrInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

/// Infer an IrType from a ConstValue.
///
/// This is used to create tydescs when writing ConstValue::Data or other
/// dynamically-typed values.
fn ir_type_of_const_value(value: &ConstValue) -> IrType {
    match value {
        ConstValue::Unit => IrType::Unit,
        ConstValue::Bool(_) => IrType::Bool,
        ConstValue::U8(_) => IrType::U8,
        ConstValue::U16(_) => IrType::U16,
        ConstValue::U32(_) => IrType::U32,
        ConstValue::U64(_) => IrType::U64,
        ConstValue::I8(_) => IrType::I8,
        ConstValue::I16(_) => IrType::I16,
        ConstValue::I32(_) => IrType::I32,
        ConstValue::I64(_) => IrType::I64,
        ConstValue::Index(_) => IrType::Index,
        ConstValue::Offset(_) => IrType::Offset,
        ConstValue::Int { .. } => IrType::Int,
        ConstValue::F32(_) => IrType::F32,
        ConstValue::F64(_) => IrType::F64,
        ConstValue::String(_) => IrType::String,
        ConstValue::Tuple(fields) => {
            IrType::Tuple(fields.iter().map(ir_type_of_const_value).collect())
        }
        ConstValue::Struct(fields) => {
            IrType::Struct(
                fields.iter()
                    .map(|(name, v)| (name.clone(), ir_type_of_const_value(v)))
                    .collect()
            )
        }
        ConstValue::Enum { variant, payload } => {
            // For enum, we can only infer a single-variant enum type.
            let payload_type = payload.as_ref().map(|p| ir_type_of_const_value(p));
            IrType::Enum(vec![(variant.clone(), payload_type)])
        }
        ConstValue::OptionNone => {
            // Cannot fully infer the inner type for None; default to Unit.
            IrType::Option(Box::new(IrType::Unit))
        }
        ConstValue::OptionSome(inner) => {
            IrType::Option(Box::new(ir_type_of_const_value(inner)))
        }
        ConstValue::ResultOk(inner) => {
            IrType::Result(Box::new(ir_type_of_const_value(inner)))
        }
        ConstValue::ResultErr(_) => {
            // Result::Err - cannot infer Ok type from Err; default to Unit.
            IrType::Result(Box::new(IrType::Unit))
        }
        ConstValue::Data(_) => IrType::Data,
        ConstValue::Error(_) => IrType::Error,
        ConstValue::List(elements) => {
            let elem_type = elements.first()
                .map(ir_type_of_const_value)
                .unwrap_or(IrType::Unit);
            IrType::List(Box::new(elem_type))
        }
        ConstValue::Set(elements) => {
            let elem_type = elements.first()
                .map(ir_type_of_const_value)
                .unwrap_or(IrType::Unit);
            IrType::Set(Box::new(elem_type))
        }
        ConstValue::Map(entries) => {
            let (key_type, value_type) = entries.first()
                .map(|(k, v)| (ir_type_of_const_value(k), ir_type_of_const_value(v)))
                .unwrap_or((IrType::Unit, IrType::Unit));
            IrType::Map(Box::new(key_type), Box::new(value_type))
        }
        ConstValue::Table { columns, rows } => {
            // Infer column types from first row if available.
            let col_types: Vec<(String, Box<IrType>)> = if let Some(first_row) = rows.first() {
                columns.iter()
                    .zip(first_row.iter())
                    .map(|(name, value)| (name.clone(), Box::new(ir_type_of_const_value(value))))
                    .collect()
            } else {
                columns.iter().map(|name| (name.clone(), Box::new(IrType::Unit))).collect()
            };
            IrType::Table(col_types)
        }
    }
}
