//! Step tables: what each unit of a session should do in a step, checked
//! against the engine.
//!
//! A step is one change to the session -- units appended, or one unit
//! edited, deleted or inserted -- and its `expect` table says, for every unit,
//! what the unit should do in that step and what its bindings should hold
//! after it:
//!
//! ```text
//! | unit | uses | this step | because | holds after |
//! |------|------|-----------|---------|-------------|
//! | 1 | - | compiles, runs | new | `b` = 251 |
//! ```
//!
//! An `expect` table states the rule, and the engine may do more work than it
//! asks for: rerun a unit the rule leaves alone, or recompile one the rule
//! only reruns. Each such row is annotated in an `engine` table after the
//! step, so the gap is written down. Doing less than the rule asks, or failing
//! differently, is never accepted, and an annotation the engine has caught up
//! with fails too, so the annotations stay true.
//!
//! The `because` column is for the reader, and is not checked.
//!
//! Every step, with a table or without, is also checked against a fresh run:
//! a new engine runs the session's current units in order, and each unit must
//! come to the same end and hold the same bindings. This is the definition of
//! a correct step, checked without trusting the tables' `holds after`.

use rmx::prelude::*;

use bct::input::Source;
use datalove_ct::query_events::QueryRecorder;
use datalove_ct::unit_analysis::{self, Pass};
use datalove_datafun::driver::WorkspaceDescriptor;
use datalove_repl::Place;
use datalove_script_engine::{Engine, Eval, SystemLibrary, Unit};

pub const SEPARATOR: &str = "----------";

// ============================================================================
// Text
// ============================================================================

/// One section of a worldfile.
pub struct Section {
    pub header: String,
    pub content: String,
}

/// Split worldfile text into its sections.
///
/// Comment lines before the first section are skipped.
pub fn sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_end() != SEPARATOR {
            let line = lines[i].trim();
            assert!(line.is_empty() || line.starts_with("//"), "text outside a section: {:?}", lines[i]);
            i += 1;
            continue;
        }
        let header = lines[i + 1].trim().S();
        assert_eq!(lines[i + 2].trim_end(), SEPARATOR, "a header is closed by a separator: {header}");
        let mut end = i + 3;
        while end < lines.len() && lines[end].trim_end() != SEPARATOR {
            end += 1;
        }
        let content = lines[i + 3..end].join("\n").trim().S();
        sections.push(Section { header, content });
        i = end;
    }
    sections
}

/// The cells of a markdown table's body rows, header and rule skipped.
fn table(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|').split('|').map(|cell| cell.trim().S()).collect()
        })
        .collect()
}

// ============================================================================
// Steps
// ============================================================================

/// One step of a session, and the table it is checked against, if any.
pub struct Step {
    pub change: Change,
    pub check: Option<Check>,
}

/// What a step's table says.
pub struct Check {
    /// What a document calls the step, when not `Step N`: an `expect`
    /// header that starts with `Step` names it up to the first `: `.
    pub label: Option<String>,
    pub title: String,
    pub expect: Vec<Row>,
    pub engine: Vec<EngineRow>,
}

impl Check {
    /// The check an `expect` section makes, from its header's words after
    /// `expect`, and its table.
    pub fn parse(header: &str, content: &str) -> Check {
        let expect = table(content).into_iter()
            .map(|cells| {
                assert_eq!(cells.len(), 5, "an expect row has five cells: {cells:?}");
                let [unit, uses, this_step, because, holds_after] = <[String; 5]>::try_from(cells).ok().X();
                Row { unit, uses, this_step, because, holds_after }
            })
            .collect();
        let (label, title) = match header.split_once(": ") {
            Some((label, title)) if label.starts_with("Step ") => (Some(label.S()), title.S()),
            _ => (None, header.S()),
        };
        Check { label, title, expect, engine: Vec::new() }
    }

    /// Add the rows of an `engine` section.
    pub fn add_engine(&mut self, content: &str) {
        self.engine = table(content).into_iter()
            .map(|cells| {
                assert_eq!(cells.len(), 3, "an engine row has three cells: {cells:?}");
                let [unit, this_step, note] = <[String; 3]>::try_from(cells).ok().X();
                EngineRow { unit, this_step, note }
            })
            .collect();
    }
}

