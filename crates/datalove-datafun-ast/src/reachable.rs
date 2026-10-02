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

/// Whether every path through these statements leaves the function.
///
/// The ownership pass wants this where an `if` or a `match` merges. A branch
/// that returns never arrives at the merge, so what it moved describes a
/// different point in the program and must not be compared against, or taken
/// for, the state of a branch that does arrive.
///
/// **This is not the negation of [`body_completes`], and the difference is
/// `break` and `continue`.** Those do not complete a body, so `body_completes`
/// says no to them, and rightly: the end of the body is not where they go. But
/// they do rejoin the program -- after the loop, or at its head -- and whatever
/// they moved is still given away when they land there. A merge that ignored
/// them would take the state of the path that did not move, and schedule a drop
/// for a value the other path had already handed over. Returning is the only
/// exit that arrives at no merge at all, so it is the only one here.
///
/// Conservative where it can afford to be, since missing a divergence costs
/// only the spurious error that not knowing about it caused in the first place:
/// a `loop` whose every exit is a `ret` is not recognized.
pub fn body_returns<'db>(statements: &[Statement<'db>]) -> bool {
    statements.iter().any(statement_returns)
}

fn statement_returns<'db>(statement: &Statement<'db>) -> bool {
    match statement {
        Statement::Ret(_) => true,
        // With no else, the way round the `if` returns nothing.
        Statement::If(stmt) => match &stmt.else_body {
            Some(else_body) => body_returns(&stmt.then_body) && body_returns(else_body),
            None => false,
        },
        Statement::Match(stmt) => {
            // Exhaustive, as `body_completes` says, so there is no falling out
            // of one: every arm returning is every path returning. An arm list
            // with nothing in it is not that.
            let default_returns = match &stmt.default_body {
                Some(body) => body_returns(body),
                None => true,
            };
            (!stmt.cases.is_empty() || stmt.default_body.is_some())
                && stmt.cases.iter().all(|case| body_returns(&case.body))
                && default_returns
        }
        _ => false,
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
