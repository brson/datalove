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
use super::{SlotId, ProgramPoint, StmtId, Position, InitState, SlotKind};
use super::cfg::ControlFlowGraph;
use super::liveness::InitializationAnalysis;
use super::slot_allocation::AllocatedSlot;
use crate::ast::{Statement, StmtFun, ExprFun, ExprFunKind};

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
    use bct::text::InternedText;

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
        ExprFunKind::Datalit(_) | ExprFunKind::ParseError(_) => {
            // No reads.
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
                walk_and_track_init(db, if_stmt.then_body(db), slots, &mut then_state, map, stmt_counter);

                // Process else branch.
                let mut else_state = state_before_if.clone();
                if let Some(else_body) = if_stmt.else_body(db) {
                    walk_and_track_init(db, else_body, slots, &mut else_state, map, stmt_counter);
                }

                // Merge states after if: Always + Always = Always, else = Sometimes.
                for i in 0..current_state.len() {
                    current_state[i] = then_state[i].merge(else_state[i]);
                }

                // Handle if-bindings.
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
            Statement::Ret(_) | Statement::Fun(_) | Statement::Require(_) |
            Statement::Import(_) | Statement::ParseError(_) => {
                // No state changes.
            }
        }
    }
}

/// Flatten all statements into a single vec.
fn flatten_statements<'db>(db: &'db dyn crate::Db, stmts: &'db [Statement<'db>]) -> Vec<&'db Statement<'db>> {
    let mut result = Vec::new();
    for stmt in stmts {
        result.push(stmt);
        match stmt {
            Statement::If(if_stmt) => {
                result.extend(flatten_statements(db, if_stmt.then_body(db)));
                if let Some(else_body) = if_stmt.else_body(db) {
                    result.extend(flatten_statements(db, else_body));
                }
            }
            _ => {}
        }
    }
    result
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
    use crate::function_analysis::liveness::analyze_initialization;

    fn parse_function<'db>(db: &'db dyn crate::Db, source_code: &str) -> StmtFun<'db> {
        let source = Source::new(db, S(source_code));
        let script = crate::parser::parse_for_test(db, source);
        let statements = script.statements(db);

        // Find the first function statement.
        for stmt in statements {
            if let crate::ast::Statement::Fun(fun) = stmt {
                return *fun;
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

        let func = parse_function(db, source);
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

        let func = parse_function(db, source);
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
fun test(cond: In bool)
    if cond
        let x = @42
    end if
    let y = x
end fun
        "#;

        let func = parse_function(db, source);
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
fun test(x: In u32)
    let y = x
end fun
        "#;

        let func = parse_function(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let errors = check_use_before_init(db, func, &slot_alloc.slots(db), init, cfg);

        // Should have no errors - parameters are always initialized.
        assert_eq!(errors.len(), 0);
    }
}
