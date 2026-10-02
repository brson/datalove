//! Salsa-tracked API for script unit lowering.
//!
//! Provides memoized, per-unit script lowering following the same pattern as
//! per-unit typechecking. Each unit is lowered independently, enabling
//! incremental recompilation when new units are added to a batch.

use rmx::prelude::*;
use std::collections::{HashMap, HashSet};
use datalove_datafun_ast::ast::{Statement, ExprFun, ExprFunKind};
use datalove_datafun_ir::{
    IrType, ConstValue, ExportBinding,
    ConstBindingInfo, ConstBindingGraph, ConstStmtId,
};
use datalove_datafun_tycheck::UnitTypecheckResultTracked;

use crate::lower;
use crate::IrTypeExt;

/// What one script unit's compilation left for the units after it.
///
/// **Held per unit rather than folded into a running total.** A fold describes
/// the whole prefix and nothing else, so after an edit it describes the
/// pre-edit suffix as well and there is no way to rebuild the context unit `i`
/// should be lowered against. Re-lowering a unit replaces its record and leaves
/// the others alone; the context for unit `i` is
/// [`lower_context_over`] applied to the records before it. See
/// `botdocs/plan-script-reactivity.md`.
#[derive(Clone, Default)]
pub struct UnitLowerRecord {
    /// The names this unit exported, as its `ScriptContext` recorded them.
    pub exports: Vec<(String, ExportBinding)>,
    /// The unit's value types, which say what an exported value is of.
    pub value_types: Vec<IrType>,
    /// The unit's slot types, which say what an exported slot is of.
    pub slot_types: Vec<IrType>,
    /// The script-level consts this unit declared, with their values.
    ///
    /// A script const outlives the unit that declared it and a later unit's
    /// function body may name one, so the values travel forward the way the
    /// bindings do.
    pub consts: Vec<(String, IrType, ConstValue)>,
    /// Names this unit exported without a value behind them.
    pub dead_exports: Vec<String>,
    /// Names from earlier units this unit assigned to.
    pub revived: Vec<String>,
    /// The names this unit provides, as the typechecker recorded them.
    ///
    /// Kept as owned text because the reach of an edit is computed across a
    /// database mutation, which nothing borrowed from salsa survives.
    pub provides: Vec<String>,
    /// Every name this unit asked its environment for.
    pub uses: Vec<String>,
    /// The modules this unit's imports resolve into, by path.
    ///
    /// The edge a module edit travels along, held here for the same reason
    /// `uses` is: the reach is walked after the database has been mutated, and
    /// nothing borrowed from salsa survives that.
    pub imports: Vec<String>,
}

/// The lowering context a unit sitting after `records` is lowered against.
///
/// Last writer wins by position, so the records are folded oldest first and a
/// later export shadows an earlier one of the same name whichever kind each is.
pub fn lower_context_over(records: &[UnitLowerRecord]) -> lower::ScriptLowerContext {
    let mut ctx = lower::ScriptLowerContext::new();
    for (index, record) in records.iter().enumerate() {
        ctx.add_exports(
            index as u32,
            &record.exports,
            &record.value_types,
            &record.slot_types,
        );
    }
    ctx.current_unit = records.len() as u32;
    ctx
}

/// The script-level consts in scope for a unit sitting after `records`.
pub fn script_consts_over(
    records: &[UnitLowerRecord],
) -> HashMap<String, (IrType, ConstValue)> {
    let mut consts = HashMap::new();
    for record in records {
        for (name, ty, value) in &record.consts {
            consts.insert(name.clone(), (ty.clone(), value.clone()));
        }
    }
    consts
}

/// The names earlier units exported and then gave away, for a unit after
/// `records`.
///
/// A name goes dead when the unit that exported it moved the value out before
/// it ended, and comes back to life when a later unit exports or assigns to it.
pub fn dead_externals_over(records: &[UnitLowerRecord]) -> Vec<String> {
    let mut dead: Vec<String> = Vec::new();
    for record in records {
        for (name, _) in &record.exports {
            dead.retain(|held| held != name);
        }
        for name in &record.revived {
            dead.retain(|held| held != name);
        }
        dead.extend(record.dead_exports.iter().cloned());
    }
    dead
}

// ============================================================================
// Phase 1: Collect Const Graph (Memoized)
// ============================================================================

