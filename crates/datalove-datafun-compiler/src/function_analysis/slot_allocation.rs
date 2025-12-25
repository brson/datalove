//! Slot allocation for function frames.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::text::InternedText;
use crate::ast::{Statement, StmtFun, StmtLet, StmtVar, StmtSet, ExprFun, ExprFunKind};
use crate::datalit::ast::TypeHint;
use super::{SlotId, SlotKind};

/// How a slot's contents are cleaned up.
///
/// This determines whether the slot needs runtime tracking and drop points.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum SlotDestruction {
    /// Slot is destroyed inline by the interpreter during expression evaluation.
    /// No drop point needed, no runtime tracking needed.
    /// Examples: BinOp/UnaryOp operand temps, if-condition temps.
    InlineDestroyed,
    /// Slot is cleaned up at scope end via drop points.
    /// May need runtime tracking if conditionally initialized/moved.
    NormalCleanup,
}

/// Expression context for temp slot allocation.
///
/// Determines whether an expression needs its own temp slot or will use
/// a destination provided by its parent.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ExprContext {
    /// Parent provides a destination (let RHS, tuple element, etc.).
    /// Expression writes directly to parent's destination, no temp needed.
    HasDest,
    /// Expression must provide its own destination.
    /// Allocate a temp slot for the result.
    NeedsDest,
}

/// Result of slot allocation.
#[salsa::tracked]
pub struct SlotAllocation<'db> {
    /// All allocated slots.
    #[returns(ref)]
    pub slots: Vec<AllocatedSlot<'db>>,
    /// Maps Name expressions to their resolved slot IDs.
    #[returns(ref)]
    pub name_resolutions: Vec<super::layout::NameResolution<'db>>,
    /// Maps let statements to their destination slot IDs.
    #[returns(ref)]
    pub let_stmt_slots: Vec<super::layout::LetStmtSlot<'db>>,
    /// Maps var statements to their destination slot IDs.
    #[returns(ref)]
    pub var_stmt_slots: Vec<super::layout::VarStmtSlot<'db>>,
    /// Maps set statements to their target slot IDs.
    #[returns(ref)]
    pub set_stmt_slots: Vec<super::layout::SetStmtSlot<'db>>,
}

impl<'db> SlotAllocation<'db> {
    /// Get the slot ID for a Name expression.
    pub fn get_slot_for_name_expr(
        self,
        db: &'db dyn crate::Db,
        expr: ExprFun<'db>,
    ) -> Option<SlotId> {
        self.name_resolutions(db).iter()
            .find(|nr| nr.expr(db) == expr)
            .map(|nr| nr.slot_id(db))
    }

    /// Get the destination slot ID for a let statement.
    pub fn get_slot_for_let_stmt(
        self,
        db: &'db dyn crate::Db,
        stmt: StmtLet<'db>,
    ) -> Option<SlotId> {
        self.let_stmt_slots(db).iter()
            .find(|ls| ls.stmt(db) == stmt)
            .map(|ls| ls.slot_id(db))
    }

    /// Get the destination slot ID for a var statement.
    pub fn get_slot_for_var_stmt(
        self,
        db: &'db dyn crate::Db,
        stmt: StmtVar<'db>,
    ) -> Option<SlotId> {
        self.var_stmt_slots(db).iter()
            .find(|vs| vs.stmt(db) == stmt)
            .map(|vs| vs.slot_id(db))
    }

    /// Get the target slot ID for a set statement.
    pub fn get_slot_for_set_stmt(
        self,
        db: &'db dyn crate::Db,
        stmt: StmtSet<'db>,
    ) -> Option<SlotId> {
        self.set_stmt_slots(db).iter()
            .find(|ss| ss.stmt(db) == stmt)
            .map(|ss| ss.slot_id(db))
    }
}

