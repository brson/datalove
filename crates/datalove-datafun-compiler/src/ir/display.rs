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

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Value(v) => write!(f, "{}", v),
            Operand::Slot(s) => write!(f, "{}", s),
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
        }
    }
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
            Instruction::FieldAccess { dest, base, field } => {
                write!(f, "{} = {}.{}", dest, base, field)
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
            Instruction::UnwrapResult { dest, is_ok, src } => {
                write!(f, "{}, {} = unwrap_result {}", dest, is_ok, src)
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
            Instruction::SlotStore { slot, value } => {
                write!(f, "store {}, {}", slot, value)
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
