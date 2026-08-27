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

// ============================================================================
// Interned identity
// ============================================================================

use bcts::module_graph::{Module, ModuleId};
use bcts::package2::{Package, PackageModule, PackageWorld};

/// Building the same module twice is the same module.
///
/// As inputs these handed out a fresh identity per call, so two callers
/// describing the same thing held handles that compared unequal, and anything
/// keyed on them saw two of everything.
#[test]
fn building_the_same_module_twice_gives_one_module() {
    let db = bcts::Database::default();

    let id = ModuleId::new(&db, "local/test/a".to_string());
    let source = Source::new(&db, "let x = 1".to_string());

    assert_eq!(Module::new(&db, id, source), Module::new(&db, id, source));

    let other = Source::new(&db, "let x = 2".to_string());
    assert_ne!(
        Module::new(&db, id, source),
        Module::new(&db, id, other),
        "a different source is a different module",
    );
}

/// A module still names the same source after that source is edited.
///
/// The stronger statement - that the handle from before the edit equals the
/// one after - cannot be written: a `Module` borrows the database, so the
/// borrow checker refuses to carry one across `set_text`. That refusal is what
/// the lifetime is for, so what is checked here is the substance of it: the
/// module is still built from the same path and the same source, and the text
/// behind that source is the edited one.
#[test]
fn a_module_still_names_its_source_after_an_edit() {
    let mut db = bcts::Database::default();

    let source = Source::new(&db, "let x = 1".to_string());
    {
        let module = Module::new(&db, ModuleId::new(&db, "local/test/a".to_string()), source);
        assert_eq!(module.source(&db), source);
    }

    source.set_text(&mut db).to("let x = 2".to_string());

    let module = Module::new(&db, ModuleId::new(&db, "local/test/a".to_string()), source);
    assert_eq!(module.source(&db), source, "still the same source");
    assert_eq!(module.id(&db).path(&db), "local/test/a", "still the same path");
    assert_eq!(source.text(&db), "let x = 2", "whose text is the edited one");
}

/// Packages and worlds are their contents too.
///
/// Note what "the same contents" means: a `Source` is an input, so building
/// one twice from equal text gives two of them, and a world built over fresh
/// sources is a different world however alike it reads. Interning pays off
/// where the sources are carried over, which is the case an incremental
/// rebuild is in.
#[test]
fn building_the_same_package_world_twice_gives_one_world() {
    use rmx::std::collections::BTreeMap;

    let db = bcts::Database::default();
    let source = Source::new(&db, "let x = 1".to_string());

    let build = |source| {
        let module = PackageModule::new(&db, "main".to_string(), source);
        let mut modules = BTreeMap::new();
        modules.insert("main".to_string(), module);
        let package = Package::new(&db, "test".to_string(), modules);
        let mut library = BTreeMap::new();
        library.insert("test".to_string(), package);
        PackageWorld::new(&db, BTreeMap::new(), library)
    };

    assert_eq!(build(source), build(source), "one world, described twice");

    // A world over a different source is a different world.
    let other = Source::new(&db, "let x = 2".to_string());
    assert_ne!(build(source), build(other));
}

/// A source built twice from equal text is two sources, not one.
///
/// This is why the world test above threads one through: `Source` is an input
/// and an input's identity is its slot, not its contents. Anything built over
/// a freshly made source is fresh too, however far up the chain it sits.
#[test]
fn equal_text_does_not_make_one_source() {
    let db = bcts::Database::default();
    assert_ne!(
        Source::new(&db, "let x = 1".to_string()),
        Source::new(&db, "let x = 1".to_string()),
    );
}
