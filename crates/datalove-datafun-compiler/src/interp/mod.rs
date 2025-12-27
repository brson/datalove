//! Frame-based interpreter with linear type semantics.
//!
//! # Overview
//!
//! Executes Datafun code after typecheck and function analysis. Uses frame-based
//! execution with destination-passing style (DPS) to minimize heap allocations.
//! All values live in caller-owned memory (frame slots); linear type semantics
//! determine when values are moved vs cloned.
//!
//! # Execution Model
//!
//! Each function call creates a `StackFrame` with a packed byte buffer for slots.
//! The `function_analysis` module computes a `FrameLayout` mapping variables and
//! temporaries to byte offsets. The `ControlFlowGraph` (CFG) drives execution
//! through basic blocks with explicit terminators.
//!
//! # Destination-Passing Style (DPS)
//!
//! Expressions receive a `Destination` specifying where to write results. This
//! avoids intermediate allocations: the result is written directly to the caller's
//! storage (frame slot or return destination). After evaluation, `dest.to_value()`
//! creates a readable `Value` from the written data.
//!
//! # Value vs Destination
//!
//! - **Destination**: Write target passed to expression evaluation ("write here")
//! - **Value**: Readable data after evaluation ("read from here")
//!
//! Both are (ptr, tydesc) pairs. Destinations are inputs; Values are outputs.
//!
//! # Linear Type Semantics
//!
//! Copy types (bool, fixed integers, f32) clone transparently. Linear types (int,
//! string, collections) use move semantics. `SlotState` tracks each slot:
//! - `Untracked`: Slot doesn't need tracking (release mode only)
//! - `Uninitialized`: Not yet written; skip cleanup
//! - `Available`: Valid data; needs cleanup if not moved
//! - `Moved`: Ownership transferred; skip cleanup
//!
//! Operators read operands by reference: for variable operands, we construct a
//! `Value` pointing directly at the slot without cloning. Compound operands
//! (nested binops, function calls, etc.) are evaluated to temp slots. The result
//! is written directly to the destination. Variables consumed by function args
//! or `ret` are marked `Moved`.
//!
//! # Control Flow
//!
//! CFG execution: process statements in a block, then follow the terminator:
//! - `Return`: Function exit
//! - `Branch`: If-condition evaluation (handles Bool, Option, Result)
//! - `Goto`: Unconditional jump (with branch-exit drops for convergence)
//! - `LoopContinue`/`LoopBreak`: Loop control
//!
//! # Cleanup
//!
//! `DropPoints` from function analysis identifies slots needing cleanup. At
//! function exit, `cleanup_frame` destroys slots still `Available`. Branch-exit
//! drops handle convergence: if a slot is moved in one branch but not another,
//! the non-moved branch drops it at exit so both paths converge consumed.
//!
//! # Key Types
//!
//! - [`InterpContext`]: Runtime, module graph, call stack, tydesc table
//! - [`Value`]: Pointer + type descriptor (readable data)
//! - [`Destination`]: Pointer + type descriptor (write target)
//! - [`StackFrame`]: Frame buffer, slot states, CFG, drop points

mod value;
mod error;
mod frame;
mod memory;
mod types;
mod alloc;
mod arith;
mod arith_widening;
mod collections;
mod literals;
mod context;
mod control;
mod slots;
mod dps;
pub mod tydesc;

pub use value::{Value, Destination};
pub use error::InterpError;
pub use frame::{SlotState, StackFrame, FrameContext, set_slot_state_vec, get_slot_state_vec};
pub use memory::{destroy_value, destroy_value_contents_only, free_value_structure};
pub use context::{InterpContext, ModuleFunctionTableGraph};

use control::{evaluate_branch_condition, eval_try_option, eval_try_result};
use frame::CfgControl;
use memory::{clone_value_to_dest, move_value_to_dest, cleanup_frame, process_block_exit_drops};
use alloc::{write_result_err_to_dest, write_data_to_dest, write_error_to_dest};
use collections::{eval_inline_list, eval_inline_set, eval_inline_map, eval_inline_anon_tuple, eval_inline_anon_struct};
use literals::{
    write_inline_int_to_dest, write_option_none_to_dest,
    write_bool_to_dest, write_f32_to_dest, write_u32_to_dest,
    write_string_to_dest,
};
use slots::{eval_operand, cleanup_operand, get_destination_for_expr, mark_temp_slot_available, mark_temp_slot_moved, read_reference_slot, get_slot_tydesc};
use dps::{WrapperKind, eval_wrapper_payload_dps};
use arith_widening::{execute_binop, execute_unop};

use crate::module_graph::ModuleId;
use crate::ast;
use crate::function_analysis::{Terminator, BlockId};
use crate::tycheck::is_unit_type;

