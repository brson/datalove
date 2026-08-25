//! Spans and diagnostics survive being held across a revision.
//!
//! They used to hold raw salsa ids. A `Text` is a tracked struct, which salsa
//! deletes when the query that produced it runs again; an `InternedText` is
//! collectable once it has gone unread for a few revisions, and its slot is
//! then handed to a different string. Reading an id past either point panics -
//! `tracked_struct.rs` does so outright, `interned.rs` under a debug assertion,
//! which means a build without debug assertions reads whatever now occupies
//! the slot instead.
//!
//! Both now name a `Source`, which is an input and so lives as long as the
//! database, and diagnostic text is stored as text rather than as an id.

use rmx::prelude::*;
use salsa::Setter as _;

use bcts::diagnostic::{DiagnosticBuilder, SpanEntry};
use bcts::input::Source;
use bcts::text::{InternedText, TextSpan};

/// A span outlives the revision it was recorded in.
#[test]
fn span_entry_survives_a_source_edit() {
    let mut db = bcts::Database::default();
    let source = Source::new(&db, "first version".to_string());

    // Record a span, the way a parser fills a span table.
    let entry = SpanEntry::new(source, 0..5);
    let (text, span) = entry.to_text_and_span(&db);
    assert_eq!(&text.as_str(&db)[span], "first");

    // Edit the source. The `Text` recorded against it is a tracked struct and
    // is gone; holding its id here used to panic on the next read.
    source.set_text(&mut db).to("second version".to_string());

    let (text, span) = entry.to_text_and_span(&db);
    assert_eq!(
        &text.as_str(&db)[span],
        "secon",
        "the span should resolve against the source's current text",
    );
}

/// Many revisions of unrelated interning do not disturb a stored span.
#[test]
fn span_entry_survives_interning_churn() {
    let mut db = bcts::Database::default();
    let source = Source::new(&db, "the quick brown fox".to_string());
    let entry = SpanEntry::new(source, 4..9);

    let churn = Source::new(&db, "churn-0".to_string());
    for revision in 1..12 {
        churn.set_text(&mut db).to(format!("churn-{}", revision));
        for extra in 0..64 {
            let _ = InternedText::new(&db, format!("filler-{}-{}", revision, extra));
        }
    }

    let (text, span) = entry.to_text_and_span(&db);
    assert_eq!(&text.as_str(&db)[span], "quick");
}

/// A stored diagnostic keeps its own words, whatever is interned after it.
#[test]
fn stored_diagnostic_survives_interning_churn() {
    let mut db = bcts::Database::default();
    let source = Source::new(&db, "let x = nope".to_string());
    let text = bcts::source_map::basic_source_map(&db, source).text(&db);

    let stored = DiagnosticBuilder::error(&db, "undefined variable: nope")
        .code("F001")
        .primary_label(TextSpan::new(text, 8..12), "not found in this scope")
        .note("check the spelling")
        .build_stored();

    // Churn enough interning to make any slot the diagnostic had referred to a
    // candidate for reuse.
    let churn = Source::new(&db, "churn-0".to_string());
    for revision in 1..12 {
        churn.set_text(&mut db).to(format!("churn-{}", revision));
        for extra in 0..64 {
            let _ = InternedText::new(&db, format!("filler-{}-{}", revision, extra));
        }
    }

    let diagnostic = stored.to_diagnostic(&db);
    assert_eq!(diagnostic.message.as_str(&db), "undefined variable: nope");
    assert_eq!(diagnostic.code.X().as_str(&db), "F001");
    assert_eq!(diagnostic.notes.len(), 1);
    assert_eq!(diagnostic.notes[0].as_str(&db), "check the spelling");

    let label = &diagnostic.labels[0];
    assert_eq!(label.message.X().as_str(&db), "not found in this scope");
    assert_eq!(&label.text.as_str(&db)[label.span.C()], "nope");
}

/// A diagnostic still reads correctly after its source has been edited.
#[test]
fn stored_diagnostic_survives_a_source_edit() {
    let mut db = bcts::Database::default();
    let source = Source::new(&db, "let x = nope".to_string());
    let text = bcts::source_map::basic_source_map(&db, source).text(&db);

    let stored = DiagnosticBuilder::error(&db, "undefined variable: nope")
        .primary_label(TextSpan::new(text, 8..12), "not found in this scope")
        .build_stored();

    source.set_text(&mut db).to("let x = also".to_string());

    // The words the diagnostic carries are its own and do not move.
    let diagnostic = stored.to_diagnostic(&db);
    assert_eq!(diagnostic.message.as_str(&db), "undefined variable: nope");
    assert_eq!(diagnostic.labels[0].message.X().as_str(&db), "not found in this scope");

    // The label still resolves against the source rather than panicking.
    let label = &diagnostic.labels[0];
    assert_eq!(&label.text.as_str(&db)[label.span.C()], "also");
}
