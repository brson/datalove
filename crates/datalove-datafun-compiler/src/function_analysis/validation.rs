//! Validation passes for linear type system.
//!
//! This module implements validation checks for:
//! - Use before initialization
//! - Double moves
//! - Use after move
//! - Uninitialized returns
//! - Unused values

use rmx::prelude::*;
use std::collections::HashMap;
use super::{SlotId, ProgramPoint, StmtId, Position, InitState, SlotKind, ExprId, BlockId};
use super::cfg::ControlFlowGraph;
use super::liveness::{InitializationAnalysis, LiveRanges};
use super::slot_allocation::AllocatedSlot;
use crate::ast::{Statement, StmtFun, ExprFun, ExprFunKind, ParamMode};

/// Analysis error from validation passes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnalysisError {
    /// Read of a slot before it has been initialized.
    UseBeforeInit {
        slot: SlotId,
        use_location: ProgramPoint,
    },
    /// Same slot moved multiple times.
    DoubleMove {
        slot: SlotId,
        first_move: ProgramPoint,
        second_move: ProgramPoint,
    },
    /// Read of a slot after it has been moved.
    UseAfterMove {
        slot: SlotId,
        use_location: ProgramPoint,
        move_location: ProgramPoint,
    },
    /// Return from function with uninitialized slot.
    UninitializedReturn {
        slot: SlotId,
        paths: Vec<ProgramPoint>,
    },
    /// Slot is written but never read.
    ValueNotUsed {
        slot: SlotId,
    },
    /// Borrowed value was moved while still borrowed (future).
    BorrowedValueMoved {
        slot: SlotId,
        borrow_location: ProgramPoint,
        move_location: ProgramPoint,
    },
}

/// Information about a read operation for validation.
#[derive(Clone, Debug)]
struct ReadLocation {
    slot_id: SlotId,
    location: ProgramPoint,
}

/// Check for use-before-initialization errors.
///
/// Detects reads of slots that are not definitely initialized at the read location.
pub fn check_use_before_init<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    init: InitializationAnalysis<'db>,
    _cfg: ControlFlowGraph<'db>,
) -> Vec<AnalysisError> {
    let mut errors = Vec::new();

    // Collect all reads with their locations.
    let reads = collect_all_reads(db, func, slots);

    // Build map from statement ID to initialization state.
    let init_states = build_init_state_map(db, func, slots, init);

    // Check each read.
    for read in reads {
        // Skip Reference slots (parameters) - they're always initialized.
        if let Some(slot) = slots.iter().find(|s| s.slot_id(db) == read.slot_id) {
            if slot.kind(db) == SlotKind::Reference {
                continue;
            }

            // Look up initialization state at this point.
            if let Some(state) = init_states.get(&(read.location.stmt_id, read.slot_id)) {
                match state {
                    InitState::Never | InitState::Sometimes => {
                        errors.push(AnalysisError::UseBeforeInit {
                            slot: read.slot_id,
                            use_location: read.location,
                        });
                    }
                    InitState::Always => {
                        // OK - slot is definitely initialized.
                    }
                }
            }
        }
    }

    errors
}

/// Collect all reads from a function with their locations.
fn collect_all_reads<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
) -> Vec<ReadLocation> {
    let mut reads = Vec::new();
    let mut stmt_counter = 0u32;

    walk_statements_for_reads(db, func.body(db), slots, &mut reads, &mut stmt_counter);

    reads
}

/// Walk statements to collect reads.
fn walk_statements_for_reads<'db>(
    db: &'db dyn crate::Db,
    statements: &[Statement<'db>],
    slots: &[AllocatedSlot<'db>],
    reads: &mut Vec<ReadLocation>,
    stmt_counter: &mut u32,
) {
    for stmt in statements {
        let stmt_id = StmtId(*stmt_counter);
        *stmt_counter += 1;

        match stmt {
            Statement::Let(let_stmt) => {
                // Reads from RHS expression.
                collect_reads_from_expr(db, let_stmt.value(db), stmt_id, slots, reads);
            }
            Statement::Ret(ret_stmt) => {
                // Reads from return expression.
                collect_reads_from_expr(db, ret_stmt.value(db), stmt_id, slots, reads);
            }
            Statement::If(if_stmt) => {
                // Reads from condition.
                collect_reads_from_expr(db, if_stmt.condition(db), stmt_id, slots, reads);

                // Process then and else branches.
                walk_statements_for_reads(db, if_stmt.then_body(db), slots, reads, stmt_counter);
                if let Some(else_body) = if_stmt.else_body(db) {
                    walk_statements_for_reads(db, else_body, slots, reads, stmt_counter);
                }
            }
            Statement::Loop(loop_stmt) => {
                // Process loop body.
                walk_statements_for_reads(db, loop_stmt.body(db), slots, reads, stmt_counter);
            }
            Statement::Break(_) | Statement::Continue(_) => {
                // No reads from control flow statements.
            }
            Statement::Fun(_) | Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No reads.
            }
        }
    }
}

