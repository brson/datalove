//! Drop point insertion for resource management.

use rmx::prelude::*;
use std::collections::HashMap;
use super::{SlotId, StmtId, BlockId, SlotKind, InitState};
use super::cfg::{ControlFlowGraph, Terminator};
use super::liveness::InitializationAnalysis;
use super::moves::{MovedAnalysis, MoveState};
use super::slot_allocation::{AllocatedSlot, SlotDestruction};
use super::copyability::{is_copy_type, get_slot_type};
use crate::ast::StmtFun;

/// Drop points for all slots.
#[salsa::tracked]
pub struct DropPoints<'db> {
    #[returns(ref)]
    pub drops: Vec<DropPoint<'db>>,
}

/// A single drop point.
#[salsa::tracked]
pub struct DropPoint<'db> {
    pub slot_id: SlotId,
    pub location: DropLocation,
    pub reason: DropReason,
}

/// Location of a drop point.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum DropLocation {
    /// Drop after a specific statement.
    AfterStmt(StmtId),
    /// Drop at block exit (before terminator, when exiting via Goto).
    BlockExit(BlockId),
}

/// Reason for dropping a value.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum DropReason {
    EndOfScope,
    EarlyReturn,
    BranchExit,      // slot consumed in sibling branch, drop here for convergence
    Moved,           // slot was moved, no drop needed
    Uninitialized,   // slot never initialized, no drop needed
}

impl<'db> DropPoints<'db> {
    /// Get all drop points for a specific slot.
    pub fn drops_for_slot(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Vec<DropPoint<'db>> {
        self.drops(db).iter().filter(|d| d.slot_id(db) == slot_id).copied().collect()
    }
}

