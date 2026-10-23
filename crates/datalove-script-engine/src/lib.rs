//! The datalove interactive scripting engine.
//!
//! An [`Engine`] holds a session of script units, compiled against a
//! workspace's modules and run as they are submitted, and re-derives what an
//! edit to one of them reaches. It knows nothing about how the units are
//! typed or shown: the REPL, `script-world`, and anything else that runs
//! units one at a time drive it the same way.

use serde::{Serialize, Deserialize};

mod engine;
pub use engine::{Engine, ModuleErrors, OnCompileError, UnitEdit};

// The system library a driver supplies to the engine.
pub use datalove_datafun::pipeline::{PackageLibrary, SystemLibrary};

/// A script unit's source, and what kind of unit it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    /// Statements, whose bindings the session keeps.
    Fragment(String),
    /// An expression, whose value is the unit's result.
    Expression(String),
}

impl Unit {
    pub fn source(&self) -> &str {
        match self {
            Unit::Fragment(source) | Unit::Expression(source) => source,
        }
    }

    pub fn is_expression(&self) -> bool {
        matches!(self, Unit::Expression(_))
    }
}

/// A binding a script fragment defined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EvalBinding {
    /// A `let` or `const` binding.
    Value { name: String, ty: String, value: String },
    /// A `var` binding.
    Slot { name: String, ty: String, value: String },
    /// A function definition.
    Function { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalExpr {
    pub ty: String,
    pub value: String,
}

/// What evaluating a unit came to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Eval {
    Nothing,
    /// The bindings a script fragment defined, in source order.
    Success(Vec<EvalBinding>),
    SuccessExpr(EvalExpr),
    Error(String),
    CrashReset(String),
    /// A runtime error, after which the session is empty.
    ///
    /// An error frees everything the runtime allocated rather than unwinding
    /// a value at a time, and every binding the session had was a value the
    /// runtime allocated, so the session starts again.
    ErrorReset(String),
    /// The unit returned early through `!` or `?`, with what it returned.
    ///
    /// It stays in the session, offering none of its bindings, and runs again
    /// when an edit reaches it.
    ReturnedEarly(String),
    /// The unit compiled and was not run, because it reads from these units
    /// and they did not run to their end. Units are named by their index in
    /// the session.
    ///
    /// It stays in the session too, and runs when an edit lets them finish.
    Blocked { on: Vec<usize> },
}
