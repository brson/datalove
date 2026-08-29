//! Pretty-printing for IR.

use rmx::regex::Regex;
use std::fmt;
use crate::*;

/// Expand `ir: "..."` fields in RON output into readable multiline format.
///
/// Transforms escaped strings like:
///   `ir: "line1\nline2\n",`
/// Into triple-quoted multiline format:
///   ```text
///   ir: """
///       line1
///       line2
///   """,
///   ```
///
/// This makes IR dumps in test expected files human-readable while preserving
/// the ability to compare actual vs expected output (both use the same format).
pub fn expand_ir_strings(ron: &str) -> String {
    // Match `ir: "...",` where the string may contain escaped characters.
    let re = Regex::new(r#"(?m)^(\s*)ir: "((?:[^"\\]|\\.)*)","#).unwrap();

    re.replace_all(ron, |caps: &rmx::regex::Captures| {
        let indent = &caps[1];
        let escaped_content = &caps[2];

        // Unescape the string content.
        let content = escaped_content
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");

        // Build multiline format with proper indentation.
        let inner_indent = format!("{}    ", indent);
        let mut result = format!("{}ir: \"\"\"\n", indent);
        for line in content.lines() {
            result.push_str(&inner_indent);
            result.push_str(line);
            result.push('\n');
        }
        result.push_str(indent);
        result.push_str("\"\"\",");
        result
    }).to_string()
}

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

impl fmt::Display for ParamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block{}", self.0)
    }
}

impl fmt::Display for FuncId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f{}", self.0)
    }
}

impl fmt::Display for IrModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "m{}", self.0)
    }
}

impl fmt::Display for CodeUnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "u{}", self.0)
    }
}

impl fmt::Display for CodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodeRef::Local(id) => write!(f, "{}", id),
            CodeRef::External { unit, id } => write!(f, "unit{}.{}", unit, id),
            CodeRef::Module { module, id } => write!(f, "{}.{}", module, id),
        }
    }
}

impl fmt::Display for TypeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeRef::Bool => write!(f, "Bool"),
            TypeRef::U8 => write!(f, "U8"),
            TypeRef::U16 => write!(f, "U16"),
            TypeRef::U32 => write!(f, "U32"),
            TypeRef::U64 => write!(f, "U64"),
            TypeRef::I8 => write!(f, "I8"),
            TypeRef::I16 => write!(f, "I16"),
            TypeRef::I32 => write!(f, "I32"),
            TypeRef::I64 => write!(f, "I64"),
            TypeRef::Index => write!(f, "Index"),
            TypeRef::Offset => write!(f, "Offset"),
            TypeRef::Int => write!(f, "Int"),
            TypeRef::Tuple(0) => write!(f, "()"),
            TypeRef::Tuple(n) => write!(f, "Tuple{}", n),
            TypeRef::AnonStruct(n) => write!(f, "Struct{}", n),
            TypeRef::Option => write!(f, "Option"),
            TypeRef::Result => write!(f, "Result"),
            TypeRef::List => write!(f, "List"),
            TypeRef::Set => write!(f, "Set"),
            TypeRef::Map => write!(f, "Map"),
        }
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Value(v) => write!(f, "{}", v),
            Operand::ValueRef(v) => write!(f, "*{}", v),
            Operand::Slot(s) => write!(f, "{}", s),
            Operand::Param(p) => write!(f, "{}", p),
            Operand::ExternalValue { unit, value } => write!(f, "unit{}.{}", unit, value),
            Operand::ExternalSlot { unit, slot } => write!(f, "unit{}.{}", unit, slot),
        }
    }
}

impl fmt::Display for SlotDest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SlotDest::Local(s) => write!(f, "{}", s),
            SlotDest::External { unit, slot } => write!(f, "unit{}.{}", unit, slot),
        }
    }
}

