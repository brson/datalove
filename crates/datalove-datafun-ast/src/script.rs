//! The units a session has submitted so far, as something a query can be keyed on.
//!
//! See `botdocs/script-reactivity-architecture.md`. The problem this type exists
//! to solve is that a per-unit analysis must be keyed on *which unit* and nothing
//! else: a key built out of the earlier units -- their outputs, or even just
//! which units they are -- moves whenever any of them is edited, inserted or
//! removed, which re-runs every later unit whether or not it uses what changed.

use rmx::prelude::*;

/// One unit of script input: a fragment of statements, or a bare expression.
///
/// An input, and that is the property everything here rests on: **a unit's
/// identity is independent of its text and of where it sits.** Editing a unit
/// is `set_text` on its [`Source`](bct::input::Source), and splicing a unit in
/// or out is `set_prev` on the unit after it, and neither moves any handle --
/// so every memo keyed on one stays where it was.
///
/// A per-unit query must not read `prev` itself, or a splice before the unit
/// makes it run again. What the earlier units left is reached through the
/// lookups that walk `prev`, which run again on a splice and come out the same
/// unless the splice changed what a name resolves to.
#[salsa::input]
#[derive(Debug)]
pub struct ScriptUnit {
    /// The source text for this unit.
    #[returns(copy)]
    pub source: bct::input::Source,
    /// Whether the unit is a bare expression rather than a fragment of statements.
    ///
    /// The two parse differently and there is nothing in the text that says
    /// which was meant: the REPL decides from how the input was classified.
    #[returns(copy)]
    pub is_expr: bool,
    /// The unit before this one, `None` for the first.
    #[returns(copy)]
    pub prev: Option<ScriptUnit>,
}

impl ScriptUnit {
    /// Make a unit for each of `units`, in order, each linked to the one before.
    pub fn chain_of(
        db: &dyn salsa::Database,
        units: &[(bct::input::Source, bool)],
    ) -> Vec<ScriptUnit> {
        units.iter().fold(Vec::new(), |mut chain, (source, is_expr)| {
            let prev = chain.last().copied();
            chain.push(ScriptUnit::new(db, *source, *is_expr, prev));
            chain
        })
    }

    /// This unit and every unit before it, oldest first.
    pub fn units(self, db: &dyn salsa::Database) -> Vec<ScriptUnit> {
        let mut units = vec![self];
        let mut current = self;
        while let Some(prev) = current.prev(db) {
            units.push(prev);
            current = prev;
        }
        units.reverse();
        units
    }
}
