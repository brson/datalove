//! Compile-time constant expression evaluation.
//!
//! Evaluates const expressions at compile time via the 3-phase CTFE pipeline:
//! 1. Collect const bindings into ConstBindingGraph (Phase 1, memoized)
//! 2. Evaluate using CtfeEvaluator (Phase 2, not memoized) - THIS MODULE
//! 3. Use pre-resolved values during lowering (Phase 3, memoized)
//!
//! This module handles Phase 2: evaluating pre-lowered IR units to produce
//! `ConstValue` results. The lowering step (AST -> IR) happens in the lower
//! crate, keeping the two concerns separate, and a const that is a literal is
//! read off there without coming here at all.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use datalove_datafun_ir::{ConstValue, IrType, IrCodeUnit, CtfeEvaluator, CtfeError};

/// Result of evaluating function-level consts in script functions.
pub struct ScriptFunctionConstsResult {
    /// Successfully evaluated consts: qualified name -> (type, value).
    pub consts: HashMap<String, (IrType, Arc<ConstValue>)>,
    /// Errors encountered during evaluation.
    pub errors: Vec<String>,
}

impl ScriptFunctionConstsResult {
    /// Create an empty result (no consts, no errors).
    pub fn empty() -> Self {
        Self {
            consts: HashMap::new(),
            errors: Vec::new(),
        }
    }
}

/// Evaluate a const expression the lower crate lowered to a unit.
pub fn evaluate_const_unit(
    unit: &IrCodeUnit,
    ir_type: &IrType,
    evaluator: &Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<Arc<ConstValue>, CtfeError> {
    evaluator.borrow_mut().evaluate(unit, ir_type).map(Arc::new)
}
