//! Interpreter errors.

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Runtime error.
    RuntimeError(String),
    /// The stack ran out: the frame stack reached its limit, or the native
    /// stack a call recursed on, the IR walker's or compiled code's, did.
    StackOverflow,
}
