//! Diagnostic spans survive an edit.
//!
//! Spans used to be keyed by raw salsa ids. An id carries a generation counter
//! that salsa bumps when it recycles a tracked-struct slot, and the diagnostic
//! path only carried the index, rebuilding the id with generation zero. On a
//! fresh database every generation is zero and the lookup matched; after an
//! edit it did not, and the error came out with no location attached.
//!
//! Keying spans on the expression's own identity, which is what these tests
//! check, takes salsa's numbering out of the picture entirely.

use rmx::prelude::*;
use salsa::Setter as _;

use datalove_datafun_compiler::Database;

/// Typecheck `text` and return the span of every diagnostic label it produced.
fn diagnostic_label_spans(db: &Database, source: bct::input::Source) -> Vec<(usize, usize)> {
    let script = datalove_datafun_parser::parse_for_diagnostics(db, source);
    let spans = datalove_datafun_parser::datafun_spans(db, source);
    let name_resolution = datalove_datafun_resolve::resolve_script_names(db, source, script.clone());

    let unit_spec = datalove_datafun_tycheck::ScriptUnitSpec::new(
        source,
        spans,
        datalove_datafun_tycheck::ScriptUnitKind::Fragment(script, name_resolution),
    );
    let batch_spec = datalove_datafun_tycheck::create_batch_spec(db, source, vec![unit_spec], vec![]);
    let _ = datalove_datafun_tycheck::type_check_script_units(db, batch_spec);

    let diagnostics = datalove_datafun_tycheck::type_check_script_units::accumulated::<
        datalove_diagnostic::TypeDiagnostic,
    >(db, batch_spec);

    diagnostics
        .iter()
        .flat_map(|d| {
            let diag = d.to_diagnostic(db);
            diag.labels.iter()
                .map(|label| (label.span.start, label.span.end))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A type error in source that is edited still reports where it is.
#[test]
fn type_error_keeps_its_span_across_a_revision() {
    // `nope` is undefined, so this is an error before and after the edit.
    let first = "let a: i32 = 1\nlet b: i32 = nope\n";
    let second = "let a: i32 = 2\nlet b: i32 = nope\n";

    let mut db = Database::default();
    let source = bct::input::Source::new(&db, first.S());

    let before = diagnostic_label_spans(&db, source);
    assert!(!before.is_empty(), "the undefined name should be reported with a span");
    for (start, end) in &before {
        assert!(start < end, "span is non-empty");
        assert!(*end <= first.len(), "span is inside the source");
    }

    // Edit the source. Reparsing recycles tracked-struct slots, which is what
    // used to invalidate the raw-id span keys.
    source.set_text(&mut db).to(second.S());

    let after = diagnostic_label_spans(&db, source);
    assert!(
        !after.is_empty(),
        "after an edit the diagnostic still has to carry a span, \
         but the label list came back empty",
    );
    for (start, end) in &after {
        assert!(start < end, "span is non-empty");
        assert!(*end <= second.len(), "span is inside the source");
    }

    // The error text did not move, so neither should its span.
    assert_eq!(
        before, after,
        "the same error in the same place should report the same span",
    );
}

/// Repeated edits keep resolving, not just the first one.
#[test]
fn type_error_keeps_its_span_across_several_revisions() {
    let mut db = Database::default();
    let source = bct::input::Source::new(&db, "let a: i32 = 0\nlet b: i32 = nope\n".S());

    let baseline = diagnostic_label_spans(&db, source);
    assert!(!baseline.is_empty(), "the undefined name should be reported with a span");

    for value in 1..6 {
        let text = format!("let a: i32 = {}\nlet b: i32 = nope\n", value);
        source.set_text(&mut db).to(text.S());

        let spans = diagnostic_label_spans(&db, source);
        assert_eq!(
            baseline, spans,
            "revision {} lost or moved the diagnostic span",
            value,
        );
    }
}

/// An expression's key is stable across an edit that does not touch it.
///
/// This is the property the span table depends on: the same expression in the
/// same function keeps the same key, whatever salsa does with its ids.
#[test]
fn expression_keys_are_stable_across_a_revision() {
    let keys = |db: &Database, source: bct::input::Source| {
        let spans = datalove_datafun_parser::datafun_spans(db, source);
        spans.entries.iter()
            .map(|e| (
                e.expr_key.fn_name.map(|n| n.as_str(db).to_string()),
                e.expr_key.local_index,
            ))
            .collect::<Vec<_>>()
    };

    let mut db = Database::default();
    let source = bct::input::Source::new(&db, "fun f(): i32\n  ret 1\nend fun\n".S());

    let before = keys(&db, source);
    assert!(!before.is_empty(), "the function body has expressions");

    // Rewrite the source to an identical body via a different revision.
    source.set_text(&mut db).to("fun f(): i32\n  ret 2\nend fun\n".S());
    let after = keys(&db, source);

    assert_eq!(
        before, after,
        "changing a literal should not renumber the expression keys",
    );
}
