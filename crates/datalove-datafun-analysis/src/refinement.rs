//! Refinement type checking for datafun.
//!
//! Prototype: detects potentially unsafe divisions (division by zero).
//!
//! Tracks predicates:
//! - NonZero: value proven to be non-zero
//! - Unknown: no proof available
//!
//! Reports warnings for divisions where divisor is Unknown.

use rmx::prelude::*;
use rmx::std::collections::HashMap;
use serde::{Serialize, Deserialize};
use datalove_datafun_ast::ast::{
    Statement, StmtFun, ExprFun, ExprFunKind, BinOp,
};

/// Result of refinement analysis for a function.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionRefinementAnalysis {
    /// Warnings about potentially unsafe operations.
    pub warnings: Vec<RefinementWarning>,
}

/// Warning about a potentially unsafe operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefinementWarning {
    /// Description of the warning.
    pub message: String,
    /// Expression index (for location, if available).
    pub expr_index: Option<usize>,
}

/// Predicate tracking for a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Predicate {
    /// Value is proven non-zero.
    NonZero,
    /// No proof available.
    Unknown,
}

/// Analysis context.
struct AnalysisCtx<'db> {
    db: &'db dyn salsa::Database,
    /// Known predicates for variables.
    predicates: HashMap<String, Predicate>,
    /// Collected warnings.
    warnings: Vec<RefinementWarning>,
}

impl<'db> AnalysisCtx<'db> {
    fn new(db: &'db dyn salsa::Database) -> Self {
        Self {
            db,
            predicates: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    fn set_predicate(&mut self, name: &str, pred: Predicate) {
        self.predicates.insert(name.to_string(), pred);
    }

    fn get_predicate(&self, name: &str) -> Predicate {
        self.predicates.get(name).copied().unwrap_or(Predicate::Unknown)
    }

    fn warn(&mut self, message: String) {
        self.warnings.push(RefinementWarning {
            message,
            expr_index: None,
        });
    }
}

/// Analyze a function for refinement type violations.
pub fn analyze_function_refinement<'db>(
    db: &'db dyn salsa::Database,
    func: StmtFun<'db>,
) -> FunctionRefinementAnalysis {
    let mut ctx = AnalysisCtx::new(db);

    // Analyze function body.
    analyze_statements(&mut ctx, func.body(db));

    FunctionRefinementAnalysis {
        warnings: ctx.warnings,
    }
}

fn analyze_statements<'db>(ctx: &mut AnalysisCtx<'db>, stmts: &[Statement<'db>]) {
    for stmt in stmts {
        analyze_statement(ctx, stmt);
    }
}

fn analyze_statement<'db>(ctx: &mut AnalysisCtx<'db>, stmt: &Statement<'db>) {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name(ctx.db).text(ctx.db).to_string();
            let expr = let_stmt.value(ctx.db);

            // Analyze the expression for divisions.
            analyze_expr(ctx, &expr);

            // Infer predicate for the new binding.
            let pred = infer_predicate(ctx, &expr);
            ctx.set_predicate(&name, pred);
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name(ctx.db).text(ctx.db).to_string();
            let expr = var_stmt.value(ctx.db);

            analyze_expr(ctx, &expr);

