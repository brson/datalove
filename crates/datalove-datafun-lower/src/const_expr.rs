//! Compile-time constant expression lowering.
//!
//! This module provides lowering of const expressions to IR. The actual
//! evaluation of const expressions is handled by the `datalove-datafun-const`
//! crate, keeping lowering and evaluation as separate concerns.
//!
//! The "lower then evaluate" pattern:
//! 1. Caller uses `lower_const_binding` to lower a const expression to IR
//! 2. Caller passes the IR to `evaluate_prepared_const` in the const crate
//!
//! This module also provides `eval_const_expr` for inline evaluation during
//! lowering when a CTFE evaluator is available in the LowerCtx.

use std::collections::HashMap;
use bct::module_graph::ModuleId;
use datalove_datafun_ast::ast::{ExprFun, ExprFunKind};
use datalove_datafun_ir::{
    ConstValue, IrType, IrCodeUnit, CodeUnitId, CodeUnitContext, ScriptContext, IrModuleId,
    Operand, Terminator,
};
use datalove_datafun_common::Type;
use datalove_datafun_sema::ResolvedCallTarget;
use super::context::LowerCtx;
use super::LowerError;

// Empty arrays for isolated contexts that don't need call resolution.
static EMPTY_CALL_TARGETS: Vec<Option<ResolvedCallTarget<'static>>> = Vec::new();

/// Evaluate a constant expression at compile time using the LowerCtx's evaluator.
///
/// This is the inline evaluation path used for function-level consts.
/// Requires a CTFE evaluator to be configured in the context.
pub fn eval_const_expr<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ConstValue, LowerError> {
    // For const references, look up the previously computed value.
    if let ExprFunKind::Place(ref place) = expr.expr(ctx.db) {
        if place.steps.is_empty() {
            let name_str = place.root.text(ctx.db);
            if let Some((_, value)) = ctx.lookup_const(name_str) {
                return Ok(value.clone());
            }
            return Err(LowerError::NotImplemented(format!(
                "non-const variable '{}' in const expression",
                name_str
            )));
        }
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

/// Lower a const expression to a minimal IrCodeUnit for execution.
///
/// Uses isolated lowering: creates a fresh `LowerCtx` with independent IR state
/// but reuses type information from the parent context.
fn lower_const_expr_to_unit<'db>(
    parent_ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<IrCodeUnit, LowerError> {
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

    // Extract IR into an IrCodeUnit.
    Ok(IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks: isolated_ctx.body.blocks,
        value_count: isolated_ctx.body.next_value,
        slot_count: isolated_ctx.body.next_slot,
        call_site_count: isolated_ctx.body.next_call_site,
        value_types: isolated_ctx.body.value_types,
        slot_types: isolated_ctx.body.slot_types,
        tracked_slots: Vec::new(),
        const_values: Vec::new(),
        symbols: isolated_ctx.symbols,
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            result: Some(result_value),
            exports: Vec::new(),
        }),
        nested_units: Vec::new(),
    })
}

