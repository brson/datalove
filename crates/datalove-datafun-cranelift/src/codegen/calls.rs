//! Function call instruction compilation.

use cranelift_codegen::ir::{types as cl_types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use datalove_datafun_ir::{CodeRef, CodeUnitId, Operand, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::{uses_sret, FunctionCompiler};

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a Call instruction.
    ///
    /// Threads rt_handle as implicit first argument to callee.
    /// For native rider functions, marshals args from pointers to i64 scalars
    /// and converts the i64 return value back to the destination type.
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

        // Check if the return type uses sret convention.
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let callee_uses_sret = uses_sret(dest_ty);

        // Build call arguments: [rt_handle, sret?, user_args...]
        let mut call_args = Vec::with_capacity(2 + args.len());
        call_args.push(rt_handle);

        // If sret, allocate space in caller's frame and pass pointer.
        let sret_ptr = if callee_uses_sret {
            let frame_slot = self.frame_slot.ok_or_else(|| {
                CraneliftError::Codegen("no frame slot for sret return value".into())
            })?;
            let dest_offset = self.layout.value_offset(dest.0);
            let ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);
            call_args.push(ptr);
            Some(ptr)
        } else {
            None
        };

        // Add user arguments (passed by pointer).
        for arg in args {
            let arg_val = self.get_operand_ptr(builder, arg)?;
            call_args.push(arg_val);
        }

        // Declare callee in this function.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);

        // Emit call.
        let call_inst = builder.ins().call(callee_ref, &call_args);

        // Get return value.
        if let Some(ptr) = sret_ptr {
            self.values.insert(dest, ptr);
        } else {
            let results = builder.inst_results(call_inst);
            if !results.is_empty() {
                self.values.insert(dest, results[0]);
            } else {
                let dummy = builder.ins().iconst(cl_types::I8, 0);
                self.values.insert(dest, dummy);
            }
        }

        Ok(())
    }

    /// Compile a call to a native rider function.
    ///
    /// Native functions use C ABI: `fn(rt: *mut u8, arg0: i64, ...) -> i64`.
    /// This loads each arg from its pointer, widens to i64, calls the native
    /// function, then truncates the i64 result to the destination type.
    fn compile_native_call(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        code_ref: &CodeRef,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Call requires runtime handle".into())
        })?;

        let callee_func_id = self.resolve_code_ref(code_ref)?;

        // Build call arguments: [rt_handle, arg0_as_i64, arg1_as_i64, ...]
        let mut call_args = Vec::with_capacity(1 + args.len());
        call_args.push(rt_handle);

        for (i, arg) in args.iter().enumerate() {
            // Get the arg's IR type.
            let arg_ty = self.get_operand_type(arg)?;

            // Get a pointer to the arg value.
            let arg_ptr = self.get_operand_ptr(builder, arg)?;

            // Load the scalar value from the pointer.
            let scalar_ty = match types::ir_type_to_cranelift(&arg_ty) {
                CraneliftRepr::Scalar(ty) => ty,
                CraneliftRepr::Aggregate(_) => {
                    return Err(CraneliftError::Unsupported(format!(
                        "native call arg {} has aggregate type {:?}", i, arg_ty
                    )));
                }
            };
            let loaded = builder.ins().load(scalar_ty, MemFlags::new(), arg_ptr, 0);

            // Widen to i64.
            let as_i64 = if scalar_ty == cl_types::I64 {
                loaded
            } else if scalar_ty.is_int() {
                builder.ins().uextend(cl_types::I64, loaded)
            } else {
                return Err(CraneliftError::Unsupported(format!(
                    "native call arg {} has non-integer type {:?}", i, arg_ty
                )));
            };

            call_args.push(as_i64);
        }

        // Declare callee and emit call.
        let callee_ref = self.module.declare_func_in_func(callee_func_id, builder.func);
        let call_inst = builder.ins().call(callee_ref, &call_args);

        // Get the i64 return value.
        let results = builder.inst_results(call_inst);
        let ret_i64 = results[0];

        // Truncate to destination type.
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let dest_scalar = match types::ir_type_to_cranelift(dest_ty) {
            CraneliftRepr::Scalar(ty) => ty,
            CraneliftRepr::Aggregate(_) => {
                return Err(CraneliftError::Unsupported(format!(
                    "native call return has aggregate type {:?}", dest_ty
                )));
            }
        };

        let result = if dest_scalar == cl_types::I64 {
            ret_i64
        } else if dest_scalar.is_int() {
            builder.ins().ireduce(dest_scalar, ret_i64)
        } else {
            return Err(CraneliftError::Unsupported(format!(
                "native call return has non-integer type {:?}", dest_ty
            )));
        };

        self.values.insert(dest, result);

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
