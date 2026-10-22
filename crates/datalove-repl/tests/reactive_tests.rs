//! Reactive sessions as step tables, checked against the engine.
//!
//! Each fixture in `fixtures/reactive/` is a worldfile of script units followed
//! by steps. A step is one modification -- an edit, a delete, an insert or an
//! append -- and an `expect` table saying what each unit should do in that
//! step and what its bindings should hold after it. The tables are the rules
//! of `mandocs/drafts/reactive-script-semantics.md`, written down per step,
//! and that document's examples are rendered from these fixtures.
//!
//! An `expect` table states the rule, and the engine may do more work than it
//! asks for: rerun a unit the rule leaves alone, or recompile one the rule
//! only reruns. Each such row is annotated in an `engine` table after the
//! step, so the gap is written down. Doing less than the rule asks, or failing
//! differently, is never accepted, and an annotation the engine has caught up
//! with fails too, so the annotations stay true.
//!
//! The section headers after the units:
//!
//! | Header | Content |
//! |--------|---------|
//! | `expect <title>` | The step table: `unit`, `uses`, `this step`, `because`, `holds after` |
//! | `expect Step 2, ...: <title>` | The same, called something other than `Step N` |
//! | `engine` | Rows where the engine is conservative: `unit`, `this step`, `note` |
//! | `scriptunit-edit <unit>` | The unit's new source |
//! | `scriptunit-delete <unit>` | Nothing |
//! | `scriptunit-insert <unit> after <unit>` | The new unit's source |
//! | `scriptunit-fragment` | A unit appended, after the first step |
//!
//! Units are named by the number they were entered as, which a delete does
//! not change, and an inserted unit by a name like `2a`.
//!
//! `BLESS=1` re-renders the document's marked regions instead of checking
//! them. The step tables themselves are never blessed: they are the rules.

use rmx::prelude::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use datalove_ct::query_events::QueryRecorder;
use datalove_repl::{Command, Engine, Eval};

const SEPARATOR: &str = "----------";

/// The document whose examples are rendered from the fixtures.
const DOC: &str = "mandocs/drafts/reactive-script-semantics.md";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reactive")
}

fn workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ============================================================================
// Fixtures
// ============================================================================

/// One fixture: the units entered first, then the steps.
struct Fixture {
    name: String,
    /// The text of the unit sections before the first step, as written.
    units_text: String,
    steps: Vec<Step>,
}

struct Step {
    /// What the document calls the step, when not `Step N`: an `expect`
    /// header that starts with `Step` names it up to the first `: `.
    label: Option<String>,
    title: String,
    change: Change,
    expect: Vec<Row>,
    engine: Vec<EngineRow>,
}

enum Change {
    /// The units before the first `expect`, entered one at a time.
    Enter(Vec<String>),
    Edit { unit: String, source: String },
    Delete { unit: String },
    Insert { unit: String, after: String, source: String },
    Append(String),
}

struct Row {
    unit: String,
    uses: String,
    this_step: String,
    because: String,
    holds_after: String,
}

struct EngineRow {
    unit: String,
    this_step: String,
    note: String,
}

struct Section {
    header: String,
    content: String,
}

