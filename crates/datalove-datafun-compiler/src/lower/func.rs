//! Function lowering.
//!
//! Two entry points:
//! - [`lower_function_for_module`]: Top-level entry for module functions (`.dlm` files).
//!   Creates a fresh `LowerCtx` and calls `lower_function_body`.
//! - [`lower_function_body`]: Shared implementation used by both module and script
//!   function lowering. For script functions, called after `swap_body_state`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use datalove_datafun_ast::ast;
use crate::module_graph::ModuleId;
use datalove_datafun_tycheck::ResolvedCallTarget;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, IrType, IrFunction, Operand, FuncId, IrModuleId, Terminator, ParamMode, ParamId};
use crate::ownership_analysis::FunctionAnalysis;
use super::context::LowerCtx;
use super::stmt::lower_statement_indexed;
use super::LowerError;

/// Lower a function to IR with available module functions in scope.
///
/// The `func_id` parameter is the pre-assigned module-local function ID.
/// Caller must run `ownership_analysis::analyze_function` first, check for errors,
/// and pass the result here. This function asserts that `analysis` has no errors.
///
/// If `resolved_param_types` is provided, use those types for parameters instead of
/// deriving from AST type hints. This is necessary when type aliases are used.
///
/// If `ctfe_evaluator` is provided, it will be used to evaluate complex const
/// expressions at compile time.
///
/// If `module_consts` is provided, those pre-evaluated const bindings will be
/// available for use within the function body.
///
/// If `const_as_let` is true, const bindings in the function body are lowered as
/// let bindings instead of being evaluated at compile time.
pub fn lower_function_for_module<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    func: ast::StmtFun<'db>,
    func_id: FuncId,
    analysis: FunctionAnalysis,
    resolved_param_types: Option<&[IrType]>,
    ctfe_evaluator: Option<Rc<RefCell<dyn CtfeEvaluator>>>,
    module_consts: Option<&HashMap<String, (IrType, ConstValue)>>,
    const_as_let: bool,
) -> Result<IrFunction, LowerError> {
    let mut ctx = LowerCtx::new_for_module(db, expr_types, call_targets, func_id_map);

    // Set up const_as_let mode if enabled.
    ctx.set_const_as_let(const_as_let);

    // Set up CTFE evaluator if provided.
    if let Some(evaluator) = ctfe_evaluator {
        ctx.set_ctfe_evaluator(evaluator);
    }

    // Pre-populate const bindings from module-level and function-level consts.
    // In const_as_let mode, skip function-level consts (they'll be lowered as let bindings).
    if let Some(consts) = module_consts {
        let func_name = func.name(db).text(db);
        let prefix = format!("{}::", func_name);

        for (name, (ir_type, value)) in consts {
            // Check if this is a function-level const for this function.
            if let Some(local_name) = name.strip_prefix(&prefix) {
                // Function-level const: add as local name.
                // Skip if const_as_let is enabled - consts will be lowered as let bindings.
                if !const_as_let {
                    ctx.add_const(local_name.to_string(), ir_type.clone(), value.clone());
                }
            } else if !name.contains("::") {
                // Module-level const: add as-is (always, even in const_as_let mode).
                ctx.add_const(name.clone(), ir_type.clone(), value.clone());
            }
            // Skip function-level consts from other functions.
        }
    }

    lower_function_body(&mut ctx, func_id, func, analysis, resolved_param_types)
}

/// Lower a function body given an already-allocated FuncId and pre-computed drop analysis.
///
/// The caller must ensure `analysis` has no errors before calling this function.
///
/// If `resolved_param_types` is provided, use those types for parameters instead of
/// deriving from AST type hints. This is necessary when type aliases are used.
pub fn lower_function_body<'db>(
    ctx: &mut LowerCtx<'db>,
    func_id: FuncId,
    func: ast::StmtFun<'db>,
    analysis: FunctionAnalysis,
    resolved_param_types: Option<&[IrType]>,
) -> Result<IrFunction, LowerError> {
    // Assert no analysis errors - caller should have checked.
    assert!(
        analysis.errors.is_empty(),
        "lower_function_body called with analysis errors: {:?}",
        analysis.errors
    );

    let name = func.name(ctx.db).text(ctx.db).to_string();

    // Set drop schedule and tracking for this function.
    ctx.body.drop_schedule = analysis.schedule;
    ctx.body.binding_info = analysis.bindings;
    ctx.body.tracking = analysis.tracking;

    // Reset counters - each function has its own statement/binding ID space.
    // Note: For nested functions, these are already 0 from swap_body_state(),
    // but we reset explicitly for module functions using the same ctx.
    ctx.body.next_stmt_id = 0;
    ctx.body.next_binding_id = 0;
    ctx.body.binding_to_operand.clear();

    // Save and set function context for try operators.
    let saved_return_type = ctx.return_type.take();
    let saved_is_script_unit = ctx.is_script_unit;
    ctx.is_script_unit = false;

    // Set return type from function signature.
    ctx.return_type = func.return_type(ctx.db).map(|ty| IrType::from_type_hint(ctx.db, &ty));

    // Allocate ParamIds for parameters with correct types and modes.
    // Record binding operands to match analysis order.
    let mut params: Vec<ParamId> = Vec::new();
    let mut param_modes = Vec::new();
    let func_params = func.params(ctx.db);
    for (i, p) in func_params.iter().enumerate() {
        let param_name = p.name.text(ctx.db).to_string();
        // Use resolved type if available, otherwise fall back to AST type hint.
        let param_type = match resolved_param_types {
            Some(types) => types[i].clone(),
            None => IrType::from_type_hint(ctx.db, &p.type_hint),
        };
        let mode = match p.mode {
            ast::ParamMode::In => ParamMode::In,
            ast::ParamMode::Out => ParamMode::Out,
            ast::ParamMode::Ref => ParamMode::Ref,
            ast::ParamMode::Mut => ParamMode::Mut,
        };
        let id = ctx.fresh_param(param_type.clone(), mode);
        let operand = Operand::Param(id);
        ctx.bind_var(&param_name, operand);
        // Record binding operand for drop schedule.
        ctx.record_binding_operand(operand);
        params.push(id);
        param_modes.push(mode);
    }

    // Lower the function body with statement indices.
    let body = func.body(ctx.db);
    for (idx, stmt) in body.iter().enumerate() {
        ctx.body.current_stmt_idx = Some(idx);
        lower_statement_indexed(ctx, stmt, idx)?;
    }
    ctx.body.current_stmt_idx = None;

    // If no explicit return, add implicit return unit.
    // Drop analysis schedules drops at function scope exit.
    let needs_return = ctx.body.blocks.is_empty()
        || !matches!(ctx.body.blocks.last().unwrap().terminator, Terminator::Return { .. });
    if needs_return {
        ctx.finish_block(Terminator::Return { value: None });
    }

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    // Get return type from context (set from function signature) before restoring.
    let return_type = ctx.return_type.clone().unwrap_or(IrType::Unit);

    // Restore saved context.
    ctx.return_type = saved_return_type;
    ctx.is_script_unit = saved_is_script_unit;

    Ok(IrFunction {
        id: func_id,
        name,
        params,
        param_modes,
        param_types: std::mem::take(&mut ctx.body.param_types),
        return_type,
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots: ctx.compute_tracked_slots(),
        tracked_params: ctx.compute_tracked_params(),
        const_values: std::mem::take(&mut ctx.body.const_values),
    })
}