/// Collect const bindings from statements and return them in dependency order.
///
/// This is Phase 1 of the 3-phase CTFE memoization pipeline:
/// 1. collect_const_graph (memoized) - pure data extraction
/// 2. evaluate_consts (not memoized) - needs interpreter
/// 3. lower_with_consts (memoized) - uses resolved values
///
/// The returned graph is hashable, enabling salsa memoization.
#[salsa::tracked(returns(clone))]
pub fn collect_const_graph<'db>(
    db: &'db dyn salsa::Database,
    statements: Vec<Statement<'db>>,
    typecheck_result: UnitTypecheckResultTracked<'db>,
) -> ConstBindingGraph {
    // First pass: collect all const bindings.
    let mut bindings = Vec::new();
    let mut const_names: HashSet<String> = HashSet::new();
    let mut name_to_stmt: HashMap<String, ConstStmtId> = HashMap::new();

    let expr_types = typecheck_result.expr_types(db);

    // The expression each binding was collected from, so that the second pass
    // does not have to go looking for it again.
    let mut binding_exprs = Vec::new();

    for stmt in &statements {
        if let Statement::Const(const_stmt) = stmt {
            let expr = const_stmt.value;
            // Const statements are numbered in the order they appear.
            let stmt_id = ConstStmtId(bindings.len() as u32);
            let name = const_stmt.name.text(db).to_string();

            // Get the type from the typechecker's table.
            let ir_type = expr_types
                .get(&datalove_datafun_ast::ast::ExprKey::of(db, expr))
                .map(|ty| IrType::from_tycheck(db, ty))
                .unwrap_or(IrType::Unit);

            const_names.insert(name.clone());
            name_to_stmt.insert(name.clone(), stmt_id);
            binding_exprs.push(expr);
            bindings.push(ConstBindingInfo {
                stmt_id,
                name,
                ir_type,
                depends_on: Vec::new(), // Filled in second pass.
            });
        }
    }

    // Second pass: analyze dependencies.
    for (binding, expr) in bindings.iter_mut().zip(&binding_exprs) {
        binding.depends_on = find_const_refs(db, *expr, &const_names, &name_to_stmt);
    }

    // Topological sort (dependencies before dependents).
    let sorted = topological_sort_consts(&bindings);

    ConstBindingGraph::new(sorted)
}

/// Find const name references in an expression.
fn find_const_refs<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    const_names: &HashSet<String>,
    name_to_stmt: &HashMap<String, ConstStmtId>,
) -> Vec<ConstStmtId> {
    let mut refs = Vec::new();
    find_const_refs_inner(db, expr, const_names, name_to_stmt, &mut refs);
    refs.sort();
    refs.dedup();
    refs
}