/// Split worldfile text into its sections.
fn sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_end() != SEPARATOR {
            assert!(lines[i].trim().is_empty(), "text outside a section: {:?}", lines[i]);
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

fn parse_fixture(path: &Path) -> Fixture {
    let text = std::fs::read_to_string(path).X();
    let name = path.file_stem().X().to_string_lossy().S();
    let units_text = text.split(&fmt!("{SEPARATOR}\nexpect")).next().X().trim().S();

    let mut steps: Vec<Step> = Vec::new();
    let mut pending: Option<Change> = None;
    let mut entered = Vec::new();

    for section in sections(&text) {
        let mut words = section.header.split_whitespace();
        let kind = words.next().X();
        let args: Vec<&str> = words.collect();
        match kind {
            "scriptunit-fragment" if steps.is_empty() => entered.push(section.content),
            "scriptunit-fragment" => pending = Some(Change::Append(section.content)),
            "scriptunit-edit" => pending = Some(Change::Edit { unit: args[0].S(), source: section.content }),
            "scriptunit-delete" => pending = Some(Change::Delete { unit: args[0].S() }),
            "scriptunit-insert" => {
                assert_eq!(args.get(1), Some(&"after"), "{name}: `scriptunit-insert <unit> after <unit>`");
                pending = Some(Change::Insert { unit: args[0].S(), after: args[2].S(), source: section.content });
            }
            "expect" => {
                let change = if steps.is_empty() {
                    Change::Enter(std::mem::take(&mut entered))
                } else {
                    pending.take().unwrap_or_else(|| panic!("{name}: an `expect` with no change before it"))
                };
                let expect = table(&section.content).into_iter()
                    .map(|cells| {
                        assert_eq!(cells.len(), 5, "{name}: an expect row has five cells: {cells:?}");
                        let [unit, uses, this_step, because, holds_after] = <[String; 5]>::try_from(cells).ok().X();
                        Row { unit, uses, this_step, because, holds_after }
                    })
                    .collect();
                let header = args.join(" ");
                let (label, title) = match header.split_once(": ") {
                    Some((label, title)) if label.starts_with("Step ") => (Some(label.S()), title.S()),
                    _ => (None, header),
                };
                steps.push(Step { label, title, change, expect, engine: Vec::new() });
            }
            "engine" => {
                let step = steps.last_mut().unwrap_or_else(|| panic!("{name}: an `engine` table before any step"));
                step.engine = table(&section.content).into_iter()
                    .map(|cells| {
                        assert_eq!(cells.len(), 3, "{name}: an engine row has three cells: {cells:?}");
                        let [unit, this_step, note] = <[String; 3]>::try_from(cells).ok().X();
                        EngineRow { unit, this_step, note }
                    })
                    .collect();
            }
            other => panic!("{name}: unknown section `{other}`"),
        }
    }
    assert!(pending.is_none(), "{name}: a change with no `expect` after it");
    Fixture { name, units_text, steps }
}

fn fixtures() -> Vec<Fixture> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixture_dir()).X()
        .map(|entry| entry.X().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "world"))
        .collect();
    paths.sort();
    paths.iter().map(|path| parse_fixture(path)).collect()
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
            dropped if dropped.trim_matches('_').ends_with(" dropped") => (false, Ran::No),
            "recompiles" => (true, Ran::No),
            "reruns" => (false, Ran::Runs),
            "compiles, runs" | "recompiles, runs" => (true, Ran::Runs),
            "compile fails" | "recompile fails" => (true, Ran::Fails),
            "compiles, returns early" | "recompiles, returns early" => (true, Ran::ReturnsEarly),
            "reruns, returns early" => (false, Ran::ReturnsEarly),
            "compiles, is blocked" | "recompiles, is blocked" => (true, Ran::Blocked),
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
            (_, Ran::Blocked) => "recompiles, is blocked",
            (_, Ran::Deleted) => "deleted",
        }
    }

    /// Whether the engine doing `self` is what the rule's `rule` looks like
    /// from outside.
    ///
    /// A fixup runs nothing, so it is seen as untouched, and the values tell
    /// the two apart. A unit that fails is not asked whether it recompiled:
    /// the doc says "recompile fails" whether or not the failing analysis
    /// was one salsa kept.
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
        Eval::Error(_) => Ran::Fails,
        Eval::ReturnedEarly(_) => Ran::ReturnsEarly,
        Eval::Blocked { .. } => Ran::Blocked,
        Eval::Nothing | Eval::Success(_) | Eval::SuccessExpr(_) => Ran::Runs,
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
    /// The units whose last compile failed, which leaves no record of what
    /// they do.
    failed: Vec<String>,
    recorder: QueryRecorder,
    /// The salsa key of each unit's typecheck, which is how a typecheck salsa
    /// ran is attributed to a unit.
    keys: Vec<(salsa::Id, String)>,
}