/// Collect reads from an expression.
fn collect_reads_from_expr<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFun<'db>,
    stmt_id: StmtId,
    slots: &[AllocatedSlot<'db>],
    reads: &mut Vec<ReadLocation>,
) {
    

    match expr.expr(db) {
        ExprFunKind::Name(name) => {
            // This is a read of a named slot.
            if let Some(slot_id) = find_slot_by_name(db, slots, name) {
                reads.push(ReadLocation {
                    slot_id,
                    location: ProgramPoint {
                        stmt_id,
                        position: Position::Before,
                    },
                });
            }
        }
        ExprFunKind::BinOp(binop) => {
            collect_reads_from_expr(db, binop.lhs(db), stmt_id, slots, reads);
            collect_reads_from_expr(db, binop.rhs(db), stmt_id, slots, reads);
        }
        ExprFunKind::UnaryOp(unary) => {
            collect_reads_from_expr(db, unary.operand(db), stmt_id, slots, reads);
        }
        ExprFunKind::FunctionCall(call) => {
            for arg in call.args(db) {
                collect_reads_from_expr(db, *arg, stmt_id, slots, reads);
            }
        }
        ExprFunKind::Tuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads);
            }
        }
        ExprFunKind::TryOption(try_opt) => {
            collect_reads_from_expr(db, try_opt.operand(db), stmt_id, slots, reads);
        }
        ExprFunKind::TryResult(try_res) => {
            collect_reads_from_expr(db, try_res.operand(db), stmt_id, slots, reads);
        }
        ExprFunKind::ParseError(_) => {
            // No reads.
        }

        // Inline literal variants.
        ExprFunKind::True(_) |
        ExprFunKind::False(_) |
        ExprFunKind::None(_) |
        ExprFunKind::Int(_) |
        ExprFunKind::Float(_) |
        ExprFunKind::Hex(_) |
        ExprFunKind::String(_) => {
            // Simple literals have no reads.
        }
        ExprFunKind::List(list) => {
            for elem in list.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads);
            }
        }
        ExprFunKind::Set(set) => {
            for elem in set.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads);
            }
        }
        ExprFunKind::Map(map) => {
            for entry in map.entries(db) {
                collect_reads_from_expr(db, entry.key(db), stmt_id, slots, reads);
                collect_reads_from_expr(db, entry.value(db), stmt_id, slots, reads);
            }
        }
        ExprFunKind::Tensor(tensor) => {
            for elem in tensor.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads);
            }
        }
        ExprFunKind::AnonTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads);
            }
        }
        ExprFunKind::AnonStruct(s) => {
            for field in s.fields(db) {
                collect_reads_from_expr(db, field.value(db), stmt_id, slots, reads);
            }
        }
        ExprFunKind::AnonEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_reads_from_expr(db, payload, stmt_id, slots, reads);
            }
        }
        ExprFunKind::Some(s) => {
            collect_reads_from_expr(db, s.payload(db), stmt_id, slots, reads);
        }
        ExprFunKind::Ok(o) => {
            collect_reads_from_expr(db, o.payload(db), stmt_id, slots, reads);
        }
        ExprFunKind::Er(e) => {
            collect_reads_from_expr(db, e.payload(db), stmt_id, slots, reads);
        }
        ExprFunKind::Data(d) => {
            collect_reads_from_expr(db, d.value(db), stmt_id, slots, reads);
        }
        ExprFunKind::Err(e) => {
            collect_reads_from_expr(db, e.value(db), stmt_id, slots, reads);
        }
    }
}

