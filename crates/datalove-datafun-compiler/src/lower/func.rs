//! Function lowering.
//!
//! Handles lowering of function definitions to IR.

use std::collections::HashMap;
use datalove_datafun_ast::ast;
use crate::module_graph::ModuleId;
use datalove_datafun_tycheck::ResolvedCallTarget;
use datalove_datafun_ir::{IrType, IrFunction, Operand, FuncId, IrModuleId, Terminator};
use crate::drop_analysis::FunctionDropAnalysis;
use super::context::LowerCtx;
use super::stmt::lower_statement_indexed;
use super::LowerError;

/// Lower a function to IR with available module functions in scope.
///
/// Caller must run `drop_analysis::analyze_function` first, check for errors,
/// and pass the result here. This function asserts that `analysis` has no errors.
pub fn lower_function_for_module<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::TypeAndHeap<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    func: ast::StmtFun<'db>,
    analysis: FunctionDropAnalysis,
) -> Result<IrFunction, LowerError> {
    let mut ctx = LowerCtx::new_for_module(db, expr_types, call_targets, func_id_map);
    let name = func.name(db).text(db).to_string();
    let param_count = func.params(db).len();

    // Define the function in the symbol table.
    let func_id = ctx.define_func(&name, param_count);

    lower_function_body(&mut ctx, func_id, func, analysis)
}

/// Lower a function body given an already-allocated FuncId and pre-computed drop analysis.
///
/// The caller must ensure `analysis` has no errors before calling this function.
pub fn lower_function_body<'db>(
    ctx: &mut LowerCtx<'db>,
    func_id: FuncId,
    func: ast::StmtFun<'db>,
    analysis: FunctionDropAnalysis,
) -> Result<IrFunction, LowerError> {
    // Assert no analysis errors - caller should have checked.
    assert!(
        analysis.errors.is_empty(),
        "lower_function_body called with analysis errors: {:?}",
        analysis.errors
    );

    let name = func.name(ctx.db).text(ctx.db).to_string();

    // Set drop schedule for this function.
    ctx.set_drop_schedule(analysis.schedule, analysis.bindings);

    // Save and set function context for try operators.
    let saved_return_type = ctx.return_type.take();
    let saved_is_script_unit = ctx.is_script_unit;
    ctx.is_script_unit = false;

    // Set return type from function signature.
    ctx.return_type = func.return_type(ctx.db).map(|ty| IrType::from_type_hint(ctx.db, &ty));

    // Allocate ValueIds for parameters with correct types.
    // Record binding operands to match analysis order.
    let params: Vec<_> = func.params(ctx.db)
        .iter()
        .map(|p| {
            let param_name = p.name(ctx.db).text(ctx.db).to_string();
            let param_type = IrType::from_type_hint(ctx.db, &p.type_hint(ctx.db));
            let id = ctx.fresh_value(param_type.clone());
            let operand = Operand::Value(id);
            ctx.bind_var(&param_name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            id
        })
        .collect();

    // Lower the function body with statement indices.
    let body = func.body(ctx.db);
    for (idx, stmt) in body.iter().enumerate() {
        ctx.current_stmt_idx = Some(idx);
        lower_statement_indexed(ctx, stmt, idx)?;
    }
    ctx.current_stmt_idx = None;

    // If no explicit return, add implicit return unit.
    // Drop analysis schedules drops at function scope exit.
    let needs_return = ctx.blocks.is_empty()
        || !matches!(ctx.blocks.last().unwrap().terminator, Terminator::Return { .. });
    if needs_return {
        ctx.finish_block(Terminator::Return { value: None });
    }

    // Restore saved context.
    ctx.return_type = saved_return_type;
    ctx.is_script_unit = saved_is_script_unit;

    Ok(IrFunction {
        id: func_id,
        name,
        params,
        blocks: std::mem::take(&mut ctx.blocks),
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
    })
}
