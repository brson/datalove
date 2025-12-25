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
pub mod tydesc;

pub use value::{Value, Destination};
pub use error::InterpError;
pub use frame::{SlotState, StackFrame};
pub use memory::{destroy_value, destroy_value_contents_only, free_value_structure};
pub use context::{InterpContext, ModuleFunctionTableGraph};
use control::{evaluate_branch_condition, eval_try_option, eval_try_result};

use frame::CfgControl;
use memory::{clone_value_to_dest, move_value_to_dest};
use types::is_copy_type;
use alloc::{write_result_err_to_dest, write_data_to_dest};
use collections::{write_map_from_values_to_dest, write_set_from_values_to_dest};
use literals::{
    write_inline_int_to_dest, write_option_none_to_dest,
    write_bool_to_dest, write_f32_to_dest, write_u32_to_dest,
    write_string_to_dest,
};
use arith_widening::{execute_binop, execute_unop};

use bct::text::InternedText;

use crate::module_graph::ModuleId;
use crate::ast;
use crate::function_analysis::{Terminator, BlockId};

// ============================================================================
// DPS Helpers for Wrapper Payloads and Compound Types
// ============================================================================

/// Get a destination for a specific tuple field from the tuple destination.
///
/// Returns the field destination with proper offset and tydesc.
pub(crate) fn get_tuple_field_dest(dest: Destination, field_index: usize) -> Option<Destination> {
    use datalove_rt::rtdt::{TyDescRef, TyDesc};

    let tuple_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let tuple_info = tuple_ref.tuple_info();

    tuple_info.field(field_index).map(|field| {
        let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
        let field_tydesc = field.tydesc().as_ptr() as *mut TyDesc;
        Destination { ptr: field_ptr, tydesc: field_tydesc }
    })
}

/// Get a destination for a specific struct field from the struct destination.
///
/// Returns the field destination with proper offset and tydesc.
/// Fields are accessed by index (assumes canonical sorted order).
pub(crate) fn get_struct_field_dest(dest: Destination, field_index: usize) -> Option<Destination> {
    use datalove_rt::rtdt::{TyDescRef, TyDesc};

    let struct_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let struct_info = struct_ref.struct_info();

    struct_info.field(field_index).map(|field| {
        let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
        let field_tydesc = field.tydesc().as_ptr() as *mut TyDesc;
        Destination { ptr: field_ptr, tydesc: field_tydesc }
    })
}

/// Create a destination for an Option's payload from the Option destination.
pub(crate) fn get_payload_dest_for_option(dest: Destination) -> Destination {
    use datalove_rt::rtdt::{TyDescRef, TyDesc, layout::compute_option_layout};

    let option_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let layout = compute_option_layout(option_ref);
    let payload_tydesc = option_ref.option_inner_ty().as_ptr() as *mut TyDesc;
    let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };

    Destination { ptr: payload_ptr, tydesc: payload_tydesc }
}

/// Create a destination for a Result's Ok payload from the Result destination.
pub(crate) fn get_ok_payload_dest_for_result(dest: Destination) -> Destination {
    use datalove_rt::rtdt::{TyDescRef, TyDesc, layout::compute_result_layout};

    let result_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
    let layout = compute_result_layout(result_ref);
    let payload_tydesc = result_ref.result_ok_ty().as_ptr() as *mut TyDesc;
    let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };

    Destination { ptr: payload_ptr, tydesc: payload_tydesc }
}

/// Wrapper kind for DPS evaluation of Some/Ok expressions.
#[derive(Copy, Clone)]
pub(crate) enum WrapperKind {
    Some,
    Ok,
}

/// Evaluate a Some or Ok expression with DPS.
///
/// Common logic for evaluating wrapper payloads directly into destination memory.
fn eval_wrapper_payload_dps<'db>(
    ctx: &mut InterpContext<'db>,
    kind: WrapperKind,
    dest: Destination,
    payload_expr: ast::ExprFun<'db>,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyTag, OptionTag, ResultTag};

    // Verify destination type and get payload setup.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    let (expected_tag, tag_value, payload_dest) = match kind {
        WrapperKind::Some => {
            if dest_tag != TyTag::Option {
                return Err(InterpError::RuntimeError(
                    format!("some requires Option destination, got {:?}", dest_tag)
                ));
            }
            (TyTag::Option, OptionTag::Some as u8, get_payload_dest_for_option(dest))
        }
        WrapperKind::Ok => {
            if dest_tag != TyTag::Result {
                return Err(InterpError::RuntimeError(
                    format!("ok requires Result destination, got {:?}", dest_tag)
                ));
            }
            (TyTag::Result, ResultTag::Ok as u8, get_ok_payload_dest_for_result(dest))
        }
    };
    let _ = expected_tag; // Used for error check above.

    // Write variant tag.
    unsafe { *(dest.ptr as *mut u8) = tag_value; }

    // Evaluate payload with DPS - writes directly to payload_dest.
    eval_expression_frame(ctx, payload_expr, payload_dest)?;

    Ok(())
}