/// A unit entered, by appending or inserting.
pub struct Entry {
    pub unit: String,
    pub source: String,
    pub expression: bool,
}

impl Entry {
    fn code(&self) -> Unit {
        match self.expression {
            true => Unit::Expression(self.source.C()),
            false => Unit::Fragment(self.source.C()),
        }
    }
}

pub enum Change {
    /// Units appended, one at a time.
    Enter(Vec<Entry>),
    Edit { unit: String, source: String },
    /// A data file's text replaced, by its `library/package/name` path.
    EditData { path: String, text: String },
    Delete { unit: String },
    /// A unit put before or after the unit `beside`.
    Insert { entry: Entry, place: Place, beside: String },
    /// A unit moved before or after the unit `beside`.
    Move { unit: String, place: Place, beside: String },
}

pub struct Row {
    pub unit: String,
    pub uses: String,
    pub this_step: String,
    pub because: String,
    pub holds_after: String,
}

pub struct EngineRow {
    pub unit: String,
    pub this_step: String,
    pub note: String,
}

// ============================================================================
// Outcomes
// ============================================================================

/// What became of a unit's run in one step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ran {
    /// Not run.
    No,
    /// Ran to its end.
    Runs,
    /// Did not compile, so did not run.
    Fails,
    ReturnsEarly,
    /// Compiled, and not run because something it reads did not finish.
    Blocked,
    Deleted,
}

/// What a unit did in one step, as a step table's `this step` says it.
///
/// _Recompiles_ is salsa running the unit's typecheck or its ownership
/// analysis again. The typecheck alone misses a unit that names a binding an
/// earlier unit now gives away or gives back: whether it may use the name is
/// decided in ownership analysis. Lowering is not memoized and is redone for
/// every unit the engine reaches, so it is not what tells a recompile from a
/// rerun.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Outcome {
    recompiles: bool,
    ran: Ran,
}

impl Outcome {
    fn parse(text: &str) -> Outcome {
        let (recompiles, ran) = match text {
            "untouched" => (false, Ran::No),
            "recompiles" => (true, Ran::No),
            "reruns" => (false, Ran::Runs),
            "compiles, runs" | "recompiles, runs" => (true, Ran::Runs),
            "compile fails" | "recompile fails" => (true, Ran::Fails),
            "compiles, returns early" | "recompiles, returns early" => (true, Ran::ReturnsEarly),
            "reruns, returns early" => (false, Ran::ReturnsEarly),
            "compiles, is blocked" | "recompiles, is blocked" => (true, Ran::Blocked),
            "is blocked" => (false, Ran::Blocked),
            "deleted" => (false, Ran::Deleted),
            other => panic!("unknown outcome `{other}`"),
        };
        Outcome { recompiles, ran }
    }

    /// How a step table would say it.
    fn describe(self) -> &'static str {
        match (self.recompiles, self.ran) {
            (false, Ran::No) => "untouched",
            (true, Ran::No) => "recompiles",
            (false, Ran::Runs) => "reruns",
            (true, Ran::Runs) => "recompiles, runs",
            (_, Ran::Fails) => "recompile fails",
            (false, Ran::ReturnsEarly) => "reruns, returns early",
            (true, Ran::ReturnsEarly) => "recompiles, returns early",
            (false, Ran::Blocked) => "is blocked",
            (true, Ran::Blocked) => "recompiles, is blocked",
            (_, Ran::Deleted) => "deleted",
        }
    }

    /// Whether the engine doing `self` is what the rule's `rule` looks like
    /// from outside.
    ///
    /// A unit that fails is not asked whether it recompiled: the doc says
    /// "recompile fails" whether or not the failing analysis was one salsa
    /// kept.
    fn is_seen_as(self, rule: Outcome) -> bool {
        match rule.ran {
            Ran::Fails | Ran::Deleted => self.ran == rule.ran,
            _ => self == rule,
        }
    }

    /// Whether the engine doing `self` where the rule says `rule` is only
    /// more work to the same end: compiling or running a unit the rule
    /// leaves alone.
    fn is_conservative_for(self, rule: Outcome) -> bool {
        let fine = |ran: Ran| matches!(ran, Ran::No | Ran::Runs);
        fine(self.ran) && fine(rule.ran)
            && self.recompiles >= rule.recompiles
            && (self.ran == Ran::Runs) >= (rule.ran == Ran::Runs)
            && self != rule
    }
}