// ============================================================================
// Main Entry Point
// ============================================================================

/// Evaluate a function call from frame-based execution.
///
/// The return value is written directly to `return_dest`.
/// All functions return a value (void functions return unit `()`).
fn eval_function_call_frame<'db>(
    ctx: &mut InterpContext<'db>,
    call_expr: ast::ExprFunctionCall<'db>,
    return_dest: Destination,
) -> Result<Option<Value>, InterpError> {
    let arg_exprs = call_expr.args(ctx.db);

    // Use pre-resolved call target from typechecking.
    let (func, func_module) = match ctx.get_call_target(call_expr) {
        Some(target) => (target.func(ctx.db), target.module_id(ctx.db)),
        None => {
            let name = call_expr.name(ctx.db);
            panic!(
                "No resolved call target for function '{}' - typechecking should have resolved this",
                name.text(ctx.db)
            );
        }
    };

    let params = func.params(ctx.db);

    // Check argument count matches parameter count.
    if arg_exprs.len() != params.len() {
        return Err(InterpError::InvalidExpression(
            format!("Function '{}' expects {} arguments but {} provided",
                func.name(ctx.db).text(ctx.db), params.len(), arg_exprs.len())
        ));
    }

    // Evaluate all arguments in frame context with temp slot destinations.
    let mut arg_values = Vec::new();
    for arg_expr in arg_exprs {
        // Get temp slot destination for this argument.
        let arg_dest = match get_destination_for_expr(ctx, *arg_expr) {
            Ok(d) => d,
            Err(e) => {
                for val in arg_values {
                    destroy_value(ctx, val);
                }
                return Err(e);
            }
        };
        match eval_expression_frame(ctx, *arg_expr, arg_dest) {
            Ok(()) => {
                // All values are now Borrowed (written to temp slots).
                mark_temp_slot_available(ctx, *arg_expr);
            }
            Err(e) => {
                // Clean up previously evaluated arguments on error.
                for val in arg_values {
                    destroy_value(ctx, val);
                }
                return Err(e);
            }
        }
        arg_values.push(arg_dest.to_value());
    }

    // Execute the function body with arguments and return destination.
    execute_function_body(ctx, func, func_module, arg_values, return_dest)
}

