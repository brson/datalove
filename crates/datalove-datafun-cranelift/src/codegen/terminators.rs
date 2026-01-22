//! Terminator instruction compilation.

use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::Terminator;

use crate::types::{self, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a terminator.
    pub(super) fn compile_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
    ) -> Result<(), CraneliftError> {
        match term {
            Terminator::Goto { target, args } => {
                let block = self.blocks[target];
                // Collect block argument values.
                let cl_args: Vec<_> = args.iter()
                    .map(|op| self.get_operand_value(builder, op))
                    .collect::<Result<_, _>>()?;
                builder.ins().jump(block, &cl_args);
            }
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                let cond_val = self.get_operand_value(builder, cond)?;
                let then_blk = self.blocks[then_block];
                let else_blk = self.blocks[else_block];
                // Collect block argument values for each branch.
                let cl_then_args: Vec<_> = then_args.iter()
                    .map(|op| self.get_operand_value(builder, op))
                    .collect::<Result<_, _>>()?;
                let cl_else_args: Vec<_> = else_args.iter()
                    .map(|op| self.get_operand_value(builder, op))
                    .collect::<Result<_, _>>()?;
                builder.ins().brif(cond_val, then_blk, &cl_then_args, else_blk, &cl_else_args);
            }
            Terminator::Return { value } => {
                if let Some(val_op) = value {
                    // Check if we're using sret convention.
                    if let Some(sret_ptr) = self.sret_param {
                        // Aggregate return: copy value to sret location.
                        // The return value is a pointer to stack-allocated data.
                        let src_ptr = self.get_operand_value(builder, val_op)?;

                        // Get the size of the return type.
                        let ret_ty = &self.func.return_type;
                        let size = types::ir_type_size(ret_ty);

                        // Use memcpy to copy the data to sret location.
                        let size_val = builder.ins().iconst(PTR_TYPE, size as i64);
                        builder.call_memcpy(self.isa.frontend_config(), sret_ptr, src_ptr, size_val);

                        // Return void.
                        builder.ins().return_(&[]);
                    } else {
                        // Scalar return: return value in register.
                        let val = self.get_operand_value(builder, val_op)?;
                        builder.ins().return_(&[val]);
                    }
                } else {
                    builder.ins().return_(&[]);
                }
            }
            Terminator::UnitEnd { result } => {
                // Normal script unit completion.
                // AOT uses scriptunit-fragment with explicit debuglog, so result is always None.
                debug_assert!(result.is_none(), "UnitEnd with result not expected in AOT");
                builder.ins().return_(&[]);
            }
            Terminator::UnitEarlyReturn { value } => {
                // Early return from script (via ret, !, or checked operators).
                // Output the value via debuglog, destroy it, then return.
                self.compile_debuglog(builder, value)?;
                self.compile_drop(builder, value)?;
                builder.ins().return_(&[]);
            }
        }
        Ok(())
    }
}
