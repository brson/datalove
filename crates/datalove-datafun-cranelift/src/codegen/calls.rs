//! Function call instruction compilation.

use rmx::prelude::*;
use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use datalove_datafun_ir::{CodeRef, CodeUnitId, IrType, Operand, ParamMode, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::{has_result, ArgPass, FunctionCompiler, LocalCallee, ParamLayout, RetPass};
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

    /// Where a call to `code_ref` passes each argument, and how it gets its
    /// result.
    fn callee_layout(&self, code_ref: &CodeRef) -> ParamLayout {
        match code_ref {
            CodeRef::Local(_) | CodeRef::External { .. } => {
                self.declared_callee(code_ref)
                    .expect("resolve_code_ref rejects a callee never declared")
                    .layout
                    .C()
            }
            CodeRef::Module { module, id } => {
                let registry = self.registry.expect("a module callee is resolved through the registry");
                let unit = registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
                    .expect("resolve_code_ref rejects a callee never declared");
                let ctx = unit.function_context().expect("a module callee that is not native is a function");
                ParamLayout::of(ctx)
            }
        }
    }

    /// How `code_ref` takes each of its parameters.
    ///
    /// The instruction says what to pass but not how the callee takes it, and
    /// an `out` parameter is the caller's to clear.
    pub(super) fn callee_param_modes(&self, code_ref: &CodeRef) -> Vec<ParamMode> {
        match code_ref {
            CodeRef::Local(_) | CodeRef::External { .. } => {
                self.declared_callee(code_ref)
                    .expect("resolve_code_ref rejects a callee never declared")
                    .param_modes
                    .C()
            }
            CodeRef::Module { module, id } => {
                let Some(registry) = self.registry else {
                    return Vec::new();
                };
                registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
                    .and_then(|unit| match unit.function_context() {
                        Some(c) => Some(c.param_modes.C()),
                        None => unit.native_context().map(|n| n.param_modes.C()),
                    })
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
            IrType::Ref(inner) => inner.as_ref().C(),
            other => other.C(),
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
                        let modes = native.param_modes.C();
                        let open_coded = match native.intrinsic {
                            Some(intrinsic) => self.compile_native_intrinsic(builder, intrinsic, &modes, dest, args)?,
                            None => false,
                        };
                        if !open_coded {
                            // A field place has no address to hand the native;
                            // `find_register_slots` makes one only for a call
                            // that is open-coded.
                            assert!(
                                !args.iter().any(|a| matches!(a, Operand::ValueRef(r) if self.field_places.contains_key(r))),
                                "a field place handed to a native call that was not open-coded"
                            );
                            self.compile_native_call(builder, dest, code_ref, args, &undetermined)?;
                        }
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
        let layout = self.callee_layout(code_ref);
        let callee_uses_sret = layout.sret.is_some();
        let mut call_args = vec![rt_handle; layout.len()];

        // Allocate space in caller's frame and pass sret pointer.
        if callee_uses_sret && has_result(dest_ty) {
            let ptr = self.dest_addr(builder, dest)?;
            call_args[layout.sret.expect("checked")] = ptr;

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
        } else if matches!(layout.ret, RetPass::Value(_)) {
            // Defined from the call's results below.
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
            if let Some(at) = layout.sret {
                call_args[at] = ptr;
            }
        }

        // Each argument by pointer, or by value where the callee takes it so.
        let param_types = self.callee_param_types(code_ref);
        assert_eq!(args.len(), layout.args.len(), "an argument per parameter");
        for (i, (arg, ((pass, at), ty))) in args.iter().zip(layout.args.iter().zip(&param_types)).enumerate() {
            match pass {
                ArgPass::Pointer => call_args[at.start] = self.arg_ptr(builder, args, i)?,
                ArgPass::Value(_) => {
                    let regs = match types::ir_type_to_cranelift(ty) {
                        CraneliftRepr::Scalar(_) => vec![self.get_operand_value(builder, arg)?],
                        CraneliftRepr::Aggregate(_) => self.operand_words(builder, arg, ty)?,
                    };
                    call_args[at.C()].copy_from_slice(&regs);
                }
            }
        }

        // Then a descriptor for each of the callee's type parameters, for what
        // this call bound it to.
        assert_eq!(bindings.len(), layout.descriptors.len(), "a binding per type parameter");
        for (at, binding) in layout.descriptors.C().zip(bindings) {
            call_args[at] = self.static_tydesc(builder, binding)?;
        }

        let results = self.emit_function_call(builder, callee_func_id, &call_args);
        self.return_if_abandoning(builder)?;
        if matches!(layout.ret, RetPass::Value(_)) {
            match types::ir_type_to_cranelift(dest_ty) {
                CraneliftRepr::Scalar(_) => {
                    self.values.insert(dest, results[0]);
                }
                CraneliftRepr::Aggregate(_) => self.define_words(builder, dest, results)?,
            }
        }

        self.mark_out_destinations_live(builder, code_ref, args);
        let modes = self.callee_param_modes(code_ref);
        self.mark_in_arguments_moved(builder, &modes, args)?;

        // For scalar sret returns, load the value from the sret location.
        if callee_uses_sret && has_result(dest_ty) {
            if let CraneliftRepr::Scalar(scalar_ty) = types::ir_type_to_cranelift(dest_ty) {
                let frame_slot = self.frame_slot.X();
                let dest_offset = self.layout.value_offset(dest.0);
                let ptr = frame_slot.addr(builder, dest_offset as i32);
                let loaded = builder.ins().load(scalar_ty, MemFlagsData::new(), ptr, 0);
                self.values.insert(dest, loaded);
            }
        }

        Ok(())
    }

    /// Where `args[i]`, passed by pointer, is.
    ///
    /// A copy value passed twice is the same operand twice, `f(v0, v0)`, and
    /// has one home; but each parameter is a place of its own, which the
    /// callee may own and hand on, and the runtime refuses two places that
    /// overlap where either is owned. So an argument naming the copy operand
    /// an earlier one did is passed a copy. A value that is not copied is
    /// passed twice only to be borrowed twice, `f(ref s, ref s)`, which one
    /// place serves.
    fn arg_ptr(&mut self, builder: &mut FunctionBuilder, args: &[Operand], i: usize) -> Result<cl_ir::Value, CraneliftError> {
        let ptr = self.get_operand_ptr(builder, &args[i])?;
        if !args[..i].contains(&args[i]) {
            return Ok(ptr);
        }
        let ty = self.get_operand_type(&args[i])?;
        if !ty.is_copy() {
            return Ok(ptr);
        }
        let size = types::ir_type_size(&ty);
        let slot = builder.create_sized_stack_slot(cl_ir::StackSlotData::new(
            cl_ir::StackSlotKind::ExplicitSlot,
            size,
            types::align_shift(size.max(1)),
        ));
        let copy = builder.ins().stack_addr(PTR_TYPE, slot, 0);
        self.copy_bytes(builder, copy, ptr, size as u64);
        Ok(copy)
    }

    /// Call a function compiled from IR, giving back what it returns in
    /// registers: through its code cell when it has one, see
    /// `set_code_cells`, and otherwise as it was declared.
    fn emit_function_call(&mut self, builder: &mut FunctionBuilder, callee: FuncId, args: &[cl_ir::Value]) -> Vec<cl_ir::Value> {
        let callee_ref = self.module.declare_func_in_func(callee, builder.func);
        let Some(&cell) = self.code_cells.get(&callee) else {
            let call = builder.ins().call(callee_ref, args);
            return builder.inst_results(call).to_vec();
        };
        let cell_addr = builder.ins().iconst(PTR_TYPE, cell as i64);
        let code = builder.ins().load(PTR_TYPE, MemFlagsData::trusted(), cell_addr, 0);
        let direct = builder.create_block();
        let via_stub = builder.create_block();
        let done = builder.create_block();
        let sig = builder.func.dfg.ext_funcs[callee_ref].signature;
        let ret_tys: Vec<cl_ir::Type> = builder.func.dfg.signatures[sig].returns.iter().map(|r| r.value_type).collect();
        for &t in &ret_tys {
            builder.append_block_param(done, t);
        }
        builder.ins().brif(code, direct, &[], via_stub, &[]);

        builder.switch_to_block(direct);
        builder.seal_block(direct);
        let call = builder.ins().call_indirect(sig, code, args);
        let results: Vec<cl_ir::BlockArg> = builder.inst_results(call).iter().map(|&v| v.into()).collect();
        builder.ins().jump(done, &results);

        builder.set_cold_block(via_stub);
        builder.switch_to_block(via_stub);
        builder.seal_block(via_stub);
        let call = builder.ins().call(callee_ref, args);
        let results: Vec<cl_ir::BlockArg> = builder.inst_results(call).iter().map(|&v| v.into()).collect();
        builder.ins().jump(done, &results);

        builder.switch_to_block(done);
        builder.seal_block(done);
        builder.block_params(done).to_vec()
    }

    /// The types `code_ref` declares its parameters as.
    fn callee_param_types(&self, code_ref: &CodeRef) -> Vec<IrType> {
        match code_ref {
            CodeRef::Local(_) | CodeRef::External { .. } => self.declared_callee(code_ref)
                .expect("resolve_code_ref rejects a callee never declared")
                .param_types
                .C(),
            CodeRef::Module { module, id } => self.registry
                .and_then(|r| r.get_module_function_as_unit(*module, CodeUnitId(id.0)))
                .and_then(|u| u.function_context())
                .expect("resolve_code_ref rejects a callee never declared")
                .param_types
                .C(),
        }
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
            self.return_abandoned(builder);
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
        self.return_abandoned(builder);
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

        for (i, arg) in args.iter().enumerate() {
            call_args.push(self.arg_ptr(builder, args, i)?);
            call_args.push(self.operand_tydesc(builder, arg)?);
        }

        // Result out-param: pointer to dest slot + tydesc.
        let dest_ty = self.func.value_types[dest.0 as usize].C();
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

        self.bind_written_dest(builder, dest, dest_ptr);
        Ok(())
    }

    /// Make `dest` the value a native wrote at `dest_ptr`: loaded, for a
    /// scalar, and for anything else the pointer.
    pub(super) fn bind_written_dest(&mut self, builder: &mut FunctionBuilder, dest: ValueId, dest_ptr: cl_ir::Value) {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        match types::ir_type_to_cranelift(dest_ty) {
            CraneliftRepr::Scalar(scalar_ty) => {
                let loaded = builder.ins().load(scalar_ty, MemFlagsData::new(), dest_ptr, 0);
                self.values.insert(dest, loaded);
            }
            CraneliftRepr::Aggregate(_) => {
                self.values.insert(dest, dest_ptr);
            }
        }
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