/// Check for double-move errors.
///
/// Detects when the same slot is moved multiple times.
/// Note: Copy moves are not considered real moves for this check.
pub fn check_double_move<'db>(
    db: &'db dyn crate::Db,
    _func: StmtFun<'db>,
    move_info: super::MoveInfo<'db>,
) -> Vec<AnalysisError> {
    use std::collections::HashMap;
    use super::MoveKind;

    let mut errors = Vec::new();

    // Group moves by slot_id, filtering out Copy moves.
    let mut moves_by_slot: HashMap<SlotId, Vec<super::MoveOp<'db>>> = HashMap::new();
    for move_op in move_info.moves(db) {
        // Skip Copy moves - they don't consume the value.
        if move_op.move_kind(db) == MoveKind::Copy {
            continue;
        }

        moves_by_slot
            .entry(move_op.slot_id(db))
            .or_insert_with(Vec::new)
            .push(*move_op);
    }

    // Check for slots with multiple moves.
    for (slot_id, moves) in moves_by_slot {
        if moves.len() >= 2 {
            // Create error with first two moves.
            // Use stmt_id directly from MoveOp.
            let first = &moves[0];
            let second = &moves[1];

            errors.push(AnalysisError::DoubleMove {
                slot: slot_id,
                first_move: ProgramPoint {
                    stmt_id: first.stmt_id(db),
                    position: Position::Before,
                },
                second_move: ProgramPoint {
                    stmt_id: second.stmt_id(db),
                    position: Position::Before,
                },
            });
        }
    }

    errors
}

/// Check for use-after-move errors.
///
/// Detects reads that occur after a slot has been moved.
/// Note: Copy moves are not considered real moves for this check.
pub fn check_use_after_move<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    move_info: super::MoveInfo<'db>,
) -> Vec<AnalysisError> {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use super::MoveKind;

    let mut errors = Vec::new();

    // Build set of Temporary slots to skip.
    let temp_slots: HashSet<SlotId> = slots.iter()
        .filter(|s| s.kind(db) == SlotKind::Temporary)
        .map(|s| s.slot_id(db))
        .collect();

    // Build a map of SlotId -> Vec<ProgramPoint> for all moves.
    // Use stmt_id directly from MoveOp instead of reverse-mapping from ExprId.
    let mut moves_by_slot: HashMap<SlotId, Vec<ProgramPoint>> = HashMap::new();
    for move_op in move_info.moves(db) {
        // Skip Copy moves - they don't consume the value.
        if move_op.move_kind(db) == MoveKind::Copy {
            continue;
        }

        // Skip Temporary slots - they have different semantics.
        if temp_slots.contains(&move_op.slot_id(db)) {
            continue;
        }

        let move_point = ProgramPoint {
            stmt_id: move_op.stmt_id(db),
            position: Position::Before,
        };

        moves_by_slot
            .entry(move_op.slot_id(db))
            .or_insert_with(Vec::new)
            .push(move_point);
    }

    // Collect all reads.
    let reads = collect_all_reads(db, func, slots);

    // Check each read to see if it follows a move.
    for read in reads {
        if let Some(move_points) = moves_by_slot.get(&read.slot_id) {
            // Check if any move happened before this read.
            for &move_point in move_points {
                if program_point_before(move_point, read.location) {
                    errors.push(AnalysisError::UseAfterMove {
                        slot: read.slot_id,
                        use_location: read.location,
                        move_location: move_point,
                    });
                    break; // Only report first move violation.
                }
            }
        }
    }

    errors
}

/// Check if point A comes before point B in program order.
fn program_point_before(a: ProgramPoint, b: ProgramPoint) -> bool {
    if a.stmt_id.0 < b.stmt_id.0 {
        true
    } else if a.stmt_id.0 == b.stmt_id.0 {
        // Same statement - check position.
        matches!((a.position, b.position), (Position::Before, Position::After))
    } else {
        false
    }
}

