//! Pretty-printing for IR.

use std::fmt;
use super::*;

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

impl fmt::Display for FuncRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FuncRef::Local(id) => write!(f, "{}", id),
            FuncRef::External { unit, func } => write!(f, "unit{}.{}", unit, func),
            FuncRef::Module { module, func } => write!(f, "{}.{}", module, func),
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
            Operand::Slot(s) => write!(f, "{}", s),
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
            ConstValue::Int { limbs, negative } => {
                if limbs.is_empty() {
                    write!(f, "0int")
                } else {
                    let s = limbs_to_decimal(limbs, *negative);
                    write!(f, "{}int", s)
                }
            }
            ConstValue::F32(n) => write!(f, "{}f32", n),
            ConstValue::String(s) => write!(f, "{:?}", s),
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
            Instruction::Call { dest, func, args } => {
                write!(f, "{} = call {}(", dest, func)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
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
            Instruction::FieldAccess { dest, base, field_index } => {
                write!(f, "{} = {}.field{}", dest, base, field_index)
            }
            Instruction::TupleIndex { dest, base, index } => {
                write!(f, "{} = {}.{}", dest, base, index)
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
                write!(f, "{} = set {{", dest)?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "}}")
            }
            Instruction::MapNew { dest, entries } => {
                write!(f, "{} = map {{", dest)?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "}}")
            }
            Instruction::TensorNew { dest, shape, elements } => {
                write!(f, "{} = tensor [", dest)?;
                for (i, dim) in shape.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", dim)?;
                }
                write!(f, "] [")?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, "]")
            }
            Instruction::SlotStore { dest, value } => {
                write!(f, "store {}, {}", dest, value)
            }
            Instruction::SlotLoad { dest, slot } => {
                write!(f, "{} = load {}", dest, slot)
            }
            Instruction::Phi { dest, incoming } => {
                write!(f, "{} = phi ", dest)?;
                for (i, (block, op)) in incoming.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "[{}: {}]", block, op)?;
                }
                Ok(())
            }
            Instruction::Drop { operand } => {
                write!(f, "drop {}", operand)
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
            Terminator::Goto(block) => {
                write!(f, "goto {}", block)
            }
            Terminator::Branch { cond, then_block, else_block } => {
                write!(f, "branch {}, {}, {}", cond, then_block, else_block)
            }
            Terminator::Return { value: Some(v) } => {
                write!(f, "return {}", v)
            }
            Terminator::Return { value: None } => {
                write!(f, "return")
            }
            Terminator::TryReturn { value: Some(v) } => {
                write!(f, "try_return {}", v)
            }
            Terminator::TryReturn { value: None } => {
                write!(f, "try_return")
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
        }
    }
}

impl fmt::Display for IrBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}:", self.id)?;
        for instr in &self.instructions {
            writeln!(f, "    {}", instr)?;
        }
        writeln!(f, "    {}", self.terminator)
    }
}

impl fmt::Display for IrFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fn {}(", self.name)?;
        for (i, param) in self.params.iter().enumerate() {
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

impl fmt::Display for ExportBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportBinding::Value(v) => write!(f, "{}", v),
            ExportBinding::Slot(s) => write!(f, "{}", s),
            ExportBinding::Function(id) => write!(f, "{}", id),
        }
    }
}

impl fmt::Display for IrScriptUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "scriptunit:")?;
        for block in &self.blocks {
            write!(f, "{}", block)?;
        }
        if !self.functions.is_empty() {
            writeln!(f)?;
            for func in &self.functions {
                write!(f, "{}", func)?;
            }
        }
        if !self.exports.is_empty() {
            writeln!(f, "exports:")?;
            for (name, binding) in &self.exports {
                writeln!(f, "    {} = {}", name, binding)?;
            }
        }
        Ok(())
    }
}