// ============================================================================
// Function Calls and Execution
// ============================================================================

/// Look up a function by name in the module graph.
///
/// Returns the function definition and its source module.
/// Looks in this order:
/// 1. Current module functions (if executing inside a module)
/// 2. Imported module functions (from current module's imports)
/// 3. Globally imported functions
pub(super) fn lookup_function<'db>(
    ctx: &InterpContext<'db>,
    name: InternedText<'db>,
) -> Result<(ast::StmtFun<'db>, Option<ModuleId>), InterpError> {
    // Check module-based lookup (when executing inside a module).
    if let Some(current_module_id) = ctx.current_module_id {
        if let Some(module_funcs) = ctx.module_functions_graph.get_module_functions(current_module_id) {
            if let Some(&func) = module_funcs.get(&name) {
                return Ok((func, Some(current_module_id)));
            }
        }

        // Check what the current module imported from other modules.
        if let Some(typecheck_result) = &ctx.module_graph_typecheck {
            let module_imports_map = typecheck_result.module_imports(ctx.db);

            if let Some(imports) = module_imports_map.get(&current_module_id) {
                // Look for the function in the imports.
                for (local_name, source_module_id, _source_name) in imports.functions(ctx.db) {
                    if *local_name == name {
                        // Found the import - look up the function AST from the source module.
                        if let Some(module_funcs) = ctx.module_functions_graph.get_module_functions(*source_module_id) {
                            if let Some(&func_ast) = module_funcs.get(&name) {
                                return Ok((func_ast, Some(*source_module_id)));
                            }
                        }
                    }
                }
            }
        }
    }

    // Check globally imported functions.
    if let Some((func, module_id)) = ctx.module_functions_graph.get(name) {
        return Ok((func, Some(module_id)));
    }

    // Function not found.
    Err(InterpError::FunctionNotFound(name.text(ctx.db).to_string()))
}

