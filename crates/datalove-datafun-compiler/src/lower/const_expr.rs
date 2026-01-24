//! Compile-time constant expression evaluation.
//!
//! Evaluates expressions at compile time. Simple literals are extracted directly.
//! Complex expressions are lowered to IR using the real lowering pipeline and
//! evaluated via a pluggable `CtfeEvaluator`.
//!
//! This module uses "isolated lowering" - creating a fresh `LowerCtx` that reuses
//! type information but has independent IR state. This ensures CTFE gets all the
//! same lowering behavior as runtime code (widening, checked ops, etc.) without
//! code duplication.

use std::collections::HashMap;
use datalove_datafun_ast::ast::{ExprFun, ExprFunKind};
use datalove_datafun_ir::{
    ConstValue, IrType, IrScriptUnit,
    Operand, Terminator, SymbolTable,
};
use super::context::LowerCtx;
use super::LowerError;

// Empty arrays for isolated contexts that don't need call resolution.
static EMPTY_CALL_TARGETS: Vec<Option<datalove_datafun_tycheck::ResolvedCallTarget<'static>>> = Vec::new();

/// Evaluate a constant expression at compile time.
///
/// - Simple literals are extracted directly without needing an evaluator.
/// - Const name references look up previously computed values.
/// - Complex expressions require a `CtfeEvaluator` to be configured.
pub fn eval_const_expr<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ConstValue, LowerError> {
    // For simple literals, extract directly.
    if let Some(value) = try_eval_literal(ctx, expr) {
        return value;
    }

    // For const references, look up the previously computed value.
    if let ExprFunKind::Name(name) = expr.expr(ctx.db) {
        let name_str = name.text(ctx.db);
        if let Some((_, value)) = ctx.lookup_const(name_str) {
            return Ok(value.clone());
        }
        return Err(LowerError::NotImplemented(format!(
            "non-const variable '{}' in const expression",
            name_str
        )));
    }

    // For complex expressions, lower to IR and use the evaluator.
    let ir_type = ctx.expr_type(expr);
    let unit = lower_const_expr_to_unit(ctx, expr)?;

    // Get the evaluator from context.
    let evaluator = ctx.ctfe_evaluator()
        .ok_or_else(|| LowerError::NotImplemented(
            "complex const expressions require a CTFE evaluator".to_string()
        ))?;

    evaluator.borrow_mut()
        .evaluate(&unit, &ir_type)
        .map_err(|e| LowerError::NotImplemented(format!("CTFE error: {}", e)))
}

