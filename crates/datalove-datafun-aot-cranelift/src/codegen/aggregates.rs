//! Aggregate type instruction compilation (Pack, Unpack, Copy).

use cranelift_codegen::ir::{InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a copy instruction.
    pub(super) fn compile_copy(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), AotError> {
        let val = self.get_operand_value(builder, src)?;
        self.values.insert(dest, val);
        Ok(())
    }

    /// Compile a pack instruction (tuple/struct creation).
    pub(super) fn compile_pack(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        fields: &[Operand],
    ) -> Result<(), AotError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];
        let repr = types::ir_type_to_cranelift(dest_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple that fits in a register.
                if fields.len() == 1 {
                    let val = self.get_operand_value(builder, &fields[0])?;
                    self.values.insert(dest, val);
                } else {
                    return Err(AotError::Unsupported(
                        "scalar pack with multiple fields".into()
                    ));
                }
            }
            CraneliftRepr::Aggregate(_layout) => {
                // Allocate in frame and store fields.
                let frame_slot = self.frame_slot.ok_or_else(|| {
                    AotError::Codegen("no frame slot for aggregate".into())
                })?;

                let dest_offset = self.layout.value_offset(dest.0);
                let base = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

                // Get field offsets.
                let field_types: Vec<_> = match dest_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "pack for non-tuple/struct: {:?}",
                            dest_ty
                        )));
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, field_op) in fields.iter().enumerate() {
                    let field_val = self.get_operand_value(builder, field_op)?;
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(_) => {
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            builder.ins().store(MemFlags::new(), field_val, addr, 0);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // TODO: memcpy for aggregate fields.
                            return Err(AotError::Unsupported(
                                "aggregate field in pack".into()
                            ));
                        }
                    }
                }

                // For aggregates, we store a pointer to the frame location.
                self.values.insert(dest, base);
            }
        }

        Ok(())
    }

    /// Compile an unpack instruction (tuple/struct destructuring).
    pub(super) fn compile_unpack(
        &mut self,
        builder: &mut FunctionBuilder,
        dests: &[ValueId],
        src: &Operand,
    ) -> Result<(), AotError> {
        // Get source type from first dest's expected type.
        // Actually we need the source operand's type.
        let src_ty = self.get_operand_type(src)?;
        let repr = types::ir_type_to_cranelift(&src_ty);

        match repr {
            CraneliftRepr::Scalar(_) => {
                // Single-element tuple.
                if dests.len() == 1 {
                    let val = self.get_operand_value(builder, src)?;
                    self.values.insert(dests[0], val);
                } else {
                    return Err(AotError::Unsupported(
                        "scalar unpack with multiple dests".into()
                    ));
                }
            }
            CraneliftRepr::Aggregate(_) => {
                // Source is a pointer to aggregate; load fields.
                let base = self.get_operand_value(builder, src)?;

                let field_types: Vec<_> = match &src_ty {
                    IrType::Tuple(tys) => tys.clone(),
                    IrType::Struct(flds) => flds.iter().map(|(_, ty)| ty.clone()).collect(),
                    _ => {
                        return Err(AotError::Unsupported(format!(
                            "unpack from non-tuple/struct: {:?}",
                            src_ty
                        )));
                    }
                };

                let offsets = types::compute_tuple_field_offsets(&field_types);

                for (i, &dest) in dests.iter().enumerate() {
                    let field_ty = &field_types[i];
                    let field_repr = types::ir_type_to_cranelift(field_ty);

                    match field_repr {
                        CraneliftRepr::Scalar(field_cl_ty) => {
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            let val = builder.ins().load(field_cl_ty, MemFlags::new(), addr, 0);
                            self.values.insert(dest, val);
                        }
                        CraneliftRepr::Aggregate(_) => {
                            // Return pointer to field.
                            let addr = builder.ins().iadd_imm(base, offsets[i] as i64);
                            self.values.insert(dest, addr);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
