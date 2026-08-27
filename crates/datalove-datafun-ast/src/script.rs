use rmx::prelude::*;

/// The REPL history: every unit submitted so far, in order.
///
/// Interned rather than an input. Submitting a unit does not mutate this, it
/// builds a longer one, so an input only ever handed out a new identity for
/// a list that had grown by one.
#[salsa::interned]
pub struct Script<'db> {
    /// All script units in submission order.
    #[returns(clone)]
    pub units: Vec<ScriptUnit<'db>>,
}

/// A single unit of script input.
/// This can be a single statement or a multiline function definition.
#[salsa::interned]
pub struct ScriptUnit<'db> {
    /// The source text for this unit.
    #[returns(copy)]
    pub source: bct::input::Source,
}
