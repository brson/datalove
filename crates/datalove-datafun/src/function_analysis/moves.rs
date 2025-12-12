//! Move tracking for linear type system.

use rmx::prelude::*;
use bct::text::InternedText;
use std::collections::HashMap;
use crate::ast::{Statement, StmtFun, ExprFun, ExprFunKind, ParamMode};
use super::{SlotId, ExprId, StmtId, LiveRanges, ProgramPoint};
use super::slot_allocation::AllocatedSlot;

/// Information about a read operation.
#[derive(Clone, Debug)]
struct ReadInfo {
    slot_id: SlotId,
    stmt_id: StmtId,
    expr_id: ExprId,
}

/// Move information for all operations.
#[salsa::tracked]
pub struct MoveInfo<'db> {
    #[returns(ref)]
    pub moves: Vec<MoveOp<'db>>,
    #[returns(ref)]
    pub last_uses: Vec<(SlotId, ExprId)>,
}

/// A single move operation.
#[salsa::tracked]
pub struct MoveOp<'db> {
    pub expr_id: ExprId,
    pub stmt_id: StmtId,
    pub slot_id: SlotId,
    pub move_kind: MoveKind,
}

/// Kind of move operation.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum MoveKind {
    FunctionCall,      // argument moved to callee
    FunctionReturn,    // return value moved to caller
    Assignment,        // let binding consumes value
    LastUse,          // last use optimization
    Copy,             // automatic copy for scalar types (bool, u32, i32, f32)
    // Clone will be added later for explicit .clone() operations
}

impl<'db> MoveInfo<'db> {
    /// Get all moves for a specific slot.
    pub fn moves_for_slot(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Vec<MoveOp<'db>> {
        self.moves(db).iter().filter(|m| m.slot_id(db) == slot_id).copied().collect()
    }

    /// Check if an expression is a last use of a slot.
    pub fn is_last_use(self, db: &'db dyn crate::Db, slot_id: SlotId, expr_id: ExprId) -> bool {
        self.last_uses(db).contains(&(slot_id, expr_id))
    }
}

/// Function registry for resolving function calls to their definitions.
struct FunctionRegistry<'db> {
    functions: HashMap<InternedText<'db>, StmtFun<'db>>,
}

impl<'db> FunctionRegistry<'db> {
    /// Build a function registry from a list of statements.
    fn build(db: &'db dyn crate::Db, statements: &[Statement<'db>]) -> Self {
        let mut functions = HashMap::new();

        for stmt in statements {
            if let Statement::Fun(fun) = stmt {
                functions.insert(fun.name(db), *fun);
            }
        }

        FunctionRegistry { functions }
    }

    /// Look up a function by name.
    fn lookup(&self, name: InternedText<'db>) -> Option<StmtFun<'db>> {
        self.functions.get(&name).copied()
    }
}

/// Compute move information for a function.
#[salsa::tracked]
pub fn compute_move_info<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &'db [AllocatedSlot<'db>],
    live_ranges: LiveRanges<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> MoveInfo<'db> {
    let mut moves = Vec::new();
    let mut reads = Vec::new();

    // Build function registry from function body for resolving calls.
    let registry = FunctionRegistry::build(db, func.body(db));

    // Counter for assigning ExprIds and StmtIds.
    let mut expr_counter = 0u32;
    let mut stmt_counter = 0u32;

    // Walk through function body to identify moves and reads.
    walk_statements(
        db,
        func.body(db),
        &registry,
        slots,
        &mut moves,
        &mut reads,
        &mut expr_counter,
        &mut stmt_counter,
        tycheck_result,
        func,
    );

    // Correlate reads with death points to identify last uses.
    let last_uses = correlate_last_uses(db, &reads, live_ranges);

    MoveInfo::new(db, moves, last_uses)
}

