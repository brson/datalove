//! Analysis-driven interpreter with linear type semantics.
//!
//! # Overview
//!
//! The interpreter executes Datafun code after typecheck and function analysis.
//! It operates in two modes: script scope for top-level statements, and frame-based
//! execution for function bodies. Both modes enforce linear type semantics and use
//! destination-passing style (DPS) to minimize heap allocations.
//!
//! # Execution Flow
//!
//! 1. **Entry**: [`execute_script_with_module_graph`] receives a typechecked script
//! 2. **Preparation**: Imports are resolved, functions are analyzed for frame layout
//! 3. **Execution**: Script units execute sequentially; each statement processes in
//!    script scope, while function calls create stack frames
//! 4. **Result**: Script returns via the `output` variable binding
//!
//! # Two Execution Modes
//!
//! **Script scope** (`ScriptScope`): Evaluates top-level `let` bindings and `fun`
//! definitions. Variables are stored by name in a hashmap with move-state tracking.
//! Linear values are moved on use; copy values are cloned.
//!
//! **Frame-based** (`StackFrame`): Evaluates function bodies. The `function_analysis`
//! module computes a `FrameLayout` mapping each variable and temporary to a byte
//! offset in a packed buffer. The `ControlFlowGraph` (CFG) drives execution through
//! basic blocks with explicit terminators for branches, loops, and returns.
//!
//! # Value Ownership Model
//!
//! The interpreter separates two ownership concepts:
//!
//! **Structural ownership** (`ValueOwnership`): Who frees the memory structure.
//! - `Borrowed`: Points into frame buffer or caller's data. Never freed by holder.
//! - `TempOwned`: Heap allocation that must be freed after use.
//!
//! **Semantic ownership** (move vs copy): Determined by type (`is_copy_type`) and
//! `SlotState` tracking. Copy types clone transparently; linear types are moved.
//!
//! # Linear Type Semantics
//!
//! Copy types (bool, fixed integers, f32) clone transparently. Linear types (int,
//! string, collections) use move semantics. `SlotState` tracks each slot:
//! - `Uninitialized`: Not yet written; skip cleanup
//! - `Available`: Contains valid data; needs cleanup if not moved
//! - `Moved`: Ownership transferred; skip cleanup
//!
//! Operators use borrow semantics (`eval_*_borrow` functions): operands are cloned
//! for the operation, leaving originals intact. Variables consumed by function args
//! or `ret` are marked `Moved`.
//!
//! # Destination-Passing Style
//!
//! Expressions accept an optional `Destination` specifying where to write results.
//! This avoids allocating temporaries when the caller already has storage. Frame
//! slots serve as destinations for let bindings and expression temporaries.
//!
//! # Control Flow
//!
//! Frame execution walks the CFG: execute statements in a block, then follow the
//! terminator. Terminators handle:
//! - `Return`: Function exit
//! - `Branch`: If-statement condition evaluation
//! - `Goto`: Unconditional jump (with branch-exit drops for convergence)
//! - `LoopContinue`/`LoopBreak`: Loop control
//!
//! # Cleanup
//!
//! `DropPoints` from function analysis identifies slots needing cleanup. At function
//! exit, `cleanup_frame` destroys slots still in `Available` state. Branch-exit
//! drops handle convergence: when a slot is moved in one branch but not another,
//! the non-moved branch drops it at exit so both paths converge with the slot
//! consumed.
//!
//! # Key Types
//!
//! - [`InterpContext`]: Runtime state, module graph, call stack, type descriptor table
//! - [`Value`]: Pointer + type descriptor + ownership location
//! - [`StackFrame`]: Packed byte buffer, per-slot state, CFG, drop points
//! - [`ScriptScope`]: Named variable bindings with move tracking
//! - [`Destination`]: Target location for DPS expression evaluation

mod value;
mod error;
mod frame;
mod memory;
mod types;
mod alloc;
mod arith;
mod arith_widening;
mod collections;
mod coerce;
mod literals;
mod context;
mod control;
pub mod tydesc;
mod script;

pub use value::{Value, Destination, ValueOwnership, EvalContext};
pub use error::InterpError;
pub use frame::{SlotState, StackFrame};
pub use memory::{destroy_value, destroy_value_contents_only, free_value_structure};
pub use context::{
    InterpContext, ScriptScope, ModuleFunctionTableGraph,
    ScriptVariable, ScriptVarState, ScriptResult,
};
pub use script::{execute_script_with_module_graph, execute_script_unit, pretty_print_value};
use control::{find_slot_by_name, evaluate_branch_condition, eval_try_option, eval_try_result};
use tydesc::type_hint_to_tydesc;

use frame::CfgControl;
use memory::{clone_value_to_dest, move_value_to_dest};
use types::is_copy_type;
use alloc::{
    allocate_option_some_from_value,
    allocate_result_ok_from_value, write_result_err_to_dest,
};
use collections::{
    allocate_tuple_from_values, allocate_struct_from_values,
    allocate_map_from_values, allocate_set_from_values,
};
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