/// Execute a function body and return its result.
///
/// This uses frame-based execution with analysis-driven slot allocation.
/// Return expressions write directly to caller's memory via `return_dest`.
/// All functions return a value (void functions return unit `()`).
pub fn execute_function_body<'db>(
    ctx: &mut InterpContext<'db>,
    func: ast::StmtFun<'db>,
    func_module: Option<ModuleId>,
    arg_values: Vec<Value>,
    return_dest: Destination,
) -> Result<Option<Value>, InterpError> {
    // Helper to clean up arguments on early error (before frame execution).
    // All arguments must be destroyed since they were never used.
    fn cleanup_args_on_error(ctx: &mut InterpContext<'_>, arg_values: Vec<Value>) {
        for arg_value in arg_values {
            destroy_value(ctx, arg_value);
        }
    }

    // Helper to clean up arguments after successful frame execution.
    // For copy types: free the original structure (function cloned it).
    // For non-copy types: check if the slot was moved (ownership transferred).
    fn cleanup_args_after_frame(
        ctx: &mut InterpContext<'_>,
        arg_values: Vec<Value>,
        slot_states: &[SlotState],
        layout: crate::function_analysis::FrameLayout<'_>,
        db: &dyn crate::Db,
    ) {
        for (i, arg_value) in arg_values.into_iter().enumerate() {
            // Use O(1) indexed lookup by parameter position.
            if let Some(slot_info) = layout.get_param_slot(db, i) {
                // Use precomputed copyability from slot_info.
                if slot_info.is_copy(db) {
                    // Copy types were cloned by the function, free the original structure.
                    free_value_structure(ctx, arg_value);
                } else {
                    // Non-copy types: check if they were moved (consumed by function).
                    let slot_id = slot_info.slot_id(db);
                    if get_slot_state_vec(slot_states, slot_id) != SlotState::Moved {
                        // Slot was never read/moved, so destroy the argument.
                        destroy_value(ctx, arg_value);
                    }
                    // All values are Borrowed - structures owned by caller's frame.
                }
            }
        }
    }

    // Helper to restore previous module context.
    fn restore_module_context(ctx: &mut InterpContext<'_>, prev: Option<ModuleId>) {
        ctx.current_module_id = prev;
    }

    // Save previous module context.
    let prev_module = ctx.current_module_id;

    // Set current module if the function is from a module.
    if let Some(module_id) = func_module {
        ctx.current_module_id = Some(module_id);
    }

    // Get function analysis from module graph typecheck result.
    let Some(typecheck_result) = &ctx.module_graph_typecheck else {
        restore_module_context(ctx, prev_module);
        cleanup_args_on_error(ctx, arg_values);
        return Err(InterpError::RuntimeError(
            "No typecheck result available - cannot execute function".to_string()
        ));
    };

    let analyses = typecheck_result.function_analyses(ctx.db);
    let analysis = {
        use salsa::plumbing::AsId;
        let index = func.as_id().index() as usize;
        match analyses.get(index).copied().flatten() {
            Some(a) => a,
            None => {
                restore_module_context(ctx, prev_module);
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    format!("No analysis found for function '{}'", func.name(ctx.db).text(ctx.db))
                ));
            }
        }
    };

    // Check for critical analysis errors (ignore warnings like ValueNotUsed).
    let critical_errors: Vec<_> = analysis.errors(ctx.db)
        .iter()
        .filter(|e| !matches!(e, crate::function_analysis::AnalysisError::ValueNotUsed { .. }))
        .collect();

    if !critical_errors.is_empty() {
        restore_module_context(ctx, prev_module);
        cleanup_args_on_error(ctx, arg_values);
        return Err(InterpError::RuntimeError(
            format!("Function '{}' has analysis errors: {:?}",
                func.name(ctx.db).text(ctx.db),
                critical_errors)
        ));
    }

    // Get frame layout, CFG, and drop points.
    let layout = analysis.frame_layout(ctx.db);
    let cfg = analysis.control_flow(ctx.db);
    let drop_points = analysis.drop_points(ctx.db);
    let total_size = layout.total_size(ctx.db) as usize;
    let slots = layout.slots(ctx.db);

    // Allocate frame data.
    let mut frame_data = vec![0u8; total_size];

    // Initialize slot states based on tracking needs.
    let mut slot_states = StackFrame::init_slot_states(slots, ctx.db);

    // Pre-compute tydescs for all slots (avoids repeated hash lookups during execution).
    let mut slot_tydescs = Vec::with_capacity(slots.len());
    for slot in slots {
        let ty = slot.ty(ctx.db);
        let datalit_ty = match ty.ty(ctx.db) {
            crate::tycheck::Type::Datalit(dt) => dt.clone(),
            _ => {
                restore_module_context(ctx, prev_module);
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    "Non-datalit type in slot (compiler bug)".to_string()
                ));
            }
        };
        let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);
        slot_tydescs.push(tydesc);
    }

    // Initialize parameters by writing argument values to their slot offsets.
    let params = func.params(ctx.db);
    for (i, param) in params.iter().enumerate() {
        // For now, only support In mode parameters.
        if param.mode(ctx.db) != ast::ParamMode::In {
            restore_module_context(ctx, prev_module);
            cleanup_args_on_error(ctx, arg_values);
            return Err(InterpError::InvalidExpression(
                format!("Parameter mode {:?} not yet supported (function '{}')",
                    param.mode(ctx.db), func.name(ctx.db).text(ctx.db))
            ));
        }

        let arg_value = arg_values[i];

        // Find the slot for this parameter using O(1) indexed lookup.
        let slot_info = match layout.get_param_slot(ctx.db, i) {
            Some(s) => s,
            None => {
                restore_module_context(ctx, prev_module);
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    format!("Parameter '{}' not found in frame layout", param.name(ctx.db).text(ctx.db))
                ));
            }
        };

        let offset = slot_info.offset(ctx.db) as usize;

        // Parameters are Reference slots (pointer-sized).
        // Store the pointer to the argument value.
        let ptr_bytes = arg_value.ptr as usize;
        frame_data[offset..offset + std::mem::size_of::<usize>()]
            .copy_from_slice(&ptr_bytes.to_ne_bytes());

        // Mark parameter slot as Available (it now contains a valid pointer).
        let slot_id = slot_info.slot_id(ctx.db);
        set_slot_state_vec(&mut slot_states, slot_id, SlotState::Available);
    }

    // Build frame context from function metadata.
    // Get return type from function analysis (unit for void functions).
    let return_type = analysis.return_type(ctx.db);
    let context = FrameContext {
        context_name: func.name(ctx.db),
        return_type,
    };

    // Create and push the stack frame.
    let frame = StackFrame {
        frame_data,
        slot_states,
        slot_tydescs,
        context,
        layout,
        cfg,
        drop_points,
        return_dest,
    };
    ctx.call_stack.push(frame);

    // Execute function body.
    let (result, exit_block_id) = execute_function_body_with_frame(ctx);

    // Pop the frame and capture slot states for argument cleanup.
    let frame = ctx.call_stack.pop().unwrap();
    let final_slot_states = frame.slot_states.clone();

    // All functions return a value (void functions return unit).
    // Return value was written directly to caller's memory via DPS.
    let result = match result {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => unreachable!("all functions return a value (void returns unit)"),
        Err(e) => Err(e),
    };

    // Clean up frame values before returning.
    cleanup_frame(ctx, frame, exit_block_id);

    // Clean up arguments based on their final slot states.
    cleanup_args_after_frame(ctx, arg_values, &final_slot_states, layout, ctx.db);

    // Restore previous module.
    restore_module_context(ctx, prev_module);

    // Handle try-operator early returns (? and ! operators).
    // Note: T→Option<T> coercion is handled during return expression evaluation via DPS,
    // so we don't need to wrap Ok values here. We only handle try-operator errors.
    use crate::datalit::ast::TypeHint;
    let return_type = func.return_type(ctx.db);
    match &result {
        Err(InterpError::OptionNone) => {
            // Early return via ? operator - create Option::None.
            if let Some(ret_type) = return_type {
                if matches!(ret_type.type_hint(ctx.db), TypeHint::Option(_)) {
                    // DPS: write None to caller's destination.
                    write_option_none_to_dest(return_dest)?;
                    return Ok(Some(return_dest.to_value()));
                }
            }
        }
        Err(InterpError::ResultErr { tydesc, ptr }) => {
            // Early return via ! operator - create Result::Err.
            if let Some(ret_type) = return_type {
                if matches!(ret_type.type_hint(ctx.db), TypeHint::Result(_)) {
                    // DPS: write Err to caller's destination.
                    write_result_err_to_dest(return_dest, *tydesc, *ptr)?;
                    return Ok(Some(return_dest.to_value()));
                }
            }
        }
        _ => {}
    }

    result
}


