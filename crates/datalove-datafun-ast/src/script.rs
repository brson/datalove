use rmx::prelude::*;

/// Input representing the REPL history.
/// This is the primary input that changes as the user submits statements.
#[salsa::input]
pub struct Script {
    /// All script units in submission order.
    #[returns(clone)]
    pub units: Vec<ScriptUnit>,
}

/// A single unit of script input.
/// This can be a single statement or a multiline function definition.
#[salsa::input]
pub struct ScriptUnit {
    /// The source text for this unit.
    #[returns(copy)]
    pub source: bct::input::Source,
}
