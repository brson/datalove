//! Terminator instruction compilation.

use cranelift_codegen::ir::InstBuilder;
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::Terminator;

use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a terminator.
    pub(super) fn compile_terminator(
        &mut self,
        builder: &mut FunctionBuilder,
        term: &Terminator,
    ) -> Result<(), AotError> {
        match term {
            Terminator::Goto(target) => {
                let block = self.blocks[target];
                builder.ins().jump(block, &[]);
            }
            Terminator::Branch { cond, then_block, else_block } => {
                let cond_val = self.get_operand_value(builder, cond)?;
                let then_blk = self.blocks[then_block];
                let else_blk = self.blocks[else_block];
                builder.ins().brif(cond_val, then_blk, &[], else_blk, &[]);
            }
            Terminator::Return { value } => {
                if let Some(val_op) = value {
                    let val = self.get_operand_value(builder, val_op)?;
                    builder.ins().return_(&[val]);
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
                // Output the value via debuglog, then return.
                self.compile_debuglog(builder, value)?;
                builder.ins().return_(&[]);
            }
        }
        Ok(())
    }
}