/// Evaluate a function call from frame-based execution.
///
/// The return value is written directly to `return_dest`.
/// Returns `None` for void functions.
fn eval_function_call_frame<'db>(
    ctx: &mut InterpContext<'db>,
    call_expr: ast::ExprFunctionCall<'db>,
    return_dest: Destination,
) -> Result<Option<Value>, InterpError> {
    let name = call_expr.name(ctx.db);
    let arg_exprs = call_expr.args(ctx.db);

    // Look up the function (script or module).
    let (func, func_module) = lookup_function(ctx, name)?;

    let params = func.params(ctx.db);

    // Check argument count matches parameter count.
    if arg_exprs.len() != params.len() {
        return Err(InterpError::InvalidExpression(
            format!("Function '{}' expects {} arguments but {} provided",
                name.text(ctx.db), params.len(), arg_exprs.len())
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
    execute_function_body(ctx, func, func_module, arg_values, Some(return_dest))
}

/// Execute a function body and return its result.
///
/// This uses frame-based execution with analysis-driven slot allocation.
/// If `return_dest` is provided, return expressions write directly to caller's memory.
/// Returns `None` for void functions.
pub fn execute_function_body<'db>(
    ctx: &mut InterpContext<'db>,
    func: ast::StmtFun<'db>,
    func_module: Option<ModuleId>,
    arg_values: Vec<Value>,
    return_dest: Option<Destination>,
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
        slots: &[crate::function_analysis::SlotInfo<'_>],
        params: &[ast::FunParam<'_>],
        db: &dyn crate::Db,
    ) {
        for (i, arg_value) in arg_values.into_iter().enumerate() {
            if is_copy_type(arg_value) {
                // Copy types were cloned by the function, free the original structure.
                free_value_structure(ctx, arg_value);
            } else {
                // Non-copy types: check if they were moved (consumed by function).
                let param_name = params[i].name(db);
                if let Some(slot_info) = slots.iter().find(|s| s.name(db) == Some(param_name)) {
                    let slot_id = slot_info.slot_id(db);
                    if slot_states[slot_id.0 as usize] != SlotState::Moved {
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
    let analysis = match analyses.iter().find(|(f, _)| *f == func).map(|(_, a)| *a) {
        Some(a) => a,
        None => {
            restore_module_context(ctx, prev_module);
            cleanup_args_on_error(ctx, arg_values);
            return Err(InterpError::RuntimeError(
                format!("No analysis found for function '{}'", func.name(ctx.db).text(ctx.db))
            ));
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

    // Get frame layout, CFG, drop points, and tracked slots.
    let layout = analysis.frame_layout(ctx.db);
    let cfg = analysis.control_flow(ctx.db);
    let drop_points = analysis.drop_points(ctx.db);
    let tracked_slots = analysis.tracked_slots(ctx.db).clone();
    let total_size = layout.total_size(ctx.db) as usize;
    let slots = layout.slots(ctx.db);

    // Allocate frame data.
    let mut frame_data = vec![0u8; total_size];

    // Initialize slot states (all Uninitialized until written to).
    let mut slot_states = vec![SlotState::Uninitialized; slots.len()];

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

        let param_name = param.name(ctx.db);
        let arg_value = arg_values[i];

        // Find the slot for this parameter.
        let slot_info = match slots.iter().find(|s| s.name(ctx.db) == Some(param_name)) {
            Some(s) => s,
            None => {
                restore_module_context(ctx, prev_module);
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    format!("Parameter '{}' not found in frame layout", param_name.text(ctx.db))
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
        slot_states[slot_id.0 as usize] = SlotState::Available;
    }

    // Create and push the stack frame.
    let frame = StackFrame {
        frame_data,
        slot_states,
        func,
        layout,
        cfg,
        drop_points,
        tracked_slots,
        return_dest,
    };
    ctx.call_stack.push(frame);

    // Execute function body.
    let result = execute_function_body_with_frame(ctx);

    // Pop the frame and capture slot states for argument cleanup.
    let frame = ctx.call_stack.pop().unwrap();
    let final_slot_states = frame.slot_states.clone();

    // Handle return value based on whether we have a return_dest.
    let result = match result {
        Ok(Some(value)) if return_dest.is_some() => {
            // Return value was written directly to caller's memory via DPS.
            // No cloning needed - just return the value as-is.
            Ok(Some(value))
        }
        Ok(Some(_value)) => {
            // No return_dest but we got a return value.
            // This should be unreachable - all function calls provide DPS destinations.
            panic!(
                "Unreachable: return value without DPS destination in function '{}'",
                func.name(ctx.db).text(ctx.db)
            );
        }
        Ok(None) => {
            // Void function returned without value.
            Ok(None)
        }
        other => other,
    };

    // Clean up frame values before returning.
    cleanup_frame(ctx, frame);

    // Clean up arguments based on their final slot states.
    cleanup_args_after_frame(ctx, arg_values, &final_slot_states, slots, params, ctx.db);

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
                    if let Some(dest) = return_dest {
                        // DPS: write None to caller's destination.
                        write_option_none_to_dest(dest)?;
                        return Ok(Some(dest.to_value()));
                    } else {
                        // This path should be unreachable now that all function calls provide DPS destinations.
                        panic!(
                            "Unreachable: OptionNone early return without DPS destination in function '{}'",
                            func.name(ctx.db).text(ctx.db)
                        );
                    }
                }
            }
        }
        Err(InterpError::ResultErr { tydesc, ptr }) => {
            // Early return via ! operator - create Result::Err.
            if let Some(ret_type) = return_type {
                if matches!(ret_type.type_hint(ctx.db), TypeHint::Result(_)) {
                    if let Some(dest) = return_dest {
                        // DPS: write Err to caller's destination.
                        write_result_err_to_dest(dest, *tydesc, *ptr)?;
                        return Ok(Some(dest.to_value()));
                    } else {
                        // This path should be unreachable now that all function calls provide DPS destinations.
                        panic!(
                            "Unreachable: ResultErr early return without DPS destination in function '{}'",
                            func.name(ctx.db).text(ctx.db)
                        );
                    }
                }
            }
        }
        _ => {}
    }

    result
}


/// Execute function body with CFG-based execution.
fn execute_function_body_with_frame<'db>(
    ctx: &mut InterpContext<'db>,
) -> Result<Option<Value>, InterpError> {
    // Get the current frame (top of stack).
    let frame_index = ctx.call_stack.len() - 1;

    // Get the function and CFG from the frame.
    let func = ctx.call_stack[frame_index].func;
    let cfg = ctx.call_stack[frame_index].cfg;

    // Start at block 0 (entry block).
    let mut current_block_id = BlockId(0);

    loop {
        let block = cfg.get_block(ctx.db, current_block_id)
            .ok_or_else(|| InterpError::RuntimeError(
                format!("Invalid block ID {:?}", current_block_id)
            ))?;

        // Execute all statements in the current block.
        for stmt_id in &block.statements {
            let stmt = cfg.get_stmt(ctx.db, *stmt_id)
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Invalid stmt ID {:?}", stmt_id)
                ))?;

            // Execute statement.
            match execute_cfg_statement(ctx, stmt)? {
                CfgControl::Continue => continue,
                CfgControl::Return(value) => return Ok(Some(value)),
                CfgControl::ReturnVoid => return Ok(None),
            }
        }

        // Handle terminator.
        match &block.terminator {
            Terminator::Return => {
                // For void functions, implicit return at end of function is OK.
                if func.return_type(ctx.db).is_none() {
                    return Ok(None);
                }
                // Non-void function reached end without ret - error.
                return Err(InterpError::RuntimeError(
                    format!("Function '{}' reached end without ret statement",
                            func.name(ctx.db).text(ctx.db))
                ));
            }
            Terminator::Branch { condition_stmt, then_block, else_block } => {
                // Get the statement that caused the branch.
                let stmt = cfg.get_stmt(ctx.db, *condition_stmt)
                    .ok_or_else(|| InterpError::RuntimeError(
                        format!("Invalid condition stmt ID {:?}", condition_stmt)
                    ))?;

                match stmt {
                    ast::Statement::If(if_s) => {
                        // If-statement: evaluate condition and branch based on result.
                        let condition_expr = if_s.condition(ctx.db);
                        let condition_dest = get_destination_for_expr(ctx, condition_expr)?;
                        eval_expression_frame(ctx, condition_expr, condition_dest)?;
                        let condition_value = condition_dest.to_value();

                        // Handle condition based on type (bool, Option, or Result).
                        let is_true = evaluate_branch_condition(
                            ctx,
                            condition_value,
                            if_s.then_binding(ctx.db),
                            if_s.else_binding(ctx.db),
                        )?;

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
                        return Err(InterpError::RuntimeError(
                            "Branch terminator with unexpected statement type".to_string()
                        ));
                    }
                }
            }
            Terminator::Goto(next_block) => {
                // Process any block-exit drops before moving to next block.
                process_block_exit_drops(ctx, current_block_id)?;
                current_block_id = *next_block;
            }
            Terminator::TryReturn => {
                // Early return from ? operator - propagate.
                return Err(InterpError::EarlyReturn);
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
                    // Bare ret in void function.
                    Ok(CfgControl::ReturnVoid)
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

/// Process block-exit drops when leaving a block via Goto.
///
/// This handles branch convergence: when a slot is moved in one branch but not another,
/// we drop it at the exit of the branch where it's not moved. This ensures the slot
/// is "consumed" on all paths to the join point.
fn process_block_exit_drops<'db>(
    ctx: &mut InterpContext<'db>,
    block_id: BlockId,
) -> Result<(), InterpError> {
    use crate::function_analysis::{DropReason, DropLocation};

    let frame_index = ctx.call_stack.len() - 1;
    let drop_points = ctx.call_stack[frame_index].drop_points;
    let layout = ctx.call_stack[frame_index].layout;
    let tracked_slots = ctx.call_stack[frame_index].tracked_slots.clone();
    let slots = layout.slots(ctx.db);

    // Build a map from SlotId to slot_index for quick lookup.
    let slot_id_to_index: std::collections::HashMap<_, _> = slots.iter()
        .enumerate()
        .map(|(idx, slot)| (slot.slot_id(ctx.db), idx))
        .collect();

    // Find all drops for this block exit.
    for drop_point in drop_points.drops(ctx.db) {
        // Only process BlockExit drops for THIS block.
        let DropLocation::BlockExit(exit_block) = drop_point.location(ctx.db) else {
            continue;
        };
        if exit_block != block_id {
            continue;
        }

        // Only process BranchExit drops.
        if drop_point.reason(ctx.db) != DropReason::BranchExit {
            continue;
        }

        let slot_id = drop_point.slot_id(ctx.db);

        // Find the slot info and index.
        let Some(&slot_index) = slot_id_to_index.get(&slot_id) else {
            continue;
        };
        let slot_info = &slots[slot_index];

        // For tracked slots (conditional init or move), check runtime state.
        // For non-tracked slots, static analysis guarantees correctness.
        if tracked_slots.contains(&slot_id) {
            if ctx.call_stack[frame_index].slot_states[slot_index] != SlotState::Available {
                continue;
            }
        }

        // Destroy the slot contents.
        let frame_data = &ctx.call_stack[frame_index].frame_data;
        let offset = slot_info.offset(ctx.db) as usize;
        let ty = slot_info.ty(ctx.db);

        let datalit_ty = match ty.ty(ctx.db) {
            crate::tycheck::Type::Datalit(dt) => dt.clone(),
            _ => continue,
        };
        let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);
        let slot_ptr = unsafe { frame_data.as_ptr().add(offset) as *mut u8 };

        let value = Value { ptr: slot_ptr, tydesc };
        destroy_value_contents_only(ctx, value);

        // Mark slot as Moved so cleanup_frame doesn't try to drop it again.
        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Moved;
    }

    Ok(())
}

/// Clean up a stack frame using analysis-computed drop points.
///
/// Uses drop_points as source of truth, combined with runtime slot_states
/// to handle conditional moves. Drop points identify non-copy, initialized, owned
/// slots that need cleanup. Runtime slot_states filter out slots that were actually
/// moved at runtime (handling conditional branches).
///
/// Temporaries are handled inline during evaluation:
/// - BinOp/UnaryOp operands: marked Moved after destroy_value
/// - If-condition temps: marked Moved after evaluate_branch_condition
/// - Return value temps: marked Moved after heap clone
fn cleanup_frame<'db>(
    ctx: &mut InterpContext<'db>,
    frame: StackFrame<'db>,
) {
    use crate::function_analysis::{DropReason, DropLocation};

    let layout = frame.layout;
    let slots = layout.slots(ctx.db);
    let drop_points = frame.drop_points;
    let tracked_slots = &frame.tracked_slots;

    // Build a map from SlotId to slot_index for quick lookup.
    let slot_id_to_index: std::collections::HashMap<_, _> = slots.iter()
        .enumerate()
        .map(|(idx, slot)| (slot.slot_id(ctx.db), idx))
        .collect();

    // Process drop points from the analysis.
    for drop_point in drop_points.drops(ctx.db) {
        let slot_id = drop_point.slot_id(ctx.db);

        // Only process actual drops, not markers for moved/uninitialized slots.
        // BranchExit drops are processed inline during CFG execution, not here.
        match drop_point.reason(ctx.db) {
            DropReason::EndOfScope | DropReason::EarlyReturn => {
                // This slot needs cleanup at function exit.
            }
            DropReason::BranchExit => {
                // BranchExit drops are handled inline during CFG execution.
                // Skip them here.
                continue;
            }
            DropReason::Moved | DropReason::Uninitialized => {
                // These are informational - no actual drop needed.
                continue;
            }
        }

        // Only process AfterStmt drops here; BlockExit drops are handled inline.
        if matches!(drop_point.location(ctx.db), DropLocation::BlockExit(_)) {
            continue;
        }

        // Find the slot info and index.
        let Some(&slot_index) = slot_id_to_index.get(&slot_id) else {
            // Slot not found in layout - shouldn't happen.
            continue;
        };
        let slot_info = &slots[slot_index];

        // For tracked slots (conditional init or move), check runtime state.
        // For non-tracked slots, static analysis guarantees correctness.
        if tracked_slots.contains(&slot_id) {
            if frame.slot_states[slot_index] != SlotState::Available {
                continue;
            }
        }

        destroy_slot_contents(ctx, slot_info, &frame.frame_data);
    }
}

/// Destroy the contents of a slot (helper for cleanup_frame).
fn destroy_slot_contents<'db>(
    ctx: &mut InterpContext<'db>,
    slot_info: &crate::function_analysis::SlotInfo<'db>,
    frame_data: &[u8],
) {
    let offset = slot_info.offset(ctx.db) as usize;

    // Get the type descriptor for this slot.
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => {
            // Skip non-datalit types.
            return;
        }
    };
    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

    let slot_ptr = unsafe { frame_data.as_ptr().add(offset) as *mut u8 };

    // Destroy the value contents only (not the structure itself).
    // The memory is part of the frame buffer and will be freed with the frame.
    let value = Value { ptr: slot_ptr, tydesc };
    destroy_value_contents_only(ctx, value);
}

// ============================================================================
// Frame-based execution helpers
// ============================================================================

/// Get a Destination for an expression's temporary slot.
fn get_destination_for_expr<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Destination, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    let slot_info = layout.get_temp_slot_for_expr(ctx.db, expr)
        .ok_or_else(|| InterpError::RuntimeError(
            "No temp slot allocated for expression".to_string()
        ))?;

    let offset = slot_info.offset(ctx.db) as usize;
    let ptr = unsafe {
        ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset)
    };
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => return Err(InterpError::RuntimeError(
            "Non-datalit type in temp slot".to_string()
        )),
    };
    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);
    Ok(Destination { ptr, tydesc })
}

