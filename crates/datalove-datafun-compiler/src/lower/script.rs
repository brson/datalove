//! Script unit lowering.
//!
//! Script units are REPL-style code fragments that can define functions, create
//! bindings, and reference values from previous units.
//!
//! Entry points:
//! - [`lower_script_unit`]: Main entry, takes `TypecheckResult`
//! - [`lower_script_fragment_raw`]: Takes raw `expr_types` for non-salsa paths
//! - [`lower_script_expr`]: For single-expression units
//!
//! When a script contains function definitions, they are lowered via
//! `lower_function_body` after swapping `FrameState` to isolate the function's IR.

use std::collections::HashMap;
use datalove_datafun_ast::ast::{self, Statement, ExprFun, ExprFunKind};
use crate::module_graph::ModuleId;
use datalove_datafun_tycheck::{TypecheckResult, ResolvedCallTarget};
use datalove_datafun_ir::{
    IrType, IrScriptUnit, Operand, Terminator, Instruction, ConstValue, SlotDest,
    ExportBinding, IrModuleId, FuncId,
    ConstBindingGraph, ResolvedConsts,
};
use crate::ownership_analysis::ScriptFunctionAnalyses;
use crate::tracked_script_ownership::ScriptAnalysisData;
use crate::ir_ext::IrTypeExt;
use super::context::{LowerCtx, ScriptLowerContext, ScriptUnitKind, FrameState};
use super::expr::{lower_expression, lower_expression_for_ref};
use super::func::lower_function_body;
use super::stmt::collect_field_path;
use super::LowerError;

/// Check if a set statement is a self-assignment (set v0 = v0) for script context.
///
/// Self-assignment is a no-op and must be detected early because the lowering
/// sequence (SlotLoad -> Drop -> SlotStore) would incorrectly destroy the value
/// before copying it.
fn is_self_assignment_script<'db>(
    ctx: &LowerCtx<'db>,
    target_name: &str,
    value_expr: ExprFun<'db>,
) -> bool {
    // Check if the value expression is just a name reference.
    if let ExprFunKind::Name(name) = value_expr.expr(ctx.db) {
        let value_name = name.text(ctx.db);
        // Check if it's the same name as the target.
        if value_name == target_name {
            // Check if target is bound to a slot (mutable variable).
            if let Some(Operand::Slot(_)) = ctx.lookup_var(target_name) {
                return true;
            }
            // Also check for external slot.
            if let Some(Operand::ExternalSlot { .. }) = ctx.lookup_var(target_name) {
                return true;
            }
        }
    }
    false
}

/// Lower a script unit.
///
/// Script units are sequences of statements (fragment) or a single expression (expr).
/// They can reference values from previous units and export bindings to subsequent units.
///
/// For fragments, caller must first call `analyze_script_fragment_tracked` to get
/// ownership analysis (`script_analysis`), then `analyze_script_functions` for `func_analyses`.
/// For expressions, pass None for `script_analysis` and an empty map for `func_analyses`.
pub fn lower_script_unit<'db>(
    db: &'db dyn salsa::Database,
    tycheck_result: TypecheckResult<'db>,
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    kind: ScriptUnitKind<'db>,
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: Option<ScriptAnalysisData>,
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
            // Use pre-computed script analysis from ownership analysis phase.
            let analysis = script_analysis
                .expect("script_analysis required for Fragment units");
            ctx.body.drop_schedule = analysis.schedule;
            ctx.body.binding_info = analysis.bindings;
            ctx.body.tracking = analysis.tracking;
            ctx.unit_end_drops = analysis.unit_end;

            // Lower all statements with index tracking.
            for (idx, stmt) in stmts.iter().enumerate() {
                ctx.body.current_stmt_idx = Some(idx);
                lower_statement_for_script(&mut ctx, stmt, idx, &func_analyses, Some(&func_param_types))?;
            }
            ctx.body.current_stmt_idx = None;

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

    // Compute unit_end values/slots BEFORE emit_unit_end_drops, because that
    // method consumes unit_end_drops which we need to compute these.
    let unit_end_values = ctx.compute_unit_end_values();
    let unit_end_slots = ctx.compute_unit_end_slots();

    // Emit drops for script-level bindings at unit end (for AOT).
    ctx.emit_unit_end_drops();

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: result.map(Operand::Value),
    });

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    Ok(IrScriptUnit {
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots: ctx.compute_tracked_slots(),
        unit_end_values,
        unit_end_slots,
        functions: ctx.functions,
        symbols: ctx.symbols,
        result,
        exports: ctx.exports,
        const_values: std::mem::take(&mut ctx.body.const_values),
    })
}

