//! The IR walker: the reference engine, which runs a body's IR an instruction
//! at a time, and runs whatever the bytecode leaves to it.
//!
//! Inside a generic a value's static type can name a type parameter; the
//! descriptor read with the value says what is really there, and the arms
//! below read layouts and widths from it rather than from the static type.

use super::*;

impl IrInterpreter {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_blocks(
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
            // Direct indexing: blocks are renumbered during lowering so blocks[i].id.0 == i.
            let block = &blocks[current_block.0 as usize];

            // Execute instructions.
            for instr in &block.instructions {
                if !self.execute_hot(instr, frame, frames)
                    && !self.execute_warm(instr, frame, frames)
                {
                    self.execute_instruction(instr, frame, ctx, registry, frames)?;
                }
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
                Terminator::Switch { discriminant, cases, default } => {
                    let disc_val = self.read_operand(discriminant, frame, frames);
                    let disc_u32 = unsafe { *(disc_val.ptr as *const u32) };
                    let target = cases.iter()
                        .find(|(v, _)| *v == disc_u32)
                        .map(|(_, b)| *b)
                        .unwrap_or(*default);
                    current_block = target;
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
    #[inline(always)]
    pub(super) fn pass_block_args(
        &mut self,
        blocks: &[IrBlock],
        target: BlockId,
        args: &[Operand],
        frame: &mut Frame,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        // Most jumps carry nothing, and should not pay for a call to say so.
        if args.is_empty() {
            return Ok(());
        }
        self.pass_block_args_nonempty(blocks, target, args, frame, frames)
    }

    #[inline(never)]
    fn pass_block_args_nonempty(
        &mut self,
        blocks: &[IrBlock],
        target: BlockId,
        args: &[Operand],
        frame: &mut Frame,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {

        // Direct indexing: blocks are renumbered so blocks[i].id.0 == i.
        let target_block = &blocks[target.0 as usize];

        // The arguments are a parallel move: each parameter gets what its
        // argument held before the jump. Moved one at a time, an argument that
        // reads another of the same block's parameters -- a loop passing its
        // carried values round, say -- would read the value just written in
        // its place, and one that passes a parameter to itself would end up
        // marked dropped. So where an argument names a parameter of the block
        // being jumped to, every argument is read out before any is written.
        let reads_target = args.iter().any(|arg| match arg {
            Operand::Value(id) | Operand::ValueRef(id) => target_block.params.contains(id),
            _ => false,
        });

        if !reads_target {
            for (param_id, arg) in target_block.params.iter().zip(args.iter()) {
                let src_val = self.read_operand(arg, frame, frames);
                let dest_slot = frame.value_dest(*param_id);
                // Block args use move semantics.
                unsafe { self.move_value(&src_val, dest_slot); }
                frame.mark_value_live(*param_id);
                Self::mark_source_dropped_all(arg, frame, frames);
            }
            return Ok(());
        }

        let mut held: Vec<Vec<u8>> = Vec::with_capacity(args.len());
        for arg in args {
            let src_val = self.read_operand(arg, frame, frames);
            let size = unsafe { (*src_val.tydesc).size as usize };
            held.push(unsafe { std::slice::from_raw_parts(src_val.ptr, size) }.to_vec());
        }
        for arg in args {
            Self::mark_source_dropped_all(arg, frame, frames);
        }
        for (param_id, bytes) in target_block.params.iter().zip(&held) {
            let dest_slot = frame.value_dest(*param_id);
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dest_slot.ptr, bytes.len()) };
            frame.mark_value_live(*param_id);
        }
        Ok(())
    }

    /// Execute `instr` if it is one of the few instructions most of a program's
    /// time is spent in, and say whether it was.
    ///
    /// Inlined into the block loop so those instructions do not pay for
    /// entering `execute_instruction`, whose frame is sized for all of the rest:
    /// its prologue and epilogue alone were about a third of its time.
    #[inline(always)]
    pub(super) fn execute_hot(&mut self, instr: &Instruction, frame: &mut Frame, frames: &mut FrameStore) -> bool {
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
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.move_value(&src_val, dest_slot); }
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_all(src, frame, frames);
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
                        // A copied type owns nothing, so whatever the slot held
                        // before needs no destroying.
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.copy_value(&src_val, dest_slot); }
                        frame.mark_slot_live(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            *unit,
                            *slot,
                            &src_val,
                        );
                    }
                }
            }
            Instruction::SlotLoadCopy { dest, slot } => {
                let slot_val = frame.slot(*slot);
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.copy_value(&slot_val, dest_slot); }
                frame.mark_value_live(*dest);
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
            Instruction::Intrinsic { dest, intrinsic, args } => {
                let dest_slot = frame.value_dest(*dest);
                self.execute_intrinsic(*intrinsic, args, dest_slot, frame, frames);
                frame.mark_value_live(*dest);
            }
            Instruction::WrapSome { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_some(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                let is_some_slot = frame.value_dest(*is_some);
                self.execute_unwrap_option(&src_val, dest_slot, is_some_slot);
                frame.mark_value_live(*dest);
                frame.mark_value_live(*is_some);
                Self::mark_source_dropped_all(src, frame, frames);
            }
            Instruction::WrapOk { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_ok(&inner_val, dest_slot);
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
                Self::mark_source_dropped_all(src, frame, frames);
            }
            _ => return false,
        }
        true
    }

    /// Execute `instr` if it is one of the instructions common enough in
    /// generic and library code to want a cheaper way in than
    /// `execute_instruction`, and say whether it was.
    ///
    /// Not inlined, unlike `execute_hot`, but small: `execute_instruction`'s
    /// frame is sized for every arm it has, and entering it was most of what
    /// these cost. Holds nothing that calls, so it needs no context.
    #[inline(never)]
    pub(super) fn execute_warm(
        &mut self,
        instr: &Instruction,
        frame: &mut Frame,
        frames: &mut FrameStore,
    ) -> bool {
        match instr {
            Instruction::SlotStoreMove { dest, value } => {
                let src_val = self.read_operand(value, frame, frames);
                match dest {
                    SlotDest::Local(slot_id) => {
                        // The compiler emits a Drop before SlotStoreMove, so the
                        // slot is empty; overwriting an occupied one would leak it.
                        frame.check_slot_empty(*slot_id);
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.move_value(&src_val, dest_slot); }
                        Self::mark_source_dropped_local(value, frame);
                        frame.mark_slot_live(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            *unit,
                            *slot,
                            &src_val,
                        );
                        Self::mark_source_dropped_local(value, frame);
                    }
                }
            }
            // A store writes and does not destroy: what the place held, if
            // anything, a drop in front of the store has destroyed.
            Instruction::ParamStore { param, value } | Instruction::ParamStoreTracked { param, value } => {
                let src_val = self.read_operand(value, frame, frames);
                let dest_ptr = frame.param_dest(*param);
                unsafe { self.move_value(&src_val, dest_ptr); }
                frame.mark_param_live(*param);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::OpAssign { place, op, rhs } => {
                let place_val = self.read_operand(place, frame, frames);
                let rhs_val = self.read_operand(rhs, frame, frames);
                self.execute_op_assign(*op, &place_val, &rhs_val);
            }
            Instruction::OpAssignChecked { overflow, place, op, rhs } => {
                let place_val = self.read_operand(place, frame, frames);
                let rhs_val = self.read_operand(rhs, frame, frames);
                let overflow_slot = frame.value_dest(*overflow);
                self.execute_op_assign_checked(*op, &place_val, &rhs_val, overflow_slot);
                frame.mark_value_live(*overflow);
            }
            Instruction::RefStore { dest, value } => {
                // Store through a reference operand.
                let src_val = self.read_operand(value, frame, frames);
                let dest_val = self.get_operand_dest(dest, frame, frames);
                let dest_ptr = Destination { ptr: dest_val.ptr, tydesc: dest_val.tydesc };
                unsafe { self.move_value(&src_val, dest_ptr); }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                // Tracked slot load: slot may have been moved, updates tracking.
                // Mark slot dropped so destroy_live_values skips it.
                let slot_val = frame.slot(*slot);
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
                Self::mark_source_dropped_all(src, frame, frames);
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
            Instruction::EnumDiscriminant { dest, src } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_enum_discriminant(&src_val, dest_slot);
                frame.mark_value_live(*dest);
                // Does not consume src (borrows only).
            }
            Instruction::EnumPayload { dest, src, variant_index } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_enum_payload(&src_val, dest_slot, *variant_index);
                frame.mark_value_live(*dest);
                // Consumes src.
                Self::mark_source_dropped_local(src, frame);
            }
            Instruction::WrapErr { dest, inner } => {
                let inner_val = self.read_operand(inner, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                self.execute_wrap_err(&inner_val, dest_slot);
                frame.mark_value_live(*dest);
                Self::mark_source_dropped_local(inner, frame);
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
                    Operand::Slot(id) => frame.slot_is_live(*id),
                    Operand::Param(id) => frame.param_is_live(*id),
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
                    return true;
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
            Instruction::ListBoundsCheck { is_valid, list, index } => {
                let list_val = self.read_operand(list, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                let list_struct = unsafe { &*(list_val.ptr as *const rtdt::List) };
                let list_size = list_struct.size.0;

                let valid = idx < list_size;
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = valid; }
                frame.mark_value_live(*is_valid);
            }
            Instruction::ListElementRef { dest, list, index } => {
                let list_val = self.read_operand(list, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                let (list_struct, element_tydesc, element_size) =
                    unsafe { list_element_info(&list_val) };

                // Compute element pointer.
                let element_ptr = unsafe { (list_struct.data as *mut u8).add(idx as usize * element_size) };

                // Store pointer in dest (ref value stores pointer, not data).
                let dest_slot = frame.value_dest(*dest);
                unsafe {
                    *(dest_slot.ptr as *mut *mut u8) = element_ptr;
                }
                frame.check_referent(*dest, element_tydesc);
                frame.mark_value_live(*dest);
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
                unsafe {
                    datalove_rt::c::dtlv_rti_clone_local(
                        self.runtime.handle(),
                        src_val.ptr,
                        src_val.tydesc,
                        dest_slot.ptr,
                        dest_slot.tydesc,
                    );
                }
                frame.mark_value_live(*dest);
                // Source is borrowed (read), not consumed.
            }
            Instruction::Nop => {}
            Instruction::GetField { dest, src, field_index } => {
                let src_val = self.read_operand(src, frame, frames);
                let dest_slot = frame.value_dest(*dest);
                // A shallow copy of the field, at the offset the descriptor of
                // what is there says.
                unsafe {
                    let offset = datalove_rt::c::dtlv_rti_field_offset(src_val.tydesc, *field_index);
                    std::ptr::copy_nonoverlapping(
                        src_val.ptr.add(offset as usize),
                        dest_slot.ptr,
                        (*dest_slot.tydesc).size as usize,
                    );
                }
                frame.mark_value_live(*dest);
            }
            Instruction::SlotStoreCopyTracked { dest, value } => {
                let src_val = self.read_operand(value, frame, frames);
                match dest {
                    SlotDest::Local(slot_id) => {
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.copy_value(&src_val, dest_slot); }
                        frame.mark_slot_live(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
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
                        // A drop in front of the store emptied the slot.
                        frame.check_slot_empty(*slot_id);
                        let dest_slot = frame.slot_dest(*slot_id);
                        unsafe { self.move_value(&src_val, dest_slot); }
                        Self::mark_source_dropped_local(value, frame);
                        frame.mark_slot_live(*slot_id);
                    }
                    SlotDest::External { unit, slot } => {
                        frames.write_external_slot(
                            *unit,
                            *slot,
                            &src_val,
                        );
                        Self::mark_source_dropped_local(value, frame);
                    }
                }
            }
            // A cursor begins and advances through the runtime's functions,
            // with no part of them open-coded.
            Instruction::CursorBegin { cursor, coll } => {
                let coll_val = self.read_operand(coll, frame, frames);
                let out = frame.slot_dest(*cursor).ptr;
                let rt = self.runtime.handle();
                let tag = unsafe { (*coll_val.tydesc).type_tag };
                let status = unsafe {
                    match (frame.cursor_kind(*cursor), tag) {
                        (CursorKind::Stride, rtdt::TyTag::List) => datalove_rt::c::dtlv_rti_list_cursor_begin_local(
                            rt, coll_val.ptr, coll_val.tydesc, out as *mut rtdt::StrideCursor),
                        (CursorKind::Stride, rtdt::TyTag::Tensor) => datalove_rt::c::dtlv_rti_tensor_cursor_begin_local(
                            rt, coll_val.ptr, coll_val.tydesc, out as *mut rtdt::StrideCursor),
                        (CursorKind::Row, rtdt::TyTag::Tensor) => datalove_rt::c::dtlv_rti_tensor_row_cursor_begin_local(
                            rt, coll_val.ptr, coll_val.tydesc, out as *mut rtdt::RowCursor),
                        (CursorKind::Tree, rtdt::TyTag::Set) => datalove_rt::c::dtlv_rti_btreeset_cursor_begin_local(
                            rt, coll_val.ptr, coll_val.tydesc, out as *mut rtdt::TreeCursor),
                        (CursorKind::Tree, rtdt::TyTag::Map) => datalove_rt::c::dtlv_rti_btreemap_cursor_begin_local(
                            rt, coll_val.ptr, coll_val.tydesc, out as *mut rtdt::TreeCursor),
                        (kind, tag) => unreachable!("a {kind:?} cursor over a {tag:?}"),
                    }
                };
                debug_assert_eq!(status, datalove_rt::c::RtStatus::Ok, "starting a cursor cannot fail");
                frame.mark_slot_live(*cursor);
            }
            // Done, elem, index and value read the cursor's own fields, which
            // begin and advance keep current.
            Instruction::CursorDone { dest, cursor } => {
                let at = frame.slot(*cursor).ptr;
                let out = frame.value_dest(*dest).ptr;
                unsafe { *out = crate::collections::cursor_done(frame.cursor_kind(*cursor), at) as u8 };
                frame.mark_value_live(*dest);
            }
            Instruction::CursorElem { dest, cursor } => {
                let at = frame.slot(*cursor).ptr;
                let out = frame.value_dest(*dest).ptr as *mut *const u8;
                unsafe { *out = crate::collections::cursor_elem(frame.cursor_kind(*cursor), at) };
                frame.mark_value_live(*dest);
            }
            Instruction::CursorIndex { dest, cursor } => {
                let at = frame.slot(*cursor).ptr;
                let out = frame.value_dest(*dest).ptr as *mut rtdt::Index;
                unsafe { *out = crate::collections::cursor_index(frame.cursor_kind(*cursor), at) };
                frame.mark_value_live(*dest);
            }
            Instruction::CursorValue { dest, cursor } => {
                debug_assert_eq!(frame.cursor_kind(*cursor), CursorKind::Tree, "only a map's cursor has values");
                let at = frame.slot(*cursor).ptr;
                let out = frame.value_dest(*dest).ptr as *mut *const u8;
                unsafe { *out = crate::collections::cursor_value(at) };
                frame.mark_value_live(*dest);
            }
            Instruction::CursorAdvance { cursor, .. } => {
                let at = frame.slot_dest(*cursor).ptr;
                let rt = self.runtime.handle();
                let status = unsafe {
                    match frame.cursor_kind(*cursor) {
                        CursorKind::Stride => datalove_rt::c::dtlv_rti_stride_cursor_advance_local(rt, at as *mut rtdt::StrideCursor),
                        CursorKind::Row => datalove_rt::c::dtlv_rti_row_cursor_advance_local(rt, at as *mut rtdt::RowCursor),
                        CursorKind::Tree => datalove_rt::c::dtlv_rti_tree_cursor_advance_local(rt, at as *mut rtdt::TreeCursor),
                    }
                };
                debug_assert_eq!(status, datalove_rt::c::RtStatus::Ok, "advancing a cursor cannot fail");
            }
            _ => return false,
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_instruction(
        &mut self,
        instr: &Instruction,
        frame: &mut Frame,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
    ) -> Result<(), InterpError> {
        match instr {
            Instruction::ParamStore { .. }
            | Instruction::ParamStoreTracked { .. }
            | Instruction::ListBoundsCheck { .. }
            | Instruction::ListElementRef { .. }
            | Instruction::WidenFixed { .. }
            | Instruction::Widen { .. }
            | Instruction::Drop { .. }
            | Instruction::DropTracked { .. }
            | Instruction::Clone { .. }
            | Instruction::GetField { .. }
            | Instruction::EnumDiscriminant { .. }
            | Instruction::EnumPayload { .. }
            | Instruction::EnumVariant { .. }
            | Instruction::WrapErr { .. }
            | Instruction::WrapNone { .. }
            | Instruction::ErrorFrom { .. }
            | Instruction::DataFrom { .. }
            | Instruction::SlotStoreCopyTracked { .. }
            | Instruction::SlotStoreMoveTracked { .. }
            | Instruction::SlotLoadMoveTracked { .. }
            | Instruction::SlotStoreMove { .. }
            | Instruction::Pack { .. }
            | Instruction::Unpack { .. }
            | Instruction::UnaryOpChecked { .. }
            | Instruction::RefStore { .. }
            | Instruction::OpAssign { .. }
            | Instruction::OpAssignChecked { .. }
            | Instruction::DropViaRef { .. }
            | Instruction::CursorBegin { .. }
            | Instruction::CursorDone { .. }
            | Instruction::CursorElem { .. }
            | Instruction::CursorIndex { .. }
            | Instruction::CursorValue { .. }
            | Instruction::CursorAdvance { .. }
            | Instruction::Nop => {
                unreachable!("{:?} is executed by execute_warm", instr)
            }
            Instruction::Const { .. } | Instruction::Copy { .. } | Instruction::Move { .. }
            | Instruction::BinOp { .. } | Instruction::UnaryOp { .. }
            | Instruction::SlotStoreCopy { .. } | Instruction::SlotLoadCopy { .. }
            | Instruction::BinOpChecked { .. } | Instruction::Intrinsic { .. }
            | Instruction::WrapSome { .. } | Instruction::UnwrapOption { .. }
            | Instruction::WrapOk { .. } | Instruction::UnwrapResult { .. } => {
                unreachable!("{:?} is executed by execute_hot", instr)
            }
            Instruction::StaticRef { dest, value } => {
                let dest_slot = frame.value_dest(*dest);
                // A reference's descriptor holds what it points at as its one field.
                let pointee = unsafe { (*(*dest_slot.tydesc).type_info.tuple.fields).tydesc };
                let ptr = self.static_const_with_tydesc(value, pointee);
                unsafe { *(dest_slot.ptr as *mut *const u8) = ptr; }
                frame.check_referent(*dest, pointee);
                frame.mark_value_live(*dest);
            }
            Instruction::RefSetField { dest, field_path, value } => {
                // Store to a field through a reference operand. Used after inlining mut params.
                let value_val = self.read_operand(value, frame, frames);
                let dest_ptr = self.get_operand_dest(dest, frame, frames);
                // Navigate to the field.
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    dest_ptr.ptr, dest_ptr.tydesc, field_path
                );
                self.write_field(current_ptr, current_tydesc, &value_val);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::RefStoreTracked { dest, value } => {
                // Store through a reference operand with tracking. Used after inlining out params.
                // The destination was uninitialized, so don't destroy old value.
                let src_val = self.read_operand(value, frame, frames);
                let dest_val = self.get_operand_dest(dest, frame, frames);
                let dest_ptr = Destination { ptr: dest_val.ptr, tydesc: dest_val.tydesc };
                unsafe { self.move_value(&src_val, dest_ptr); }
                // Mark the destination as initialized.
                match dest {
                    Operand::Slot(slot) => frame.mark_slot_live(*slot),
                    Operand::Param(param) => frame.mark_param_live(*param),
                    Operand::ExternalSlot { unit, slot } => frames.mark_external_slot_live(*unit, *slot),
                    // Other operand types don't have tracking in the same way.
                    _ => {}
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::RefSetFieldTracked { dest, field_path, value } => {
                // Store to a field through a reference operand with tracking. Used after inlining out params.
                let value_val = self.read_operand(value, frame, frames);
                let dest_ptr = self.get_operand_dest(dest, frame, frames);
                // Navigate to the field.
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    dest_ptr.ptr, dest_ptr.tydesc, field_path
                );
                // Don't destroy old value (was uninitialized), just store new value.
                self.write_field(current_ptr, current_tydesc, &value_val);
                // Mark the destination as initialized.
                match dest {
                    Operand::Slot(slot) => frame.mark_slot_live(*slot),
                    Operand::Param(param) => frame.mark_param_live(*param),
                    Operand::ExternalSlot { unit, slot } => frames.mark_external_slot_live(*unit, *slot),
                    // Other operand types don't have tracking in the same way.
                    _ => {}
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::SlotLoadMove { dest, slot } => {
                // Precise slot load: ownership analysis guarantees slot is occupied.
                let slot_val = frame.slot(*slot);
                let dest_slot = frame.value_dest(*dest);
                unsafe { self.move_value(&slot_val, dest_slot); }
                frame.mark_value_live(*dest);
                frame.mark_slot_dropped(*slot);
            }
            // ComptimeCall behaves exactly like Call - the specialization metadata is
            // only used by the specialization pass.
            Instruction::Call { dest, func, args, bindings, .. }
            | Instruction::ComptimeCall { dest, func, args, bindings, .. } => {
                self.execute_call(func, args, bindings, *dest, frame, ctx, registry, frames)?;
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
            Instruction::ListGet { dest, is_valid, list, index } => {
                let list_val = self.read_operand(list, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                let (list_struct, element_tydesc, element_size) =
                    unsafe { list_element_info(&list_val) };
                let list_size = list_struct.size.0;

                let valid = idx < list_size;
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = valid; }
                frame.mark_value_live(*is_valid);

                if valid {
                    let dest_slot = frame.value_dest(*dest);
                    let rt_handle = self.runtime.handle();
                    let element_ptr =
                        unsafe { list_struct.data.add(idx as usize * element_size) };
                    let status = unsafe {
                        datalove_rt::c::dtlv_rti_clone_local(
                            rt_handle,
                            element_ptr,
                            element_tydesc,
                            dest_slot.ptr,
                            element_tydesc,
                        )
                    };
                    assert_eq!(status, datalove_rt::c::RtStatus::Ok, "ListGet clone failed");
                    frame.mark_value_live(*dest);
                }
                // If invalid, dest is uninitialized — caller must not use it.
            }
            Instruction::ListSet { is_valid, list, index, value } => {
                let list_val = self.read_operand(list, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let value_val = self.read_operand(value, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                // The runtime checks the index and destroys what was there. It
                // takes the value whether or not it lands.
                let status = unsafe {
                    datalove_rt::c::dtlv_rti_list_set_local(
                        self.runtime.handle(), list_val.ptr as *mut u8, list_val.tydesc,
                        idx, value_val.ptr as *mut u8, value_val.tydesc)
                };
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = status == datalove_rt::c::RtStatus::Ok; }
                frame.mark_value_live(*is_valid);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::MapGet { dest, is_valid, map, key } => {
                let map_val = self.read_operand(map, frame, frames);
                let key_val = self.read_operand(key, frame, frames);
                let found = self.map_get(&map_val, &key_val, frame.value_dest(*dest));
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = found; }
                frame.mark_value_live(*is_valid);
                if found {
                    frame.mark_value_live(*dest);
                }
            }
            Instruction::MapContainsKey { is_valid, map, key } => {
                let map_val = self.read_operand(map, frame, frames);
                let key_val = self.read_operand(key, frame, frames);

                let (map_tydesc, _, _) = unsafe { map_tydesc_info(&map_val) };

                let mut found = false;
                let rt_handle = self.runtime.handle();
                let status = unsafe {
                    datalove_rt::c::dtlv_rti_btreemap_contains_key_local(
                        rt_handle,
                        map_val.ptr,
                        map_tydesc,
                        key_val.ptr,
                        key_val.tydesc,
                        &mut found,
                    )
                };
                assert_eq!(status, datalove_rt::c::RtStatus::Ok, "MapContainsKey failed");

                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = found; }
                frame.mark_value_live(*is_valid);
            }
            Instruction::MapSetValue { is_valid, map, key, value } => {
                let map_val = self.read_operand(map, frame, frames);
                let key_val = self.read_operand(key, frame, frames);
                let value_val = self.read_operand(value, frame, frames);

                let (map_tydesc, _, _) = unsafe { map_tydesc_info(&map_val) };

                // The runtime takes the value whether or not the key is there.
                let rt_handle = self.runtime.handle();
                let status = unsafe {
                    datalove_rt::c::dtlv_rti_btreemap_set_value_local(
                        rt_handle,
                        map_val.ptr as *mut u8,
                        map_tydesc,
                        key_val.ptr,
                        key_val.tydesc,
                        value_val.ptr as *mut u8,
                        value_val.tydesc,
                    )
                };
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = status == datalove_rt::c::RtStatus::Ok; }
                frame.mark_value_live(*is_valid);

                // Mark value operand as consumed (moved into map).
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::MapValueRef { dest, map, key } => {
                let map_val = self.read_operand(map, frame, frames);
                let key_val = self.read_operand(key, frame, frames);

                let (map_tydesc, _, value_tydesc) =
                    unsafe { map_tydesc_info(&map_val) };

                let mut value_ptr: *mut u8 = std::ptr::null_mut();
                let rt_handle = self.runtime.handle();
                let status = unsafe {
                    datalove_rt::c::dtlv_rti_btreemap_get_value_ref_local(
                        rt_handle,
                        map_val.ptr,
                        map_tydesc,
                        key_val.ptr,
                        key_val.tydesc,
                        &mut value_ptr,
                    )
                };
                assert_eq!(status, datalove_rt::c::RtStatus::Ok, "MapValueRef failed");

                // Store pointer in dest.
                let dest_slot = frame.value_dest(*dest);
                unsafe {
                    *(dest_slot.ptr as *mut *mut u8) = value_ptr;
                }
                frame.check_referent(*dest, value_tydesc);
                frame.mark_value_live(*dest);
            }
            Instruction::MapUpsert { map, key, value } => {
                let map_val = self.read_operand(map, frame, frames);
                let key_val = self.read_operand(key, frame, frames);
                let value_val = self.read_operand(value, frame, frames);

                let (map_tydesc, key_tydesc, value_tydesc) =
                    unsafe { map_tydesc_info(&map_val) };

                let rt_handle = self.runtime.handle();
                let status = unsafe {
                    datalove_rt::c::dtlv_rti_btreemap_insert_local(
                        rt_handle,
                        map_val.ptr as *mut u8,
                        map_tydesc,
                        key_val.ptr,
                        key_tydesc,
                        value_val.ptr,
                        value_tydesc,
                    )
                };
                assert_eq!(status, datalove_rt::c::RtStatus::Ok, "MapUpsert failed");

                // Mark key and value operands as consumed.
                Self::mark_source_dropped_local(key, frame);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::TensorBoundsCheck { is_valid, tensor, index } => {
                let tensor_val = self.read_operand(tensor, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                let tensor_struct = unsafe { &*(tensor_val.ptr as *const rtdt::Tensor) };
                let shape_ptr = tensor_struct.shape;
                assert!(!shape_ptr.is_null(), "TensorBoundsCheck: shape is null");
                let dim0 = unsafe { (*shape_ptr).0 };

                let valid = idx < dim0;
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = valid; }
                frame.mark_value_live(*is_valid);
            }
            Instruction::TensorGet { dest, is_valid, tensor, index } => {
                let tensor_val = self.read_operand(tensor, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                let tensor_struct = unsafe { &*(tensor_val.ptr as *const rtdt::Tensor) };
                let tensor_tydesc = tensor_val.tydesc;
                let rank = unsafe { (*tensor_tydesc).type_info.tensor.rank };
                let element_tydesc = unsafe { (*tensor_tydesc).type_info.tensor.element_tydesc };
                let shape_ptr = tensor_struct.shape;
                assert!(!shape_ptr.is_null(), "TensorGet: shape is null");
                let dim0 = unsafe { (*shape_ptr).0 };

                let valid = idx < dim0;
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = valid; }
                frame.mark_value_live(*is_valid);

                if valid {
                    if rank == 1 {
                        // Rank 1: clone element at strided offset.
                        let strides_ptr = tensor_struct.strides;
                        let offset = tensor_struct.offset_elems.0;
                        let stride0 = unsafe { (*strides_ptr).0 };
                        let linear_offset = offset.saturating_add(idx.saturating_mul(stride0));
                        let element_size = unsafe { (*element_tydesc).size as usize };
                        let element_ptr = unsafe {
                            tensor_struct.ptr_base.add(linear_offset as usize * element_size)
                        };
                        let dest_slot = frame.value_dest(*dest);
                        let rt_handle = self.runtime.handle();
                        let status = unsafe {
                            datalove_rt::c::dtlv_rti_clone_local(
                                rt_handle,
                                element_ptr,
                                element_tydesc,
                                dest_slot.ptr,
                                dest_slot.tydesc,
                            )
                        };
                        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "TensorGet clone failed");
                    } else {
                        // Rank > 1: hyperplane clone via runtime.
                        let dest_slot = frame.value_dest(*dest);
                        let rt_handle = self.runtime.handle();
                        let status = unsafe {
                            datalove_rt::c::dtlv_rti_tensor_hyperplane_clone_local(
                                rt_handle,
                                tensor_val.ptr,
                                tensor_tydesc,
                                idx,
                                dest_slot.ptr,
                            )
                        };
                        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "TensorGet hyperplane_clone failed");
                    }
                    frame.mark_value_live(*dest);
                }
            }
            Instruction::TensorSet { is_valid, tensor, index, value } => {
                let tensor_val = self.read_operand(tensor, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let value_val = self.read_operand(value, frame, frames);

                // As ListSet. Rank 1 only, so the index is the indices array.
                let status = unsafe {
                    datalove_rt::c::dtlv_rti_tensor_set_local(
                        self.runtime.handle(), tensor_val.ptr as *mut u8, tensor_val.tydesc,
                        idx_val.ptr as *const rtdt::IndexRepr,
                        value_val.ptr as *mut u8, value_val.tydesc)
                };
                let is_valid_dest = frame.value_dest(*is_valid);
                unsafe { *(is_valid_dest.ptr as *mut bool) = status == datalove_rt::c::RtStatus::Ok; }
                frame.mark_value_live(*is_valid);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::TensorIndexRef { dest, tensor, index } => {
                let tensor_val = self.read_operand(tensor, frame, frames);
                let idx_val = self.read_operand(index, frame, frames);
                let idx = unsafe { *(idx_val.ptr as *const rtdt::IndexRepr) };

                let tensor_struct = unsafe { &*(tensor_val.ptr as *const rtdt::Tensor) };
                let rank = unsafe { (*tensor_val.tydesc).type_info.tensor.rank };
                let element_tydesc = unsafe { (*tensor_val.tydesc).type_info.tensor.element_tydesc };

                let strides_ptr = tensor_struct.strides;
                let offset = tensor_struct.offset_elems.0;
                let stride0 = unsafe { (*strides_ptr).0 };
                let linear_offset = offset.saturating_add(idx.saturating_mul(stride0));

                if rank == 1 {
                    // Rank 1: ref to element.
                    let element_size = unsafe { (*element_tydesc).size as usize };
                    let element_ptr = unsafe {
                        (tensor_struct.ptr_base as *mut u8).add(linear_offset as usize * element_size)
                    };
                    let dest_slot = frame.value_dest(*dest);
                    unsafe { *(dest_slot.ptr as *mut *mut u8) = element_ptr; }
                    frame.check_referent(*dest, element_tydesc);
                } else {
                    // Rank > 1: a view of the row, made where the frame keeps
                    // it for this reference.
                    let at = frame.layout().tensor_view_offsets[dest.0 as usize]
                        .expect("a frame keeps a view for each tensor view reference");
                    let view_ptr = unsafe { frame.base_ptr().add(at as usize) } as *mut rtdt::Tensor;
                    unsafe {
                        view_ptr.write(rtdt::Tensor {
                            ptr_base: tensor_struct.ptr_base,
                            offset_elems: rtdt::Index(linear_offset),
                            capacity_elems: rtdt::Index::ZERO,
                            shape: tensor_struct.shape.add(1),
                            strides: tensor_struct.strides.add(1),
                            layout: tensor_struct.layout,
                        });
                    }
                    let dest_slot = frame.value_dest(*dest);
                    unsafe { *(dest_slot.ptr as *mut *mut u8) = view_ptr as *mut u8; }
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
                // And what it points at. Inside a generic the layout's
                // descriptor for this value says `data`, because that is what
                // the static type says, while the field is whatever the caller
                // really passed. Reading through it later has no other way to
                // find out.
                let field_tydesc = unsafe {
                    datalove_rt::c::dtlv_rti_field_tydesc(src_val.tydesc, *field_index)
                };
                frame.check_referent(*dest, field_tydesc);
                frame.mark_value_live(*dest);
            }
            Instruction::SetField { slot, field_path, value } => {
                let value_val = self.read_operand(value, frame, frames);

                // Get the slot's base pointer and tydesc.
                let slot_info = match slot {
                    SlotDest::Local(id) => frame.slot_dest(*id),
                    SlotDest::External { unit, slot } => frames.external_slot_dest(*unit, *slot),
                };

                // Navigate field path to find target field.
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );

                // Copy new value to target field.
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        value_val.ptr,
                        current_ptr,
                        size,
                    );
                }
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::ParamSetField { param, field_path, value }
            | Instruction::ParamSetFieldTracked { param, field_path, value } => {
                let value_val = self.read_operand(value, frame, frames);
                let slot_info = frame.param_dest(*param);
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );
                self.write_field(current_ptr, current_tydesc, &value_val);
                frame.mark_param_live(*param);
                Self::mark_source_dropped_local(value, frame);
            }
            Instruction::DropField { base, field_path } => {
                let place = self.get_operand_dest(base, frame, frames);
                let (field_ptr, field_tydesc) = self.navigate_field_path(
                    place.ptr, place.tydesc, field_path
                );
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(
                        self.runtime.handle(),
                        field_ptr,
                        field_tydesc,
                    );
                }
            }
            // Slot tracking variants - these track SLOT state, not value state.
            Instruction::SetFieldTracked { slot, field_path, value } => {
                let value_val = self.read_operand(value, frame, frames);
                let slot_info = match slot {
                    SlotDest::Local(id) => frame.slot_dest(*id),
                    SlotDest::External { unit, slot } => frames.external_slot_dest(*unit, *slot),
                };
                let (current_ptr, current_tydesc) = self.navigate_field_path(
                    slot_info.ptr, slot_info.tydesc, field_path
                );
                let size = unsafe { (*current_tydesc).size as usize };
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        value_val.ptr,
                        current_ptr,
                        size,
                    );
                }
                // The slot holds something once a field of it is written, as the
                // compiled backends record.
                match slot {
                    SlotDest::Local(id) => frame.mark_slot_live(*id),
                    SlotDest::External { unit, slot } => frames.mark_external_slot_live(*unit, *slot),
                }
                Self::mark_source_dropped_local(value, frame);
            }
        }
        Ok(())
    }

    /// Mark a local operand as dropped after a move.
    ///
    /// Handles Value, Slot, and Param operands in the current frame.
    /// External operands are ignored since they belong to other frames and are
    /// handled separately (typically in terminators via `mark_source_dropped_all`).
    pub(super) fn mark_source_dropped_local(operand: &Operand, frame: &mut Frame) {

        match operand {
            Operand::Value(id) | Operand::ValueRef(id) => frame.mark_value_dropped(*id),
            Operand::Slot(id) => frame.mark_slot_dropped(*id),
            Operand::Param(id) => frame.mark_param_dropped(*id),
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {}
        }
    }

    /// Mark any operand as dropped after a move, including external operands.
    ///
    /// Used for terminators (Exit, EarlyExit) and the Move instruction where
    /// external values/slots may be consumed.
    pub(super) fn mark_source_dropped_all(operand: &Operand, frame: &mut Frame, frames: &mut FrameStore) {
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

    /// Read an operand.
    ///
    /// Inlined everywhere, which is most of the instructions there are, so the
    /// three kinds an operand nearly always is are read here and the rest out of
    /// line.
    #[inline(always)]
    pub(crate) fn read_operand(
        &self,
        op: &Operand,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Value {
        match op {
            Operand::Value(id) => frame.value(*id),
            Operand::Slot(id) => frame.slot(*id),
            Operand::Param(id) => frame.param(*id),
            _ => Self::read_operand_rare(op, frame, frames),
        }
    }

    #[inline(never)]
    fn read_operand_rare(op: &Operand, frame: &Frame, frames: &FrameStore) -> Value {
        match op {
            Operand::ValueRef(id) => frame.value_deref(*id),
            Operand::ExternalValue { unit, value } => {
                frames.external_value(*unit, *value).unwrap()
            }
            Operand::ExternalSlot { unit, slot } => {
                frames.external_slot(*unit, *slot).unwrap()
            }
            Operand::Value(_) | Operand::Slot(_) | Operand::Param(_) => {
                unreachable!("read_operand reads these itself")
            }
        }
    }

    /// Get pointer to operand's destination without checking initialization.
    ///
    /// Used for Out params where we need to pass a pointer to an uninitialized slot.
    /// For ValueRef operands, dereferences to get the actual destination.
    ///
    /// Panics if operand is an earlier unit's value, which is immutable
    /// (compiler bug).
    pub(super) fn get_operand_dest(&self, op: &Operand, frame: &mut Frame, frames: &FrameStore) -> Value {
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
            Operand::Param(id) => {
                // This function's own out parameter, passed straight on to
                // another. What it names is the caller's place, which is
                // where the callee should write, so it goes across as it is.
                let dest = frame.param_dest(*id);
                Value { ptr: dest.ptr, tydesc: dest.tydesc }
            }
            Operand::ExternalSlot { unit, slot } => {
                let dest = frames.external_slot_dest(*unit, *slot);
                Value { ptr: dest.ptr, tydesc: dest.tydesc }
            }
            Operand::ExternalValue { .. } => {
                panic!("get_operand_dest: invalid operand {:?} for out param", op)
            }
        }
    }

    // -------------------------------------------------------------------------
    // Call instruction helpers
    // -------------------------------------------------------------------------

    /// Look a key up in a map and clone what it finds into `dest`, saying
    /// whether there was anything.
    pub(crate) fn map_get(&mut self, map_val: &Value, key_val: &Value, dest: Destination) -> bool {
        let (map_tydesc, _, value_tydesc) = unsafe { map_tydesc_info(map_val) };

        let mut value_ptr: *mut u8 = std::ptr::null_mut();
        let rt_handle = self.runtime.handle();
        let status = unsafe {
            datalove_rt::c::dtlv_rti_btreemap_get_value_ref_local(
                rt_handle, map_val.ptr, map_tydesc, key_val.ptr, key_val.tydesc, &mut value_ptr,
            )
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "MapGet get_value_ref failed");
        if value_ptr.is_null() {
            return false;
        }

        let status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(rt_handle, value_ptr, value_tydesc, dest.ptr, dest.tydesc)
        };
        assert_eq!(status, datalove_rt::c::RtStatus::Ok, "MapGet clone failed");
        true
    }

    /// Move a value into a field, whatever the field held having been
    /// destroyed already.
    fn write_field(
        &mut self,
        target_ptr: *mut u8,
        target_tydesc: *const rtdt::TyDesc,
        value: &crate::value::Value,
    ) {
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, target_ptr, (*target_tydesc).size as usize);
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

    #[inline(always)]
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
            copy_bytes(src.ptr, dest.ptr, (*src.tydesc).size as usize);
        }
    }

    #[inline(always)]
    pub(super) unsafe fn move_value(&self, src: &Value, dest: Destination) {
        unsafe {
            // Move is always a shallow copy - ownership transfers to dest.
            // The source should be marked as dropped so it won't be destroyed.
            copy_bytes(src.ptr, dest.ptr, (*src.tydesc).size as usize);
        }
    }

    /// Execute Drop: run destructor for a value.
    pub(super) fn execute_drop(&mut self, val: &Value) {
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                self.runtime.handle(),
                val.ptr,
                val.tydesc,
            );
        }
    }
}