/// Mark a temporary slot as Available after writing a value to it.
fn mark_temp_slot_available<'db>(ctx: &mut InterpContext<'db>, expr: ast::ExprFun<'db>) {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    if let Some(slot_info) = layout.get_temp_slot_for_expr(ctx.db, expr) {
        let slot_id = slot_info.slot_id(ctx.db);
        ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;
    }
}

/// Mark a temporary slot as Moved after its contents have been destroyed.
///
/// This prevents cleanup_frame from trying to destroy the slot again.
fn mark_temp_slot_moved<'db>(ctx: &mut InterpContext<'db>, expr: ast::ExprFun<'db>) {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    if let Some(slot_info) = layout.get_temp_slot_for_expr(ctx.db, expr) {
        let slot_id = slot_info.slot_id(ctx.db);
        ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
    }
}

/// Read a pointer value from a Reference slot (parameter).
fn read_reference_slot<'db>(
    frame: &StackFrame<'db>,
    slot_info: crate::function_analysis::SlotInfo<'db>,
    db: &'db dyn crate::Db,
) -> *mut u8 {
    let offset = slot_info.offset(db) as usize;
    let ptr_bytes = &frame.frame_data[offset..offset + std::mem::size_of::<usize>()];
    let ptr_value = usize::from_ne_bytes(ptr_bytes.try_into().unwrap());
    ptr_value as *mut u8
}