/// Lower a script fragment unit with raw expr_types.
///
/// Like `lower_script_unit` but takes expr_types directly instead of TypecheckResult.
/// Used when typechecking with context (non-salsa version).
///
/// Caller must first call `analyze_script_fragment_tracked` to get ownership analysis,
/// then call `analyze_script_functions` to get `func_analyses`.
///
/// If `func_param_types` is provided, use those resolved param types for function parameters
/// instead of deriving from AST type hints. This is needed for type alias support.
///
/// Pre-resolved const values from Phase 2 of CTFE pipeline.
/// When provided, lowering will use these values for const bindings.
///
/// DEPRECATED: This is being phased out in favor of const inlining pass.
/// Lowering should always use const_as_let mode, then const values are
/// inlined in a separate pass.
pub struct PreResolvedConsts<'a> {
    /// Script-level const binding graph.
    pub graph: &'a ConstBindingGraph,
    /// Script-level resolved const values.
    pub values: &'a ResolvedConsts,
    /// Function-level consts with qualified names (`func_name::const_name`).
    pub func_consts: &'a HashMap<String, (IrType, ConstValue)>,
}

/// Options for script lowering.
///
/// DEPRECATED: This is being phased out. Lowering will always use const_as_let
/// semantics, with const inlining happening in a separate pass.
#[derive(Debug, Clone, Default)]
pub struct ScriptLowerOptions {
    /// When true, const bindings in functions are lowered as let bindings
    /// instead of being evaluated at compile time.
    pub const_as_let: bool,
}

pub fn lower_script_fragment_raw<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    stmts: Vec<Statement<'db>>,
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: ScriptAnalysisData,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
    // DEPRECATED: resolved_consts is no longer used - consts are lowered as let bindings
    // and inlined in a separate pass. Pass None.
    _resolved_consts: Option<PreResolvedConsts<'_>>,
    // DEPRECATED: options.const_as_let is always true now.
    _options: ScriptLowerOptions,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // Always use const_as_let mode - consts are lowered as let bindings
    // and inlined by the const_inline pass after lowering.
    ctx.set_const_as_let(true);

    // Use pre-computed script analysis from ownership analysis phase.
    ctx.body.drop_schedule = script_analysis.schedule;
    ctx.body.binding_info = script_analysis.bindings;
    ctx.body.tracking = script_analysis.tracking;
    ctx.unit_end_drops = script_analysis.unit_end;

    // Pre-register all functions to enable forward references (mutual recursion).
    // This must happen before lowering any function bodies.
    for stmt in &stmts {
        if let Statement::Fun(fun_stmt) = stmt {
            let func_name = fun_stmt.name(db).text(db).to_string();
            let param_count = fun_stmt.params(db).len();
            ctx.pre_register_func(&func_name, param_count);
        }
    }

    // Lower all statements with index tracking.
    for (idx, stmt) in stmts.iter().enumerate() {
        ctx.body.current_stmt_idx = Some(idx);
        lower_statement_for_script(&mut ctx, stmt, idx, &func_analyses, func_param_types)?;
    }
    ctx.body.current_stmt_idx = None;

    // Compute unit_end and tracked slots BEFORE emit_unit_end_drops,
    // because that method consumes unit_end_drops which we need.
    let unit_end_values = ctx.compute_unit_end_values();
    let unit_end_slots = ctx.compute_unit_end_slots();
    let tracked_slots = ctx.compute_tracked_slots();

    // Emit drops for script-level bindings at unit end (for AOT).
    ctx.emit_unit_end_drops();

    // Fragment units have no result value.
    ctx.finish_block(Terminator::UnitEnd { result: None });

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    Ok(IrScriptUnit {
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots,
        unit_end_values,
        unit_end_slots,
        functions: ctx.functions,
        symbols: ctx.symbols,
        result: None,
        exports: ctx.exports,
        const_values: std::mem::take(&mut ctx.body.const_values),
    })
}