/// Execute function body with CFG-based execution.
///
/// Returns (result, exit_block_id) where exit_block_id is the block that caused the return.
fn execute_function_body_with_frame<'db>(
    ctx: &mut InterpContext<'db>,
) -> (Result<Option<Value>, InterpError>, BlockId) {
    // Get the current frame (top of stack).
    let frame_index = ctx.call_stack.len() - 1;

    // Get context and CFG from the frame.
    let context_name = ctx.call_stack[frame_index].context.context_name;
    let return_type = ctx.call_stack[frame_index].context.return_type;
    let cfg = ctx.call_stack[frame_index].cfg;

    // Start at block 0 (entry block).
    let mut current_block_id = BlockId(0);

    loop {
        let block = match cfg.get_block(ctx.db, current_block_id) {
            Some(b) => b,
            None => return (Err(InterpError::RuntimeError(
                format!("Invalid block ID {:?}", current_block_id)
            )), current_block_id),
        };

        // Execute all statements in the current block.
        for stmt_id in &block.statements {
            let stmt = match cfg.get_stmt(ctx.db, *stmt_id) {
                Some(s) => s,
                None => return (Err(InterpError::RuntimeError(
                    format!("Invalid stmt ID {:?}", stmt_id)
                )), current_block_id),
            };

            // Execute statement.
            match execute_cfg_statement(ctx, stmt) {
                Ok(CfgControl::Continue) => continue,
                Ok(CfgControl::Return(value)) => return (Ok(Some(value)), current_block_id),
                Err(e) => return (Err(e), current_block_id),
            }
        }

        // Handle terminator.
        match &block.terminator {
            Terminator::Return => {
                // For void functions (unit return type), implicit return is OK.
                if is_unit_type(ctx.db, return_type) {
                    match write_unit_to_return_dest(ctx) {
                        Ok(value) => return (Ok(Some(value)), current_block_id),
                        Err(e) => return (Err(e), current_block_id),
                    }
                }
                // Non-void function reached end without ret - error.
                return (Err(InterpError::RuntimeError(
                    format!("Function '{}' reached end without ret statement",
                            context_name.text(ctx.db))
                )), current_block_id);
            }
            Terminator::Branch { condition_stmt, then_block, else_block } => {
                // Get the statement that caused the branch.
                let stmt = match cfg.get_stmt(ctx.db, *condition_stmt) {
                    Some(s) => s,
                    None => return (Err(InterpError::RuntimeError(
                        format!("Invalid condition stmt ID {:?}", condition_stmt)
                    )), current_block_id),
                };

                match stmt {
                    ast::Statement::If(if_s) => {
                        // If-statement: evaluate condition and branch based on result.
                        let condition_expr = if_s.condition(ctx.db);
                        let condition_dest = match get_destination_for_expr(ctx, condition_expr) {
                            Ok(d) => d,
                            Err(e) => return (Err(e), current_block_id),
                        };
                        if let Err(e) = eval_expression_frame(ctx, condition_expr, condition_dest) {
                            return (Err(e), current_block_id);
                        }
                        let condition_value = condition_dest.to_value();

                        // Handle condition based on type (bool, Option, or Result).
                        let is_true = match evaluate_branch_condition(ctx, condition_value, *if_s) {
                            Ok(b) => b,
                            Err(e) => return (Err(e), current_block_id),
                        };

                        // Mark condition temp as Moved (contents destroyed by evaluate_branch_condition).
                        mark_temp_slot_moved(ctx, condition_expr);

                        current_block_id = if is_true { *then_block } else { *else_block };
                    }
                    ast::Statement::Let(_) | ast::Statement::Var(_) => {
                        // Let/var-statement with try operator: branching decision already made.
                        // If we reached this point, the try succeeded (otherwise an error
                        // would have propagated). Go to then_block (continuation).
                        current_block_id = *then_block;
                    }
                    _ => {
                        return (Err(InterpError::RuntimeError(
                            "Branch terminator with unexpected statement type".to_string()
                        )), current_block_id);
                    }
                }
            }
            Terminator::Goto(next_block) => {
                // Process any block-exit drops before moving to next block.
                if let Err(e) = process_block_exit_drops(ctx, current_block_id) {
                    return (Err(e), current_block_id);
                }
                current_block_id = *next_block;
            }
            Terminator::TryReturn => {
                // Early return from ? operator - propagate.
                return (Err(InterpError::EarlyReturn), current_block_id);
            }
            Terminator::LoopContinue(header_block) => {
                // Jump to loop header for next iteration.
                current_block_id = *header_block;
            }
            Terminator::LoopBreak(exit_block) => {
                // Jump to after the loop.
                current_block_id = *exit_block;
            }
        }
    }
}

