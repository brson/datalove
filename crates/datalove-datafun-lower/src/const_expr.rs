//! Compile-time constant expression lowering.
//!
//! This module provides lowering of const expressions to IR. The actual
//! evaluation of const expressions is handled by the `datalove-datafun-const`
//! crate, keeping lowering and evaluation as separate concerns.
//!
//! The "lower then evaluate" pattern:
//! 1. Caller uses `lower_const_binding` to lower a const expression to IR
//! 2. Caller passes the IR to `evaluate_prepared_const` in the const crate

use std::collections::HashMap;
use bct::module_graph::ModuleId;
use datalove_datafun_ast::ast::{ExprFun, ExprFunKind};
use datalove_datafun_ir::{
    ConstValue, IrType, IrCodeUnit, CodeUnitId, CodeUnitContext, ScriptContext, IrModuleId,
    Operand, Terminator,
};
use super::context::{DataFiles, LowerCtx};
use super::LowerError;
use datalove_datafun_sema::{ExprTypes, CallTargets};

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
    expr_types: &'db ExprTypes<'db>,
    call_targets: &'db CallTargets<'db>,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    return_type: Option<IrType>,
    lowered_functions: &[std::sync::Arc<IrCodeUnit>],
    func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    module_func_id_map: Option<&'db HashMap<(ModuleId<'db>, String), (IrModuleId, datalove_datafun_ir::FuncId)>>,
    data_files: &'db DataFiles,
) -> Result<IrCodeUnit, LowerError> {
    use datalove_datafun_ir::{CodeRef, FuncId};
    use std::collections::HashSet;

    // Create a fresh LowerCtx with the provided type information and module func_id_map.
    let mut ctx = LowerCtx::new_for_module(
        db,
        expr_types,
        Some(call_targets),
        module_func_id_map,
        data_files,
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

    // The local functions the const can reach, from the lowered set, which
    // the unit carries beside it for the evaluator to call. Every one it
    // reaches, not only the ones it calls: a function it calls may call
    // another, and the evaluator finds that one among these too.
    let local_calls = |blocks: &[datalove_datafun_ir::IrBlock]| -> Vec<FuncId> {
        blocks.iter()
            .flat_map(|block| block.instructions.iter())
            .filter_map(|instr| match instr.call_target() {
                Some((CodeRef::Local(id), _)) => Some(FuncId(id.0)),
                _ => None,
            })
            .collect()
    };
    let mut reached: HashSet<FuncId> = HashSet::new();
    let mut nested_units: Vec<IrCodeUnit> = Vec::new();
    let mut pending = local_calls(&ctx.body.blocks);
    while let Some(func_id) = pending.pop() {
        if !reached.insert(func_id) {
            continue;
        }
        if let Some(unit) = lowered_functions.iter().find(|f| f.id.0 == func_id.0) {
            pending.extend(local_calls(&unit.blocks));
            nested_units.push((**unit).clone());
        }
    }
    nested_units.sort_by_key(|unit| unit.id.0);

    // Extract IR into an IrCodeUnit.
    Ok(IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks: ctx.body.blocks,
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        value_types: ctx.body.value_types,
        slot_types: ctx.body.slot_types,
        tracked_slots: Vec::new(),
        const_values: Vec::new(),
        symbols: ctx.symbols,
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            result: Some(result_value),
            result_name: None,
            exports: Vec::new(),
        }),
        nested_units,
    })
}

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
    expr_types: &'db ExprTypes<'db>,
    call_targets: &'db CallTargets<'db>,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    return_type: Option<IrType>,
    lowered_functions: &[std::sync::Arc<IrCodeUnit>],
    func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    module_func_id_map: Option<&'db HashMap<(ModuleId<'db>, String), (IrModuleId, datalove_datafun_ir::FuncId)>>,
    data_files: &'db DataFiles,
) -> Result<(Option<IrCodeUnit>, Option<ConstValue>), LowerError> {
    // Try simple literal extraction first.
    if let Some(value) = try_extract_literal(db, expr, ir_type, data_files) {
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
    let func_id_map = module_func_id_map;

    // Lower to IR unit for CTFE evaluation.
    let unit = lower_const_expr_to_unit_standalone(
        db, expr, expr_types, call_targets, resolved_consts, return_type,
        lowered_functions, func_name_to_id, func_id_map, data_files,
    )?;

    Ok((Some(unit), None))
}

/// Try to extract a literal value directly without interpreter.
///
/// Returns `Some(value)` for simple literals (bool, int, float, string, none)
/// and for data files, which are literals of any size.
/// Returns `None` for complex expressions that need lowering and CTFE.
pub fn try_extract_literal<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    data_files: &DataFiles,
) -> Option<ConstValue> {
    match expr.expr(db) {
        ExprFunKind::DataFile(file) => {
            let source = *data_files.get(&file.path(db))
                .expect("the typechecker found the data file");
            Some(crate::datafile::data_file_value(db, source, ir_type))
        }

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
            let raw = s.value.as_str(db);
            Some(ConstValue::String(super::literal::string_literal_value(raw)))
        }

        _ => None,
    }
}