impl fmt::Display for ConstValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstValue::Unit => write!(f, "()"),
            ConstValue::Bool(b) => write!(f, "{}", b),
            ConstValue::U8(n) => write!(f, "{}u8", n),
            ConstValue::U16(n) => write!(f, "{}u16", n),
            ConstValue::U32(n) => write!(f, "{}u32", n),
            ConstValue::U64(n) => write!(f, "{}u64", n),
            ConstValue::I8(n) => write!(f, "{}i8", n),
            ConstValue::I16(n) => write!(f, "{}i16", n),
            ConstValue::I32(n) => write!(f, "{}i32", n),
            ConstValue::I64(n) => write!(f, "{}i64", n),
            ConstValue::Index(n) => write!(f, "{}index", n),
            ConstValue::Offset(n) => write!(f, "{}offset", n),
            ConstValue::Int { limbs, negative } => {
                if limbs.is_empty() {
                    write!(f, "0int")
                } else {
                    let s = limbs_to_decimal(limbs, *negative);
                    write!(f, "{}int", s)
                }
            }
            ConstValue::F32(n) => write!(f, "{}f32", n),
            ConstValue::F64(n) => write!(f, "{}f64", n),
            ConstValue::String(s) => write!(f, "{:?}", s),
            ConstValue::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, ")")
            }
            ConstValue::Struct(fields) => {
                write!(f, "{{")?;
                for (i, (name, val)) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{} = {}", name, val)?;
                }
                write!(f, "}}")
            }
            ConstValue::Enum { variant, payload } => {
                write!(f, "enum {}", variant)?;
                if let Some(p) = payload {
                    write!(f, "({})", p)?;
                }
                Ok(())
            }
            ConstValue::OptionSome(v) => write!(f, "some {}", v),
            ConstValue::OptionNone => write!(f, "none"),
            ConstValue::ResultOk(v) => write!(f, "ok {}", v),
            ConstValue::ResultErr(v) => write!(f, "err {}", v),
            ConstValue::Data(v) => write!(f, "data {}", v),
            ConstValue::Error(v) => write!(f, "error {}", v),
            ConstValue::List(elems) => {
                write!(f, "[")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }
            ConstValue::Set(elems) => {
                write!(f, "#{{")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", e)?;
                }
                write!(f, "}}")
            }
            ConstValue::Map(entries) => {
                write!(f, "%{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{} = {}", k, v)?;
                }
                write!(f, "}}")
            }
            ConstValue::Table { columns, rows } => {
                write!(f, "{{| ")?;
                for (i, col) in columns.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", col)?;
                }
                for row in rows {
                    write!(f, "; ")?;
                    for (i, val) in row.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", val)?;
                    }
                }
                write!(f, " |}}")
            }
        }
    }
}

