//! Compile-time constant expression evaluation.
//!
//! Evaluates const expressions at compile time in three CTFE phases:
//! 1. Collect const bindings into ConstBindingGraph (Phase 1, memoized)
//! 2. Evaluate using CtfeEvaluator (Phase 2, not memoized) - THIS MODULE
//! 3. Use pre-resolved values during lowering (Phase 3, memoized)
//!
//! This module handles Phase 2: evaluating pre-lowered IR units to produce
//! `ConstValue` results. The lowering step (AST -> IR) happens in the lower
//! crate, keeping the two concerns separate, and a const that is a literal is
//! read off there without coming here at all.

use std::sync::Arc;
use datalove_datafun_ir::{ConstValue, IrType, IrCodeUnit, CtfeEvaluator, CtfeError};

/// Evaluate a const expression the lower crate lowered to a unit.
pub fn evaluate_const_unit(
    unit: &IrCodeUnit,
    ir_type: &IrType,
    evaluator: &mut dyn CtfeEvaluator,
) -> Result<Arc<ConstValue>, CtfeError> {
    evaluator.evaluate(unit, ir_type).map(Arc::new)
}
