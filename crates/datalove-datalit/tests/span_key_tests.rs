//! Expression spans are keyed by position, not by salsa id.
//!
//! A salsa `Id` is an index and a generation, and it only means anything in the
//! revision that minted it. A table that stores one is correct only for as long
//! as the table and the ids in it are produced together - true here, because
//! `datalit_spans` depends on `parse`, but true by a coupling nothing at the
//! call site can see. A position in the parse is ours, and means the same thing
//! whenever it is read.
//!
//! The numbering is a single sequence across a parse, which the parser has to
//! keep going by hand: a sub-parser starts where its parent had got to and
//! hands the counter back when it finishes. Get that wrong and two expressions
//! share a position, which is what these tests are watching for.

use rmx::prelude::*;

use bct::input::Source;
use datalove_datalit::Database;

/// The positions recorded for `src`, in table order.
fn positions(src: &str) -> Vec<u32> {
    let db = Database::default();
    let source = Source::new(&db, src.S());
    let _ = datalove_datalit::parser::parse(&db, source);
    datalove_datalit::spans::datalit_spans(&db, source)
        .entries(&db)
        .iter()
        .map(|e| e.expr_index)
        .collect()
}

/// The source text each recorded span points at.
fn slices(src: &str) -> Vec<String> {
    let db = Database::default();
    let source = Source::new(&db, src.S());
    let _ = datalove_datalit::parser::parse(&db, source);
    let text = source.text(&db);
    let mut out: Vec<String> = datalove_datalit::spans::datalit_spans(&db, source)
        .entries(&db)
        .iter()
        .map(|e| {
            let (_, span) = e.entry.to_text_and_span(&db);
            text[span.start..span.end].S()
        })
        .collect();
    out.sort();
    out
}

/// No two expressions in one parse may share a position.
///
/// Nesting is the case that matters: each braced group is parsed by a
/// sub-parser with its own state, and only the counter keeps them apart.
#[test]
fn every_expression_gets_its_own_position() {
    for src in [
        "1",
        "[1, 2, 3]",
        "[[1, 2], [3, 4]]",
        "[[[1]]]",
        ": ?u32 / 42",
        ": %{u32 = u32} / %{\n  0 = 5,\n  2 = 2,\n  3 = : u32 / 2,\n}",
        "[7, 7, 7, 7]",
    ] {
        let found = positions(src);
        let mut unique = found.C();
        unique.sort();
        unique.dedup();
        assert_eq!(
            found.len(), unique.len(),
            "parsing {:?} gave positions {:?}, which repeat; a sub-parser is \
             numbering from somewhere its parent will number from too",
            src, found,
        );
        assert_eq!(
            unique, (0..found.len() as u32).collect::<Vec<_>>(),
            "parsing {:?} gave positions {:?}; they should run from 0 without \
             gaps, since every expression takes exactly one",
            src, found,
        );
    }
}

/// Repeated identical elements are still distinct expressions.
///
/// These hash alike, so under the old scheme they were told apart only by the
/// disambiguator salsa assigns within a query.
#[test]
fn identical_elements_get_distinct_positions() {
    let found = positions("[7, 7, 7, 7]");
    assert_eq!(found.len(), 5, "the list and its four elements");
    let mut unique = found.C();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 5, "four identical 7s are still four expressions");
}

/// The positions point at the right text.
///
/// Unique positions would be worth nothing if they addressed the wrong spans.
#[test]
fn positions_address_the_right_spans() {
    assert_eq!(
        slices("[10, 20, 30]"),
        vec!["10".S(), "20".S(), "30".S(), "[10, 20, 30]".S()],
    );
    assert_eq!(
        slices("[[1, 2], [3]]"),
        vec!["1".S(), "2".S(), "3".S(), "[1, 2]".S(), "[3]".S(), "[[1, 2], [3]]".S()],
    );
}
