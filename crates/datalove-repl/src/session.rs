//! The engine's session, with the numbers the REPL knows its units by.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use crate::script::{self, Engine, Unit};
use crate::{Command, Eval, Place, ReplCommand, UnitStep};

/// A unit of the session, and its number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NumberedUnit {
    pub number: u32,
    pub unit: Unit,
}

/// An engine, and the numbers of the units in its session.
///
/// **A unit is numbered when it is made, by appending or inserting, and
/// keeps its number** while the session lasts. Numbers count from 1, are
/// never reused, and start again at 1 when the session is reset. So a command
/// naming a unit means the same unit whatever is inserted or deleted after
/// it, and a replay numbers the units as they were numbered the first time.
/// The engine knows units only by position, so the numbers are kept here,
/// beside it, and every command that changes the session goes through here.
pub struct Session {
    engine: Engine,
    units: Vec<NumberedUnit>,
    next: u32,
    /// What the last command to change the session did to the units, by
    /// number; see [`Session::last_step`].
    last_step: Vec<UnitStep>,
}

impl Session {
    pub fn new(engine: Engine) -> Session {
        Session { engine, units: Vec::new(), next: 1, last_step: Vec::new() }
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
    pub fn eval(&mut self, command: Command) -> Eval {
        let eval = match command {
            Command::Unit(unit) => {
                let eval = self.engine.eval(unit.C());
                if eval.resets() {
                    self.forget();
                } else {
                    self.append(unit);
                }
                self.take_step(self.engine.last_step().to_vec());
                Eval::Unit(self.numbered(eval))
            }
            Command::ReplCommand(command) => match command {
                ReplCommand::Unknown => Eval::CommandError("unknown command".S()),
                ReplCommand::Usage(usage) => Eval::CommandError(usage),
                ReplCommand::Reset => {
                    self.engine.reset();
                    self.forget();
                    Eval::Reset
                }
                ReplCommand::Edit { unit, source } => self.edit(unit, source),
                ReplCommand::Delete { unit } => self.delete(unit),
                ReplCommand::Insert { place, unit, source } => self.insert(place, unit, source),
                ReplCommand::Restore { unit, after, source } => self.restore(unit, after, source),
                ReplCommand::Describe { unit } => match self.index_of(unit) {
                    Some(index) => Eval::Detail(self.describe(index)),
                    None => no_unit(unit),
                },
                ReplCommand::Renumber { next } => {
                    self.next = next;
                    Eval::Edited(Vec::new())
                }
                ReplCommand::Exit | ReplCommand::Help | ReplCommand::Save | ReplCommand::Load
                | ReplCommand::Undo | ReplCommand::Redo => {
                    Eval::CommandError(fmt!("{command:?} is the REPL app's, which keeps the history"))
                }
            },
        };
        assert_eq!(
            self.engine.unit_count(), self.units.len(),
            "the engine's session and its numbers are the same length",
        );
        eval
    }

    fn edit(&mut self, number: u32, source: Unit) -> Eval {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let old = &self.units[index].unit;
        if old.is_expression() != source.is_expression() {
            return Eval::CommandError(fmt!(
                "unit {number} is {}, and the edit is {}; a unit keeps its kind",
                kind(old), kind(&source),
            ));
        }
        let step = self.guarded(number, "editing", |engine| engine.edit_unit(index, source.source()));
        self.splice(step, |units| units[index].unit = source)
    }

    fn delete(&mut self, number: u32) -> Eval {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let step = self.guarded(number, "deleting", |engine| engine.remove_unit(index));
        self.splice(step, |units| {
            units.remove(index);
        })
    }

    fn insert(&mut self, place: Place, number: u32, source: Unit) -> Eval {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let index = place.index(index);
        let step = self.guarded(number, "inserting beside", |engine| {
            engine.insert_unit(index, source.source(), source.is_expression())
        });
        let made = NumberedUnit { number: self.next, unit: source };
        let eval = self.splice(step, |units| units.insert(index, made));
        if !eval.reset_the_session() {
            self.next += 1;
        }
        eval
    }

    /// What the session has to say about the unit at `index`.
    fn describe(&mut self, index: usize) -> crate::UnitDetail {
        let number = self.units[index].number;
        let effects = self.engine.unit_effects(index);
        crate::UnitDetail {
            unit: number,
            diagnostics: self.engine.unit_diagnostics(index, &fmt!("unit {number}"), script::TextStyle::Ansi),
            bindings: self.engine.unit_bindings(index),
            provides: effects.provides,
            uses: effects.uses,
            takes: effects.taken,
            overwrites: effects.overwritten,
            updates: effects.updated,
        }
    }

    /// Put a unit back with its number, as it was before it was deleted, or
    /// before the append or insert that made it was taken back.
    fn restore(&mut self, number: u32, after: Option<u32>, source: Unit) -> Eval {
        let index = match after {
            Some(after) => match self.index_of(after) {
                Some(index) => index + 1,
                None => return no_unit(after),
            },
            None => 0,
        };
        assert!(self.index_of(number).is_none(), "unit {number} is restored while it is there");
        let step = self.guarded(number, "restoring", |engine| {
            engine.insert_unit(index, source.source(), source.is_expression())
        });
        let made = NumberedUnit { number, unit: source };
        self.splice(step, |units| units.insert(index, made))
    }

    /// Change the numbers as the engine changed its units, and say what the
    /// step did to each unit by number.
    ///
    /// `change` is not made if the engine panicked, which leaves an empty
    /// session. The step names units by where they are after the change, and
    /// a runtime error in it leaves the session empty too.
    fn splice(
        &mut self,
        step: Result<Vec<script::UnitStep>, UnitStep>,
        change: impl FnOnce(&mut Vec<NumberedUnit>),
    ) -> Eval {
        let step = match step {
            Ok(step) => step,
            Err(crash) => return Eval::Edited(vec![crash]),
        };
        change(&mut self.units);
        let eval = Eval::Edited(self.take_step(step));
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
            UnitStep { unit: number, compiled: false, eval: Some(script::Eval::CrashReset(message)) }
        })
    }

    /// Keep the engine's step as the last, by number, and return it.
    fn take_step(&mut self, step: Vec<script::UnitStep>) -> Vec<UnitStep> {
        self.last_step = step.into_iter()
            .map(|script::UnitStep { unit, compiled, eval }| UnitStep {
                unit: self.units[unit].number,
                compiled: compiled.any(),
                eval: eval.map(|eval| self.numbered(eval)),
            })
            .collect();
        self.last_step.C()
    }

    /// Name the units a `Blocked` waits on by number rather than position.
    fn numbered(&self, eval: script::Eval) -> script::Eval {
        match eval {
            script::Eval::Blocked { on } => script::Eval::Blocked {
                on: on.into_iter().map(|index| self.units[index].number as usize).collect(),
            },
            eval => eval,
        }
    }

    fn append(&mut self, unit: Unit) {
        self.units.push(NumberedUnit { number: self.next, unit });
        self.next += 1;
    }

    fn forget(&mut self) {
        self.units.clear();
        self.next = 1;
        self.last_step.clear();
    }

    fn index_of(&self, number: u32) -> Option<usize> {
        self.units.iter().position(|unit| unit.number == number)
    }
}

fn no_unit(number: u32) -> Eval {
    Eval::CommandError(fmt!("there is no unit {number}"))
}

fn kind(unit: &Unit) -> &'static str {
    match unit {
        Unit::Fragment(_) => "statements",
        Unit::Expression(_) => "an expression",
    }
}
