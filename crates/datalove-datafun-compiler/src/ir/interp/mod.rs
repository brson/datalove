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

#[cfg(test)]
mod tests;

pub use error::InterpError;
pub use value::{Value, Destination};
pub use layout::IrLayout;
pub use tydesc::IrTyDescTable;
pub use frame::{Frame, FrameStore};
pub use env::{FunctionRegistry, ScriptEnvironment, ExecutionContext};

use datalove_rt::rtdt::{self, TyDescRef};
use super::{
    IrFunction, IrScriptUnit, IrBlock, IrType, Instruction, Terminator,
    ValueId, BlockId, Operand, SlotDest, ConstValue, BinOp, UnaryOp,
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
}

impl IrInterpreter {
    pub fn new() -> Self {
        Self {
            runtime: datalove_rt::rust::Runtime::new(),
            tydesc_table: IrTyDescTable::new(),
        }
    }

    /// Get the runtime handle for memory management.
    pub fn runtime_handle(&self) -> datalove_rt::c::LocalRtHandle {
        self.runtime.handle()
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
        // Compute layout.
        let layout = IrLayout::compute(
            &func.value_types,
            &func.slot_types,
            &mut self.tydesc_table,
        );

        // Create frame.
        let mut frame = Frame::new(layout);

        // Move arguments into parameter slots.
        // In linear type system, args are consumed by the call.
        for (i, &param_id) in func.params.iter().enumerate() {
            if i < args.len() {
                let dest = frame.value_dest(param_id)?;
                let src = &args[i];
                unsafe {
                    self.move_value(src, dest)?;
                }
                frame.mark_value_initialized(param_id);
            }
        }

        // Execute blocks, writing return value directly to ret_dest.
        // Functions use ret_dest for Return/TryReturn, not expr_dest.
        let result = self.execute_blocks(&func.blocks, &mut frame, ret_dest, None, ctx, registry, frames);

        // Note: We no longer call frame.destroy_all() here. IR has precise drops
        // via drop_analysis, so all values should be explicitly dropped before
        // return. The leak checker will catch any issues.

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

        // Create frame.
        let mut frame = Frame::new(layout);

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
        let mut prev_block: Option<BlockId> = None;

        loop {
            let block = blocks.iter()
                .find(|b| b.id == current_block)
                .ok_or(InterpError::BlockNotFound(current_block))?;

            // Execute instructions.
            for instr in &block.instructions {
                match instr {
                    Instruction::Phi { dest, incoming } => {
                        let pred = prev_block.expect("phi in entry block");
                        self.execute_phi(*dest, incoming, pred, frame, frames)?;
                    }
                    _ => {
                        self.execute_instruction(instr, frame, ret_dest, ctx, registry, frames)?;
                    }
                }
            }

            // Handle terminator.
            prev_block = Some(current_block);
            match &block.terminator {
                Terminator::Goto(target) => {
                    current_block = *target;
                }
                Terminator::Branch { cond, then_block, else_block } => {
                    let cond_val = self.read_operand(cond, frame, frames)?;
                    let cond_bool = unsafe { *(cond_val.ptr as *const bool) };
                    current_block = if cond_bool { *then_block } else { *else_block };
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
                Terminator::TryReturn { value } => {
                    if let Some(op) = value {
                        let val = self.read_operand(op, frame, frames)?;
                        // Use move_value (shallow copy).
                        unsafe { self.move_value(&val, ret_dest)?; }
                        // Mark source as dropped.
                        match op {
                            Operand::Value(id) => frame.mark_value_dropped(*id),
                            Operand::Slot(id) => frame.mark_slot_dropped(*id),
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
                    // Write to ret_dest (Result<(), Error> type).
                    unsafe { self.move_value(&val, ret_dest)?; }
                    match value {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
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

    fn execute_phi(
        &mut self,
        dest: ValueId,
        incoming: &[(BlockId, Operand)],
        pred: BlockId,
        frame: &mut Frame,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        // Find the operand corresponding to the predecessor block.
        let operand = incoming.iter()
            .find(|(block, _)| *block == pred)
            .map(|(_, op)| op)
            .ok_or_else(|| InterpError::PhiMissingPredecessor {
                dest,
                pred,
                available: incoming.iter().map(|(b, _)| *b).collect(),
            })?;

        let src_val = self.read_operand(operand, frame, frames)?;
        let dest_slot = frame.value_dest(dest)?;
        // Phi uses move semantics - the value from the taken edge is consumed.
        unsafe { self.move_value(&src_val, dest_slot)?; }
        frame.mark_value_initialized(dest);

        // Mark source as dropped.
        match operand {
            Operand::Value(id) => frame.mark_value_dropped(*id),
            Operand::Slot(id) => frame.mark_slot_dropped(*id),
            Operand::ExternalValue { unit, value } => {
                frames.mark_external_value_dropped(*unit, *value);
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.mark_external_slot_dropped(*unit, *slot);
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
                            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                        }
                    }
                }
            }
            Instruction::SlotLoad { dest, slot } => {
                let slot_val = frame.slot(*slot)?;
                let dest_slot = frame.value_dest(*dest)?;
                // Move value from slot (consuming). Used for contexts where the
                // slot value is being consumed (function args, return, etc.).
                // For borrowing contexts (binop/unaryop), use Operand::Slot directly.
                unsafe { self.move_value(&slot_val, dest_slot)?; }
                frame.mark_value_initialized(*dest);
                // Mark slot as dropped since we moved the value out.
                frame.mark_slot_dropped(*slot);
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
            Instruction::TupleIndex { dest, base, index } => {
                let base_val = self.read_operand(base, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_tuple_index(&base_val, *index, dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::FieldAccess { dest, base, field_index } => {
                let base_val = self.read_operand(base, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_field_access(&base_val, *field_index, dest_slot)?;
                frame.mark_value_initialized(*dest);
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
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::WrapNone { dest } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_wrap_none(dest_slot)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                let src_val = self.read_operand(src, frame, frames)?;
                let dest_slot = frame.value_dest(*dest)?;
                let is_some_slot = frame.value_dest(*is_some)?;
                self.execute_unwrap_option(&src_val, dest_slot, is_some_slot)?;
                frame.mark_value_initialized(*dest);
                frame.mark_value_initialized(*is_some);
                // Mark source as consumed - Option is destructured.
                match src {
                    Operand::Value(id) => frame.mark_value_dropped(*id),
                    Operand::Slot(id) => frame.mark_slot_dropped(*id),
                    Operand::ExternalValue { unit, value } => {
                        frames.mark_external_value_dropped(*unit, *value);
                    }
                    Operand::ExternalSlot { unit, slot } => {
                        frames.mark_external_slot_dropped(*unit, *slot);
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
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::Call { dest, func, args } => {
                // Look up the function and determine the correct context for the callee.
                let (callee, callee_unit) = ctx.get_function_with_context(func, registry)?;

                // Evaluate arguments.
                let arg_vals: Vec<Value> = args.iter()
                    .map(|op| self.read_operand(op, frame, frames))
                    .collect::<Result<_, _>>()?;

                // Get destination for return value.
                let dest_slot = frame.value_dest(*dest)?;

                // Mark arg sources as dropped BEFORE call - they're moved immediately.
                // Must do this before call_in_context because callee destroys them.
                for arg in args {
                    match arg {
                        Operand::Value(id) => frame.mark_value_dropped(*id),
                        Operand::Slot(id) => frame.mark_slot_dropped(*id),
                        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                    }
                }

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
                frame.mark_value_initialized(*dest);
            }
            Instruction::ListNew { dest, elements } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_list_new(elements, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::SetNew { dest, elements } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_set_new(elements, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::MapNew { dest, entries } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_map_new(entries, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::TensorNew { dest, shape, elements } => {
                let dest_slot = frame.value_dest(*dest)?;
                self.execute_tensor_new(shape, elements, dest_slot, frame, frames)?;
                frame.mark_value_initialized(*dest);
            }
            Instruction::Phi { .. } => {
                // Phi nodes are handled separately in execute_blocks before other instructions.
                // This branch should not be reached since we skip Phi in the instruction loop.
                unreachable!("Phi instructions are handled separately in execute_blocks")
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
                    // External values/slots are in other frames, handled separately.
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
                }
            }
            Instruction::Nop => {}
        }
        Ok(())
    }

    fn read_operand(
        &self,
        op: &Operand,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<Value, InterpError> {
        match op {
            Operand::Value(id) => frame.value(*id),
            Operand::Slot(id) => frame.slot(*id),
            Operand::ExternalValue { unit, value } => {
                frames.external_value(*unit, *value)
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.external_slot(*unit, *slot)
            }
        }
    }

    fn write_const(&mut self, value: &ConstValue, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            match value {
                ConstValue::Unit => {
                    // Unit is zero-sized, nothing to write.
                }
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
                        // Zero.
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

    /// Check if a type tag is a fixed-width integer (u8-u64, i8-i64).
    fn is_fixed_width_int(tag: rtdt::TyTag) -> bool {
        matches!(tag,
            rtdt::TyTag::U8 | rtdt::TyTag::U16 | rtdt::TyTag::U32 | rtdt::TyTag::U64 |
            rtdt::TyTag::I8 | rtdt::TyTag::I16 | rtdt::TyTag::I32 | rtdt::TyTag::I64
        )
    }

    /// Widen a fixed-width integer value to an Int in a stack-allocated buffer.
    ///
    /// Returns the widened Int representation. The caller is responsible for
    /// destroying the Int (freeing its limbs) after use.
    unsafe fn widen_to_int(
        &self,
        src: &Value,
        int_buf: &mut rtdt::Int,
    ) -> Result<(), InterpError> {
        unsafe {
            let type_tag = (*src.tydesc).type_tag;

            // Extract magnitude and sign from the fixed-width integer.
            let (magnitude, is_negative): (u64, bool) = match type_tag {
                rtdt::TyTag::U8 => (*(src.ptr as *const u8) as u64, false),
                rtdt::TyTag::U16 => (*(src.ptr as *const u16) as u64, false),
                rtdt::TyTag::U32 => (*(src.ptr as *const u32) as u64, false),
                rtdt::TyTag::U64 => (*(src.ptr as *const u64), false),
                rtdt::TyTag::I8 => {
                    let v = *(src.ptr as *const i8);
                    if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
                }
                rtdt::TyTag::I16 => {
                    let v = *(src.ptr as *const i16);
                    if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
                }
                rtdt::TyTag::I32 => {
                    let v = *(src.ptr as *const i32);
                    if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
                }
                rtdt::TyTag::I64 => {
                    let v = *(src.ptr as *const i64);
                    if v == i64::MIN {
                        // Special case: i64::MIN cannot be negated without overflow.
                        // Its magnitude is 2^63 = 0x8000_0000_0000_0000.
                        (0x8000_0000_0000_0000u64, true)
                    } else if v < 0 {
                        ((-v) as u64, true)
                    } else {
                        (v as u64, false)
                    }
                }
                _ => return Err(InterpError::TypeMismatch(
                    format!("Cannot widen type {:?} to Int", type_tag)
                )),
            };

            let rt_handle = self.runtime.handle();

            if magnitude == 0 {
                int_buf.data = std::ptr::null();
                int_buf.size_and_sign = 0;
                int_buf.capacity = 0;
            } else if magnitude <= u32::MAX as u64 {
                // Fits in one limb.
                let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    rt_handle, 4, 4, 1
                ) as *mut u32;
                *limb_ptr = magnitude as u32;
                int_buf.data = limb_ptr;
                int_buf.size_and_sign = if is_negative { -1 } else { 1 };
                int_buf.capacity = 1;
            } else {
                // Needs two limbs (for u64/i64 values > u32::MAX).
                let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    rt_handle, 4, 4, 2
                ) as *mut u32;
                // Low limb first (little-endian limb order).
                *limb_ptr = magnitude as u32;
                *limb_ptr.add(1) = (magnitude >> 32) as u32;
                int_buf.data = limb_ptr;
                int_buf.size_and_sign = if is_negative { -2 } else { 2 };
                int_buf.capacity = 2;
            }
        }

        Ok(())
    }

    /// Destroy a temporary Int's limb allocation.
    unsafe fn destroy_temp_int(&self, int_buf: &rtdt::Int) {
        unsafe {
            if !int_buf.data.is_null() && int_buf.capacity > 0 {
                datalove_rt::c::dtlv_rti_mem_free_raw_local(
                    self.runtime.handle(),
                    4, // align
                    4, // elem_size
                    int_buf.capacity,
                    int_buf.data as *mut u8,
                );
            }
        }
    }

    fn execute_binop(
        &mut self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate binop implementations for integer types.
        macro_rules! int_binop {
            ($tag:ident, $ty:ty, $lhs:expr, $rhs:expr, $dest:expr, $op:expr) => {
                if (*$lhs.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($lhs.ptr as *const $ty);
                    let b = *($rhs.ptr as *const $ty);
                    match $op {
                        BinOp::Add => { *($dest.ptr as *mut $ty) = a.wrapping_add(b); return Ok(()); }
                        BinOp::Sub => { *($dest.ptr as *mut $ty) = a.wrapping_sub(b); return Ok(()); }
                        BinOp::Mul => { *($dest.ptr as *mut $ty) = a.wrapping_mul(b); return Ok(()); }
                        BinOp::Div => {
                            if b == 0 { return Err(InterpError::DivisionByZero); }
                            *($dest.ptr as *mut $ty) = a.wrapping_div(b);
                            return Ok(());
                        }
                        BinOp::Mod => {
                            if b == 0 { return Err(InterpError::DivisionByZero); }
                            *($dest.ptr as *mut $ty) = a.wrapping_rem(b);
                            return Ok(());
                        }
                        BinOp::Lt => { *($dest.ptr as *mut bool) = a < b; return Ok(()); }
                        BinOp::Le => { *($dest.ptr as *mut bool) = a <= b; return Ok(()); }
                        BinOp::Gt => { *($dest.ptr as *mut bool) = a > b; return Ok(()); }
                        BinOp::Ge => { *($dest.ptr as *mut bool) = a >= b; return Ok(()); }
                        BinOp::Eq => { *($dest.ptr as *mut bool) = a == b; return Ok(()); }
                        BinOp::Ne => { *($dest.ptr as *mut bool) = a != b; return Ok(()); }
                        BinOp::BitAnd => { *($dest.ptr as *mut $ty) = a & b; return Ok(()); }
                        BinOp::BitOr => { *($dest.ptr as *mut $ty) = a | b; return Ok(()); }
                        BinOp::BitXor => { *($dest.ptr as *mut $ty) = a ^ b; return Ok(()); }
                        BinOp::Shl => { *($dest.ptr as *mut $ty) = a.wrapping_shl(b as u32); return Ok(()); }
                        BinOp::Shr => { *($dest.ptr as *mut $ty) = a.wrapping_shr(b as u32); return Ok(()); }
                        _ => {}
                    }
                }
            };
        }

        unsafe {
            let lhs_tag = (*lhs.tydesc).type_tag;
            let dest_tag = (*dest.tydesc).type_tag;

            // Widening arithmetic: fixed-width int operands -> Int result.
            // This is triggered when dest is Int but operands are fixed-width ints.
            if dest_tag == rtdt::TyTag::Int && Self::is_fixed_width_int(lhs_tag) {
                use datalove_rt::c::RtStatus;

                // Allocate temporary Ints on the stack for widened operands.
                let mut lhs_int = std::mem::MaybeUninit::<rtdt::Int>::uninit();
                let mut rhs_int = std::mem::MaybeUninit::<rtdt::Int>::uninit();

                self.widen_to_int(lhs, lhs_int.assume_init_mut())?;
                self.widen_to_int(rhs, rhs_int.assume_init_mut())?;

                let lhs_int = lhs_int.assume_init();
                let rhs_int = rhs_int.assume_init();

                let rt_handle = self.runtime.handle();
                let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

                let lhs_ptr = &lhs_int as *const rtdt::Int as *mut u8;
                let rhs_ptr = &rhs_int as *const rtdt::Int as *mut u8;

                let status = match op {
                    BinOp::Add => datalove_rt::c::dtlv_rti_int_add(
                        rt_handle,
                        lhs_ptr, int_tydesc,
                        rhs_ptr, int_tydesc,
                        dest.ptr, int_tydesc,
                    ),
                    BinOp::Sub => datalove_rt::c::dtlv_rti_int_sub(
                        rt_handle,
                        lhs_ptr, int_tydesc,
                        rhs_ptr, int_tydesc,
                        dest.ptr, int_tydesc,
                    ),
                    BinOp::Mul => datalove_rt::c::dtlv_rti_int_mul(
                        rt_handle,
                        lhs_ptr, int_tydesc,
                        rhs_ptr, int_tydesc,
                        dest.ptr, int_tydesc,
                    ),
                    _ => {
                        // Clean up temporaries before returning error.
                        self.destroy_temp_int(&lhs_int);
                        self.destroy_temp_int(&rhs_int);
                        return Err(InterpError::TypeMismatch(
                            format!("widening arithmetic not supported for {:?}", op)
                        ));
                    }
                };

                // Clean up temporary Int allocations.
                self.destroy_temp_int(&lhs_int);
                self.destroy_temp_int(&rhs_int);

                if status != RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        format!("widening Int {:?} operation failed", op)
                    ));
                }
                return Ok(());
            }

            // Same-type operations: fixed-width int operands with fixed-width int result.
            let tag = lhs_tag;

            // Try all integer types.
            int_binop!(I8, i8, lhs, rhs, dest, op);
            int_binop!(I16, i16, lhs, rhs, dest, op);
            int_binop!(I32, i32, lhs, rhs, dest, op);
            int_binop!(I64, i64, lhs, rhs, dest, op);
            int_binop!(U8, u8, lhs, rhs, dest, op);
            int_binop!(U16, u16, lhs, rhs, dest, op);
            int_binop!(U32, u32, lhs, rhs, dest, op);
            int_binop!(U64, u64, lhs, rhs, dest, op);

            // Bigint operations via runtime (when operands are already Int).
            if tag == rtdt::TyTag::Int {
                use datalove_rt::c::RtStatus;

                let rt_handle = self.runtime.handle();
                let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

                let status = match op {
                    BinOp::Add => datalove_rt::c::dtlv_rti_int_add(
                        rt_handle,
                        lhs.ptr, int_tydesc,
                        rhs.ptr, int_tydesc,
                        dest.ptr, int_tydesc,
                    ),
                    BinOp::Sub => datalove_rt::c::dtlv_rti_int_sub(
                        rt_handle,
                        lhs.ptr, int_tydesc,
                        rhs.ptr, int_tydesc,
                        dest.ptr, int_tydesc,
                    ),
                    BinOp::Mul => datalove_rt::c::dtlv_rti_int_mul(
                        rt_handle,
                        lhs.ptr, int_tydesc,
                        rhs.ptr, int_tydesc,
                        dest.ptr, int_tydesc,
                    ),
                    BinOp::Div => {
                        let status = datalove_rt::c::dtlv_rti_int_div_checked(
                            rt_handle,
                            lhs.ptr, int_tydesc,
                            rhs.ptr, int_tydesc,
                            dest.ptr, int_tydesc,
                        );
                        if status != RtStatus::Ok {
                            return Err(InterpError::DivisionByZero);
                        }
                        return Ok(());
                    }
                    // Int comparison operations via runtime.
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        let cmp = datalove_rt::c::dtlv_rti_int_cmp(lhs.ptr, rhs.ptr);
                        let result = match op {
                            BinOp::Eq => cmp == 0,
                            BinOp::Ne => cmp != 0,
                            BinOp::Lt => cmp < 0,
                            BinOp::Le => cmp <= 0,
                            BinOp::Gt => cmp > 0,
                            BinOp::Ge => cmp >= 0,
                            _ => unreachable!(),
                        };
                        *(dest.ptr as *mut bool) = result;
                        return Ok(());
                    }
                    _ => return Err(InterpError::TypeMismatch(
                        format!("unsupported Int binop {:?}", op)
                    )),
                };

                if status != RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        format!("Int {:?} operation failed", op)
                    ));
                }
                return Ok(());
            }

            // F32 operations.
            if tag == rtdt::TyTag::F32 {
                let a = *(lhs.ptr as *const f32);
                let b = *(rhs.ptr as *const f32);
                match op {
                    BinOp::Add => { *(dest.ptr as *mut f32) = a + b; return Ok(()); }
                    BinOp::Sub => { *(dest.ptr as *mut f32) = a - b; return Ok(()); }
                    BinOp::Mul => { *(dest.ptr as *mut f32) = a * b; return Ok(()); }
                    BinOp::Div => { *(dest.ptr as *mut f32) = a / b; return Ok(()); }
                    BinOp::Lt => { *(dest.ptr as *mut bool) = a < b; return Ok(()); }
                    BinOp::Le => { *(dest.ptr as *mut bool) = a <= b; return Ok(()); }
                    BinOp::Gt => { *(dest.ptr as *mut bool) = a > b; return Ok(()); }
                    BinOp::Ge => { *(dest.ptr as *mut bool) = a >= b; return Ok(()); }
                    BinOp::Eq => { *(dest.ptr as *mut bool) = a == b; return Ok(()); }
                    BinOp::Ne => { *(dest.ptr as *mut bool) = a != b; return Ok(()); }
                    _ => {}
                }
            }

            // Boolean operations.
            if tag == rtdt::TyTag::Bool {
                let a = *(lhs.ptr as *const bool);
                let b = *(rhs.ptr as *const bool);
                match op {
                    BinOp::And => { *(dest.ptr as *mut bool) = a && b; return Ok(()); }
                    BinOp::Or => { *(dest.ptr as *mut bool) = a || b; return Ok(()); }
                    BinOp::Eq => { *(dest.ptr as *mut bool) = a == b; return Ok(()); }
                    BinOp::Ne => { *(dest.ptr as *mut bool) = a != b; return Ok(()); }
                    _ => {}
                }
            }

            Err(InterpError::TypeMismatch(
                format!("unsupported binop {:?} for type {:?}", op, tag)
            ))
        }
    }

    fn execute_unaryop(
        &mut self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate unaryop implementations for signed integer types.
        macro_rules! signed_int_unaryop {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $op:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    match $op {
                        UnaryOp::Neg => { *($dest.ptr as *mut $ty) = a.wrapping_neg(); return Ok(()); }
                        UnaryOp::BitNot => { *($dest.ptr as *mut $ty) = !a; return Ok(()); }
                        UnaryOp::Not => {}
                    }
                }
            };
        }

        // Macro for unsigned integers (only BitNot).
        macro_rules! unsigned_int_unaryop {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $op:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    match $op {
                        UnaryOp::BitNot => { *($dest.ptr as *mut $ty) = !a; return Ok(()); }
                        _ => {}
                    }
                }
            };
        }

        unsafe {
            let tag = (*src.tydesc).type_tag;

            // Signed integer negation and bitnot.
            signed_int_unaryop!(I8, i8, src, dest, op);
            signed_int_unaryop!(I16, i16, src, dest, op);
            signed_int_unaryop!(I32, i32, src, dest, op);
            signed_int_unaryop!(I64, i64, src, dest, op);

            // Unsigned integer bitnot.
            unsigned_int_unaryop!(U8, u8, src, dest, op);
            unsigned_int_unaryop!(U16, u16, src, dest, op);
            unsigned_int_unaryop!(U32, u32, src, dest, op);
            unsigned_int_unaryop!(U64, u64, src, dest, op);

            // F32 negation.
            if tag == rtdt::TyTag::F32 && op == UnaryOp::Neg {
                let a = *(src.ptr as *const f32);
                *(dest.ptr as *mut f32) = -a;
                return Ok(());
            }

            // Boolean not.
            if tag == rtdt::TyTag::Bool && op == UnaryOp::Not {
                let a = *(src.ptr as *const bool);
                *(dest.ptr as *mut bool) = !a;
                return Ok(());
            }

            // Bigint negation.
            if tag == rtdt::TyTag::Int && op == UnaryOp::Neg {
                use datalove_rt::c::RtStatus;

                let rt_handle = self.runtime.handle();
                let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

                let status = datalove_rt::c::dtlv_rti_int_neg(
                    rt_handle,
                    src.ptr, int_tydesc,
                    dest.ptr, int_tydesc,
                );
                if status != RtStatus::Ok {
                    return Err(InterpError::RuntimeError(
                        "Int negation failed".to_string()
                    ));
                }
                return Ok(());
            }

            Err(InterpError::TypeMismatch(
                format!("unsupported unaryop {:?} for type {:?}", op, tag)
            ))
        }
    }

    /// Pack fields into a tuple at destination.
    fn execute_pack_tuple(
        &mut self,
        fields: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let tuple_info = (*dest.tydesc).type_info.tuple;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*tuple_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
        Ok(())
    }

    /// Pack fields into a struct at destination.
    fn execute_pack_struct(
        &mut self,
        fields: &[Value],
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let struct_info = (*dest.tydesc).type_info.struct_;
            for (i, field) in fields.iter().enumerate() {
                let field_info = &*struct_info.fields.add(i);
                let field_dest = dest.ptr.add(field_info.offset as usize);
                let size = (*field.tydesc).size as usize;
                std::ptr::copy_nonoverlapping(field.ptr, field_dest, size);
            }
        }
        Ok(())
    }

    /// Access a tuple field.
    fn execute_tuple_index(
        &self,
        base: &Value,
        index: u32,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let tuple_info = (*base.tydesc).type_info.tuple;
            if index >= tuple_info.num_fields {
                return Err(InterpError::RuntimeError(
                    format!("tuple index {} out of bounds (len {})", index, tuple_info.num_fields)
                ));
            }
            let field_info = &*tuple_info.fields.add(index as usize);
            let field_ptr = base.ptr.add(field_info.offset as usize);
            let size = (*field_info.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(field_ptr, dest.ptr, size);
        }
        Ok(())
    }

    /// Access a struct field by index.
    fn execute_field_access(
        &self,
        base: &Value,
        field_index: u32,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let struct_info = (*base.tydesc).type_info.struct_;
            if field_index >= struct_info.num_fields {
                return Err(InterpError::RuntimeError(
                    format!("field index {} out of bounds (len {})", field_index, struct_info.num_fields)
                ));
            }
            let field_info = &*struct_info.fields.add(field_index as usize);
            let field_ptr = base.ptr.add(field_info.offset as usize);
            let size = (*field_info.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(field_ptr, dest.ptr, size);
        }
        Ok(())
    }

    /// Execute checked arithmetic operation.
    fn execute_binop_checked(
        &self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
        overflow_dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate checked binop implementations for integer types.
        macro_rules! checked_int_binop {
            ($tag:ident, $ty:ty, $lhs:expr, $rhs:expr, $dest:expr, $overflow:expr, $op:expr) => {
                if (*$lhs.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($lhs.ptr as *const $ty);
                    let b = *($rhs.ptr as *const $ty);
                    let (result, overflowed) = match $op {
                        BinOp::Add => a.overflowing_add(b),
                        BinOp::Sub => a.overflowing_sub(b),
                        BinOp::Mul => a.overflowing_mul(b),
                        BinOp::Div => {
                            // checked_div returns None on div-by-zero or overflow.
                            match a.checked_div(b) {
                                Some(r) => (r, false),
                                None => (0 as $ty, true),
                            }
                        }
                        _ => return Err(InterpError::TypeMismatch(
                            format!("checked binop only supports Add/Sub/Mul/Div, got {:?}", $op)
                        )),
                    };
                    *($dest.ptr as *mut $ty) = result;
                    *($overflow.ptr as *mut bool) = overflowed;
                    return Ok(());
                }
            };
        }

        unsafe {
            checked_int_binop!(I8, i8, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I16, i16, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I32, i32, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I64, i64, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U8, u8, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U16, u16, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U32, u32, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U64, u64, lhs, rhs, dest, overflow_dest, op);

            let tag = (*lhs.tydesc).type_tag;
            Err(InterpError::TypeMismatch(
                format!("unsupported checked binop {:?} for type {:?}", op, tag)
            ))
        }
    }

    /// Execute checked unary operation (e.g., negation with overflow detection).
    fn execute_unaryop_checked(
        &self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
        overflow_dest: Destination,
    ) -> Result<(), InterpError> {
        // Only negation can overflow for signed integers.
        if op != UnaryOp::Neg {
            return Err(InterpError::TypeMismatch(
                format!("checked unaryop only supports Neg, got {:?}", op)
            ));
        }

        // Macro to generate checked negation for signed integer types.
        macro_rules! checked_signed_neg {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $overflow:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    let (result, overflowed) = a.overflowing_neg();
                    *($dest.ptr as *mut $ty) = result;
                    *($overflow.ptr as *mut bool) = overflowed;
                    return Ok(());
                }
            };
        }

        unsafe {
            checked_signed_neg!(I8, i8, src, dest, overflow_dest);
            checked_signed_neg!(I16, i16, src, dest, overflow_dest);
            checked_signed_neg!(I32, i32, src, dest, overflow_dest);
            checked_signed_neg!(I64, i64, src, dest, overflow_dest);

            let tag = (*src.tydesc).type_tag;
            Err(InterpError::TypeMismatch(
                format!("checked negation only supported for signed integers, got {:?}", tag)
            ))
        }
    }

    /// Wrap a value in Some.
    fn execute_wrap_some(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let option_info = (*dest.tydesc).type_info.option;
            let layout = rtdt::layout::compute_option_layout(TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Some (1).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::Some as u8;
            // Copy inner value.
            let inner_size = (*option_info.inner_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Create a None value.
    fn execute_wrap_none(&self, dest: Destination) -> Result<(), InterpError> {
        unsafe {
            // Tag is at offset 0. Set to None (0).
            *(dest.ptr as *mut u8) = rtdt::OptionTag::None as u8;
        }
        Ok(())
    }

    /// Unwrap an Option, producing (inner_value, is_some).
    fn execute_unwrap_option(
        &self,
        src: &Value,
        dest: Destination,
        is_some_dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let option_info = (*src.tydesc).type_info.option;
            let layout = rtdt::layout::compute_option_layout(TyDescRef::from_ptr(src.tydesc));
            // Tag is at offset 0.
            let tag = *(src.ptr as *const u8);
            let is_some = tag != rtdt::OptionTag::None as u8;
            *(is_some_dest.ptr as *mut bool) = is_some;
            if is_some {
                let inner_size = (*option_info.inner_tydesc).size as usize;
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    dest.ptr,
                    inner_size,
                );
            }
        }
        Ok(())
    }

    /// Wrap a value in Ok.
    fn execute_wrap_ok(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let result_info = (*dest.tydesc).type_info.result;
            let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Ok.
            *(dest.ptr as *mut u8) = rtdt::ResultTag::Ok as u8;
            // Copy inner value.
            let inner_size = (*result_info.ok_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Wrap a value in Err.
    fn execute_wrap_err(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ptr(dest.tydesc));
            // Tag is at offset 0. Set to Err.
            *(dest.ptr as *mut u8) = rtdt::ResultTag::Err as u8;
            // Copy Error value.
            let inner_size = (*inner.tydesc).size as usize;
            std::ptr::copy_nonoverlapping(
                inner.ptr,
                dest.ptr.add(layout.payload_offset as usize),
                inner_size,
            );
        }
        Ok(())
    }

    /// Unwrap a Result, producing (ok_value, err_value, is_ok).
    ///
    /// - ok_dest: receives Ok payload when is_ok=true
    /// - err_dest: receives Error when is_ok=false
    fn execute_unwrap_result(
        &self,
        src: &Value,
        ok_dest: Destination,
        err_dest: Destination,
        is_ok_dest: Destination,
    ) -> Result<(), InterpError> {
        unsafe {
            let result_info = (*src.tydesc).type_info.result;
            let layout = rtdt::layout::compute_result_layout(TyDescRef::from_ptr(src.tydesc));
            // Tag is at offset 0.
            let tag = *(src.ptr as *const u8);
            let is_ok = tag == rtdt::ResultTag::Ok as u8;
            *(is_ok_dest.ptr as *mut bool) = is_ok;
            // Copy payload to appropriate destination.
            if is_ok {
                let ok_size = (*result_info.ok_tydesc).size as usize;
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    ok_dest.ptr,
                    ok_size,
                );
            } else {
                let err_size = std::mem::size_of::<rtdt::Error>();
                std::ptr::copy_nonoverlapping(
                    src.ptr.add(layout.payload_offset as usize),
                    err_dest.ptr,
                    err_size,
                );
            }
        }
        Ok(())
    }

    /// Create Error from any value (consumes inner - linear semantics).
    fn execute_error_from(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        let rt_handle = self.runtime.handle();
        let inner_size = unsafe { (*inner.tydesc).size as usize };

        // Allocate heap storage for the inner value.
        let moved_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner.tydesc, 1)
        };
        if moved_ptr.is_null() {
            return Err(InterpError::RuntimeError(
                "Failed to allocate Error inner storage".to_string()
            ));
        }

        // Move inner value to heap storage (bitwise copy).
        unsafe {
            std::ptr::copy_nonoverlapping(inner.ptr, moved_ptr, inner_size);
        }

        // Write Error struct to destination.
        // Error has same layout as Data, so we use Data::from_pointers and transmute.
        unsafe {
            let data = rtdt::Data::from_pointers(inner.tydesc, moved_ptr);
            std::ptr::write(
                dest.ptr as *mut rtdt::Error,
                std::mem::transmute(data)
            );
        }

        Ok(())
    }

