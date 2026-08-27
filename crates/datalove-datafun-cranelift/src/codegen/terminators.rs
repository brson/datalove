//! Terminator instruction compilation.

use cranelift_codegen::ir::{BlockArg, InstBuilder};
use cranelift_frontend::{FunctionBuilder, Switch};
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
                let cl_args: Vec<BlockArg> = args.iter()
                    .map(|op| self.get_operand_value(builder, op).map(BlockArg::from))
                    .collect::<Result<_, _>>()?;
                builder.ins().jump(block, &cl_args);
            }
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                let cond_val = self.get_operand_value(builder, cond)?;
                let then_blk = self.blocks[then_block];
                let else_blk = self.blocks[else_block];
                // Collect block argument values for each branch.
                let cl_then_args: Vec<BlockArg> = then_args.iter()
                    .map(|op| self.get_operand_value(builder, op).map(BlockArg::from))
                    .collect::<Result<_, _>>()?;
                let cl_else_args: Vec<BlockArg> = else_args.iter()
                    .map(|op| self.get_operand_value(builder, op).map(BlockArg::from))
                    .collect::<Result<_, _>>()?;
                builder.ins().brif(cond_val, then_blk, &cl_then_args, else_blk, &cl_else_args);
            }
            Terminator::Return { value } => {
                // Function return. All non-Unit returns use sret convention.
                if let Some(val_op) = value {
                    let sret_ptr = self.sret_param.expect("non-Unit return requires sret param");
                    let ret_ty = &self.func_ctx.return_type;

                    match types::ir_type_to_cranelift(ret_ty) {
                        types::CraneliftRepr::Scalar(_) => {
                            // Scalar: store value directly to sret pointer.
                            let val = self.get_operand_value(builder, val_op)?;
                            builder.ins().store(cranelift_codegen::ir::MemFlagsData::new(), val, sret_ptr, 0);
                        }
                        types::CraneliftRepr::Aggregate(_) => {
                            // Aggregate: memcpy from source pointer to sret pointer.
                            let src_ptr = self.get_operand_value(builder, val_op)?;
                            let size = types::ir_type_size(ret_ty);
                            let size_val = builder.ins().iconst(PTR_TYPE, size as i64);
                            builder.call_memcpy(self.isa.frontend_config(), sret_ptr, src_ptr, size_val);
                        }
                    }
                    builder.ins().return_(&[]);
                } else {
                    builder.ins().return_(&[]);
                }
            }
            Terminator::UnitEnd { result } => {
                // Script unit end.
                // AOT uses scriptunit-fragment with explicit debuglog, so result is always None.
                debug_assert!(result.is_none(), "UnitEnd with result not expected in AOT script");
                builder.ins().return_(&[]);
            }
            Terminator::UnitEarlyReturn { value } => {
                // Early exit from script (via ret, !, or checked operators).
                // Output the value via debuglog, destroy it, then return.
                self.compile_debuglog(builder, value)?;
                self.compile_drop(builder, value)?;
                builder.ins().return_(&[]);
            }
            Terminator::Switch { discriminant, cases, default } => {
                let disc_val = self.get_operand_value(builder, discriminant)?;
                let default_block = self.blocks[default];
                let mut switch = Switch::new();
                for (val, target) in cases {
                    switch.set_entry(*val as u128, self.blocks[target]);
                }
                switch.emit(builder, disc_val, default_block);
            }
        }
        Ok(())
    }
}