use script::eval_expression_in_script_scope;

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
/// Used by both frame and script scope evaluation.
pub(crate) fn eval_wrapper_payload_dps<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    kind: WrapperKind,
    dest: Option<Destination>,
    payload_expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyTag, OptionTag, ResultTag};

    let Some(dest) = dest else {
        let name = match kind {
            WrapperKind::Some => "some",
            WrapperKind::Ok => "ok",
        };
        return Err(InterpError::RuntimeError(
            format!("{} expression requires type context (use type hint)", name)
        ));
    };

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

    // Evaluate payload with DPS based on context.
    let payload_value = match eval_ctx {
        EvalContext::Frame => eval_expression_frame(ctx, payload_expr, payload_dest)?,
        EvalContext::ScriptScope => {
            script::eval_expression_in_script_scope(ctx, payload_expr, Some(payload_dest))?
        }
    };

    // Handle case where operation didn't use dest (e.g., bigint ops).
    if payload_value.ownership == ValueOwnership::TempOwned {
        let size = unsafe { (*payload_value.tydesc).size as usize };
        unsafe {
            std::ptr::copy_nonoverlapping(payload_value.ptr, payload_dest.ptr, size);
        }
        memory::free_value_structure(ctx, payload_value);
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

// ============================================================================
// Function Calls and Execution
// ============================================================================

/// Look up a function by name in script scope or imported modules.
///
/// Returns the function definition and its source module (if from a module).
/// Looks in this order:
/// 1. Script-level functions
/// 2. Current module functions (if executing inside a module)
/// 3. Imported module functions (from current module's imports)
/// 4. Script-level imported functions
pub(super) fn lookup_function<'db>(
    ctx: &InterpContext<'db>,
    name: InternedText<'db>,
) -> Result<(ast::StmtFun<'db>, Option<ModuleId>), InterpError> {
    // First check script scope.
    if let Some(&func) = ctx.script_scope.functions.get(&name) {
        return Ok((func, None));
    }

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

    // Check script-level imported functions.
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
        let value = match eval_expression_frame(ctx, *arg_expr, arg_dest) {
            Ok(v) => {
                if v.ownership == ValueOwnership::Borrowed {
                    mark_temp_slot_available(ctx, *arg_expr);
                }
                v
            }
            Err(e) => {
                // Clean up previously evaluated arguments on error.
                for val in arg_values {
                    destroy_value(ctx, val);
                }
                return Err(e);
            }
        };
        arg_values.push(value);
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
                    } else if arg_value.ownership == ValueOwnership::TempOwned {
                        // Slot was moved: contents were consumed by function.
                        // Free the structure since it's TempOwned (caller allocated it).
                        free_value_structure(ctx, arg_value);
                    }
                    // Borrowed values have their structures owned elsewhere.
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

    // Get function analysis.
    // First check script-level function analyses, then ModuleGraph-based.
    let analysis = if let Some(analysis) = ctx.script_function_analyses.get(&func) {
        *analysis
    } else if let Some(typecheck_result) = &ctx.module_graph_typecheck {
        let analyses = typecheck_result.function_analyses(ctx.db);
        match analyses.iter().find(|(f, _)| *f == func).map(|(_, a)| *a) {
            Some(a) => a,
            None => {
                restore_module_context(ctx, prev_module);
                cleanup_args_on_error(ctx, arg_values);
                return Err(InterpError::RuntimeError(
                    format!("No analysis found for function '{}'", func.name(ctx.db).text(ctx.db))
                ));
            }
        }
    } else {
        restore_module_context(ctx, prev_module);
        cleanup_args_on_error(ctx, arg_values);
        return Err(InterpError::RuntimeError(
            "No typecheck result available - cannot execute function".to_string()
        ));
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
        Ok(Some(value)) if value.ownership == ValueOwnership::Borrowed => {
            // No return_dest and result is Borrowed (pointing to frame memory).
            // This path should be unreachable now that all function calls provide DPS destinations.
            panic!(
                "Unreachable: Borrowed return value without DPS destination in function '{}'",
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
                        return write_option_none_to_dest(dest).map(Some);
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
                        return write_result_err_to_dest(dest, *tydesc, *ptr).map(Some);
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
                        let condition_value = eval_expression_frame(ctx, condition_expr, condition_dest)?;

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
                    ast::Statement::Let(_) => {
                        // Let-statement with try operator: branching decision already made.
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

        let value = Value {
            ptr: slot_ptr,
            tydesc,
            ownership: ValueOwnership::Borrowed,
        };
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
    let value = Value {
        ptr: slot_ptr,
        tydesc,
        ownership: ValueOwnership::Borrowed,
    };
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

/// Write a value to a Local or Temporary slot.
fn write_value_to_slot<'db>(
    frame: &mut StackFrame<'db>,
    slot_info: crate::function_analysis::SlotInfo<'db>,
    value: Value,
    db: &'db dyn crate::Db,
) -> Result<(), InterpError> {
    let offset = slot_info.offset(db) as usize;
    let size = unsafe { (*value.tydesc).size as usize };

    unsafe {
        // Copy the actual value bytes into the slot.
        std::ptr::copy_nonoverlapping(
            value.ptr,
            frame.frame_data.as_mut_ptr().add(offset),
            size
        );
    }

    Ok(())
}

/// Execute a let statement in frame-based mode.
fn execute_let_statement_frame<'db>(
    ctx: &mut InterpContext<'db>,
    let_stmt: ast::StmtLet<'db>,
) -> Result<(), InterpError> {
    // Find destination slot FIRST so we can pass it to expression evaluation.
    let frame_index = ctx.call_stack.len() - 1;
    let name = let_stmt.name(ctx.db);
    let layout = ctx.call_stack[frame_index].layout;
    let slot_info = match find_slot_by_name(ctx.db, layout, name) {
        Some(s) => s,
        None => {
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
                format!("Non-datalit type in slot '{}'", name.text(ctx.db))
            ));
        }
    };
    let dest_tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

    // Evaluate expression with DPS into slot.
    let dest = Destination { ptr: dest_ptr, tydesc: dest_tydesc };
    let value = eval_expression_frame(ctx, let_stmt.value(ctx.db), dest)?;

    // If DPS was used (Borrowed), the value was written directly to slot.
    // If not (TempOwned), we need to write and free.
    if value.ownership == ValueOwnership::TempOwned {
        // Write value to slot.
        if let Err(e) = write_value_to_slot(&mut ctx.call_stack[frame_index], slot_info, value, ctx.db) {
            destroy_value(ctx, value);
            return Err(e);
        }
        // Free the heap-allocated value structure after copying to frame.
        free_value_structure(ctx, value);
    }

    // Mark slot as Available.
    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;

    Ok(())
}

