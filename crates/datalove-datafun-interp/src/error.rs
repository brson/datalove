//! Interpreter errors.

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Arithmetic overflow.
    Overflow,
    /// Division by zero.
    DivisionByZero,
    /// Runtime error.
    RuntimeError(String),
    /// The frame stack reached its limit.
    StackOverflow,
}
