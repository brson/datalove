//! The engine's session, with the numbers the REPL knows its units by.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use crate::script::{self, Engine, Unit};
use crate::{Command, Outcome, Place, ReplCommand, UnitStep};

/// A unit of the session, and its number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberedUnit {
    pub number: u32,
    pub unit: Unit,
}

/// An engine, and the units of its session with their numbers.
///
/// **A unit's number is its id in the engine**: given when it is made, by
/// appending or inserting, and kept while the session lasts. Numbers count
/// from 1, are never reused, and start again at 1 when the session is reset.
/// So a command naming a unit means the same unit whatever is inserted or
/// deleted around it, and a replay numbers the units as they were numbered
/// the first time. The units' text is kept here, beside the engine, and every
/// command that changes the session goes through here.
pub struct Session {
    engine: Engine,
    units: Vec<NumberedUnit>,
    /// What the last command to change the session did to the units, by
    /// number; see [`Session::last_step`].
    last_step: Vec<UnitStep>,
}

impl Session {
    pub fn new(engine: Engine) -> Session {
        Session { engine, units: Vec::new(), last_step: Vec::new() }
    }

    pub fn engine(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// The session's units, in order.
    pub fn units(&self) -> &[NumberedUnit] {
        &self.units
    }

    /// What the last command to change the session did to each unit it
    /// compiled or ran, in the session's order.
    ///
    /// Only a unit, an edit, a delete or an insert that the engine carried
    /// out is a step; a command that leaves the session alone, or one that
    /// fails, leaves the last step as it was, and a reset leaves no units to
    /// have done anything.
    pub fn last_step(&self) -> &[UnitStep] {
        &self.last_step
    }

    /// Evaluate a command.
    ///
    /// Only units and the commands that change the session come here; the
    /// rest are the REPL app's, which carries them out itself, and are an
    /// error here, as in a script run without the app.
    pub fn eval(&mut self, command: Command) -> Outcome {
        let eval = match command {
            Command::Unit(unit) => {
                let eval = self.engine.eval(unit.C());
                if eval.resets() {
                    self.forget();
                } else {
                    self.append(unit);
                }
                self.take_step(self.engine.last_step().to_vec());
                Outcome::Unit(eval)
            }
            Command::ReplCommand(command) => match command {
                ReplCommand::Unknown => Outcome::CommandError("unknown command".S()),
                ReplCommand::Usage(usage) => Outcome::CommandError(usage),
                ReplCommand::Reset => {
                    self.engine.reset();
                    self.forget();
                    Outcome::Reset
                }
                ReplCommand::Edit { unit, source } => self.edit(unit, source),
                ReplCommand::EditData { path, text } => self.edit_data(&path, &text),
                ReplCommand::Delete { unit } => self.delete(unit),
                ReplCommand::Insert { place, unit, source } => self.insert(place, unit, source),
                ReplCommand::Restore { unit, after, source } => self.restore(unit, after, source),
                ReplCommand::Move { unit, place, beside } => self.move_beside(unit, place, beside),
                ReplCommand::Reposition { unit, after } => self.reposition(unit, after),
                ReplCommand::Describe { unit } => match self.index_of(unit) {
                    Some(index) => Outcome::Detail(self.describe(index)),
                    None => no_unit(unit),
                },
                ReplCommand::Renumber { next } => {
                    self.engine.set_next_id(next);
                    Outcome::Edited(Vec::new())
                }
                ReplCommand::Exit | ReplCommand::Help | ReplCommand::Save | ReplCommand::Load
                | ReplCommand::Undo | ReplCommand::Redo => {
                    Outcome::CommandError(fmt!("{command:?} is the REPL app's, which keeps the history"))
                }
            },
        };
        assert!(
            self.engine.unit_ids().iter().eq(self.units.iter().map(|unit| &unit.number)),
            "the engine's units are the session's, with the same numbers",
        );
        eval
    }

    fn edit(&mut self, number: u32, source: Unit) -> Outcome {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let old = &self.units[index].unit;
        if old.is_expression() != source.is_expression() {
            return Outcome::CommandError(fmt!(
                "unit {number} is {}, and the edit is {}; a unit keeps its kind",
                kind(old), kind(&source),
            ));
        }
        let step = self.guarded(number, "editing", |engine| engine.edit_unit(number, source.source()));
        self.splice(step, |units| units[index].unit = source)
    }

    /// Replace a data file's text, as a stand-in for the world changing it.
    ///
    /// The data is not the session's, so the units stay as they are, and
    /// the step is what the change did to the units that depend on it. A
    /// panic is reported as the first unit's, since the change names none.
    fn edit_data(&mut self, path: &str, text: &str) -> Outcome {
        if self.engine.module_compiler().data_source(path).is_none() {
            return Outcome::CommandError(fmt!("there is no data {path}"));
        }
        let [library, package, name] = path.splitn(3, '/').collect::<Vec<_>>()[..] else {
            return Outcome::CommandError(fmt!("there is no data {path}"));
        };
        let first = self.units.first().map_or(0, |unit| unit.number);
        let step = self.guarded(first, "editing data before", |engine| {
            let first = engine.first_session();
            engine.edit_data(library, package, name, text).remove(&first)
                .expect("every session is re-derived")
        });
        self.splice(step, |_| ())
    }

    fn delete(&mut self, number: u32) -> Outcome {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let step = self.guarded(number, "deleting", |engine| engine.delete_unit(number));
        self.splice(step, |units| {
            units.remove(index);
        })
    }

    fn insert(&mut self, place: Place, number: u32, source: Unit) -> Outcome {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let index = place.index(index);
        let made = NumberedUnit { number: self.engine.next_id(), unit: source.C() };
        let step = self.guarded(number, "inserting beside", |engine| {
            engine.insert_unit(index, source.source(), source.is_expression())
        });
        self.splice(step, |units| units.insert(index, made))
    }

    fn move_beside(&mut self, number: u32, place: Place, beside: u32) -> Outcome {
        let Some(from) = self.index_of(number) else {
            return no_unit(number);
        };
        if number == beside {
            return Outcome::CommandError(fmt!("unit {number} cannot be moved beside itself"));
        }
        let Some(index) = self.index_without(beside, from) else {
            return no_unit(beside);
        };
        self.move_to(number, from, place.index(index))
    }

    /// Move a unit to after another, or first, as undoing or redoing a move
    /// does.
    fn reposition(&mut self, number: u32, after: Option<u32>) -> Outcome {
        let Some(from) = self.index_of(number) else {
            return no_unit(number);
        };
        let to = match after {
            Some(after) => match self.index_without(after, from) {
                Some(index) => index + 1,
                None => return no_unit(after),
            },
            None => 0,
        };
        self.move_to(number, from, to)
    }

    /// Move the unit at `from` to `to`, counted in the session as it is
    /// after the move.
    fn move_to(&mut self, number: u32, from: usize, to: usize) -> Outcome {
        if from == to {
            return Outcome::CommandError(fmt!("unit {number} is there already"));
        }
        let step = self.guarded(number, "moving", |engine| engine.move_unit(number, to));
        self.splice(step, |units| {
            let unit = units.remove(from);
            units.insert(to, unit);
        })
    }

    /// Where unit `number` sits once the unit at `taken` is taken out.
    fn index_without(&self, number: u32, taken: usize) -> Option<usize> {
        let index = self.index_of(number)?;
        Some(if index > taken { index - 1 } else { index })
    }

    /// What the session has to say about the unit at `index`.
    fn describe(&mut self, index: usize) -> crate::UnitDetail {
        let number = self.units[index].number;
        let accesses = self.engine.unit_accesses(number);
        crate::UnitDetail {
            unit: number,
            diagnostics: self.engine.unit_diagnostics(number, &fmt!("unit {number}"), script::TextStyle::Ansi),
            bindings: self.engine.unit_bindings(number),
            provides: accesses.provides,
            reads: accesses.reads,
            takes: accesses.takes,
            overwrites: accesses.overwrites,
            updates: accesses.updates,
            revives: accesses.revives,
        }
    }

    /// Put a unit back with its number, as it was before it was deleted, or
    /// before the append or insert that made it was taken back.
    fn restore(&mut self, number: u32, after: Option<u32>, source: Unit) -> Outcome {
        let index = match after {
            Some(after) => match self.index_of(after) {
                Some(index) => index + 1,
                None => return no_unit(after),
            },
            None => 0,
        };
        assert!(self.index_of(number).is_none(), "unit {number} is restored while it is there");
        let step = self.guarded(number, "restoring", |engine| {
            engine.restore_unit(index, number, source.source(), source.is_expression())
        });
        let made = NumberedUnit { number, unit: source };
        self.splice(step, |units| units.insert(index, made))
    }

    /// Change the units as the engine changed its own, and say what the step
    /// did to each unit by number.
    ///
    /// `change` is not made if the engine panicked, which leaves an empty
    /// session. A runtime error in the step leaves the session empty too.
    fn splice(
        &mut self,
        step: Result<Vec<script::UnitStep>, UnitStep>,
        change: impl FnOnce(&mut Vec<NumberedUnit>),
    ) -> Outcome {
        let step = match step {
            Ok(step) => step,
            Err(crash) => return Outcome::Edited(vec![crash]),
        };
        change(&mut self.units);
        let eval = Outcome::Edited(self.take_step(step));
        if eval.reset_the_session() {
            self.forget();
        }
        eval
    }

    /// Run one of the engine's edits, resetting the session if it panics,
    /// and reporting the panic as unit `number`'s, the one the command
    /// named, while `doing` it.
    ///
    /// The engine catches a panic in evaluating a unit, and nothing else, so
    /// one here would take the worker with it.
    fn guarded(
        &mut self,
        number: u32,
        doing: &str,
        edit: impl FnOnce(&mut Engine) -> Vec<script::UnitStep>,
    ) -> Result<Vec<script::UnitStep>, UnitStep> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| edit(&mut self.engine)));
        result.map_err(|panic| {
            self.engine.reset();
            self.forget();
            let message = fmt!("Panic {doing} unit {number}: {}", script::panic_message(&*panic));
            UnitStep { unit: number, compiled: false, eval: Some(script::Eval::Crashed(message)) }
        })
    }

    /// Keep the engine's step as the last, and return it.
    fn take_step(&mut self, step: Vec<script::UnitStep>) -> Vec<UnitStep> {
        self.last_step = step.into_iter()
            .map(|script::UnitStep { unit, compiled, eval }| UnitStep {
                unit,
                compiled: compiled.any(),
                eval,
            })
            .collect();
        self.last_step.C()
    }

    /// Keep the unit the engine just appended, under its id.
    fn append(&mut self, unit: Unit) {
        let number = *self.engine.unit_ids().last().expect("the engine kept the unit appended");
        self.units.push(NumberedUnit { number, unit });
    }

    fn forget(&mut self) {
        self.units.clear();
        self.last_step.clear();
    }

    fn index_of(&self, number: u32) -> Option<usize> {
        self.units.iter().position(|unit| unit.number == number)
    }
}

fn no_unit(number: u32) -> Outcome {
    Outcome::CommandError(fmt!("there is no unit {number}"))
}

fn kind(unit: &Unit) -> &'static str {
    match unit {
        Unit::Fragment(_) => "a fragment",
        Unit::Expression(_) => "an expression",
    }
}
