//! Compile-time constant expression evaluation.
//!
//! Evaluates const expressions at compile time via the 3-phase CTFE pipeline:
//! 1. Collect const bindings into ConstBindingGraph (Phase 1, memoized)
//! 2. Evaluate using CtfeEvaluator (Phase 2, not memoized) - THIS MODULE
//! 3. Use pre-resolved values during lowering (Phase 3, memoized)
//!
//! This module handles Phase 2: evaluating pre-lowered IR units to produce
//! `ConstValue` results. The lowering step (AST -> IR) happens in the lower
//! crate, keeping the two concerns separate.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use datalove_datafun_ir::{
    ConstValue, IrType, IrCodeUnit, ConstBindingGraph, ResolvedConsts,
    ConstEvalError, CtfeEvaluator, CtfeError, ConstStmtId,
};

/// Result of evaluating function-level consts in script functions.
pub struct ScriptFunctionConstsResult {
    /// Successfully evaluated consts: qualified name -> (type, value).
    pub consts: HashMap<String, (IrType, ConstValue)>,
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

/// A const expression prepared for evaluation.
///
/// Either a simple literal value (no interpreter needed) or a pre-lowered
/// IR unit that must be executed.
pub enum PreparedConst {
    /// Simple literal extracted without lowering.
    Simple(ConstValue),
    /// Pre-lowered IR unit that needs interpreter execution.
    Unit(IrCodeUnit),
}

/// Evaluate a single pre-lowered const expression.
///
/// Takes a prepared const (either simple value or IR unit) and returns
/// the evaluated ConstValue.
pub fn evaluate_prepared_const(
    prepared: &PreparedConst,
    ir_type: &IrType,
    evaluator: &Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<ConstValue, CtfeError> {
    match prepared {
        PreparedConst::Simple(value) => Ok(value.clone()),
        PreparedConst::Unit(unit) => {
            evaluator.borrow_mut().evaluate(unit, ir_type)
        }
    }
}

/// Evaluate all const bindings using pre-lowered IR units.
///
/// Evaluates consts in topological order (dependencies before dependents).
/// Each binding has already been lowered to either a simple value or an
/// IR unit by the lower crate.
///
/// # Arguments
///
/// * `graph` - The const binding graph with dependency order
/// * `prepared` - Pre-lowered consts, indexed by stmt_id
/// * `evaluator` - The CTFE evaluator for executing IR units
pub fn evaluate_consts_prepared(
    graph: &ConstBindingGraph,
    prepared: &HashMap<ConstStmtId, PreparedConst>,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<ResolvedConsts, ConstEvalError> {
    let mut resolved = ResolvedConsts::new();

    for binding in &graph.bindings {
        let prepared_const = prepared.get(&binding.stmt_id)
            .expect("prepared const should exist for binding");

        let value = evaluate_prepared_const(prepared_const, &binding.ir_type, &evaluator)
            .map_err(|e| match e {
                CtfeError::InterpError(msg) if msg.contains("gas") => {
                    ConstEvalError::GasExpired { binding_name: binding.name.clone() }
                }
                CtfeError::InterpError(msg) => {
                    ConstEvalError::LoweringFailed {
                        binding_name: binding.name.clone(),
                        message: msg,
                    }
                }
                CtfeError::EarlyReturn(msg) => {
                    ConstEvalError::EarlyReturn {
                        binding_name: binding.name.clone(),
                        message: msg,
                    }
                }
                CtfeError::UnsupportedType(ty) => {
                    ConstEvalError::UnsupportedType {
                        binding_name: binding.name.clone(),
                        type_name: ty,
                    }
                }
            })?;

        resolved.insert(binding.stmt_id, binding.name.clone(), value);
    }

    Ok(resolved)
}

/// Evaluate function-level consts using pre-lowered IR units.
///
/// This extends Phase 2 to evaluate consts defined inside function bodies.
/// Returns a map of qualified names (`func_name::const_name`) to values.
///
/// # Arguments
///
/// * `prepared` - Pre-lowered consts keyed by qualified name
/// * `evaluator` - The CTFE evaluator for executing IR units
pub fn evaluate_function_consts_prepared(
    prepared: &HashMap<String, (IrType, PreparedConst)>,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> ScriptFunctionConstsResult {
    let mut consts = HashMap::new();
    let mut errors = Vec::new();

    // Evaluated in name order. The evaluator carries state from one const to
    // the next, and failures are reported in the order they are met, so
    // neither should depend on how a hash map happened to lay the consts out.
    let mut names: Vec<&String> = prepared.keys().collect();
    names.sort();

    for qualified_name in names {
        let (ir_type, prepared_const) = &prepared[qualified_name];
        match evaluate_prepared_const(prepared_const, ir_type, &evaluator) {
            Ok(value) => {
                consts.insert(qualified_name.clone(), (ir_type.clone(), value));
            }
            Err(e) => {
                errors.push(format!("{}: {}", qualified_name, e));
            }
        }
    }

    ScriptFunctionConstsResult { consts, errors }
}

/// Try to extract a literal value directly from an expression kind.
///
/// This is a helper for simple cases where no lowering or interpretation
/// is needed. Returns None if the expression is not a simple literal.
///
/// Note: This requires access to the AST, which is not available in this
/// crate. Callers should use the lower crate's try_extract_literal or
/// eval_const_expr_simple functions instead.
pub fn simple_literal_from_const_value(value: &ConstValue) -> Option<ConstValue> {
    // ConstValue is already a literal, so just clone it.
    Some(value.clone())
}
