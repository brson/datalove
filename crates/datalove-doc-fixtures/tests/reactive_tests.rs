//! Reactive sessions as step tables, checked against the engine.
//!
//! Each fixture in `reactive/` is a worldfile of script units followed
//! by steps. A step is one modification -- an edit, a delete, an insert, a
//! move or an append -- and an `expect` table saying what each unit should do in that
//! step and what its bindings should hold after it. Every fixture is shown in
//! `botdocs/script-reactivity-cases.md`, rendered from here, which is where
//! the rules they check are written down. How a table is checked is
//! `datalove_doc_fixtures::step_tables`.
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
//! | `scriptunit-move <unit> before\|after <unit>` | Nothing |
//! | `scriptunit-fragment [<unit>]` | A unit appended, after the first step |
//!
//! Units are named by the number they were entered as, whether appended or
//! inserted, and a delete renumbers nothing. A unit's header may name the id
//! it takes, and an insert's must; either way it is checked to be the next.
//!
//! `BLESS=1` re-renders the document's marked regions instead of checking
//! them. The step tables themselves are never blessed: they are the rules.

use rmx::prelude::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use datalove_doc_fixtures::step_tables::{Change, Check, Entry, Step, Workspace, run, sections};
use datalove_repl::Place;

const SEPARATOR: &str = datalove_doc_fixtures::step_tables::SEPARATOR;

/// The document whose examples are rendered from the fixtures, all of them.
const DOC: &str = "botdocs/script-reactivity-cases.md";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("reactive")
}

fn workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ============================================================================
// Fixtures
// ============================================================================

/// One fixture: the units entered first, then the steps, every one checked.
struct Fixture {
    name: String,
    /// The text of the unit sections before the first step, as written.
    units_text: String,
    steps: Vec<Step>,
}

impl Fixture {
    fn check(&self, number: usize) -> &Check {
        self.steps[number - 1].check.as_ref().expect("every step of a fixture is checked")
    }
}

fn parse_fixture(path: &Path) -> Fixture {
    let text = std::fs::read_to_string(path).X();
    let name = path.file_stem().X().to_string_lossy().S();
    let units_text = text.split(&fmt!("{SEPARATOR}\nexpect")).next().X().trim().S();

    let mut steps: Vec<Step> = Vec::new();
    let mut pending: Option<Change> = None;
    let mut entered = Vec::new();
    // The id the next unit entered takes, by fragment or by insert.
    let mut next = 1;
    let mut take_id = |id: Option<&str>| {
        let unit = next.to_string();
        if let Some(id) = id {
            assert_eq!(id, unit, "{name}: a new unit takes the next id");
        }
        next += 1;
        unit
    };

    for section in sections(&text) {
        let mut words = section.header.split_whitespace();
        let kind = words.next().X();
        let args: Vec<&str> = words.collect();
        match kind {
            "scriptunit-fragment" => {
                let entry = Entry { unit: take_id(args.first().copied()), source: section.content, expression: false };
                match steps.is_empty() {
                    true => entered.push(entry),
                    false => pending = Some(Change::Enter(vec![entry])),
                }
            }
            "scriptunit-edit" => pending = Some(Change::Edit { unit: args[0].S(), source: section.content }),
            "scriptunit-delete" => pending = Some(Change::Delete { unit: args[0].S() }),
            "scriptunit-move" => {
                let place = match args.get(1) {
                    Some(&"before") => Place::Before,
                    Some(&"after") => Place::After,
                    _ => panic!("{name}: `scriptunit-move <unit> before|after <unit>`"),
                };
                pending = Some(Change::Move { unit: args[0].S(), place, beside: args[2].S() });
            }
            "scriptunit-insert" => {
                assert_eq!(args.get(1), Some(&"after"), "{name}: `scriptunit-insert <unit> after <unit>`");
                let entry = Entry { unit: take_id(Some(args[0])), source: section.content, expression: false };
                pending = Some(Change::Insert { entry, place: Place::After, beside: args[2].S() });
            }
            "expect" => {
                let change = if steps.is_empty() {
                    Change::Enter(std::mem::take(&mut entered))
                } else {
                    pending.take().unwrap_or_else(|| panic!("{name}: an `expect` with no change before it"))
                };
                let check = Check::parse(&args.join(" "), &section.content);
                steps.push(Step { change, check: Some(check) });
            }
            "engine" => {
                let step = steps.last_mut().unwrap_or_else(|| panic!("{name}: an `engine` table before any step"));
                step.check.as_mut().X().add_engine(&section.content);
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

#[test]
fn every_step_table_holds_for_the_engine() {
    let mut failures = Vec::new();
    for fixture in fixtures() {
        failures.extend(run(&fixture.name, Workspace::system(), &fixture.steps));
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
    let mut by_step = Vec::new();
    for step in &fixture.steps {
        match &step.change {
            Change::Enter(entered) => {
                for entry in entered {
                    set(&mut sources, &entry.unit, &entry.source);
                }
            }
            Change::Edit { unit, source } => set(&mut sources, unit, source),
            Change::Insert { entry, .. } => set(&mut sources, &entry.unit, &entry.source),
            Change::Delete { .. } | Change::Move { .. } | Change::EditData { .. } => {}
        }
        by_step.push(sources.clone());
    }
    by_step
}

fn render_step(fixture: &Fixture, number: usize) -> String {
    let step = fixture.check(number);
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

/// The regions a document shows, as `(fixture, what)`.
fn regions(doc: &str) -> Vec<(String, String)> {
    doc.lines()
        .filter_map(|line| line.trim().strip_prefix("<!-- world ")?.strip_suffix(" -->"))
        .filter_map(|marker| marker.split_once(' '))
        .map(|(name, what)| (name.S(), what.S()))
        .collect()
}

#[test]
fn the_document_shows_the_fixtures() {
    let path = workspace_dir().join(DOC);
    let doc = std::fs::read_to_string(&path).X();
    let fixtures = fixtures();

    let shown = regions(&doc);
    let missing: Vec<String> = fixtures.iter()
        .flat_map(|fixture| {
            std::iter::once("code".S())
                .chain((1..=fixture.steps.len()).map(|number| fmt!("step {number}")))
                .filter(|what| !shown.contains(&(fixture.name.C(), what.C())))
                .map(|what| fmt!("{} {what}", fixture.name))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(missing.is_empty(), "{DOC} does not show: {}", missing.join(", "));

    let rendered = rerender(&doc, &fixtures);
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