/// Compute drop points for all slots in a function.
///
/// This identifies where each slot must be dropped to ensure proper resource cleanup.
/// Copy types are automatically skipped as they don't require cleanup.
///
/// Uses per-block move analysis to generate precise drops:
/// 1. At function exits (Return/TryReturn): drop slots not moved on that path
/// 2. At branch convergence (Goto to join blocks): when a slot is moved in one
///    branch but not another, drop it in the branch where it's not moved
#[salsa::tracked]
pub fn compute_drop_points<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    cfg: ControlFlowGraph<'db>,
    slots: &'db [AllocatedSlot<'db>],
    init_analysis: InitializationAnalysis<'db>,
    moved_analysis: MovedAnalysis<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> DropPoints<'db> {
    let mut drops = Vec::new();
    let blocks = cfg.blocks(db);
    let edges = cfg.edges(db);

    // Build a map of incoming edges for each block (to find join points).
    let mut incoming: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    for edge in edges {
        incoming.entry(edge.to).or_default().push(edge.from);
    }

    // Phase 1: Insert drops at branch convergence points (Goto to join blocks).
    // For each join block (block with multiple incoming edges), check if any slot
    // has different move states across predecessors. If so, insert drops in the
    // predecessors where the slot is NOT moved.
    for (_join_block_id, predecessors) in &incoming {
        if predecessors.len() <= 1 {
            // Not a join point, skip.
            continue;
        }

        // For each slot, check move states across all predecessors.
        for slot in slots {
            let slot_id = slot.slot_id(db);

            // Skip reference slots.
            if slot.kind(db) == SlotKind::Reference {
                continue;
            }

            // Skip inline-destroyed slots (destroyed inline by interpreter).
            if slot.destruction(db) == SlotDestruction::InlineDestroyed {
                continue;
            }

            // Skip copy types.
            let slot_type = get_slot_type(db, slot, tycheck_result, func);
            if is_copy_type(db, slot_type) {
                continue;
            }

            // Get exit move state for each predecessor.
            let mut any_always = false;
            let mut never_preds = Vec::new();

            for &pred_block_id in predecessors {
                // Check if initialized at predecessor exit.
                let init_state = init_analysis.get_exit_state(db, pred_block_id, slot_id)
                    .unwrap_or(InitState::Never);

                if matches!(init_state, InitState::Never) {
                    // Not initialized on this path, no drop needed.
                    continue;
                }

                let move_state = moved_analysis.get_exit_state(db, pred_block_id, slot_id)
                    .unwrap_or(MoveState::Never);

                match move_state {
                    MoveState::Always => {
                        any_always = true;
                    }
                    MoveState::Never => {
                        never_preds.push(pred_block_id);
                    }
                    MoveState::Sometimes => {
                        // Already conditionally moved - can't statically resolve.
                        // Will need runtime check at function exit.
                    }
                }
            }

            // If any predecessor has Always (definitely moved) and some have Never,
            // insert drops in the Never predecessors.
            if any_always && !never_preds.is_empty() {
                for pred_block_id in never_preds {
                    drops.push(DropPoint::new(
                        db,
                        slot_id,
                        DropLocation::BlockExit(pred_block_id),
                        DropReason::BranchExit,
                    ));
                }
            }
        }
    }

    // Phase 2: Insert drops at function exits (Return/TryReturn).
    for block in blocks {
        match &block.terminator {
            Terminator::Return | Terminator::TryReturn => {
                let reason = if matches!(block.terminator, Terminator::Return) {
                    DropReason::EndOfScope
                } else {
                    DropReason::EarlyReturn
                };

                // For each slot, check if it's initialized at this exit and needs dropping.
                for slot in slots {
                    let slot_id = slot.slot_id(db);

                    // Reference slots are never dropped (caller owns the data).
                    if slot.kind(db) == SlotKind::Reference {
                        continue;
                    }

                    // Inline-destroyed slots are handled by interpreter, no drop point needed.
                    if slot.destruction(db) == SlotDestruction::InlineDestroyed {
                        continue;
                    }

                    // Check if the slot is initialized at this block's exit.
                    let init_state = init_analysis.get_exit_state(db, block.block_id, slot_id)
                        .unwrap_or(InitState::Never);

                    match init_state {
                        InitState::Never => {
                            // Never initialized, no drop needed.
                            continue;
                        }
                        InitState::Sometimes | InitState::Always => {
                            // Initialized, continue with drop check.
                        }
                    }

                    // Check per-block move state for THIS specific exit.
                    let move_state = moved_analysis.get_exit_state(db, block.block_id, slot_id)
                        .unwrap_or(MoveState::Never);

                    match move_state {
                        MoveState::Always => {
                            // Definitely moved on all paths to this exit, no drop needed.
                            continue;
                        }
                        MoveState::Sometimes | MoveState::Never => {
                            // May or may not be moved - generate drop point.
                            // Runtime will verify if still available.
                        }
                    }

                    // If the slot has a copy type, no drop is needed.
                    let slot_type = get_slot_type(db, slot, tycheck_result, func);
                    if is_copy_type(db, slot_type) {
                        continue;
                    }

                    // Insert drop at the last statement of this exit block.
                    if let Some(&last_stmt) = block.statements.last() {
                        drops.push(DropPoint::new(
                            db,
                            slot_id,
                            DropLocation::AfterStmt(last_stmt),
                            reason,
                        ));
                    }
                }
            }
            Terminator::Branch { .. } | Terminator::Goto(_) => {
                // Drops at Goto handled in Phase 1 (branch convergence).
            }
            Terminator::LoopContinue(_) | Terminator::LoopBreak(_) => {
                // No drops at loop control flow.
                // Drops happen at the exit points (returns).
            }
        }
    }

    DropPoints::new(db, drops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::function_analysis::cfg::build_cfg;
    use crate::function_analysis::slot_allocation::allocate_slots;
    use crate::function_analysis::liveness::{compute_live_ranges, analyze_initialization};
    use crate::function_analysis::moves::{compute_move_info, analyze_moves_per_block};
    use bct::input::Source;
    use bct::text::InternedText;

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
    fn test_drop_simple_local() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): u32
    let x = @42
    ret x
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);
        let moved_analysis = analyze_moves_per_block(db, func, cfg, &slot_alloc.slots(db), move_info);
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, moved_analysis, tycheck_result);

        // x is moved to the return, so it should NOT have a drop point.
        let drops = drop_points.drops(db);
        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();

        let x_drops: Vec<_> = drops.iter()
            .filter(|d| d.slot_id(db) == x_slot.slot_id(db))
            .collect();

        // x is moved by the return, so no drop.
        assert_eq!(x_drops.len(), 0, "x should not be dropped (it's moved)");
    }

    #[test]
    fn test_drop_parameter_not_dropped() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32): u32
    ret x
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);
        let moved_analysis = analyze_moves_per_block(db, func, cfg, &slot_alloc.slots(db), move_info);
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, moved_analysis, tycheck_result);

        // Parameters (Reference slots) should never be dropped.
        let drops = drop_points.drops(db);
        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();

        assert_eq!(x_slot.kind(db), SlotKind::Reference);

        let x_drops: Vec<_> = drops.iter()
            .filter(|d| d.slot_id(db) == x_slot.slot_id(db))
            .collect();

        assert_eq!(x_drops.len(), 0, "parameters (Reference slots) should never be dropped");
    }

    #[test]
    fn test_drop_local_not_moved() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): [u32]
    let x = [42]
    let y = [10]
    ret x
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);
        let moved_analysis = analyze_moves_per_block(db, func, cfg, &slot_alloc.slots(db), move_info);
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, moved_analysis, tycheck_result);

        // x is moved, y is not moved and is non-copy, so y should be dropped.
        let drops = drop_points.drops(db);
        let y_name = InternedText::new(db, "y");
        let y_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(y_name)).unwrap();

        let y_drops: Vec<_> = drops.iter()
            .filter(|d| d.slot_id(db) == y_slot.slot_id(db))
            .collect();

        assert_eq!(y_drops.len(), 1, "y should have exactly one drop point");
        assert_eq!(y_drops[0].reason(db), DropReason::EndOfScope);
    }

    #[test]
    fn test_drop_with_conditional() {
        let ref db = crate::Database::default();

        // Use non-copy types (lists) to properly test conditional drop behavior.
        let source = r#"
fun test(cond: bool): [u32]
    let x = [42]
    let y = [10]
    if cond
        ret x
    else
        ret y
    end if
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);
        let moved_analysis = analyze_moves_per_block(db, func, cfg, &slot_alloc.slots(db), move_info);
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, moved_analysis, tycheck_result);

        // With per-block move analysis:
        // - In then branch: x is moved (ret x), y is not moved → y needs drop
        // - In else branch: y is moved (ret y), x is not moved → x needs drop
        // So we should have 2 drop points total (one for each exit path).
        let drops = drop_points.drops(db);

        let x_name = InternedText::new(db, "x");
        let y_name = InternedText::new(db, "y");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        let y_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(y_name)).unwrap();

        let x_drops: Vec<_> = drops.iter().filter(|d| d.slot_id(db) == x_slot.slot_id(db)).collect();
        let y_drops: Vec<_> = drops.iter().filter(|d| d.slot_id(db) == y_slot.slot_id(db)).collect();

        // x should be dropped in the else branch (where it's not moved).
        assert_eq!(x_drops.len(), 1, "x should have drop in else branch");
        // y should be dropped in the then branch (where it's not moved).
        assert_eq!(y_drops.len(), 1, "y should have drop in then branch");
    }

    #[test]
    fn test_drop_multiple_locals() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): [u32]
    let a = [1]
    let b = [2]
    let c = [3]
    ret a
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);
        let moved_analysis = analyze_moves_per_block(db, func, cfg, &slot_alloc.slots(db), move_info);
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, moved_analysis, tycheck_result);

        // a is moved, b and c are not moved and are non-copy, so b and c should be dropped.
        let drops = drop_points.drops(db);

        let b_name = InternedText::new(db, "b");
        let c_name = InternedText::new(db, "c");
        let b_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(b_name)).unwrap();
        let c_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(c_name)).unwrap();

        let b_drops: Vec<_> = drops.iter()
            .filter(|d| d.slot_id(db) == b_slot.slot_id(db))
            .collect();
        let c_drops: Vec<_> = drops.iter()
            .filter(|d| d.slot_id(db) == c_slot.slot_id(db))
            .collect();

        assert_eq!(b_drops.len(), 1, "b should have exactly one drop point");
        assert_eq!(c_drops.len(), 1, "c should have exactly one drop point");
        assert_eq!(b_drops[0].reason(db), DropReason::EndOfScope);
        assert_eq!(c_drops[0].reason(db), DropReason::EndOfScope);
    }
}