fn find_const_refs_inner<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    const_names: &HashSet<String>,
    name_to_stmt: &HashMap<String, ConstStmtId>,
    refs: &mut Vec<ConstStmtId>,
) {
    match expr.expr(db) {
        ExprFunKind::BinOp(binop) => {
            find_const_refs_inner(db, binop.lhs, const_names, name_to_stmt, refs);
            find_const_refs_inner(db, binop.rhs, const_names, name_to_stmt, refs);
        }
        ExprFunKind::UnaryOp(unop) => {
            find_const_refs_inner(db, unop.operand, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Tuple(t) => {
            for elem in &t.elements {
                find_const_refs_inner(db, *elem, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::List(l) => {
            for elem in &l.elements {
                find_const_refs_inner(db, *elem, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::Set(s) => {
            for elem in &s.elements {
                find_const_refs_inner(db, *elem, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::Map(m) => {
            for entry in &m.entries {
                find_const_refs_inner(db, entry.key, const_names, name_to_stmt, refs);
                find_const_refs_inner(db, entry.value, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::Tensor(t) => {
            for elem in &t.elements {
                find_const_refs_inner(db, *elem, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::Table(t) => {
            for row in &t.rows {
                for elem in &row.elements {
                    find_const_refs_inner(db, *elem, const_names, name_to_stmt, refs);
                }
            }
        }
        ExprFunKind::FunctionCall(call) => {
            for arg in call.args(db) {
                find_const_refs_inner(db, *arg, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::IntrinsicCall(call) => {
            for arg in &call.args {
                find_const_refs_inner(db, *arg, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::CloneCoerce(c) => {
            find_const_refs_inner(db, c.operand, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Hinted(h) => {
            find_const_refs_inner(db, h.inner, const_names, name_to_stmt, refs);
        }
        ExprFunKind::TryOption(t) => {
            find_const_refs_inner(db, t.operand, const_names, name_to_stmt, refs);
        }
        ExprFunKind::TryResult(t) => {
            find_const_refs_inner(db, t.operand, const_names, name_to_stmt, refs);
        }
        ExprFunKind::FieldProj(p) => {
            find_const_refs_inner(db, p.base, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Some(s) => {
            find_const_refs_inner(db, s.payload, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Ok(o) => {
            find_const_refs_inner(db, o.payload, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Er(e) => {
            find_const_refs_inner(db, e.payload, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Data(d) => {
            find_const_refs_inner(db, d.value, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Error(e) => {
            find_const_refs_inner(db, e.value, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Term(t) => {
            find_const_refs_inner(db, t.payload, const_names, name_to_stmt, refs);
        }
        ExprFunKind::EnumLiteral(e) => {
            find_const_refs_inner(db, e.variant, const_names, name_to_stmt, refs);
        }
        ExprFunKind::Index(i) => {
            find_const_refs_inner(db, i.base, const_names, name_to_stmt, refs);
            find_const_refs_inner(db, i.index, const_names, name_to_stmt, refs);
        }
        ExprFunKind::AnonTuple(t) => {
            for elem in &t.elements {
                find_const_refs_inner(db, *elem, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::AnonStruct(s) => {
            for field in &s.fields {
                find_const_refs_inner(db, field.value, const_names, name_to_stmt, refs);
            }
        }
        ExprFunKind::Place(ref place) => {
            // Zero-step Place is a bare variable reference — check for const ref.
            if place.steps.is_empty() {
                let name_str = place.root.text(db);
                if const_names.contains(name_str) {
                    if let Some(stmt_id) = name_to_stmt.get(name_str) {
                        refs.push(*stmt_id);
                    }
                }
            }
            // Walk index sub-expressions for const refs.
            for step in &place.steps {
                if let datalove_datafun_ast::ast::PlaceStep::Index(idx) = step {
                    find_const_refs_inner(db, idx.index, const_names, name_to_stmt, refs);
                }
            }
        }

        // A literal holds no subexpression, so it names no const. Listed rather
        // than swept up by a wildcard: this walk decides what a const is
        // evaluated after, and a kind that goes unwalked does not fail, it
        // silently loses the edge and the topological sort puts the const
        // before the one it reads. Naming every kind makes the next one added
        // a compile error here.
        ExprFunKind::True(_)
        | ExprFunKind::False(_)
        | ExprFunKind::None(_)
        | ExprFunKind::Int(_)
        | ExprFunKind::Float(_)
        | ExprFunKind::Hex(_)
        | ExprFunKind::String(_)
        | ExprFunKind::Atom(_)
        | ExprFunKind::ParseError(_) => {}
    }
}

/// Topological sort of const bindings.
///
/// Returns bindings in order where dependencies come before dependents.
/// Panics on cycles (user error should be caught earlier by typechecker).
fn topological_sort_consts(bindings: &[ConstBindingInfo]) -> Vec<ConstBindingInfo> {
    let n = bindings.len();
    if n == 0 {
        return Vec::new();
    }

    // Build index map: stmt_id -> index.
    let stmt_to_idx: HashMap<ConstStmtId, usize> = bindings.iter()
        .enumerate()
        .map(|(i, b)| (b.stmt_id, i))
        .collect();

    // Build in-degree counts and adjacency list.
    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (idx, binding) in bindings.iter().enumerate() {
        for dep_stmt in &binding.depends_on {
            if let Some(&dep_idx) = stmt_to_idx.get(dep_stmt) {
                in_degree[idx] += 1;
                dependents[dep_idx].push(idx);
            }
        }
    }

    // Kahn's algorithm.
    let mut queue: Vec<usize> = in_degree.iter()
        .enumerate()
        .filter(|(_, deg)| **deg == 0)
        .map(|(i, _)| i)
        .collect();

    let mut result = Vec::with_capacity(n);

    while let Some(idx) = queue.pop() {
        result.push(bindings[idx].clone());
        for &dep_idx in &dependents[idx] {
            in_degree[dep_idx] -= 1;
            if in_degree[dep_idx] == 0 {
                queue.push(dep_idx);
            }
        }
    }

    if result.len() != n {
        panic!("cycle detected in const dependencies");
    }

    result
}
