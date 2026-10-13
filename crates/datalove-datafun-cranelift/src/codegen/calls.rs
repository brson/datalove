//! Function call instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use datalove_datafun_ir::{CodeRef, CodeUnitId, IrType, Operand, ParamMode, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::{uses_sret, FunctionCompiler, LocalCallee};
use datalove_rt::c::{self, RtHeader};

/// Where in what the runtime handle points at compiled code finds whether its
/// calls are being abandoned, and the limit to the stack; see `RtHeader`.
const ABANDONING_OFFSET: i32 = std::mem::offset_of!(RtHeader, abandoning) as i32;
const STACK_LIMIT_OFFSET: i32 = std::mem::offset_of!(RtHeader, stack_limit) as i32;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// The callee a `Local` or `External` reference names, as it was declared.
    ///
    /// Two keyspaces, because a `CodeUnitId` numbers one unit's functions from
    /// zero and says nothing about whose: a body that calls its own function 0
    /// and an earlier unit's function 0 names two different functions by the
    /// same id.
    fn declared_callee(&self, code_ref: &CodeRef) -> Option<&LocalCallee> {
        match code_ref {
            CodeRef::Local(id) => self.local_funcs.get(&CodeUnitId(id.0)),
            CodeRef::External { unit, id } => {
                self.external_funcs.get(&(*unit, CodeUnitId(id.0)))
            }
            CodeRef::Module { .. } => None,
        }
    }

    /// Whether `code_ref`'s signature takes a pointer to return through.
    fn callee_uses_sret(&self, code_ref: &CodeRef) -> bool {
        match code_ref {
            CodeRef::Local(_) | CodeRef::External { .. } => {
                self.declared_callee(code_ref)
                    .expect("resolve_code_ref rejects a callee never declared")
                    .uses_sret
            }
            CodeRef::Module { module, id } => {
                let registry = self.registry.expect("a module callee is resolved through the registry");
                let unit = registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
                    .expect("resolve_code_ref rejects a callee never declared");
                let ctx = unit.function_context().expect("a module callee that is not native is a function");
                uses_sret(&ctx.return_type)
            }
        }
    }

    /// How `code_ref` takes each of its parameters.
    ///
    /// The instruction says what to pass but not how the callee takes it, and
    /// an `out` parameter is the caller's to clear.
    fn callee_param_modes(&self, code_ref: &CodeRef) -> Vec<ParamMode> {
        match code_ref {
            CodeRef::Local(_) | CodeRef::External { .. } => {
                self.declared_callee(code_ref)
                    .expect("resolve_code_ref rejects a callee never declared")
                    .param_modes
                    .clone()
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

    /// Record that the call wrote every `out` argument's destination.
    ///
    /// The callee promises to write one before it returns, so the destination
    /// holds a value afterwards and whatever drops it has to know. Without this
    /// an uninitialized `var` passed as `out` came back still reading
    /// uninitialized, and the drop at the end of its scope skipped what the
    /// call had put there. A destination that was already initialized dropped
    /// correctly, its tracking byte having been live since before the call,
    /// which is why this went unseen.
    fn mark_out_destinations_live(
        &mut self,
        builder: &mut FunctionBuilder,
        code_ref: &CodeRef,
        args: &[Operand],
    ) {
        let modes = self.callee_param_modes(code_ref);
        for (i, arg) in args.iter().enumerate() {
            if modes.get(i) != Some(&ParamMode::Out) {
                continue;
            }
            match arg {
                Operand::Param(param) => self.mark_param_live(builder, *param),
                other => self.mark_tracking_live(builder, other),
            }
        }
    }

    /// The descriptor for an operand, as a runtime pointer: its type's static
    /// symbol, or for a dynamic type its table entry.
    pub(super) fn operand_tydesc(
        &mut self,
        builder: &mut FunctionBuilder,
        operand: &Operand,
    ) -> Result<cl_ir::Value, CraneliftError> {
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
        // A dynamic type's descriptor is its table entry.
        if ty.is_dynamic() {
            return self.table_entry(&ty);
        }
        let tydesc_id = self.tydesc(&ty)?;
        let tydesc_gv = self.module.declare_data_in_func(tydesc_id, builder.func);
        Ok(builder.ins().symbol_value(PTR_TYPE, tydesc_gv))
    }

    pub(super) fn compile_call(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        code_ref: &CodeRef,
        args: &[Operand],
        bindings: &[IrType],
    ) -> Result<(), CraneliftError> {
        // Check if target is a native rider function.
        if let CodeRef::Module { module, id } = code_ref {
            let ir_func_id = CodeUnitId(id.0);
            if let Some(registry) = self.registry {
                if let Some(unit) = registry.get_module_function_as_unit(*module, ir_func_id) {
                    if let Some(native) = unit.native_context() {
                        // A descriptor for each type parameter no argument
                        // determines, from what this call bound it to.
                        let undetermined = native.undetermined_bindings(bindings);
                        let modes = native.param_modes.clone();
                        self.compile_native_call(builder, dest, code_ref, args, &undetermined)?;
                        return self.mark_in_arguments_moved(builder, &modes, args);
                    }
                }
            }
        }

        // Regular function call path.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Call requires runtime handle".into())
        })?;

        let callee_func_id = self.resolve_code_ref(code_ref)?;

        // The callee's own return type says whether it returns through a
        // pointer; a generic returning `T` does even where this call bound `T`
        // to `()`.
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let callee_uses_sret = self.callee_uses_sret(code_ref);

        // Build call arguments: [rt_handle, sret?, user_args...]
        let mut call_args = Vec::with_capacity(2 + args.len());
        call_args.push(rt_handle);

        // Allocate space in caller's frame and pass sret pointer.
        if callee_uses_sret && uses_sret(dest_ty) {
            let ptr = self.dest_addr(builder, dest)?;
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
        } else {
            // A unit return travels in no register and needs no room written
            // to, but it is still a value here, and something may go on to
            // read it -- `debuglog f()` does. So it gets the address of its
            // own slot, which has no size and nothing in it.
            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("no frame slot for a unit return".into())
            })?;
            let dest_offset = self.layout.value_offset(dest.0);
            let ptr = frame_slot.addr(builder, dest_offset as i32);
            self.values.insert(dest, ptr);
            // A generic returning a `T` bound to `()` still takes the
            // pointer, and writes nothing through it.
            if callee_uses_sret {
                call_args.push(ptr);
            }
        }


        // Add user arguments, passed by pointer.
        for arg in args {
            let arg_val = self.get_operand_ptr(builder, arg)?;
            call_args.push(arg_val);
        }

        // Then a descriptor for each of the callee's type parameters, for what
        // this call bound it to.
        for binding in bindings {
            let value = self.static_tydesc(builder, binding)?;
            call_args.push(value);
        }

        // Declare callee in this function and emit call.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);
        builder.ins().call(callee_ref, &call_args);
        self.return_if_abandoning(builder)?;

        self.mark_out_destinations_live(builder, code_ref, args);
        let modes = self.callee_param_modes(code_ref);
        self.mark_in_arguments_moved(builder, &modes, args)?;

        // For scalar sret returns, load the value from the sret location.
        if callee_uses_sret && uses_sret(dest_ty) {
            if let CraneliftRepr::Scalar(scalar_ty) = types::ir_type_to_cranelift(dest_ty) {
                let frame_slot = self.frame_slot.unwrap();
                let dest_offset = self.layout.value_offset(dest.0);
                let ptr = frame_slot.addr(builder, dest_offset as i32);
                let loaded = builder.ins().load(scalar_ty, MemFlagsData::new(), ptr, 0);
                self.values.insert(dest, loaded);
            }
        }

        Ok(())
    }

    /// In a function's entry, stop rather than overflow the stack if it is past
    /// the runtime's limit; see `RtHeader::stack_limit`.
    ///
    /// The JIT's code abandons the calls under way, back to the interpreter
    /// that entered it, which reports a stack overflow. Code with no
    /// interpreter under it, AOT's, has no way back up and ends the process
    /// with a message (`dtlv_rti_stack_overflow`).
    pub(super) fn check_stack(&mut self, builder: &mut FunctionBuilder) -> Result<(), CraneliftError> {
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("a stack check requires the runtime handle".into())
        })?;
        let sp = builder.ins().get_stack_pointer(PTR_TYPE);
        let limit = builder.ins().load(
            PTR_TYPE, MemFlagsData::new(), rt_handle, STACK_LIMIT_OFFSET);
        let too_deep = builder.ins().icmp(cl_ir::condcodes::IntCC::UnsignedLessThan, sp, limit);
        let stop = builder.create_block();
        let carry_on = builder.create_block();
        builder.ins().brif(too_deep, stop, &[], carry_on, &[]);
        builder.set_cold_block(stop);
        builder.switch_to_block(stop);
        builder.seal_block(stop);
        if self.abandon_on_errors {
            let reason = builder.ins().iconst(cl_ir::types::I8, c::ABANDONED_FOR_STACK as i64);
            builder.ins().store(MemFlagsData::new(), reason, rt_handle, ABANDONING_OFFSET);
            builder.ins().return_(&[]);
        } else {
            let overflow = self.module.declare_func_in_func(self.runtime.stack_overflow, builder.func);
            builder.ins().call(overflow, &[rt_handle]);
            builder.ins().trap(cl_ir::TrapCode::STACK_OVERFLOW);
        }
        builder.switch_to_block(carry_on);
        builder.seal_block(carry_on);
        Ok(())
    }

    /// Return at once if the call just made is being abandoned: an
    /// interpreted function under it failed, and every compiled frame between
    /// it and the interpreter goes without finishing. JIT-compiled code only;
    /// see `RtHeader::abandoning`.
    fn return_if_abandoning(&mut self, builder: &mut FunctionBuilder) -> Result<(), CraneliftError> {
        if !self.abandon_on_errors {
            return Ok(());
        }
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("a call that can be abandoned requires the runtime handle".into())
        })?;
        let flag = builder.ins().load(cl_ir::types::I8, MemFlagsData::new(), rt_handle, ABANDONING_OFFSET);
        let abandon = builder.create_block();
        let carry_on = builder.create_block();
        builder.ins().brif(flag, abandon, &[], carry_on, &[]);
        builder.switch_to_block(abandon);
        builder.seal_block(abandon);
        builder.ins().return_(&[]);
        builder.switch_to_block(carry_on);
        builder.seal_block(carry_on);
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
        undetermined: &[IrType],
    ) -> Result<(), CraneliftError> {
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Call requires runtime handle".into())
        })?;

        let callee_func_id = self.resolve_code_ref(code_ref)?;

        // Build call arguments: [rt_handle, (ptr, tydesc)..., result_out, result_tydesc]
        let mut call_args = Vec::with_capacity(1 + args.len() * 2 + 2);
        call_args.push(rt_handle);

        for arg in args {
            call_args.push(self.get_operand_ptr(builder, arg)?);
            call_args.push(self.operand_tydesc(builder, arg)?);
        }

        // Result out-param: pointer to dest slot + tydesc.
        let dest_ty = self.func.value_types[dest.0 as usize].clone();
        let dest_ptr = self.dest_addr(builder, dest)?;
        let dest_tydesc_addr = self.static_tydesc(builder, &dest_ty)?;

        call_args.push(dest_ptr);
        call_args.push(dest_tydesc_addr);

        // Then a descriptor for each type parameter no argument determines,
        // for what the call bound it to.
        for ty in undetermined {
            let value = self.static_tydesc(builder, ty)?;
            call_args.push(value);
        }

        // Declare callee and emit call.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);
        let _call_inst = builder.ins().call(callee_ref, &call_args);

        // Result was written to dest_ptr by the callee.
        // For scalars, load the value; for aggregates, use the pointer.
        match types::ir_type_to_cranelift(&dest_ty) {
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
            CodeRef::Local(_) | CodeRef::External { .. } => {
                Ok(self.declared_callee(code_ref)
                    .unwrap_or_else(|| panic!(
                        "callee {:?} was not declared; every call target is declared \
                         before the body naming it is compiled",
                        code_ref))
                    .func_id)
            }
            CodeRef::Module { module, id } => {
                let ir_func_id = CodeUnitId(id.0);
                Ok(*self.module_funcs.get(&(*module, ir_func_id)).unwrap_or_else(|| panic!(
                    "module function {:?}::{:?} was not declared; every call target is \
                     declared before the body naming it is compiled",
                    module, id)))
            }
        }
    }
}