/// Walk statements to collect move operations and reads.
fn walk_statements<'db>(
    db: &'db dyn crate::Db,
    statements: &[Statement<'db>],
    registry: &FunctionRegistry<'db>,
    slots: &[AllocatedSlot<'db>],
    moves: &mut Vec<MoveOp<'db>>,
    reads: &mut Vec<ReadInfo>,
    expr_counter: &mut u32,
    stmt_counter: &mut u32,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
    func: StmtFun<'db>,
) {
    for stmt in statements {
        let stmt_id = StmtId(*stmt_counter);
        *stmt_counter += 1;

        match stmt {
            Statement::Let(let_stmt) => {
                // Let binding: RHS is moved to LHS slot.
                let value_expr = let_stmt.value(db);
                let name = let_stmt.name(db);

                // Find the slot for this let binding.
                if let Some(slot_id) = find_slot_by_name(db, slots, name) {
                    // Collect all moves from the RHS expression.
                    collect_moves_from_expr(
                        db,
                        value_expr,
                        slot_id,
                        MoveKind::Assignment,
                        stmt_id,
                        registry,
                        slots,
                        moves,
                        expr_counter,
                        tycheck_result,
                        func,
                    );
                    // Also collect reads from the RHS expression.
                    collect_reads_from_expr(
                        db,
                        value_expr,
                        stmt_id,
                        slots,
                        reads,
                        expr_counter,
                    );
                }
            }
            Statement::Ret(ret_stmt) => {
                // Return statement: returned value is moved to caller.
                let value_expr = ret_stmt.value(db);
                let expr_id = ExprId(*expr_counter);
                *expr_counter += 1;

                // Collect moves from the return expression.
                collect_moves_from_expr_for_return(
                    db,
                    value_expr,
                    expr_id,
                    stmt_id,
                    registry,
                    slots,
                    moves,
                    expr_counter,
                    tycheck_result,
                    func,
                );
                // Also collect reads from the return expression.
                collect_reads_from_expr(
                    db,
                    value_expr,
                    stmt_id,
                    slots,
                    reads,
                    expr_counter,
                );
            }
            Statement::If(if_stmt) => {
                // Collect reads from the condition.
                collect_reads_from_expr(
                    db,
                    if_stmt.condition(db),
                    stmt_id,
                    slots,
                    reads,
                    expr_counter,
                );

                // Process then and else branches.
                walk_statements(db, if_stmt.then_body(db), registry, slots, moves, reads, expr_counter, stmt_counter, tycheck_result, func);
                if let Some(else_body) = if_stmt.else_body(db) {
                    walk_statements(db, else_body, registry, slots, moves, reads, expr_counter, stmt_counter, tycheck_result, func);
                }
            }
            Statement::Loop(loop_stmt) => {
                // Process loop body.
                walk_statements(db, loop_stmt.body(db), registry, slots, moves, reads, expr_counter, stmt_counter, tycheck_result, func);
            }
            Statement::Break(_) | Statement::Continue(_) => {
                // No moves or reads in control flow statements.
            }
            Statement::Fun(_) | Statement::Require(_) | Statement::Import(_) | Statement::ParseError(_) => {
                // No moves or reads in these statements.
            }
        }
    }
}

/// Collect moves from an expression (for let bindings).
fn collect_moves_from_expr<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFun<'db>,
    target_slot: SlotId,
    move_kind: MoveKind,
    stmt_id: StmtId,
    registry: &FunctionRegistry<'db>,
    slots: &[AllocatedSlot<'db>],
    moves: &mut Vec<MoveOp<'db>>,
    expr_counter: &mut u32,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
    func: StmtFun<'db>,
) {
    let expr_id = ExprId(*expr_counter);
    *expr_counter += 1;

    match expr.expr(db) {
        ExprFunKind::Name(name) => {
            // Name expression: this is a move of the named slot.
            if let Some(source_slot) = find_slot_by_name(db, slots, name) {
                // Get slot info and type to check copyability.
                let slot_info = slots.iter()
                    .find(|s| s.slot_id(db) == source_slot)
                    .expect("slot should exist");
                let slot_type = super::copyability::get_slot_type(db, slot_info, tycheck_result, func);

                // Determine actual move kind based on copyability.
                let actual_move_kind = if super::copyability::is_copy_type(db, slot_type) {
                    MoveKind::Copy
                } else {
                    move_kind  // Use the passed-in kind.
                };

                moves.push(MoveOp::new(db, expr_id, stmt_id, source_slot, actual_move_kind));
            }
        }
        ExprFunKind::FunctionCall(call) => {
            // Function call: arguments might be moved depending on parameter modes.
            process_function_call(db, call, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::Tuple(tuple) => {
            // Tuple: each element might be moved.
            for element in tuple.elements(db) {
                collect_moves_from_expr(
                    db,
                    *element,
                    target_slot,
                    move_kind,
                    stmt_id,
                    registry,
                    slots,
                    moves,
                    expr_counter,
                    tycheck_result,
                    func,
                );
            }
        }
        ExprFunKind::BinOp(binop) => {
            // Binary operation: both operands might be moved.
            collect_moves_from_expr(db, binop.lhs(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            collect_moves_from_expr(db, binop.rhs(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::UnaryOp(unary) => {
            // Unary operation: operand might be moved.
            collect_moves_from_expr(db, unary.operand(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::TryOption(try_op) => {
            // Try option: operand might be moved.
            collect_moves_from_expr(db, try_op.operand(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::TryResult(try_op) => {
            // Try result: operand might be moved.
            collect_moves_from_expr(db, try_op.operand(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::ParseError(_) => {
            // No moves in parse errors.
        }

        // Inline literal variants - collect moves from nested expressions.
        ExprFunKind::True(_) |
        ExprFunKind::False(_) |
        ExprFunKind::None(_) |
        ExprFunKind::Int(_) |
        ExprFunKind::Float(_) |
        ExprFunKind::Hex(_) |
        ExprFunKind::String(_) => {
            // Simple literals have no moves.
        }
        ExprFunKind::List(list) => {
            for elem in list.elements(db) {
                collect_moves_from_expr(db, *elem, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Set(set) => {
            for elem in set.elements(db) {
                collect_moves_from_expr(db, *elem, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Map(map) => {
            for entry in map.entries(db) {
                collect_moves_from_expr(db, entry.key(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
                collect_moves_from_expr(db, entry.value(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Tensor(tensor) => {
            for elem in tensor.elements(db) {
                collect_moves_from_expr(db, *elem, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::AnonTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_moves_from_expr(db, *elem, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::NamedTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_moves_from_expr(db, *elem, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::AnonStruct(s) => {
            for field in s.fields(db) {
                collect_moves_from_expr(db, field.value(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::NamedStruct(s) => {
            for field in s.fields(db) {
                collect_moves_from_expr(db, field.value(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::AnonEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_moves_from_expr(db, payload, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::NamedEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_moves_from_expr(db, payload, target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Data(d) => {
            collect_moves_from_expr(db, d.value(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::Err(e) => {
            collect_moves_from_expr(db, e.value(db), target_slot, move_kind, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
    }
}

/// Collect moves from an expression for return statements.
fn collect_moves_from_expr_for_return<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFun<'db>,
    return_expr_id: ExprId,
    stmt_id: StmtId,
    registry: &FunctionRegistry<'db>,
    slots: &[AllocatedSlot<'db>],
    moves: &mut Vec<MoveOp<'db>>,
    expr_counter: &mut u32,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
    func: StmtFun<'db>,
) {
    match expr.expr(db) {
        ExprFunKind::Name(name) => {
            // Name expression: this is a move of the named slot for return.
            if let Some(slot) = find_slot_by_name(db, slots, name) {
                // Get slot info and type to check copyability.
                let slot_info = slots.iter()
                    .find(|s| s.slot_id(db) == slot)
                    .expect("slot should exist");
                let slot_type = super::copyability::get_slot_type(db, slot_info, tycheck_result, func);

                // Determine actual move kind based on copyability.
                let actual_move_kind = if super::copyability::is_copy_type(db, slot_type) {
                    MoveKind::Copy
                } else {
                    MoveKind::FunctionReturn
                };

                moves.push(MoveOp::new(db, return_expr_id, stmt_id, slot, actual_move_kind));
            }
        }
        ExprFunKind::FunctionCall(call) => {
            // Function call: the result is moved to return, but also process arguments.
            process_function_call(db, call, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::Tuple(tuple) => {
            // Tuple: each element might be moved.
            for element in tuple.elements(db) {
                collect_moves_from_expr_for_return(
                    db,
                    *element,
                    return_expr_id,
                    stmt_id,
                    registry,
                    slots,
                    moves,
                    expr_counter,
                    tycheck_result,
                    func,
                );
            }
        }
        ExprFunKind::BinOp(binop) => {
            // Binary operation: both operands might be moved.
            collect_moves_from_expr_for_return(db, binop.lhs(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            collect_moves_from_expr_for_return(db, binop.rhs(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::UnaryOp(unary) => {
            // Unary operation: operand might be moved.
            collect_moves_from_expr_for_return(db, unary.operand(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::TryOption(try_op) => {
            collect_moves_from_expr_for_return(db, try_op.operand(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::TryResult(try_op) => {
            collect_moves_from_expr_for_return(db, try_op.operand(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::ParseError(_) => {
            // No moves in parse errors.
        }

        // Inline literal variants - collect moves from nested expressions.
        ExprFunKind::True(_) |
        ExprFunKind::False(_) |
        ExprFunKind::None(_) |
        ExprFunKind::Int(_) |
        ExprFunKind::Float(_) |
        ExprFunKind::Hex(_) |
        ExprFunKind::String(_) => {
            // Simple literals have no moves.
        }
        ExprFunKind::List(list) => {
            for elem in list.elements(db) {
                collect_moves_from_expr_for_return(db, *elem, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Set(set) => {
            for elem in set.elements(db) {
                collect_moves_from_expr_for_return(db, *elem, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Map(map) => {
            for entry in map.entries(db) {
                collect_moves_from_expr_for_return(db, entry.key(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
                collect_moves_from_expr_for_return(db, entry.value(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Tensor(tensor) => {
            for elem in tensor.elements(db) {
                collect_moves_from_expr_for_return(db, *elem, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::AnonTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_moves_from_expr_for_return(db, *elem, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::NamedTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_moves_from_expr_for_return(db, *elem, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::AnonStruct(s) => {
            for field in s.fields(db) {
                collect_moves_from_expr_for_return(db, field.value(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::NamedStruct(s) => {
            for field in s.fields(db) {
                collect_moves_from_expr_for_return(db, field.value(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::AnonEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_moves_from_expr_for_return(db, payload, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::NamedEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_moves_from_expr_for_return(db, payload, return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
            }
        }
        ExprFunKind::Data(d) => {
            collect_moves_from_expr_for_return(db, d.value(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
        ExprFunKind::Err(e) => {
            collect_moves_from_expr_for_return(db, e.value(db), return_expr_id, stmt_id, registry, slots, moves, expr_counter, tycheck_result, func);
        }
    }
}

/// Process a function call to identify moves in arguments.
fn process_function_call<'db>(
    db: &'db dyn crate::Db,
    call: crate::ast::ExprFunctionCall<'db>,
    stmt_id: StmtId,
    registry: &FunctionRegistry<'db>,
    slots: &[AllocatedSlot<'db>],
    moves: &mut Vec<MoveOp<'db>>,
    expr_counter: &mut u32,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
    func: StmtFun<'db>,
) {
    let name = call.name(db);
    let args = call.args(db);

    // Resolve the function to get parameter modes.
    if let Some(callee) = registry.lookup(name) {
        let params = callee.params(db);

        // Process each argument with its corresponding parameter mode.
        for (i, arg) in args.iter().enumerate() {
            if let Some(param) = params.get(i) {
                let expr_id = ExprId(*expr_counter);
                *expr_counter += 1;

                // If parameter mode is In, this is a move.
                if param.mode(db) == ParamMode::In {
                    // Check if the argument is a name (direct move).
                    if let ExprFunKind::Name(arg_name) = arg.expr(db) {
                        if let Some(slot_id) = find_slot_by_name(db, slots, arg_name) {
                            // Get slot info and type to check copyability.
                            let slot_info = slots.iter()
                                .find(|s| s.slot_id(db) == slot_id)
                                .expect("slot should exist");
                            let slot_type = super::copyability::get_slot_type(db, slot_info, tycheck_result, func);

                            // Determine actual move kind based on copyability.
                            let actual_move_kind = if super::copyability::is_copy_type(db, slot_type) {
                                MoveKind::Copy
                            } else {
                                MoveKind::FunctionCall
                            };

                            moves.push(MoveOp::new(db, expr_id, stmt_id, slot_id, actual_move_kind));
                        }
                    } else {
                        // For complex expressions, recursively collect moves.
                        // The target_slot is not meaningful here, use a dummy value.
                        collect_moves_from_expr(
                            db,
                            *arg,
                            SlotId(0), // Dummy
                            MoveKind::FunctionCall,
                            stmt_id,
                            registry,
                            slots,
                            moves,
                            expr_counter,
                            tycheck_result,
                            func,
                        );
                    }
                } else {
                    // For Ref/Mut/Out parameters, no move occurs.
                    // But we still need to recursively process the argument for nested calls.
                    // Actually, for now we won't track borrows, so skip.
                }
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
    reads: &mut Vec<ReadInfo>,
    expr_counter: &mut u32,
) {
    let expr_id = ExprId(*expr_counter);
    *expr_counter += 1;

    match expr.expr(db) {
        ExprFunKind::Name(name) => {
            // Name expression: this is a read of the named slot.
            if let Some(slot_id) = find_slot_by_name(db, slots, name) {
                reads.push(ReadInfo {
                    slot_id,
                    stmt_id,
                    expr_id,
                });
            }
        }
        ExprFunKind::BinOp(binop) => {
            // Binary operation: both operands are read.
            collect_reads_from_expr(db, binop.lhs(db), stmt_id, slots, reads, expr_counter);
            collect_reads_from_expr(db, binop.rhs(db), stmt_id, slots, reads, expr_counter);
        }
        ExprFunKind::UnaryOp(unary) => {
            // Unary operation: operand is read.
            collect_reads_from_expr(db, unary.operand(db), stmt_id, slots, reads, expr_counter);
        }
        ExprFunKind::FunctionCall(call) => {
            // Function call: all arguments are read.
            for arg in call.args(db) {
                collect_reads_from_expr(db, *arg, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::Tuple(tuple) => {
            // Tuple: all elements are read.
            for elem in tuple.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::TryOption(try_op) => {
            // Try option: operand is read.
            collect_reads_from_expr(db, try_op.operand(db), stmt_id, slots, reads, expr_counter);
        }
        ExprFunKind::TryResult(try_op) => {
            // Try result: operand is read.
            collect_reads_from_expr(db, try_op.operand(db), stmt_id, slots, reads, expr_counter);
        }
        ExprFunKind::ParseError(_) => {
            // No reads in parse errors.
        }

        // Inline literal variants - collect reads from nested expressions.
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
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::Set(set) => {
            for elem in set.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::Map(map) => {
            for entry in map.entries(db) {
                collect_reads_from_expr(db, entry.key(db), stmt_id, slots, reads, expr_counter);
                collect_reads_from_expr(db, entry.value(db), stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::Tensor(tensor) => {
            for elem in tensor.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::AnonTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::NamedTuple(tuple) => {
            for elem in tuple.elements(db) {
                collect_reads_from_expr(db, *elem, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::AnonStruct(s) => {
            for field in s.fields(db) {
                collect_reads_from_expr(db, field.value(db), stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::NamedStruct(s) => {
            for field in s.fields(db) {
                collect_reads_from_expr(db, field.value(db), stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::AnonEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_reads_from_expr(db, payload, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::NamedEnum(e) => {
            if let Some(payload) = e.payload(db) {
                collect_reads_from_expr(db, payload, stmt_id, slots, reads, expr_counter);
            }
        }
        ExprFunKind::Data(d) => {
            collect_reads_from_expr(db, d.value(db), stmt_id, slots, reads, expr_counter);
        }
        ExprFunKind::Err(e) => {
            collect_reads_from_expr(db, e.value(db), stmt_id, slots, reads, expr_counter);
        }
    }
}

/// Correlate reads with death points from liveness analysis to identify last uses.
fn correlate_last_uses<'db>(
    db: &'db dyn crate::Db,
    reads: &[ReadInfo],
    live_ranges: LiveRanges<'db>,
) -> Vec<(SlotId, ExprId)> {
    let mut last_uses = Vec::new();

    // Group reads by slot_id to find the last read for each slot.
    let mut reads_by_slot: HashMap<SlotId, Vec<&ReadInfo>> = HashMap::new();
    for read in reads {
        reads_by_slot.entry(read.slot_id).or_insert_with(Vec::new).push(read);
    }

    // For each slot, find its last read(s).
    for (slot_id, slot_reads) in reads_by_slot {
        if slot_reads.is_empty() {
            continue;
        }

        // Find the maximum stmt_id among all reads for this slot.
        let max_stmt_id = slot_reads.iter()
            .map(|r| r.stmt_id)
            .max()
            .unwrap();

        // All reads at that maximum stmt_id are last uses.
        for read in slot_reads {
            if read.stmt_id == max_stmt_id {
                last_uses.push((slot_id, read.expr_id));
            }
        }
    }

    last_uses
}

/// Find a slot by name.
fn find_slot_by_name<'db>(db: &'db dyn crate::Db, slots: &[AllocatedSlot<'db>], name: InternedText<'db>) -> Option<SlotId> {
    slots.iter()
        .find(|s| s.name(db) == Some(name))
        .map(|s| s.slot_id(db))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::function_analysis::slot_allocation::SlotAllocation;
    use crate::function_analysis::cfg::build_cfg;
    use crate::function_analysis::slot_allocation::allocate_slots;
    use crate::function_analysis::liveness::{compute_live_ranges, analyze_initialization};
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
    fn test_move_simple_let_binding() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32)
    let y = x
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Verify we have one move: x -> y (Copy because u32 is copy).
        let moves = move_info.moves(db);
        assert_eq!(moves.len(), 1);

        let move_op = moves[0];
        assert_eq!(move_op.move_kind(db), MoveKind::Copy);

        // The moved slot should be x.
        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        assert_eq!(move_op.slot_id(db), x_slot.slot_id(db));
    }

    #[test]
    fn test_move_return_statement() {
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

        // Verify we have one move: x returned (FunctionReturn).
        let moves = move_info.moves(db);
        assert_eq!(moves.len(), 1);

        let move_op = moves[0];
        assert_eq!(move_op.move_kind(db), MoveKind::Copy); // u32 is copy

        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        assert_eq!(move_op.slot_id(db), x_slot.slot_id(db));
    }

    #[test]
    fn test_move_function_call_in_parameter() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32): u32
    fun helper(a: u32): u32
        ret a
    end fun
    ret helper(x)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Verify we have one move: x moved to helper (FunctionCall).
        let moves = move_info.moves(db);
        assert_eq!(moves.len(), 1);

        let move_op = moves[0];
        assert_eq!(move_op.move_kind(db), MoveKind::Copy); // u32 is copy

        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        assert_eq!(move_op.slot_id(db), x_slot.slot_id(db));
    }

    #[test]
    fn test_move_function_call_ref_parameter() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32): u32
    fun helper(ref a: u32): u32
        ret @0
    end fun
    ret helper(x)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Verify we have NO moves because parameter is Ref, not In.
        let moves = move_info.moves(db);
        assert_eq!(moves.len(), 0);
    }

    #[test]
    fn test_move_nested_function_calls() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32, y: u32): u32
    fun helper(a: u32): u32
        ret a
    end fun
    let z = helper(x)
    ret helper(y)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Verify we have two moves: x and y to helper calls (Copy because u32 is copy).
        let moves = move_info.moves(db);
        assert_eq!(moves.len(), 2);

        // Both should be Copy moves.
        assert_eq!(moves[0].move_kind(db), MoveKind::Copy);
        assert_eq!(moves[1].move_kind(db), MoveKind::Copy);

        // Find slots for x and y.
        let x_name = InternedText::new(db, "x");
        let y_name = InternedText::new(db, "y");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        let y_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(y_name)).unwrap();

        // Collect moved slot IDs.
        let moved_slots: Vec<SlotId> = moves.iter().map(|m| m.slot_id(db)).collect();

        // Both x and y should be moved.
        assert!(moved_slots.contains(&x_slot.slot_id(db)));
        assert!(moved_slots.contains(&y_slot.slot_id(db)));
    }

    #[test]
    fn test_move_tuple_construction() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32, y: u32)
    let z = (x, y)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Verify we have two moves: x and y into the tuple (Copy because u32 is copy).
        let moves = move_info.moves(db);
        assert_eq!(moves.len(), 2);

        // Both should be Copy moves.
        assert_eq!(moves[0].move_kind(db), MoveKind::Copy);
        assert_eq!(moves[1].move_kind(db), MoveKind::Copy);

        let x_name = InternedText::new(db, "x");
        let y_name = InternedText::new(db, "y");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        let y_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(y_name)).unwrap();

        let moved_slots: Vec<SlotId> = moves.iter().map(|m| m.slot_id(db)).collect();
        assert!(moved_slots.contains(&x_slot.slot_id(db)));
        assert!(moved_slots.contains(&y_slot.slot_id(db)));
    }

    #[test]
    fn test_last_use_simple_linear() {
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

        // Verify we have identified the last use of x.
        let last_uses = move_info.last_uses(db);
        assert_eq!(last_uses.len(), 1);

        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();

        // Check that x is marked as having a last use.
        let has_last_use = last_uses.iter().any(|(slot_id, _)| *slot_id == x_slot.slot_id(db));
        assert!(has_last_use, "x should have a last use");
    }

    #[test]
    fn test_last_use_multiple_reads() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32): u32
    let y = x +! x
    ret y
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // x is read twice in the same statement, so we should have 2 last uses (one for each read at the death point).
        let last_uses = move_info.last_uses(db);

        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();

        // Count how many times x appears in last_uses.
        let x_last_use_count = last_uses.iter().filter(|(slot_id, _)| *slot_id == x_slot.slot_id(db)).count();
        assert_eq!(x_last_use_count, 2, "x should have 2 last uses (both reads in the same statement)");
    }

    #[test]
    fn test_last_use_with_conditional() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32): u32
    if @true
        ret x
    else
        ret @0
    end if
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // x is used in the then branch, and that should be its last use.
        let last_uses = move_info.last_uses(db);

        let x_name = InternedText::new(db, "x");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();

        // Check that x has a last use.
        let has_last_use = last_uses.iter().any(|(slot_id, _)| *slot_id == x_slot.slot_id(db));
        assert!(has_last_use, "x should have a last use in the then branch");
    }

    #[test]
    fn test_last_use_parameter_multiple_uses() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32): u32
    let a = x
    let b = a
    ret b
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Each variable (x, a, b) should have exactly one last use.
        let last_uses = move_info.last_uses(db);

        let x_name = InternedText::new(db, "x");
        let a_name = InternedText::new(db, "a");
        let b_name = InternedText::new(db, "b");

        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        let a_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(a_name)).unwrap();
        let b_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(b_name)).unwrap();

        // Check that each slot has exactly one last use.
        let x_count = last_uses.iter().filter(|(slot_id, _)| *slot_id == x_slot.slot_id(db)).count();
        let a_count = last_uses.iter().filter(|(slot_id, _)| *slot_id == a_slot.slot_id(db)).count();
        let b_count = last_uses.iter().filter(|(slot_id, _)| *slot_id == b_slot.slot_id(db)).count();

        assert_eq!(x_count, 1, "x should have exactly one last use");
        assert_eq!(a_count, 1, "a should have exactly one last use");
        assert_eq!(b_count, 1, "b should have exactly one last use");
    }

    #[test]
    fn test_last_use_in_tuple() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(x: u32, y: u32)
    let z = (x, y)
end fun
        "#;

        let (func, tycheck_result) = parse_and_typecheck(db, source);
        let slot_alloc = allocate_slots(db, func);
        let cfg = build_cfg(db, func);
        let init = analyze_initialization(db, func, cfg, &slot_alloc.slots(db));
        let live_ranges = compute_live_ranges(db, func, cfg, &slot_alloc.slots(db), init);
        let move_info = compute_move_info(db, func, &slot_alloc.slots(db), live_ranges, tycheck_result);

        // Both x and y should have last uses when they're used in the tuple.
        let last_uses = move_info.last_uses(db);

        let x_name = InternedText::new(db, "x");
        let y_name = InternedText::new(db, "y");
        let x_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(x_name)).unwrap();
        let y_slot = slot_alloc.slots(db).iter().find(|s| s.name(db) == Some(y_name)).unwrap();

        let x_has_last_use = last_uses.iter().any(|(slot_id, _)| *slot_id == x_slot.slot_id(db));
        let y_has_last_use = last_uses.iter().any(|(slot_id, _)| *slot_id == y_slot.slot_id(db));

        assert!(x_has_last_use, "x should have a last use");
        assert!(y_has_last_use, "y should have a last use");
    }
}
