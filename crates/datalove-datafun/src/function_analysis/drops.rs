//! Drop point insertion for resource management.

use rmx::prelude::*;
use std::collections::{HashMap, HashSet};
use super::{SlotId, ProgramPoint, StmtId, Position, SlotKind, InitState};
use super::cfg::{ControlFlowGraph, Terminator};
use super::liveness::{LiveRanges, InitializationAnalysis};
use super::moves::MoveInfo;
use super::slot_allocation::AllocatedSlot;
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
    pub location: ProgramPoint,
    pub reason: DropReason,
}

/// Reason for dropping a value.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum DropReason {
    EndOfScope,
    EarlyReturn,
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
#[salsa::tracked]
pub fn compute_drop_points<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    cfg: ControlFlowGraph<'db>,
    slots: &'db [AllocatedSlot<'db>],
    init_analysis: InitializationAnalysis<'db>,
    move_info: MoveInfo<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> DropPoints<'db> {
    let mut drops = Vec::new();
    let blocks = cfg.blocks(db);

    // Collect all moved slots.
    let moved_slots: HashSet<SlotId> = move_info.moves(db)
        .iter()
        .map(|m| m.slot_id(db))
        .collect();

    // For each exit block, insert drops for all slots that need dropping.
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

                    // If the slot was moved, no drop is needed.
                    if moved_slots.contains(&slot_id) {
                        continue;
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
                            ProgramPoint {
                                stmt_id: last_stmt,
                                position: Position::After,
                            },
                            reason,
                        ));
                    }
                }
            }
            Terminator::Branch { .. } | Terminator::Goto(_) => {
                // No drops at branches or gotos.
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
    use crate::function_analysis::slot_allocation::SlotAllocation;
    use crate::function_analysis::cfg::build_cfg;
    use crate::function_analysis::slot_allocation::allocate_slots;
    use crate::function_analysis::liveness::{compute_live_ranges, analyze_initialization};
    use crate::function_analysis::moves::compute_move_info;
    use bct::input::Source;
    use bct::text::InternedText;

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

    fn parse_and_typecheck<'db>(db: &'db dyn crate::Db, source_code: &str) -> (StmtFun<'db>, crate::tycheck::TypecheckResult<'db>) {
        let source = Source::new(db, S(source_code));
        let script = crate::parser::parse_for_diagnostics(db, source);
        let tycheck_result = crate::tycheck::type_check(db, script, vec![], vec![]);
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
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, move_info, tycheck_result);

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
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, move_info, tycheck_result);

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
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, move_info, tycheck_result);

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

        let source = r#"
fun test(cond: bool): u32
    let x = @42
    let y = @10
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
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, move_info, tycheck_result);

        // Both x and y are moved (x in then branch, y in else branch).
        // Current implementation tracks moves globally, so both are marked as moved.
        // Neither should have drop points.
        let drops = drop_points.drops(db);

        // Verify that slots marked as moved don't have drop points.
        assert_eq!(drops.len(), 0, "moved slots should not have drop points");
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
        let drop_points = compute_drop_points(db, func, cfg, &slot_alloc.slots(db), init, move_info, tycheck_result);

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
