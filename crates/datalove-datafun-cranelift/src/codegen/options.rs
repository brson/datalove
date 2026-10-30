//! Option and Result type instruction compilation.

use cranelift_codegen::ir::{types as cl_types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{IrType, Operand, ValueId};
use datalove_rtdt::{Error as RtError, OptionTag, ResultTag};

use crate::types::{self, CraneliftRepr};
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

        let (dest_ty, inner_ty) = (dest_ty.clone(), inner_ty.clone());
        let dest_addr = self.wrap_dest(builder, dest)?;

        // Write tag (Some = 2).
        let tag_val = builder.ins().iconst(cl_types::I8, OptionTag::Some as i64);
        builder.ins().store(MemFlagsData::new(), tag_val, dest_addr, 0);

        // Get inner value and copy to payload location.
        let payload_addr = self.payload_addr(builder, dest_addr, &dest_ty, 0)?;
        self.put_value(builder, payload_addr, &inner_ty, inner)?;

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
        let dest_addr = self.wrap_dest(builder, dest)?;

        // Write tag (None = 1).
        let tag_val = builder.ins().iconst(cl_types::I8, OptionTag::None as i64);
        builder.ins().store(MemFlagsData::new(), tag_val, dest_addr, 0);

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

        let (dest_ty, ok_ty) = (dest_ty.clone(), ok_ty.clone());
        let dest_addr = self.wrap_dest(builder, dest)?;

        // Write tag (Ok = 1).
        let tag_val = builder.ins().iconst(cl_types::I8, ResultTag::Ok as i64);
        builder.ins().store(MemFlagsData::new(), tag_val, dest_addr, 0);

        // Get inner value and copy to payload location.
        let payload_addr = self.payload_addr(builder, dest_addr, &dest_ty, 0)?;
        self.put_value(builder, payload_addr, &ok_ty, inner)?;

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

        if !matches!(dest_ty, IrType::Result(_)) {
            return Err(CraneliftError::Codegen(format!(
                "WrapErr dest is not Result: {:?}",
                dest_ty
            )));
        }
        let error_size = std::mem::size_of::<RtError>() as u32;
        let dest_ty = dest_ty.clone();
        let dest_addr = self.wrap_dest(builder, dest)?;

        // Write tag (Err = 2).
        let tag_val = builder.ins().iconst(cl_types::I8, ResultTag::Err as i64);
        builder.ins().store(MemFlagsData::new(), tag_val, dest_addr, 0);

        // Copy Error value to payload location.
        let payload_addr = self.payload_addr(builder, dest_addr, &dest_ty, 0)?;
        let inner_ptr = self.get_operand_ptr(builder, inner)?;
        self.copy_bytes(builder, payload_addr, inner_ptr, error_size as u64);

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
        use datalove_datafun_ir::frame_layout::tracking;

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

        let inner_ty = inner_ty.clone();

        // Get source address.
        let src_addr = self.get_operand_ptr(builder, src)?;

        // Load tag.
        let tag = builder.ins().load(cl_types::I8, MemFlagsData::new(), src_addr, 0);

        // is_some = (tag == Some = 2).
        let some_tag = builder.ins().iconst(cl_types::I8, OptionTag::Some as i64);
        let is_some_val = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            tag,
            some_tag,
        );
        self.values.insert(is_some, is_some_val);

        // Compute payload address.
        let payload_addr = self.payload_addr(builder, src_addr, &src_ty, 0)?;

        // For dest, either load the value (scalar) or store the pointer (aggregate).
        let inner_repr = types::ir_type_to_cranelift(&inner_ty);
        match inner_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                // Load payload value. Value is undefined if None, but that's ok
                // since caller should check is_some first.
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), payload_addr, 0);
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
            let frame_addr = frame_slot.addr(builder, 0);

            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);

            let track_addr = builder.ins().iadd_imm_s(frame_addr, track_offset as i64);
            let track_val = builder.ins().select(is_some_val, live_val, uninit_val);
            builder.ins().store(MemFlagsData::new(), track_val, track_addr, 0);
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

        // Get variant info from enum-like type.
        let variants = dest_ty.enum_variants().ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "EnumVariant dest is not enum-like: {:?}",
                dest_ty
            ))
        })?;

        let payload_ty = variants[variant_index as usize].1.clone();
        let dest_ty = dest_ty.clone();
        let dest_addr = self.dest_addr(builder, dest)?;

        // Write discriminant (variant_index as u32) at offset 0.
        let disc_val = builder.ins().iconst(cl_types::I32, variant_index as i64);
        builder.ins().store(MemFlagsData::new(), disc_val, dest_addr, 0);

        // Copy payload if present.
        if let Some(payload_op) = payload {
            let payload_ty = payload_ty.ok_or_else(|| {
                CraneliftError::Codegen("EnumVariant has payload but variant has no payload type".into())
            })?;
            let payload_addr = self.payload_addr(builder, dest_addr, &dest_ty, variant_index)?;
            self.put_value(builder, payload_addr, &payload_ty, payload_op)?;
        }

        // Store pointer to Enum in values map.
        self.values.insert(dest, dest_addr);
        Ok(())
    }

    /// Compile an EnumDiscriminant instruction: dest = discriminant(src).
    ///
    /// Reads the u32 tag from the enum value without consuming it.
    pub(super) fn compile_enum_discriminant(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
    ) -> Result<(), CraneliftError> {
        // Get source address (enum is always aggregate).
        let src_addr = self.get_operand_ptr(builder, src)?;

        // Load discriminant (u32 at offset 0).
        let disc = builder.ins().load(cl_types::I32, MemFlagsData::new(), src_addr, 0);
        self.values.insert(dest, disc);
        Ok(())
    }

    /// Compile an EnumPayload instruction: dest = payload(src, variant_index).
    ///
    /// Extracts the payload from the enum value, consuming it.
    pub(super) fn compile_enum_payload(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        src: &Operand,
        variant_index: u32,
    ) -> Result<(), CraneliftError> {
        let src_ty = self.get_operand_type(src)?;
        let variants = src_ty.enum_variants().ok_or_else(|| {
            CraneliftError::Codegen(format!(
                "EnumPayload src is not enum-like: {:?}", src_ty
            ))
        })?;

        let payload_ty = variants[variant_index as usize].1.clone()
            .ok_or_else(|| CraneliftError::Codegen("EnumPayload variant has no payload type".into()))?;

        // Get source address.
        let src_addr = self.get_operand_ptr(builder, src)?;
        let payload_addr = self.payload_addr(builder, src_addr, &src_ty, variant_index)?;

        // Copy payload to dest.
        let payload_repr = types::ir_type_to_cranelift(&payload_ty);
        match payload_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), payload_addr, 0);
                self.values.insert(dest, val);
            }
            CraneliftRepr::Aggregate(_) => {
                let dest_addr = self.dest_addr(builder, dest)?;
                self.copy_value_bytes(builder, dest_addr, payload_addr, &payload_ty)?;
                self.values.insert(dest, dest_addr);
            }
        }
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
        use datalove_datafun_ir::frame_layout::tracking;

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

        let ok_ty = ok_ty.clone();

        // Get source address.
        let src_addr = self.get_operand_ptr(builder, src)?;

        // Load tag.
        let tag = builder.ins().load(cl_types::I8, MemFlagsData::new(), src_addr, 0);

        // is_ok = (tag == Ok = 1).
        let ok_tag = builder.ins().iconst(cl_types::I8, ResultTag::Ok as i64);
        let is_ok_val = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            tag,
            ok_tag,
        );
        self.values.insert(is_ok, is_ok_val);

        // Compute payload address.
        let payload_addr = self.payload_addr(builder, src_addr, &src_ty, 0)?;

        // For ok_dest, either load value (scalar) or store pointer (aggregate).
        let ok_repr = types::ir_type_to_cranelift(&ok_ty);
        match ok_repr {
            CraneliftRepr::Scalar(cl_ty) => {
                let val = builder.ins().load(cl_ty, MemFlagsData::new(), payload_addr, 0);
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
            let frame_addr = frame_slot.addr(builder, 0);

            let live_val = builder.ins().iconst(cl_types::I8, tracking::LIVE as i64);
            let uninit_val = builder.ins().iconst(cl_types::I8, tracking::UNINIT as i64);

            if let Some(ok_offset) = ok_track_offset {
                // ok tracking byte = is_ok ? LIVE : UNINIT
                let ok_track_addr = builder.ins().iadd_imm_s(frame_addr, ok_offset as i64);
                let ok_track_val = builder.ins().select(is_ok_val, live_val, uninit_val);
                builder.ins().store(MemFlagsData::new(), ok_track_val, ok_track_addr, 0);
            }

            if let Some(err_offset) = err_track_offset {
                // err tracking byte = is_ok ? UNINIT : LIVE
                let err_track_addr = builder.ins().iadd_imm_s(frame_addr, err_offset as i64);
                let err_track_val = builder.ins().select(is_ok_val, uninit_val, live_val);
                builder.ins().store(MemFlagsData::new(), err_track_val, err_track_addr, 0);
            }
        }

        Ok(())
    }
}