/// Execute a statement within CFG-based execution.
///
/// In CFG mode, if-statements don't execute their bodies here - the CFG
/// terminator handles branching. The condition is evaluated when handling
/// the Branch terminator.
fn execute_cfg_statement<'db>(
    ctx: &mut InterpContext<'db>,
    stmt: &ast::Statement<'db>,
) -> Result<CfgControl, InterpError> {
    match stmt {
        ast::Statement::Let(let_stmt) => {
            // Evaluate expression and store in frame slot.
            execute_let_statement_frame(ctx, *let_stmt)?;
            Ok(CfgControl::Continue)
        }
        ast::Statement::Var(var_stmt) => {
            // Same as let - evaluate expression and store in mutable slot.
            execute_var_statement_frame(ctx, *var_stmt)?;
            Ok(CfgControl::Continue)
        }
        ast::Statement::Set(set_stmt) => {
            // Mutate existing mutable slot.
            execute_set_statement_frame(ctx, *set_stmt)?;
            Ok(CfgControl::Continue)
        }
        ast::Statement::Ret(ret_stmt) => {
            // Handle bare ret (void function) vs ret with value.
            match ret_stmt.value(ctx.db) {
                Some(expr) => {
                    // Evaluate the return expression (no dest - value escapes frame).
                    // Note: @none/@error in return position require typed context from function return type.
                    let value = eval_return_expression_frame(ctx, expr)?;
                    Ok(CfgControl::Return(value))
                }
                None => {
                    // Bare ret in void function - return unit `()`.
                    let value = write_unit_to_return_dest(ctx)?;
                    Ok(CfgControl::Return(value))
                }
            }
        }
        ast::Statement::If(_) => {
            // In CFG mode, if-statements are handled by the Branch terminator.
            // We don't execute the body here - just continue.
            // The condition will be evaluated when we reach the Block's terminator.
            Ok(CfgControl::Continue)
        }
        ast::Statement::Loop(_) => {
            // In CFG mode, loops are handled by CFG structure.
            // The body is in a separate block; we just continue to the terminator.
            Ok(CfgControl::Continue)
        }
        ast::Statement::Break(_) | ast::Statement::Continue(_) => {
            // Break/continue are handled by CFG terminators.
            Ok(CfgControl::Continue)
        }
        ast::Statement::Fun(_) => {
            Err(InterpError::RuntimeError("Nested functions not supported".to_string()))
        }
        ast::Statement::Require(_) | ast::Statement::Import(_) => {
            // These are handled at the module level.
            Ok(CfgControl::Continue)
        }
        ast::Statement::ParseError(_) => {
            Err(InterpError::RuntimeError("Parse error in function".to_string()))
        }
    }
}

// ============================================================================
// Statement Execution
// ============================================================================

