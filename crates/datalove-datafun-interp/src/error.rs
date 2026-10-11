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

impl std::fmt::Display for InterpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpError::RuntimeError(message) => f.write_str(message),
            InterpError::StackOverflow => f.write_str("stack overflow"),
        }
    }
}

impl std::error::Error for InterpError {}
