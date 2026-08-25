//! Script unit lowering.
//!
//! Script units are REPL-style code fragments that can define functions, create
//! bindings, and reference values from previous units.
//!
//! Entry points:
//! - [`lower_script_fragment_raw`]: For statement sequences (fragments)
//! - [`lower_script_expr`]: For single-expression units
//!
//! When a script contains function definitions, they are lowered via
//! `lower_function_body` after swapping `FrameState` to isolate the function's IR.

use std::collections::{HashMap, HashSet};
use bct::module_graph::ModuleId;
use datalove_datafun_ast::ast::{self, Statement, ExprFun, ExprFunKind};
use datalove_datafun_common::Type;
use datalove_datafun_sema::ResolvedCallTarget;
use datalove_datafun_ir::{
    IrType, IrCodeUnit, CodeUnitId, CodeUnitContext, ScriptContext,
    Operand, Terminator, Instruction, ConstValue, SlotDest, ExportBinding, IrModuleId, FuncId,
};
use datalove_datafun_sema::ScriptAnalysisData;
use crate::ScriptFunctionAnalyses;
use super::context::{LowerCtx, ScriptLowerContext, FrameState};
use super::expr::{lower_expression, lower_expression_for_ref};
use super::func::lower_function_body;
use super::stmt::{collect_field_path_from_place, lower_statement};
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
    if let ExprFunKind::Place(ref place) = value_expr.expr(ctx.db) {
        if !place.steps.is_empty() {
            return false;
        }
        let value_name = place.root.text(ctx.db);
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

/// Lower a script fragment unit.
/// Lower a script fragment to IR.
///
/// Caller must first call `analyze_script_fragment_tracked` to get ownership analysis,
/// then call `analyze_script_functions` to get `func_analyses`.
///
/// If `func_param_types` and `func_return_types` are provided, use those resolved types
/// for function parameters and return type instead of deriving from AST type hints.
/// This is needed for type alias support.
///
/// If `lowered_functions` is provided, those functions are reused instead of being
/// re-lowered. The tuple contains the lowered functions and a map from function
/// name to FuncId.
///
/// Const bindings are lowered as let bindings. The const inlining pass runs
/// separately to replace them with literal values.
pub fn lower_script_fragment_raw<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    stmts: Vec<Statement<'db>>,
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: ScriptAnalysisData,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
    func_return_types: Option<&HashMap<String, IrType>>,
    lowered_functions: Option<(Vec<IrCodeUnit>, HashMap<String, FuncId>)>,
) -> Result<IrCodeUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // Use pre-computed script analysis from ownership analysis phase.
    ctx.body.drop_schedule = script_analysis.schedule;
    ctx.body.binding_info = script_analysis.bindings;
    ctx.body.tracking = script_analysis.tracking;
    ctx.unit_end_drops = script_analysis.unit_end;
    ctx.body.adapt_sites = script_analysis.adapt_sites;

    // Track which functions have already been lowered (to skip in statement handling).
    let already_lowered_funcs: HashSet<String>;

    // Handle lowered functions if provided.
    if let Some((functions, func_name_to_id)) = lowered_functions {
        // Use lowered functions directly.
        ctx.functions = functions;

        // Register each function with its existing FuncId so call resolution works.
        for (name, func_id) in &func_name_to_id {
            ctx.register_func_with_id(name, 0, *func_id);
        }

        already_lowered_funcs = func_name_to_id.keys().cloned().collect();
    } else {
        already_lowered_funcs = HashSet::new();

        // Pre-register all functions to enable forward references (mutual recursion).
        // This must happen before lowering any function bodies.
        for stmt in &stmts {
            if let Statement::Fun(fun_stmt) = stmt {
                let func_name = fun_stmt.name(db).text(db).to_string();
                let param_count = fun_stmt.params(db).len();
                ctx.pre_register_func(&func_name, param_count);
            }
        }
    }

    // Lower all statements with index tracking.
    for (idx, stmt) in stmts.iter().enumerate() {
        ctx.body.current_stmt_idx = Some(idx);
        lower_statement_for_script(&mut ctx, stmt, idx, &func_analyses, func_param_types, func_return_types, &already_lowered_funcs)?;
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

    Ok(IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        call_site_count: ctx.body.next_call_site,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots,
        const_values: std::mem::take(&mut ctx.body.const_values),
        symbols: ctx.symbols,
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values,
            unit_end_slots,
            result: None,
            result_name: None,
            exports: ctx.exports,
        }),
        nested_units: ctx.functions,
    })
}

