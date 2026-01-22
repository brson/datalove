//! Function call instruction compilation.

use cranelift_codegen::ir::{types as cl_types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{FuncId, Module};

use datalove_datafun_ir::{FuncRef, Operand, ValueId};

use crate::types::PTR_TYPE;
use crate::CraneliftError;

use super::{uses_sret, FunctionCompiler};

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a Call instruction.
    ///
    /// Threads rt_handle as implicit first argument to callee.
    /// For aggregate returns, allocates space in caller's frame and passes sret pointer.
    pub(super) fn compile_call(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        func_ref: &FuncRef,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
        // Get rt_handle for threading.
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            CraneliftError::Codegen("Call requires runtime handle".into())
        })?;

        // Look up or declare the callee.
        let callee_func_id = self.resolve_func_ref(func_ref)?;

        // Check if the return type uses sret convention.
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let callee_uses_sret = uses_sret(dest_ty);

        // Build call arguments: [rt_handle, sret?, user_args...]
        let mut call_args = Vec::with_capacity(2 + args.len());
        call_args.push(rt_handle);

        // If sret, allocate space in caller's frame and pass pointer.
        let sret_ptr = if callee_uses_sret {
            // Get the destination's offset in our frame (already allocated by FrameLayout).
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

        // Add user arguments.
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
            // Sret: the return value is the pointer to our frame's space.
            self.values.insert(dest, ptr);
        } else {
            // Non-sret: get return value from call results.
            let results = builder.inst_results(call_inst);
            if !results.is_empty() {
                self.values.insert(dest, results[0]);
            } else {
                // Void return - use dummy value.
                let dummy = builder.ins().iconst(cl_types::I8, 0);
                self.values.insert(dest, dummy);
            }
        }

        Ok(())
    }

    /// Resolve a FuncRef to a Cranelift FuncId.
    pub(super) fn resolve_func_ref(&mut self, func_ref: &FuncRef) -> Result<FuncId, CraneliftError> {
        match func_ref {
            FuncRef::Local(ir_func_id) => {
                // Look up in local_funcs or declare.
                if let Some(&func_id) = self.local_funcs.get(ir_func_id) {
                    return Ok(func_id);
                }

                // For now, assume local functions aren't pre-declared.
                // This requires the callee to be compiled before the caller,
                // or a two-pass approach (declare all, then define all).
                Err(CraneliftError::Unsupported(format!(
                    "local function {:?} not yet declared - needs two-pass compilation",
                    ir_func_id
                )))
            }
            FuncRef::External { unit, func } => {
                Err(CraneliftError::Unsupported(format!(
                    "external function call (unit={}, func={:?}) not yet implemented",
                    unit, func
                )))
            }
            FuncRef::Module { module, func } => {
                // Look up in module_funcs (pre-declared in three-pass compilation).
                self.module_funcs.get(&(*module, *func)).copied().ok_or_else(|| {
                    CraneliftError::Unsupported(format!(
                        "module function ({:?}, {:?}) not pre-compiled",
                        module, func
                    ))
                })
            }
        }
    }
}
