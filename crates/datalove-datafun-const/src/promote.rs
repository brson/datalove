//! Promoting a function's consts to statics.
//!
//! After const inlining, a const binding in a function body is a value defined
//! by a `Const`: built where it is declared, read and borrowed in place, and
//! dropped at the end of its scope, once per call. A specialized copy's const
//! parameters have the same shape, a `Const` at entry standing for each.
//!
//! A const is borrowed wherever it is named, which ownership analysis enforces,
//! so nothing ever consumes such a value. That makes it safe to build once: the
//! defining `Const` becomes a `StaticRef`, the value's type becomes a reference
//! to what it was, every use reads through that reference, and its drops go,
//! since nothing owns what it points at.
//!
//! Only values the unit lists in `const_values` are promoted, and only those
//! still defined by a `Const`: a const naming a const parameter in the
//! unspecialized original is an ordinary binding, and so is every const when
//! const inlining is skipped.

use std::collections::HashSet;
use std::sync::Arc;

use datalove_datafun_ir::{
    map_operands_in_instruction, map_operands_in_terminator, Instruction, IrCodeUnit, IrType,
    Operand, ValueId,
};

/// Promote a function's const bindings of non-copy types to statics.
pub fn promote_function_consts(func: &mut IrCodeUnit) {
    let consts: HashSet<ValueId> = func.const_values.iter().map(|(_, v)| *v).collect();

    let mut promoted: HashSet<ValueId> = HashSet::new();
    for block in &mut func.blocks {
        for instr in &mut block.instructions {
            let Instruction::Const { dest, value } = instr else { continue };
            if !consts.contains(dest) || func.value_types[dest.0 as usize].is_copy() {
                continue;
            }
            let dest = *dest;
            let value = Arc::new(std::mem::replace(value, datalove_datafun_ir::ConstValue::Unit));
            *instr = Instruction::StaticRef { dest, value: datalove_datafun_ir::SharedConst(value) };
            promoted.insert(dest);
        }
    }
    if promoted.is_empty() {
        return;
    }

    for v in &promoted {
        let ty = &mut func.value_types[v.0 as usize];
        *ty = IrType::Ref(Box::new(std::mem::replace(ty, IrType::Unit)));
    }

    let through_ref = |op: &Operand| match op {
        Operand::Value(v) if promoted.contains(v) => Operand::ValueRef(*v),
        other => other.clone(),
    };
    for block in &mut func.blocks {
        block.instructions.retain(|instr| match instr {
            Instruction::Drop { operand } | Instruction::DropTracked { operand } => {
                !matches!(operand, Operand::Value(v) if promoted.contains(v))
            }
            _ => true,
        });
        for instr in &mut block.instructions {
            *instr = map_operands_in_instruction(instr, through_ref);
        }
        block.terminator = map_operands_in_terminator(&block.terminator, through_ref);
    }

    // A const whose only use was its own drop at the end of its scope, as one
    // passed only as a const argument is once specialization has taken the
    // argument away, is now used by nothing.
    crate::dce::eliminate_dead_code_unit(func);
}