/// Lower only the functions from a script fragment.
///
/// This is used to pre-lower functions before CTFE, so that const expressions
/// that call functions can reuse the already-lowered function IR instead of
/// re-lowering them.
///
/// The `script_ctx` parameter provides access to accumulated function bindings
/// from previous compilation units, enabling cross-unit function calls.
///
/// The `func_id_map` parameter maps module functions to their IR locations,
/// enabling calls to module functions from script functions.
///
/// Returns a vector of lowered functions and a map from function name to FuncId.
pub fn lower_script_functions<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    stmts: &[Statement<'db>],
    func_analyses: &ScriptFunctionAnalyses<'db>,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
    func_return_types: Option<&HashMap<String, IrType>>,
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
) -> Result<(Vec<IrCodeUnit>, HashMap<String, FuncId>), LowerError> {
    // Create a minimal context with the accumulated script context.
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // Pre-register all functions to enable forward references (mutual recursion).
    for stmt in stmts {
        if let Statement::Fun(fun_stmt) = stmt {
            let func_name = fun_stmt.name(db).text(db).to_string();
            let param_count = fun_stmt.params(db).len();
            ctx.pre_register_func(&func_name, param_count);
        }
    }

    // Build a map of function names to FuncIds.
    let mut func_name_to_id: HashMap<String, FuncId> = HashMap::new();
    let mut lowered_units: Vec<IrCodeUnit> = Vec::new();

    // Lower only the function statements.
    for stmt in stmts {
        if let Statement::Fun(fun_stmt) = stmt {
            // Look up pre-computed analysis.
            let analysis = func_analyses.get(fun_stmt)
                .expect("function analysis not found")
                .clone();

            let func_name = fun_stmt.name(db).text(db).to_string();
            let param_count = fun_stmt.params(db).len();
            let func_id = ctx.define_func(&func_name, param_count);

            func_name_to_id.insert(func_name.clone(), func_id);

            // Swap in fresh state for function body.
            let saved = ctx.swap_body_state(FrameState::new());

            // Look up resolved types for this function.
            let func_name_str = fun_stmt.name(db).text(db);
            let resolved_params = func_param_types
                .and_then(|m| m.get(func_name_str))
                .map(|v| v.as_slice());
            let resolved_return = func_return_types
                .and_then(|m| m.get(func_name_str))
                .cloned();

            // Lower the function body (returns IrCodeUnit).
            let unit = lower_function_body(&mut ctx, func_id, *fun_stmt, analysis, resolved_params, resolved_return)?;

            // Restore parent state.
            ctx.swap_body_state(saved);

            lowered_units.push(unit);
        }
    }

    Ok((lowered_units, func_name_to_id))
}

