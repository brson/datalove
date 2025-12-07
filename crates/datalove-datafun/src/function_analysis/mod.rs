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
    /// Expression temporaries - actual storage in callee's frame.
    /// Uses actual type size/alignment. Dropped at end of scope if not moved.
    Temporary,
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
    let init_analysis = liveness::analyze_initialization(db, func, control_flow, slots);

    // Phase 4: Live ranges.
    let live_ranges = liveness::compute_live_ranges(db, func, control_flow, slots, init_analysis);

    // Phase 5: Move tracking.
    let move_info = moves::compute_move_info(db, func, slots, live_ranges, tycheck_result);

    // Phase 6: Drop points.
    let drop_points = drops::compute_drop_points(db, func, control_flow, slots, init_analysis, move_info, tycheck_result);

    // Phase 7: Frame layout with types.
    let frame_layout = build_frame_layout(db, func, slots, tycheck_result);

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
    slots: &[slot_allocation::AllocatedSlot<'db>],
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> FrameLayout<'db> {
    use salsa::plumbing::AsId;
    use crate::ast::{Statement, ExprFun};

    let expr_types = tycheck_result.expr_types(db);

    // Build a map from slot names to their types.
    let mut slots_with_types = Vec::new();

    for slot in slots {
        let ty = match slot.kind(db) {
            SlotKind::Reference => {
                // Parameter - get type from parameter type hint.
                get_param_type(db, func, slot.name(db))
            }
            SlotKind::Local => {
                // Let binding - get type from RHS expression.
                get_local_type(db, func, slot.name(db), expr_types)
            }
            SlotKind::Temporary => {
                // Temporary - look up type from the creating expression.
                if let Some(expr) = slot.expr(db) {
                    use salsa::plumbing::AsId;
                    let expr_id = expr.as_id();
                    let index = expr_id.index() as usize;

                    expr_types.get(index)
                        .and_then(|opt| *opt)
                        .unwrap_or_else(|| create_placeholder_type(db))
                } else {
                    // No expression tracked, use placeholder.
                    create_placeholder_type(db)
                }
            }
        };

        slots_with_types.push((slot.slot_id(db), slot.name(db), slot.kind(db), ty, slot.expr(db)));
    }

    FrameLayout::compute_layout(db, slots_with_types)
}

/// Get type for a parameter slot.
fn get_param_type<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    param_name: Option<InternedText<'db>>,
) -> crate::tycheck::TypeAndHeap<'db> {
    if let Some(name) = param_name {
        for param in func.params(db) {
            if param.name(db) == name {
                // Convert type hint to type.
                let type_hint = param.type_hint(db);
                return convert_type_hint_to_type(db, type_hint);
            }
        }
    }
    // Fallback to placeholder if param not found.
    create_placeholder_type(db)
}

/// Get type for a local (let binding or if-binding) slot.
fn get_local_type<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    local_name: Option<InternedText<'db>>,
    expr_types: &[Option<crate::tycheck::TypeAndHeap<'db>>],
) -> crate::tycheck::TypeAndHeap<'db> {
    use salsa::plumbing::AsId;
    use crate::ast::{Statement, ExprFun};

    if let Some(name) = local_name {
        // Find the statement with this name (let or if-binding).
        if let Some(ty) = find_local_type_recursive(db, func.body(db), name, expr_types) {
            return ty;
        }
    }
    // Fallback to placeholder if local not found or no type.
    create_placeholder_type(db)
}

/// Recursively search for a local's type in statement list.
fn find_local_type_recursive<'db>(
    db: &'db dyn crate::Db,
    stmts: &[crate::ast::Statement<'db>],
    name: InternedText<'db>,
    expr_types: &[Option<crate::tycheck::TypeAndHeap<'db>>],
) -> Option<crate::tycheck::TypeAndHeap<'db>> {
    use salsa::plumbing::AsId;
    use crate::ast::Statement;
    use crate::tycheck::{Type, TypeAndHeap};

    for stmt in stmts {
        match stmt {
            Statement::Let(let_stmt) => {
                if let_stmt.name(db) == name {
                    // First, check if there's an explicit type hint.
                    if let Some(type_hint) = let_stmt.type_hint(db) {
                        return Some(convert_type_hint_to_type(db, type_hint));
                    }

                    // No type hint - get the type from the RHS expression.
                    let value_expr = let_stmt.value(db);
                    let expr_id = value_expr.as_id();
                    let index = expr_id.index() as usize;

                    if let Some(Some(ty)) = expr_types.get(index) {
                        return Some(*ty);
                    }
                }
            }
            Statement::If(if_stmt) => {
                // Check if this is a then_binding or else_binding.
                if Some(name) == if_stmt.then_binding(db) {
                    // Then binding type is the inner type of Option or Ok type of Result.
                    let condition = if_stmt.condition(db);
                    let expr_id = condition.as_id();
                    let index = expr_id.index() as usize;

                    if let Some(Some(cond_ty)) = expr_types.get(index) {
                        if let Type::Datalit(datalit_ty) = cond_ty.ty(db) {
                            match datalit_ty {
                                crate::datalit::tycheck::Type::Option(opt) => {
                                    let inner = opt.inner_type(db);
                                    return Some(TypeAndHeap::new(
                                        db,
                                        inner.heap(db),
                                        Type::Datalit(inner.ty(db).clone()),
                                    ));
                                }
                                crate::datalit::tycheck::Type::Result(res) => {
                                    let ok_ty = res.inner_type(db);
                                    return Some(TypeAndHeap::new(
                                        db,
                                        ok_ty.heap(db),
                                        Type::Datalit(ok_ty.ty(db).clone()),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if Some(name) == if_stmt.else_binding(db) {
                    // Else binding type is the Error type for Result.
                    // For now, return Error type.
                    return Some(TypeAndHeap::new(
                        db,
                        crate::datalit::ast::Heap::Local,
                        Type::Datalit(crate::datalit::tycheck::Type::Error),
                    ));
                }

                // Recurse into then and else bodies.
                if let Some(ty) = find_local_type_recursive(db, if_stmt.then_body(db), name, expr_types) {
                    return Some(ty);
                }
                if let Some(else_body) = if_stmt.else_body(db) {
                    if let Some(ty) = find_local_type_recursive(db, else_body, name, expr_types) {
                        return Some(ty);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Convert a type hint to a TypeAndHeap.
fn convert_type_hint_to_type<'db>(
    db: &'db dyn crate::Db,
    type_hint: crate::datalit::ast::TypeHintAndHeap<'db>,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::tycheck::{Type, TypeAndHeap};
    use crate::datalit;

    // Convert the type hint using datalit's conversion.
    let datalit_ty = datalit::tycheck::convert_type_hint(db, type_hint)
        .unwrap_or_else(|_| {
            // Fallback to bool if conversion fails.
            datalit::tycheck::TypeAndHeap::new(db, datalit::ast::Heap::Local, datalit::tycheck::Type::Bool)
        });

    TypeAndHeap::new(db, datalit_ty.heap(db), Type::Datalit(datalit_ty.ty(db).clone()))
}

/// Create a placeholder type (bool on local heap) for slots without type info.
fn create_placeholder_type<'db>(
    db: &'db dyn crate::Db,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::tycheck::{Type, TypeAndHeap};
    use crate::datalit;

    TypeAndHeap::new(
        db,
        datalit::ast::Heap::Local,
        Type::Datalit(datalit::tycheck::Type::Bool)
    )
}
