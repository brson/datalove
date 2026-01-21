//! Script unit lowering.
//!
//! Handles lowering of script units (fragments and expressions).

use std::collections::HashMap;
use datalove_datafun_ast::ast::{self, Statement, ExprFun, ExprFunKind};
use crate::module_graph::ModuleId;
use datalove_datafun_tycheck::{TypecheckResult, ResolvedCallTarget};
use datalove_datafun_ir::{
    IrType, IrScriptUnit, Operand, Terminator, Instruction, ConstValue, SlotDest,
    ExportBinding, BlockId, IrModuleId, FuncId,
};
use crate::ownership_analysis::{ScriptFunctionAnalyses, analyze_script_statements, format_analysis_errors};
use crate::ir_ext::IrTypeExt;
use super::context::{LowerCtx, ScriptLowerContext, ScriptUnitKind};
use super::expr::{lower_expression, lower_expression_for_ref};
use super::func::lower_function_body;
use super::stmt::collect_field_path;
use super::LowerError;

/// Lower a script unit.
///
/// Script units are sequences of statements (fragment) or a single expression (expr).
/// They can reference values from previous units and export bindings to subsequent units.
///
/// For fragments, caller must first call `analyze_script_functions` to get `func_analyses`.
/// For expressions, pass an empty map since there are no function definitions.
///
/// When `for_aot` is true, emits Drop instructions for script-level bindings at unit end.
pub fn lower_script_unit<'db>(
    db: &'db dyn salsa::Database,
    tycheck_result: TypecheckResult<'db>,
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    kind: ScriptUnitKind<'db>,
    func_analyses: ScriptFunctionAnalyses<'db>,
    for_aot: bool,
) -> Result<IrScriptUnit, LowerError> {
    let expr_types = tycheck_result.expr_types(db);
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // Build map of function name -> resolved param types for type alias support.
    let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
    for (name, func_type) in tycheck_result.function_types(db) {
        let param_types: Vec<IrType> = func_type.param_types(db)
            .iter()
            .map(|ty| IrType::from_tycheck(db, ty))
            .collect();
        func_param_types.insert(name.text(db).to_string(), param_types);
    }

    let result = match kind {
        ScriptUnitKind::Fragment(stmts) => {
            // Analyze script statements for drop schedule.
            let script_analysis = analyze_script_statements(db, expr_types, call_targets, &stmts, for_aot);

            // Check for drop analysis errors (use-after-move, move-in-loop, etc.)
            if !script_analysis.errors.is_empty() {
                return Err(LowerError::DropAnalysisError(
                    format_analysis_errors(&script_analysis.errors)
                ));
            }

            ctx.drop_schedule = script_analysis.schedule;
            ctx.binding_info = script_analysis.bindings;
            ctx.unit_end_drops = script_analysis.unit_end;

            // Lower all statements with index tracking.
            for (idx, stmt) in stmts.iter().enumerate() {
                ctx.current_stmt_idx = Some(idx);
                lower_statement_for_script(&mut ctx, stmt, idx, &func_analyses, Some(&func_param_types))?;
            }
            ctx.current_stmt_idx = None;

            // Fragment units have no result value.
            None
        }
        ScriptUnitKind::Expr(expr) => {
            // Expression units don't have statements, no drop schedule needed.
            // Lower the expression and capture the result.
            let value_id = lower_expression(&mut ctx, expr)?;
            Some(value_id)
        }
    };

    // Emit drops for script-level bindings at unit end (for AOT).
    ctx.emit_unit_end_drops();

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: result.map(Operand::Value),
    });

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    Ok(IrScriptUnit {
        blocks: std::mem::take(&mut ctx.blocks),
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result,
        exports: ctx.exports,
    })
}

