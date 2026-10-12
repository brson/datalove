//! Operands and values: what a `ValueId` is in Cranelift, an operand's
//! address, value and type.

use super::*;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Record a result the runtime wrote into `result_ptr` as `dest`.
    ///
    /// `values` holds an address for an aggregate and the value itself for a
    /// scalar, and every read of an operand takes what is there as one or the
    /// other by the type. A runtime call writes through a pointer whatever
    /// the type, so a scalar destination has to be loaded back out of it --
    /// left as the address, the next read of it takes the address for the
    /// value. That is how `(term Note : u32 / 42)@` came out as a stack
    /// address widened into the enum: a term is transparent over its payload,
    /// so cloning one has a scalar destination.
    pub(super) fn record_runtime_result(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        result_ptr: cl_ir::Value,
    ) {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        match types::ir_type_to_cranelift(dest_ty) {
            CraneliftRepr::Scalar(cl_ty) => {
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), result_ptr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                self.values.insert(dest, result_ptr);
            }
        }
    }

    /// Get a pointer to an operand's value.
    ///
    /// For aggregates already in memory, returns the pointer directly.
    /// For scalars in registers, spills to a temporary stack location.
    pub(super) fn get_operand_ptr(
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
            if let Some(&ptr) = self.dyn_slot_storage.get(slot_id) {
                return Ok(ptr);
            }
            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("no frame slot for slot operand".into())
            })?;
            let slot_offset = self.layout.slot_offset(slot_id.0);
            let addr = frame_slot.addr(builder, slot_offset as i32);
            return Ok(addr);
        }

        // ValueRef: the stored value is a pointer - return it directly.
        // This is used for ref/mut/out params to get the dereferenced location.
        if let Operand::ValueRef(vid) = operand {
            return self.value(builder, *vid);
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
                    let addr = frame_slot.addr(builder, offset as i32);
                    builder.ins().store(MemFlagsData::new(), val, addr, 0);
                    return Ok(addr);
                }

                // For other operand types, create a temporary slot.
                // This is a simple approach - we create a new stack slot for each spill.
                let size = cl_ty.bytes();
                let slot_data = cl_ir::StackSlotData::new(
                    cl_ir::StackSlotKind::ExplicitSlot,
                    size,
                    types::align_shift(size.max(1)),
                );
                let temp_slot = builder.create_sized_stack_slot(slot_data);
                let addr = builder.ins().stack_addr(PTR_TYPE, temp_slot, 0);
                builder.ins().store(MemFlagsData::new(), val, addr, 0);
                Ok(addr)
            }
        }
    }

    /// Get a Cranelift value for an operand.
    pub(super) fn get_operand_value(
        &self,
        builder: &mut FunctionBuilder,
        op: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
        match op {
            Operand::Value(vid) => self.value(builder, *vid),
            Operand::ValueRef(vid) => {
                // ValueRef: the stored value is a pointer. Dereference it.
                let ptr = self.value(builder, *vid)?;

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
                        Ok(builder.ins().load(cl_ty, MemFlagsData::new(), ptr, 0))
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
                        Ok(builder.ins().load(cl_ty, MemFlagsData::new(), param_ptr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // For aggregates, return the pointer itself.
                        Ok(param_ptr)
                    }
                }
            }
            Operand::Slot(slot_id) if self.dyn_slot_storage.contains_key(slot_id) => {
                Ok(self.dyn_slot_storage[slot_id])
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
                        let addr = frame_slot.addr(builder, slot_offset as i32);
                        Ok(builder.ins().load(cl_ty, MemFlagsData::new(), addr, 0))
                    }
                    CraneliftRepr::Aggregate(_) => {
                        // Aggregate: return pointer to slot location.
                        Ok(frame_slot.addr(builder, slot_offset as i32))
                    }
                }
            }
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                Err(CraneliftError::Unsupported(format!(
                    "reading {:?}, a binding of an earlier script unit",
                    op
                )))
            }
        }
    }

    /// Get the type of an operand.
    pub(super) fn get_operand_type(&self, op: &Operand) -> Result<IrType, CraneliftError> {
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
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                Err(CraneliftError::Unsupported(format!(
                    "the type of {:?}, a binding of an earlier script unit",
                    op
                )))
            }
        }
    }

    /// The Cranelift value for `vid`: what defined it, or in an OSR entry, for
    /// one defined before the loop, what the interpreter's frame holds.
    pub(super) fn value(&self, builder: &mut FunctionBuilder, vid: ValueId) -> Result<cl_ir::Value, CraneliftError> {
        if let Some(v) = self.values.get(&vid) {
            return Ok(*v);
        }
        let Some(osr) = &self.osr else {
            return Err(CraneliftError::Codegen(format!("undefined value: {:?}", vid)));
        };
        if let Some(&var) = osr.carried.get(&vid) {
            return Ok(builder.use_var(var));
        }
        self.materialize(builder, vid)
    }

    /// Emit a zero constant for a scalar Cranelift type.
    ///
    /// Handles float types (f32const/f64const) and integer types (iconst).
    pub(super) fn emit_scalar_zero(
        builder: &mut FunctionBuilder,
        cl_ty: cl_ir::Type,
    ) -> cl_ir::Value {
        if cl_ty == cl_types::F32 {
            builder.ins().f32const(0.0f32)
        } else if cl_ty == cl_types::F64 {
            builder.ins().f64const(0.0f64)
        } else {
            builder.ins().iconst(cl_ty, 0)
        }
    }
}
