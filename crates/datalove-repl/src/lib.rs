//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;

/// Evaluate a REPL input string and return the result.
///
/// This is a stub implementation that will be filled in with actual
/// datalit parsing, type checking, and evaluation.
pub fn eval(input: &str) -> AnyResult<String> {
    // TODO: Implement actual REPL evaluation:
    // 1. Parse the input using datalove_datalit::parser::parse
    // 2. Resolve names using datalove_datalit::resolve::resolve_names
    // 3. Type check using datalove_datalit::tycheck::type_check
    // 4. Instantiate and evaluate the expression
    // 5. Format and return the result

    Ok(format!("(not yet evaluated: {})", input))
}
