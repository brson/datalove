//! Substituting parameters in instructions and terminators.
//!
//! Two transformations need this: inlining, which replaces a callee's
//! parameters with the caller's argument operands, and const parameter
//! monomorphization, which replaces a comptime parameter with the constant it
//! was called with and renumbers the parameters that remain.
//!
//! The match over instructions is exhaustive on purpose. A variant that falls
//! through unsubstituted leaves a reference to a parameter the transformed
//! function may no longer have, and nothing downstream reports that.

use std::collections::HashMap;

use crate::{Instruction, Operand, ParamId, Terminator};

/// Replace Param operands in an instruction with the corresponding replacement operands.
pub fn replace_params_in_instruction(
    instr: &Instruction,
    replacements: &HashMap<ParamId, Operand>,
) -> Instruction {
    let replace_operand = |op: &Operand| -> Operand {
        if let Operand::Param(p) = op {
            if let Some(replacement) = replacements.get(p) {
                return replacement.clone();
            }
        }
        op.clone()
    };

    match instr {
        Instruction::Copy { dest, src } => Instruction::Copy {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Move { dest, src } => Instruction::Move {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::BinOp { dest, op, lhs, rhs } => Instruction::BinOp {
            dest: *dest,
            op: *op,
            lhs: replace_operand(lhs),
            rhs: replace_operand(rhs),
        },
        Instruction::UnaryOp { dest, op, operand } => Instruction::UnaryOp {
            dest: *dest,
            op: *op,
            operand: replace_operand(operand),
        },
        Instruction::BinOpChecked {
            dest,
            overflow,
            op,
            lhs,
            rhs,
        } => Instruction::BinOpChecked {
            dest: *dest,
            overflow: *overflow,
            op: *op,
            lhs: replace_operand(lhs),
            rhs: replace_operand(rhs),
        },
        Instruction::UnaryOpChecked {
            dest,
            overflow,
            op,
            operand,
        } => Instruction::UnaryOpChecked {
            dest: *dest,
            overflow: *overflow,
            op: *op,
            operand: replace_operand(operand),
        },
        Instruction::Widen { dest, src } => Instruction::Widen {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::WidenFixed { dest, src } => Instruction::WidenFixed {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Clone { dest, src } => Instruction::Clone {
            dest: *dest,
            src: replace_operand(src),
        },
        // The descriptors say what the callee's type parameters were bound to,
        // which no operand substitution changes. Blanking them would drop what
        // a generic callee needs wherever nothing recomputes them afterwards.
        Instruction::Call { site_id, dest, func, args, type_args, shape_descriptors } =>
            Instruction::Call {
                site_id: *site_id,
                dest: *dest,
                func: func.clone(),
                args: args.iter().map(replace_operand).collect(),
                type_args: type_args.clone(),
                shape_descriptors: shape_descriptors.clone(),
            },
        Instruction::ComptimeCall { dest, func, args, discriminant, comptime_param_indices } => Instruction::ComptimeCall {
            dest: *dest,
            func: func.clone(),
            args: args.iter().map(replace_operand).collect(),
            discriminant: *discriminant,
            comptime_param_indices: comptime_param_indices.clone(),
        },
        Instruction::Pack { dest, ty, fields } => Instruction::Pack {
            dest: *dest,
            ty: ty.clone(),
            fields: fields.iter().map(replace_operand).collect(),
        },
        Instruction::Unpack { dests, src } => Instruction::Unpack {
            dests: dests.clone(),
            src: replace_operand(src),
        },
        Instruction::GetField {
            dest,
            src,
            field_index,
        } => Instruction::GetField {
            dest: *dest,
            src: replace_operand(src),
            field_index: *field_index,
        },
        Instruction::GetFieldRef {
            dest,
            src,
            field_index,
        } => Instruction::GetFieldRef {
            dest: *dest,
            src: replace_operand(src),
            field_index: *field_index,
        },
        Instruction::DataBorrow { dest, src } => Instruction::DataBorrow {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::WrapSome { dest, inner } => Instruction::WrapSome {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::WrapOk { dest, inner } => Instruction::WrapOk {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::WrapErr { dest, inner } => Instruction::WrapErr {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::EnumVariant {
            dest,
            variant_index,
            payload,
        } => Instruction::EnumVariant {
            dest: *dest,
            variant_index: *variant_index,
            payload: payload.as_ref().map(replace_operand),
        },
        Instruction::EnumDiscriminant { dest, src } => Instruction::EnumDiscriminant {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::EnumPayload { dest, src, variant_index } => Instruction::EnumPayload {
            dest: *dest,
            src: replace_operand(src),
            variant_index: *variant_index,
        },
        Instruction::UnwrapOption { dest, is_some, src } => Instruction::UnwrapOption {
            dest: *dest,
            is_some: *is_some,
            src: replace_operand(src),
        },
        Instruction::UnwrapResult {
            ok_dest,
            err_dest,
            is_ok,
            src,
        } => Instruction::UnwrapResult {
            ok_dest: *ok_dest,
            err_dest: *err_dest,
            is_ok: *is_ok,
            src: replace_operand(src),
        },
        Instruction::ErrorFrom { dest, inner } => Instruction::ErrorFrom {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::DataFrom { dest, inner } => Instruction::DataFrom {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::Erase { dest, src } => Instruction::Erase {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::EraseTracked { dest, src } => Instruction::EraseTracked {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Reify { dest, src } => Instruction::Reify {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::ListNew { dest, elements, descriptor } => Instruction::ListNew {
            dest: *dest,
            elements: elements.iter().map(replace_operand).collect(),
            descriptor: *descriptor,
        },
        Instruction::SetNew { dest, elements, descriptor } => Instruction::SetNew {
            dest: *dest,
            elements: elements.iter().map(replace_operand).collect(),
            descriptor: *descriptor,
        },
        Instruction::MapNew { dest, entries, descriptor } => Instruction::MapNew {
            dest: *dest,
            entries: entries
                .iter()
                .map(|(k, v)| (replace_operand(k), replace_operand(v)))
                .collect(),
            descriptor: *descriptor,
        },
        Instruction::TensorNew {
            dest,
            shape,
            elements,
        } => Instruction::TensorNew {
            dest: *dest,
            shape: shape.clone(),
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::TableNew { dest, rows } => Instruction::TableNew {
            dest: *dest,
            rows: rows.iter().map(replace_operand).collect(),
        },
        Instruction::SlotStoreCopy { dest, value } => Instruction::SlotStoreCopy {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreCopyTracked { dest, value } => Instruction::SlotStoreCopyTracked {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreMove { dest, value } => Instruction::SlotStoreMove {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreMoveTracked { dest, value } => Instruction::SlotStoreMoveTracked {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SetField {
            slot,
            field_path,
            value,
        } => Instruction::SetField {
            slot: slot.clone(),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::SetFieldTracked {
            slot,
            field_path,
            value,
        } => Instruction::SetFieldTracked {
            slot: slot.clone(),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::ParamStore { param, value } => {
            // If the param is being replaced, convert to RefStore.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefStore {
                    dest: replacement.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamStore {
                    param: *param,
                    value: replace_operand(value),
                }
            }
        }
        Instruction::ParamStoreTracked { param, value } => {
            // If the param is being replaced, convert to RefStoreTracked.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefStoreTracked {
                    dest: replacement.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamStoreTracked {
                    param: *param,
                    value: replace_operand(value),
                }
            }
        }
        Instruction::ParamSetField {
            param,
            field_path,
            value,
        } => {
            // If the param is being replaced, convert to RefSetField.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefSetField {
                    dest: replacement.clone(),
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamSetField {
                    param: *param,
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            }
        }
        Instruction::ParamSetFieldTracked {
            param,
            field_path,
            value,
        } => {
            // If the param is being replaced, convert to RefSetFieldTracked.
            if let Some(replacement) = replacements.get(param) {
                Instruction::RefSetFieldTracked {
                    dest: replacement.clone(),
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            } else {
                Instruction::ParamSetFieldTracked {
                    param: *param,
                    field_path: field_path.clone(),
                    value: replace_operand(value),
                }
            }
        }
        Instruction::RefStore { dest, value } => Instruction::RefStore {
            dest: replace_operand(dest),
            value: replace_operand(value),
        },
        Instruction::RefStoreTracked { dest, value } => Instruction::RefStoreTracked {
            dest: replace_operand(dest),
            value: replace_operand(value),
        },
        Instruction::RefSetField {
            dest,
            field_path,
            value,
        } => Instruction::RefSetField {
            dest: replace_operand(dest),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::RefSetFieldTracked {
            dest,
            field_path,
            value,
        } => Instruction::RefSetFieldTracked {
            dest: replace_operand(dest),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::Drop { operand } => Instruction::Drop {
            operand: replace_operand(operand),
        },
        Instruction::DropTracked { operand } => Instruction::DropTracked {
            operand: replace_operand(operand),
        },
        Instruction::DebugLog { operand } => Instruction::DebugLog {
            operand: replace_operand(operand),
        },
        Instruction::Intrinsic {
            dest,
            intrinsic,
            args,
        } => Instruction::Intrinsic {
            dest: *dest,
            intrinsic: intrinsic.clone(),
            args: args.iter().map(replace_operand).collect(),
        },
        Instruction::UnitEndDrop { operand } => Instruction::UnitEndDrop {
            operand: replace_operand(operand),
        },
        Instruction::UnitEndDropTracked { operand } => Instruction::UnitEndDropTracked {
            operand: replace_operand(operand),
        },
        // These don't have replaceable operands.
        Instruction::Const { .. }
        | Instruction::WrapNone { .. }
        | Instruction::SlotLoadCopy { .. }
        | Instruction::SlotLoadMove { .. }
        | Instruction::SlotLoadMoveTracked { .. }
        | Instruction::DropViaRef { .. }
        | Instruction::Nop => instr.clone(),
        Instruction::ListGet { dest, is_valid, list, index } => Instruction::ListGet {
            dest: *dest,
            is_valid: *is_valid,
            list: replace_operand(list),
            index: replace_operand(index),
        },
        Instruction::ListBoundsCheck { is_valid, list, index } => Instruction::ListBoundsCheck {
            is_valid: *is_valid,
            list: replace_operand(list),
            index: replace_operand(index),
        },
        Instruction::ListSet { list, index, value } => Instruction::ListSet {
            list: replace_operand(list),
            index: replace_operand(index),
            value: replace_operand(value),
        },
        Instruction::ListElementRef { dest, list, index } => Instruction::ListElementRef {
            dest: *dest,
            list: replace_operand(list),
            index: replace_operand(index),
        },
        Instruction::MapGet { dest, is_valid, map, key } => Instruction::MapGet {
            dest: *dest,
            is_valid: *is_valid,
            map: replace_operand(map),
            key: replace_operand(key),
        },
        Instruction::MapContainsKey { is_valid, map, key } => Instruction::MapContainsKey {
            is_valid: *is_valid,
            map: replace_operand(map),
            key: replace_operand(key),
        },
        Instruction::MapSetValue { map, key, value } => Instruction::MapSetValue {
            map: replace_operand(map),
            key: replace_operand(key),
            value: replace_operand(value),
        },
        Instruction::MapValueRef { dest, map, key } => Instruction::MapValueRef {
            dest: *dest,
            map: replace_operand(map),
            key: replace_operand(key),
        },
        Instruction::MapUpsert { map, key, value } => Instruction::MapUpsert {
            map: replace_operand(map),
            key: replace_operand(key),
            value: replace_operand(value),
        },
        Instruction::TensorGet { dest, is_valid, tensor, index } => Instruction::TensorGet {
            dest: *dest,
            is_valid: *is_valid,
            tensor: replace_operand(tensor),
            index: replace_operand(index),
        },
        Instruction::TensorBoundsCheck { is_valid, tensor, index } => Instruction::TensorBoundsCheck {
            is_valid: *is_valid,
            tensor: replace_operand(tensor),
            index: replace_operand(index),
        },
        Instruction::TensorSet { tensor, index, value } => Instruction::TensorSet {
            tensor: replace_operand(tensor),
            index: replace_operand(index),
            value: replace_operand(value),
        },
        Instruction::TensorIndexRef { dest, tensor, index } => Instruction::TensorIndexRef {
            dest: *dest,
            tensor: replace_operand(tensor),
            index: replace_operand(index),
        },
    }
}

/// Replace Param operands in a terminator with the corresponding replacement operands.
pub fn replace_params_in_terminator(
    term: &Terminator,
    replacements: &HashMap<ParamId, Operand>,
) -> Terminator {
    let replace_operand = |op: &Operand| -> Operand {
        if let Operand::Param(p) = op {
            if let Some(replacement) = replacements.get(p) {
                return replacement.clone();
            }
        }
        op.clone()
    };

    match term {
        Terminator::Goto { target, args } => Terminator::Goto {
            target: *target,
            args: args.iter().map(&replace_operand).collect(),
        },
        Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
            Terminator::Branch {
                cond: replace_operand(cond),
                then_block: *then_block,
                then_args: then_args.iter().map(&replace_operand).collect(),
                else_block: *else_block,
                else_args: else_args.iter().map(&replace_operand).collect(),
            }
        }
        Terminator::Switch { discriminant, cases, default } => Terminator::Switch {
            discriminant: replace_operand(discriminant),
            cases: cases.clone(),
            default: *default,
        },
        Terminator::Return { value } => Terminator::Return {
            value: value.as_ref().map(&replace_operand),
        },
        Terminator::UnitEnd { result } => Terminator::UnitEnd {
            result: result.as_ref().map(&replace_operand),
        },
        Terminator::UnitEarlyReturn { value } => Terminator::UnitEarlyReturn {
            value: replace_operand(value),
        },
    }
}
