//! Function analysis for linear type system.
//!
//! This module implements per-function analysis that provides:
//! - Packed frame layout with computed offsets for all slots
//! - Liveness ranges for each slot
//! - Move and borrow tracking
//! - Drop point insertion

use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::StmtFun;

mod cfg;
mod copyability;
mod layout;
mod liveness;
mod moves;
mod drops;
mod slot_allocation;
mod type_sizing;
mod validation;

pub use cfg::*;
pub use copyability::*;
pub use layout::*;
pub use liveness::*;
pub use moves::*;
pub use drops::*;
pub use slot_allocation::*;
pub use type_sizing::*;
pub use validation::*;

/// Complete analysis result for a function.
#[salsa::tracked]
pub struct FunctionAnalysis<'db> {
    pub function: StmtFun<'db>,
    pub frame_layout: FrameLayout<'db>,
    pub live_ranges: LiveRanges<'db>,
    pub move_info: MoveInfo<'db>,
    pub drop_points: DropPoints<'db>,
    pub control_flow: ControlFlowGraph<'db>,
    #[returns(ref)]
    pub errors: Vec<AnalysisError>,
}

/// Unique identifier for a slot in the frame.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct SlotId(pub u32);

/// Unique identifier for a statement.
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct StmtId(pub u32);

/// Unique identifier for an expression.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct ExprId(pub u32);

/// Unique identifier for a basic block.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct BlockId(pub u32);

/// Kind of slot in the frame.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum SlotKind {
    /// All parameters (In/Out/Ref/Mut) - pointer to caller's data.
    /// Always pointer-sized (8 bytes on 64-bit), but the `ty` field in SlotInfo
    /// holds the *referenced* type, not pointer-to-type.
    /// Live for entire function duration, never dropped (caller owns the data).
    Reference,
    /// Let bindings - actual storage in callee's frame.
    /// Uses actual type size/alignment. Dropped at end of scope if not moved.
    Local,
    /// Var bindings - mutable storage in callee's frame.
    /// Uses actual type size/alignment. Can be mutated via `set`.
    Mutable,
    /// Expression temporaries - actual storage in callee's frame.
    /// Uses actual type size/alignment. Dropped at end of scope if not moved.
    Temporary,
}

impl SlotKind {
    /// Derive ownership from slot kind.
    ///
    /// Reference slots are borrowed (caller owns the data).
    /// Local and Temporary slots are owned (frame must destroy at drop point).
    pub fn ownership(self) -> SlotOwnership {
        match self {
            SlotKind::Reference => SlotOwnership::Borrowed,
            SlotKind::Local | SlotKind::Mutable | SlotKind::Temporary => SlotOwnership::Owned,
        }
    }
}

/// Ownership status of a slot.
///
/// Determines who is responsible for destroying the slot's contents.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum SlotOwnership {
    /// Frame owns this slot, must destroy at drop point.
    Owned,
    /// Caller owns, frame must not destroy.
    Borrowed,
}

/// Position within a statement (before or after).
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum Position {
    Before,
    After,
}

/// Point in the program for liveness tracking.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct ProgramPoint {
    pub stmt_id: StmtId,
    pub position: Position,
}

/// Initialization state for a slot.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum InitState {
    Always,      // definitely initialized
    Sometimes,   // conditionally initialized (if-branches)
    Never,       // never initialized on this path
}

