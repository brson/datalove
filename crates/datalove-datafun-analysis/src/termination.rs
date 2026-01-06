//! Termination detection for datafun loops.
//!
//! Analyzes loops with carries to detect termination evidence:
//! - Numeric carries that decrease by a constant each iteration
//! - Collection carries that shrink (list tail patterns)
//!
//! Reports `Unknown` for loops where termination cannot be proven.
//! This is informational, not an error.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use datalove_datafun_ast::ast::{
    Statement, StmtFun, StmtLoop, StmtContinue,
    ExprFun, ExprFunKind, BinOp,
};

/// Result of termination analysis for a function.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionTerminationAnalysis {
    /// Analyses for each loop in the function.
    pub loops: Vec<LoopTerminationAnalysis>,
}

/// Result of termination analysis for a single loop.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoopTerminationAnalysis {
    /// Index of the loop in the function's statement list (for identification).
    pub loop_index: usize,
    /// Termination status.
    pub status: TerminationStatus,
}

/// Termination status for a loop.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TerminationStatus {
    /// Loop provably terminates.
    Terminates {
        /// Human-readable explanation of why it terminates.
        evidence: String,
    },
    /// Termination cannot be proven (but may still terminate).
    Unknown {
        /// Why termination could not be proven.
        reason: String,
    },
    /// Loop has no carries (basic loop, termination depends on break).
    NoCarries,
}

/// Analyze a function for loop termination.
pub fn analyze_function_termination<'db>(
    db: &'db dyn salsa::Database,
    func: StmtFun<'db>,
) -> FunctionTerminationAnalysis {
    let mut loops = Vec::new();
    analyze_statements(db, func.body(db), &mut loops, 0);
    FunctionTerminationAnalysis { loops }
}

fn analyze_statements<'db>(
    db: &'db dyn salsa::Database,
    stmts: &[Statement<'db>],
    loops: &mut Vec<LoopTerminationAnalysis>,
    base_index: usize,
) {
    for (i, stmt) in stmts.iter().enumerate() {
        match stmt {
            Statement::Loop(loop_stmt) => {
                let analysis = analyze_loop(db, *loop_stmt, base_index + i);
                loops.push(analysis);
                // Recurse into loop body for nested loops.
                analyze_statements(db, loop_stmt.body(db), loops, 0);
            }
            Statement::If(if_stmt) => {
                // Recurse into if branches.
                analyze_statements(db, if_stmt.then_body(db), loops, 0);
                if let Some(else_body) = if_stmt.else_body(db) {
                    analyze_statements(db, else_body, loops, 0);
                }
            }
            Statement::Fun(inner_func) => {
                // Recurse into nested functions (rare but possible).
                analyze_statements(db, inner_func.body(db), loops, 0);
            }
            _ => {}
        }
    }
}

fn analyze_loop<'db>(
    db: &'db dyn salsa::Database,
    loop_stmt: StmtLoop<'db>,
    loop_index: usize,
) -> LoopTerminationAnalysis {
    let carries = loop_stmt.carries(db);

    if carries.is_empty() {
        return LoopTerminationAnalysis {
            loop_index,
            status: TerminationStatus::NoCarries,
        };
    }

    // Build map of carry names for lookup.
    let carry_names: Vec<String> = carries
        .iter()
        .map(|c| c.name(db).text(db).to_string())
        .collect();

    // Find all continue statements in the loop body and analyze them.
    let continues = find_continues(db, loop_stmt.body(db));

    if continues.is_empty() {
        // No continues found - loop must use break to exit.
        // This is actually fine, but we can't prove termination from carries.
        return LoopTerminationAnalysis {
            loop_index,
            status: TerminationStatus::Unknown {
                reason: "No continue statements found".to_string(),
            },
        };
    }

    // Check if any carry decreases on all continue paths.
    for (carry_idx, carry_name) in carry_names.iter().enumerate() {
        let mut all_decrease = true;
        let mut decrease_evidence = None;

        for cont in &continues {
            let values = cont.values(db);
            if carry_idx >= values.len() {
                // Continue doesn't provide enough values.
                all_decrease = false;
                break;
            }

            let new_value = &values[carry_idx];
            match check_decreasing(db, new_value, carry_name) {
                Some(evidence) => {
                    decrease_evidence = Some(evidence);
                }
                None => {
                    all_decrease = false;
                    break;
                }
            }
        }

        if all_decrease {
            if let Some(evidence) = decrease_evidence {
                return LoopTerminationAnalysis {
                    loop_index,
                    status: TerminationStatus::Terminates {
                        evidence: format!("Carry '{}' {}", carry_name, evidence),
                    },
                };
            }
        }
    }

    LoopTerminationAnalysis {
        loop_index,
        status: TerminationStatus::Unknown {
            reason: "No carry proven to decrease on all continue paths".to_string(),
        },
    }
}

