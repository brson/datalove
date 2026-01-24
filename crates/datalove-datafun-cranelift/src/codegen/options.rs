//! Option and Result type instruction compilation.

use cranelift_codegen::ir::{types as cl_types, InstBuilder, MemFlags};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};
use datalove_rtdt::{Error as RtError, OptionTag, ResultTag};

use crate::types::{self, CraneliftRepr, PTR_TYPE};
use crate::CraneliftError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a WrapSome instruction: dest = Some(inner).
    pub(super) fn compile_wrap_some(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &Operand,
    ) -> Result<(), CraneliftError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Get inner type from Option<inner>.
        let inner_ty = match dest_ty {
            IrType::Option(t) => t.as_ref(),
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "WrapSome dest is not Option: {:?}",
                    dest_ty
                )));
            }
        };

        // Compute Option layout.
        let inner_layout = types::ir_type_to_cranelift(inner_ty).layout();
        let tag_size = 1u32;
        let payload_offset = types::align_up(tag_size, inner_layout.align);

        // Get destination address in frame.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for WrapSome".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write tag (Some = 2).
        let tag_val = builder.ins().iconst(cl_types::I8, OptionTag::Some as i64);
        builder.ins().store(MemFlags::new(), tag_val, dest_addr, 0);

        // Get inner value and copy to payload location.
        let payload_addr = builder.ins().iadd_imm(dest_addr, payload_offset as i64);
        let inner_repr = types::ir_type_to_cranelift(inner_ty);

        match inner_repr {
            CraneliftRepr::Scalar(_) => {
                let inner_val = self.get_operand_value(builder, inner)?;
                builder.ins().store(MemFlags::new(), inner_val, payload_addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let inner_ptr = self.get_operand_ptr(builder, inner)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), payload_addr, inner_ptr, size);
            }
        }

        // Store pointer to Option in values map.
        self.values.insert(dest, dest_addr);
        Ok(())
    }

    /// Compile a WrapNone instruction: dest = None.
    pub(super) fn compile_wrap_none(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
    ) -> Result<(), CraneliftError> {
        // Get destination address in frame.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for WrapNone".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write tag (None = 1).
        let tag_val = builder.ins().iconst(cl_types::I8, OptionTag::None as i64);
        builder.ins().store(MemFlags::new(), tag_val, dest_addr, 0);

        // Store pointer to Option in values map.
        self.values.insert(dest, dest_addr);
        Ok(())
    }

    /// Compile a WrapOk instruction: dest = Ok(inner).
    pub(super) fn compile_wrap_ok(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &Operand,
    ) -> Result<(), CraneliftError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Get ok type from Result<ok>.
        let ok_ty = match dest_ty {
            IrType::Result(t) => t.as_ref(),
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "WrapOk dest is not Result: {:?}",
                    dest_ty
                )));
            }
        };

        // Compute Result layout.
        let ok_layout = types::ir_type_to_cranelift(ok_ty).layout();
        let error_align = std::mem::align_of::<RtError>() as u32;
        let tag_size = 1u32;
        let max_payload_align = ok_layout.align.max(error_align);
        let payload_offset = types::align_up(tag_size, max_payload_align);

        // Get destination address in frame.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for WrapOk".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write tag (Ok = 1).
        let tag_val = builder.ins().iconst(cl_types::I8, ResultTag::Ok as i64);
        builder.ins().store(MemFlags::new(), tag_val, dest_addr, 0);

        // Get inner value and copy to payload location.
        let payload_addr = builder.ins().iadd_imm(dest_addr, payload_offset as i64);
        let ok_repr = types::ir_type_to_cranelift(ok_ty);

        match ok_repr {
            CraneliftRepr::Scalar(_) => {
                let inner_val = self.get_operand_value(builder, inner)?;
                builder.ins().store(MemFlags::new(), inner_val, payload_addr, 0);
            }
            CraneliftRepr::Aggregate(layout) => {
                let inner_ptr = self.get_operand_ptr(builder, inner)?;
                let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                builder.call_memcpy(self.isa.frontend_config(), payload_addr, inner_ptr, size);
            }
        }

        // Store pointer to Result in values map.
        self.values.insert(dest, dest_addr);
        Ok(())
    }

    /// Compile a WrapErr instruction: dest = Err(error).
    pub(super) fn compile_wrap_err(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        inner: &Operand,
    ) -> Result<(), CraneliftError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Get ok type from Result<ok> to compute layout.
        let ok_ty = match dest_ty {
            IrType::Result(t) => t.as_ref(),
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "WrapErr dest is not Result: {:?}",
                    dest_ty
                )));
            }
        };

        // Compute Result layout.
        let ok_layout = types::ir_type_to_cranelift(ok_ty).layout();
        let error_size = std::mem::size_of::<RtError>() as u32;
        let error_align = std::mem::align_of::<RtError>() as u32;
        let tag_size = 1u32;
        let max_payload_align = ok_layout.align.max(error_align);
        let payload_offset = types::align_up(tag_size, max_payload_align);

        // Get destination address in frame.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for WrapErr".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write tag (Err = 2).
        let tag_val = builder.ins().iconst(cl_types::I8, ResultTag::Err as i64);
        builder.ins().store(MemFlags::new(), tag_val, dest_addr, 0);

        // Copy Error value to payload location.
        let payload_addr = builder.ins().iadd_imm(dest_addr, payload_offset as i64);
        let inner_ptr = self.get_operand_ptr(builder, inner)?;
        let size = builder.ins().iconst(PTR_TYPE, error_size as i64);
        builder.call_memcpy(self.isa.frontend_config(), payload_addr, inner_ptr, size);

        // Store pointer to Result in values map.
        self.values.insert(dest, dest_addr);
        Ok(())
    }

    /// Compile an UnwrapOption instruction: (dest, is_some) = unwrap(src).
    pub(super) fn compile_unwrap_option(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        is_some: ValueId,
        src: &Operand,
    ) -> Result<(), CraneliftError> {
        use crate::layout::tracking;

        // Get source Option type.
        let src_ty = self.get_operand_type(src)?;
        let inner_ty = match &src_ty {
            IrType::Option(t) => t.as_ref(),
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "UnwrapOption src is not Option: {:?}",
                    src_ty
                )));
            }
        };

        // Compute Option layout.
        let inner_layout = types::ir_type_to_cranelift(inner_ty).layout();
        let tag_size = 1u32;
        let payload_offset = types::align_up(tag_size, inner_layout.align);

        // Get source address.
        let src_addr = self.get_operand_ptr(builder, src)?;

        // Load tag.
        let tag = builder.ins().load(cl_types::I8, MemFlags::new(), src_addr, 0);

        // is_some = (tag == Some = 2).
        let some_tag = builder.ins().iconst(cl_types::I8, OptionTag::Some as i64);
        let is_some_val = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            tag,
            some_tag,
        );
        self.values.insert(is_some, is_some_val);

        // Compute payload address.
        let payload_addr = builder.ins().iadd_imm(src_addr, payload_offset as i64);

        // For dest, either load the value (scalar) or store the pointer (aggregate).
        let inner_repr = types::ir_type_to_cranelift(inner_ty);
        match inner_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Load payload value. Value is undefined if None, but that's ok
                // since caller should check is_some first.
                let val = builder.ins().load(cl_ty, MemFlags::new(), payload_addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                // Store pointer to payload.
                self.values.insert(dest, payload_addr);
            }
        }

        // Conditionally mark dest as LIVE based on is_some.
        // Only valid if it was Some.
        if let Some(track_offset) = self.layout.values[dest.0 as usize].tracking_byte {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let frame_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, 0);

            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);

            let track_addr = builder.ins().iadd_imm(frame_addr, track_offset as i64);
            let track_val = builder.ins().select(is_some_val, live_val, uninit_val);
            builder.ins().store(MemFlags::new(), track_val, track_addr, 0);
        }

        Ok(())
    }

    /// Compile an EnumVariant instruction: dest = Variant(payload).
    pub(super) fn compile_enum_variant(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        variant_index: u32,
        payload: Option<&Operand>,
    ) -> Result<(), CraneliftError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Get variant info from Enum type.
        let variants = match dest_ty {
            IrType::Enum(v) => v,
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "EnumVariant dest is not Enum: {:?}",
                    dest_ty
                )));
            }
        };

        // Compute payload offset for this variant.
        let payload_offset = if let Some(payload_ty) = &variants[variant_index as usize].1 {
            let payload_layout = types::ir_type_to_cranelift(payload_ty).layout();
            types::align_up(4, payload_layout.align) // 4 = discriminant size
        } else {
            0
        };

        // Get destination address in frame.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            CraneliftError::Codegen("no frame slot for EnumVariant".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let dest_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Write discriminant (variant_index as u32) at offset 0.
        let disc_val = builder.ins().iconst(cl_types::I32, variant_index as i64);
        builder.ins().store(MemFlags::new(), disc_val, dest_addr, 0);

        // Copy payload if present.
        if let Some(payload_op) = payload {
            let payload_ty = &variants[variant_index as usize].1.as_ref().ok_or_else(|| {
                CraneliftError::Codegen("EnumVariant has payload but variant has no payload type".into())
            })?;
            let payload_repr = types::ir_type_to_cranelift(payload_ty);
            let payload_addr = builder.ins().iadd_imm(dest_addr, payload_offset as i64);

            match payload_repr {
                CraneliftRepr::Scalar(_) => {
                    let val = self.get_operand_value(builder, payload_op)?;
                    builder.ins().store(MemFlags::new(), val, payload_addr, 0);
                }
                CraneliftRepr::Aggregate(layout) => {
                    let src_ptr = self.get_operand_ptr(builder, payload_op)?;
                    let size = builder.ins().iconst(PTR_TYPE, layout.size as i64);
                    builder.call_memcpy(self.isa.frontend_config(), payload_addr, src_ptr, size);
                }
            }
        }

        // Store pointer to Enum in values map.
        self.values.insert(dest, dest_addr);
        Ok(())
    }

    /// Compile an UnwrapResult instruction: (ok_dest, err_dest, is_ok) = unwrap(src).
    pub(super) fn compile_unwrap_result(
        &mut self,
        builder: &mut FunctionBuilder,
        ok_dest: ValueId,
        err_dest: ValueId,
        is_ok: ValueId,
        src: &Operand,
    ) -> Result<(), CraneliftError> {
        use crate::layout::tracking;

        // Get source Result type.
        let src_ty = self.get_operand_type(src)?;
        let ok_ty = match &src_ty {
            IrType::Result(t) => t.as_ref(),
            _ => {
                return Err(CraneliftError::Codegen(format!(
                    "UnwrapResult src is not Result: {:?}",
                    src_ty
                )));
            }
        };

        // Compute Result layout.
        let ok_layout = types::ir_type_to_cranelift(ok_ty).layout();
        let error_align = std::mem::align_of::<RtError>() as u32;
        let tag_size = 1u32;
        let max_payload_align = ok_layout.align.max(error_align);
        let payload_offset = types::align_up(tag_size, max_payload_align);

        // Get source address.
        let src_addr = self.get_operand_ptr(builder, src)?;

        // Load tag.
        let tag = builder.ins().load(cl_types::I8, MemFlags::new(), src_addr, 0);

        // is_ok = (tag == Ok = 1).
        let ok_tag = builder.ins().iconst(cl_types::I8, ResultTag::Ok as i64);
        let is_ok_val = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            tag,
            ok_tag,
        );
        self.values.insert(is_ok, is_ok_val);

        // Compute payload address.
        let payload_addr = builder.ins().iadd_imm(src_addr, payload_offset as i64);

        // For ok_dest, either load value (scalar) or store pointer (aggregate).
        let ok_repr = types::ir_type_to_cranelift(ok_ty);
        match ok_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                let val = builder.ins().load(cl_ty, MemFlags::new(), payload_addr, 0);
                self.values.insert(ok_dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                self.values.insert(ok_dest, payload_addr);
            }
        }

        // For err_dest, always store pointer to payload (Error is aggregate).
        self.values.insert(err_dest, payload_addr);

        // Conditionally mark ok_dest or err_dest as LIVE based on is_ok.
        // Only one of them is valid at runtime.
        let ok_track_offset = self.layout.values[ok_dest.0 as usize].tracking_byte;
        let err_track_offset = self.layout.values[err_dest.0 as usize].tracking_byte;

        if ok_track_offset.is_some() || err_track_offset.is_some() {
            let frame_slot = self.frame_slot
                .expect("tracking requires frame slot");
            let frame_addr = builder.ins().stack_addr(PTR_TYPE, frame_slot, 0);

            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);

            if let Some(ok_offset) = ok_track_offset {
                // ok tracking byte = is_ok ? LIVE : UNINIT
                let ok_track_addr = builder.ins().iadd_imm(frame_addr, ok_offset as i64);
                let ok_track_val = builder.ins().select(is_ok_val, live_val, uninit_val);
                builder.ins().store(MemFlags::new(), ok_track_val, ok_track_addr, 0);
            }

            if let Some(err_offset) = err_track_offset {
                // err tracking byte = is_ok ? UNINIT : LIVE
                let err_track_addr = builder.ins().iadd_imm(frame_addr, err_offset as i64);
                let err_track_val = builder.ins().select(is_ok_val, uninit_val, live_val);
                builder.ins().store(MemFlags::new(), err_track_val, err_track_addr, 0);
            }
        }

        Ok(())
    }
}
