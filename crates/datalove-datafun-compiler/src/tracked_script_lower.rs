//! Salsa-tracked API for script unit lowering.
//!
//! Provides memoized, per-unit script lowering following the same pattern as
//! per-unit typechecking. Each unit is lowered independently, enabling
//! incremental recompilation when new units are added to a batch.

use rmx::prelude::*;
use std::collections::{HashMap, HashSet};
use salsa::plumbing::AsId;
use datalove_datafun_ast::ast::{Statement, ExprFun, ExprFunKind};
use datalove_datafun_ir::{
    IrType, IrModuleId, FuncId, ValueId, SlotId, ExportBinding,
    ConstBindingInfo, ConstBindingGraph, ConstStmtId,
};
use datalove_datafun_tycheck::{
    UnitTypecheckResultTracked, ModuleSpec,
};

use crate::module_graph::ModuleId;
use crate::lower;
use crate::IrTypeExt;

/// Accumulated bindings passed to subsequent script units for lowering.
///
/// Plain data struct (not tracked) - compared by Salsa via Eq/Hash.
/// Contains exports from all prior units in the batch.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
#[derive(salsa::SalsaValue)]
pub struct AccumulatedLowerBindings {
    /// Value bindings: (name, unit_index, value_id).
    pub values: Vec<(String, u32, ValueId)>,
    /// Slot bindings: (name, unit_index, slot_id).
    pub slots: Vec<(String, u32, SlotId)>,
    /// Value types: (name, type).
    pub value_types: Vec<(String, IrType)>,
    /// Slot types: (name, type).
    pub slot_types: Vec<(String, IrType)>,
    /// Function bindings: (name, unit_index, func_id).
    pub functions: Vec<(String, u32, FuncId)>,
    /// Current unit index.
    pub current_unit: u32,
}

impl AccumulatedLowerBindings {
    /// Convert to ScriptLowerContext for lowering.
    pub fn to_script_lower_context(&self) -> lower::ScriptLowerContext {
        let mut ctx = lower::ScriptLowerContext::new();
        ctx.current_unit = self.current_unit;

        for (name, unit, value_id) in &self.values {
            ctx.values.insert(name.clone(), (*unit, *value_id));
        }
        for (name, unit, slot_id) in &self.slots {
            ctx.slots.insert(name.clone(), (*unit, *slot_id));
        }
        for (name, ty) in &self.value_types {
            ctx.value_types.insert(name.clone(), ty.clone());
        }
        for (name, ty) in &self.slot_types {
            ctx.slot_types.insert(name.clone(), ty.clone());
        }
        for (name, unit, func_id) in &self.functions {
            ctx.functions.insert(name.clone(), (*unit, *func_id));
        }

        ctx
    }

    /// Update from exports of a completed unit.
    pub fn add_exports(&mut self, unit_index: u32, exports: &[(String, ExportBinding)], value_types: &[IrType], slot_types: &[IrType]) {
        for (name, binding) in exports {
            match binding {
                ExportBinding::Value(v) => {
                    // Remove any slot with same name.
                    self.slots.retain(|(n, _, _)| n != name);
                    self.slot_types.retain(|(n, _)| n != name);
                    // Remove any existing value with same name (shadowing).
                    self.values.retain(|(n, _, _)| n != name);
                    self.value_types.retain(|(n, _)| n != name);

                    self.values.push((name.clone(), unit_index, *v));
                    if let Some(ty) = value_types.get(v.0 as usize) {
                        self.value_types.push((name.clone(), ty.clone()));
                    }
                }
                ExportBinding::Slot(s) => {
                    // Remove any value with same name.
                    self.values.retain(|(n, _, _)| n != name);
                    self.value_types.retain(|(n, _)| n != name);
                    // Remove any existing slot with same name (shadowing).
                    self.slots.retain(|(n, _, _)| n != name);
                    self.slot_types.retain(|(n, _)| n != name);

                    self.slots.push((name.clone(), unit_index, *s));
                    if let Some(ty) = slot_types.get(s.0 as usize) {
                        self.slot_types.push((name.clone(), ty.clone()));
                    }
                }
                ExportBinding::Function(unit_id) => {
                    // Remove any existing function with same name.
                    self.functions.retain(|(n, _, _)| n != name);
                    self.functions.push((name.clone(), unit_index, FuncId(unit_id.0)));
                }
            }
        }

        self.current_unit = unit_index + 1;
    }
}

/// Build func_id_map from module specs for cross-module call resolution.
pub fn build_func_id_map<'db>(
    db: &'db dyn salsa::Database,
    module_specs: &[ModuleSpec<'db>],
) -> HashMap<(ModuleId, String), (IrModuleId, FuncId)> {
    let mut func_id_map = HashMap::new();

    for (ir_module_idx, spec) in module_specs.iter().enumerate() {
        let ir_module_id = IrModuleId(ir_module_idx as u32);
        let mut next_func_id: u32 = 0;

        for statement in &spec.parsed.statements {
            if let Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).S();
                let func_id = FuncId(next_func_id);
                next_func_id += 1;
                func_id_map.insert((spec.module_id, func_name), (ir_module_id, func_id));
            }
        }
    }

    func_id_map
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

    for stmt in &statements {
        if let Statement::Const(const_stmt) = stmt {
            let expr = const_stmt.value;
            // Use the expression's salsa ID as the const's ID (unique per const).
            let stmt_id = expr.as_id();
            let name = const_stmt.name.text(db).to_string();
            let expr_id = expr.as_id();

            // Get the type from the typechecker's table.
            let ir_type = expr_types
                .get(&datalove_datafun_ast::ast::ExprKey::of(db, expr))
                .map(|ty| IrType::from_tycheck(db, ty))
                .unwrap_or(IrType::Unit);

            const_names.insert(name.clone());
            name_to_stmt.insert(name.clone(), stmt_id);
            bindings.push(ConstBindingInfo {
                stmt_id,
                name,
                expr_id,
                ir_type,
                depends_on: Vec::new(), // Filled in second pass.
            });
        }
    }

    // Second pass: analyze dependencies.
    for binding in &mut bindings {
        // Find the expression for this binding.
        let expr = statements.iter()
            .find_map(|s| match s {
                Statement::Const(c) if c.value.as_id() == binding.stmt_id => Some(c.value),
                _ => None,
            });

        if let Some(expr) = expr {
            let deps = find_const_refs(db, expr, &const_names, &name_to_stmt);
            binding.depends_on = deps;
        }
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
        ExprFunKind::FunctionCall(call) => {
            for arg in call.args(db) {
                find_const_refs_inner(db, *arg, const_names, name_to_stmt, refs);
            }
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

        // Literals and other simple expressions have no references.
        _ => {}
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