            let pred = infer_predicate(ctx, &expr);
            ctx.set_predicate(&name, pred);
        }
        Statement::Set(set_stmt) => {
            let expr = set_stmt.value(ctx.db);
            analyze_expr(ctx, &expr);

            // Update predicate for the target.
            let name = set_stmt.name(ctx.db).text(ctx.db).to_string();
            let pred = infer_predicate(ctx, &expr);
            ctx.set_predicate(&name, pred);
        }
        Statement::Ret(ret_stmt) => {
            if let Some(expr) = ret_stmt.value(ctx.db) {
                analyze_expr(ctx, &expr);
            }
        }
        Statement::If(if_stmt) => {
            // Analyze condition.
            let cond = if_stmt.condition(ctx.db);
            analyze_expr(ctx, &cond);

            // Check for pattern: if x != 0 ... then x is NonZero in then branch.
            let then_predicates = infer_condition_predicates(ctx, &cond, true);
            let else_predicates = infer_condition_predicates(ctx, &cond, false);

            // Analyze then branch with inferred predicates.
            let saved = ctx.predicates.clone();
            for (name, pred) in then_predicates {
                ctx.set_predicate(&name, pred);
            }
            analyze_statements(ctx, if_stmt.then_body(ctx.db));

            // Analyze else branch with opposite predicates.
            ctx.predicates = saved.clone();
            for (name, pred) in else_predicates {
                ctx.set_predicate(&name, pred);
            }
            if let Some(else_body) = if_stmt.else_body(ctx.db) {
                analyze_statements(ctx, else_body);
            }

            // Restore original predicates (conservative merge).
            ctx.predicates = saved;
        }
        Statement::Loop(loop_stmt) => {
            // Analyze loop body.
            analyze_statements(ctx, loop_stmt.body(ctx.db));
        }
        Statement::Break(break_stmt) => {
            for expr in break_stmt.values(ctx.db) {
                analyze_expr(ctx, expr);
            }
        }
        Statement::Continue(cont_stmt) => {
            for expr in cont_stmt.values(ctx.db) {
                analyze_expr(ctx, expr);
            }
        }
        Statement::Fun(inner_func) => {
            // Analyze nested function separately.
            let _ = analyze_function_refinement(ctx.db, *inner_func);
        }
        _ => {}
    }
}

fn analyze_expr<'db>(ctx: &mut AnalysisCtx<'db>, expr: &ExprFun<'db>) {
    match expr.expr(ctx.db) {
        ExprFunKind::BinOp(binop) => {
            let op = binop.op(ctx.db);
            let lhs = binop.lhs(ctx.db);
            let rhs = binop.rhs(ctx.db);

            // Recursively analyze operands.
            analyze_expr(ctx, &lhs);
            analyze_expr(ctx, &rhs);

            // Check for division operations.
            if matches!(op, BinOp::Div | BinOp::DivChecked | BinOp::DivOptional) {
                let divisor_pred = infer_predicate(ctx, &rhs);
                if divisor_pred == Predicate::Unknown {
                    let divisor_desc = describe_expr(ctx, &rhs);
                    ctx.warn(format!(
                        "Division by '{}' may be zero (divisor not proven non-zero)",
                        divisor_desc
                    ));
                }
            }
        }
        ExprFunKind::FunctionCall(call) => {
            for arg in call.args(ctx.db) {
                analyze_expr(ctx, arg);
            }
        }
        ExprFunKind::Tuple(tuple) => {
            for elem in tuple.elements(ctx.db) {
                analyze_expr(ctx, elem);
            }
        }
        ExprFunKind::UnaryOp(unary) => {
            analyze_expr(ctx, &unary.operand(ctx.db));
        }
        ExprFunKind::TryOption(try_opt) => {
            analyze_expr(ctx, &try_opt.operand(ctx.db));
        }
        ExprFunKind::TryResult(try_res) => {
            analyze_expr(ctx, &try_res.operand(ctx.db));
        }
        ExprFunKind::List(list) => {
            for elem in list.elements(ctx.db) {
                analyze_expr(ctx, elem);
            }
        }
        ExprFunKind::Set(set) => {
            for elem in set.elements(ctx.db) {
                analyze_expr(ctx, elem);
            }
        }
        ExprFunKind::Map(map) => {
            for entry in map.entries(ctx.db) {
                analyze_expr(ctx, &entry.key(ctx.db));
                analyze_expr(ctx, &entry.value(ctx.db));
            }
        }
        ExprFunKind::AnonTuple(tuple) => {
            for elem in tuple.elements(ctx.db) {
                analyze_expr(ctx, elem);
            }
        }
        ExprFunKind::AnonStruct(st) => {
            for field in st.fields(ctx.db) {
                analyze_expr(ctx, &field.value(ctx.db));
            }
        }
        _ => {
            // Literals, names, etc. - no sub-expressions.
        }
    }
}