/// An operand value for binop/unop evaluation.
///
/// For variable references, points directly to the variable's slot (no clone).
/// For compound expressions, points to a temp slot holding the evaluated result.
struct Operand<'db> {
    value: Value,
    /// Whether cleanup is needed after use (true for compound exprs, false for variable refs).
    needs_cleanup: bool,
    /// The expression, used for marking temp slot as moved during cleanup.
    expr: crate::ast::ExprFun<'db>,
}

/// Evaluate an operand for binop/unop.
///
/// For variable references, returns a Value pointing directly to the slot (no clone).
/// For compound expressions, evaluates to a temp slot and returns a Value pointing there.
fn eval_operand<'db>(
    ctx: &mut InterpContext<'db>,
    expr: crate::ast::ExprFun<'db>,
) -> Result<Operand<'db>, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    match expr.expr(ctx.db) {
        crate::ast::ExprFunKind::Name(name) => {
            // Find slot by resolved slot ID.
            let layout = ctx.call_stack[frame_index].layout;
            let slot_info = layout.get_slot_for_name_expr(ctx.db, expr)
                .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

            let slot_id = slot_info.slot_id(ctx.db);

            // Check slot state (debug-only).
            #[cfg(debug_assertions)]
            if ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] == SlotState::Moved {
                return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
            }

            // Get type info.
            let ty = slot_info.ty(ctx.db);
            let datalit_ty = match ty.ty(ctx.db) {
                crate::tycheck::Type::Datalit(dt) => dt.clone(),
                _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
            };
            let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

            // Get pointer to value (reference or local slot).
            let ptr = if slot_info.kind(ctx.db) == crate::function_analysis::SlotKind::Reference {
                read_reference_slot(&ctx.call_stack[frame_index], slot_info, ctx.db)
            } else {
                let offset = slot_info.offset(ctx.db) as usize;
                unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 }
            };

            // Return Value pointing to slot. DO NOT mark as moved.
            Ok(Operand {
                value: Value { ptr, tydesc },
                needs_cleanup: false,
                expr,
            })
        }
        _ => {
            // Compound expression: evaluate to its temp slot.
            let temp_dest = get_destination_for_expr(ctx, expr)?;
            eval_expression_frame(ctx, expr, temp_dest)?;
            mark_temp_slot_available(ctx, expr);
            Ok(Operand {
                value: temp_dest.to_value(),
                needs_cleanup: true,
                expr,
            })
        }
    }
}