/// Find all continue statements in a statement list (non-recursive into nested loops).
fn find_continues<'db>(
    db: &'db dyn salsa::Database,
    stmts: &[Statement<'db>],
) -> Vec<StmtContinue<'db>> {
    let mut continues = Vec::new();
    find_continues_inner(db, stmts, &mut continues);
    continues
}

fn find_continues_inner<'db>(
    db: &'db dyn salsa::Database,
    stmts: &[Statement<'db>],
    continues: &mut Vec<StmtContinue<'db>>,
) {
    for stmt in stmts {
        match stmt {
            Statement::Continue(cont) => {
                continues.push(*cont);
            }
            Statement::If(if_stmt) => {
                // Recurse into if branches.
                find_continues_inner(db, if_stmt.then_body(db), continues);
                if let Some(else_body) = if_stmt.else_body(db) {
                    find_continues_inner(db, else_body, continues);
                }
            }
            Statement::Loop(_) => {
                // Don't recurse into nested loops - their continues target the inner loop.
            }
            _ => {}
        }
    }
}

/// Check if an expression is decreasing relative to a carry variable.
///
/// Returns Some(evidence_string) if proven decreasing, None otherwise.
fn check_decreasing<'db>(
    db: &'db dyn salsa::Database,
    expr: &ExprFun<'db>,
    carry_name: &str,
) -> Option<String> {
    match expr.expr(db) {
        ExprFunKind::BinOp(binop) => {
            let op = binop.op(db);
            let lhs = binop.lhs(db);
            let rhs = binop.rhs(db);

            // Pattern: carry - constant
            if matches!(op, BinOp::Sub | BinOp::SubChecked | BinOp::SubOptional) {
                if is_name(db, &lhs, carry_name) && is_positive_constant(db, &rhs) {
                    return Some("decreases by subtraction".to_string());
                }
            }

            // Pattern: carry + negative (less common but valid)
            // Skip for now.

            None
        }
        ExprFunKind::FunctionCall(call) => {
            // Pattern: tail(carry) or similar list operations.
            let func_name = call.name(db).text(db);
            let args = call.args(db);

            if (func_name == "tail" || func_name == "rest" || func_name == "cdr")
                && args.len() == 1
                && is_name(db, &args[0], carry_name)
            {
                return Some("shrinks via tail/rest".to_string());
            }

            // Pattern: list.tail() method call - would need different AST structure.
            None
        }
        _ => None,
    }
}

/// Check if an expression is just a name matching the given identifier.
fn is_name<'db>(db: &'db dyn salsa::Database, expr: &ExprFun<'db>, name: &str) -> bool {
    matches!(expr.expr(db), ExprFunKind::Name(n) if n.text(db) == name)
}

/// Check if an expression is a positive integer constant.
fn is_positive_constant<'db>(db: &'db dyn salsa::Database, expr: &ExprFun<'db>) -> bool {
    match expr.expr(db) {
        ExprFunKind::Int(int_expr) => {
            // Check if the value is positive.
            let text = int_expr.value(db).text(db);
            // Simple check: doesn't start with '-' and isn't "0".
            !text.starts_with('-') && text != "0"
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would go here, but require setting up salsa DB.
    // For now, the module compiles and the logic is testable via fixtures.
}