/// Convert limbs (little-endian base 2^32) to decimal string.
fn limbs_to_decimal(limbs: &[u32], negative: bool) -> String {
    if limbs.is_empty() {
        return "0".to_string();
    }

    // Work with a copy of the limbs.
    let mut working = limbs.to_vec();

    // Convert to decimal by repeated division by 10^9.
    const DIVISOR: u64 = 1_000_000_000;
    let mut chunks = Vec::new();

    loop {
        // Divide working by DIVISOR, collecting remainder.
        let mut remainder: u64 = 0;
        let mut all_zero = true;

        for i in (0..working.len()).rev() {
            let current = (remainder << 32) | (working[i] as u64);
            working[i] = (current / DIVISOR) as u32;
            remainder = current % DIVISOR;

            if working[i] != 0 {
                all_zero = false;
            }
        }

        chunks.push(remainder as u32);

        if all_zero {
            break;
        }
    }

    // Build string from chunks in reverse order.
    let mut result = String::new();

    if negative {
        result.push('-');
    }

    // First chunk has no leading zeros.
    result.push_str(&chunks.last().unwrap().to_string());

    // Remaining chunks are padded to 9 digits.
    for i in (0..chunks.len() - 1).rev() {
        result.push_str(&format!("{:09}", chunks[i]));
    }

    result
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Mod => "mod",
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::Lt => "lt",
            BinOp::Le => "le",
            BinOp::Gt => "gt",
            BinOp::Ge => "ge",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::BitAnd => "bitand",
            BinOp::BitOr => "bitor",
            BinOp::BitXor => "bitxor",
            BinOp::Shl => "shl",
            BinOp::Shr => "shr",
            BinOp::LogicAnd => "logic_and",
            BinOp::LogicOr => "logic_or",
            BinOp::LogicXor => "logic_xor",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            UnaryOp::Neg => "neg",
            UnaryOp::Not => "not",
            UnaryOp::BitNot => "bitnot",
            UnaryOp::LogicNot => "logic_not",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Instruction::Const { dest, value } => {
                write!(f, "{} = const {}", dest, value)
            }
            Instruction::Copy { dest, src } => {
                write!(f, "{} = copy {}", dest, src)
            }
            Instruction::Move { dest, src } => {
                write!(f, "{} = move {}", dest, src)
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                write!(f, "{} = {} {}, {}", dest, op, lhs, rhs)
            }
            Instruction::UnaryOp { dest, op, operand } => {
                write!(f, "{} = {} {}", dest, op, operand)
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                write!(f, "{}, {} = {}.checked {}, {}", dest, overflow, op, lhs, rhs)
            }
            Instruction::UnaryOpChecked { dest, overflow, op, operand } => {
                write!(f, "{}, {} = {}.checked {}", dest, overflow, op, operand)
            }
            Instruction::Widen { dest, src } => {
                write!(f, "{} = widen {}", dest, src)
            }
            Instruction::WidenFixed { dest, src } => {
                write!(f, "{} = widen_fixed {}", dest, src)
            }
            Instruction::Clone { dest, src } => {
                write!(f, "{} = clone {}", dest, src)
            }
            Instruction::Call { site_id, dest, func, args } => {
                write!(f, "{} = call @{} {}(", dest, site_id.0, func)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Instruction::ComptimeCall { dest, func, args, discriminant, comptime_param_indices } => {
                write!(f, "{} = comptime_call {}(", dest, func)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ") [disc={}, comptime_params={:?}]", discriminant, comptime_param_indices)
            }
            Instruction::Pack { dest, ty, fields } => {
                write!(f, "{} = pack {} {{", dest, ty)?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", field)?;
                }
                write!(f, "}}")
            }
            Instruction::Unpack { dests, src } => {
                write!(f, "(")?;
                for (i, dest) in dests.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", dest)?;
                }
                write!(f, ") = unpack {}", src)
            }
            Instruction::GetField { dest, src, field_index } => {
                write!(f, "{} = getfield {}.{}", dest, src, field_index)
            }
            Instruction::GetFieldRef { dest, src, field_index } => {
                write!(f, "{} = getfieldref {}.{}", dest, src, field_index)
            }
            Instruction::WrapSome { dest, inner } => {
                write!(f, "{} = some {}", dest, inner)
            }
            Instruction::WrapOk { dest, inner } => {
                write!(f, "{} = ok {}", dest, inner)
            }
            Instruction::WrapErr { dest, inner } => {
                write!(f, "{} = err {}", dest, inner)
            }
            Instruction::WrapNone { dest } => {
                write!(f, "{} = none", dest)
            }
            Instruction::EnumVariant { dest, variant_index, payload } => {
                if let Some(p) = payload {
                    write!(f, "{} = enum_variant {} {}", dest, variant_index, p)
                } else {
                    write!(f, "{} = enum_variant {}", dest, variant_index)
                }
            }
            Instruction::EnumDiscriminant { dest, src } => {
                write!(f, "{} = enum_discriminant {}", dest, src)
            }
            Instruction::EnumPayload { dest, src, variant_index } => {
                write!(f, "{} = enum_payload {} {}", dest, src, variant_index)
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                write!(f, "{}, {} = unwrap_option {}", dest, is_some, src)
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                write!(f, "{}, {}, {} = unwrap_result {}", ok_dest, err_dest, is_ok, src)
            }
            Instruction::ErrorFrom { dest, inner } => {
                write!(f, "{} = error_from {}", dest, inner)
            }
            Instruction::DataFrom { dest, inner } => {
                write!(f, "{} = data_from {}", dest, inner)
            }
            Instruction::Erase { dest, src } => {
                write!(f, "{} = erase {}", dest, src)
            }
            Instruction::Reify { dest, src } => {
                write!(f, "{} = reify {}", dest, src)
            }
            Instruction::ListNew { dest, elements } => {
                write!(f, "{} = list [", dest)?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "]")
            }
            Instruction::SetNew { dest, elements } => {
                write!(f, "{} = #{{", dest)?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "}}")
            }
            Instruction::MapNew { dest, entries } => {
                write!(f, "{} = %{{", dest)?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Instruction::TensorNew { dest, shape, elements } => {
                write!(f, "{} = [| ", dest)?;
                write_tensor_elements_ir(f, shape, elements)?;
                write!(f, " |]")
            }
            Instruction::TableNew { dest, rows } => {
                write!(f, "{} = table [", dest)?;
                for (i, row) in rows.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", row)?;
                }
                write!(f, "]")
            }
            Instruction::SlotStoreCopy { dest, value } => {
                write!(f, "store.copy {}, {}", dest, value)
            }
            Instruction::SlotStoreCopyTracked { dest, value } => {
                write!(f, "store.copy.tracked {}, {}", dest, value)
            }
            Instruction::SlotStoreMove { dest, value } => {
                write!(f, "store.move {}, {}", dest, value)
            }
            Instruction::SlotStoreMoveTracked { dest, value } => {
                write!(f, "store.move.tracked {}, {}", dest, value)
            }
            Instruction::SetField { slot, field_path, value } => {
                write!(f, "setfield {}", slot)?;
                for idx in field_path {
                    write!(f, ".{}", idx)?;
                }
                write!(f, ", {}", value)
            }
            Instruction::SetFieldTracked { slot, field_path, value } => {
                write!(f, "setfield.tracked {}", slot)?;
                for idx in field_path {
                    write!(f, ".{}", idx)?;
                }
                write!(f, ", {}", value)
            }
            Instruction::ParamStore { param, value } => {
                write!(f, "store {}, {}", param, value)
            }
            Instruction::ParamStoreTracked { param, value } => {
                write!(f, "store.tracked {}, {}", param, value)
            }
            Instruction::ParamSetField { param, field_path, value } => {
                write!(f, "setfield {}", param)?;
                for idx in field_path {
                    write!(f, ".{}", idx)?;
                }
                write!(f, ", {}", value)
            }
            Instruction::ParamSetFieldTracked { param, field_path, value } => {
                write!(f, "setfield.tracked {}", param)?;
                for idx in field_path {
                    write!(f, ".{}", idx)?;
                }
                write!(f, ", {}", value)
            }
            Instruction::RefStore { dest, value } => {
                write!(f, "refstore {}, {}", dest, value)
            }
            Instruction::RefSetField { dest, field_path, value } => {
                write!(f, "refsetfield {}", dest)?;
                for idx in field_path {
                    write!(f, ".{}", idx)?;
                }
                write!(f, ", {}", value)
            }
            Instruction::RefStoreTracked { dest, value } => {
                write!(f, "refstore.tracked {}, {}", dest, value)
            }
            Instruction::RefSetFieldTracked { dest, field_path, value } => {
                write!(f, "refsetfield.tracked {}", dest)?;
                for idx in field_path {
                    write!(f, ".{}", idx)?;
                }
                write!(f, ", {}", value)
            }
            Instruction::SlotLoadCopy { dest, slot } => {
                write!(f, "{} = load.copy {}", dest, slot)
            }
            Instruction::SlotLoadMove { dest, slot } => {
                write!(f, "{} = load.move {}", dest, slot)
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                write!(f, "{} = load.move.tracked {}", dest, slot)
            }
            Instruction::Drop { operand } => {
                write!(f, "drop {}", operand)
            }
            Instruction::DropTracked { operand } => {
                write!(f, "drop.tracked {}", operand)
            }
            Instruction::DropViaRef { ref_value } => {
                write!(f, "drop.ref {}", ref_value)
            }
            Instruction::UnitEndDrop { operand } => {
                write!(f, "unit_end_drop {}", operand)
            }
            Instruction::UnitEndDropTracked { operand } => {
                write!(f, "unit_end_drop.tracked {}", operand)
            }
            Instruction::ListGet { dest, is_valid, list, index } => {
                write!(f, "{}, {} = listget {}[{}]", dest, is_valid, list, index)
            }
            Instruction::ListBoundsCheck { is_valid, list, index } => {
                write!(f, "{} = listboundscheck {}[{}]", is_valid, list, index)
            }
            Instruction::ListSet { list, index, value } => {
                write!(f, "listset {}[{}] = {}", list, index, value)
            }
            Instruction::ListElementRef { dest, list, index } => {
                write!(f, "{} = listelementref {}[{}]", dest, list, index)
            }
            Instruction::MapGet { dest, is_valid, map, key } => {
                write!(f, "{}, {} = mapget {}[{}]", dest, is_valid, map, key)
            }
            Instruction::MapContainsKey { is_valid, map, key } => {
                write!(f, "{} = mapcontainskey {}[{}]", is_valid, map, key)
            }
            Instruction::MapSetValue { map, key, value } => {
                write!(f, "mapsetvalue {}[{}] = {}", map, key, value)
            }
            Instruction::MapValueRef { dest, map, key } => {
                write!(f, "{} = mapvalueref {}[{}]", dest, map, key)
            }
            Instruction::MapUpsert { map, key, value } => {
                write!(f, "mapupsert {}[{}] = {}", map, key, value)
            }
            Instruction::TensorGet { dest, is_valid, tensor, index } => {
                write!(f, "{}, {} = tensorget {}[{}]", dest, is_valid, tensor, index)
            }
            Instruction::TensorBoundsCheck { is_valid, tensor, index } => {
                write!(f, "{} = tensorboundscheck {}[{}]", is_valid, tensor, index)
            }
            Instruction::TensorSet { tensor, index, value } => {
                write!(f, "tensorset {}[{}] = {}", tensor, index, value)
            }
            Instruction::TensorIndexRef { dest, tensor, index } => {
                write!(f, "{} = tensorindexref {}[{}]", dest, tensor, index)
            }
            Instruction::DebugLog { operand } => {
                write!(f, "debuglog {}", operand)
            }
            Instruction::Intrinsic { dest, intrinsic, args } => {
                write!(f, "{} = intrinsic {:?}(", dest, intrinsic)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Instruction::Nop => {
                write!(f, "nop")
            }
        }
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Goto { target, args } => {
                write!(f, "goto {}", target)?;
                if !args.is_empty() {
                    write!(f, "(")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                write!(f, "branch {}, {}", cond, then_block)?;
                if !then_args.is_empty() {
                    write!(f, "(")?;
                    for (i, arg) in then_args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ")")?;
                }
                write!(f, ", {}", else_block)?;
                if !else_args.is_empty() {
                    write!(f, "(")?;
                    for (i, arg) in else_args.iter().enumerate() {
                        if i > 0 { write!(f, ", ")?; }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            Terminator::Return { value: Some(v) } => {
                write!(f, "return {}", v)
            }
            Terminator::Return { value: None } => {
                write!(f, "return")
            }
            Terminator::UnitEnd { result: Some(v) } => {
                write!(f, "unit_end {}", v)
            }
            Terminator::UnitEnd { result: None } => {
                write!(f, "unit_end")
            }
            Terminator::UnitEarlyReturn { value } => {
                write!(f, "unit_early_return {}", value)
            }
            Terminator::Switch { discriminant, cases, default } => {
                write!(f, "switch {}, [", discriminant)?;
                for (i, (val, block)) in cases.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{} => {}", val, block)?;
                }
                write!(f, "], default => {}", default)
            }
        }
    }
}