/// Execute a let statement in frame-based mode.
fn execute_let_statement_frame<'db>(
    ctx: &mut InterpContext<'db>,
    let_stmt: ast::StmtLet<'db>,
) -> Result<(), InterpError> {
    // Find destination slot using resolved slot ID.
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;
    let slot_info = match layout.get_slot_for_let_stmt(ctx.db, let_stmt) {
        Some(s) => s,
        None => {
            let name = let_stmt.name(ctx.db);
            return Err(InterpError::RuntimeError(
                format!("Let binding '{}' not found in frame", name.text(ctx.db))
            ));
        }
    };

    let slot_id = slot_info.slot_id(ctx.db);

    // Create destination from slot.
    let offset = slot_info.offset(ctx.db) as usize;
    let dest_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset) };
    let dest_tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);

    // Evaluate expression with DPS into slot.
    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
    eval_expression_frame(ctx, let_stmt.value(ctx.db), dest)?;

    // Mark slot as Available.
    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Available);

    Ok(())
}

/// Execute a var statement in frame-based mode.
///
/// Same as let, but allocates to a Mutable slot.
fn execute_var_statement_frame<'db>(
    ctx: &mut InterpContext<'db>,
    var_stmt: ast::StmtVar<'db>,
) -> Result<(), InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;
    let slot_info = match layout.get_slot_for_var_stmt(ctx.db, var_stmt) {
        Some(s) => s,
        None => {
            let name = var_stmt.name(ctx.db);
            return Err(InterpError::RuntimeError(
                format!("Var binding '{}' not found in frame", name.text(ctx.db))
            ));
        }
    };

    let slot_id = slot_info.slot_id(ctx.db);

    // Create destination from slot.
    let offset = slot_info.offset(ctx.db) as usize;
    let dest_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset) };
    let dest_tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);

    // Evaluate expression with DPS into slot.
    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
    eval_expression_frame(ctx, var_stmt.value(ctx.db), dest)?;

    // Mark slot as Available.
    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Available);

    Ok(())
}

/// Execute a set statement in frame-based mode.
///
/// Mutates an existing mutable slot. For linear types, destroys old value after
/// evaluating RHS (to handle self-referential cases like `set x = x + x`).
fn execute_set_statement_frame<'db>(
    ctx: &mut InterpContext<'db>,
    set_stmt: ast::StmtSet<'db>,
) -> Result<(), InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;
    let slot_info = match layout.get_slot_for_set_stmt(ctx.db, set_stmt) {
        Some(s) => s,
        None => {
            let name = set_stmt.name(ctx.db);
            return Err(InterpError::RuntimeError(
                format!("Variable '{}' not found in frame", name.text(ctx.db))
            ));
        }
    };

    let slot_id = slot_info.slot_id(ctx.db);
    let ty = slot_info.ty(ctx.db);
    // Use precomputed copyability from slot_info.
    let is_copy = slot_info.is_copy(ctx.db);
    let tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);

    if is_copy {
        // Copy type: evaluate directly to slot.
        let offset = slot_info.offset(ctx.db) as usize;
        let dest_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset) };
        let dest = Destination { ptr: dest_ptr, tydesc };
        eval_expression_frame(ctx, set_stmt.value(ctx.db), dest)?;
    } else {
        // Linear type: must handle self-reference (e.g., `set x = x + x`).
        // 1. Evaluate RHS to a temp buffer (reads old x value).
        // 2. Destroy old value in slot.
        // 3. Move new value from temp to slot.

        // Allocate temp buffer for the new value.
        let type_layout = crate::function_analysis::compute_datafun_type_layout(
            ctx.db,
            ty,
        );
        let mut temp_buffer = vec![0u8; type_layout.size as usize];
        let temp_ptr = temp_buffer.as_mut_ptr();

        // Evaluate RHS into temp.
        let temp_dest = Destination { ptr: temp_ptr, tydesc };
        eval_expression_frame(ctx, set_stmt.value(ctx.db), temp_dest)?;

        // Destroy old value in slot.
        let offset = slot_info.offset(ctx.db) as usize;
        let slot_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset) };
        let old_value = Value { ptr: slot_ptr, tydesc };
        destroy_value(ctx, old_value);

        // Move new value from temp to slot (memcpy, no clone/destroy needed).
        unsafe {
            std::ptr::copy_nonoverlapping(temp_ptr, slot_ptr, type_layout.size as usize);
        }

        // Temp buffer is dropped but the data was moved out, not destroyed.
        std::mem::forget(temp_buffer);
    }

    // Slot remains Available after set.
    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Available);

    Ok(())
}

