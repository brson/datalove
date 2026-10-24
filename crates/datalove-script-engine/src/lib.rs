//! The datalove interactive scripting engine.
//!
//! An [`Engine`] holds a session of script units, compiled against a
//! workspace's modules and run as they are submitted, and re-derives what an
//! edit to one of them reaches. It knows nothing about how the units are
//! typed or shown: the REPL, `script-world`, and anything else that runs
//! units one at a time drive it the same way.

use serde::{Serialize, Deserialize};

mod engine;
pub use engine::{Compiled, Engine, ModuleErrors, OnCompileError, UnitStep};

// The system library a driver supplies to the engine.
pub use datalove_datafun::pipeline::{PackageLibrary, SystemLibrary};
// How `Engine::unit_diagnostics` renders.
pub use bct::render::TextStyle;

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

/// A binding the session offers, as its environment lists it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnvBinding {
    pub name: String,
    pub ty: String,
    /// What it holds, printed; `None` for one a later unit gave away.
    pub value: Option<String>,
}

/// Why a blocked unit was not run, naming the units it waits on as the
/// caller names them: by number in the REPL, by position from one in a file.
pub fn blocked_reason(on: &[impl std::fmt::Display]) -> String {
    let named: Vec<String> = on.iter().map(|unit| unit.to_string()).collect();
    match named.as_slice() {
        [one] => format!("not run: it reads from unit {one}, which did not finish"),
        [rest @ .., last] => format!("not run: it reads from units {} and {last}, which did not finish", rest.join(", ")),
        [] => unreachable!("a blocked unit waits on some unit"),
    }
}

/// What a panic carried, as a message.
pub fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        message.to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "Unknown panic".to_string()
    }
}

/// A binding a script fragment defined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EvalBinding {
    /// A `let` or `const` binding, and what it holds; `None` for one the
    /// unit gave away itself.
    Value { name: String, ty: String, value: Option<String> },
    /// A `var` binding, and what it holds; `None` as for a `let`.
    Slot { name: String, ty: String, value: Option<String> },
    /// A function definition.
    Function { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalExpr {
    pub ty: String,
    pub value: String,
}

/// What evaluating a unit came to.
///
/// The variants are the unit's states as the REPL names them: it ran, its
/// expression came to a value, it did not compile, it returned early, or it
/// was blocked; or the session went with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Eval {
    /// A fragment ran to its end, with the bindings it provides in source
    /// order; none, for one like a `require` or a `set`.
    Ran(Vec<EvalBinding>),
    /// An expression unit ran, and came to this value.
    Value(EvalExpr),
    /// The unit did not compile, for these reasons.
    NotCompiled(String),
    /// The engine panicked, after which the session is empty.
    Crashed(String),
    /// A runtime error, after which the session is empty.
    ///
    /// An error frees everything the runtime allocated rather than unwinding
    /// a value at a time, and every binding the session had was a value the
    /// runtime allocated, so the session starts again.
    RuntimeError(String),
    /// The unit returned early through `!` or `?`, with what it returned.
    ///
    /// It stays in the session, providing none of its bindings, and reruns
    /// when an edit reaches it.
    ReturnedEarly(String),
    /// The unit compiled and was not run, because it reads from these units
    /// and they did not finish.
    ///
    /// The engine names the units by their index in the session; a driver
    /// that numbers units, as the REPL does, renames them by number before
    /// anything shows them. It stays in the session too, and runs when an
    /// edit lets them finish.
    Blocked { on: Vec<usize> },
}

impl Eval {
    /// Whether the unit took the session with it: a runtime error or a
    /// crash, after which the session is empty.
    pub fn resets(&self) -> bool {
        matches!(self, Eval::RuntimeError(_) | Eval::Crashed(_))
    }

    /// Whether the unit was run, if only to an early return or a runtime
    /// error, rather than failing to compile or being blocked.
    pub fn was_run(&self) -> bool {
        match self {
            Eval::Ran(_) | Eval::Value(_)
            | Eval::ReturnedEarly(_) | Eval::RuntimeError(_) => true,
            Eval::NotCompiled(_) | Eval::Blocked { .. } | Eval::Crashed(_) => false,
        }
    }
}