/// Evaluate a constant expression without a full LowerCtx.
///
/// This is used for module-level const evaluation where we don't have access
/// to a CTFE evaluator (tracked functions can't take trait objects).
///
/// Supports:
/// - Simple literals (int, float, bool, string, none)
/// - References to previously evaluated consts
///
/// Does NOT support complex expressions (binary ops, function calls, etc.).
pub fn eval_const_expr_simple<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
) -> Result<ConstValue, LowerError> {
    match expr.expr(db) {
        ExprFunKind::True(_) => Ok(ConstValue::Bool(true)),
        ExprFunKind::False(_) => Ok(ConstValue::Bool(false)),
        ExprFunKind::None(_) => Ok(ConstValue::OptionNone),

        ExprFunKind::Int(int_expr) => {
            let text = int_expr.value.text(db);
            super::literal::parse_int_const(text, ir_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
        }

        ExprFunKind::Float(float_expr) => {
            let text = float_expr.value.text(db);
            super::literal::parse_float_const(text, ir_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
        }

        ExprFunKind::String(s) => {
            Ok(ConstValue::String(s.value.as_str(db).to_string()))
        }

        ExprFunKind::Name(name) => {
            let name_str = name.text(db);
            if let Some((_, value)) = resolved_consts.get(name_str) {
                Ok(value.clone())
            } else {
                Err(LowerError::NotImplemented(format!(
                    "non-const variable '{}' in const expression",
                    name_str
                )))
            }
        }

        _ => Err(LowerError::NotImplemented(
            "complex const expressions not supported in modules (no CTFE evaluator)".to_string()
        )),
    }
}

/// Evaluate a constant expression using the CTFE evaluator.
///
/// This handles complex expressions that can't be evaluated as simple literals.
/// Used for module-level const evaluation where we have access to the evaluator
/// outside of tracked salsa functions.
pub fn eval_const_expr_with_evaluator<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    evaluator: std::rc::Rc<std::cell::RefCell<dyn datalove_datafun_ir::CtfeEvaluator>>,
) -> Result<ConstValue, LowerError> {
    // Lower the expression to a minimal IR unit using isolated lowering.
    let unit = lower_const_expr_to_unit_standalone(db, expr, expr_types, resolved_consts)?;

    // Evaluate using the CTFE evaluator.
    evaluator.borrow_mut()
        .evaluate(&unit, ir_type)
        .map_err(|e| LowerError::NotImplemented(format!("CTFE error: {}", e)))
}

/// Try to evaluate a literal expression directly without the interpreter.
fn try_eval_literal<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Option<Result<ConstValue, LowerError>> {
    match expr.expr(ctx.db) {
        ExprFunKind::True(_) => Some(Ok(ConstValue::Bool(true))),
        ExprFunKind::False(_) => Some(Ok(ConstValue::Bool(false))),
        ExprFunKind::None(_) => Some(Ok(ConstValue::OptionNone)),

        ExprFunKind::Int(int_expr) => {
            let ir_type = ctx.expr_type(expr);
            let text = int_expr.value.text(ctx.db);
            Some(
                super::literal::parse_int_const(text, &ir_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
            )
        }

        ExprFunKind::Float(float_expr) => {
            let ir_type = ctx.expr_type(expr);
            let text = float_expr.value.text(ctx.db);
            Some(
                super::literal::parse_float_const(text, &ir_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
            )
        }

        ExprFunKind::String(s) => {
            Some(Ok(ConstValue::String(s.value.as_str(ctx.db).to_string())))
        }

        _ => None, // Not a simple literal.
    }
}

/// Lower a const expression to a minimal IrScriptUnit for execution.
///
/// Uses isolated lowering: creates a fresh `LowerCtx` with independent IR state
/// but reuses type information from the parent context. This ensures CTFE
/// expressions get the same lowering behavior as runtime code (widening, etc.).
fn lower_const_expr_to_unit<'db>(
    parent_ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<IrScriptUnit, LowerError> {
    // Create isolated LowerCtx that reuses type info but has fresh IR state.
    // We pass empty call_targets since const expressions don't support function calls yet.
    let mut isolated_ctx = LowerCtx::new(
        parent_ctx.db,
        parent_ctx.expr_types,
        // SAFETY: We're creating a temporary context. The empty slice is fine
        // because const expressions shouldn't contain function calls.
        // If they do, we'll get a panic which is appropriate.
        &EMPTY_CALL_TARGETS,
    );

    // Copy const bindings from parent so we can reference previously evaluated consts.
    isolated_ctx.const_bindings = parent_ctx.const_bindings.clone();

    // Use the real lowering pipeline.
    let result_value = super::expr::lower_expression(&mut isolated_ctx, expr)?;

    // Finish the block with a UnitEnd terminator.
    isolated_ctx.finish_block(Terminator::UnitEnd {
        result: Some(Operand::Value(result_value)),
    });

    // Renumber blocks for sequential IDs.
    isolated_ctx.renumber_blocks();

    // Extract IR into an IrScriptUnit.
    Ok(IrScriptUnit {
        blocks: isolated_ctx.body.blocks,
        value_count: isolated_ctx.body.next_value,
        slot_count: isolated_ctx.body.next_slot,
        value_types: isolated_ctx.body.value_types,
        slot_types: isolated_ctx.body.slot_types,
        tracked_values: Vec::new(), // No tracking needed for CTFE.
        tracked_slots: Vec::new(),
        unit_end_values: Vec::new(),
        unit_end_slots: Vec::new(),
        functions: Vec::new(),
        symbols: SymbolTable::new(),
        result: Some(result_value),
        exports: Vec::new(),
    })
}

/// Lower a const expression to IrScriptUnit without needing a parent LowerCtx.
///
/// Used for module-level const evaluation and Phase 2 of the memoized pipeline.
fn lower_const_expr_to_unit_standalone<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
) -> Result<IrScriptUnit, LowerError> {
    // Create a fresh LowerCtx with the provided type information.
    let mut ctx = LowerCtx::new(
        db,
        expr_types,
        &EMPTY_CALL_TARGETS,
    );

    // Pre-populate const bindings from previously resolved values.
    ctx.const_bindings = resolved_consts.clone();

    // Use the real lowering pipeline.
    let result_value = super::expr::lower_expression(&mut ctx, expr)?;

    // Finish the block with a UnitEnd terminator.
    ctx.finish_block(Terminator::UnitEnd {
        result: Some(Operand::Value(result_value)),
    });

    // Renumber blocks for sequential IDs.
    ctx.renumber_blocks();

    // Extract IR into an IrScriptUnit.
    Ok(IrScriptUnit {
        blocks: ctx.body.blocks,
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        value_types: ctx.body.value_types,
        slot_types: ctx.body.slot_types,
        tracked_values: Vec::new(),
        tracked_slots: Vec::new(),
        unit_end_values: Vec::new(),
        unit_end_slots: Vec::new(),
        functions: Vec::new(),
        symbols: SymbolTable::new(),
        result: Some(result_value),
        exports: Vec::new(),
    })
}

// ============================================================================
// Phase 2: Evaluate Consts (Not Memoized)
// ============================================================================

use std::cell::RefCell;
use std::rc::Rc;
use salsa::plumbing::AsId;
use datalove_datafun_ast::ast::Statement;
use datalove_datafun_ir::{
    ConstBindingGraph, ResolvedConsts,
    ConstEvalError, CtfeEvaluator, CtfeError,
};
use datalove_datafun_tycheck::Type;

/// Evaluate all const bindings from a ConstBindingGraph.
///
/// This is Phase 2 of the 3-phase CTFE memoization pipeline.
/// NOT memoized because it requires a trait object (CtfeEvaluator).
///
/// Evaluates consts in topological order (dependencies before dependents).
/// Simple literals are extracted directly; complex expressions use the evaluator.
pub fn evaluate_consts<'db>(
    db: &'db dyn salsa::Database,
    graph: &ConstBindingGraph,
    statements: &[Statement<'db>],
    expr_types: &'db [Option<Type<'db>>],
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<ResolvedConsts, ConstEvalError> {
    let mut resolved = ResolvedConsts::new();

    for binding in &graph.bindings {
        // Find the expression for this binding.
        let expr = statements.iter()
            .find_map(|s| match s {
                Statement::Const(c) if c.value.as_id() == binding.stmt_id => Some(c.value),
                _ => None,
            });

        let expr = match expr {
            Some(e) => e,
            None => {
                // This shouldn't happen if the graph was built correctly.
                panic!("const binding expression not found: {}", binding.name);
            }
        };

        // Try simple literal extraction first.
        if let Some(value) = try_extract_literal(db, expr, &binding.ir_type) {
            resolved.insert(binding.stmt_id, binding.name.clone(), value);
            continue;
        }

        // Check for const references - substitute with already-evaluated values.
        if let ExprFunKind::Name(name) = expr.expr(db) {
            let name_str = name.text(db);
            if let Some(value) = resolved.get_by_name(name_str) {
                resolved.insert(binding.stmt_id, binding.name.clone(), value.clone());
                continue;
            }
            // Not a const reference we've evaluated - fall through to CTFE.
        }

        // Build resolved_consts HashMap for the lowering context.
        let resolved_consts_map: HashMap<String, (IrType, ConstValue)> = resolved
            .iter()
            .map(|(name, value)| (name.to_string(), (binding.ir_type.clone(), value.clone())))
            .collect();

        // Lower to IR unit using isolated lowering (gets widening, etc.).
        let unit = lower_const_expr_to_unit_standalone(db, expr, expr_types, &resolved_consts_map)
            .map_err(|e| ConstEvalError::LoweringFailed {
                binding_name: binding.name.clone(),
                message: e.to_string(),
            })?;

        let value = evaluator.borrow_mut()
            .evaluate(&unit, &binding.ir_type)
            .map_err(|e| match e {
                CtfeError::InterpError(msg) if msg.contains("gas") => {
                    ConstEvalError::GasExpired { binding_name: binding.name.clone() }
                }
                _ => {
                    // Other interpreter errors are unexpected.
                    panic!("unexpected CTFE error evaluating const '{}': {}", binding.name, e);
                }
            })?;

        resolved.insert(binding.stmt_id, binding.name.clone(), value);
    }

    Ok(resolved)
}

/// Try to extract a literal value directly without interpreter.
fn try_extract_literal<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
) -> Option<ConstValue> {
    match expr.expr(db) {
        ExprFunKind::True(_) => Some(ConstValue::Bool(true)),
        ExprFunKind::False(_) => Some(ConstValue::Bool(false)),
        ExprFunKind::None(_) => Some(ConstValue::OptionNone),

        ExprFunKind::Int(int_expr) => {
            let text = int_expr.value.text(db);
            super::literal::parse_int_const(text, ir_type).ok()
        }

        ExprFunKind::Float(float_expr) => {
            let text = float_expr.value.text(db);
            super::literal::parse_float_const(text, ir_type).ok()
        }

        ExprFunKind::String(s) => {
            Some(ConstValue::String(s.value.as_str(db).to_string()))
        }

        _ => None,
    }
}