/// Lower a script expression unit.
///
/// Like `lower_script_unit` but takes expr_types directly and an expression.
/// Expression units don't have const bindings, so no pre-resolution is needed.
pub fn lower_script_expr<'db>(
    db: &'db dyn salsa::Database,
    expr_types: &'db [Option<Type<'db>>],
    call_targets: &'db [Option<ResolvedCallTarget<'db>>],
    func_id_map: &'db HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
    script_ctx: ScriptLowerContext,
    expr: ExprFun<'db>,
) -> Result<IrCodeUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, expr_types, call_targets, func_id_map, script_ctx);

    // An expression unit that is just a name is the prompt asking to see a
    // binding, not to take it. Name it and compute nothing.
    let result_name = shown_binding(&ctx, expr);
    let result = match result_name {
        Some(_) => None,
        None => Some(lower_expression(&mut ctx, expr)?),
    };

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: result.map(Operand::Value),
    });

    // Renumber blocks for O(1) lookup in interpreter.
    ctx.renumber_blocks();

    // Expression units don't create script-level bindings, so unit_end is empty.
    Ok(IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks: std::mem::take(&mut ctx.body.blocks),
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        call_site_count: ctx.body.next_call_site,
        value_types: std::mem::take(&mut ctx.body.value_types),
        slot_types: std::mem::take(&mut ctx.body.slot_types),
        tracked_slots: ctx.compute_tracked_slots(),
        const_values: Vec::new(), // Expression units don't have const bindings.
        symbols: ctx.symbols,
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            result,
            result_name,
            exports: ctx.exports,
        }),
        nested_units: ctx.functions,
    })
}

/// The binding an expression unit merely shows, if that is all it does.
///
/// A bare name bound by an earlier unit needs no code: the caller reads that
/// binding where it lives, which is both cheaper and non-consuming.
fn shown_binding<'db>(ctx: &LowerCtx<'db>, expr: ExprFun<'db>) -> Option<String> {
    let ExprFunKind::Place(ref place) = expr.expr(ctx.db) else {
        return None;
    };
    if !place.steps.is_empty() {
        return None;
    }
    let name = place.root.text(ctx.db);
    match ctx.lookup_var(name) {
        Some(Operand::ExternalValue { .. }) | Some(Operand::ExternalSlot { .. }) => Some(name.to_string()),
        _ => None,
    }
}

