//! Splitting the parse in two keeps edits from spreading further than they mean to.
//!
//! `parse_module_full` returns the statements and the span table together;
//! `parse_module_ast` projects out just the statements. Name resolution reads
//! the projection, so an edit that moves spans around without changing a
//! statement stops at the projection: it returns an equal value, salsa
//! backdates it, and nothing downstream re-runs.
//!
//! Statements compare by tracked-struct identity, so an edit that changes a
//! literal but no name is equal here too - the literal lives in a tracked
//! field, which typechecking reads and name resolution does not. Renaming a
//! function is the case that really does change what names there are, and it is
//! the control below.

use rmx::prelude::*;

use datalove_ct::query_events::QueryRecorder;
use datalove_datafun::incremental::{IncrementalModuleWorld, Roots, extract_dependencies};
use datalove_datafun_compiler::Database;
use datalove_datafun_compiler::module_graph::parse_module_graph;

/// What ran when `local/test/a` was edited to `edit`.
struct AfterEdit {
    full_parses: usize,
    projections: usize,
    resolutions: usize,
}

fn after_editing_a(edit: &str) -> AfterEdit {
    let recorder = QueryRecorder::new();
    let mut db = Database::recording(&recorder);

    let mut world = IncrementalModuleWorld::new();
    world.add_module(&db, "local/test/a", "fun f(): i32\n  ret 1\nend fun\n");
    world.add_module(&db, "local/test/b", "fun g(): i32\n  ret 2\nend fun\n");

    let compile = |db: &Database, world: &IncrementalModuleWorld| {
        let deps = extract_dependencies(world, db);
        let (graph, requires) = world.build_graph(db, &deps, &Roots::All);
        let parsed = parse_module_graph(db, graph, requires, Vec::new());
        let _ = datalove_datafun_resolve::resolve_all_names(db, parsed);
    };

    compile(&db, &world);
    world.update_source(&mut db, "local/test/a", edit);
    recorder.clear();
    compile(&db, &world);

    let executed = recorder.take();
    let count = |name: &str| executed.iter().filter(|e| e.query == name).count();
    AfterEdit {
        full_parses: count("parse_module_full"),
        projections: count("parse_module_ast"),
        resolutions: count("resolve_module_names"),
    }
}

/// An edit that adds nothing after the last statement moves no span at all.
///
/// This also pins down that the projection is derived from the full parse
/// rather than parsing the source a second time. Written the other way round
/// the two queries are independent, the full parse backdating no longer spares
/// the projection, and this fails - which is the only cheap signal there is,
/// since two parses of one source produce the same tracked-struct identities
/// and so cost almost no extra memory to show up in a count.
#[test]
fn an_edit_that_moves_no_span_stops_at_the_full_parse() {
    let ran = after_editing_a("fun f(): i32\n  ret 1\nend fun\n\n");
    assert_eq!(ran.full_parses, 1, "the edited module has to be parsed again");
    assert_eq!(
        ran.projections, 0,
        "the full parse returned an equal value, so the projection should not \
         have been asked for at all",
    );
    assert_eq!(ran.resolutions, 0, "no name changed, so nothing should resolve again");
}

/// An edit that shifts every span still leaves the statements equal.
#[test]
fn an_edit_that_only_moves_spans_stops_at_the_projection() {
    let ran = after_editing_a("\nfun f(): i32\n  ret 1\nend fun\n");
    assert_eq!(ran.full_parses, 1, "the spans moved, so the full parse re-runs");
    assert_eq!(ran.projections, 1, "and the projection is asked for again");
    assert_eq!(
        ran.resolutions, 0,
        "but the statements are equal, so the projection backdates and name \
         resolution should not re-run",
    );
}

/// Changing a literal changes a tracked field, not the set of names.
#[test]
fn changing_a_literal_does_not_reach_name_resolution() {
    let ran = after_editing_a("fun f(): i32\n  ret 99\nend fun\n");
    assert_eq!(ran.full_parses, 1);
    assert_eq!(
        ran.resolutions, 0,
        "the literal lives in a tracked field; name resolution never reads it",
    );
}

/// Renaming a function does change the names, so this is the case that re-runs.
///
/// Without this the three tests above would pass just as well if name
/// resolution had stopped running for every edit.
#[test]
fn renaming_a_function_does_re_resolve_names() {
    let ran = after_editing_a("fun renamed(): i32\n  ret 1\nend fun\n");
    assert_eq!(ran.full_parses, 1);
    assert_eq!(ran.projections, 1);
    assert_eq!(
        ran.resolutions, 1,
        "renaming a function has to re-run name resolution for that module",
    );
}
