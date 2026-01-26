//! Compile-time constant expression evaluation.
//!
//! Evaluates const expressions at compile time via the 3-phase CTFE pipeline:
//! 1. Collect const bindings into ConstBindingGraph (Phase 1, memoized)
//! 2. Evaluate using CtfeEvaluator (Phase 2, not memoized)
//! 3. Use pre-resolved values during lowering (Phase 3, memoized)
//!
//! This module uses "isolated lowering" - creating a fresh `LowerCtx` that reuses
//! type information but has independent IR state. This ensures CTFE gets all the
//! same lowering behavior as runtime code (widening, checked ops, etc.).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use salsa::plumbing::AsId;
use datalove_datafun_ast::ast::{ExprFun, ExprFunKind, Statement};
use datalove_datafun_ir::{
    ConstValue, IrType, IrScriptUnit, ConstBindingGraph, ResolvedConsts,
    ConstEvalError, CtfeEvaluator, CtfeError,
    Operand, Terminator, SymbolTable,
};
use datalove_datafun_tycheck::Type;
use crate::ir_ext::IrTypeExt;
use super::context::LowerCtx;
use super::LowerError;

// Empty arrays for isolated contexts that don't need call resolution.
static EMPTY_CALL_TARGETS: Vec<Option<datalove_datafun_tycheck::ResolvedCallTarget<'static>>> = Vec::new();

/// Evaluate a constant expression at compile time using the LowerCtx's evaluator.
///
/// This is the inline evaluation path used for function-level consts.
/// Requires a CTFE evaluator to be configured in the context.
pub fn eval_const_expr<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ConstValue, LowerError> {
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

    // Lower to IR and use the evaluator.
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

/// Lower a const expression to a minimal IrScriptUnit for execution.
///
/// Uses isolated lowering: creates a fresh `LowerCtx` with independent IR state
/// but reuses type information from the parent context.
fn lower_const_expr_to_unit<'db>(
    parent_ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<IrScriptUnit, LowerError> {
    // Create isolated LowerCtx that reuses type info but has fresh IR state.
    let mut isolated_ctx = LowerCtx::new(
        parent_ctx.db,
        parent_ctx.expr_types,
        &EMPTY_CALL_TARGETS,
    );

    // Copy const bindings from parent so we can reference previously evaluated consts.
    isolated_ctx.const_bindings = parent_ctx.const_bindings.clone();

    // Copy return type from parent for early-return operators (? and !).
    isolated_ctx.return_type = parent_ctx.return_type.clone();

    // Mark as script unit so early-return uses UnitEarlyReturn terminator.
    isolated_ctx.is_script_unit = true;

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
        tracked_slots: Vec::new(),
        unit_end_values: Vec::new(),
        unit_end_slots: Vec::new(),
        functions: Vec::new(),
        symbols: SymbolTable::new(),
        result: Some(result_value),
        exports: Vec::new(),
    })
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
///
/// The `return_type` is the enclosing function's return type, needed for
/// early-return operators (`?` and `!`).
pub fn eval_const_expr_with_evaluator<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    expr_types: &'db [Option<Type<'db>>],
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    return_type: Option<IrType>,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<ConstValue, LowerError> {
    // Lower the expression to a minimal IR unit using isolated lowering.
    let unit = lower_const_expr_to_unit_standalone(db, expr, expr_types, resolved_consts, return_type)?;

    // Evaluate using the CTFE evaluator.
    evaluator.borrow_mut()
        .evaluate(&unit, ir_type)
        .map_err(|e| LowerError::NotImplemented(format!("CTFE error: {}", e)))
}