/// Evaluate an expression in frame-based mode.
///
/// The result is written directly to `dest` and a Borrowed value is returned.
fn eval_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // Find slot by name.
            let layout = ctx.call_stack[frame_index].layout;
            let slot_info = find_slot_by_name(ctx.db, layout, name)
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

                if is_copy {
                    // Copy: clone to dest.
                    let borrowed = Value { ptr, tydesc, ownership: ValueOwnership::Borrowed };
                    let result = clone_value_to_dest(ctx, borrowed, dest);
                    Ok(result)
                } else {
                    // Move: take ownership of the reference.
                    // Return as Borrowed because caller owns the structure memory.
                    // Contents will be destroyed by consumer, structure freed by caller cleanup.
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                    Ok(Value { ptr, tydesc, ownership: ValueOwnership::Borrowed })
                }
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

                    let source_value = Value { ptr: frame_ptr, tydesc, ownership: ValueOwnership::Borrowed };
                    let result = clone_value_to_dest(ctx, source_value, dest);
                    Ok(result)
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

                    let source_value = Value { ptr: frame_ptr, tydesc, ownership: ValueOwnership::Borrowed };

                    // Move to destination (shallow copy), mark source as Moved.
                    let result = move_value_to_dest(source_value, dest);
                    ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
                    Ok(result)
                }
            }
        }

        ast::ExprFunKind::BinOp(binop_expr) => {
            // Get temp slot destinations for subexpressions.
            let lhs_expr = binop_expr.lhs(ctx.db);
            let rhs_expr = binop_expr.rhs(ctx.db);
            let lhs_dest = get_destination_for_expr(ctx, lhs_expr)?;
            let rhs_dest = get_destination_for_expr(ctx, rhs_expr)?;

            // Evaluate lhs in borrow context (binops don't consume operands).
            let lhs = eval_expression_frame_borrow(ctx, lhs_expr, lhs_dest)?;

            // Evaluate rhs in borrow context.
            let rhs = match eval_expression_frame_borrow(ctx, rhs_expr, rhs_dest) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };

            // Execute binop with borrowed operands.
            execute_binop(ctx, binop_expr.op(ctx.db), &lhs, &rhs, dest)?;

            // Clean up temporary operand values and mark their slots as Moved.
            destroy_value(ctx, lhs);
            if lhs.ownership == ValueOwnership::Borrowed {
                mark_temp_slot_moved(ctx, lhs_expr);
            }
            destroy_value(ctx, rhs);
            if rhs.ownership == ValueOwnership::Borrowed {
                mark_temp_slot_moved(ctx, rhs_expr);
            }

            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
        }

        ast::ExprFunKind::FunctionCall(call_expr) => {
            // Void functions can't be used in expression context (typechecker ensures this).
            eval_function_call_frame(ctx, call_expr, dest)?
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Void function '{}' cannot be used in expression context",
                            call_expr.name(ctx.db).text(ctx.db))
                ))
        }

        ast::ExprFunKind::UnaryOp(unary_expr) => {
            // Get temp slot for operand.
            let operand_expr = unary_expr.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;

            // Evaluate operand in borrow context (unary ops don't consume operands).
            let operand = eval_expression_frame_borrow(ctx, operand_expr, operand_dest)?;

            // Execute unop with borrowed operand.
            execute_unop(ctx, unary_expr.op(ctx.db), &operand, dest)?;

            // Clean up temporary operand value and mark slot as Moved.
            destroy_value(ctx, operand);
            if operand.ownership == ValueOwnership::Borrowed {
                mark_temp_slot_moved(ctx, operand_expr);
            }

            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
        }

        ast::ExprFunKind::Tuple(tuple_expr) => {
            let dest_tydesc = unsafe { datalove_rt::rtdt::TyDescRef::from_ptr(dest.tydesc) };
            let elements = tuple_expr.elements(ctx.db);

            // Evaluate each element directly to its field offset in the tuple.
            for (elem_expr, field) in elements.iter().zip(dest_tydesc.iter_tuple_fields()) {
                let field_ptr = unsafe { dest.ptr.add(field.offset() as usize) };
                let field_dest = Destination { ptr: field_ptr, tydesc: field.tydesc().as_ptr() };

                // Evaluate element directly to field destination.
                let elem_value = eval_expression_frame(ctx, *elem_expr, field_dest)?;

                // If element used its own temp slot, it's been written to our field now.
                // The element's temp slot is no longer needed.
                if elem_value.ownership == ValueOwnership::Borrowed {
                    mark_temp_slot_available(ctx, *elem_expr);
                }
            }

            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
        }

        ast::ExprFunKind::TryOption(try_op) => {
            // Evaluate operand to its temp slot.
            let operand_expr = try_op.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;
            let operand = eval_expression_frame(ctx, operand_expr, operand_dest)?;
            // Apply try-option operator.
            eval_try_option(ctx, operand)
        }

        ast::ExprFunKind::TryResult(try_op) => {
            // Evaluate operand to its temp slot.
            let operand_expr = try_op.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;
            let operand = eval_expression_frame(ctx, operand_expr, operand_dest)?;
            // Apply try-result operator.
            eval_try_result(ctx, operand)
        }

        // Inline literal variants - always write to dest.
        ast::ExprFunKind::True(_) => write_bool_to_dest(dest, true),
        ast::ExprFunKind::False(_) => write_bool_to_dest(dest, false),
        ast::ExprFunKind::None(_) => write_option_none_to_dest(dest),
        ast::ExprFunKind::Int(int_expr) => write_inline_int_to_dest(ctx, &int_expr, dest),
        ast::ExprFunKind::Float(float_expr) => {
            let value_str = float_expr.value(ctx.db).as_str(ctx.db);
            let value: f32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse float: {}", e)))?;
            write_f32_to_dest(dest, value)
        }
        ast::ExprFunKind::Hex(hex_expr) => {
            let value_str = hex_expr.value(ctx.db).as_str(ctx.db);
            let hex_digits = value_str.trim_start_matches("0x").trim_start_matches("0X");
            let value: u32 = u32::from_str_radix(hex_digits, 16)
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse hex: {}", e)))?;
            write_u32_to_dest(dest, value)
        }
        ast::ExprFunKind::String(string_expr) => write_string_to_dest(ctx, &string_expr, dest),
        ast::ExprFunKind::List(list_expr) => {
            eval_inline_list(ctx, EvalContext::Frame, expr, &list_expr, Some(dest))
        }
        ast::ExprFunKind::Set(set_expr) => {
            eval_inline_set(ctx, EvalContext::Frame, &set_expr, Some(dest))
        }
        ast::ExprFunKind::Map(map_expr) => {
            eval_inline_map(ctx, EvalContext::Frame, &map_expr, Some(dest))
        }
        ast::ExprFunKind::Tensor(_) => {
            Err(InterpError::InvalidExpression("Tensor not yet implemented".to_string()))
        }
        ast::ExprFunKind::AnonTuple(tuple_expr) => {
            eval_inline_anon_tuple(ctx, EvalContext::Frame, &tuple_expr, Some(dest))
        }
        ast::ExprFunKind::AnonStruct(struct_expr) => {
            eval_inline_anon_struct(ctx, EvalContext::Frame, &struct_expr, Some(dest))
        }
        ast::ExprFunKind::AnonEnum(_) => {
            Err(InterpError::InvalidExpression("Enum not yet implemented".to_string()))
        }
        ast::ExprFunKind::Some(some_expr) => {
            eval_wrapper_payload_dps(ctx, EvalContext::Frame, WrapperKind::Some, Some(dest), some_expr.payload(ctx.db))
        }
        ast::ExprFunKind::Ok(ok_expr) => {
            eval_wrapper_payload_dps(ctx, EvalContext::Frame, WrapperKind::Ok, Some(dest), ok_expr.payload(ctx.db))
        }
        ast::ExprFunKind::Er(er_expr) => {
            let payload_expr = er_expr.payload(ctx.db);
            let payload_dest = get_destination_for_expr(ctx, payload_expr)?;
            let payload = eval_expression_frame(ctx, payload_expr, payload_dest)?;
            literals::write_result_er_from_value(ctx, payload, dest)
        }
        ast::ExprFunKind::Data(data_expr) => {
            // Evaluate inner expression to its temp slot.
            let inner_expr = data_expr.value(ctx.db);
            let inner_dest = get_destination_for_expr(ctx, inner_expr)?;
            let inner_value = eval_expression_frame(ctx, inner_expr, inner_dest)?;
            // Wrap in Data (clones inner_value if borrowed).
            let result = alloc::allocate_data_from_value(ctx, inner_value)?;
            // Clean up original inner value (allocate_data_from_value cloned it).
            if inner_value.ownership == ValueOwnership::Borrowed {
                destroy_value(ctx, inner_value);
                mark_temp_slot_moved(ctx, inner_expr);
            }
            Ok(result)
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
/// If the frame has a `return_dest`, the expression is evaluated and written to
/// caller's memory, with coercion if needed. Otherwise, falls back to heap allocation.
fn eval_return_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let frame_index = ctx.call_stack.len() - 1;

    // Check if we have a return destination from caller.
    if let Some(return_dest) = ctx.call_stack[frame_index].return_dest {
        // Evaluate expression with DPS into return destination.
        let value = eval_expression_frame(ctx, expr, return_dest)?;

        // Handle case where expression didn't use dest (e.g., Name returning borrowed value).
        if value.ptr != return_dest.ptr {
            let size = unsafe { (*value.tydesc).size as usize };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    value.ptr,
                    return_dest.ptr,
                    size,
                );
            }
            if value.ownership == ValueOwnership::TempOwned {
                free_value_structure(ctx, value);
            }
        }

        // Return as Borrowed - caller owns the destination memory.
        return Ok(Value {
            ptr: return_dest.ptr,
            tydesc: return_dest.tydesc,
            ownership: ValueOwnership::Borrowed,
        });
    }

    // No return_dest - fall back to heap allocation (script scope path).
    // Get the function's return type to check if wrapping/coercion is needed.
    let func = ctx.call_stack[frame_index].func;
    let return_type = func.return_type(ctx.db);

    // Check if expression needs typed context (some/ok/er/none/error).
    let needs_typed_dest = matches!(
        expr.expr(ctx.db),
        ast::ExprFunKind::None(_) | ast::ExprFunKind::Err(_) |
        ast::ExprFunKind::Some(_) | ast::ExprFunKind::Ok(_) | ast::ExprFunKind::Er(_)
    );

    if needs_typed_dest {
        // Get the function's return type to provide as destination.
        if let Some(ret_type) = return_type {
            let ret_tydesc = type_hint_to_tydesc(ctx, ret_type);
            let ret_ptr = unsafe {
                datalove_rt::c::dtlv_rti_mem_alloc_local(
                    ctx.runtime.handle(),
                    ret_tydesc,
                    1
                )
            };
            if ret_ptr.is_null() {
                return Err(InterpError::RuntimeError("Failed to allocate return buffer".to_string()));
            }

            let dest = Destination { ptr: ret_ptr, tydesc: ret_tydesc };
            // Evaluate expression with the typed destination.
            let result = eval_expression_frame(ctx, expr, dest);
            if let Err(e) = result {
                // Free the allocated buffer on error to avoid leaks.
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        ctx.runtime.handle(),
                        ret_tydesc,
                        1,
                        ret_ptr
                    );
                }
                return Err(e);
            }
            let value = result.unwrap();

            // Convert Borrowed to TempOwned since this escapes the frame.
            if value.ownership == ValueOwnership::Borrowed && value.ptr == ret_ptr {
                return Ok(Value { ptr: ret_ptr, tydesc: ret_tydesc, ownership: ValueOwnership::TempOwned });
            } else {
                return Ok(value);
            }
        }
    }

    // Evaluate expression to its temp slot.
    let expr_dest = get_destination_for_expr(ctx, expr)?;
    let value = eval_expression_frame(ctx, expr, expr_dest)?;

    // Check if we need to wrap the value in Option/Result for return type coercion.
    // This handles the case where return_dest is None (e.g., nested function calls).
    if let Some(ret_type) = return_type {
        use crate::datalit::ast::TypeHint;
        match ret_type.type_hint(ctx.db) {
            TypeHint::Option(_) => {
                // Check if value needs wrapping (not already an Option).
                let value_tag = unsafe { (*value.tydesc).type_tag };
                if value_tag != TyTag::Option {
                    // Wrap value in Some.
                    return allocate_option_some_from_value(ctx, value);
                }
            }
            TypeHint::Result(_) => {
                // Check if value needs wrapping (not already a Result).
                let value_tag = unsafe { (*value.tydesc).type_tag };
                if value_tag != TyTag::Result {
                    // Wrap value in Ok.
                    return allocate_result_ok_from_value(ctx, value);
                }
            }
            _ => {}
        }
    }

    Ok(value)
}