/// Internal builder for slot allocation.
struct SlotAllocationBuilder<'db> {
    /// All allocated slots.
    slots: Vec<(SlotId, Option<String>, SlotKind, Option<ExprFun<'db>>, SlotDestruction)>,
    /// Next slot ID to allocate.
    next_slot_id: u32,
    /// Current scope: maps variable names to their most recent slot.
    scope: HashMap<InternedText<'db>, SlotId>,
    /// Maps Name expressions to their resolved slot IDs.
    name_resolutions: HashMap<ExprFun<'db>, SlotId>,
    /// Maps let statements to their destination slot IDs.
    let_stmt_slots: HashMap<StmtLet<'db>, SlotId>,
    /// Maps var statements to their destination slot IDs.
    var_stmt_slots: HashMap<StmtVar<'db>, SlotId>,
    /// Maps set statements to their target slot IDs.
    set_stmt_slots: HashMap<StmtSet<'db>, SlotId>,
}

/// A slot that has been allocated.
#[salsa::tracked]
pub struct AllocatedSlot<'db> {
    pub slot_id: SlotId,
    pub name: Option<InternedText<'db>>,
    pub kind: SlotKind,
    pub expr: Option<ExprFun<'db>>,
    /// How this slot's contents are cleaned up.
    pub destruction: SlotDestruction,
}