/// Evaluate an expression in frame-based mode.
///
/// The result is written directly to `dest`.
fn eval_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // Find slot by resolved slot ID.
            let layout = ctx.call_stack[frame_index].layout;
            let slot_info = layout.get_slot_for_name_expr(ctx.db, expr)
                .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

            let slot_id = slot_info.slot_id(ctx.db);

            // Check slot state (debug-only - static analysis catches use-after-move).
            #[cfg(debug_assertions)]
            if ctx.call_stack[frame_index].get_slot_state(slot_id) == SlotState::Moved {
                return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
            }

            // Use precomputed copyability from slot_info.
            let is_copy = slot_info.is_copy(ctx.db);

            // Get cached tydesc for this slot.
            let tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);

            // Read value from slot.
            let kind = slot_info.kind(ctx.db);

            if kind == crate::function_analysis::SlotKind::Reference {
                // Reference slot: read pointer to caller's value.
                let ptr = read_reference_slot(&ctx.call_stack[frame_index], slot_info, ctx.db);
                let source_value = Value { ptr, tydesc };

                if is_copy {
                    // Copy: clone to dest.
                    clone_value_to_dest(ctx, source_value, dest);
                } else {
                    // Move: shallow copy to dest, mark slot as moved.
                    move_value_to_dest(source_value, dest);
                    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Moved);
                }
                Ok(())
            } else {
                // Local/Temporary slot.
                let offset = slot_info.offset(ctx.db) as usize;
                let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };
                let source_value = Value { ptr: frame_ptr, tydesc };

                if is_copy {
                    // For Copy types, clone to dest.
                    clone_value_to_dest(ctx, source_value, dest);
                } else {
                    // For Move types, move to dest, then mark source as moved.
                    // The move does a shallow copy (memcpy), transferring heap ownership.
                    move_value_to_dest(source_value, dest);
                    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Moved);
                }
                Ok(())
            }
        }

        ast::ExprFunKind::BinOp(binop_expr) => {
            let lhs_expr = binop_expr.lhs(ctx.db);
            let rhs_expr = binop_expr.rhs(ctx.db);

            // Evaluate operands (references for Names, temps for compound).
            let lhs = eval_operand(ctx, lhs_expr)?;
            let rhs = match eval_operand(ctx, rhs_expr) {
                Ok(r) => r,
                Err(e) => {
                    cleanup_operand(ctx, lhs);
                    return Err(e);
                }
            };

            // Execute binop directly to dest.
            // Aliasing (x = x + 1) is safe: arithmetic reads operands before writing.
            if let Err(e) = execute_binop(ctx, binop_expr.op(ctx.db), &lhs.value, &rhs.value, dest) {
                cleanup_operand(ctx, lhs);
                cleanup_operand(ctx, rhs);
                return Err(e);
            }

            // Cleanup compound operands.
            cleanup_operand(ctx, lhs);
            cleanup_operand(ctx, rhs);

            Ok(())
        }

        ast::ExprFunKind::FunctionCall(call_expr) => {
            // All functions return a value (void functions return unit).
            eval_function_call_frame(ctx, call_expr, dest)?;
            Ok(())
        }

        ast::ExprFunKind::UnaryOp(unary_expr) => {
            let operand_expr = unary_expr.operand(ctx.db);

            // Evaluate operand (reference for Name, temp for compound).
            let operand = eval_operand(ctx, operand_expr)?;

            // Execute unop directly to dest.
            if let Err(e) = execute_unop(ctx, unary_expr.op(ctx.db), &operand.value, dest) {
                cleanup_operand(ctx, operand);
                return Err(e);
            }

            // Cleanup compound operand.
            cleanup_operand(ctx, operand);

            Ok(())
        }

        ast::ExprFunKind::Tuple(tuple_expr) => {
            let dest_tydesc = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(dest.tydesc) };
            let elements = tuple_expr.elements(ctx.db);

            // Evaluate each element directly to its field offset in the tuple.
            for (elem_expr, field) in elements.iter().zip(dest_tydesc.iter_tuple_fields()) {
                let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
                let field_dest = Destination { ptr: field_ptr, tydesc: field.tydesc().as_ptr() };

                // Evaluate element directly to field destination.
                eval_expression_frame(ctx, *elem_expr, field_dest)?;

                // Element used its own temp slot, it's been written to our field now.
                // The element's temp slot is no longer needed.
                mark_temp_slot_available(ctx, *elem_expr);
            }

            Ok(())
        }

        ast::ExprFunKind::TryOption(try_op) => {
            // Evaluate operand to its temp slot.
            let operand_expr = try_op.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;
            eval_expression_frame(ctx, operand_expr, operand_dest)?;
            let operand = operand_dest.to_value();
            // Apply try-option operator with DPS.
            eval_try_option(ctx, operand, dest)?;
            Ok(())
        }

        ast::ExprFunKind::TryResult(try_op) => {
            // Evaluate operand to its temp slot.
            let operand_expr = try_op.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;
            eval_expression_frame(ctx, operand_expr, operand_dest)?;
            let operand = operand_dest.to_value();
            // Apply try-result operator with DPS.
            eval_try_result(ctx, operand, dest)?;
            Ok(())
        }

        // Inline literal variants - always write to dest.
        ast::ExprFunKind::True(_) => {
            write_bool_to_dest(dest, true);
            Ok(())
        }
        ast::ExprFunKind::False(_) => {
            write_bool_to_dest(dest, false);
            Ok(())
        }
        ast::ExprFunKind::None(_) => {
            write_option_none_to_dest(dest)?;
            Ok(())
        }
        ast::ExprFunKind::Int(int_expr) => {
            write_inline_int_to_dest(ctx, &int_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::Float(float_expr) => {
            let value_str = float_expr.value(ctx.db).as_str(ctx.db);
            let value: f32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse float: {}", e)))?;
            write_f32_to_dest(dest, value);
            Ok(())
        }
        ast::ExprFunKind::Hex(hex_expr) => {
            let value_str = hex_expr.value(ctx.db).as_str(ctx.db);
            let hex_digits = value_str.trim_start_matches("0x").trim_start_matches("0X");
            let value: u32 = u32::from_str_radix(hex_digits, 16)
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse hex: {}", e)))?;
            write_u32_to_dest(dest, value);
            Ok(())
        }
        ast::ExprFunKind::String(string_expr) => {
            write_string_to_dest(ctx, &string_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::List(list_expr) => {
            eval_inline_list(ctx, expr, &list_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::Set(set_expr) => {
            eval_inline_set(ctx, &set_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::Map(map_expr) => {
            eval_inline_map(ctx, &map_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::Tensor(_) => {
            Err(InterpError::InvalidExpression("Tensor not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonTuple(tuple_expr) => {
            eval_inline_anon_tuple(ctx, &tuple_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::AnonStruct(struct_expr) => {
            eval_inline_anon_struct(ctx, &struct_expr, dest)?;
            Ok(())
        }
        ast::ExprFunKind::AnonEnum(_) => {
            Err(InterpError::InvalidExpression("Enum not yet implemented".to_string()))
        }
        ast::ExprFunKind::Some(some_expr) => {
            eval_wrapper_payload_dps(ctx, WrapperKind::Some, dest, some_expr.payload(ctx.db))
        }
        ast::ExprFunKind::Ok(ok_expr) => {
            eval_wrapper_payload_dps(ctx, WrapperKind::Ok, dest, ok_expr.payload(ctx.db))
        }
        ast::ExprFunKind::Er(er_expr) => {
            let payload_expr = er_expr.payload(ctx.db);
            let payload_dest = get_destination_for_expr(ctx, payload_expr)?;
            eval_expression_frame(ctx, payload_expr, payload_dest)?;
            let payload = payload_dest.to_value();
            literals::write_result_er_from_value(ctx, payload, dest)?;
            Ok(())
        }
        ast::ExprFunKind::Data(data_expr) => {
            // Evaluate inner expression to its temp slot.
            let inner_expr = data_expr.value(ctx.db);
            let inner_dest = get_destination_for_expr(ctx, inner_expr)?;
            eval_expression_frame(ctx, inner_expr, inner_dest)?;
            let inner_value = inner_dest.to_value();
            // Wrap in Data (clones inner_value).
            write_data_to_dest(ctx, inner_value, dest)?;
            // We cloned inner for Data; destroy original and mark slot.
            destroy_value(ctx, inner_value);
            mark_temp_slot_moved(ctx, inner_expr);
            Ok(())
        }
        ast::ExprFunKind::Err(err_expr) => {
            // Evaluate inner expression to its temp slot.
            let inner_expr = err_expr.value(ctx.db);
            let inner_dest = get_destination_for_expr(ctx, inner_expr)?;
            eval_expression_frame(ctx, inner_expr, inner_dest)?;
            let inner_value = inner_dest.to_value();
            // Wrap in Error (clones inner_value).
            write_error_to_dest(ctx, inner_value, dest)?;
            // We cloned inner for Error; destroy original and mark slot.
            destroy_value(ctx, inner_value);
            mark_temp_slot_moved(ctx, inner_expr);
            Ok(())
        }
        ast::ExprFunKind::ParseError(_) => {
            Err(InterpError::InvalidExpression("Parse error in expression".to_string()))
        }
    }
}

/// Evaluate a return expression with destination from frame.
fn eval_return_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let return_dest = ctx.call_stack[frame_index].return_dest;

    // Evaluate expression with DPS into return destination.
    eval_expression_frame(ctx, expr, return_dest)?;

    // Return value pointing to caller's destination memory.
    Ok(return_dest.to_value())
}

/// Return unit `()` for void functions.
///
/// Void functions return unit type, so bare `ret` and implicit returns
/// need to return a unit value. Unit is a ZST (size 0), so no bytes are written.
fn write_unit_to_return_dest<'db>(
    ctx: &mut InterpContext<'db>,
) -> Result<Value, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let return_dest = ctx.call_stack[frame_index].return_dest;

    // Unit is a ZST (size 0). Nothing to write.
    Ok(return_dest.to_value())
}

