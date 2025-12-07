//! Liveness analysis and initialization tracking.

use rmx::prelude::*;
use std::collections::HashMap;
use super::{SlotId, ProgramPoint, InitState, BlockId, StmtId, SlotKind, Position};
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
    pub fn merge(self, other: InitState) -> InitState {
        use InitState::*;
        match (self, other) {
            (Always, Always) => Always,
            (Never, Never) => Never,
            _ => Sometimes,
        }
    }

    /// Merge multiple initialization states.
    pub fn merge_all(states: impl IntoIterator<Item = InitState>) -> InitState {
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

    // Initial state: Reference slots (except Out parameters) are always initialized,
    // others are never initialized. Out parameters start uninitialized.
    let initial_state: Vec<InitState> = slots
        .iter()
        .map(|slot| {
            if slot.kind(db) == SlotKind::Reference {
                // Check if this is an Out parameter.
                if let Some(slot_name) = slot.name(db) {
                    for param in func.params(db) {
                        if param.name(db) == slot_name && param.mode(db) == crate::ast::ParamMode::Out {
                            return InitState::Never;
                        }
                    }
                }
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
                if let Some(slot) = find_slot_by_name(db, slots, let_stmt.name(db)) {
                    state[slot.0 as usize] = InitState::Always;
                }
            }
            Statement::If(if_stmt) => {
                // If-bindings create initialized slots in their respective branches.
                // This is handled by the CFG - different blocks for then/else.
                // Here we just mark if-bindings as potentially initialized.
                if let Some(then_name) = if_stmt.then_binding(db) {
                    if let Some(slot) = find_slot_by_name(db, slots, then_name) {
                        state[slot.0 as usize] = InitState::Sometimes;
                    }
                }
                if let Some(else_name) = if_stmt.else_binding(db) {
                    if let Some(slot) = find_slot_by_name(db, slots, else_name) {
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
    db: &'db dyn crate::Db,
    slots: &[AllocatedSlot<'db>],
    name: bct::text::InternedText<'db>,
) -> Option<SlotId> {
    slots
        .iter()
        .find(|slot| slot.name(db) == Some(name))
        .map(|slot| slot.slot_id(db))
}

/// Compute live ranges for all slots in a function.
///
/// This analyzes the CFG to determine birth (write) and death (last read) points
/// for each slot.
#[salsa::tracked]
pub fn compute_live_ranges<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    cfg: ControlFlowGraph<'db>,
    slots: &'db [AllocatedSlot<'db>],
    init: InitializationAnalysis<'db>,
) -> LiveRanges<'db> {
    use std::collections::HashMap;

    let blocks = cfg.blocks(db);
    let all_stmts = flatten_statements(db, func.body(db));

    // Track birth and death points for each slot.
    let mut birth_points: HashMap<SlotId, ProgramPoint> = HashMap::new();
    let mut last_use_points: HashMap<SlotId, ProgramPoint> = HashMap::new();

    // Reference slots (parameters) are born at function entry.
    if !blocks.is_empty() && !all_stmts.is_empty() {
        let entry_point = ProgramPoint {
            stmt_id: StmtId(0),
            position: Position::Before,
        };

        for slot in slots {
            if slot.kind(db) == SlotKind::Reference {
                birth_points.insert(slot.slot_id(db), entry_point);
            }
        }
    }

    // Walk through all blocks and statements to find birth and death points.
    for block in blocks {
        for &stmt_id in &block.statements {
            let stmt_idx = stmt_id.0 as usize;
            if stmt_idx >= all_stmts.len() {
                continue;
            }

            let stmt = all_stmts[stmt_idx];

            // Find birth points (where slots are written).
            match *stmt {
                Statement::Let(let_stmt) => {
                    // The let binding writes to a slot.
                    if let Some(slot_id) = find_slot_by_name(db, slots, let_stmt.name(db)) {
                        birth_points.insert(slot_id, ProgramPoint {
                            stmt_id,
                            position: Position::After,
                        });
                    }

                    // The RHS expression reads from slots.
                    collect_reads(db, let_stmt.value(db), stmt_id, slots, &mut last_use_points);
                }
                Statement::Ret(ret_stmt) => {
                    // Return reads from slots.
                    collect_reads(db, ret_stmt.value(db), stmt_id, slots, &mut last_use_points);
                }
                Statement::If(if_stmt) => {
                    // Condition reads from slots.
                    collect_reads(db, if_stmt.condition(db), stmt_id, slots, &mut last_use_points);

                    // If-bindings are birth points.
                    if let Some(then_name) = if_stmt.then_binding(db) {
                        if let Some(slot_id) = find_slot_by_name(db, slots, then_name) {
                            birth_points.insert(slot_id, ProgramPoint {
                                stmt_id,
                                position: Position::After,
                            });
                        }
                    }
                    if let Some(else_name) = if_stmt.else_binding(db) {
                        if let Some(slot_id) = find_slot_by_name(db, slots, else_name) {
                            birth_points.insert(slot_id, ProgramPoint {
                                stmt_id,
                                position: Position::After,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Reference slots (parameters) are live for entire function duration.
    // They die at function exit, regardless of their last use, because the caller
    // owns the data and the reference must remain valid throughout the call.
    if !blocks.is_empty() && !all_stmts.is_empty() {
        let last_stmt_id = StmtId((all_stmts.len() - 1) as u32);
        let exit_point = ProgramPoint {
            stmt_id: last_stmt_id,
            position: Position::After,
        };

        for slot in slots {
            if slot.kind(db) == SlotKind::Reference {
                // Override any earlier use points - parameters are live until function exit.
                last_use_points.insert(slot.slot_id(db), exit_point);
            }
        }
    }

    // Build live ranges from birth and death points.
    let mut ranges = Vec::new();

    for slot in slots {
        let slot_id = slot.slot_id(db);

        // Get initialization state from first block if available.
        let init_state = if !blocks.is_empty() {
            init.get_exit_state(db, blocks[0].block_id, slot_id)
                .unwrap_or(InitState::Never)
        } else {
            if slot.kind(db) == SlotKind::Reference {
                InitState::Always
            } else {
                InitState::Never
            }
        };

        // If we have both birth and death, create a live range.
        if let (Some(&birth), Some(&death)) = (birth_points.get(&slot_id), last_use_points.get(&slot_id)) {
            let range = LiveRange::new(db, slot_id, birth, death, init_state);
            ranges.push(range);
        }
    }

    LiveRanges::new(db, ranges)
}

/// Collect all reads from an expression.
fn collect_reads<'db>(
    db: &'db dyn crate::Db,
    expr: crate::ast::ExprFun<'db>,
    stmt_id: StmtId,
    slots: &[AllocatedSlot<'db>],
    last_use_points: &mut HashMap<SlotId, ProgramPoint>,
) {
    use crate::ast::ExprFunKind;

    match expr.expr(db) {
        ExprFunKind::Name(name) => {
            // This is a read of a named slot.
            if let Some(slot_id) = find_slot_by_name(db, slots, name) {
                let use_point = ProgramPoint {
                    stmt_id,
                    position: Position::Before,
                };
                // Always update to latest use (this becomes the last use).
                last_use_points.insert(slot_id, use_point);
            }
        }
        ExprFunKind::BinOp(binop) => {
            // Both sides of binary op are reads.
            collect_reads(db, binop.lhs(db), stmt_id, slots, last_use_points);
            collect_reads(db, binop.rhs(db), stmt_id, slots, last_use_points);
        }
        ExprFunKind::UnaryOp(unop) => {
            collect_reads(db, unop.operand(db), stmt_id, slots, last_use_points);
        }
        ExprFunKind::FunctionCall(call) => {
            // All arguments are reads.
            for arg in call.args(db) {
                collect_reads(db, *arg, stmt_id, slots, last_use_points);
            }
        }
        ExprFunKind::Tuple(tuple) => {
            // All tuple elements are reads.
            for elem in tuple.elements(db) {
                collect_reads(db, *elem, stmt_id, slots, last_use_points);
            }
        }
        ExprFunKind::TryOption(try_opt) => {
            collect_reads(db, try_opt.operand(db), stmt_id, slots, last_use_points);
        }
        ExprFunKind::TryResult(try_res) => {
            collect_reads(db, try_res.operand(db), stmt_id, slots, last_use_points);
        }
        ExprFunKind::Datalit(_) | ExprFunKind::ParseError(_) => {
            // Literals don't read from slots.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bct::input::Source;
    use crate::function_analysis::cfg::build_cfg;
    use crate::function_analysis::slot_allocation::allocate_slots;
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

    /// Find a slot by name in the slot allocation.
    fn find_slot_by_name<'db>(
        db: &'db dyn crate::Db,
        slot_alloc: &SlotAllocation<'db>,
        name: &str,
    ) -> SlotId {
        for slot in slot_alloc.slots(db) {
            if let Some(slot_name) = slot.name(db) {
                if slot_name.text(db) == name {
                    return slot.slot_id(db);
                }
            }
        }
        panic!("Slot '{}' not found", name);
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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        // x is a Reference slot (parameter) - Always initialized.
        // y is a Local slot - becomes Always after let statement.
        let x_slot = find_slot_by_name(db, &slot_alloc, "x");
        let y_slot = find_slot_by_name(db, &slot_alloc, "y");

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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let x_slot = find_slot_by_name(db, &slot_alloc, "x");
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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let x_slot = find_slot_by_name(db, &slot_alloc, "x");
        let y_slot = find_slot_by_name(db, &slot_alloc, "y");

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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let x_slot = find_slot_by_name(db, &slot_alloc, "x");
        let y_slot = find_slot_by_name(db, &slot_alloc, "y");
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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        let a_slot = find_slot_by_name(db, &slot_alloc, "a");
        let b_slot = find_slot_by_name(db, &slot_alloc, "b");
        let c_slot = find_slot_by_name(db, &slot_alloc, "c");
        let d_slot = find_slot_by_name(db, &slot_alloc, "d");

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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));

        // x is only initialized in the innermost then-branch.
        let x_slot = find_slot_by_name(db, &slot_alloc, "x");

        // Find the innermost then block.
        let blocks = cfg.blocks(db);
        let inner_then = &blocks[2];  // Should be the block with let x.

        // x is Always at exit of inner then block.
        assert_eq!(init.get_exit_state(db, inner_then.block_id, x_slot), Some(InitState::Always));

        // x is Sometimes after outer if (only initialized on one path).
        let final_block = blocks.last().unwrap();
        assert_eq!(init.get_entry_state(db, final_block.block_id, x_slot), Some(InitState::Sometimes));
    }

    #[test]
    fn test_liveness_simple_linear() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: In u32)
    let y = x
    ret y
end fun
        "#;

        let func = parse_function(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        // x is a Reference slot (parameter).
        let x_range = live_ranges.get_range(db, slot_alloc.slots(db)[0].slot_id(db)).unwrap();
        assert_eq!(x_range.slot_id(db), slot_alloc.slots(db)[0].slot_id(db));
        assert_eq!(x_range.birth(db).position, Position::Before);
        assert_eq!(x_range.death(db).position, Position::After);
        assert_eq!(x_range.is_initialized(db), InitState::Always);

        // y is a Local slot.
        let y_range = live_ranges.get_range(db, slot_alloc.slots(db)[1].slot_id(db)).unwrap();
        assert_eq!(y_range.slot_id(db), slot_alloc.slots(db)[1].slot_id(db));
        // Birth is after the let statement.
        assert_eq!(y_range.birth(db).position, Position::After);
        // Death is before the ret statement (where y is read).
        assert_eq!(y_range.death(db).position, Position::Before);
    }

    #[test]
    fn test_liveness_multiple_uses() {
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
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        // b is read once (in let c = b).
        let b_slot = find_slot_by_name(db, &slot_alloc, "b");
        let b_range = live_ranges.get_range(db, b_slot).unwrap();
        assert_eq!(b_range.birth(db).stmt_id, StmtId(0));  // let b = a
        assert_eq!(b_range.death(db).stmt_id, StmtId(1));  // let c = b

        // c is read once (in let d = c).
        let c_slot = find_slot_by_name(db, &slot_alloc, "c");
        let c_range = live_ranges.get_range(db, c_slot).unwrap();
        assert_eq!(c_range.birth(db).stmt_id, StmtId(1));  // let c = b
        assert_eq!(c_range.death(db).stmt_id, StmtId(2));  // let d = c

        // d is read once (in ret d).
        let d_slot = find_slot_by_name(db, &slot_alloc, "d");
        let d_range = live_ranges.get_range(db, d_slot).unwrap();
        assert_eq!(d_range.birth(db).stmt_id, StmtId(2));  // let d = c
        assert_eq!(d_range.death(db).stmt_id, StmtId(3));  // ret d
    }

    #[test]
    fn test_liveness_binary_op() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(a: In u32, b: In u32)
    let c = a +! b
    ret c
end fun
        "#;

        let func = parse_function(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        // a and b are Reference slots (parameters), so they're live for entire function.
        let a_range = live_ranges.get_range(db, slot_alloc.slots(db)[0].slot_id(db)).unwrap();
        let b_range = live_ranges.get_range(db, slot_alloc.slots(db)[1].slot_id(db)).unwrap();

        // Both are born at function entry.
        assert_eq!(a_range.birth(db).position, Position::Before);
        assert_eq!(b_range.birth(db).position, Position::Before);

        // Both die at function exit (Reference slots are live for entire function).
        assert_eq!(a_range.death(db).position, Position::After);
        assert_eq!(b_range.death(db).position, Position::After);

        // c is a Local slot, born when written, dies when read.
        let c_range = live_ranges.get_range(db, slot_alloc.slots(db)[2].slot_id(db)).unwrap();
        assert_eq!(c_range.birth(db).stmt_id, StmtId(0));  // let c = ...
        assert_eq!(c_range.death(db).stmt_id, StmtId(1));  // ret c
    }

    #[test]
    fn test_liveness_tuple() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(a: In u32, b: In u32)
    let c = (a, b)
    ret c
end fun
        "#;

        let func = parse_function(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        // a and b are Reference slots (parameters), live for entire function.
        let a_range = live_ranges.get_range(db, slot_alloc.slots(db)[0].slot_id(db)).unwrap();
        let b_range = live_ranges.get_range(db, slot_alloc.slots(db)[1].slot_id(db)).unwrap();

        // Both die at function exit (Reference slots are live for entire function).
        assert_eq!(a_range.death(db).position, Position::After);
        assert_eq!(b_range.death(db).position, Position::After);
    }

    #[test]
    fn test_liveness_parameter_spans_function() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: In u32)
    let a = @1
    let b = @2
    ret x
end fun
        "#;

        let func = parse_function(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);

        // x is a parameter, born at entry, dies at exit.
        let x_range = live_ranges.get_range(db, slot_alloc.slots(db)[0].slot_id(db)).unwrap();
        assert_eq!(x_range.birth(db).stmt_id, StmtId(0));
        assert_eq!(x_range.birth(db).position, Position::Before);

        // Last use is in ret x, which is statement 2.
        assert_eq!(x_range.death(db).stmt_id, StmtId(2));
    }
}