/// Check for uninitialized-return errors.
///
/// Detects return from function with uninitialized Out parameters.
pub fn check_uninitialized_return<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    init: InitializationAnalysis<'db>,
    cfg: ControlFlowGraph<'db>,
) -> Vec<AnalysisError> {
    use super::cfg::Terminator;

    let mut errors = Vec::new();

    // Find all exit blocks (those with Return or TryReturn terminators).
    let exit_blocks: Vec<BlockId> = cfg.blocks(db)
        .iter()
        .filter(|block| {
            matches!(block.terminator, Terminator::Return | Terminator::TryReturn)
        })
        .map(|block| block.block_id)
        .collect();

    // For each exit block, check Out parameters are initialized.
    for block_id in exit_blocks {
        // Identify Out parameters.
        for param in func.params(db) {
            if param.mode(db) != ParamMode::Out {
                continue;
            }

            // Find the slot for this parameter.
            let param_name = param.name(db);
            let slot = slots.iter().find(|s| s.name(db) == Some(param_name));

            if let Some(slot_info) = slot {
                let slot_id = slot_info.slot_id(db);

                // Check initialization state at block exit.
                if let Some(state) = init.get_exit_state(db, block_id, slot_id) {
                    match state {
                        InitState::Never | InitState::Sometimes => {
                            errors.push(AnalysisError::UninitializedReturn {
                                slot: slot_id,
                                paths: vec![ProgramPoint {
                                    stmt_id: StmtId(0), // Placeholder - would need block's last stmt.
                                    position: Position::After,
                                }],
                            });
                        }
                        InitState::Always => {
                            // OK - Out parameter is properly initialized.
                        }
                    }
                }
            }
        }
    }

    errors
}

/// Check for value-not-used errors.
///
/// Detects Local and Temporary slots that are written but never read.
pub fn check_value_not_used<'db>(
    db: &'db dyn crate::Db,
    slots: &[AllocatedSlot<'db>],
    live_ranges: LiveRanges<'db>,
) -> Vec<AnalysisError> {
    let mut errors = Vec::new();

    // Check each slot.
    for slot in slots {
        // Skip Reference slots (parameters - they are used by caller).
        // Skip Temporary slots (expression intermediates - always consumed immediately).
        if slot.kind(db) == SlotKind::Reference || slot.kind(db) == SlotKind::Temporary {
            continue;
        }

        let slot_id = slot.slot_id(db);

        // Look for a live range for this slot.
        let range = live_ranges.ranges(db)
            .iter()
            .find(|r| r.slot_id(db) == slot_id);

        match range {
            None => {
                // No live range means slot was never written or used.
                // This shouldn't happen for allocated slots, but handle it.
                errors.push(AnalysisError::ValueNotUsed { slot: slot_id });
            }
            Some(r) => {
                // Check if death point equals birth point - no actual use.
                if r.death(db) == r.birth(db) {
                    errors.push(AnalysisError::ValueNotUsed { slot: slot_id });
                }
            }
        }
    }

    errors
}

/// Build a map from ExprId to StmtId.
///
/// Note: This function is kept for debugging purposes but is no longer used
/// in the main validation logic. MoveOp now stores stmt_id directly.
#[allow(dead_code)]
fn build_expr_to_stmt_map<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
) -> HashMap<ExprId, StmtId> {
    let mut map = HashMap::new();
    let mut expr_counter = 0u32;
    let mut stmt_counter = 0u32;

    walk_for_expr_mapping(db, func.body(db), &mut map, &mut expr_counter, &mut stmt_counter);

    map
}

/// Walk statements to map ExprIds to StmtIds.
fn walk_for_expr_mapping<'db>(
    db: &'db dyn crate::Db,
    statements: &[Statement<'db>],
    map: &mut HashMap<ExprId, StmtId>,
    expr_counter: &mut u32,
    stmt_counter: &mut u32,
) {
    for stmt in statements {
        let stmt_id = StmtId(*stmt_counter);
        *stmt_counter += 1;

        match stmt {
            Statement::Let(let_stmt) => {
                map_expr_ids(db, let_stmt.value(db), stmt_id, map, expr_counter);
            }
            Statement::Ret(ret_stmt) => {
                map_expr_ids(db, ret_stmt.value(db), stmt_id, map, expr_counter);
            }
            Statement::If(if_stmt) => {
                map_expr_ids(db, if_stmt.condition(db), stmt_id, map, expr_counter);
                walk_for_expr_mapping(db, if_stmt.then_body(db), map, expr_counter, stmt_counter);
                if let Some(else_body) = if_stmt.else_body(db) {
                    walk_for_expr_mapping(db, else_body, map, expr_counter, stmt_counter);
                }
            }
            Statement::Loop(loop_stmt) => {
                walk_for_expr_mapping(db, loop_stmt.body(db), map, expr_counter, stmt_counter);
            }
            Statement::Break(_) | Statement::Continue(_) => {
                // No expressions in control flow statements.
            }
            Statement::Fun(_) | Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No expressions.
            }
        }
    }
}