// ============================================================================
// Borrow Context Evaluation
// ============================================================================

/// Evaluate an expression in borrow context (for binop/unop operands).
///
/// In borrow context, linear type variables are cloned instead of moved.
/// This implements the ref semantics for operator arguments.
fn eval_expression_frame_borrow<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    match expr.expr(ctx.db) {
        ast::ExprFunKind::Name(name) => {
            // Find slot by name.
            let layout = ctx.call_stack[frame_index].layout;
            let slot_info = find_slot_by_name(ctx.db, layout, name)
                .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

            let slot_id = slot_info.slot_id(ctx.db);

            // Check slot state (debug-only - static analysis catches use-after-move).
            #[cfg(debug_assertions)]
            if ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] == SlotState::Moved {
                return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
            }

            // Get type info for the slot.
            let ty = slot_info.ty(ctx.db);
            let datalit_ty = match ty.ty(ctx.db) {
                crate::tycheck::Type::Datalit(dt) => dt.clone(),
                _ => return Err(InterpError::RuntimeError("Non-datalit type in slot".to_string())),
            };
            let tydesc = ctx.tydesc_table.get_or_create(&datalit_ty);

            // In borrow context, ALWAYS clone to destination (never move).
            // This is the key difference from regular evaluation.
            let kind = slot_info.kind(ctx.db);

            if kind == crate::function_analysis::SlotKind::Reference {
                // Reference slot: read pointer to caller's value.
                let ptr = read_reference_slot(&ctx.call_stack[frame_index], slot_info, ctx.db);
                let borrowed = Value { ptr, tydesc, ownership: ValueOwnership::Borrowed };

                let result = clone_value_to_dest(ctx, borrowed, dest);
                // Note: We do NOT mark slot as Moved - this is borrow context.
                Ok(result)
            } else {
                // Local/Temporary slot - clone to destination.
                let offset = slot_info.offset(ctx.db) as usize;
                let frame_ptr = unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 };
                let source_value = Value { ptr: frame_ptr, tydesc, ownership: ValueOwnership::Borrowed };

                let result = clone_value_to_dest(ctx, source_value, dest);
                // Note: We do NOT mark slot as Moved - this is borrow context.
                Ok(result)
            }
        }

        // For nested binops/unops, stay in borrow context.
        ast::ExprFunKind::BinOp(binop_expr) => {
            let lhs_expr = binop_expr.lhs(ctx.db);
            let rhs_expr = binop_expr.rhs(ctx.db);
            let lhs_dest = get_destination_for_expr(ctx, lhs_expr)?;
            let rhs_dest = get_destination_for_expr(ctx, rhs_expr)?;

            let lhs = eval_expression_frame_borrow(ctx, lhs_expr, lhs_dest)?;

            let rhs = match eval_expression_frame_borrow(ctx, rhs_expr, rhs_dest) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs);
                    return Err(e);
                }
            };

            // Execute binop with borrowed operands.
            execute_binop(ctx, binop_expr.op(ctx.db), &lhs, &rhs, dest)?;

            // Clean up temporary operand values and mark slots as Moved.
            destroy_value(ctx, lhs);
            if lhs.ownership == ValueOwnership::Borrowed {
                mark_temp_slot_moved(ctx, lhs_expr);
            }
            destroy_value(ctx, rhs);
            if rhs.ownership == ValueOwnership::Borrowed {
                mark_temp_slot_moved(ctx, rhs_expr);
            }

            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
        }

        ast::ExprFunKind::UnaryOp(unary_expr) => {
            let operand_expr = unary_expr.operand(ctx.db);
            let operand_dest = get_destination_for_expr(ctx, operand_expr)?;

            let operand = eval_expression_frame_borrow(ctx, operand_expr, operand_dest)?;

            // Execute unop with borrowed operand.
            execute_unop(ctx, unary_expr.op(ctx.db), &operand, dest)?;

            // Clean up temporary operand value and mark slot as Moved.
            destroy_value(ctx, operand);
            if operand.ownership == ValueOwnership::Borrowed {
                mark_temp_slot_moved(ctx, operand_expr);
            }

            Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
        }

        // For function calls in borrow context, the call itself uses normal semantics
        // (arguments may be moved depending on parameter modes).
        ast::ExprFunKind::FunctionCall(call_expr) => {
            // Void functions can't be used in expression context (typechecker ensures this).
            eval_function_call_frame(ctx, call_expr, dest)?
                .ok_or_else(|| InterpError::RuntimeError(
                    format!("Void function '{}' cannot be used in expression context",
                            call_expr.name(ctx.db).text(ctx.db))
                ))
        }

        // For other expressions (literals, etc.), delegate to normal evaluation.
        // These don't involve variable access so borrow vs move doesn't matter.
        _ => eval_expression_frame(ctx, expr, dest),
    }
}

