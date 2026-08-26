//! An edit that does not move a span leaves the span table alone.
//!
//! A span table used to record the salsa id of the `Text` its spans point
//! into. That id changes on every edit, because the text is one of the fields
//! `Text` is identified by, so every entry in the table differed after any
//! edit however small, and nothing reading the table could be reused.
//!
//! Recording the `Source` instead - an input, whose id does not move - means
//! entries differ only when the spans themselves do.

use rmx::prelude::*;
use salsa::Setter as _;

use std::sync::{Arc, Mutex};

use bct::input::Source;

#[salsa::db]
struct LoggingDatabase {
    storage: salsa::Storage<Self>,
    events: Arc<Mutex<Vec<salsa::Event>>>,
}

#[salsa::db]
impl salsa::Database for LoggingDatabase {}

impl LoggingDatabase {
    fn new() -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                events_clone.lock().unwrap().push(event);
            }))),
            events,
        }
    }

    fn executed(&self) -> usize {
        self.events.lock().X().iter()
            .filter(|e| matches!(e.kind, salsa::EventKind::WillExecute { .. }))
            .count()
    }

    fn clear(&self) {
        self.events.lock().X().clear();
    }
}

/// Stands in for anything that reads the span table.
#[salsa::tracked(returns(copy))]
fn count_spans(db: &dyn salsa::Database, source: Source) -> usize {
    datalove_datafun_parser::datafun_spans(db, source).entries.len()
}

const ORIGINAL: &str = "fun f(): i32\n  ret 1\nend fun\n";
/// Same length as `ORIGINAL`, so every span keeps its position.
const SAME_LENGTH: &str = "fun f(): i32\n  ret 2\nend fun\n";
/// Longer, so the spans after the literal shift along.
const LONGER: &str = "fun f(): i32\n  ret 4242\nend fun\n";

fn queries_to_recount(edited_to: &str) -> usize {
    let mut db = LoggingDatabase::new();
    let source = Source::new(&db, ORIGINAL.S());
    let _ = count_spans(&db, source);

    db.clear();
    source.set_text(&mut db).to(edited_to.S());
    let _ = count_spans(&db, source);
    db.executed()
}

/// An edit that moves nothing lets a reader of the span table be reused.
#[test]
fn an_edit_that_moves_no_span_costs_less_than_one_that_does() {
    let same_length = queries_to_recount(SAME_LENGTH);
    let longer = queries_to_recount(LONGER);

    assert!(
        same_length < longer,
        "an edit that leaves every span where it was should let salsa reuse \
         the readers of the span table, but it ran {} queries against {} for \
         an edit that does move them",
        same_length,
        longer,
    );
}

/// The span table itself is unchanged by an edit that moves nothing.
#[test]
fn an_edit_that_moves_no_span_leaves_the_table_identical() {
    // Take the table as owned data: it borrows the database, so it cannot be
    // carried across the edit.
    fn table(db: &LoggingDatabase, source: Source) -> Vec<(Option<String>, u32, usize, usize)> {
        let spans = datalove_datafun_parser::datafun_spans(db, source);
        spans.entries.iter()
            .map(|e| (
                e.expr_key.fn_name.map(|n| n.as_str(db).to_string()),
                e.expr_key.local_index,
                e.entry.span.start,
                e.entry.span.end,
            ))
            .collect()
    }

    let mut db = LoggingDatabase::new();
    let source = Source::new(&db, ORIGINAL.S());

    let before = table(&db, source);
    assert!(!before.is_empty(), "the function body has spans");

    source.set_text(&mut db).to(SAME_LENGTH.S());
    assert_eq!(before, table(&db, source), "no span should have moved");

    source.set_text(&mut db).to(LONGER.S());
    assert_ne!(before, table(&db, source), "a longer literal should move what follows it");
}
