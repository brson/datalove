//! Recording a session as a trace: each step holds the session as the REPL
//! would show it.

use rmx::prelude::*;

use datalove_repl::app::UnitStatus;
use datalove_repl::{Trace, record_world};

/// Record a worldfile of these sections, each a header and its source.
fn record(sections: &[(&str, &str)]) -> Trace {
    let fs = datalove_vio::MemFs::new();
    let path = std::path::Path::new("/session.world");
    let text: String = sections.iter()
        .map(|(header, source)| fmt!("----------\n{header}\n----------\n{source}\n"))
        .collect();
    fs.add_file(path, text);
    record_world(&fs, datalove_sys_packages::system_library(), path).X()
}

/// Each unit of a step: its number, status, and whether the step compiled
/// and ran it.
fn units(trace: &Trace, step: usize) -> Vec<(u32, UnitStatus, bool, bool)> {
    trace.steps[step].units.iter()
        .map(|unit| (unit.number, unit.status, unit.compiled, unit.ran))
        .collect()
}

#[test]
fn a_trace_follows_appends_and_edits() {
    let trace = record(&[
        ("scriptunit-fragment", "let a: int = 1"),
        ("scriptunit-fragment", "let b = a + 1"),
        ("repl-command", "/edit 1 let a: int = \"no\""),
        ("repl-command", "/edit 1 let a: int = 5"),
    ]);
    assert_eq!(trace.steps.len(), 4);
    assert_eq!(units(&trace, 0), [(1, UnitStatus::Ran, true, true)]);
    // An append leaves the units before it as they were, unmarked.
    assert_eq!(units(&trace, 1), [(1, UnitStatus::Ran, false, false), (2, UnitStatus::Ran, true, true)]);
    // The edit that breaks unit 1 takes `a` from unit 2.
    assert_eq!(units(&trace, 2), [(1, UnitStatus::NotCompiled, true, false), (2, UnitStatus::NotCompiled, true, false)]);
    assert!(!trace.steps[2].units[0].detail.diagnostics.is_empty(), "a unit that did not compile has diagnostics");
    assert_eq!(units(&trace, 3), [(1, UnitStatus::Ran, true, true), (2, UnitStatus::Ran, true, true)]);

    let b = &trace.steps[3].units[1];
    assert_eq!(b.detail.bindings, [("b".S(), Some("6".S()))]);
    assert_eq!(b.detail.reads, ["a"]);
    assert_eq!(trace.steps[3].environment.iter().map(|binding| binding.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn a_command_that_changes_nothing_leaves_the_session() {
    let trace = record(&[
        ("scriptunit-fragment", "let a: int = 1"),
        ("repl-command", "/delete 7"),
        ("repl-command", "/reset"),
    ]);
    assert_eq!(units(&trace, 1), units(&trace, 0));
    assert!(trace.steps[2].units.is_empty());
    assert!(trace.steps[2].environment.is_empty());
}

/// The worldfile's own data and modules are the session's library.
#[test]
fn a_world_holds_its_library() {
    let trace = record(&[
        ("data local/app/numbers", "[1, 2, 3]"),
        ("module local/app/sums", "fun total(ref xs: [int]): int\n  var sum = 0\n  for x in ref xs\n    set sum = sum + x\n  end for\n  ret sum\nend fun"),
        ("scriptunit-fragment", "require data local/app/numbers: [int]\nrequire module local/app/sums\nimport sums.total"),
        ("scriptunit-expr", "total(ref numbers)"),
    ]);
    let step = trace.steps.last().X();
    assert_eq!(step.units.iter().map(|unit| unit.status).collect::<Vec<_>>(), [UnitStatus::Ran, UnitStatus::Ran]);
    let datalove_repl::Outcome::Unit(datalove_repl::script::Eval::Value(value)) = &step.outcome else {
        panic!("the expression has a value: {:?}", step.outcome);
    };
    assert_eq!(value.value, "6");
}