fn ran(eval: &Eval) -> Ran {
    match eval {
        Eval::NotCompiled(_) => Ran::Fails,
        Eval::ReturnedEarly(_) => Ran::ReturnsEarly,
        Eval::Blocked { .. } => Ran::Blocked,
        Eval::Ran(_) | Eval::Value(_) => Ran::Runs,
        other => panic!("a unit came to {other:?}"),
    }
}

/// A unit's bindings as a step table's `holds after` writes them.
fn holds(bindings: &[(String, Option<String>)]) -> String {
    if bindings.is_empty() {
        return "-".S();
    }
    bindings.iter()
        .map(|(name, value)| match value {
            Some(value) => fmt!("`{name}` = {value}"),
            None => fmt!("`{name}` moved"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ============================================================================
// Running
// ============================================================================

/// A session in progress: the engine and the names of its units, in order.
struct Session {
    engine: Engine,
    units: Vec<String>,
    /// Each unit's current source, in the same order.
    code: Vec<Unit>,
    /// What each unit came to when it last ran or failed to, in the same
    /// order.
    states: Vec<Ran>,
    /// The units whose last compile failed, which leaves no record of what
    /// they do.
    failed: Vec<String>,
    recorder: QueryRecorder,
    /// The units the engine says it typechecked or analyzed for ownership in
    /// the step just applied, which the recorder checks.
    analyzed: Vec<String>,
    also_roots: Vec<String>,
}

impl Session {
    fn start(sys: SystemLibrary, workspace: WorkspaceDescriptor, also_roots: Vec<String>) -> Session {
        let recorder = QueryRecorder::new();
        let db = datalove_datafun::Database::recording(&recorder);
        let engine = Engine::in_database(db, sys, workspace, also_roots.C()).expect("the engine starts");
        assert!(engine.module_errors().is_empty(), "the modules compile: {:?}", engine.module_errors().broken);
        recorder.clear();
        Session {
            engine, units: Vec::new(), code: Vec::new(), states: Vec::new(), failed: Vec::new(),
            recorder, analyzed: Vec::new(), also_roots,
        }
    }

    /// What unit `index` does to earlier units' bindings, as a step table's
    /// `uses` column says it, one entry per binding.
    fn uses(&self, index: usize) -> Vec<String> {
        let ids = self.engine.unit_ids();
        let accesses = self.engine.unit_accesses(ids[index]);
        let mut uses: Vec<String> = accesses.uses.iter()
            .filter_map(|name| {
                let provider = (0..index).rev()
                    .find(|earlier| self.engine.unit_accesses(ids[*earlier]).provides.contains(name))?;
                let has = |names: &[String]| names.contains(name);
                let verb = if has(&accesses.revives) {
                    "revives"
                } else if has(&accesses.takes) {
                    "takes"
                } else if has(&accesses.updates) {
                    "updates"
                } else if has(&accesses.overwrites) {
                    "overwrites"
                } else {
                    "reads"
                };
                Some(fmt!("{verb} `{name}` ({})", self.units[provider]))
            })
            .collect();
        uses.sort();
        uses
    }

    fn index(&self, unit: &str) -> usize {
        self.units.iter().position(|name| name == unit)
            .unwrap_or_else(|| panic!("no unit `{unit}` in the session"))
    }

    /// The engine's id for the unit named `unit`.
    fn id(&self, unit: &str) -> u32 {
        self.engine.unit_ids()[self.index(unit)]
    }

    /// The name of the unit the engine knows by `id`.
    fn name(&self, id: u32) -> String {
        let index = self.engine.unit_ids().iter().position(|held| *held == id)
            .unwrap_or_else(|| panic!("no unit {id} in the session"));
        self.units[index].C()
    }

    /// Make a step's change, and say what each unit did.
    fn apply(&mut self, change: &Change) -> Vec<(String, Outcome)> {
        self.analyzed.clear();
        let (ran, analyses) = unit_analysis::collect(|| {
            let mut ran: Vec<(String, Ran)> = Vec::new();
            match change {
                Change::Enter(entries) => {
                    for entry in entries {
                        ran.push(self.append(entry));
                    }
                }
                Change::Edit { unit, source } => {
                    let index = self.index(unit);
                    self.code[index] = match self.code[index].is_expression() {
                        true => Unit::Expression(source.C()),
                        false => Unit::Fragment(source.C()),
                    };
                    let reports = self.engine.edit_unit(self.id(unit), source);
                    ran.extend(self.reported(reports));
                }
                Change::EditData { path, text } => {
                    let [library, package, name] = path.splitn(3, '/').collect::<Vec<_>>()[..] else {
                        panic!("data `{path}` is not a library/package/name path");
                    };
                    let first = self.engine.first_session();
                    let reports = self.engine.edit_data(library, package, name, text).remove(&first)
                        .expect("every session is re-derived");
                    ran.extend(self.reported(reports));
                }
                Change::Delete { unit } => {
                    let id = self.id(unit);
                    let index = self.index(unit);
                    self.units.remove(index);
                    self.code.remove(index);
                    self.states.remove(index);
                    ran.push((unit.C(), Ran::Deleted));
                    let reports = self.engine.delete_unit(id);
                    ran.extend(self.reported(reports));
                }
                Change::Move { unit, place, beside } => {
                    let id = self.id(unit);
                    let from = self.index(unit);
                    let name = self.units.remove(from);
                    let code = self.code.remove(from);
                    let state = self.states.remove(from);
                    let to = place.index(self.index(beside));
                    self.units.insert(to, name);
                    self.code.insert(to, code);
                    self.states.insert(to, state);
                    let reports = self.engine.move_unit(id, to);
                    ran.extend(self.reported(reports));
                }
                Change::Insert { entry, place, beside } => {
                    let index = place.index(self.index(beside));
                    self.units.insert(index, entry.unit.C());
                    self.code.insert(index, entry.code());
                    self.states.insert(index, Ran::No);
                    let reports = self.engine.insert_unit(index, &entry.source, entry.expression);
                    ran.extend(self.reported(reports));
                }
            }
            ran
        });
        let recompiled = self.recompiled(&analyses);
        for (unit, ran) in &ran {
            self.failed.retain(|name| name != unit);
            if *ran == Ran::Fails {
                self.failed.push(unit.C());
            }
            if !matches!(ran, Ran::No | Ran::Deleted) {
                let index = self.index(unit);
                self.states[index] = *ran;
            }
        }

        let mut outcomes: Vec<(String, Outcome)> = ran.iter()
            .map(|(unit, ran)| (unit.C(), Outcome { recompiles: recompiled.contains(unit), ran: *ran }))
            .collect();
        for unit in &recompiled {
            if !ran.iter().any(|(name, _)| name == unit) {
                outcomes.push((unit.C(), Outcome { recompiles: true, ran: Ran::No }));
            }
        }
        outcomes
    }

    /// The units whose typecheck or ownership analysis salsa ran in this step.
    ///
    /// What salsa ran is what it reports, and `analyses` only says which unit
    /// each of those ran for.
    ///
    /// An ownership analysis is attributed to the unit the compiler was
    /// compiling when it ran, which it marks for this. Its key can't say:
    /// the analysis is keyed on what earlier units left behind, so it runs
    /// under a key no unit has had before exactly when that changes.
    ///
    /// A typecheck's key can't say either: it is keyed on the unit and the
    /// session's environment, which changes when a unit requires a module,
    /// and the earlier units are typechecked again under the new one as a
    /// later unit looks a name up through them. Nor can the unit being
    /// compiled, since one unit's typecheck runs inside another's. So the
    /// typecheck says itself which unit it is for, in `analyses`, and every
    /// typecheck salsa ran must have said so, once.
    fn recompiled(&mut self, analyses: &[(Pass, Source)]) -> Vec<String> {
        let events = self.recorder.take();
        // The engine hears which units it analyzed only on its own thread.
        for event in &events {
            let analysis = event.query == "typecheck_script_unit" || event.query.starts_with("analyze_script_");
            assert!(!(analysis && event.on_worker), "{} ran on a worker, where the engine cannot see it", event.query);
        }
        let typechecks = events.iter().filter(|event| event.query == "typecheck_script_unit").count();
        let typechecked: Vec<Source> = analyses.iter()
            .filter(|(pass, _)| *pass == Pass::Typecheck)
            .map(|(_, source)| *source)
            .collect();
        assert_eq!(typechecked.len(), typechecks, "every typecheck salsa ran says which unit it is for, once");

        // The units' sources after the step; a unit the step deleted has none
        // left to name.
        let sources = self.engine.unit_sources();
        let mut recompiled: Vec<String> = Vec::new();
        let mut note = |unit: &String| {
            if !recompiled.contains(unit) {
                recompiled.push(unit.C());
            }
        };
        for source in typechecked {
            if let Some(index) = sources.iter().position(|unit| *unit == source) {
                note(&self.units[index]);
            }
        }
        for event in &events {
            if !event.query.starts_with("analyze_script_") {
                continue;
            }
            let index = event.compiling.expect("an ownership analysis runs inside a unit's compile");
            note(&self.units[index]);
        }
        recompiled
    }

    /// What each unit the engine reports came to, and the units it says it
    /// analyzed, for the recorder to check.
    ///
    /// A unit analyzed and not re-derived did not run.
    fn reported(&mut self, reports: Vec<datalove_script_engine::UnitStep>) -> Vec<(String, Ran)> {
        self.note_analyzed();
        reports.iter()
            .map(|report| (self.name(report.unit), report.eval.as_ref().map_or(Ran::No, ran)))
            .collect()
    }

    /// Note the units the engine's last step says it typechecked or analyzed
    /// for ownership; the recorder sees only those two passes.
    fn note_analyzed(&mut self) {
        for report in self.engine.last_step() {
            if report.compiled.typecheck || report.compiled.ownership {
                let unit = self.name(report.unit);
                if !self.analyzed.contains(&unit) {
                    self.analyzed.push(unit);
                }
            }
        }
    }

    fn append(&mut self, entry: &Entry) -> (String, Ran) {
        self.units.push(entry.unit.C());
        self.code.push(entry.code());
        let eval = self.engine.eval(entry.code());
        self.note_analyzed();
        self.states.push(ran(&eval));
        (entry.unit.C(), ran(&eval))
    }

    /// Say every way this session differs from a fresh run of its current
    /// code: each unit, in order, must have come to the same end and hold the
    /// same bindings.
    ///
    /// This is what a step is for, so it is checked whether or not the step
    /// has a table, and it does not trust the tables' `holds after`.
    fn differs_from_fresh(&mut self, at: &str) -> Vec<String> {
        let sys = datalove_sys_packages::system_library();
        let mut fresh = Engine::with_options(
            sys, self.engine.workspace().C(), datalove_datafun::DebugOutputMode::Disabled, self.also_roots.C(),
        ).expect("the engine starts");
        let states: Vec<Ran> = self.code.iter().map(|unit| ran(&fresh.eval(unit.C()))).collect();
        let ids = self.engine.unit_ids();
        let fresh_ids = fresh.unit_ids();
        let mut failures = Vec::new();
        for (index, unit) in self.units.iter().enumerate() {
            if self.states[index] != states[index] {
                failures.push(fmt!(
                    "{at}: unit {unit} came to {:?}, and to {:?} in a fresh run",
                    self.states[index], states[index],
                ));
                continue;
            }
            let held = holds(&self.engine.unit_bindings(ids[index]));
            let fresh_held = holds(&fresh.unit_bindings(fresh_ids[index]));
            if held != fresh_held {
                failures.push(fmt!("{at}: unit {unit} holds {held}, and {fresh_held} in a fresh run"));
            }
        }
        failures
    }
}

/// The workspace a session runs in.
pub struct Workspace {
    pub sys: SystemLibrary,
    pub descriptor: WorkspaceDescriptor,
    /// Modules compiled whether or not a unit requires them.
    pub also_roots: Vec<String>,
}

impl Workspace {
    /// The system library and nothing else.
    pub fn system() -> Workspace {
        let sys = datalove_sys_packages::system_library();
        let descriptor = WorkspaceDescriptor::from_system_library(&sys);
        Workspace { sys, descriptor, also_roots: Vec::new() }
    }
}

/// Run a session's steps, and say every way the engine disagreed with their
/// tables. `name` says whose steps they are in what goes wrong.
pub fn run(name: &str, workspace: Workspace, steps: &[Step]) -> Vec<String> {
    let mut session = Session::start(workspace.sys, workspace.descriptor, workspace.also_roots);
    let mut failures = Vec::new();

    for (number, step) in steps.iter().enumerate() {
        let touched = session.apply(&step.change);
        failures.extend(session.differs_from_fresh(&fmt!("{name} step {}", number + 1)));
        let Some(check) = &step.check else {
            continue;
        };
        let at = fmt!("{name} step {} ({})", number + 1, check.title);
        let mut recompiled: Vec<&String> = touched.iter()
            .filter(|(_, outcome)| outcome.recompiles)
            .map(|(name, _)| name)
            .collect();
        recompiled.sort();
        let mut analyzed: Vec<&String> = session.analyzed.iter().collect();
        analyzed.sort();
        if recompiled != analyzed {
            failures.push(fmt!(
                "{at}: salsa recompiled units {recompiled:?}, and the engine says it analyzed {analyzed:?}",
            ));
        }
        let observed = |unit: &str| {
            touched.iter().find(|(name, _)| name == unit).map(|(_, outcome)| *outcome)
                .unwrap_or(Outcome { recompiles: false, ran: Ran::No })
        };

        let deleted: Vec<&String> = touched.iter()
            .filter(|(_, outcome)| outcome.ran == Ran::Deleted)
            .map(|(name, _)| name)
            .collect();
        let mut listed: Vec<&String> = check.expect.iter().map(|row| &row.unit).collect();
        listed.sort();
        let mut present: Vec<&String> = session.units.iter().chain(deleted).collect();
        present.sort();
        if listed != present {
            failures.push(fmt!("{at}: the table lists units {listed:?}, the session has {present:?}"));
            continue;
        }

        for row in &check.expect {
            let rule = Outcome::parse(&row.this_step);
            let engine_row = check.engine.iter().find(|engine| engine.unit == row.unit);
            let expected = match engine_row {
                Some(engine) => {
                    let outcome = Outcome::parse(&engine.this_step);
                    if !outcome.is_conservative_for(rule) {
                        failures.push(fmt!(
                            "{at}: unit {}: the engine row says `{}`, which is not only more work than `{}`",
                            row.unit, engine.this_step, row.this_step,
                        ));
                    }
                    outcome
                }
                None => rule,
            };
            let seen = observed(&row.unit);
            if !seen.is_seen_as(expected) {
                let tighter = engine_row.is_some() && seen.is_seen_as(rule);
                failures.push(if tighter {
                    fmt!("{at}: unit {}: the engine now does what the rule says, `{}`; drop its engine row",
                        row.unit, row.this_step)
                } else if engine_row.is_none() && seen.is_conservative_for(rule) {
                    fmt!("{at}: unit {}: expected `{}`, the engine is conservative and `{}`; \
                        annotate it in an `engine` table if that is known",
                        row.unit, row.this_step, seen.describe())
                } else {
                    fmt!("{at}: unit {}: expected `{}`, got `{}`", row.unit, expected_text(row, engine_row), seen.describe())
                });
            }

            if seen.ran == Ran::Deleted {
                continue;
            }
            let index = session.index(&row.unit);
            if !session.failed.contains(&row.unit) {
                let mut listed: Vec<String> = match row.uses.as_str() {
                    "-" => Vec::new(),
                    uses => uses.split(", ").map(str::S).collect(),
                };
                listed.sort();
                let found = session.uses(index);
                if listed != found {
                    let found = if found.is_empty() { "-".S() } else { found.join(", ") };
                    failures.push(fmt!("{at}: unit {} {found}, the table says {}", row.unit, row.uses));
                }
            }
            let held = holds(&session.engine.unit_bindings(session.engine.unit_ids()[index]));
            if held != row.holds_after {
                failures.push(fmt!(
                    "{at}: unit {} holds {held}, the table says {}",
                    row.unit, row.holds_after,
                ));
            }
        }
        for engine in &check.engine {
            if !check.expect.iter().any(|row| row.unit == engine.unit) {
                failures.push(fmt!("{at}: an engine row for unit {}, which the table does not list", engine.unit));
            }
        }
    }
    failures
}

fn expected_text(row: &Row, engine: Option<&EngineRow>) -> String {
    match engine {
        Some(engine) => fmt!("{}` (engine; the rule says `{}", engine.this_step, row.this_step),
        None => row.this_step.C(),
    }
}
