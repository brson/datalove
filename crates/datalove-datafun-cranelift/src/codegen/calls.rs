//! Function call instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use cranelift_codegen::ir::types as cl_types;
use datalove_datafun_ir::{CodeRef, CodeUnitId, IrType, Operand, ParamId, ParamMode, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::{uses_sret, FunctionCompiler};

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a Call instruction.
    ///
    /// Threads rt_handle as implicit first argument to callee.
    /// For native rider functions, marshals args from pointers to i64 scalars
    /// and converts the i64 return value back to the destination type.
    /// How a rider takes each of its parameters.
    fn native_param_modes(&self, code_ref: &CodeRef) -> Vec<ParamMode> {
        let CodeRef::Module { module, id } = code_ref else { return Vec::new() };
        let Some(registry) = self.registry else { return Vec::new() };
        registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
            .and_then(|unit| unit.native_context().map(|c| c.param_modes.clone()))
            .unwrap_or_default()
    }

    /// The shapes `code_ref` declared, read from the same place its parameter
    /// descriptors are, since the two together are its trailing arguments.
    fn callee_descriptor_shapes(
        &self,
        code_ref: &CodeRef,
    ) -> Vec<datalove_datafun_ir::DescriptorShape> {
        match code_ref {
            CodeRef::Local(id) | CodeRef::External { id, .. } => {
                self.local_funcs.get(&CodeUnitId(id.0))
                    .map(|callee| callee.descriptor_shapes.clone())
                    .unwrap_or_default()
            }
            CodeRef::Module { module, id } => {
                let Some(registry) = self.registry else { return Vec::new() };
                registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
                    .and_then(|unit| unit.function_context()
                        .map(|c| c.descriptor_shapes.clone()))
                    .unwrap_or_default()
            }
        }
    }

    /// The parameters of `code_ref` whose descriptor the call site supplies.
    ///
    /// This has to agree with what `build_signature_for_func` put in the
    /// callee's signature, so both read the same `descriptor_params`. A
    /// function compiled beside this one carries its own on the `LocalCallee`
    /// it was declared as; one in a module is read back off the registry.
    fn callee_descriptor_params(&self, code_ref: &CodeRef) -> Vec<ParamId> {
        match code_ref {
            CodeRef::Local(id) | CodeRef::External { id, .. } => {
                self.local_funcs.get(&CodeUnitId(id.0))
                    .expect("resolve_code_ref rejects a local callee never declared")
                    .descriptor_params
                    .clone()
            }
            CodeRef::Module { module, id } => {
                let Some(registry) = self.registry else {
                    return Vec::new();
                };
                registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
                    .and_then(|unit| unit.function_context().map(|c| c.descriptor_params.clone()))
                    .unwrap_or_default()
            }
        }
    }

    /// How `code_ref` takes each of its parameters.
    ///
    /// Read from the same place the descriptor list is, and for the same
    /// reason: the instruction says what to pass but not how the callee
    /// takes it, and an `out` parameter is the caller's to clear.
    fn callee_param_modes(&self, code_ref: &CodeRef) -> Vec<ParamMode> {
        match code_ref {
            CodeRef::Local(id) | CodeRef::External { id, .. } => {
                self.local_funcs.get(&CodeUnitId(id.0))
                    .map(|callee| callee.param_modes.clone())
                    .unwrap_or_default()
            }
            CodeRef::Module { module, id } => {
                let Some(registry) = self.registry else {
                    return Vec::new();
                };
                registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
                    .and_then(|unit| unit.function_context().map(|c| c.param_modes.clone()))
                    .unwrap_or_default()
            }
        }
    }

    /// Destroy whatever an `out` argument's destination holds now.
    ///
    /// The callee writes a fresh value there and its tracking byte starts
    /// uninitialized, so its first store destroys nothing. Something has to,
    /// or the old value is simply dropped on the floor: this is the same step
    /// the interpreter takes before it binds an out parameter.
    fn destroy_out_destinations(
        &mut self,
        builder: &mut FunctionBuilder,
        code_ref: &CodeRef,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
        let modes = self.callee_param_modes(code_ref);
        if !modes.iter().any(|m| *m == ParamMode::Out) {
            return Ok(());
        }

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("out parameter requires runtime handle".into())
        })?;
        let destroy = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen(
                "out parameter requires runtime imports".into()))?
            .destroy_local;

        for (i, arg) in args.iter().enumerate() {
            if modes.get(i) != Some(&ParamMode::Out) {
                continue;
            }
            let ptr = self.get_operand_ptr(builder, arg)?;
            let tydesc = self.operand_tydesc(builder, arg)?;
            let destroy_ref = self.module.declare_func_in_func(destroy, builder.func);

            // This function's own out parameter, passed straight on, names a
            // destination its caller already cleared and nothing has written
            // to since. Its tracking byte says so, and freeing what was freed
            // is worse than leaking it.
            let guard = match arg {
                Operand::Param(param) => self.param_tracking_byte_offset(*param),
                _ => None,
            };
            let Some(track_offset) = guard else {
                builder.ins().call(destroy_ref, &[rt_handle, ptr, tydesc]);
                continue;
            };

            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("out parameter tracking requires frame slot".into())
            })?;
            let track_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, track_offset as i32);
            let track_val = builder.ins().load(cl_types::I8, MemFlagsData::new(), track_addr, 0);
            let live = builder.ins().iconst(cl_types::I8, crate::layout::tracking::LIVE as i64);
            let is_live = builder.ins().icmp(
                cranelift_codegen::ir::condcodes::IntCC::Equal, track_val, live);

            let destroy_block = builder.create_block();
            let after_block = builder.create_block();
            builder.ins().brif(is_live, destroy_block, &[], after_block, &[]);

            builder.switch_to_block(destroy_block);
            builder.seal_block(destroy_block);
            builder.ins().call(destroy_ref, &[rt_handle, ptr, tydesc]);
            builder.ins().jump(after_block, &[]);

            builder.switch_to_block(after_block);
            builder.seal_block(after_block);
        }
        Ok(())
    }

    /// Borrow what a wrapped argument holds: its value pointer and descriptor.
    ///
    /// A container of a type parameter travels wrapped once it is owned, and a
    /// borrowed parameter wants the container itself with a descriptor beside
    /// it. Both come out of the wrapper together, so they are read together
    /// and neither can be taken from somewhere the other was not.
    fn borrow_through_wrapper(
        &mut self,
        builder: &mut FunctionBuilder,
        arg: &Operand,
    ) -> Result<(cl_ir::Value, cl_ir::Value), CraneliftError> {
        let parts = self.runtime.as_ref()
            .ok_or_else(|| CraneliftError::Codegen(
                "borrowing through a wrapper requires runtime imports".into()))?
            .data_parts;

        let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot, 16, 3));
        let value_out = builder.ins().stack_addr(PTR_TYPE, slot, 0);
        let tydesc_out = builder.ins().stack_addr(PTR_TYPE, slot, 8);

        let data_ptr = self.get_operand_ptr(builder, arg)?;
        let parts_ref = self.module.declare_func_in_func(parts, builder.func);
        builder.ins().call(parts_ref, &[data_ptr, value_out, tydesc_out]);

        let value = builder.ins().load(PTR_TYPE, MemFlagsData::new(), value_out, 0);
        let tydesc = builder.ins().load(PTR_TYPE, MemFlagsData::new(), tydesc_out, 0);
        Ok((value, tydesc))
    }

    /// Whether an argument arrives wrapped where the callee wants it borrowed.
    fn arg_needs_unwrapping(
        &self,
        code_ref: &CodeRef,
        args: &[Operand],
        index: usize,
    ) -> Result<bool, CraneliftError> {
        let modes = self.callee_param_modes(code_ref);
        if !matches!(modes.get(index), Some(ParamMode::Ref) | Some(ParamMode::Mut)) {
            return Ok(false);
        }
        // One of our own borrowed parameters is already a pointer at the value
        // with a descriptor beside it, never a wrapper around it, even though
        // its type reads `data`. Reading through it would take the first bytes
        // of the value for a wrapper's two pointers.
        if let Operand::Param(param_id) = &args[index] {
            if self.descriptor_values.contains_key(param_id) {
                return Ok(false);
            }
        }
        Ok(self.get_operand_type(&args[index])? == IrType::Data)
    }

    /// The descriptor for an operand, as a runtime pointer.
    ///
    /// A parameter whose descriptor was supplied by our own caller uses that:
    /// its static type says `data` where a type parameter stood, so a
    /// descriptor built from that type would misdescribe the value. Everything
    /// else is described by its type, and the descriptor is a static symbol.
    pub(super) fn operand_tydesc(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
        if let Operand::Param(param_id) = operand {
            if let Some(&supplied) = self.descriptor_values.get(param_id) {
                return Ok(supplied);
            }
        }
        let ty = self.get_operand_type(operand)?;
        self.static_tydesc(builder, &ty)
    }

    /// The static descriptor symbol for a type, as a runtime pointer.
    pub(super) fn static_tydesc(
        &mut self,
        builder: &mut FunctionBuilder,
        ty: &IrType,
    ) -> Result<cl_ir::Value, CraneliftError> {
        // A reference describes the type it points at.
        let ty = match ty {
            IrType::Ref(inner) => inner.as_ref().clone(),
            other => other.clone(),
        };
        let tydesc_id = self.tydesc_emitter.get(&ty).ok_or_else(|| {
            CraneliftError::Codegen(format!("TyDesc not found for type {:?}", ty))
        })?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        Ok(builder.ins().symbol_value(PTR_TYPE, tydesc_gv))
    }

    pub(super) fn compile_call(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        code_ref: &CodeRef,
        args: &[Operand],
        shape_descriptors: &[datalove_datafun_ir::DescriptorRef],
    ) -> Result<(), CraneliftError> {
        // Check if target is a native rider function.
        if let CodeRef::Module { module, id } = code_ref {
            let ir_func_id = CodeUnitId(id.0);
            if let Some(registry) = self.registry {
                if let Some(unit) = registry.get_module_function_as_unit(*module, ir_func_id) {
                    if unit.native_context().is_some() {
                        return self.compile_native_call(builder, dest, code_ref, args);
                    }
                }
            }
        }

        // Regular function call path.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Call requires runtime handle".into())
        })?;

        let callee_func_id = self.resolve_code_ref(code_ref)?;

        // All non-Unit returns use sret convention.
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let callee_uses_sret = uses_sret(dest_ty);

        // Build call arguments: [rt_handle, sret?, user_args...]
        let mut call_args = Vec::with_capacity(2 + args.len());
        call_args.push(rt_handle);

        // Allocate space in caller's frame and pass sret pointer.
        if callee_uses_sret {
            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("no frame slot for sret return value".into())
            })?;
            let dest_offset = self.layout.value_offset(dest.0);
            let ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
            call_args.push(ptr);

            // For aggregates, the pointer IS the value (used by downstream as address).
            // For scalars, we load the value from sret location after the call.
            match types::ir_type_to_cranelift(dest_ty) {
                CraneliftRepr::Aggregate(_) => {
                    self.values.insert(dest, ptr);
                }
                CraneliftRepr::Scalar(_) => {
                    // Will load after call below.
                }
            }
        }

        self.destroy_out_destinations(builder, code_ref, args)?;

        // Add user arguments (passed by pointer). An argument that arrives
        // wrapped where the callee wants it borrowed is read through first,
        // and the descriptor that comes with it is used below.
        let mut borrowed_parts: Vec<Option<(cl_ir::Value, cl_ir::Value)>> =
            vec![None; args.len()];
        for (i, arg) in args.iter().enumerate() {
            if self.arg_needs_unwrapping(code_ref, args, i)? {
                borrowed_parts[i] = Some(self.borrow_through_wrapper(builder, arg)?);
            }
        }
        for (i, arg) in args.iter().enumerate() {
            let arg_val = match borrowed_parts[i] {
                Some((value, _)) => value,
                None => self.get_operand_ptr(builder, arg)?,
            };
            call_args.push(arg_val);
        }

        // Then a descriptor for each parameter whose own type does not describe
        // what it will receive. This is the place that knows: the argument here
        // has a concrete type, or a descriptor our own caller handed us.
        for param_id in self.callee_descriptor_params(code_ref) {
            let index = param_id.0 as usize;
            let arg = args.get(index).ok_or_else(|| {
                CraneliftError::Codegen(format!(
                    "callee wants a descriptor for parameter {} but got {} arguments",
                    param_id.0, args.len(),
                ))
            })?.clone();
            let tydesc_addr = match borrowed_parts.get(index).copied().flatten() {
                Some((_, tydesc)) => tydesc,
                None => self.operand_tydesc(builder, &arg)?,
            };
            call_args.push(tydesc_addr);
        }

        // Then one for each shape the callee builds a collection of, as worked
        // out when the shape sets settled. Read rather than derived, so this
        // side and the callee's signature cannot disagree about the trailing
        // arguments, and so the descriptor emitter saw the same types.
        for r in shape_descriptors {
            let value = match r {
                datalove_datafun_ir::DescriptorRef::Static(ty) => {
                    self.static_tydesc(builder, ty)?
                }
                datalove_datafun_ir::DescriptorRef::Own(i) => {
                    *self.shape_descriptor_values.get(*i as usize).ok_or_else(|| {
                        CraneliftError::Codegen(format!(
                            "shape {} was declared but never passed", i))
                    })?
                }
            };
            call_args.push(value);
        }

        // Declare callee in this function and emit call.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);
        builder.ins().call(callee_ref, &call_args);

        // For scalar sret returns, load the value from the sret location.
        if callee_uses_sret {
            if let CraneliftRepr::Scalar(scalar_ty) = types::ir_type_to_cranelift(dest_ty) {
                let frame_slot = self.frame_slot.unwrap();
                let dest_offset = self.layout.value_offset(dest.0);
                let ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
                let loaded = builder.ins().load(scalar_ty, MemFlagsData::new(), ptr, 0);
                self.values.insert(dest, loaded);
            }
        }

        Ok(())
    }

    /// Compile a call to a native rider function.
    ///
    /// Matches the runtime C ABI: each arg is a `(ptr, tydesc)` pair,
    /// return value via out-param, function returns RtStatus.
    ///
    /// `fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> i8`
    fn compile_native_call(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        code_ref: &CodeRef,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
        use datalove_datafun_ir::IrType;

        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Call requires runtime handle".into())
        })?;

        let callee_func_id = self.resolve_code_ref(code_ref)?;

        // Build call arguments: [rt_handle, (ptr, tydesc)..., result_out, result_tydesc]
        let mut call_args = Vec::with_capacity(1 + args.len() * 2 + 2);
        call_args.push(rt_handle);

        // A collection of a type parameter is wrapped once it is owned, and a
        // rider works on the collection itself with a descriptor beside it.
        // Both are inside the wrapper, so an argument that arrives wrapped
        // where the native wants the thing itself is read through first. The
        // interpreter does this for every call; here it was done for calls to
        // module functions and not for calls to riders, so a generic handing a
        // list it had built to `list_push` gave it a `data`.
        // Decided by the mode, not by the native's own parameter type: a
        // generic native's types are erased too, so `mut self: [T]` reads
        // `data` there just as the argument does. What tells them apart is that
        // the native borrows its collection and takes its element, and only a
        // borrow is passed through a wrapper.
        let native_param_modes = self.native_param_modes(code_ref);
        for (i, arg) in args.iter().enumerate() {
            // One of our own borrowed parameters is already a pointer at the
            // value with a descriptor beside it, never a wrapper around it,
            // even though its type reads `data`. Same as at a call to a module
            // function.
            let forwarded = matches!(arg, Operand::Param(p)
                if self.descriptor_values.contains_key(p));
            let wants_unwrapping = !forwarded
                && self.get_operand_type(arg)? == IrType::Data
                && matches!(native_param_modes.get(i),
                    Some(ParamMode::Ref) | Some(ParamMode::Mut));
            let (arg_ptr, tydesc_addr) = if wants_unwrapping {
                self.borrow_through_wrapper(builder, arg)?
            } else {
                (self.get_operand_ptr(builder, arg)?, self.operand_tydesc(builder, arg)?)
            };

            call_args.push(arg_ptr);
            call_args.push(tydesc_addr);
        }

        // Result out-param: pointer to dest slot + tydesc.
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for native call result".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        let dest_tydesc_id = self.tydesc_emitter.get(dest_ty).ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "TyDesc not found for return type {:?} in native call", dest_ty
            ))
        })?;
        let dest_tydesc_gv = self.module.declare_data_in_func(dest_tydesc_id, builder.func);
        let dest_tydesc_addr = builder.ins().symbol_value(PTR_TYPE, dest_tydesc_gv);

        call_args.push(dest_ptr);
        call_args.push(dest_tydesc_addr);

        // Declare callee and emit call.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);
        let _call_inst = builder.ins().call(callee_ref, &call_args);

        // Result was written to dest_ptr by the callee.
        // For scalars, load the value; for aggregates, use the pointer.
        match types::ir_type_to_cranelift(dest_ty) {
            CraneliftRepr::Scalar(scalar_ty) => {
                let loaded = builder.ins().load(scalar_ty, MemFlagsData::new(), dest_ptr, 0);
                self.values.insert(dest, loaded);
            }
            CraneliftRepr::Aggregate(_) => {
                self.values.insert(dest, dest_ptr);
            }
        }

        Ok(())
    }

    /// Resolve a CodeRef to a Cranelift FuncId.
    pub(super) fn resolve_code_ref(&mut self, code_ref: &CodeRef) -> Result<FuncId, CraneliftError> {
        match code_ref {
            CodeRef::Local(id) => {
                let ir_func_id = CodeUnitId(id.0);
                if let Some(callee) = self.local_funcs.get(&ir_func_id) {
                    return Ok(callee.func_id);
                }

                Err(CraneliftError::Unsupported(format!(
                    "local function {:?} not yet declared - needs two-pass compilation",
                    id
                )))
            }
            CodeRef::External { unit, id } => {
                Err(CraneliftError::Unsupported(format!(
                    "external function call (unit={}, id={:?}) not yet implemented",
                    unit, id
                )))
            }
            CodeRef::Module { module, id } => {
                let ir_func_id = CodeUnitId(id.0);
                self.module_funcs.get(&(*module, ir_func_id)).copied().ok_or_else(|| {
                    CraneliftError::Unsupported(format!(
                        "module function ({:?}, {:?}) not pre-compiled",
                        module, id
                    ))
                })
            }
        }
    }
}