/// Cleanup an operand after use if needed.
fn cleanup_operand<'db>(ctx: &mut InterpContext<'db>, operand: Operand<'db>) {
    if operand.needs_cleanup {
        destroy_value(ctx, operand.value);
        mark_temp_slot_moved(ctx, operand.expr);
    }
}

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
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => {
            return Err(InterpError::RuntimeError(
                format!("Non-datalit type in slot '{}'", let_stmt.name(ctx.db).text(ctx.db))
            ));
        }
    };
    let dest_tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

    // Evaluate expression with DPS into slot.
    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
    eval_expression_frame(ctx, let_stmt.value(ctx.db), dest)?;

    // Mark slot as Available.
    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;

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
    let ty = slot_info.ty(ctx.db);
    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => {
            return Err(InterpError::RuntimeError(
                format!("Non-datalit type in slot '{}'", var_stmt.name(ctx.db).text(ctx.db))
            ));
        }
    };
    let dest_tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

    // Evaluate expression with DPS into slot.
    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
    eval_expression_frame(ctx, var_stmt.value(ctx.db), dest)?;

    // Mark slot as Available.
    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;

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
    let is_copy = crate::function_analysis::is_copy_type(ctx.db, ty);

    let datalit_ty = match ty.ty(ctx.db) {
        crate::tycheck::Type::Datalit(dt) => dt.clone(),
        _ => {
            return Err(InterpError::RuntimeError(
                format!("Non-datalit type in slot '{}'", set_stmt.name(ctx.db).text(ctx.db))
            ));
        }
    };
    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

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
    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;

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
            if ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] == SlotState::Moved {
                return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
            }

            // Check copyability.
            let ty = slot_info.ty(ctx.db);
            let is_copy = crate::function_analysis::is_copy_type(ctx.db, ty);

            // Read value from slot.
            let kind = slot_info.kind(ctx.db);

            if kind == crate::function_analysis::SlotKind::Reference {
                // Reference slot: read pointer to caller's value.
                let ptr = read_reference_slot(&ctx.call_stack[frame_index], slot_info, ctx.db);

                // Get tydesc from slot's type info (not from the data pointer).
                let datalit_ty = match ty.ty(ctx.db) {
                    crate::tycheck::Type::Datalit(dt) => dt.clone(),
                    _ => return Err(InterpError::RuntimeError("Non-datalit type in reference slot".to_string())),
                };
                let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

                let source_value = Value { ptr, tydesc };

                if is_copy {
                    // Copy: clone to dest.
                    clone_value_to_dest(ctx, source_value, dest);
                } else {
                    // Move: shallow copy to dest, mark slot as moved.
                    move_value_to_dest(source_value, dest);
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                }
                Ok(())
            } else {
                // Local/Temporary slot.
                if is_copy {
                    // For Copy types, clone to dest.
                    let offset = slot_info.offset(ctx.db) as usize;
                    let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };

                    let datalit_ty = match ty.ty(ctx.db) {
                        crate::tycheck::Type::Datalit(dt) => dt.clone(),
                        _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
                    };
                    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

                    let source_value = Value { ptr: frame_ptr, tydesc };
                    clone_value_to_dest(ctx, source_value, dest);
                } else {
                    // For Move types, move to dest, then mark source as moved.
                    // The move does a shallow copy (memcpy), transferring heap ownership.
                    let offset = slot_info.offset(ctx.db) as usize;
                    let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };

                    let datalit_ty = match ty.ty(ctx.db) {
                        crate::tycheck::Type::Datalit(dt) => dt.clone(),
                        _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
                    };
                    let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

                    let source_value = Value { ptr: frame_ptr, tydesc };

                    // Move to destination (shallow copy), mark source as Moved.
                    move_value_to_dest(source_value, dest);
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
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
            // Void functions can't be used in expression context (typechecker ensures this).
            eval_function_call_frame(ctx, call_expr, dest)?
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Void function '{}' cannot be used in expression context",
                            call_expr.name(ctx.db).text(ctx.db))
                ))?;
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
        ast::ExprFunKind::Err(_) => {
            Err(InterpError::InvalidExpression(
                "@error literal requires type context".to_string()
            ))
        }
        ast::ExprFunKind::ParseError(_) => {
            Err(InterpError::InvalidExpression("Parse error in expression".to_string()))
        }
    }
}

