//! Liveness analysis and initialization tracking.

use rmx::prelude::*;
use std::collections::HashMap;
use super::{SlotId, ProgramPoint, InitState, BlockId, StmtId, SlotKind};
use super::cfg::ControlFlowGraph;
use super::slot_allocation::AllocatedSlot;
use crate::ast::{Statement, StmtFun};

/// Live ranges for all slots.
#[salsa::tracked]
pub struct LiveRanges<'db> {
    #[returns(ref)]
    pub ranges: Vec<LiveRange<'db>>,
}

/// Live range for a single slot.
#[salsa::tracked]
pub struct LiveRange<'db> {
    pub slot_id: SlotId,
    pub birth: ProgramPoint,    // where value is written
    pub death: ProgramPoint,    // last use
    pub is_initialized: InitState,
}

impl<'db> LiveRanges<'db> {
    /// Get the live range for a slot.
    pub fn get_range(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Option<LiveRange<'db>> {
        self.ranges(db).iter().find(|r| r.slot_id(db) == slot_id).copied()
    }
}

/// Initialization analysis result.
///
/// Tracks initialization state for each slot at block boundaries.
#[salsa::tracked]
pub struct InitializationAnalysis<'db> {
    /// State of each slot at entry to each block.
    #[returns(ref)]
    pub entry_states: Vec<(BlockId, Vec<InitState>)>,
    /// State of each slot at exit from each block.
    #[returns(ref)]
    pub exit_states: Vec<(BlockId, Vec<InitState>)>,
}

impl<'db> InitializationAnalysis<'db> {
    /// Get the initialization state at block entry.
    pub fn get_entry_state(
        self,
        db: &'db dyn crate::Db,
        block_id: BlockId,
        slot_id: SlotId,
    ) -> Option<InitState> {
        self.entry_states(db)
            .iter()
            .find(|(bid, _)| *bid == block_id)
            .and_then(|(_, states)| states.get(slot_id.0 as usize))
            .copied()
    }

    /// Get the initialization state at block exit.
    pub fn get_exit_state(
        self,
        db: &'db dyn crate::Db,
        block_id: BlockId,
        slot_id: SlotId,
    ) -> Option<InitState> {
        self.exit_states(db)
            .iter()
            .find(|(bid, _)| *bid == block_id)
            .and_then(|(_, states)| states.get(slot_id.0 as usize))
            .copied()
    }
}

impl InitState {
    /// Merge two initialization states (for CFG joins).
    fn merge(self, other: InitState) -> InitState {
        use InitState::*;
        match (self, other) {
            (Always, Always) => Always,
            (Never, Never) => Never,
            _ => Sometimes,
        }
    }

    /// Merge multiple initialization states.
    fn merge_all(states: impl IntoIterator<Item = InitState>) -> InitState {
        let mut iter = states.into_iter();
        let first = iter.next().unwrap_or(InitState::Never);
        iter.fold(first, |acc, s| acc.merge(s))
    }
}