impl fmt::Display for IrBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.id)?;
        if !self.params.is_empty() {
            write!(f, "(")?;
            for (i, param) in self.params.iter().enumerate() {
                if i > 0 { write!(f, ", ")?; }
                write!(f, "{}", param)?;
            }
            write!(f, ")")?;
        }
        writeln!(f, ":")?;
        for instr in &self.instructions {
            writeln!(f, "    {}", instr)?;
        }
        writeln!(f, "    {}", self.terminator)
    }
}

impl fmt::Display for IrModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, func) in self.functions.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", func)?;
        }
        Ok(())
    }
}

impl fmt::Display for IrCodeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.context {
            CodeUnitContext::Function(ctx) => {
                write!(f, "fn {}(", self.name)?;
                for (i, param) in ctx.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                writeln!(f, "):")?;
                for block in &self.blocks {
                    write!(f, "{}", block)?;
                }
                Ok(())
            }
            CodeUnitContext::Script(_) => {
                writeln!(f, "scriptunit:")?;
                for block in &self.blocks {
                    write!(f, "{}", block)?;
                }
                // Print nested functions.
                for nested in &self.nested_units {
                    writeln!(f)?;
                    write!(f, "{}", nested)?;
                }
                Ok(())
            }
            CodeUnitContext::Native(ctx) => {
                writeln!(f, "native fn {} -> symbol {}", self.name, ctx.symbol)
            }
        }
    }
}

