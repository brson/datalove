//! Whether control can reach the end of a list of statements.
//!
//! Two checks want this, and they want the same answer: a function with a
//! return type has to return a value on every path out, and a function with an
//! out parameter has to write it on every path out. What both mean by "every
//! path" includes the end of the body, which is a return the source does not
//! write down, and neither should report against one the body cannot arrive at.
//!
//! Only as much of it as those two need. Anything not named here is taken to
//! complete, which at worst repeats a report a `ret` already made rather than
//! inventing one against unreachable code.

use crate::ast::Statement;

/// Whether control can reach the end of these statements.
pub fn body_completes<'db>(statements: &[Statement<'db>]) -> bool {
    statements.iter().all(statement_completes)
}

fn statement_completes<'db>(statement: &Statement<'db>) -> bool {
    match statement {
        Statement::Ret(_) | Statement::Break(_) | Statement::Continue(_) => false,
        Statement::If(stmt) => match &stmt.else_body {
            // Either way through is a way through.
            Some(else_body) => body_completes(&stmt.then_body) || body_completes(else_body),
            // No else is a way through that does nothing.
            None => true,
        },
        Statement::Loop(stmt) => match stmt.condition {
            // A condition may be false the first time it is read.
            Some(_) => true,
            // Otherwise the only way out is a `break` written for this loop.
            None => contains_break(&stmt.body),
        },
        Statement::Match(stmt) => {
            // A match covers every variant or says `case default`, which the
            // typechecker sees to, so there is no falling out of one: an arm
            // that completes is the only way through.
            stmt.cases.iter().any(|case| body_completes(&case.body))
                || stmt.default_body.as_ref().is_some_and(|body| body_completes(body))
        }
        _ => true,
    }
}

/// Whether a `break` in these statements leaves the loop they belong to.
///
/// A `break` inside a nested loop belongs to that one, so nested loops are not
/// descended into; everything else that holds statements is.
fn contains_break<'db>(statements: &[Statement<'db>]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Break(_) => true,
        Statement::Loop(_) => false,
        Statement::If(stmt) => {
            contains_break(&stmt.then_body)
                || stmt.else_body.as_ref().is_some_and(|body| contains_break(body))
        }
        Statement::Match(stmt) => {
            stmt.cases.iter().any(|case| contains_break(&case.body))
                || stmt.default_body.as_ref().is_some_and(|body| contains_break(body))
        }
        _ => false,
    })
}