impl<'db> SlotAllocationBuilder<'db> {
    /// Create a new empty allocation.
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            next_slot_id: 0,
            scope: HashMap::new(),
            name_resolutions: HashMap::new(),
            let_stmt_slots: HashMap::new(),
            var_stmt_slots: HashMap::new(),
            set_stmt_slots: HashMap::new(),
        }
    }

    /// Allocate a new slot with normal cleanup.
    fn alloc_slot(&mut self, name: Option<String>, kind: SlotKind, expr: Option<ExprFun<'db>>) -> SlotId {
        self.alloc_slot_with_destruction(name, kind, expr, SlotDestruction::NormalCleanup)
    }

    /// Allocate a new slot that will be destroyed inline (no drop point needed).
    #[allow(dead_code)]
    fn alloc_slot_inline(&mut self, name: Option<String>, kind: SlotKind, expr: Option<ExprFun<'db>>) -> SlotId {
        self.alloc_slot_with_destruction(name, kind, expr, SlotDestruction::InlineDestroyed)
    }

    /// Allocate a new slot with specified destruction mode.
    fn alloc_slot_with_destruction(
        &mut self,
        name: Option<String>,
        kind: SlotKind,
        expr: Option<ExprFun<'db>>,
        destruction: SlotDestruction,
    ) -> SlotId {
        let slot_id = SlotId(self.next_slot_id);
        self.next_slot_id += 1;
        self.slots.push((slot_id, name, kind, expr, destruction));
        slot_id
    }

    /// Analyze a list of statements.
    fn analyze_statements(&mut self, db: &'db dyn crate::Db, stmts: &[Statement<'db>]) {
        for stmt in stmts {
            self.analyze_statement(db, stmt);
        }
    }

    /// Analyze a single statement.
    fn analyze_statement(&mut self, db: &'db dyn crate::Db, stmt: &Statement<'db>) {
        match stmt {
            Statement::Let(let_stmt) => {
                // Check if let has a coercible type hint (Option/Result/Data).
                // If so, the RHS needs a temp because interpreter evaluates without dest
                // first to check if coercion is needed.
                let needs_coercion_check = let_stmt.type_hint(db).map_or(false, |th| {
                    matches!(
                        th.type_hint(db),
                        TypeHint::Option(_) | TypeHint::Result(_) | TypeHint::Data
                    )
                });

                let rhs_ctx = if needs_coercion_check {
                    ExprContext::NeedsDest
                } else {
                    ExprContext::HasDest
                };

                // FIRST: Analyze RHS (uses current scope, before new slot).
                self.analyze_expr(db, let_stmt.value(db), rhs_ctx);

                // THEN: Allocate slot and update scope.
                let name = let_stmt.name(db);
                let name_str = name.text(db).to_string();
                let slot_id = self.alloc_slot(Some(name_str), SlotKind::Local, None);

                // Record statement -> slot mapping.
                self.let_stmt_slots.insert(*let_stmt, slot_id);

                // Update scope (shadowing any previous binding).
                self.scope.insert(name, slot_id);
            }
            Statement::Var(var_stmt) => {
                // Check if var has a coercible type hint.
                let needs_coercion_check = var_stmt.type_hint(db).map_or(false, |th| {
                    matches!(
                        th.type_hint(db),
                        TypeHint::Option(_) | TypeHint::Result(_) | TypeHint::Data
                    )
                });

                let rhs_ctx = if needs_coercion_check {
                    ExprContext::NeedsDest
                } else {
                    ExprContext::HasDest
                };

                // FIRST: Analyze RHS (uses current scope, before new slot).
                self.analyze_expr(db, var_stmt.value(db), rhs_ctx);

                // THEN: Allocate mutable slot and update scope.
                let name = var_stmt.name(db);
                let name_str = name.text(db).to_string();
                let slot_id = self.alloc_slot(Some(name_str), SlotKind::Mutable, None);

                // Record statement -> slot mapping.
                self.var_stmt_slots.insert(*var_stmt, slot_id);

                // Update scope (shadowing any previous binding).
                self.scope.insert(name, slot_id);
            }
            Statement::Set(set_stmt) => {
                // Set uses an existing mutable slot, no new slot allocation.
                // Look up target in scope and record mapping.
                let name = set_stmt.name(db);
                if let Some(&slot_id) = self.scope.get(&name) {
                    self.set_stmt_slots.insert(*set_stmt, slot_id);
                }

                // RHS writes directly to the existing slot.
                self.analyze_expr(db, set_stmt.value(db), ExprContext::HasDest);
            }
            Statement::Fun(_) => {
                // Nested functions not yet supported.
            }
            Statement::Ret(ret_stmt) => {
                // Return expression needs its own temp (value escapes frame).
                if let Some(value) = ret_stmt.value(db) {
                    self.analyze_expr(db, value, ExprContext::NeedsDest);
                }
            }
            Statement::If(if_stmt) => {
                // Condition needs its own temp for branching.
                // This temp is destroyed inline after branch evaluation.
                self.analyze_expr_inline(db, if_stmt.condition(db));

                // Allocate slot for then binding if present and add to scope.
                if let Some(name) = if_stmt.then_binding(db) {
                    let name_str = name.text(db).to_string();
                    let slot_id = self.alloc_slot(Some(name_str), SlotKind::Local, None);
                    self.scope.insert(name, slot_id);
                }
                self.analyze_statements(db, if_stmt.then_body(db));

                // Allocate slot for else binding if present and add to scope.
                if let Some(name) = if_stmt.else_binding(db) {
                    let name_str = name.text(db).to_string();
                    let slot_id = self.alloc_slot(Some(name_str), SlotKind::Local, None);
                    self.scope.insert(name, slot_id);
                }
                if let Some(else_body) = if_stmt.else_body(db) {
                    self.analyze_statements(db, else_body);
                }
            }
            Statement::Loop(loop_stmt) => {
                // Recursively analyze loop body.
                self.analyze_statements(db, loop_stmt.body(db));
            }
            Statement::Break(_) | Statement::Continue(_) => {
                // No slots needed for control flow statements.
            }
            Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No slots needed.
            }
        }
    }

    /// Analyze an expression that will be destroyed inline (e.g., if-condition).
    ///
    /// The top-level expression gets an InlineDestroyed temp, and subexpressions
    /// that need temps also get InlineDestroyed (since they're part of the same
    /// evaluation that's destroyed inline).
    fn analyze_expr_inline(&mut self, db: &'db dyn crate::Db, expr: ExprFun<'db>) {
        self.analyze_expr_with_destruction(db, expr, ExprContext::NeedsDest, SlotDestruction::InlineDestroyed);
    }

    /// Analyze an expression and allocate temporaries based on context.
    ///
    /// - HasDest: Parent provides destination, no temp needed for this expression.
    /// - NeedsDest: Expression must allocate its own temp slot.
    ///
    /// Note: Subexpressions may still need temps even if parent has dest.
    fn analyze_expr(&mut self, db: &'db dyn crate::Db, expr: ExprFun<'db>, ctx: ExprContext) {
        self.analyze_expr_with_destruction(db, expr, ctx, SlotDestruction::NormalCleanup);
    }

    /// Analyze an expression with specified destruction mode for allocated temps.
    fn analyze_expr_with_destruction(
        &mut self,
        db: &'db dyn crate::Db,
        expr: ExprFun<'db>,
        ctx: ExprContext,
        destruction: SlotDestruction,
    ) {
        match expr.expr(db) {
            ExprFunKind::Name(name) => {
                // Resolve name to slot using current scope.
                if let Some(&slot_id) = self.scope.get(&name) {
                    self.name_resolutions.insert(expr, slot_id);
                }

                // Name expressions never need their own temp:
                // - If HasDest: clones directly to parent's destination
                // - If NeedsDest + move type: returns borrowed ref to source
                // - If NeedsDest + copy type: needs temp, but we can't know type here
                //
                // For copy types with NeedsDest, we still need a temp. But since we
                // don't know types at allocation time, we allocate conservatively
                // only when NeedsDest.
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::BinOp(binop) => {
                // Operands always need temps (borrow semantics).
                // Operands are destroyed inline after the operation.
                self.analyze_expr_with_destruction(db, binop.lhs(db), ExprContext::NeedsDest, SlotDestruction::InlineDestroyed);
                self.analyze_expr_with_destruction(db, binop.rhs(db), ExprContext::NeedsDest, SlotDestruction::InlineDestroyed);
                // Result temp depends on context; uses caller's destruction mode.
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::FunctionCall(call) => {
                // Arguments need temps (evaluated before call).
                // Arguments inherit destruction mode from parent expression.
                for arg in call.args(db) {
                    self.analyze_expr_with_destruction(db, *arg, ExprContext::NeedsDest, destruction);
                }
                // Result temp depends on context.
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Tuple(tuple) => {
                // Elements write to tuple field offsets if parent has dest.
                for elem in tuple.elements(db) {
                    self.analyze_expr_with_destruction(db, *elem, ctx, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::UnaryOp(unary) => {
                // Operand needs temp (borrow semantics).
                // Operand is destroyed inline after the operation.
                self.analyze_expr_with_destruction(db, unary.operand(db), ExprContext::NeedsDest, SlotDestruction::InlineDestroyed);
                // Result temp depends on context; uses caller's destruction mode.
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::TryOption(try_opt) => {
                // Operand needs temp (for unwrapping).
                self.analyze_expr_with_destruction(db, try_opt.operand(db), ExprContext::NeedsDest, destruction);
                // Result temp depends on context.
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::TryResult(try_res) => {
                // Operand needs temp (for unwrapping).
                self.analyze_expr_with_destruction(db, try_res.operand(db), ExprContext::NeedsDest, destruction);
                // Result temp depends on context.
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::ParseError(_) => {
                // No slots needed.
            }

            // Simple literals - temp depends on context.
            ExprFunKind::True(_) |
            ExprFunKind::False(_) |
            ExprFunKind::None(_) |
            ExprFunKind::Int(_) |
            ExprFunKind::Float(_) |
            ExprFunKind::Hex(_) |
            ExprFunKind::String(_) => {
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }

            // Collection literals - propagate context to elements.
            ExprFunKind::List(list) => {
                for elem in list.elements(db) {
                    self.analyze_expr_with_destruction(db, *elem, ctx, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Set(set) => {
                // Set elements need their own temp slots (can't DPS into set structure).
                for elem in set.elements(db) {
                    self.analyze_expr_with_destruction(db, *elem, ExprContext::NeedsDest, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Map(map) => {
                // Map keys/values need their own temp slots (can't DPS into map structure).
                for entry in map.entries(db) {
                    self.analyze_expr_with_destruction(db, entry.key(db), ExprContext::NeedsDest, destruction);
                    self.analyze_expr_with_destruction(db, entry.value(db), ExprContext::NeedsDest, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Tensor(tensor) => {
                for elem in tensor.elements(db) {
                    self.analyze_expr_with_destruction(db, *elem, ctx, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::AnonTuple(tuple) => {
                for elem in tuple.elements(db) {
                    self.analyze_expr_with_destruction(db, *elem, ctx, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::AnonStruct(s) => {
                for field in s.fields(db) {
                    self.analyze_expr_with_destruction(db, field.value(db), ctx, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::AnonEnum(e) => {
                // Payload writes to enum data area if parent has dest.
                if let Some(payload) = e.payload(db) {
                    self.analyze_expr_with_destruction(db, payload, ctx, destruction);
                }
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Some(s) => {
                // Payload writes to option payload if parent has dest.
                self.analyze_expr_with_destruction(db, s.payload(db), ctx, destruction);
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Ok(o) => {
                // Payload writes to result payload if parent has dest.
                self.analyze_expr_with_destruction(db, o.payload(db), ctx, destruction);
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Er(e) => {
                // Payload needs its own temp slot.
                self.analyze_expr_with_destruction(db, e.payload(db), ExprContext::NeedsDest, destruction);
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Data(d) => {
                // Inner value needs its own temp slot.
                self.analyze_expr_with_destruction(db, d.value(db), ExprContext::NeedsDest, destruction);
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
            ExprFunKind::Err(e) => {
                // Inner value writes to error payload if parent has dest.
                self.analyze_expr_with_destruction(db, e.value(db), ctx, destruction);
                if ctx == ExprContext::NeedsDest {
                    self.alloc_slot_with_destruction(None, SlotKind::Temporary, Some(expr), destruction);
                }
            }
        }
    }
}

/// Analyze a function and allocate all slots.
#[salsa::tracked]
pub fn allocate_slots<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
) -> SlotAllocation<'db> {
    let mut builder = SlotAllocationBuilder::new();

    // Allocate reference slots for all parameters and add to scope.
    // All parameters (In/Out/Ref/Mut) are passed by reference.
    for param in func.params(db) {
        let name = param.name(db);
        let name_str = name.text(db).to_string();
        let slot_id = builder.alloc_slot(Some(name_str), SlotKind::Reference, None);
        builder.scope.insert(name, slot_id);
    }

    // Allocate slots for body statements.
    builder.analyze_statements(db, func.body(db));

    // Convert builder slots to AllocatedSlot Salsa structs.
    let slots = builder.slots.into_iter().map(|(slot_id, name, kind, expr, destruction)| {
        let interned_name = name.map(|n| InternedText::new(db, n));
        AllocatedSlot::new(db, slot_id, interned_name, kind, expr, destruction)
    }).collect();

    use super::layout::{NameResolution, LetStmtSlot, VarStmtSlot, SetStmtSlot};

    let name_resolutions: Vec<_> = builder.name_resolutions.into_iter()
        .map(|(expr, slot_id)| NameResolution::new(db, expr, slot_id))
        .collect();
    let let_stmt_slots: Vec<_> = builder.let_stmt_slots.into_iter()
        .map(|(stmt, slot_id)| LetStmtSlot::new(db, stmt, slot_id))
        .collect();
    let var_stmt_slots: Vec<_> = builder.var_stmt_slots.into_iter()
        .map(|(stmt, slot_id)| VarStmtSlot::new(db, stmt, slot_id))
        .collect();
    let set_stmt_slots: Vec<_> = builder.set_stmt_slots.into_iter()
        .map(|(stmt, slot_id)| SetStmtSlot::new(db, stmt, slot_id))
        .collect();

    SlotAllocation::new(
        db,
        slots,
        name_resolutions,
        let_stmt_slots,
        var_stmt_slots,
        set_stmt_slots,
    )
}