/// Lower a const expression to IrScriptUnit without needing a parent LowerCtx.
///
/// Used for module-level const evaluation and Phase 2 of the memoized pipeline.
fn lower_const_expr_to_unit_standalone<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    expr_types: &'db [Option<Type<'db>>],
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    return_type: Option<IrType>,
) -> Result<IrScriptUnit, LowerError> {
    // Create a fresh LowerCtx with the provided type information.
    let mut ctx = LowerCtx::new(
        db,
        expr_types,
        &EMPTY_CALL_TARGETS,
    );

    // Pre-populate const bindings from previously resolved values.
    ctx.const_bindings = resolved_consts.clone();

    // Set return type for early-return operators (? and !).
    ctx.return_type = return_type;

    // Mark as script unit so early-return uses UnitEarlyReturn terminator.
    ctx.is_script_unit = true;

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
        // Use the type from the graph for each binding, not the current binding's type.
        let resolved_consts_map: HashMap<String, (IrType, ConstValue)> = resolved
            .iter()
            .filter_map(|(name, value)| {
                // Look up the type for this const from the graph.
                let const_type = graph.bindings.iter()
                    .find(|b| b.name == name)
                    .map(|b| b.ir_type.clone())?;
                Some((name.to_string(), (const_type, value.clone())))
            })
            .collect();

        // Lower to IR unit using isolated lowering (gets widening, etc.).
        // Script-level consts don't have a function return type.
        let unit = lower_const_expr_to_unit_standalone(db, expr, expr_types, &resolved_consts_map, None)
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

/// Evaluate function-level consts in script functions.
///
/// This extends Phase 2 to also evaluate consts defined inside function bodies.
/// Returns a map of qualified names (`func_name::const_name`) to values,
/// along with any errors encountered.
///
/// Function-level consts can reference:
/// - Script-level consts (from `script_level_consts`)
/// - Earlier function-level consts in the same function
pub fn evaluate_script_function_consts<'db>(
    db: &'db dyn salsa::Database,
    statements: &[Statement<'db>],
    expr_types: &'db [Option<Type<'db>>],
    script_level_consts: &HashMap<String, (IrType, ConstValue)>,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> ScriptFunctionConstsResult {
    let mut consts = HashMap::new();
    let mut errors = Vec::new();

    for statement in statements {
        if let Statement::Fun(func_stmt) = statement {
            let func_name = func_stmt.name(db).text(db);
            // Get function's return type for early-return operators.
            let func_return_type = func_stmt.return_type(db)
                .map(|ty| IrType::from_type_hint(db, &ty));
            // Track local consts for this function.
            let mut func_local_consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();

            for func_body_stmt in func_stmt.body(db).iter() {
                if let Statement::Const(const_stmt) = func_body_stmt {
                    let name = const_stmt.name.text(db).to_string();
                    let init_expr = const_stmt.value;

                    // Get the type from the typechecker.
                    let expr_id = init_expr.as_id();
                    let index = expr_id.index() as usize;
                    let ir_type = match expr_types.get(index).cloned().flatten() {
                        Some(ty) => IrType::from_tycheck(db, &ty),
                        None => {
                            errors.push(format!("{}::{}: missing type information", func_name, name));
                            continue;
                        }
                    };

                    // Build lookup map: script-level + function-local consts.
                    let mut lookup_map = script_level_consts.clone();
                    for (local_name, (ty, val)) in &func_local_consts {
                        lookup_map.insert(local_name.clone(), (ty.clone(), val.clone()));
                    }

                    // Try simple evaluation first.
                    let value = match eval_const_expr_simple(db, init_expr, &ir_type, &lookup_map) {
                        Ok(v) => v,
                        Err(_) => {
                            // Fall back to CTFE evaluator for complex expressions.
                            match eval_const_expr_with_evaluator(
                                db,
                                init_expr,
                                &ir_type,
                                expr_types,
                                &lookup_map,
                                func_return_type.clone(),
                                evaluator.clone(),
                            ) {
                                Ok(v) => v,
                                Err(e) => {
                                    errors.push(format!("{}::{}: {}", func_name, name, e));
                                    continue;
                                }
                            }
                        }
                    };

                    // Store locally for other consts in this function.
                    func_local_consts.insert(name.clone(), (ir_type.clone(), value.clone()));

                    // Store with qualified name for the result.
                    let qualified_name = format!("{}::{}", func_name, name);
                    consts.insert(qualified_name, (ir_type, value));
                }
            }
        }
    }

    ScriptFunctionConstsResult { consts, errors }
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