/// Lower a script fragment unit with raw expr_types.
///
/// Like `lower_script_unit` but takes expr_types directly instead of TypecheckResult.
/// Used when typechecking with context (non-salsa version).
///
/// Caller must first call `analyze_script_functions` to get `func_analyses`.
///
/// When `for_aot` is true, emits Drop instructions for script-level bindings at unit end.
///
/// If `func_param_types` is provided, use those resolved param types for function parameters
/// instead of deriving from AST type hints. This is needed for type alias support.
pub fn lower_script_fragment_raw<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    stmts: Vec<Statement<'db>>,
    func_analyses: ScriptFunctionAnalyses<'db>,
    for_aot: bool,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // Analyze script statements for drop schedule.
    let script_analysis = analyze_script_statements(db, expr_types, call_targets, &stmts, for_aot);

    // Check for drop analysis errors (use-after-move, move-in-loop, etc.)
    if !script_analysis.errors.is_empty() {
        return Err(LowerError::DropAnalysisError(
            format_analysis_errors(&script_analysis.errors)
        ));
    }

    ctx.drop_schedule = script_analysis.schedule;
    ctx.binding_info = script_analysis.bindings;
    ctx.unit_end_drops = script_analysis.unit_end;

    // Lower all statements with index tracking.
    for (idx, stmt) in stmts.iter().enumerate() {
        ctx.current_stmt_idx = Some(idx);
        lower_statement_for_script(&mut ctx, stmt, idx, &func_analyses, func_param_types)?;
    }
    ctx.current_stmt_idx = None;

    // Emit drops for script-level bindings at unit end (for AOT).
    ctx.emit_unit_end_drops();

    // Fragment units have no result value.
    ctx.finish_block(Terminator::UnitEnd { result: None });

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    Ok(IrScriptUnit {
        blocks: std::mem::take(&mut ctx.blocks),
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result: None,
        exports: ctx.exports,
    })
}

/// Lower a script expression unit.
///
/// Like `lower_script_unit` but takes expr_types directly and an expression.
///
/// The `for_aot` parameter is accepted for API consistency but has no effect
/// since expressions don't create script-level bindings that need dropping.
#[allow(unused_variables)]
pub fn lower_script_expr<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    expr: ExprFun<'db>,
    for_aot: bool,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // Lower the expression and capture the result.
    let value_id = lower_expression(&mut ctx, expr)?;

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: Some(Operand::Value(value_id)),
    });

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    Ok(IrScriptUnit {
        blocks: std::mem::take(&mut ctx.blocks),
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result: Some(value_id),
        exports: ctx.exports,
    })
}