/// Main entry point for function analysis.
///
/// Orchestrates all analysis passes and produces a complete FunctionAnalysis result.
#[salsa::tracked]
pub fn analyze_function<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> FunctionAnalysis<'db> {
    // Phase 1: Build CFG.
    let control_flow = cfg::build_cfg(db, func);

    // Phase 2: Allocate slots.
    let slot_allocation = slot_allocation::allocate_slots(db, func);
    let slots = slot_allocation.slots(db);

    // Phase 3: Initialization analysis.
    let init_analysis = liveness::analyze_initialization(db, func, control_flow, slot_allocation);

    // Phase 4: Live ranges.
    let live_ranges = liveness::compute_live_ranges(db, func, control_flow, slot_allocation, init_analysis);

    // Phase 5: Move tracking.
    let move_info = moves::compute_move_info(db, func, slot_allocation, live_ranges, tycheck_result);

    // Phase 5.5: Per-block move analysis for precise drop points.
    let moved_analysis = moves::analyze_moves_per_block(db, func, control_flow, slots, move_info);

    // Phase 6: Drop points (using per-block move analysis).
    let drop_points = drops::compute_drop_points(db, func, control_flow, slots, slot_allocation, init_analysis, moved_analysis, tycheck_result);

    // Phase 6.5: Identify slots that need runtime tracking.
    // Include:
    // - Slots with conditional initialization (Sometimes init)
    // - Slots with conditional moves (Sometimes moved)
    // - Local and Temporary slots with NormalCleanup (may need drop points)
    // Exclude:
    // - InlineDestroyed slots (destroyed inline by interpreter, no runtime tracking needed)
    // - Reference (parameter) slots (static analysis sufficient for drop points,
    //   but non-copy Reference slots need tracking for argument cleanup).
    let mut tracked_set: std::collections::HashSet<SlotId> = std::collections::HashSet::new();
    tracked_set.extend(init_analysis.conditionally_initialized_slots(db));
    tracked_set.extend(moved_analysis.conditionally_moved_slots(db));
    // Include Local/Temporary slots with NormalCleanup.
    for slot in slots {
        if matches!(slot.kind(db), SlotKind::Local | SlotKind::Temporary)
            && slot.destruction(db) == slot_allocation::SlotDestruction::NormalCleanup
        {
            tracked_set.insert(slot.slot_id(db));
        }
    }

    // Phase 7: Frame layout with types.
    // Pass tracked_set so each SlotInfo can precompute needs_state_tracking.
    let frame_layout = build_frame_layout(db, func, slot_allocation, tycheck_result, &tracked_set);

    // Phase 8: Validation.
    let mut errors = Vec::new();
    errors.extend(validation::check_use_before_init(db, func, slots, init_analysis, control_flow));
    errors.extend(validation::check_double_move(db, func, move_info));
    errors.extend(validation::check_use_after_move(db, func, slots, move_info));
    errors.extend(validation::check_uninitialized_return(db, func, slots, init_analysis, control_flow));
    errors.extend(validation::check_value_not_used(db, slots, live_ranges));

    FunctionAnalysis::new(
        db,
        func,
        frame_layout,
        live_ranges,
        move_info,
        drop_points,
        control_flow,
        errors,
    )
}

/// Build frame layout by extracting types for slots.
fn build_frame_layout<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slot_allocation: slot_allocation::SlotAllocation<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
    tracked_slots: &std::collections::HashSet<SlotId>,
) -> FrameLayout<'db> {
    use salsa::plumbing::AsId;

    let expr_types = tycheck_result.expr_types(db);
    let slots = slot_allocation.slots(db);

    // Build reverse maps: slot_id -> statement for let/var/if-binding.
    let let_stmt_by_slot: std::collections::HashMap<_, _> = slot_allocation
        .let_stmt_slots(db)
        .iter()
        .map(|ls| (ls.slot_id(db), ls.stmt(db)))
        .collect();
    let var_stmt_by_slot: std::collections::HashMap<_, _> = slot_allocation
        .var_stmt_slots(db)
        .iter()
        .map(|vs| (vs.slot_id(db), vs.stmt(db)))
        .collect();
    let if_binding_by_slot: std::collections::HashMap<_, _> = slot_allocation
        .if_binding_slots(db)
        .iter()
        .map(|ibs| (ibs.slot_id(db), (ibs.stmt(db), ibs.is_then_binding(db))))
        .collect();

    let mut slots_with_types = Vec::new();

    for slot in slots {
        let slot_id = slot.slot_id(db);

        let ty = match slot.kind(db) {
            SlotKind::Reference => {
                get_param_type(db, func, slot.name(db))
            }
            SlotKind::Local => {
                // Local slots come from let statements or if-bindings.
                if let Some(let_stmt) = let_stmt_by_slot.get(&slot_id) {
                    get_type_from_stmt_rhs(let_stmt.value(db), expr_types)
                } else if let Some((if_stmt, is_then)) = if_binding_by_slot.get(&slot_id) {
                    get_type_from_if_binding(db, *if_stmt, *is_then, expr_types)
                } else {
                    panic!("Local slot must come from let statement or if-binding");
                }
            }
            SlotKind::Mutable => {
                let var_stmt = var_stmt_by_slot.get(&slot_id)
                    .expect("Mutable slot must have corresponding var statement");
                get_type_from_stmt_rhs(var_stmt.value(db), expr_types)
            }
            SlotKind::Temporary => {
                let expr = slot.expr(db).expect("Temporary slot must have expression");
                let expr_id = expr.as_id();
                let index = expr_id.index() as usize;
                // Use .get() because expr_types is a sparse vector indexed by global salsa IDs.
                // The vector may be smaller than the expression ID if this expression
                // wasn't processed by the typechecker (which would be a bug).
                match expr_types.get(index).copied().flatten() {
                    Some(ty) => ty,
                    None => panic!(
                        "Temporary expression must have type from typechecker. \
                         Expression ID {} but expr_types.len() = {}",
                        index, expr_types.len()
                    ),
                }
            }
        };

        slots_with_types.push((slot_id, slot.name(db), slot.kind(db), ty, slot.expr(db)));
    }

    FrameLayout::compute_layout(
        db,
        slots_with_types,
        slot_allocation.name_resolutions(db).clone(),
        slot_allocation.let_stmt_slots(db).clone(),
        slot_allocation.var_stmt_slots(db).clone(),
        slot_allocation.set_stmt_slots(db).clone(),
        slot_allocation.if_binding_slots(db).clone(),
        slot_allocation.struct_field_orders(db).clone(),
        tracked_slots,
    )
}