/// Evaluate a return expression with destination from frame.
///
/// The frame must have a `return_dest`; all callers now provide one.
fn eval_return_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    // Check if we have a return destination from caller.
    if let Some(return_dest) = ctx.call_stack[frame_index].return_dest {
        // Evaluate expression with DPS into return destination.
        eval_expression_frame(ctx, expr, return_dest)?;

        // Return value pointing to caller's destination memory.
        return Ok(return_dest.to_value());
    }

    // No return_dest - this was the "script scope" fallback path.
    // Script scope was removed; all callers now provide return_dest.
    unreachable!("return_dest should always be Some - script scope was removed");
}

// ============================================================================
// Collection Expression Evaluation
// ============================================================================

/// Evaluate inline list expression with DPS.
///
/// Writes the List struct directly to dest. The list's internal data buffer
/// is still heap-allocated via the runtime allocator.
fn eval_inline_list<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    list_expr: &ast::ExprList<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    use crate::tycheck::Type;
    use crate::datalit::tycheck::Type as DatalitType;

    let elements = list_expr.elements(ctx.db);

    // Get element type from typechecker.
    let type_and_heap = ctx.get_expr_type(expr).ok_or_else(|| {
        InterpError::RuntimeError("List type not found in typechecker (compiler bug)".to_string())
    })?;

    let Type::Datalit(DatalitType::List(list_type)) = type_and_heap.ty(ctx.db) else {
        return Err(InterpError::RuntimeError(
            "Expected List type (compiler bug)".to_string()
        ));
    };

    let elem_ty = list_type.element_type(ctx.db);
    let elem_tydesc = ctx.tydesc_table.get_or_create(elem_ty.ty(ctx.db));

    eval_list_with_element_tydesc(ctx, elements, elem_tydesc, dest)
}

