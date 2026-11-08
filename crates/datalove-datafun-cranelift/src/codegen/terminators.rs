//! Terminator instruction compilation.

use rmx::prelude::*;
use cranelift_codegen::ir::{BlockArg, InstBuilder};
use cranelift_frontend::{FunctionBuilder, Switch};
use cranelift_module::Module;

use datalove_datafun_ir::{BlockId, Operand, Terminator};

use crate::types;
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// The Cranelift arguments of a jump to `target` passing `args`: the
    /// words of each to a parameter held as words, the value or address of
    /// each to any other.
    fn block_args(
        &mut self,
        builder: &mut FunctionBuilder,
        target: BlockId,
        args: &[Operand],
    ) -> Result<Vec<BlockArg>, CraneliftError> {
        let func = self.func;
        let params = &func.blocks.iter().find(|b| b.id == target).expect("a jump's target is a block").params;
        let mut cl_args = Vec::with_capacity(args.len());
        for (arg, &param) in args.iter().zip(params) {
            if self.block_param_words(param).is_some() {
                let ty = &func.value_types[param.0 as usize];
                cl_args.extend(self.operand_words(builder, arg, ty)?.into_iter().map(BlockArg::from));
            } else {
                cl_args.push(BlockArg::from(self.get_operand_value(builder, arg)?));
            }
        }
        Ok(cl_args)
    }

    /// Compile a terminator.
    pub(super) fn compile_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
    ) -> Result<(), CraneliftError> {
        match term {
            Terminator::Goto { target, args } => {
                let block = self.blocks[target];
                let cl_args = self.block_args(builder, *target, args)?;
                builder.ins().jump(block, &cl_args);
            }
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                let cond_val = self.get_operand_value(builder, cond)?;
                let then_blk = self.blocks[then_block];
                let else_blk = self.blocks[else_block];
                let cl_then_args = self.block_args(builder, *then_block, then_args)?;
                let cl_else_args = self.block_args(builder, *else_block, else_args)?;
                builder.ins().brif(cond_val, then_blk, &cl_then_args, else_blk, &cl_else_args);
            }
            Terminator::Return { value: Some(val_op) } if matches!(self.ret, crate::codegen::RetPass::Value(_)) => {
                let ret_ty = self.func_ctx.return_type.C();
                let regs = match types::ir_type_to_cranelift(&ret_ty) {
                    types::CraneliftRepr::Scalar(_) => vec![self.get_operand_value(builder, val_op)?],
                    types::CraneliftRepr::Aggregate(_) => self.operand_words(builder, val_op, &ret_ty)?,
                };
                self.leave_dynamic(builder);
                builder.ins().return_(&regs);
            }
            Terminator::Return { value } => {
                // Function return through `sret`.
                if let Some(val_op) = value {
                    let sret_ptr = self.sret_param.expect("non-Unit return requires sret param");
                    let ret_ty = &self.func_ctx.return_type;

                    match types::ir_type_to_cranelift(ret_ty) {
                        types::CraneliftRepr::Scalar(_) => {
                            // Scalar: store value directly to sret pointer.
                            let val = self.get_operand_value(builder, val_op)?;
                            builder.ins().store(cranelift_codegen::ir::MemFlagsData::new(), val, sret_ptr, 0);
                        }
                        types::CraneliftRepr::Aggregate(_) if self.words_of(val_op).is_some() => {
                            let words = self.words_of(val_op).expect("checked").C();
                            super::words::store_words_at(builder, sret_ptr, ret_ty, &words);
                        }
                        types::CraneliftRepr::Aggregate(_) => {
                            // Aggregate: memcpy from source pointer to sret
                            // pointer, unless it was built there.
                            let src_ptr = self.get_operand_value(builder, val_op)?;
                            if src_ptr != sret_ptr {
                                let ret_ty = ret_ty.C();
                                self.copy_value_bytes(builder, sret_ptr, src_ptr, &ret_ty)?;
                            }
                        }
                    }
                }
                self.leave_dynamic(builder);
                builder.ins().return_(&[]);
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