/// Lower a statement in script unit context.
///
/// This handles function definitions by lowering them and adding to the unit's functions.
/// The `func_analyses` map must contain pre-computed analyses for all function statements.
///
/// If `func_param_types` is provided, use those resolved param types for function parameters
/// instead of deriving from AST type hints. This is needed for type alias support.
fn lower_statement_for_script<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
    stmt_idx: usize,
    func_analyses: &ScriptFunctionAnalyses<'db>,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name.text(ctx.db).to_string();
            let init_expr = let_stmt.value;
            let value_id = lower_expression(ctx, init_expr)?;
            let operand = Operand::Value(value_id);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            // Export the binding.
            ctx.exports.push((name, ExportBinding::Value(value_id)));
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name.text(ctx.db).to_string();
            let init_expr = var_stmt.value;
            let slot_type = ctx.expr_type(init_expr);
            let slot = ctx.fresh_slot(slot_type);
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.emit(Instruction::SlotStore {
                dest: SlotDest::Local(slot),
                value: Operand::Value(value_id),
            });
            let operand = Operand::Slot(slot);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            // Export the binding.
            ctx.exports.push((name, ExportBinding::Slot(slot)));
            Ok(())
        }
        Statement::Set(set_stmt) => {
            // No export needed for assignment.
            match &set_stmt.target {
                ast::SetTarget::Name(n) => {
                    let name = n.text(ctx.db).to_string();
                    let value_id = lower_expression(ctx, set_stmt.value)?;
                    if let Some(operand) = ctx.lookup_var(&name) {
                        match operand {
                            Operand::Slot(slot) => {
                                // Drop old value before storing new one.
                                if let Some(slot_type) = ctx.slot_type(slot).cloned() {
                                    if !slot_type.is_copy() {
                                        ctx.emit(Instruction::Drop { operand: Operand::Slot(slot) });
                                    }
                                }
                                ctx.emit(Instruction::SlotStore {
                                    dest: SlotDest::Local(slot),
                                    value: Operand::Value(value_id),
                                });
                                Ok(())
                            }
                            Operand::ExternalSlot { unit, slot } => {
                                // Drop old value before storing new one.
                                if let Some(slot_type) = ctx.external_slot_type(&name).cloned() {
                                    if !slot_type.is_copy() {
                                        ctx.emit(Instruction::Drop {
                                            operand: Operand::ExternalSlot { unit, slot },
                                        });
                                    }
                                }
                                ctx.emit(Instruction::SlotStore {
                                    dest: SlotDest::External { unit, slot },
                                    value: Operand::Value(value_id),
                                });
                                Ok(())
                            }
                            _ => Err(LowerError::VariableNotMutable(name)),
                        }
                    } else {
                        Err(LowerError::VariableNotFound(name))
                    }
                }
                ast::SetTarget::Proj(proj) => {
                    // Walk the projection chain to find root and collect field path.
                    let (root_name, field_path) = collect_field_path(ctx, proj)?;
                    let root_name_str = root_name.text(ctx.db).to_string();

                    // Lower the value expression.
                    let value_id = lower_expression(ctx, set_stmt.value)?;

                    // Look up the root slot.
                    match ctx.lookup_var(&root_name_str) {
                        Some(Operand::Slot(slot)) => {
                            // Emit SetField instruction.
                            ctx.emit(Instruction::SetField {
                                slot: SlotDest::Local(slot),
                                field_path,
                                value: Operand::Value(value_id),
                            });
                            Ok(())
                        }
                        Some(Operand::ExternalSlot { unit, slot }) => {
                            // Emit SetField instruction for external slot.
                            ctx.emit(Instruction::SetField {
                                slot: SlotDest::External { unit, slot },
                                field_path,
                                value: Operand::Value(value_id),
                            });
                            Ok(())
                        }
                        _ => Err(LowerError::VariableNotMutable(root_name_str)),
                    }
                }
            }
        }
        Statement::Ret(ret_stmt) => {
            // In scripts, return means early return from the unit.
            let value = if let Some(expr) = ret_stmt.value {
                let value_id = lower_expression(ctx, expr)?;
                Operand::Value(value_id)
            } else {
                // Return unit value for bare `ret`.
                let unit_val = ctx.fresh_value(IrType::Unit);
                ctx.emit(Instruction::Const {
                    dest: unit_val,
                    value: ConstValue::Unit,
                });
                Operand::Value(unit_val)
            };
            // Emit drops for nested scopes (but not top-level bindings).
            ctx.emit_before_return_drops(stmt_idx);
            ctx.finish_block(Terminator::UnitEarlyReturn { value });
            // Start a new unreachable block.
            let new_block = ctx.fresh_block();
            ctx.start_block(new_block);
            Ok(())
        }
        Statement::Fun(fun_stmt) => {
            // Look up pre-computed analysis.
            let analysis = func_analyses.get(fun_stmt)
                .expect("function analysis not found - caller must run analyze_script_functions first")
                .clone();

            // Define the function in the symbol table first (allows recursion).
            let func_name = fun_stmt.name(ctx.db).text(ctx.db).to_string();
            let param_count = fun_stmt.params(ctx.db).len();
            let func_id = ctx.define_func(&func_name, param_count);

            // Save current lowering state.
            let saved_blocks = std::mem::take(&mut ctx.blocks);
            let saved_instructions = std::mem::take(&mut ctx.current_instructions);
            let saved_current_block = ctx.current_block;
            let saved_next_block = ctx.next_block;
            let saved_next_value = ctx.next_value;
            let saved_next_slot = ctx.next_slot;
            let saved_next_param = ctx.next_param;
            let saved_variables = std::mem::take(&mut ctx.variables);
            // Also save binding/type state that gets modified by lower_function_body.
            let saved_binding_to_operand = std::mem::take(&mut ctx.binding_to_operand);
            let saved_next_binding_id = ctx.next_binding_id;
            let saved_param_types = std::mem::take(&mut ctx.param_types);
            let saved_param_modes = std::mem::take(&mut ctx.param_modes);
            let saved_value_types = std::mem::take(&mut ctx.value_types);
            let saved_slot_types = std::mem::take(&mut ctx.slot_types);

            // Reset for function body.
            ctx.current_block = BlockId(0);
            ctx.next_block = 1;
            ctx.next_value = 0;
            ctx.next_slot = 0;
            ctx.next_param = 0;
            ctx.next_binding_id = 0;

            // Look up resolved param types for this function.
            let func_name_str = fun_stmt.name(ctx.db).text(ctx.db);
            let resolved_params = func_param_types
                .and_then(|m| m.get(func_name_str))
                .map(|v| v.as_slice());

            // Lower the function body with resolved param types for type alias support.
            let func = lower_function_body(ctx, func_id, *fun_stmt, analysis, resolved_params)?;

            // Restore parent state.
            ctx.blocks = saved_blocks;
            ctx.current_instructions = saved_instructions;
            ctx.current_block = saved_current_block;
            ctx.next_block = saved_next_block;
            ctx.next_value = saved_next_value;
            ctx.next_slot = saved_next_slot;
            ctx.next_param = saved_next_param;
            ctx.variables = saved_variables;
            ctx.binding_to_operand = saved_binding_to_operand;
            ctx.next_binding_id = saved_next_binding_id;
            ctx.param_types = saved_param_types;
            ctx.param_modes = saved_param_modes;
            ctx.value_types = saved_value_types;
            ctx.slot_types = saved_slot_types;

            // Add the function to the unit's functions.
            ctx.functions.push(func);

            // Export the function.
            ctx.exports.push((func_name, ExportBinding::Function(func_id)));
            Ok(())
        }
        Statement::If(if_stmt) => {
            super::stmt::lower_if(ctx, if_stmt, stmt_idx)
        }
        Statement::Loop(loop_stmt) => {
            super::stmt::lower_loop(ctx, loop_stmt, stmt_idx)
        }
        Statement::Break(_) => {
            let loop_ctx = ctx.loop_stack.last()
                .ok_or(LowerError::BreakOutsideLoop)?;
            let break_target = loop_ctx.exit;

            // Emit drops for all scopes up to the loop.
            ctx.emit_before_break_drops(stmt_idx);

            ctx.finish_block(Terminator::Goto { target: break_target, args: Vec::new() });
            // Start unreachable block for code after break.
            let dead_block = ctx.fresh_block();
            ctx.start_block(dead_block);
            Ok(())
        }
        Statement::Continue(_) => {
            let loop_ctx = ctx.loop_stack.last()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            let continue_target = loop_ctx.header;

            // Emit drops for current loop iteration.
            ctx.emit_before_continue_drops(stmt_idx);

            ctx.finish_block(Terminator::Goto { target: continue_target, args: Vec::new() });
            // Start unreachable block for code after continue.
            let dead_block = ctx.fresh_block();
            ctx.start_block(dead_block);
            Ok(())
        }
        Statement::Require(_) | Statement::Import(_) => {
            // Module-level, handled elsewhere.
            Ok(())
        }
        Statement::DebugLog(stmt) => {
            let debug_expr = stmt.value;
            // Use lower_expression_for_ref to handle field projections with GetFieldRef.
            // This borrows the value instead of copying, which is correct for debuglog
            // and avoids the shallow-copy problem with move types.
            let operand = lower_expression_for_ref(ctx, debug_expr)?;
            ctx.emit(Instruction::DebugLog { operand });
            // Check if the expression produces a temporary that needs dropping.
            // - Named bindings (Value/Slot/Param): returned directly, NOT temps
            // - Field projections: use GetFieldRef (ref is Copy, no drop)
            // - Other expressions: produce temps (Values), need drop
            let expr_type = ctx.expr_type(debug_expr);
            if !expr_type.is_copy() {
                let needs_drop = match debug_expr.expr(ctx.db) {
                    ExprFunKind::Name(_) => {
                        // Named bindings are returned directly by lower_expression_for_ref.
                        // They're managed by the binding system, not temps.
                        false
                    }
                    ExprFunKind::FieldProj(_) => {
                        // Field projections use GetFieldRef which produces a ref.
                        // Refs are Copy, so no drop needed.
                        false
                    }
                    _ => true,
                };
                if needs_drop {
                    ctx.emit(Instruction::Drop { operand });
                }
            }
            Ok(())
        }
        Statement::TypeAlias(_) => {
            // Type aliases are resolved at typecheck time; nothing to lower.
            Ok(())
        }
        Statement::ParseError(_) => {
            Err(LowerError::ParseError)
        }
    }
}