    /// Create Data from any value (consumes inner - linear semantics).
    fn execute_data_from(
        &self,
        inner: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        let rt_handle = self.runtime.handle();
        let inner_size = unsafe { (*inner.tydesc).size as usize };

        // Allocate heap storage for the inner value.
        let moved_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner.tydesc, 1)
        };
        if moved_ptr.is_null() {
            return Err(InterpError::RuntimeError(
                "Failed to allocate Data inner storage".to_string()
            ));
        }

        // Move inner value to heap storage (bitwise copy).
        unsafe {
            std::ptr::copy_nonoverlapping(inner.ptr, moved_ptr, inner_size);
        }

        // Write Data struct to destination.
        unsafe {
            let data = rtdt::Data::from_pointers(inner.tydesc, moved_ptr);
            std::ptr::write(dest.ptr as *mut rtdt::Data, data);
        }

        Ok(())
    }

    /// Execute ListNew: create a list from operands.
    fn execute_list_new(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let list_ptr = dest.ptr;
        let list_tydesc = dest.tydesc;

        // Get element tydesc from list tydesc.
        let list_tydesc_ref = unsafe { TyDescRef::from_ptr(list_tydesc) };
        let element_tydesc = list_tydesc_ref.list_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };

        // Create empty list at dest.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_create_local(rt_handle, list_ptr, list_tydesc)
        };
        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to create list".to_string()));
        }

        if elements.is_empty() {
            return Ok(());
        }

        // Reserve capacity for all elements.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_list_reserve_local(
                rt_handle,
                list_ptr,
                list_tydesc,
                elements.len() as u32,
            )
        };
        if status != RtStatus::Ok {
            unsafe {
                datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, list_tydesc);
            }
            return Err(InterpError::RuntimeError("Failed to reserve list capacity".to_string()));
        }

        // Copy each element into the list's data buffer.
        for (i, elem_op) in elements.iter().enumerate() {
            let elem_val = self.read_operand(elem_op, frame, frames)?;

            // Get pointer to element slot in list's data buffer.
            let data_ptr = unsafe { (*(list_ptr as *const rtdt::List)).data as *mut u8 };
            let elem_dest_ptr = unsafe { data_ptr.add(i * element_size) };

            // Copy element value into list.
            unsafe {
                std::ptr::copy_nonoverlapping(elem_val.ptr, elem_dest_ptr, element_size);
            }

            // Update list size.
            unsafe {
                let list = list_ptr as *mut rtdt::List;
                (*list).size = (i + 1) as u32;
            }
        }

        Ok(())
    }

    /// Execute SetNew: create a set from operands.
    fn execute_set_new(
        &mut self,
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let set_ptr = dest.ptr;
        let set_tydesc = dest.tydesc;

        // Get element tydesc from set tydesc.
        let set_tydesc_ref = unsafe { TyDescRef::from_ptr(set_tydesc) };
        let element_tydesc = set_tydesc_ref.set_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };
        let element_align = unsafe { (*element_tydesc).align };

        if elements.is_empty() {
            // Create empty set.
            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreeset_create_local(rt_handle, set_ptr, set_tydesc)
            };
            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError("Failed to create empty set".to_string()));
            }
            return Ok(());
        }

        // Read all element values.
        let mut elem_values: Vec<Value> = elements.iter()
            .map(|op| self.read_operand(op, frame, frames))
            .collect::<Result<_, _>>()?;

        // Sort elements using runtime comparison.
        elem_values.sort_by(|a, b| {
            unsafe {
                let result = datalove_rt::c::dtlv_rti_cmp_total_local(
                    rt_handle,
                    a.ptr,
                    element_tydesc,
                    b.ptr,
                    element_tydesc,
                );
                match result {
                    datalove_rt::c::RtOrdering::Less => std::cmp::Ordering::Less,
                    datalove_rt::c::RtOrdering::Greater => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            }
        });

        // Allocate temporary buffer for sorted elements.
        let buffer_size = (elem_values.len() * element_size) as u32;
        let buffer = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, buffer_size, element_align, 1)
        };
        if buffer.is_null() {
            return Err(InterpError::RuntimeError("Failed to allocate set buffer".to_string()));
        }

        // Copy elements into buffer.
        for (i, value) in elem_values.iter().enumerate() {
            unsafe {
                let elem_dest = buffer.add(i * element_size);
                std::ptr::copy_nonoverlapping(value.ptr, elem_dest, element_size);
            }
        }

        // Build B-tree from sorted buffer.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreeset_build_from_sorted_slice_local(
                rt_handle,
                set_ptr,
                element_tydesc,
                buffer,
                elem_values.len() as u32,
            )
        };

        // Free temporary buffer.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_raw_local(
                rt_handle,
                buffer_size,
                element_align,
                1,
                buffer,
            );
        }

        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to build set B-tree".to_string()));
        }

        Ok(())
    }

    /// Execute MapNew: create a map from key-value pairs.
    fn execute_map_new(
        &mut self,
        entries: &[(Operand, Operand)],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use datalove_rt::c::RtStatus;

        let rt_handle = self.runtime.handle();
        let map_ptr = dest.ptr;
        let map_tydesc = dest.tydesc;

        // Get key and value tydescs from map tydesc.
        let map_tydesc_ref = unsafe { TyDescRef::from_ptr(map_tydesc) };
        let key_tydesc = map_tydesc_ref.map_key_ty().as_ptr();
        let value_tydesc = map_tydesc_ref.map_value_ty().as_ptr();
        let key_size = unsafe { (*key_tydesc).size as usize };
        let value_size = unsafe { (*value_tydesc).size as usize };
        let key_align = unsafe { (*key_tydesc).align };
        let value_align = unsafe { (*value_tydesc).align };

        if entries.is_empty() {
            // Create empty map.
            let status = unsafe {
                datalove_rt::c::dtlv_rti_btreemap_create_local(rt_handle, map_ptr, map_tydesc)
            };
            if status != RtStatus::Ok {
                return Err(InterpError::RuntimeError("Failed to create empty map".to_string()));
            }
            return Ok(());
        }

        // Read all key-value pairs.
        let mut kv_pairs: Vec<(Value, Value)> = entries.iter()
            .map(|(k_op, v_op)| {
                let k = self.read_operand(k_op, frame, frames)?;
                let v = self.read_operand(v_op, frame, frames)?;
                Ok((k, v))
            })
            .collect::<Result<_, InterpError>>()?;

        // Sort by key using runtime comparison.
        kv_pairs.sort_by(|a, b| {
            unsafe {
                let result = datalove_rt::c::dtlv_rti_cmp_total_local(
                    rt_handle,
                    a.0.ptr,
                    key_tydesc,
                    b.0.ptr,
                    key_tydesc,
                );
                match result {
                    datalove_rt::c::RtOrdering::Less => std::cmp::Ordering::Less,
                    datalove_rt::c::RtOrdering::Greater => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            }
        });

        // Allocate temporary buffers for keys and values.
        let keys_buffer_size = (kv_pairs.len() * key_size) as u32;
        let values_buffer_size = (kv_pairs.len() * value_size) as u32;

        let keys_buffer = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, keys_buffer_size, key_align, 1)
        };
        if keys_buffer.is_null() {
            return Err(InterpError::RuntimeError("Failed to allocate keys buffer".to_string()));
        }

        let values_buffer = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, values_buffer_size, value_align, 1)
        };
        if values_buffer.is_null() {
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_raw_local(
                    rt_handle,
                    keys_buffer_size,
                    key_align,
                    1,
                    keys_buffer,
                );
            }
            return Err(InterpError::RuntimeError("Failed to allocate values buffer".to_string()));
        }

        // Copy keys and values into buffers.
        for (i, (key, value)) in kv_pairs.iter().enumerate() {
            unsafe {
                let key_dest = keys_buffer.add(i * key_size);
                let value_dest = values_buffer.add(i * value_size);
                std::ptr::copy_nonoverlapping(key.ptr, key_dest, key_size);
                std::ptr::copy_nonoverlapping(value.ptr, value_dest, value_size);
            }
        }

        // Build B-tree from sorted slices.
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_build_from_sorted_slices_local(
                rt_handle,
                map_ptr,
                key_tydesc,
                value_tydesc,
                keys_buffer,
                values_buffer,
                kv_pairs.len() as u32,
            )
        };

        // Free temporary buffers.
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_raw_local(
                rt_handle,
                keys_buffer_size,
                key_align,
                1,
                keys_buffer,
            );
            datalove_rt::c::dtlv_rti_mem_free_raw_local(
                rt_handle,
                values_buffer_size,
                value_align,
                1,
                values_buffer,
            );
        }

        if status != RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to build map B-tree".to_string()));
        }

        Ok(())
    }

    /// Execute TensorNew: create a tensor from shape and elements.
    fn execute_tensor_new(
        &mut self,
        shape: &[u32],
        elements: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        let rt_handle = self.runtime.handle();
        let tensor_ptr = dest.ptr;
        let tensor_tydesc = dest.tydesc;

        // Get element tydesc from tensor tydesc.
        let tensor_tydesc_ref = unsafe { TyDescRef::from_ptr(tensor_tydesc) };
        let element_tydesc = tensor_tydesc_ref.tensor_element_ty().as_ptr();
        let element_size = unsafe { (*element_tydesc).size as usize };

        let rank = shape.len();
        let total_elems: usize = shape.iter().map(|&d| d as usize).product();

        // Allocate tensor data array.
        let data_ptr = if total_elems > 0 {
            let array_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, element_tydesc, total_elems as u32)
            };
            if array_ptr.is_null() {
                return Err(InterpError::RuntimeError("Failed to allocate tensor data".to_string()));
            }

            // Copy each element into the data buffer.
            for (i, elem_op) in elements.iter().enumerate() {
                let elem_val = self.read_operand(elem_op, frame, frames)?;
                let elem_dest = unsafe { array_ptr.add(i * element_size) };
                unsafe {
                    std::ptr::copy_nonoverlapping(elem_val.ptr, elem_dest, element_size);
                }
            }
            array_ptr
        } else {
            std::ptr::null_mut()
        };

        // Allocate shape array.
        let shape_ptr = if rank > 0 {
            let shape_array = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    rt_handle,
                    std::mem::size_of::<u32>() as u32,
                    std::mem::align_of::<u32>() as u32,
                    rank as u32,
                ) as *mut u32
            };
            if shape_array.is_null() {
                // Cleanup data if allocated.
                if !data_ptr.is_null() {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, element_tydesc, total_elems as u32, data_ptr);
                    }
                }
                return Err(InterpError::RuntimeError("Failed to allocate tensor shape".to_string()));
            }
            for (i, &dim) in shape.iter().enumerate() {
                unsafe { *shape_array.add(i) = dim; }
            }
            shape_array as *const u32
        } else {
            std::ptr::null()
        };

        // Allocate and compute strides array.
        let strides_ptr = if rank > 0 {
            let strides_array = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                    rt_handle,
                    std::mem::size_of::<u32>() as u32,
                    std::mem::align_of::<u32>() as u32,
                    rank as u32,
                ) as *mut u32
            };
            if strides_array.is_null() {
                // Cleanup.
                if !data_ptr.is_null() {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, element_tydesc, total_elems as u32, data_ptr);
                    }
                }
                if !shape_ptr.is_null() {
                    unsafe {
                        datalove_rt::c::dtlv_rti_mem_free_raw_local(
                            rt_handle,
                            (rank * std::mem::size_of::<u32>()) as u32,
                            std::mem::align_of::<u32>() as u32,
                            1,
                            shape_ptr as *mut u8,
                        );
                    }
                }
                return Err(InterpError::RuntimeError("Failed to allocate tensor strides".to_string()));
            }

            // Compute strides for row-major layout.
            for i in 0..rank {
                let stride = shape[i+1..rank].iter().map(|&d| d).product::<u32>();
                unsafe { *strides_array.add(i) = if stride == 0 { 1 } else { stride }; }
            }

            strides_array as *const u32
        } else {
            std::ptr::null()
        };

        // Fill in the Tensor struct.
        unsafe {
            let tensor = tensor_ptr as *mut rtdt::Tensor;
            (*tensor).ptr_base = data_ptr;
            (*tensor).capacity_elems = total_elems as u32;
            (*tensor).offset_elems = 0;
            (*tensor).shape = shape_ptr;
            (*tensor).strides = strides_ptr;
            (*tensor).layout = rtdt::TensorLayout::RowMajor;
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