/// Lower a const expression to IrCodeUnit without needing a parent LowerCtx.
///
/// Used for module-level const evaluation. This is the public entry point for
/// the "lower then evaluate" pattern where callers first lower const expressions,
/// then pass the resulting IR units to the const crate for evaluation.
///
/// The `lowered_functions` parameter provides already-lowered function IR for CTFE calls.
/// The `module_func_id_map` parameter enables cross-module function calls in const expressions.
pub fn lower_const_expr_to_unit_standalone<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    expr_types: &'db [Option<Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    return_type: Option<IrType>,
    lowered_functions: &[IrCodeUnit],
    func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    module_func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, datalove_datafun_ir::FuncId)>,
) -> Result<IrCodeUnit, LowerError> {
    use datalove_datafun_ir::{CodeRef, FuncId, Instruction};
    use std::collections::HashSet;

    // Create a fresh LowerCtx with the provided type information and module func_id_map.
    let mut ctx = LowerCtx::new_for_module(
        db,
        expr_types,
        call_targets,
        module_func_id_map,
    );

    // Pre-populate const bindings from previously resolved values.
    ctx.const_bindings = resolved_consts.clone();

    // Set return type for early-return operators (? and !).
    ctx.return_type = return_type;

    // Mark as script unit so early-return uses UnitEarlyReturn terminator.
    ctx.is_script_unit = true;

    // Pre-register all functions using the name-to-id map so call resolution works.
    for (func_name, &func_id) in func_name_to_id {
        // Find the param count from the lowered function.
        if let Some(unit) = lowered_functions.iter().find(|f| f.id.0 == func_id.0) {
            let param_count = unit.function_context().map(|c| c.params.len()).unwrap_or(0);
            ctx.register_func_with_id(func_name, param_count, func_id);
        }
    }

    // Use the real lowering pipeline.
    let result_value = super::expr::lower_expression(&mut ctx, expr)?;

    // Finish the block with a UnitEnd terminator.
    ctx.finish_block(Terminator::UnitEnd {
        result: Some(Operand::Value(result_value)),
    });

    // Renumber blocks for sequential IDs.
    ctx.renumber_blocks();

    // Collect called local functions from the lowered blocks.
    let mut called_func_ids: HashSet<FuncId> = HashSet::new();
    for block in &ctx.body.blocks {
        for instr in &block.instructions {
            if let Instruction::Call { func, .. } = instr {
                if let CodeRef::Local(id) = func {
                    called_func_ids.insert(FuncId(id.0));
                }
            }
        }
    }

    // Include called functions from the lowered set (no re-lowering needed).
    let mut nested_units: Vec<IrCodeUnit> = Vec::new();
    for func_id in called_func_ids {
        // Find the function in lowered functions.
        if let Some(unit) = lowered_functions.iter().find(|f| f.id.0 == func_id.0) {
            nested_units.push(unit.clone());
        }
    }

    // Extract IR into an IrCodeUnit.
    Ok(IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks: ctx.body.blocks,
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        call_site_count: ctx.body.next_call_site,
        value_types: ctx.body.value_types,
        slot_types: ctx.body.slot_types,
        tracked_slots: Vec::new(),
        const_values: Vec::new(),
        symbols: ctx.symbols,
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            result: Some(result_value),
            exports: Vec::new(),
        }),
        nested_units,
    })
}

/// Empty module func_id_map for contexts that don't need cross-module function calls.
static EMPTY_MODULE_FUNC_ID_MAP: std::sync::LazyLock<HashMap<(ModuleId, String), (IrModuleId, datalove_datafun_ir::FuncId)>> =
    std::sync::LazyLock::new(HashMap::new);

/// Lower a single const binding, returning either a simple value or an IR unit.
///
/// This is the primary entry point for the "lower then evaluate" pattern.
/// Callers should:
/// 1. Call this function for each const binding
/// 2. Pass the results to `evaluate_prepared_const` in the const crate
///
/// Returns `Ok((None, Some(value)))` for simple literals that don't need interpretation.
/// Returns `Ok((Some(unit), None))` for complex expressions that need CTFE.
///
/// The `resolved_consts` parameter contains previously evaluated consts that this
/// expression may reference.
/// The `module_func_id_map` parameter enables cross-module function calls in const expressions.
pub fn lower_const_binding<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    expr_types: &'db [Option<Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    return_type: Option<IrType>,
    lowered_functions: &[IrCodeUnit],
    func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    module_func_id_map: Option<&'db HashMap<(ModuleId, String), (IrModuleId, datalove_datafun_ir::FuncId)>>,
) -> Result<(Option<IrCodeUnit>, Option<ConstValue>), LowerError> {
    // Try simple literal extraction first.
    if let Some(value) = try_extract_literal(db, expr, ir_type) {
        return Ok((None, Some(value)));
    }

    // Check for const references - substitute with already-evaluated values.
    if let ExprFunKind::Place(ref place) = expr.expr(db) {
        if place.steps.is_empty() {
            let name_str = place.root.text(db);
            if let Some((_, value)) = resolved_consts.get(name_str) {
                return Ok((None, Some(value.clone())));
            }
        }
    }

    // Use provided module func_id_map or empty one.
    let func_id_map = module_func_id_map.unwrap_or(&EMPTY_MODULE_FUNC_ID_MAP);

    // Lower to IR unit for CTFE evaluation.
    let unit = lower_const_expr_to_unit_standalone(
        db, expr, expr_types, call_targets, resolved_consts, return_type,
        lowered_functions, func_name_to_id, func_id_map,
    )?;

    Ok((Some(unit), None))
}

/// Try to extract a literal value directly without interpreter.
///
/// Returns `Some(value)` for simple literals (bool, int, float, string, none).
/// Returns `None` for complex expressions that need lowering and CTFE.
pub fn try_extract_literal<'db>(
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
