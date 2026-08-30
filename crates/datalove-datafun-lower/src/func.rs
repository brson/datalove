//! Function lowering.
//!
//! Two entry points:
//! - [`lower_function_for_module`]: Top-level entry for module functions (`.dlm` files).
//!   Creates a fresh `LowerCtx` and calls `lower_function_body`.
//! - [`lower_function_body`]: Shared implementation used by both module and script
//!   function lowering. For script functions, called after `swap_body_state`.

use std::collections::HashMap;
use bct::module_graph::ModuleId;
use datalove_datafun_ast::ast;
use datalove_datafun_ir::{
    IrType, IrCodeUnit, CodeUnitId, CodeUnitContext, FunctionContext,
    Operand, FuncId, IrModuleId, Terminator, ParamMode, ParamId, SymbolTable, ConstValue,
};
use datalove_datafun_sema::FunctionAnalysis;
use super::context::LowerCtx;
use super::stmt::lower_statement;
use super::LowerError;
use datalove_datafun_sema::{ExprTypes, CallTargets};

/// Lower a function to IR with available module functions in scope.
///
/// The `func_id` parameter is the pre-assigned module-local function ID.
/// Caller must run `ownership_analysis::analyze_function` first, check for errors,
/// and pass the result here. This function asserts that `analysis` has no errors.
///
/// If `resolved_param_types` is provided, use those types for parameters instead of
/// deriving from AST type hints. This is necessary when type aliases are used.
/// Similarly, `resolved_return_type` provides the resolved return type.
///
/// If `ctfe_evaluator` is provided, it will be used to evaluate complex const
/// expressions at compile time.
///
/// If `module_consts` is provided, those pre-evaluated const bindings will be
/// available for use within the function body.
pub fn lower_function_for_module<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db ExprTypes<'db>,
    call_targets: &'db CallTargets<'db>,
    func_id_map: &'db HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>,
    func: ast::StmtFun<'db>,
    func_id: FuncId,
    analysis: FunctionAnalysis<'db>,
    resolved_param_types: Option<&[IrType]>,
    resolved_return_type: Option<IrType>,
    module_consts: &HashMap<String, (IrType, ConstValue)>,
) -> Result<IrCodeUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_module(db, expr_types, Some(call_targets), Some(func_id_map));
    // Module-level consts are in scope for the whole module. Seeding them here
    // puts them on the same path as an already-evaluated function-level const,
    // so a reference emits a fresh Const and is dropped as an expression
    // temporary, with no separate substitution step.
    for (name, (ir_type, value)) in module_consts {
        ctx.add_const(name.clone(), ir_type.clone(), value.clone());
    }
    lower_function_body(&mut ctx, func_id, func, analysis, resolved_param_types, resolved_return_type)
}

/// Lower a function body given an already-allocated FuncId and pre-computed drop analysis.
///
/// The caller must ensure `analysis` has no errors before calling this function.
///
/// If `resolved_param_types` is provided, use those types for parameters instead of
/// deriving from AST type hints. This is necessary when type aliases are used.
/// Similarly, `resolved_return_type` provides the resolved return type.
pub fn lower_function_body<'db>(
    ctx: &mut LowerCtx<'db>,
    func_id: FuncId,
    func: ast::StmtFun<'db>,
    analysis: FunctionAnalysis<'db>,
    resolved_param_types: Option<&[IrType]>,
    resolved_return_type: Option<IrType>,
) -> Result<IrCodeUnit, LowerError> {
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
    ctx.body.adapt_sites = analysis.adapt_sites;

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

    // Set return type from resolved type if available, otherwise from AST type hint.
    ctx.return_type = match resolved_return_type {
        Some(ty) => Some(ty),
        None => func.return_type(ctx.db).map(|ty| IrType::from_type_hint(ctx.db, &ty)),
    };

    // Allocate ParamIds for parameters with correct types and modes.
    // Record binding operands to match analysis order.
    let mut params: Vec<ParamId> = Vec::new();
    let mut param_modes = Vec::new();
    // A borrowed parameter whose type mentions one of this function's type
    // parameters is not converted at the boundary, so this function's own type
    // for it does not describe what arrives. The caller supplies the
    // descriptor; see `FunctionContext::descriptor_params`.
    let mut descriptor_params: Vec<ParamId> = Vec::new();
    let type_params = func.type_params(ctx.db).clone();
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

        let borrowed = matches!(mode, ParamMode::Ref | ParamMode::Mut);
        if borrowed
            && datalove_datafun_ir::type_hint_mentions_param(&p.type_hint, &type_params)
        {
            descriptor_params.push(id);
        }
    }

    // Lower the function body with statement indices.
    let body = func.body(ctx.db);
    for (idx, stmt) in body.iter().enumerate() {
        ctx.body.current_stmt_idx = Some(idx);
        lower_statement(ctx, stmt)?;
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

    Ok(IrCodeUnit {
        id: CodeUnitId(func_id.0),
        name,
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        call_site_count: ctx.body.next_call_site,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots: ctx.compute_tracked_slots(),
        const_values: std::mem::take(&mut ctx.body.const_values),
        symbols: SymbolTable::new(),
        context: CodeUnitContext::Function(FunctionContext {
            params,
            param_modes,
            param_types: std::mem::take(&mut ctx.body.param_types),
            return_type,
            tracked_params: ctx.compute_tracked_params(),
            descriptor_params,
        }),
        nested_units: Vec::new(),
    })
}

