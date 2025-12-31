//! Function lowering.
//!
//! Handles lowering of function definitions to IR.

use crate::ast;
use crate::tycheck::TypecheckResult;
use crate::Db;
use super::super::{IrType, IrFunction, Operand, FuncId, Terminator};
use super::context::LowerCtx;
use super::scope::ScopeKind;
use super::stmt::lower_statement;
use super::LowerError;

/// Lower a function to IR.
///
/// For standalone function lowering (not in a script context).
pub fn lower_function<'db>(
    db: &'db dyn Db,
    tycheck_result: TypecheckResult<'db>,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    lower_function_with_expr_types(db, tycheck_result.expr_types(db), func)
}

/// Lower a function to IR using pre-computed expr_types.
///
/// This variant is useful when lowering functions from a module graph
/// where expr_types are combined across all modules.
pub fn lower_function_with_expr_types<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    lower_function_for_module(db, expr_types, &[], func)
}

/// Lower a function to IR with available module functions in scope.
///
/// This variant is used when lowering module functions that may call
/// other module functions (imported from other modules).
pub fn lower_function_for_module<'db>(
    db: &'db dyn Db,
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    available_functions: &[String],
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let mut ctx = LowerCtx::new_for_module(db, expr_types, available_functions);
    let name = func.name(db).text(db).to_string();
    let param_count = func.params(db).len();

    // Define the function in the symbol table.
    let func_id = ctx.define_func(&name, param_count);

    lower_function_body(&mut ctx, func_id, func)
}

/// Lower a function body given an already-allocated FuncId.
pub fn lower_function_body<'db>(
    ctx: &mut LowerCtx<'db>,
    func_id: FuncId,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let name = func.name(ctx.db).text(ctx.db).to_string();

    // Save and set function context for try operators.
    let saved_return_type = ctx.return_type.take();
    let saved_is_script_unit = ctx.is_script_unit;
    ctx.is_script_unit = false;

    // Set return type from function signature.
    ctx.return_type = func.return_type(ctx.db).map(|ty| IrType::from_type_hint(ctx.db, &ty));

    // Enter function scope for drop tracking.
    ctx.scope_tracker.enter_scope(ScopeKind::Function);

    // Allocate ValueIds for parameters with correct types.
    let params: Vec<_> = func.params(ctx.db)
        .iter()
        .map(|p| {
            let param_name = p.name(ctx.db).text(ctx.db).to_string();
            let param_type = IrType::from_type_hint(ctx.db, &p.type_hint(ctx.db));
            let id = ctx.fresh_value(param_type.clone());
            ctx.bind_var(&param_name, Operand::Value(id));
            // Record parameter for drop tracking.
            ctx.scope_tracker.record_binding(Operand::Value(id), param_type);
            id
        })
        .collect();

    // Lower the function body.
    for stmt in func.body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // If no explicit return, add implicit return unit.
    // Emit drops before the implicit return.
    if ctx.current_instructions.is_empty()
        || !matches!(ctx.blocks.last().map(|b| &b.terminator), Some(Terminator::Return { .. }))
    {
        // Check if we already have a return as the last instruction.
        let needs_return = ctx.blocks.is_empty()
            || !matches!(ctx.blocks.last().unwrap().terminator, Terminator::Return { .. });
        if needs_return {
            // Emit drops before implicit return.
            let drops = ctx.scope_tracker.exit_scope();
            ctx.emit_drops(drops);
            ctx.finish_block(Terminator::Return { value: None });
        }
    } else {
        // Scope already exited by explicit return, just pop it.
        ctx.scope_tracker.scopes.pop();
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