impl Session {
    fn start() -> Session {
        let recorder = QueryRecorder::new();
        let db = datalove_datafun::Database::recording(&recorder);
        let engine = Engine::in_database(db, datalove_sys_packages::system_library())
            .expect("the engine starts");
        recorder.clear();
        Session { engine, units: Vec::new(), failed: Vec::new(), recorder, keys: Vec::new() }
    }

    /// What unit `index` does to earlier units' bindings, as a step table's
    /// `uses` column says it, one entry per binding.
    fn uses(&self, index: usize) -> Vec<String> {
        let effects = self.engine.unit_effects(index);
        let mut uses: Vec<String> = effects.uses.iter()
            .filter_map(|name| {
                let provider = (0..index).rev()
                    .find(|earlier| self.engine.unit_effects(*earlier).provides.contains(name))?;
                let has = |names: &[String]| names.contains(name);
                let verb = if has(&effects.revived) {
                    "revives"
                } else if has(&effects.taken) {
                    "takes"
                } else if has(&effects.updated) {
                    "updates"
                } else if has(&effects.overwritten) {
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

    /// Make a step's change, and say what each unit did.
    fn apply(&mut self, change: &Change) -> Vec<(String, Outcome)> {
        let mut ran: Vec<(String, Ran)> = Vec::new();
        // Where the units from on get keys they did not have: the appended
        // units, or the suffix of a splice, which is chained anew.
        let rekeyed_from = match change {
            Change::Enter(sources) => {
                let from = self.units.len();
                for source in sources {
                    ran.push(self.append(source));
                }
                Some(from)
            }
            Change::Append(source) => {
                let from = self.units.len();
                ran.push(self.append(source));
                Some(from)
            }
            Change::Edit { unit, source } => {
                let index = self.index(unit);
                let reports = self.engine.edit_unit(index, source);
                ran.extend(self.reported(reports));
                None
            }
            Change::Delete { unit } => {
                let index = self.index(unit);
                self.units.remove(index);
                ran.push((unit.C(), Ran::Deleted));
                let reports = self.engine.remove_unit(index);
                ran.extend(self.reported(reports));
                Some(index)
            }
            Change::Insert { unit, after, source } => {
                let index = self.index(after) + 1;
                self.units.insert(index, unit.C());
                let reports = self.engine.insert_unit(index, source, false);
                ran.extend(self.reported(reports));
                Some(index)
            }
        };
        let recompiled = self.recompiled(rekeyed_from);
        for (unit, ran) in &ran {
            self.failed.retain(|name| name != unit);
            if *ran == Ran::Fails {
                self.failed.push(unit.C());
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
    /// An ownership analysis is attributed to the unit the compiler was
    /// compiling when it ran, which it marks for this. Its key can't say:
    /// the analysis is keyed on what earlier units left behind, so it runs
    /// under a key no unit has had before exactly when that changes.
    ///
    /// A typecheck is attributed by its key instead, because the engine
    /// typechecks units outside their own compile, and one unit's typecheck
    /// can run inside another's. A key salsa has used before names the unit
    /// it was used for. An edit
    /// moves no key, because a unit's typecheck is keyed on handles that
    /// survive `set_text`. Appending gives the new units keys, and so does a
    /// splice to every unit after it, because the chain of units is built
    /// again from there; those are typechecked in index order, so the new keys
    /// are attributed in the order they were first seen.
    fn recompiled(&mut self, rekeyed_from: Option<usize>) -> Vec<String> {
        let events = self.recorder.take();
        let mut seen: Vec<salsa::Id> = Vec::new();
        for event in &events {
            if event.query == "typecheck_script_unit" && !seen.contains(&event.key) {
                seen.push(event.key);
            }
        }
        if let Some(from) = rekeyed_from {
            let rekeyed = &self.units[from..];
            self.keys.retain(|(_, unit)| !rekeyed.contains(unit));
            let fresh: Vec<salsa::Id> = seen.iter().copied()
                .filter(|key| !self.keys.iter().any(|(known, _)| known == key))
                .collect();
            assert_eq!(
                fresh.len(), rekeyed.len(),
                "every unit from {from} on is typechecked under a new key, and nothing else is",
            );
            self.keys.extend(fresh.into_iter().zip(rekeyed.iter().cloned()));
        }
        let mut recompiled: Vec<String> = seen.iter()
            .map(|key| {
                self.keys.iter().find(|(known, _)| known == key)
                    .map(|(_, unit)| unit.C())
                    .expect("a typecheck under a key no unit has, so a key moved")
            })
            .collect();
        for event in &events {
            if !event.query.starts_with("analyze_script_") {
                continue;
            }
            let index = event.compiling.expect("an ownership analysis runs inside a unit's compile");
            let unit = &self.units[index];
            if !recompiled.contains(unit) {
                recompiled.push(unit.C());
            }
        }
        recompiled
    }

    fn reported(&self, reports: Vec<datalove_repl::UnitEdit>) -> Vec<(String, Ran)> {
        reports.iter()
            .map(|report| (self.units[report.unit].C(), ran(&report.eval)))
            .collect()
    }

    fn append(&mut self, source: &str) -> (String, Ran) {
        let next = self.units.iter()
            .filter_map(|name| name.parse::<usize>().ok())
            .max()
            .unwrap_or(0) + 1;
        let unit = next.to_string();
        self.units.push(unit.C());
        let eval = self.engine.eval(Command::ScriptStatement(source.S()));
        (unit, ran(&eval))
    }
}

/// Run one fixture, and say every way the engine disagreed with it.
fn run(fixture: &Fixture) -> Vec<String> {
    let mut session = Session::start();
    let mut failures = Vec::new();

    for (number, step) in fixture.steps.iter().enumerate() {
        let at = fmt!("{} step {} ({})", fixture.name, number + 1, step.title);
        let touched = session.apply(&step.change);
        let observed = |unit: &str| {
            touched.iter().find(|(name, _)| name == unit).map(|(_, outcome)| *outcome)
                .unwrap_or(Outcome { recompiles: false, ran: Ran::No })
        };

        let deleted: Vec<&String> = touched.iter()
            .filter(|(_, outcome)| outcome.ran == Ran::Deleted)
            .map(|(name, _)| name)
            .collect();
        let mut listed: Vec<&String> = step.expect.iter().map(|row| &row.unit).collect();
        listed.sort();
        let mut present: Vec<&String> = session.units.iter().chain(deleted).collect();
        present.sort();
        if listed != present {
            failures.push(fmt!("{at}: the table lists units {listed:?}, the session has {present:?}"));
            continue;
        }

        for row in &step.expect {
            let rule = Outcome::parse(&row.this_step);
            let engine_row = step.engine.iter().find(|engine| engine.unit == row.unit);
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
            let held = holds(&session.engine.unit_bindings(index));
            if held != row.holds_after {
                failures.push(fmt!(
                    "{at}: unit {} holds {held}, the table says {}",
                    row.unit, row.holds_after,
                ));
            }
        }
        for engine in &step.engine {
            if !step.expect.iter().any(|row| row.unit == engine.unit) {
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

#[test]
fn every_step_table_holds_for_the_engine() {
    let mut failures = Vec::new();
    for fixture in fixtures() {
        failures.extend(run(&fixture));
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

// ============================================================================
// The document
// ============================================================================

/// One unit's source on one line, as the `code` column shows it.
fn one_line(source: &str) -> String {
    source.lines()
        .map(|line| line.split("//").next().X().trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// The sources each step's table shows, by unit, without running anything.
fn sources_by_step(fixture: &Fixture) -> Vec<Vec<(String, String)>> {
    let mut sources: Vec<(String, String)> = Vec::new();
    let set = |sources: &mut Vec<(String, String)>, unit: &str, source: &str| {
        match sources.iter_mut().find(|(name, _)| name == unit) {
            Some((_, held)) => *held = source.S(),
            None => sources.push((unit.S(), source.S())),
        }
    };
    let mut next = 1;
    let mut by_step = Vec::new();
    for step in &fixture.steps {
        match &step.change {
            Change::Enter(entered) => {
                for source in entered {
                    set(&mut sources, &next.to_string(), source);
                    next += 1;
                }
            }
            Change::Append(source) => {
                set(&mut sources, &next.to_string(), source);
                next += 1;
            }
            Change::Edit { unit, source } | Change::Insert { unit, source, .. } => set(&mut sources, unit, source),
            Change::Delete { .. } => {}
        }
        by_step.push(sources.clone());
    }
    by_step
}

fn render_step(fixture: &Fixture, number: usize) -> String {
    let step = &fixture.steps[number - 1];
    let sources = &sources_by_step(fixture)[number - 1];
    let source = |unit: &str| sources.iter().find(|(name, _)| name == unit).map(|(_, source)| one_line(source)).X();

    let mut out = String::new();
    let label = step.label.C().unwrap_or_else(|| fmt!("Step {number}"));
    writeln!(out, "**{label}: {}.**", step.title).X();
    writeln!(out).X();
    writeln!(out, "| unit | code | uses from earlier units | this step | because | holds after |").X();
    writeln!(out, "|------|------|-------------------------|-----------|---------|-------------|").X();
    for row in &step.expect {
        writeln!(out, "| {} | `{}` | {} | {} | {} | {} |",
            row.unit, source(&row.unit), row.uses, row.this_step, row.because, row.holds_after).X();
    }
    if !step.engine.is_empty() {
        writeln!(out).X();
        writeln!(out, "<!--").X();
        writeln!(out, "Engine today, more than the rule asks:").X();
        for engine in &step.engine {
            writeln!(out, "unit {} {}: {}", engine.unit, engine.this_step, engine.note).X();
        }
        writeln!(out, "-->").X();
    }
    out
}

fn render(fixture: &Fixture, what: &str) -> String {
    match what.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["code"] => fmt!("```datalove\n{}\n```\n", fixture.units_text),
        ["step", number] => render_step(fixture, number.parse().X()),
        other => panic!("unknown region `{}`", other.join(" ")),
    }
}

/// The document with every marked region rendered again.
///
/// A region is `<!-- world NAME WHAT -->`, where `WHAT` is `code` or
/// `step N`, through `<!-- end world -->`.
fn rerender(doc: &str, fixtures: &[Fixture]) -> String {
    let mut out = String::new();
    let mut lines = doc.lines();
    while let Some(line) = lines.next() {
        writeln!(out, "{line}").X();
        let Some(marker) = line.trim().strip_prefix("<!-- world ").and_then(|rest| rest.strip_suffix(" -->")) else {
            continue;
        };
        let (name, what) = marker.split_once(' ').unwrap_or_else(|| panic!("a region names a fixture and what of it: {line}"));
        let fixture = fixtures.iter().find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("no fixture `{name}`"));
        for skipped in lines.by_ref() {
            if skipped.trim() == "<!-- end world -->" {
                break;
            }
        }
        write!(out, "{}", render(fixture, what)).X();
        writeln!(out, "<!-- end world -->").X();
    }
    out
}

#[test]
fn the_document_shows_the_fixtures() {
    let path = workspace_dir().join(DOC);
    let doc = std::fs::read_to_string(&path).X();
    let rendered = rerender(&doc, &fixtures());
    if rendered == doc {
        return;
    }
    if std::env::var("BLESS").is_ok() {
        std::fs::write(&path, rendered).X();
        return;
    }
    let differs = doc.lines().zip(rendered.lines())
        .position(|(held, wanted)| held != wanted)
        .map(|line| line + 1)
        .unwrap_or(doc.lines().count().min(rendered.lines().count()) + 1);
    panic!("{DOC} differs from its fixtures from line {differs}; run with BLESS=1 to render it");
}