/// Lower a statement in script unit context.
///
/// This handles function definitions by lowering them and adding to the unit's functions.
/// The `func_analyses` map must contain pre-computed analyses for all function statements.
///
/// If `func_param_types` and `func_return_types` are provided, use those resolved types
/// for function parameters and return type instead of deriving from AST type hints.
/// This is needed for type alias support.
///
/// If `already_lowered_funcs` contains a function name, that function has already been lowered
/// and only the export should be emitted (body lowering is skipped).
fn lower_statement_for_script<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
    _stmt_idx: usize,
    func_analyses: &ScriptFunctionAnalyses<'db>,
    func_param_types: Option<&HashMap<String, Vec<IrType>>>,
    func_return_types: Option<&HashMap<String, IrType>>,
    already_lowered_funcs: &HashSet<String>,
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

            let operand = Operand::Slot(slot);
            ctx.bind_var(&name, operand);
            // Record binding operand for drop schedule. This has to happen
            // before the store, which asks whether the slot is tracked.
            ctx.record_binding_operand(operand);

            // Only emit store instruction if there's an initializer.
            if let Some(value_id) = init_value {
                if is_copy {
                    ctx.emit_slot_store_copy(SlotDest::Local(slot), Operand::Value(value_id));
                } else {
                    ctx.emit_slot_store_move(SlotDest::Local(slot), Operand::Value(value_id));
                }
            }
            // If no initializer, slot is uninitialized and tracked.

            // Export the binding.
            ctx.exports.push((name.clone(), ExportBinding::Slot(slot)));
            Ok(())
        }
        Statement::Set(set_stmt) => {
            // No export needed for assignment.
            let place = &set_stmt.target;
            let root_name_str = place.root.text(ctx.db).to_string();

            if place.steps.is_empty() {
                // Simple name assignment.
                if is_self_assignment_script(ctx, &root_name_str, set_stmt.value) {
                    return Ok(());
                }

                let value_id = lower_expression(ctx, set_stmt.value)?;
                let operand = ctx.lookup_var(&root_name_str)
                    .unwrap_or_else(|| panic!("variable '{}' not found - typechecker should catch this", root_name_str));
                match operand {
                    Operand::Slot(slot) => {
                        let is_copy = ctx.slot_type(slot).map(|t| t.is_copy()).unwrap_or(false);
                        if !is_copy {
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
                        let is_copy = ctx.external_slot_type(&root_name_str).map(|t| t.is_copy()).unwrap_or(false);
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
                    _ => panic!("assignment to immutable variable '{}' - typechecker should catch this", root_name_str),
                }
            } else if place.steps.iter().any(|s| matches!(s, ast::PlaceStep::Index(_))) {
                // Contains index — delegate to the general lowering in stmt.rs.
                lower_statement(ctx, stmt)
            } else {
                // Pure field projections.
                let field_path = collect_field_path_from_place(ctx, place)?;
                let value_id = lower_expression(ctx, set_stmt.value)?;
                match ctx.lookup_var(&root_name_str) {
                    Some(Operand::Slot(slot)) => {
                        ctx.emit_set_field(SlotDest::Local(slot), field_path, Operand::Value(value_id));
                        Ok(())
                    }
                    Some(Operand::ExternalSlot { unit, slot }) => {
                        ctx.emit_set_field(SlotDest::External { unit, slot }, field_path, Operand::Value(value_id));
                        Ok(())
                    }
                    _ => panic!("assignment to field of immutable variable '{}' - typechecker should catch this", root_name_str),
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
            let func_name = fun_stmt.name(ctx.db).text(ctx.db).to_string();

            // If this function was already lowered, just export it (body already in ctx.functions).
            if already_lowered_funcs.contains(&func_name) {
                let code_ref = ctx.lookup_func(&func_name)
                    .expect("lowered function should be registered");
                let unit_id = match code_ref {
                    datalove_datafun_ir::CodeRef::Local(id) => id,
                    _ => panic!("lowered function should be local"),
                };
                ctx.exports.push((func_name, ExportBinding::Function(unit_id)));
                return Ok(());
            }

            // Look up pre-computed analysis.
            let analysis = func_analyses.get(fun_stmt)
                .expect("function analysis not found - caller must run analyze_script_functions first")
                .clone();

            // Define the function in the symbol table first (allows recursion).
            let param_count = fun_stmt.params(ctx.db).len();
            let func_id = ctx.define_func(&func_name, param_count);

            // Swap in fresh state for function body.
            let saved = ctx.swap_body_state(FrameState::new());

            // Look up resolved types for this function.
            let func_name_str = fun_stmt.name(ctx.db).text(ctx.db);
            let resolved_params = func_param_types
                .and_then(|m| m.get(func_name_str))
                .map(|v| v.as_slice());
            let resolved_return = func_return_types
                .and_then(|m| m.get(func_name_str))
                .cloned();

            // Lower the function body with resolved types for type alias support.
            let unit = lower_function_body(ctx, func_id, *fun_stmt, analysis, resolved_params, resolved_return)?;

            // Restore parent state.
            ctx.swap_body_state(saved);

            // Add the function to the unit's functions.
            ctx.functions.push(unit);

            // Export the function.
            ctx.exports.push((func_name, ExportBinding::Function(datalove_datafun_ir::CodeUnitId(func_id.0))));
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
            // Const bindings are lowered as let bindings.
            // The const inlining pass runs later to replace with literal values.
            let name = const_stmt.name.text(ctx.db).to_string();
            let init_expr = const_stmt.value;
            let value_id = lower_expression(ctx, init_expr)?;
            let operand = Operand::Value(value_id);
            ctx.bind_var(&name, operand);
            ctx.record_binding_operand(operand);
            ctx.exports.push((name.clone(), ExportBinding::Value(value_id)));
            ctx.body.const_values.push((name, value_id));
            Ok(())
        }
        Statement::Match(match_stmt) => {
            super::stmt::lower_match(ctx, match_stmt, stmt_idx)
        }
        Statement::NativeFun(_) => {
            // Native function declarations have no body to lower.
            Ok(())
        }
        Statement::ExprStatement(stmt) => {
            // Lower the function call expression and discard the unit result.
            lower_expression(ctx, stmt.expr)?;
            ctx.emit_expr_temp_drops();
            Ok(())
        }
        Statement::ParseError(_) => {
            panic!("parse error node reached lowering - callers should check for parse errors before lowering")
        }
    }
}
