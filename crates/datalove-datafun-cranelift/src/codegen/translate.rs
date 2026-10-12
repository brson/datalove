//! Compiling a body: its blocks, their parameters, and each instruction to
//! the codegen for its kind.

use super::*;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile the function and return the Cranelift FuncId.
    ///
    /// This declares the function under its own name with Export linkage and
    /// then defines it. Use `compile_predeclared` for functions that have
    /// already been declared, and `compile_as` where the name has to be one the
    /// caller chooses.
    pub fn compile(self) -> Result<FuncId, CraneliftError> {
        let symbol = self.func.name.clone();
        self.compile_as(&symbol).map(|defined| defined.id)
    }

    /// Compile the function under a symbol of the caller's choosing.
    ///
    /// A module's function is named by its own name alone, and two modules can
    /// share one: `list.get` and `map.get` both compile to `get`. Declaring
    /// both in one module is a collision, and it goes one of two ways. Where
    /// the arities differ -- they do here, because `map.get` takes descriptors
    /// for two type parameters and `set.get` for one -- the second declaration
    /// is refused outright. Where they agree it is accepted, and every call
    /// meant for one of them reaches the other.
    ///
    /// So anything compiling functions from more than one module into a single
    /// module has to name them apart itself.
    pub fn compile_as(mut self, symbol: &str) -> Result<Defined, CraneliftError> {
        // Build function signature.
        let sig = self.build_signature();

        // Declare function in module.
        let func_id = self.module
            .declare_function(symbol, Linkage::Export, &sig)
            .map_err(|e| CraneliftError::Module(format!("declare function: {}", e)))?;

        self.compile_body(func_id, sig)
    }

    /// Compile a function that has already been declared.
    ///
    /// Use this for two-pass compilation where functions are declared first.
    pub fn compile_predeclared(mut self, func_id: FuncId) -> Result<FuncId, CraneliftError> {
        let sig = self.build_signature();
        self.compile_body(func_id, sig).map(|defined| defined.id)
    }

    /// Compile the function body using the given FuncId and signature.
    pub(super) fn compile_body(&mut self, func_id: FuncId, sig: cl_ir::Signature) -> Result<Defined, CraneliftError> {
        self.check_dynamic_support()?;

        // Create Cranelift function.
        let mut cl_func = cl_ir::Function::with_name_signature(
            cl_ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        // Create function builder context.
        let mut fb_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut cl_func, &mut fb_ctx);

        // The blocks to compile: every one, or for an OSR entry, those the
        // header reaches.
        let reachable = self.osr_spec.as_ref().map(|spec| reachable_from(self.func, spec.header));
        let compiled = |id: BlockId| reachable.as_ref().is_none_or(|r| r.contains(&id));

        // Create frame stack slot if needed.
        if self.layout.frame_size > 0 && self.osr_spec.is_none() {
            let slot_data = cl_ir::StackSlotData::new(
                cl_ir::StackSlotKind::ExplicitSlot,
                self.layout.frame_size,
                types::align_shift(self.layout.frame_align),
            );
            self.frame_slot = Some(FrameBase::Slot(builder.create_sized_stack_slot(slot_data)));
        }

        // Create blocks with their parameters.
        for block in &self.func.blocks {
            if !compiled(block.id) {
                continue;
            }
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

        if self.osr_spec.is_some() {
            self.enter_osr(&mut builder, reachable.clone().expect("an OSR entry has its reachable blocks"))?;
        } else {
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
            self.check_stack(&mut builder)?;

            // Check if this function uses sret.
            let has_sret = uses_sret(&self.func_ctx.return_type);
            let user_param_start = if has_sret {
                // Second param is sret pointer.
                self.sret_param = Some(param_values[1]);
                2
            } else {
                1
            };

            // User params start after implicit params, and the type parameter
            // descriptors the caller supplied follow them.
            let user_param_count = self.func_ctx.param_types.len();
            for (i, &val) in param_values[user_param_start..].iter().take(user_param_count).enumerate() {
                let param_id = ParamId(i as u32);
                // Track param values for get_operand_value.
                self.param_values.insert(param_id, val);
            }
            let type_param_start = user_param_start + user_param_count;
            self.type_param_values = param_values[type_param_start..].iter().copied()
                .take(self.func_ctx.type_param_count as usize)
                .collect();

            // Initialize aggregate slots and tracking bytes region.
            // - Aggregate slots: 0xFF poison makes uninitialized reads obvious
            // - Tracking bytes: 0x00 (UNINIT), so DropTracked skips them
            if let Some(frame_slot) = self.frame_slot {
                // Fill aggregate slots with 0xFF poison pattern.
                for (slot_idx, slot_ty) in self.func.slot_types.iter().enumerate() {
                    if dynamic::is_dynamic_value(slot_ty) {
                        continue;
                    }
                    let repr = types::ir_type_to_cranelift(slot_ty);
                    if let CraneliftRepr::Aggregate(layout) = repr {
                        let slot_offset = self.layout.slot_offset(slot_idx as u32);
                        let addr = frame_slot.addr(&mut builder, slot_offset as i32);
                        let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                        let poison = builder.ins().iconst(cl_types::I8, 0xFF_u8 as i64);
                        builder.call_memset(self.isa.frontend_config(), addr, poison, size);
                    }
                }

                // Zero-init tracking bytes.
                if self.layout.tracking_count > 0 {
                    let track_addr = frame_slot.addr(&mut builder, self.layout.tracking_offset as i32);
                    let size = builder.ins().iconst(PTR_TYPE, self.layout.tracking_count as i64);
                    let zero = builder.ins().iconst(cl_types::I8, 0);
                    builder.call_memset(self.isa.frontend_config(), track_addr, zero, size);
                }
            }
            self.enter_dynamic(&mut builder)?;
        }

        // Compile each block.
        let func = self.func;
        for ir_block in func.blocks.iter().filter(|b| compiled(b.id)) {
            self.compile_block(&mut builder, ir_block)?;
        }

        // Seal all blocks now that all predecessors are known (required for loops).
        builder.seal_all_blocks();

        // Finalize function.
        builder.finalize(self.isa.frontend_config());

        // Define function in module.
        let mut ctx = cranelift_codegen::Context::new();
        ctx.func = cl_func;

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(define_error)?;
        let code_size = ctx.compiled_code().expect("a function just defined").code_info().total_size;

        Ok(Defined { id: func_id, code_size })
    }

    /// Compile one block: its parameters, instructions and terminator.
    fn compile_block(&mut self, builder: &mut FunctionBuilder, ir_block: &IrBlock) -> Result<(), CraneliftError> {
        let cl_block = self.blocks[&ir_block.id];
        // Switch to block (entry already switched, unless this is an OSR
        // entry, whose entry is its own).
        if ir_block.id != BlockId(0) || self.osr.is_some() {
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
                    CraneliftRepr::Aggregate(_) => {
                        // Aggregate: block param is PTR to source. Copy to local frame.
                        let dest_addr = self.dest_addr(builder, *ir_value_id)?;
                        if dynamic::is_dynamic_value(&param_ty) {
                            // A move rather than a copy: an OSR entry passes a
                            // dynamic parameter its own storage.
                            let size = self.value_size(builder, &param_ty)?;
                            builder.call_memmove(self.isa.frontend_config(), dest_addr, cl_param, size);
                        } else {
                            self.copy_value_bytes(builder, dest_addr, cl_param, &param_ty)?;
                        }

                        // Use local address for this value.
                        self.values.insert(*ir_value_id, dest_addr);

                        // Mark as live if tracked.
                        self.mark_value_live(builder, *ir_value_id);
                    }
                }
            }

            // Don't seal yet - wait until all blocks are compiled for loop back-edges.
        }

        // Compile instructions.
        for inst in &ir_block.instructions {
            self.compile_instruction(builder, inst)?;
        }

        // In an OSR entry, a carried value made here is its variable's new
        // definition from here on; see `carried_into`.
        if let Some(osr) = &self.osr {
            let made: Vec<(ValueId, Variable)> = ir_block.params.iter().copied()
                .chain(ir_block.instructions.iter().flat_map(Instruction::dests))
                .filter_map(|vid| osr.carried.get(&vid).map(|&var| (vid, var)))
                .collect();
            for (vid, var) in made {
                let v = self.values.remove(&vid).expect("a block defines what its instructions make");
                builder.def_var(var, v);
            }
        }

        // Compile terminator.
        self.compile_terminator(builder, &ir_block.terminator)?;
        Ok(())
    }

    /// Build the Cranelift function signature.
    ///
    /// All functions have an implicit rt_handle as first parameter.
    /// For aggregate returns, an sret pointer is the second parameter.
    /// User-visible parameters follow.
    pub(super) fn build_signature(&self) -> cl_ir::Signature {
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
            Instruction::StaticRef { dest, value } => {
                let addr = match self.static_consts.get(&static_const_key(value)) {
                    Some(StaticConstLoc::Address(addr)) => builder.ins().iconst(PTR_TYPE, *addr as i64),
                    Some(StaticConstLoc::Data(data_id)) => {
                        let gv = self.module.declare_data_in_func(*data_id, builder.func);
                        builder.ins().symbol_value(PTR_TYPE, gv)
                    }
                    None => return Err(CraneliftError::Unsupported(format!(
                        "a static const with nowhere to live: {}", value.0))),
                };
                // The reference is the address.
                self.values.insert(*dest, addr);
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
            Instruction::OpAssign { place, op, rhs } => {
                self.compile_op_assign(builder, place, *op, rhs)?;
            }
            Instruction::OpAssignChecked { overflow, place, op, rhs } => {
                self.compile_op_assign_checked(builder, *overflow, place, *op, rhs)?;
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
            Instruction::ListSet { is_valid, list, index, value } => {
                self.compile_list_set(builder, *is_valid, list, index, value)?;
            }
            Instruction::ListElementRef { dest, list, index } => {
                self.compile_list_element_ref(builder, *dest, list, index)?;
            }
            Instruction::MapGet { dest, is_valid, map, key } => {
                self.compile_map_get(builder, *dest, *is_valid, map, key)?;
            }
            Instruction::MapContainsKey { is_valid, map, key } => {
                self.compile_map_contains_key(builder, *is_valid, map, key)?;
            }
            Instruction::MapSetValue { is_valid, map, key, value } => {
                self.compile_map_set_value(builder, *is_valid, map, key, value)?;
            }
            Instruction::MapValueRef { dest, map, key } => {
                self.compile_map_value_ref(builder, *dest, map, key)?;
            }
            Instruction::MapUpsert { map, key, value } => {
                self.compile_map_upsert(builder, map, key, value)?;
            }
            Instruction::TensorGet { dest, is_valid, tensor, index } => {
                self.compile_tensor_get(builder, *dest, *is_valid, tensor, index)?;
            }
            Instruction::TensorBoundsCheck { is_valid, tensor, index } => {
                self.compile_tensor_bounds_check(builder, *is_valid, tensor, index)?;
            }
            Instruction::TensorSet { is_valid, tensor, index, value } => {
                self.compile_tensor_set(builder, *is_valid, tensor, index, value)?;
            }
            Instruction::TensorIndexRef { dest, tensor, index } => {
                self.compile_tensor_index_ref(builder, *dest, tensor, index)?;
            }
            Instruction::DebugLog { operand } => {
                self.compile_debuglog(builder, operand)?;
            }
            Instruction::Call { dest, func, args, bindings, .. } => {
                self.compile_call(builder, *dest, func, args, bindings)?;
            }
            // ComptimeCall behaves exactly like Call - the specialization metadata is
            // only used by the specialization pass. Without specialization, this calls
            // the original function with original args.
            Instruction::ComptimeCall { dest, func, args, bindings, .. } => {
                self.compile_call(builder, *dest, func, args, bindings)?;
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
            Instruction::DropField { base, field_path } => {
                self.compile_drop_field(builder, base, field_path)?;
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
}