impl fmt::Display for ExportBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportBinding::Value(v) => write!(f, "{}", v),
            ExportBinding::Slot(s) => write!(f, "{}", s),
            ExportBinding::Function(id) => write!(f, "{}", id),
        }
    }
}

/// Write tensor elements with multi-comma layout for IR display.
fn write_tensor_elements_ir(
    f: &mut fmt::Formatter<'_>,
    shape: &[u32],
    elements: &[Operand],
) -> fmt::Result {
    if elements.is_empty() || shape.is_empty() {
        return Ok(());
    }
    let rank = shape.len();
    write_tensor_group_ir(f, shape, elements, 0)?;

    // When the outermost dimension is 1, the highest comma level (rank - 1)
    // never appears as a separator. Emit trailing commas so the parser can
    // infer the correct rank.
    if rank > 1 && shape[0] == 1 {
        for _ in 0..(rank - 1) {
            write!(f, ",")?;
        }
    }
    Ok(())
}

/// Recursively write a tensor group at the given dimension level.
fn write_tensor_group_ir(
    f: &mut fmt::Formatter<'_>,
    shape: &[u32],
    elements: &[Operand],
    dim: usize,
) -> fmt::Result {
    let rank = shape.len();

    if dim == rank - 1 {
        // Innermost dimension: space-separated elements.
        for (i, elem) in elements.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", elem)?;
        }
        return Ok(());
    }

    let group_size: usize = shape[dim + 1..].iter().map(|&d| d as usize).product();
    let num_groups = shape[dim] as usize;
    let comma_count = rank - dim - 1;

    for (i, chunk) in elements.chunks(group_size).enumerate().take(num_groups) {
        if i > 0 {
            for _ in 0..comma_count {
                write!(f, ",")?;
            }
            write!(f, " ")?;
        }
        write_tensor_group_ir(f, shape, chunk, dim + 1)?;
    }

    Ok(())
}