// ============================================================================
// Unified Expression Evaluation
// ============================================================================

/// Evaluate an expression in the given context.
///
/// This is the unified entry point for expression evaluation that dispatches
/// to context-specific implementations for variable lookup while sharing
/// code for literals and operations.
fn eval_expression<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    expr: ast::ExprFun<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    match eval_ctx {
        EvalContext::ScriptScope => eval_expression_in_script_scope(ctx, expr, dest),
        EvalContext::Frame => {
            // Frame context always has destinations (temp slots allocated at analysis time).
            let dest = dest.expect("Frame context requires destination");
            eval_expression_frame(ctx, expr, dest)
        }
    }
}

/// Evaluate inline list expression in the given context.
///
/// Uses type information when available to get element tydesc upfront,
/// enabling DPS optimization for element evaluation.
pub(super) fn eval_inline_list<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    expr: ast::ExprFun<'db>,
    list_expr: &ast::ExprList<'db>,
    _dest: Option<Destination>,
) -> Result<Value, InterpError> {
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

    eval_list_with_element_tydesc(ctx, eval_ctx, elements, elem_tydesc)
}

/// Evaluate list elements with known element tydesc, using DPS.
fn eval_list_with_element_tydesc<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    elements: &[ast::ExprFun<'db>],
    element_tydesc: *const datalove_rt::rtdt::TyDesc,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::List;
    use datalove_rt::c::RtStatus;

    let list_tydesc = ctx.tydesc_table.create_list_from_element_tydesc(element_tydesc);
    let rt_handle = ctx.runtime.handle();

    // Allocate list struct.
    let list_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, list_tydesc, 1)
    };
    if list_ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate list".to_string()));
    }

    // Initialize empty list.
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_create_local(rt_handle, list_ptr, list_tydesc)
    };
    if status != RtStatus::Ok {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, list_tydesc, 1, list_ptr);
        }
        return Err(InterpError::RuntimeError("Failed to create list".to_string()));
    }

    if elements.is_empty() {
        return Ok(Value {
            ptr: list_ptr,
            tydesc: list_tydesc,
            ownership: ValueOwnership::TempOwned,
        });
    }

    // Reserve capacity for all elements (allocates data buffer via runtime allocator).
    let status = unsafe {
        datalove_rt::c::dtlv_rti_list_reserve_local(rt_handle, list_ptr, list_tydesc, elements.len() as u32)
    };
    if status != RtStatus::Ok {
        unsafe {
            datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, list_tydesc);
            datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, list_tydesc, 1, list_ptr);
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

        match eval_expression(ctx, eval_ctx, *elem, Some(elem_dest)) {
            Ok(value) => {
                if value.ownership == ValueOwnership::TempOwned {
                    // Expression didn't use dest - copy result and free.
                    unsafe {
                        std::ptr::copy_nonoverlapping(value.ptr, elem_dest_ptr, element_size);
                    }
                    free_value_structure(ctx, value);
                }
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
                    datalove_rt::c::dtlv_rti_list_destroy_local(rt_handle, list_ptr, list_tydesc);
                    datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, list_tydesc, 1, list_ptr);
                }
                return Err(e);
            }
        }
    }

    Ok(Value {
        ptr: list_ptr,
        tydesc: list_tydesc,
        ownership: ValueOwnership::TempOwned,
    })
}