/// Lower a script expression unit.
///
/// Like `lower_script_unit` but takes expr_types directly and an expression.
/// Expression units don't have const bindings, so no pre-resolution is needed.
pub fn lower_script_expr<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    expr: ExprFun<'db>,
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

    // Expression units don't create script-level bindings, so unit_end is empty.
    Ok(IrScriptUnit {
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots: ctx.compute_tracked_slots(),
        unit_end_values: Vec::new(),
        unit_end_slots: Vec::new(),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result: Some(value_id),
        exports: ctx.exports,
        const_values: Vec::new(), // Expression units don't have const bindings.
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
    _stmt_idx: usize,
    func_analyses: &ScriptFunctionAnalyses<'db>,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
) -> Result<(), LowerError> {
    // Allocate a globally-unique statement ID that matches ownership analysis.
    // This is critical: ownership analysis uses alloc_stmt_id() for ALL statements,
    // so lowering must do the same to ensure drop schedule lookups match.
    let stmt_idx = ctx.alloc_stmt_id(stmt);
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

            // Get type and optionally lower the initializer.
            let (slot_type, init_value) = if let Some(init_expr) = var_stmt.value {
                let slot_type = ctx.expr_type(init_expr);
                let value_id = lower_expression(ctx, init_expr)?;
                (slot_type, Some(value_id))
            } else {
                // Uninitialized var - get type from type hint.
                let type_hint = var_stmt.type_hint.as_ref()
                    .expect("uninitialized var must have type hint");
                let slot_type = IrType::from_type_hint(ctx.db, type_hint);
                (slot_type, None)
            };

            let is_copy = slot_type.is_copy();
            let slot = ctx.fresh_slot(slot_type);

            // Only emit store instruction if there's an initializer.
            if let Some(value_id) = init_value {
                if is_copy {
                    ctx.emit_slot_store_copy(SlotDest::Local(slot), Operand::Value(value_id));
                } else {
                    ctx.emit_slot_store_move(SlotDest::Local(slot), Operand::Value(value_id));
                }
            }
            // If no initializer, slot is uninitialized and tracked.

            let operand = Operand::Slot(slot);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule.
            ctx.record_binding_operand(operand);
            // Export the binding.
            ctx.exports.push((name.clone(), ExportBinding::Slot(slot)));
            Ok(())
        }
        Statement::Set(set_stmt) => {
            // No export needed for assignment.
            match &set_stmt.target {
                ast::SetTarget::Name(n) => {
                    let name = n.text(ctx.db).to_string();

                    // Check for self-assignment (set v0 = v0). This is a no-op but would
                    // cause incorrect behavior because SlotLoad returns a pointer to the
                    // slot's memory, and Drop would destroy that memory before SlotStore
                    // copies from it.
                    if is_self_assignment_script(ctx, &name, set_stmt.value) {
                        return Ok(());
                    }

                    let value_id = lower_expression(ctx, set_stmt.value)?;
                    let operand = ctx.lookup_var(&name)
                        .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", name));
                    match operand {
                        Operand::Slot(slot) => {
                            // Drop old value before storing new one.
                            let is_copy = ctx.slot_type(slot).map(|t| t.is_copy()).unwrap_or(false);
                            if !is_copy {
                                // Use DropTracked for tracked slots, Drop for precise.
                                let operand = Operand::Slot(slot);
                                if ctx.is_operand_tracked(operand) {
                                    ctx.emit(Instruction::DropTracked { operand });
                                } else {
                                    ctx.emit(Instruction::Drop { operand });
                                }
                            }
                            if is_copy {
                                ctx.emit_slot_store_copy(SlotDest::Local(slot), Operand::Value(value_id));
                            } else {
                                ctx.emit_slot_store_move(SlotDest::Local(slot), Operand::Value(value_id));
                            }
                            Ok(())
                        }
                        Operand::ExternalSlot { unit, slot } => {
                            // Drop old value before storing new one.
                            // Use DropTracked: external slots are script-level bindings, always tracked.
                            let is_copy = ctx.external_slot_type(&name).map(|t| t.is_copy()).unwrap_or(false);
                            if !is_copy {
                                ctx.emit(Instruction::DropTracked {
                                    operand: Operand::ExternalSlot { unit, slot },
                                });
                            }
                            if is_copy {
                                ctx.emit_slot_store_copy(SlotDest::External { unit, slot }, Operand::Value(value_id));
                            } else {
                                ctx.emit_slot_store_move(SlotDest::External { unit, slot }, Operand::Value(value_id));
                            }
                            Ok(())
                        }
                        _ => panic!("assignment to immutable variable '{}' - typechecker should catch this", name),
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
                            ctx.emit_set_field(SlotDest::Local(slot), field_path, Operand::Value(value_id));
                            Ok(())
                        }
                        Some(Operand::ExternalSlot { unit, slot }) => {
                            // Emit SetField instruction for external slot.
                            ctx.emit_set_field(SlotDest::External { unit, slot }, field_path, Operand::Value(value_id));
                            Ok(())
                        }
                        _ => panic!("assignment to field of immutable variable '{}' - typechecker should catch this", root_name_str),
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
                ctx.emit_const(unit_val, ConstValue::Unit);
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

            // Swap in fresh state for function body.
            let saved = ctx.swap_body_state(FrameState::new());

            // Look up resolved param types for this function.
            let func_name_str = fun_stmt.name(ctx.db).text(ctx.db);
            let resolved_params = func_param_types
                .and_then(|m| m.get(func_name_str))
                .map(|v| v.as_slice());

            // Lower the function body with resolved param types for type alias support.
            let func = lower_function_body(ctx, func_id, *fun_stmt, analysis, resolved_params)?;

            // Restore parent state.
            ctx.swap_body_state(saved);

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
            // Typechecker validates break is inside a loop (F050).
            let loop_ctx = ctx.body.loop_stack.last()
                .unwrap_or_else(|| panic!("break outside loop - typechecker should catch this"));
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
            // Typechecker validates continue is inside a loop (F051).
            let loop_ctx = ctx.body.loop_stack.last()
                .unwrap_or_else(|| panic!("continue outside loop - typechecker should catch this"));
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
            // Use lower_expression_for_ref to handle field projections with GetFieldRef.
            // This borrows the value instead of copying, avoiding shallow-copy issues.
            let operand = lower_expression_for_ref(ctx, stmt.value)?;
            // If the operand is a ref, dereference it for reading.
            let operand = ctx.deref_if_ref(operand);
            ctx.emit(Instruction::DebugLog { operand });
            // Drop any expression temporaries (e.g., string literals, binop results).
            // lower_expression_for_ref records temps for compound expressions,
            // but not for Name lookups or FieldProj (which use refs).
            ctx.emit_expr_temp_drops();
            Ok(())
        }
        Statement::TypeAlias(_) => {
            // Type aliases are resolved at typecheck time; nothing to lower.
            Ok(())
        }
        Statement::Const(const_stmt) => {
            let name = const_stmt.name.text(ctx.db).to_string();
            let init_expr = const_stmt.value;

            // In const_as_let mode, lower const as let binding (for both script and function level).
            if ctx.const_as_let() {
                let value_id = lower_expression(ctx, init_expr)?;
                let operand = Operand::Value(value_id);
                ctx.bind_var(&name, operand);
                // Record binding operand for drop schedule.
                ctx.record_binding_operand(operand);
                // Export the binding for cross-unit reference.
                ctx.exports.push((name.clone(), ExportBinding::Value(value_id)));
                // Track as const for const inlining pass.
                ctx.body.const_values.push((name, value_id));
                return Ok(());
            }

            // Check if already pre-resolved (from Phase 2).
            if ctx.lookup_const(&name).is_some() {
                return Ok(());
            }

            // Evaluate the const expression using CTFE.
            let ir_type = ctx.expr_type(init_expr);
            let value = super::const_expr::eval_const_expr(ctx, init_expr)?;
            ctx.add_const(name, ir_type, value);
            Ok(())
        }
        Statement::ParseError(_) => {
            panic!("parse error node reached lowering - callers should check for parse errors before lowering")
        }
    }
}