/// Analyze initialization of slots across a function's CFG.
#[salsa::tracked]
pub fn analyze_initialization<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    cfg: ControlFlowGraph<'db>,
    slots: &'db [AllocatedSlot<'db>],
) -> InitializationAnalysis<'db> {
    let slot_count = slots.len();
    let blocks = cfg.blocks(db);

    // Initialize entry/exit states for all blocks using HashMap temporarily.
    let mut entry_map: HashMap<BlockId, Vec<InitState>> = HashMap::new();
    let mut exit_map: HashMap<BlockId, Vec<InitState>> = HashMap::new();

    // Initial state: Reference slots are always initialized, others are never initialized.
    let initial_state: Vec<InitState> = slots
        .iter()
        .map(|slot| {
            if slot.kind == SlotKind::Reference {
                InitState::Always
            } else {
                InitState::Never
            }
        })
        .collect();

    // Entry block starts with initial state.
    if !blocks.is_empty() {
        entry_map.insert(blocks[0].block_id, initial_state.clone());
    }

    // Initialize all blocks with Never for non-entry blocks.
    for block in blocks {
        if !entry_map.contains_key(&block.block_id) {
            entry_map.insert(block.block_id, vec![InitState::Never; slot_count]);
        }
        exit_map.insert(block.block_id, vec![InitState::Never; slot_count]);
    }

    // Fixed-point iteration: propagate initialization through CFG.
    let mut changed = true;
    while changed {
        changed = false;

        for block in blocks {
            // Get entry state for this block.
            let entry = entry_map.get(&block.block_id).unwrap().clone();

            // Compute exit state by executing statements.
            let mut state = entry.clone();
            compute_block_exit_state(db, func, &block.statements, &mut state, slots);

            // Update exit state if changed.
            let old_exit = exit_map.get(&block.block_id).unwrap();
            if &state != old_exit {
                exit_map.insert(block.block_id, state.clone());
                changed = true;
            }

            // Propagate to successor blocks.
            for edge in cfg.edges(db) {
                if edge.from == block.block_id {
                    let successor_entry = entry_map.get_mut(&edge.to).unwrap();
                    let block_exit = exit_map.get(&block.block_id).unwrap();

                    // Merge states: for each slot, merge with current successor entry.
                    for i in 0..slot_count {
                        let merged = successor_entry[i].merge(block_exit[i]);
                        if merged != successor_entry[i] {
                            successor_entry[i] = merged;
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    // Convert HashMap to Vec for Salsa storage.
    let entry_states: Vec<(BlockId, Vec<InitState>)> =
        entry_map.into_iter().collect();
    let exit_states: Vec<(BlockId, Vec<InitState>)> =
        exit_map.into_iter().collect();

    InitializationAnalysis::new(db, entry_states, exit_states)
}

/// Compute the exit state for a block by executing its statements.
fn compute_block_exit_state<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    stmt_ids: &[StmtId],
    state: &mut Vec<InitState>,
    slots: &[AllocatedSlot<'db>],
) {
    // Build a flattened list of all statements (including nested ones).
    let all_stmts = flatten_statements(db, func.body(db));

    for &stmt_id in stmt_ids {
        let stmt_idx = stmt_id.0 as usize;
        if stmt_idx >= all_stmts.len() {
            continue;
        }

        match *all_stmts[stmt_idx] {
            Statement::Let(let_stmt) => {
                // Find the slot for this let binding.
                if let Some(slot) = find_slot_by_name(slots, let_stmt.name(db)) {
                    state[slot.0 as usize] = InitState::Always;
                }
            }
            Statement::If(if_stmt) => {
                // If-bindings create initialized slots in their respective branches.
                // This is handled by the CFG - different blocks for then/else.
                // Here we just mark if-bindings as potentially initialized.
                if let Some(then_name) = if_stmt.then_binding(db) {
                    if let Some(slot) = find_slot_by_name(slots, then_name) {
                        state[slot.0 as usize] = InitState::Sometimes;
                    }
                }
                if let Some(else_name) = if_stmt.else_binding(db) {
                    if let Some(slot) = find_slot_by_name(slots, else_name) {
                        state[slot.0 as usize] = InitState::Sometimes;
                    }
                }
            }
            _ => {
                // Other statements don't initialize slots.
            }
        }
    }
}

/// Flatten all statements (including nested ones) into a single vec.
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

/// Find a slot by its name.
fn find_slot_by_name<'db>(
    slots: &[AllocatedSlot<'db>],
    name: bct::text::InternedText<'db>,
) -> Option<SlotId> {
    slots
        .iter()
        .find(|slot| slot.name == Some(name))
        .map(|slot| slot.slot_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bct::input::Source;
    use crate::function_analysis::cfg::build_cfg;
    use crate::function_analysis::slot_allocation::SlotAllocation;

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
    fn test_init_simple_linear() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: In u32)
    let y = x
    ret y
end fun
        "#;

        let func = parse_function(db, source);
        let allocation = SlotAllocation::analyze_function(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &allocation.slots);

        // x is a Reference slot (parameter) - Always initialized.
        // y is a Local slot - becomes Always after let statement.
        let x_slot = allocation.slots[0].slot_id;
        let y_slot = allocation.slots[1].slot_id;

        let entry_block = cfg.blocks(db)[0].block_id;

        // At entry: x is Always (parameter), y is Never.
        assert_eq!(init.get_entry_state(db, entry_block, x_slot), Some(InitState::Always));
        assert_eq!(init.get_entry_state(db, entry_block, y_slot), Some(InitState::Never));

        // At exit: both x and y are Always.
        assert_eq!(init.get_exit_state(db, entry_block, x_slot), Some(InitState::Always));
        assert_eq!(init.get_exit_state(db, entry_block, y_slot), Some(InitState::Always));
    }

    #[test]
    fn test_init_conditional() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(cond: In bool)
    if cond
        let x = @42
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let allocation = SlotAllocation::analyze_function(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &allocation.slots);

        let x_slot = allocation.slots[1].slot_id;
        let blocks = cfg.blocks(db);

        // Entry block: x is Never.
        assert_eq!(init.get_entry_state(db, blocks[0].block_id, x_slot), Some(InitState::Never));
    }

    #[test]
    fn test_init_if_with_else() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(cond: In bool)
    if cond
        let x = @1
    else
        let y = @2
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let allocation = SlotAllocation::analyze_function(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &allocation.slots);

        let x_slot = allocation.slots[1].slot_id;  // x from then-branch
        let y_slot = allocation.slots[2].slot_id;  // y from else-branch

        let blocks = cfg.blocks(db);

        // Entry: both x and y are Never.
        assert_eq!(init.get_entry_state(db, blocks[0].block_id, x_slot), Some(InitState::Never));
        assert_eq!(init.get_entry_state(db, blocks[0].block_id, y_slot), Some(InitState::Never));

        // Then block (has let x).
        let then_block = &blocks[1];
        assert_eq!(init.get_exit_state(db, then_block.block_id, x_slot), Some(InitState::Always));
        assert_eq!(init.get_exit_state(db, then_block.block_id, y_slot), Some(InitState::Never));

        // Else block (has let y).
        let else_block = &blocks[2];
        assert_eq!(init.get_exit_state(db, else_block.block_id, x_slot), Some(InitState::Never));
        assert_eq!(init.get_exit_state(db, else_block.block_id, y_slot), Some(InitState::Always));

        // Join block: x is Sometimes, y is Sometimes.
        let join_block = &blocks[3];
        assert_eq!(init.get_entry_state(db, join_block.block_id, x_slot), Some(InitState::Sometimes));
        assert_eq!(init.get_entry_state(db, join_block.block_id, y_slot), Some(InitState::Sometimes));
    }

    #[test]
    fn test_init_both_branches() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(cond: In bool)
    if cond
        let x = @1
    else
        let y = @2
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let allocation = SlotAllocation::analyze_function(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &allocation.slots);

        let x_slot = allocation.slots[1].slot_id;  // x from then-branch
        let y_slot = allocation.slots[2].slot_id;  // y from else-branch
        let blocks = cfg.blocks(db);

        // Entry: both x and y slots are Never.
        assert_eq!(init.get_entry_state(db, blocks[0].block_id, x_slot), Some(InitState::Never));
        assert_eq!(init.get_entry_state(db, blocks[0].block_id, y_slot), Some(InitState::Never));

        // After if (join block): both are Sometimes (each initialized in one branch only).
        let join_block = &blocks[3];
        assert_eq!(init.get_entry_state(db, join_block.block_id, x_slot), Some(InitState::Sometimes));
        assert_eq!(init.get_entry_state(db, join_block.block_id, y_slot), Some(InitState::Sometimes));
    }

    #[test]
    fn test_init_multiple_lets() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(a: In u32)
    let b = a
    let c = b
    let d = c
    ret d
end fun
        "#;

        let func = parse_function(db, source);
        let allocation = SlotAllocation::analyze_function(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &allocation.slots);

        let a_slot = allocation.slots[0].slot_id;
        let b_slot = allocation.slots[1].slot_id;
        let c_slot = allocation.slots[2].slot_id;
        let d_slot = allocation.slots[3].slot_id;

        let entry_block = cfg.blocks(db)[0].block_id;

        // Entry: only a is initialized (parameter).
        assert_eq!(init.get_entry_state(db, entry_block, a_slot), Some(InitState::Always));
        assert_eq!(init.get_entry_state(db, entry_block, b_slot), Some(InitState::Never));
        assert_eq!(init.get_entry_state(db, entry_block, c_slot), Some(InitState::Never));
        assert_eq!(init.get_entry_state(db, entry_block, d_slot), Some(InitState::Never));

        // Exit: all are initialized.
        assert_eq!(init.get_exit_state(db, entry_block, a_slot), Some(InitState::Always));
        assert_eq!(init.get_exit_state(db, entry_block, b_slot), Some(InitState::Always));
        assert_eq!(init.get_exit_state(db, entry_block, c_slot), Some(InitState::Always));
        assert_eq!(init.get_exit_state(db, entry_block, d_slot), Some(InitState::Always));
    }

    #[test]
    fn test_init_nested_if() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(a: In bool, b: In bool)
    if a
        if b
            let x = @1
        end if
    end if
    ret @0
end fun
        "#;

        let func = parse_function(db, source);
        let allocation = SlotAllocation::analyze_function(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &allocation.slots);

        // x is only initialized in the innermost then-branch.
        let x_slot = allocation.slots[2].slot_id;

        // Find the innermost then block.
        let blocks = cfg.blocks(db);
        let inner_then = &blocks[2];  // Should be the block with let x.

        // x is Always at exit of inner then block.
        assert_eq!(init.get_exit_state(db, inner_then.block_id, x_slot), Some(InitState::Always));

        // x is Sometimes after outer if (only initialized on one path).
        let final_block = blocks.last().unwrap();
        assert_eq!(init.get_entry_state(db, final_block.block_id, x_slot), Some(InitState::Sometimes));
    }
}
