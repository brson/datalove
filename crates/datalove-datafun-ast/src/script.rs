//! The script a session has submitted so far, as something a query can be keyed on.
//!
//! See `botdocs/plan-script-reactivity.md`. The problem these two types exist to
//! solve is that a per-unit analysis must be keyed on a *position in the script*
//! rather than on anything derived from the units before it: a key built out of
//! the earlier units' outputs moves whenever any of them is edited, which re-runs
//! every later unit whether or not it uses what changed.

use rmx::prelude::*;

/// One unit of script input: a fragment of statements, or a bare expression.
///
/// Interned over its [`Source`](bct::input::Source), which is an input, and that
/// is the property everything here rests on: **a source's identity is
/// independent of its text.** Editing a unit means `set_text` on the source
/// behind this handle, which leaves the handle -- and so every memo keyed on it
/// -- exactly where it was.
#[salsa::interned]
pub struct ScriptUnit<'db> {
    /// The source text for this unit.
    #[returns(copy)]
    pub source: bct::input::Source,
    /// Whether the unit is a bare expression rather than a fragment of statements.
    ///
    /// The two parse differently and there is nothing in the text that says
    /// which was meant: the REPL decides from how the input was classified.
    #[returns(copy)]
    pub is_expr: bool,
}

/// The script as it stands up to and including one unit.
///
/// A cons list: `prev` is the script up to the unit before this one, and `None`
/// is the first unit. So a handle is both "which unit" and "which units come
/// before it", which is what a per-unit query needs to be keyed on.
///
/// **Why a chain and not a vector of units.** Appending a unit must leave the
/// earlier units' keys alone, or a REPL re-analyzes its whole history on every
/// line. A vector of the whole script fails that -- the handle changes on every
/// append -- and a vector of the prefix costs a copy and a hash of the prefix at
/// every step of a walk back through it. Extending a chain is one interned node.
#[salsa::interned]
pub struct Script<'db> {
    /// The script up to the unit before this one.
    #[returns(copy)]
    pub prev: Option<Script<'db>>,
    /// The unit this handle is about.
    #[returns(copy)]
    pub unit: ScriptUnit<'db>,
}

impl<'db> Script<'db> {
    /// Build the chain for `units`, in order, and hand back the head.
    ///
    /// `None` for an empty script, which has no unit for a handle to be about.
    pub fn from_units(
        db: &'db dyn salsa::Database,
        units: &[ScriptUnit<'db>],
    ) -> Option<Script<'db>> {
        units.iter().fold(None, |prev, unit| Some(Script::new(db, prev, *unit)))
    }

    /// This script's units, oldest first.
    pub fn units(self, db: &'db dyn salsa::Database) -> Vec<ScriptUnit<'db>> {
        let mut units = self.chain(db);
        units.reverse();
        units.iter().map(|script| script.unit(db)).collect()
    }

    /// This script and every prefix of it, newest first.
    pub fn chain(self, db: &'db dyn salsa::Database) -> Vec<Script<'db>> {
        let mut chain = vec![self];
        let mut current = self;
        while let Some(prev) = current.prev(db) {
            chain.push(prev);
            current = prev;
        }
        chain
    }

    /// How many units the script has.
    pub fn len(self, db: &'db dyn salsa::Database) -> usize {
        self.chain(db).len()
    }
}