/// Map all expression IDs in an expression to a statement ID.
fn map_expr_ids<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFun<'db>,
    stmt_id: StmtId,
    map: &mut HashMap<ExprId, StmtId>,
    expr_counter: &mut u32,
) {
    let expr_id = ExprId(*expr_counter);
    *expr_counter += 1;
    map.insert(expr_id, stmt_id);

    match expr.expr(db) {
        ExprFunKind::BinOp(binop) => {
            map_expr_ids(db, binop.lhs(db), stmt_id, map, expr_counter);
            map_expr_ids(db, binop.rhs(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::UnaryOp(unary) => {
            map_expr_ids(db, unary.operand(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::FunctionCall(call) => {
            for arg in call.args(db) {
                map_expr_ids(db, *arg, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::Tuple(tuple) => {
            for elem in tuple.elements(db) {
                map_expr_ids(db, *elem, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::TryOption(try_opt) => {
            map_expr_ids(db, try_opt.operand(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::TryResult(try_res) => {
            map_expr_ids(db, try_res.operand(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::Name(_) | ExprFunKind::ParseError(_) => {
            // No nested expressions.
        }

        // Inline literal variants.
        ExprFunKind::True(_) |
        ExprFunKind::False(_) |
        ExprFunKind::None(_) |
        ExprFunKind::Int(_) |
        ExprFunKind::Float(_) |
        ExprFunKind::Hex(_) |
        ExprFunKind::String(_) => {
            // Simple literals have no nested expressions.
        }
        ExprFunKind::List(list) => {
            for elem in list.elements(db) {
                map_expr_ids(db, *elem, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::Set(set) => {
            for elem in set.elements(db) {
                map_expr_ids(db, *elem, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::Map(map_expr) => {
            for entry in map_expr.entries(db) {
                map_expr_ids(db, entry.key(db), stmt_id, map, expr_counter);
                map_expr_ids(db, entry.value(db), stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::Tensor(tensor) => {
            for elem in tensor.elements(db) {
                map_expr_ids(db, *elem, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::AnonTuple(tuple) => {
            for elem in tuple.elements(db) {
                map_expr_ids(db, *elem, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::AnonStruct(s) => {
            for field in s.fields(db) {
                map_expr_ids(db, field.value(db), stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::AnonEnum(e) => {
            if let Some(payload) = e.payload(db) {
                map_expr_ids(db, payload, stmt_id, map, expr_counter);
            }
        }
        ExprFunKind::Some(s) => {
            map_expr_ids(db, s.payload(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::Ok(o) => {
            map_expr_ids(db, o.payload(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::Er(e) => {
            map_expr_ids(db, e.payload(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::Data(d) => {
            map_expr_ids(db, d.value(db), stmt_id, map, expr_counter);
        }
        ExprFunKind::Err(e) => {
            map_expr_ids(db, e.value(db), stmt_id, map, expr_counter);
        }
    }
}

/// Build a map of initialization states for each (statement, slot) pair.
///
/// This is a simplified linear walk that doesn't follow the full CFG.
/// It tracks initialization state as we walk through statements sequentially.
fn build_init_state_map<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    _init: InitializationAnalysis<'db>,
) -> HashMap<(StmtId, SlotId), InitState> {
    let mut map = HashMap::new();

    // Initial state: Reference slots are always initialized, others are never.
    let mut current_state: Vec<InitState> = slots
        .iter()
        .map(|slot| {
            if slot.kind(db) == SlotKind::Reference {
                InitState::Always
            } else {
                InitState::Never
            }
        })
        .collect();

    let mut stmt_counter = 0u32;
    walk_and_track_init(db, func.body(db), slots, &mut current_state, &mut map, &mut stmt_counter);

    map
}

/// Walk statements and track initialization state.
fn walk_and_track_init<'db>(
    db: &'db dyn crate::Db,
    statements: &[Statement<'db>],
    slots: &[AllocatedSlot<'db>],
    current_state: &mut Vec<InitState>,
    map: &mut HashMap<(StmtId, SlotId), InitState>,
    stmt_counter: &mut u32,
) {
    for stmt in statements {
        let stmt_id = StmtId(*stmt_counter);
        *stmt_counter += 1;

        // Record current state for this statement.
        for (slot_idx, slot) in slots.iter().enumerate() {
            let slot_id = slot.slot_id(db);
            if let Some(&state) = current_state.get(slot_idx) {
                map.insert((stmt_id, slot_id), state);
            }
        }

        match stmt {
            Statement::Let(let_stmt) => {
                // Update state: this slot is now initialized.
                if let Some(slot_id) = find_slot_by_name(db, slots, let_stmt.name(db)) {
                    let slot_idx = slot_id.0 as usize;
                    if slot_idx < current_state.len() {
                        current_state[slot_idx] = InitState::Always;
                    }
                }
            }
            Statement::If(if_stmt) => {
                // For if statements, we need to track both branches.
                let state_before_if = current_state.clone();

                // Process then branch.
                let mut then_state = state_before_if.clone();
                // If there's a then_binding, mark it as initialized in then branch.
                if let Some(then_name) = if_stmt.then_binding(db) {
                    if let Some(slot_id) = find_slot_by_name(db, slots, then_name) {
                        let slot_idx = slot_id.0 as usize;
                        if slot_idx < then_state.len() {
                            then_state[slot_idx] = InitState::Always;
                        }
                    }
                }
                walk_and_track_init(db, if_stmt.then_body(db), slots, &mut then_state, map, stmt_counter);

                // Process else branch.
                let mut else_state = state_before_if.clone();
                // If there's an else_binding, mark it as initialized in else branch.
                if let Some(else_name) = if_stmt.else_binding(db) {
                    if let Some(slot_id) = find_slot_by_name(db, slots, else_name) {
                        let slot_idx = slot_id.0 as usize;
                        if slot_idx < else_state.len() {
                            else_state[slot_idx] = InitState::Always;
                        }
                    }
                }
                if let Some(else_body) = if_stmt.else_body(db) {
                    walk_and_track_init(db, else_body, slots, &mut else_state, map, stmt_counter);
                }

                // Merge states after if: Always + Always = Always, else = Sometimes.
                for i in 0..current_state.len() {
                    current_state[i] = then_state[i].merge(else_state[i]);
                }

                // Handle if-bindings for merged state (they're only valid in their respective branches).
                if let Some(then_name) = if_stmt.then_binding(db) {
                    if let Some(slot_id) = find_slot_by_name(db, slots, then_name) {
                        let slot_idx = slot_id.0 as usize;
                        if slot_idx < current_state.len() {
                            current_state[slot_idx] = InitState::Sometimes;
                        }
                    }
                }
                if let Some(else_name) = if_stmt.else_binding(db) {
                    if let Some(slot_id) = find_slot_by_name(db, slots, else_name) {
                        let slot_idx = slot_id.0 as usize;
                        if slot_idx < current_state.len() {
                            current_state[slot_idx] = InitState::Sometimes;
                        }
                    }
                }
            }
            Statement::Loop(loop_stmt) => {
                // For loops, track the body. Note: loop body may execute 0 or more times.
                let state_before_loop = current_state.clone();
                walk_and_track_init(db, loop_stmt.body(db), slots, current_state, map, stmt_counter);
                // After loop: merge with before-loop state (loop may not execute).
                for i in 0..current_state.len() {
                    current_state[i] = state_before_loop[i].merge(current_state[i]);
                }
            }
            Statement::Break(_) | Statement::Continue(_) => {
                // No state changes for control flow statements.
            }
            Statement::Ret(_) | Statement::Fun(_) | Statement::Require(_) |
            Statement::Import(_) | Statement::ParseError(_) => {
                // No state changes.
            }
        }
    }
}


/// Find a slot by name.
fn find_slot_by_name<'db>(
    db: &'db dyn crate::Db,
    slots: &[AllocatedSlot<'db>],
    name: bct::text::InternedText<'db>,
) -> Option<SlotId> {
    slots
        .iter()
        .find(|s| s.name(db) == Some(name))
        .map(|s| s.slot_id(db))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bct::input::Source;
    use crate::function_analysis::cfg::build_cfg;
    use crate::function_analysis::slot_allocation::allocate_slots;
    use crate::function_analysis::liveness::{analyze_initialization, compute_live_ranges};
    use crate::function_analysis::moves::compute_move_info;

    fn parse_and_typecheck<'db>(db: &'db dyn crate::Db, source_code: &str) -> (StmtFun<'db>, crate::tycheck::TypecheckResult<'db>) {
        let source = Source::new(db, S(source_code));
        let script = crate::parser::parse_for_diagnostics(db, source);
        let tycheck_result = crate::tycheck::type_check(db, source, script);
        let statements = script.statements(db);

        // Find the first function statement.
        for stmt in statements {
            if let crate::ast::Statement::Fun(fun) = stmt {
                return (*fun, tycheck_result);
            }
        }

        panic!("No function found in source code");
    }

    #[test]
    fn test_use_before_init_detects_error() {
        let ref db = crate::Database::default();

        let source = r#"
fun test()
    let x = y
    let y = @42
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_use_before_init(db, func, &slot_alloc.slots(db), init, cfg);

        // Should detect use of uninitialized y.
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            AnalysisError::UseBeforeInit { slot, .. } => {
                // y should be the uninitialized slot.
                let y_name = bct::text::InternedText::new(db, "y");
                let y_slot = slot_alloc.slots(db)
                    .iter()
                    .find(|s| s.name(db) == Some(y_name))
                    .unwrap();
                assert_eq!(*slot, y_slot.slot_id(db));
            }
            _ => panic!("Expected UseBeforeInit error"),
        }
    }

    #[test]
    fn test_use_before_init_no_error_when_initialized() {
        let ref db = crate::Database::default();

        let source = r#"
fun test()
    let x = @42
    let y = x
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_use_before_init(db, func, &slot_alloc.slots(db), init, cfg);

        // Should have no errors - x is initialized before use.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_use_before_init_conditional() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(cond: bool)
    if cond
        let x = @42
    end if
    let y = x
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_use_before_init(db, func, &slot_alloc.slots(db), init, cfg);

        // Should detect use of x which is only sometimes initialized.
        assert!(errors.len() > 0);
        let has_x_error = errors.iter().any(|e| {
            if let AnalysisError::UseBeforeInit { slot, .. } = e {
                let x_name = bct::text::InternedText::new(db, "x");
                let x_slot = slot_alloc.slots(db)
                    .iter()
                    .find(|s| s.name(db) == Some(x_name))
                    .map(|s| s.slot_id(db));
                x_slot == Some(*slot)
            } else {
                false
            }
        });
        assert!(has_x_error, "Should detect use of conditionally-initialized x");
    }

    #[test]
    fn test_use_before_init_parameter_ok() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32)
    let y = x
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_use_before_init(db, func, &slot_alloc.slots(db), init, cfg);

        // Should have no errors - parameters are always initialized.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_double_move_detects_error() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: [u32])
    fun helper(a: [u32]): [u32]
        ret a
    end fun
    let y = helper(x)
    let z = helper(x)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_double_move(db, func, move_info);

        // Should detect double move of x.
        assert_eq!(errors.len(), 1);
        match &errors[0] {
            AnalysisError::DoubleMove { slot, .. } => {
                let x_name = bct::text::InternedText::new(db, "x");
                let x_slot = slot_alloc.slots(db)
                    .iter()
                    .find(|s| s.name(db) == Some(x_name))
                    .unwrap();
                assert_eq!(*slot, x_slot.slot_id(db));
            }
            _ => panic!("Expected DoubleMove error"),
        }
    }

    #[test]
    fn test_double_move_different_slots_ok() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32, y: u32)
    fun helper(a: u32): u32
        ret a
    end fun
    let a = helper(x)
    let b = helper(y)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_double_move(db, func, move_info);

        // Should have no errors - moving different slots is fine.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_double_move_single_move_ok() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32)
    fun helper(a: u32): u32
        ret a
    end fun
    let y = helper(x)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_double_move(db, func, move_info);

        // Should have no errors - single move is fine.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_use_after_move_detects_error() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: [u32])
    fun helper(a: [u32]): [u32]
        ret a
    end fun
    let y = helper(x)
    let z = x
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_use_after_move(db, func, &slot_alloc.slots(db), move_info);

        // Should detect error: 'x' is read in 'let z = x' after being moved to helper().
        assert!(errors.len() > 0, "Expected at least one use-after-move error");
        let has_x_error = errors.iter().any(|e| {
            if let AnalysisError::UseAfterMove { slot, .. } = e {
                let x_name = bct::text::InternedText::new(db, "x");
                slot_alloc.slots(db)
                    .iter()
                    .any(|s| s.slot_id(db) == *slot && s.name(db) == Some(x_name))
            } else {
                false
            }
        });
        assert!(has_x_error, "Expected use-after-move error for slot 'x'");
    }

    #[test]
    fn test_use_after_move_single_use_ok() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32)
    fun helper(a: u32): u32
        ret a
    end fun
    let y = helper(x)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_use_after_move(db, func, &slot_alloc.slots(db), move_info);

        // Should have no errors - 'x' is only used once.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_use_after_move_in_branch() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: [u32], cond: u32)
    fun helper(a: [u32]): [u32]
        ret a
    end fun
    if cond
        let y = helper(x)
    end if
    let z = x
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_use_after_move(db, func, &slot_alloc.slots(db), move_info);

        // Conservative: should detect error even though move is conditionally executed.
        // The use of 'x' after the if-statement comes after the move inside the if-statement.
        assert!(errors.len() > 0, "Expected use-after-move error when move is in branch");
    }

    #[test]
    fn test_uninitialized_return_detects_error() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32, out result: u32)
    let y = x
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_uninitialized_return(db, func, &slot_alloc.slots(db), init, cfg);

        // Should detect error: Out parameter 'result' is not initialized.
        assert!(errors.len() > 0, "Expected uninitialized-return error");
        let has_result_error = errors.iter().any(|e| {
            if let AnalysisError::UninitializedReturn { slot, .. } = e {
                let result_name = bct::text::InternedText::new(db, "result");
                slot_alloc.slots(db)
                    .iter()
                    .any(|s| s.slot_id(db) == *slot && s.name(db) == Some(result_name))
            } else {
                false
            }
        });
        assert!(has_result_error, "Expected uninitialized-return error for 'result'");
    }

    #[test]
    fn test_uninitialized_return_conditional() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32, out result: u32)
    if x
        let result = @42
    end if
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_uninitialized_return(db, func, &slot_alloc.slots(db), init, cfg);

        // Should detect error: Out parameter 'result' is only sometimes initialized.
        assert!(errors.len() > 0, "Expected uninitialized-return error for conditional init");
    }

    #[test]
    fn test_uninitialized_return_properly_initialized() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32, out result: u32)
    let result = x
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_uninitialized_return(db, func, &slot_alloc.slots(db), init, cfg);

        // Should have no errors - Out parameter is properly initialized.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_value_not_used_detects_error() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32): u32
    let unused = @42
    ret x
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        let errors = check_value_not_used(db, &slot_alloc.slots(db), live_ranges);

        // Should detect error: 'unused' is never read.
        assert!(errors.len() > 0, "Expected value-not-used error");
        let has_unused_error = errors.iter().any(|e| {
            if let AnalysisError::ValueNotUsed { slot } = e {
                let unused_name = bct::text::InternedText::new(db, "unused");
                slot_alloc.slots(db)
                    .iter()
                    .any(|s| s.slot_id(db) == *slot && s.name(db) == Some(unused_name))
            } else {
                false
            }
        });
        assert!(has_unused_error, "Expected value-not-used error for 'unused'");
    }

    #[test]
    fn test_value_not_used_all_values_used() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(x: u32): u32
    let y = x
    ret y
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        let errors = check_value_not_used(db, &slot_alloc.slots(db), live_ranges);

        // Should have no errors - all values are used.
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn test_value_not_used_parameter_ok() {
        let ref db = crate::Database::default();
        let source = r#"
fun test(unused_param: u32): u32
    ret @0
end fun
        "#;

        let (func, _tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        let errors = check_value_not_used(db, &slot_alloc.slots(db), live_ranges);

        // Should have no errors - parameters (Reference slots) are skipped.
        assert_eq!(errors.len(), 0);
    }

    /// Regression test: UseAfterMove false positive in branching patterns.
    ///
    /// The pattern `if self |value| ret self else |error| ret other` should NOT
    /// trigger UseAfterMove because each variable is only used once in its
    /// respective branch.
    ///
    /// This was a bug where the ExprId-to-StmtId mapping was incomplete, causing
    /// moves in else branches to be incorrectly mapped to StmtId(0). Fixed by
    /// storing stmt_id directly in MoveOp.
    #[test]
    fn test_use_after_move_branching_no_false_positive() {
        let ref db = crate::Database::default();
        let source = r#"
fun or_result(self: !u32, other: !u32): !u32
  if self |value|
    ret self
  else |error|
    ret other
  end if
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        let errors = check_use_after_move(db, func, &slot_alloc.slots(db), move_info);

        // Each variable is only used once in its respective branch, so no errors.
        assert_eq!(errors.len(), 0,
            "or_result should have no UseAfterMove errors - each variable is used once");
    }
}
