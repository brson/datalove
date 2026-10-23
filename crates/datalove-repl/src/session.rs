//! The engine's session, with the numbers the REPL knows its units by.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use crate::script::{self, Engine, Unit, UnitEdit};
use crate::{Command, Eval, Place, ReplCommand, UnitReport};

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
}

impl Session {
    pub fn new(engine: Engine) -> Session {
        Session { engine, units: Vec::new(), next: 1 }
    }

    pub fn engine(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// The session's units, in order.
    pub fn units(&self) -> &[NumberedUnit] {
        &self.units
    }

    /// Evaluate a command.
    ///
    /// A REPL command that does not change the session is the app's to carry
    /// out, and is handed back to it. One that does is done here, in the
    /// order the commands came in, so a unit submitted after a `/reset` runs
    /// in the new session.
    pub fn eval(&mut self, command: Command) -> Eval {
        let eval = match command {
            Command::Unit(unit) => {
                let eval = self.engine.eval(unit.C());
                let eval = self.numbered(eval);
                if Eval::Unit(eval.C()).reset_the_session() {
                    self.forget();
                } else {
                    self.append(unit);
                }
                Eval::Unit(eval)
            }
            Command::ReplCommand(command) => match command {
                ReplCommand::Unknown => Eval::CommandError("unknown command".S()),
                ReplCommand::Usage(usage) => Eval::CommandError(usage),
                ReplCommand::Reset => {
                    self.engine.reset();
                    self.forget();
                    Eval::CallerInterpret(ReplCommand::Reset)
                }
                ReplCommand::Edit { unit, source } => self.edit(unit, source),
                ReplCommand::Delete { unit } => self.delete(unit),
                ReplCommand::Insert { place, unit, source } => self.insert(place, unit, source),
                command @ (ReplCommand::Exit | ReplCommand::Help | ReplCommand::Save | ReplCommand::Load) => {
                    Eval::CallerInterpret(command)
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
        let reports = self.guarded(number, |engine| engine.edit_unit(index, source.source()));
        self.splice(number, reports, |units| units[index].unit = source)
    }

    fn delete(&mut self, number: u32) -> Eval {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let reports = self.guarded(number, |engine| engine.remove_unit(index));
        self.splice(number, reports, |units| {
            units.remove(index);
        })
    }

    fn insert(&mut self, place: Place, number: u32, source: Unit) -> Eval {
        let Some(index) = self.index_of(number) else {
            return no_unit(number);
        };
        let index = match place {
            Place::Before => index,
            Place::After => index + 1,
        };
        let reports = self.guarded(number, |engine| {
            engine.insert_unit(index, source.source(), source.is_expression())
        });
        let made = NumberedUnit { number: self.next, unit: source };
        let eval = self.splice(number, reports, |units| units.insert(index, made));
        if !eval.reset_the_session() {
            self.next += 1;
        }
        eval
    }

    /// Change the numbers as the engine changed its units, and report what
    /// the units it reached came to by number.
    ///
    /// `change` is not made if the engine panicked, which leaves an empty
    /// session, and the panic is reported as unit `number`'s, the one the
    /// command named. The reports name units by where they are after the
    /// change, and a runtime error among them leaves the session empty too.
    fn splice(
        &mut self,
        number: u32,
        reports: Result<Vec<UnitEdit>, String>,
        change: impl FnOnce(&mut Vec<NumberedUnit>),
    ) -> Eval {
        let reports = match reports {
            Ok(reports) => reports,
            Err(crash) => {
                return Eval::Edited(vec![UnitReport { unit: number, eval: script::Eval::CrashReset(crash) }]);
            }
        };
        change(&mut self.units);
        let reports: Vec<UnitReport> = reports.into_iter()
            .map(|UnitEdit { unit, eval }| UnitReport {
                unit: self.units[unit].number,
                eval: self.numbered(eval),
            })
            .collect();
        let eval = Eval::Edited(reports);
        if eval.reset_the_session() {
            self.forget();
        }
        eval
    }

    /// Run one of the engine's edits, resetting the session if it panics.
    ///
    /// The engine catches a panic in evaluating a unit, and nothing else, so
    /// one here would take the worker with it.
    fn guarded(
        &mut self,
        number: u32,
        edit: impl FnOnce(&mut Engine) -> Vec<UnitEdit>,
    ) -> Result<Vec<UnitEdit>, String> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| edit(&mut self.engine)));
        result.map_err(|panic| {
            self.engine.reset();
            self.forget();
            let message = panic.downcast_ref::<&str>().map(|s| s.S())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "Unknown panic".S());
            fmt!("Edit panic, editing unit {number}: {message}")
        })
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