/// Evaluate inline set expression in the given context.
pub(super) fn eval_inline_set<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    set_expr: &ast::ExprSet<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let elements = set_expr.elements(ctx.db);
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        // In frame context, use temp slots for elements.
        let elem_dest = match eval_ctx {
            EvalContext::Frame => Some(get_destination_for_expr(ctx, *elem)?),
            EvalContext::ScriptScope => None,
        };
        match eval_expression(ctx, eval_ctx, *elem, elem_dest) {
            Ok(v) => values.push(v),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    let set_value = allocate_set_from_values(ctx, values)?;

    // If dest provided, copy result there and return as Borrowed.
    if let Some(d) = dest {
        let size = unsafe { (*set_value.tydesc).size as usize };
        unsafe { std::ptr::copy_nonoverlapping(set_value.ptr, d.ptr, size); }
        free_value_structure(ctx, set_value);
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, ownership: ValueOwnership::Borrowed })
    } else {
        Ok(set_value)
    }
}

/// Evaluate inline map expression in the given context.
pub(super) fn eval_inline_map<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    map_expr: &ast::ExprMap<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    let entries = map_expr.entries(ctx.db);
    let mut kv_pairs = Vec::with_capacity(entries.len());

    for entry in entries {
        let key_expr = entry.key(ctx.db);
        // In frame context, use temp slots for keys.
        let key_dest = match eval_ctx {
            EvalContext::Frame => Some(get_destination_for_expr(ctx, key_expr)?),
            EvalContext::ScriptScope => None,
        };
        let key = match eval_expression(ctx, eval_ctx, key_expr, key_dest) {
            Ok(v) => v,
            Err(e) => {
                for (k, v) in kv_pairs {
                    destroy_value(ctx, k);
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        };

        let value_expr = entry.value(ctx.db);
        // In frame context, use temp slots for values.
        let value_dest = match eval_ctx {
            EvalContext::Frame => Some(get_destination_for_expr(ctx, value_expr)?),
            EvalContext::ScriptScope => None,
        };
        let value = match eval_expression(ctx, eval_ctx, value_expr, value_dest) {
            Ok(v) => v,
            Err(e) => {
                destroy_value(ctx, key);
                for (k, v) in kv_pairs {
                    destroy_value(ctx, k);
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        };

        kv_pairs.push((key, value));
    }

    let map_value = allocate_map_from_values(ctx, kv_pairs)?;

    // If dest provided, copy result there and return as Borrowed.
    if let Some(d) = dest {
        let size = unsafe { (*map_value.tydesc).size as usize };
        unsafe { std::ptr::copy_nonoverlapping(map_value.ptr, d.ptr, size); }
        free_value_structure(ctx, map_value);
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, ownership: ValueOwnership::Borrowed })
    } else {
        Ok(map_value)
    }
}

/// Evaluate inline anonymous tuple expression in the given context.
pub(super) fn eval_inline_anon_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    tuple_expr: &ast::ExprAnonTuple<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag};

    let elements = tuple_expr.elements(ctx.db);

    // DPS path: if dest is a tuple with matching field count, write directly.
    if let Some(dest) = dest {
        let dest_tag = unsafe { (*dest.tydesc).type_tag };
        if dest_tag == TyTag::Tuple {
            let tuple_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
            let tuple_info = tuple_ref.tuple_info();

            if tuple_info.num_fields() as usize == elements.len() {
                // DPS: evaluate each element directly into its field slot.
                for (i, elem) in elements.iter().enumerate() {
                    let field_dest = get_tuple_field_dest(dest, i)
                        .expect("field index should be valid");

                    match eval_expression(ctx, eval_ctx, *elem, Some(field_dest)) {
                        Ok(field_value) => {
                            // Handle case where operation didn't use dest.
                            if field_value.ownership == ValueOwnership::TempOwned {
                                let size = unsafe { (*field_value.tydesc).size as usize };
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        field_value.ptr,
                                        field_dest.ptr,
                                        size,
                                    );
                                }
                                free_value_structure(ctx, field_value);
                            }
                        }
                        Err(e) => {
                            // Clean up already-written fields.
                            for j in 0..i {
                                let written_field = get_tuple_field_dest(dest, j)
                                    .expect("field index should be valid");
                                let field_value = Value {
                                    ptr: written_field.ptr,
                                    tydesc: written_field.tydesc,
                                    ownership: ValueOwnership::Borrowed,
                                };
                                destroy_value(ctx, field_value);
                            }
                            return Err(e);
                        }
                    }
                }

                return Ok(Value {
                    ptr: dest.ptr,
                    tydesc: dest.tydesc,
                    ownership: ValueOwnership::Borrowed,
                });
            }
        }
    }

    // Fallback: no dest or type mismatch - allocate new tuple.
    let mut values = Vec::with_capacity(elements.len());

    for elem in elements {
        // In frame context, use temp slots for elements.
        let elem_dest = match eval_ctx {
            EvalContext::Frame => Some(get_destination_for_expr(ctx, *elem)?),
            EvalContext::ScriptScope => None,
        };
        match eval_expression(ctx, eval_ctx, *elem, elem_dest) {
            Ok(v) => values.push(v),
            Err(e) => {
                for v in values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    allocate_tuple_from_values(ctx, values)
}

/// Evaluate inline anonymous struct expression in the given context.
pub(super) fn eval_inline_anon_struct<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext,
    struct_expr: &ast::ExprAnonStruct<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag};

    let expr_fields = struct_expr.fields(ctx.db);
    let mut sorted_fields: Vec<_> = expr_fields.iter()
        .map(|f| (f.name(ctx.db), f.value(ctx.db)))
        .collect();
    sorted_fields.sort_by_key(|(name, _)| name.as_str(ctx.db));

    // DPS path: if dest is a struct with matching field count, write directly.
    // Both expression fields and dest fields are in canonical sorted order.
    if let Some(dest) = dest {
        let dest_tag = unsafe { (*dest.tydesc).type_tag };
        if dest_tag == TyTag::Struct {
            let struct_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
            let struct_info = struct_ref.struct_info();

            if struct_info.num_fields() as usize == sorted_fields.len() {
                // DPS: evaluate each field directly into its slot.
                for (i, (_name, value_expr)) in sorted_fields.iter().enumerate() {
                    let field_dest = get_struct_field_dest(dest, i)
                        .expect("field index should be valid");

                    match eval_expression(ctx, eval_ctx, *value_expr, Some(field_dest)) {
                        Ok(field_value) => {
                            // Handle case where operation didn't use dest.
                            if field_value.ownership == ValueOwnership::TempOwned {
                                let size = unsafe { (*field_value.tydesc).size as usize };
                                unsafe {
                                    std::ptr::copy_nonoverlapping(
                                        field_value.ptr,
                                        field_dest.ptr,
                                        size,
                                    );
                                }
                                free_value_structure(ctx, field_value);
                            }
                        }
                        Err(e) => {
                            // Clean up already-written fields.
                            for j in 0..i {
                                let written_field = get_struct_field_dest(dest, j)
                                    .expect("field index should be valid");
                                let field_value = Value {
                                    ptr: written_field.ptr,
                                    tydesc: written_field.tydesc,
                                    ownership: ValueOwnership::Borrowed,
                                };
                                destroy_value(ctx, field_value);
                            }
                            return Err(e);
                        }
                    }
                }

                return Ok(Value {
                    ptr: dest.ptr,
                    tydesc: dest.tydesc,
                    ownership: ValueOwnership::Borrowed,
                });
            }
        }
    }

    // Fallback: no dest or type mismatch - allocate new struct.
    let mut field_values = Vec::with_capacity(sorted_fields.len());

    for (name, value_expr) in sorted_fields {
        // In frame context, use temp slots for field values.
        let value_dest = match eval_ctx {
            EvalContext::Frame => Some(get_destination_for_expr(ctx, value_expr)?),
            EvalContext::ScriptScope => None,
        };
        match eval_expression(ctx, eval_ctx, value_expr, value_dest) {
            Ok(v) => field_values.push((name, v)),
            Err(e) => {
                for (_, v) in field_values {
                    destroy_value(ctx, v);
                }
                return Err(e);
            }
        }
    }

    allocate_struct_from_values(ctx, field_values)
}