/// Infer predicate for an expression.
fn infer_predicate<'db>(ctx: &AnalysisCtx<'db>, expr: &ExprFun<'db>) -> Predicate {
    match expr.expr(ctx.db) {
        ExprFunKind::Int(int_expr) => {
            let text = int_expr.value(ctx.db).text(ctx.db);
            // Non-zero if not "0".
            if text != "0" {
                Predicate::NonZero
            } else {
                Predicate::Unknown
            }
        }
        ExprFunKind::Hex(hex_expr) => {
            let text = hex_expr.value(ctx.db).text(ctx.db);
            // Non-zero if not "0x0" or "0x00" etc.
            let digits = text.trim_start_matches("0x").trim_start_matches("0X");
            if digits.chars().any(|c| c != '0') {
                Predicate::NonZero
            } else {
                Predicate::Unknown
            }
        }
        ExprFunKind::Name(name) => {
            ctx.get_predicate(name.text(ctx.db))
        }
        _ => Predicate::Unknown,
    }
}

/// Infer predicates from a condition expression.
///
/// If `in_then` is true, returns predicates valid in the then branch.
/// If `in_then` is false, returns predicates valid in the else branch.
fn infer_condition_predicates<'db>(
    ctx: &AnalysisCtx<'db>,
    cond: &ExprFun<'db>,
    in_then: bool,
) -> Vec<(String, Predicate)> {
    match cond.expr(ctx.db) {
        ExprFunKind::BinOp(binop) => {
            let op = binop.op(ctx.db);
            let lhs = binop.lhs(ctx.db);
            let rhs = binop.rhs(ctx.db);

            // Pattern: x != 0 or 0 != x
            if op == BinOp::Ne {
                if let Some(name) = extract_nonzero_check(ctx, &lhs, &rhs) {
                    if in_then {
                        return vec![(name, Predicate::NonZero)];
                    }
                }
            }

            // Pattern: x == 0 (inverse)
            if op == BinOp::Eq {
                if let Some(name) = extract_nonzero_check(ctx, &lhs, &rhs) {
                    if !in_then {
                        // In else branch of x == 0, x is non-zero.
                        return vec![(name, Predicate::NonZero)];
                    }
                }
            }

            // Pattern: x > 0 or x .> 0
            if matches!(op, BinOp::Gt) {
                if let (ExprFunKind::Name(name), true) = (lhs.expr(ctx.db), is_zero(ctx, &rhs)) {
                    if in_then {
                        return vec![(name.text(ctx.db).to_string(), Predicate::NonZero)];
                    }
                }
            }

            vec![]
        }
        _ => vec![],
    }
}

/// Check if condition is `name != 0` or `0 != name`, return the name.
fn extract_nonzero_check<'db>(
    ctx: &AnalysisCtx<'db>,
    lhs: &ExprFun<'db>,
    rhs: &ExprFun<'db>,
) -> Option<String> {
    // name != 0
    if let ExprFunKind::Name(name) = lhs.expr(ctx.db) {
        if is_zero(ctx, rhs) {
            return Some(name.text(ctx.db).to_string());
        }
    }
    // 0 != name
    if let ExprFunKind::Name(name) = rhs.expr(ctx.db) {
        if is_zero(ctx, lhs) {
            return Some(name.text(ctx.db).to_string());
        }
    }
    None
}

/// Check if an expression is the literal zero.
fn is_zero<'db>(ctx: &AnalysisCtx<'db>, expr: &ExprFun<'db>) -> bool {
    match expr.expr(ctx.db) {
        ExprFunKind::Int(int_expr) => {
            int_expr.value(ctx.db).text(ctx.db) == "0"
        }
        _ => false,
    }
}

/// Describe an expression for error messages.
fn describe_expr<'db>(ctx: &AnalysisCtx<'db>, expr: &ExprFun<'db>) -> String {
    match expr.expr(ctx.db) {
        ExprFunKind::Name(name) => name.text(ctx.db).to_string(),
        ExprFunKind::Int(int_expr) => int_expr.value(ctx.db).text(ctx.db).to_string(),
        ExprFunKind::BinOp(_) => "<expr>".to_string(),
        ExprFunKind::FunctionCall(call) => format!("{}(...)", call.name(ctx.db).text(ctx.db)),
        _ => "<expr>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would go here.
}
