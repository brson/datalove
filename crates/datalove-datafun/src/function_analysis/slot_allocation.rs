//! Slot allocation for function frames.

use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::{Statement, StmtLet, StmtFun, StmtRet, StmtIf, ExprFun, ExprFunKind};
use super::{SlotId, SlotKind};

/// Result of slot allocation.
#[salsa::tracked]
pub struct SlotAllocation<'db> {
    /// All allocated slots.
    #[returns(ref)]
    pub slots: Vec<AllocatedSlot<'db>>,
}

/// Internal builder for slot allocation.
struct SlotAllocationBuilder<'db> {
    /// All allocated slots.
    slots: Vec<(SlotId, Option<String>, SlotKind, Option<ExprFun<'db>>)>,
    /// Next slot ID to allocate.
    next_slot_id: u32,
}

/// A slot that has been allocated.
#[salsa::tracked]
pub struct AllocatedSlot<'db> {
    pub slot_id: SlotId,
    pub name: Option<InternedText<'db>>,
    pub kind: SlotKind,
    pub expr: Option<ExprFun<'db>>,
}

impl<'db> SlotAllocationBuilder<'db> {
    /// Create a new empty allocation.
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            next_slot_id: 0,
        }
    }

    /// Allocate a new slot.
    fn alloc_slot(&mut self, name: Option<String>, kind: SlotKind, expr: Option<ExprFun<'db>>) -> SlotId {
        let slot_id = SlotId(self.next_slot_id);
        self.next_slot_id += 1;
        self.slots.push((slot_id, name, kind, expr));
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
                // Allocate slot for the let binding.
                let name_str = let_stmt.name(db).text(db).to_string();
                self.alloc_slot(Some(name_str), SlotKind::Local, None);
                // Analyze the value expression.
                self.analyze_expr(db, let_stmt.value(db));
            }
            Statement::Fun(_) => {
                // Nested functions not yet supported.
            }
            Statement::Ret(ret_stmt) => {
                self.analyze_expr(db, ret_stmt.value(db));
            }
            Statement::If(if_stmt) => {
                self.analyze_expr(db, if_stmt.condition(db));

                // Allocate slot for then binding if present.
                if let Some(name) = if_stmt.then_binding(db) {
                    let name_str = name.text(db).to_string();
                    self.alloc_slot(Some(name_str), SlotKind::Local, None);
                }
                self.analyze_statements(db, if_stmt.then_body(db));

                // Allocate slot for else binding if present.
                if let Some(name) = if_stmt.else_binding(db) {
                    let name_str = name.text(db).to_string();
                    self.alloc_slot(Some(name_str), SlotKind::Local, None);
                }
                if let Some(else_body) = if_stmt.else_body(db) {
                    self.analyze_statements(db, else_body);
                }
            }
            Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No slots needed.
            }
        }
    }

    /// Analyze an expression and allocate temporaries.
    fn analyze_expr(&mut self, db: &'db dyn crate::Db, expr: ExprFun<'db>) {
        match expr.expr(db) {
            ExprFunKind::Datalit(_) => {
                // Literals might need temporaries, but for now we'll handle them later.
            }
            ExprFunKind::Name(_) => {
                // Variable reference, no temporary needed.
            }
            ExprFunKind::BinOp(binop) => {
                self.analyze_expr(db, binop.lhs(db));
                self.analyze_expr(db, binop.rhs(db));
                // Binary operation needs a temporary for its result.
                self.alloc_slot(None, SlotKind::Temporary, Some(expr));
            }
            ExprFunKind::FunctionCall(call) => {
                for arg in call.args(db) {
                    self.analyze_expr(db, *arg);
                }
                // Function call needs a temporary for its result.
                self.alloc_slot(None, SlotKind::Temporary, Some(expr));
            }
            ExprFunKind::Tuple(tuple) => {
                for elem in tuple.elements(db) {
                    self.analyze_expr(db, *elem);
                }
                // Tuple construction needs a temporary.
                self.alloc_slot(None, SlotKind::Temporary, Some(expr));
            }
            ExprFunKind::UnaryOp(unary) => {
                self.analyze_expr(db, unary.operand(db));
                // Unary operation needs a temporary.
                self.alloc_slot(None, SlotKind::Temporary, Some(expr));
            }
            ExprFunKind::TryOption(try_opt) => {
                self.analyze_expr(db, try_opt.operand(db));
                // Try operation needs a temporary.
                self.alloc_slot(None, SlotKind::Temporary, Some(expr));
            }
            ExprFunKind::TryResult(try_res) => {
                self.analyze_expr(db, try_res.operand(db));
                // Try operation needs a temporary.
                self.alloc_slot(None, SlotKind::Temporary, Some(expr));
            }
            ExprFunKind::ParseError(_) => {
                // No slots needed.
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

    // Allocate reference slots for all parameters.
    // All parameters (In/Out/Ref/Mut) are passed by reference.
    for param in func.params(db) {
        let name_str = param.name(db).text(db).to_string();
        builder.alloc_slot(Some(name_str), SlotKind::Reference, None);
    }

    // Allocate slots for body statements.
    builder.analyze_statements(db, func.body(db));

    // Convert builder slots to AllocatedSlot Salsa structs.
    let slots = builder.slots.into_iter().map(|(slot_id, name, kind, expr)| {
        let interned_name = name.map(|n| InternedText::new(db, n));
        AllocatedSlot::new(db, slot_id, interned_name, kind, expr)
    }).collect();

    SlotAllocation::new(db, slots)
}
