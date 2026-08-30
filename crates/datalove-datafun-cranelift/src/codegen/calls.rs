//! Function call instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use datalove_datafun_ir::{CodeRef, CodeUnitId, IrType, Operand, ParamId, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::{uses_sret, FunctionCompiler};

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a Call instruction.
    ///
    /// Threads rt_handle as implicit first argument to callee.
    /// For native rider functions, marshals args from pointers to i64 scalars
    /// and converts the i64 return value back to the destination type.
    /// The parameters of `code_ref` whose descriptor the call site supplies.
    ///
    /// Empty for anything this compiler cannot see the body of, which is
    /// correct because a function only asks for these if it is generic, and a
    /// generic function is always compiled alongside its callers.
    fn callee_descriptor_params(&self, code_ref: &CodeRef) -> Vec<ParamId> {
        let CodeRef::Module { module, id } = code_ref else {
            return Vec::new();
        };
        let Some(registry) = self.registry else {
            return Vec::new();
        };
        registry.get_module_function_as_unit(*module, CodeUnitId(id.0))
            .and_then(|unit| unit.function_context().map(|c| c.descriptor_params.clone()))
            .unwrap_or_default()
    }

    /// The descriptor for an operand, as a runtime pointer.
    ///
    /// A parameter whose descriptor was supplied by our own caller uses that:
    /// its static type says `data` where a type parameter stood, so a
    /// descriptor built from that type would misdescribe the value. Everything
    /// else is described by its type, and the descriptor is a static symbol.
    fn operand_tydesc(
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
    fn static_tydesc(
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

        // Add user arguments (passed by pointer).
        for arg in args {
            let arg_val = self.get_operand_ptr(builder, arg)?;
            call_args.push(arg_val);
        }

        // Then a descriptor for each parameter whose own type does not describe
        // what it will receive. This is the place that knows: the argument here
        // has a concrete type, or a descriptor our own caller handed us.
        for param_id in self.callee_descriptor_params(code_ref) {
            let arg = args.get(param_id.0 as usize).ok_or_else(|| {
                CraneliftError::Codegen(format!(
                    "callee wants a descriptor for parameter {} but got {} arguments",
                    param_id.0, args.len(),
                ))
            })?.clone();
            let tydesc_addr = self.operand_tydesc(builder, &arg)?;
            call_args.push(tydesc_addr);
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

        for arg in args.iter() {
            let arg_ptr = self.get_operand_ptr(builder, arg)?;
            let tydesc_addr = self.operand_tydesc(builder, arg)?;

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
                if let Some(&func_id) = self.local_funcs.get(&ir_func_id) {
                    return Ok(func_id);
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