/// Get type from the RHS expression of a let/var statement.
fn get_type_from_stmt_rhs<'db>(
    value_expr: crate::ast::ExprFun<'db>,
    expr_types: &[Option<crate::tycheck::TypeAndHeap<'db>>],
) -> crate::tycheck::TypeAndHeap<'db> {
    use salsa::plumbing::AsId;
    let expr_id = value_expr.as_id();
    let index = expr_id.index() as usize;
    // Use .get() because expr_types is a sparse vector indexed by global salsa IDs.
    expr_types.get(index)
        .copied()
        .flatten()
        .expect("Statement RHS expression must have type from typechecker")
}

/// Get type for an if-statement binding.
///
/// For then-bindings: unwrap Option/Result to get inner type.
/// For else-bindings (Result only): get Error type.
pub(crate) fn get_type_from_if_binding<'db>(
    db: &'db dyn crate::Db,
    if_stmt: crate::ast::StmtIf<'db>,
    is_then_binding: bool,
    expr_types: &[Option<crate::tycheck::TypeAndHeap<'db>>],
) -> crate::tycheck::TypeAndHeap<'db> {
    use salsa::plumbing::AsId;
    use crate::tycheck::Type;
    use crate::datalit;

    // Get the condition expression type.
    let condition = if_stmt.condition(db);
    let expr_id = condition.as_id();
    let index = expr_id.index() as usize;
    let condition_ty = expr_types.get(index)
        .copied()
        .flatten()
        .expect("If condition expression must have type from typechecker");

    if is_then_binding {
        // Then-binding: unwrap Option or Result to get inner type.
        match condition_ty.ty(db) {
            Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                let inner = opt.inner_type(db);
                crate::tycheck::TypeAndHeap::new(
                    db,
                    inner.heap(db),
                    Type::Datalit(inner.ty(db).clone()),
                )
            }
            Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                let inner = res.inner_type(db);
                crate::tycheck::TypeAndHeap::new(
                    db,
                    inner.heap(db),
                    Type::Datalit(inner.ty(db).clone()),
                )
            }
            _ => panic!("If-binding condition must have Option or Result type"),
        }
    } else {
        // Else-binding: for Result, get Error type.
        match condition_ty.ty(db) {
            Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                crate::tycheck::TypeAndHeap::new(
                    db,
                    datalit::ast::Heap::Omitted,
                    Type::Datalit(datalit::tycheck::Type::Error),
                )
            }
            _ => panic!("If else-binding condition must have Result type"),
        }
    }
}

/// Get type for a parameter slot.
fn get_param_type<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    param_name: Option<InternedText<'db>>,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::tycheck::{Type, TypeAndHeap};
    use crate::datalit;

    let name = param_name.expect("Parameter slot must have a name");

    for param in func.params(db) {
        if param.name(db) == name {
            // Convert type hint to type using datalit's conversion.
            let type_hint = param.type_hint(db);
            let datalit_ty = datalit::tycheck::convert_type_hint(db, type_hint)
                .expect("Parameter type hint must be valid");
            return TypeAndHeap::new(db, datalit_ty.heap(db), Type::Datalit(datalit_ty.ty(db).clone()));
        }
    }

    panic!("Parameter slot '{}' not found in function parameters", name.text(db));
}