/// Evaluate list elements with known element tydesc, using DPS.
///
/// Writes the List struct to dest.ptr. The internal data buffer is heap-allocated.
fn eval_list_with_element_tydesc<'db>(
    ctx: &mut InterpContext<'db>,
    elements: &[ast::ExprFun<'db>],
    element_tydesc: *const datalove_rt::rtdt::TyDesc,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::List;
    use datalove_rt::c::RtStatus;

    let rt_handle = ctx.runtime.handle();
    let list_ptr = dest.ptr;

    // Initialize empty list at dest.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(rt_handle, list_ptr, dest.tydesc)
    };
    if status != RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create list".to_string()));
    }

    if elements.is_empty() {
        return Ok(());
    }

    // Reserve capacity for all elements (allocates data buffer via runtime allocator).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(rt_handle, list_ptr, dest.tydesc, elements.len() as u32)
    };
    if status != RtStatus::Ok {
        unsafe {
            datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, dest.tydesc);
        }
        return Err(InterpError::RuntimeError("Failed to reserve list capacity".to_string()));
    }

    let element_size = unsafe { (*element_tydesc).size as usize };

    // Evaluate each element with DPS into the list buffer.
    for (i, elem) in elements.iter().enumerate() {
        // Get pointer to element slot in list's data buffer.
        let data_ptr = unsafe { (*(list_ptr as *const List)).data as *mut u8 };
        let elem_dest_ptr = unsafe { data_ptr.add(i * element_size) };
        let elem_dest = Destination { ptr: elem_dest_ptr, tydesc: element_tydesc };

        match eval_expression_frame(ctx, *elem, elem_dest) {
            Ok(()) => {
                // Update list size.
                unsafe {
                    let list = list_ptr as *mut List;
                    (*list).size = (i + 1) as u32;
                }
            }
            Err(e) => {
                // Destroy already-written elements and the list.
                // The list's destroy will clean up all elements and the data buffer.
                unsafe {
                    datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, dest.tydesc);
                }
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Evaluate inline set expression with DPS.
fn eval_inline_set<'db>(
    ctx: &mut InterpContext<'db>,
    set_expr: &ast::ExprSet<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    let elements = set_expr.elements(ctx.db);
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        let elem_dest = get_destination_for_expr(ctx, *elem)?;
        match eval_expression_frame(ctx, *elem, elem_dest) {
            Ok(()) => values.push(elem_dest.to_value()),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    write_set_from_values_to_dest(ctx, values, dest)
}

/// Evaluate inline map expression with DPS.
fn eval_inline_map<'db>(
    ctx: &mut InterpContext<'db>,
    map_expr: &ast::ExprMap<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    let entries = map_expr.entries(ctx.db);
    let mut kv_pairs = Vec::with_capacity(entries.len());

    for entry in entries {
        let key_expr = entry.key(ctx.db);
        let key_dest = get_destination_for_expr(ctx, key_expr)?;
        if let Err(e) = eval_expression_frame(ctx, key_expr, key_dest) {
            for (k, v) in kv_pairs {
                destroy_value(ctx, k);
                destroy_value(ctx, v);
            }
            return Err(e);
        }
        let key = key_dest.to_value();

        let value_expr = entry.value(ctx.db);
        let value_dest = get_destination_for_expr(ctx, value_expr)?;
        if let Err(e) = eval_expression_frame(ctx, value_expr, value_dest) {
            destroy_value(ctx, key);
            for (k, v) in kv_pairs {
                destroy_value(ctx, k);
                destroy_value(ctx, v);
            }
            return Err(e);
        }
        let value = value_dest.to_value();

        kv_pairs.push((key, value));
    }

    write_map_from_values_to_dest(ctx, kv_pairs, dest)
}

/// Evaluate inline anonymous tuple expression with DPS.
fn eval_inline_anon_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    tuple_expr: &ast::ExprAnonTuple<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag};

    let elements = tuple_expr.elements(ctx.db);

    // DPS path: if dest is a tuple with matching field count, write directly.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    if dest_tag == TyTag::Tuple {
        let tuple_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let tuple_info = tuple_ref.tuple_info();

        if tuple_info.num_fields() as usize == elements.len() {
            // DPS: evaluate each element directly into its field slot.
            for (i, elem) in elements.iter().enumerate() {
                let field_dest = get_tuple_field_dest(dest, i)
                    .expect("field index should be valid");

                if let Err(e) = eval_expression_frame(ctx, *elem, field_dest) {
                    // Clean up already-written fields.
                    for j in 0..i {
                        let written_field = get_tuple_field_dest(dest, j)
                            .expect("field index should be valid");
                        destroy_value(ctx, written_field.to_value());
                    }
                    return Err(e);
                }
            }

            return Ok(());
        }
    }

    // Fallback was for type mismatch - but dest should always match the expression type.
    unreachable!(
        "eval_inline_anon_tuple: dest type mismatch - expected Tuple with {} fields, got {:?}",
        elements.len(),
        dest_tag
    );
}

/// Evaluate inline anonymous struct expression with DPS.
fn eval_inline_anon_struct<'db>(
    ctx: &mut InterpContext<'db>,
    struct_expr: &ast::ExprAnonStruct<'db>,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag};

    let expr_fields = struct_expr.fields(ctx.db);
    let mut sorted_fields: Vec<_> = expr_fields.iter()
        .map(|f| (f.name(ctx.db), f.value(ctx.db)))
        .collect();
    sorted_fields.sort_by_key(|(name, _)| name.as_str(ctx.db));

    // DPS path: if dest is a struct with matching field count, write directly.
    // Both expression fields and dest fields are in canonical sorted order.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    if dest_tag == TyTag::Struct {
        let struct_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let struct_info = struct_ref.struct_info();

        if struct_info.num_fields() as usize == sorted_fields.len() {
            // DPS: evaluate each field directly into its slot.
            for (i, (_name, value_expr)) in sorted_fields.iter().enumerate() {
                let field_dest = get_struct_field_dest(dest, i)
                    .expect("field index should be valid");

                if let Err(e) = eval_expression_frame(ctx, *value_expr, field_dest) {
                    // Clean up already-written fields.
                    for j in 0..i {
                        let written_field = get_struct_field_dest(dest, j)
                            .expect("field index should be valid");
                        destroy_value(ctx, written_field.to_value());
                    }
                    return Err(e);
                }
            }

            return Ok(());
        }
    }

    // Fallback was for type mismatch - but dest should always match the expression type.
    unreachable!(
        "eval_inline_anon_struct: dest type mismatch - expected Struct with {} fields, got {:?}",
        sorted_fields.len(),
        dest_tag
    );
}

