//! High-durability inputs are not revalidated by a low-durability edit.
//!
//! The system library is read once and does not change while a session runs,
//! so its sources are created at `Durability::HIGH`. Salsa tracks the last
//! revision at which each durability level changed, which lets it skip
//! revalidating a query whose inputs are all more durable than anything that
//! has been touched since. Editing a local module should therefore leave
//! everything reached only through `sys/std` alone.

use rmx::prelude::*;
use salsa::Setter as _;

use std::sync::{Arc, Mutex};

use bct::input::Source;
use salsa::Durability;

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

    fn validated(&self) -> usize {
        self.events.lock().X().iter()
            .filter(|e| matches!(e.kind, salsa::EventKind::DidValidateMemoizedValue { .. }))
            .count()
    }
}

/// Count the words in a source. Stands in for any analysis of a module.
#[salsa::tracked(returns(copy))]
fn word_count(db: &dyn salsa::Database, source: Source) -> usize {
    source.text(db).split_whitespace().count()
}

/// A second layer, so there is a dependency chain for salsa to walk.
#[salsa::tracked(returns(copy))]
fn doubled_word_count(db: &dyn salsa::Database, source: Source) -> usize {
    word_count(db, source) * 2
}

/// A low-durability edit does not make salsa walk high-durability work.
///
/// Durability does not change what is invalidated - a query over an untouched
/// input was never invalid - it changes whether salsa has to walk the
/// dependency chain to find that out. With the system library at HIGH and
/// nothing at that durability having changed, salsa can mark the whole chain
/// verified without visiting it.
#[test]
fn low_durability_edit_does_not_walk_high_durability_work() {
    fn revalidations_after_local_edit(system_durability: Durability) -> usize {
        let mut db = LoggingDatabase::new();

        let system = Source::builder("fun a fun b fun c".to_string())
            .text_durability(system_durability)
            .new(&db);
        let local = Source::builder("let x = 1".to_string())
            .text_durability(Durability::LOW)
            .new(&db);

        assert_eq!(doubled_word_count(&db, system), 12);
        assert_eq!(word_count(&db, local), 4);

        // Edit the local module only.
        local.set_text(&mut db)
            .with_durability(Durability::LOW)
            .to("let x = 2".to_string());

        db.clear();
        assert_eq!(doubled_word_count(&db, system), 12);
        db.validated()
    }

    let durable = revalidations_after_local_edit(Durability::HIGH);
    let not_durable = revalidations_after_local_edit(Durability::LOW);

    assert!(
        durable < not_durable,
        "marking the system library durable should save salsa some walking: \
         it revalidated {} entries either way",
        durable,
    );
}

/// Editing the durable source does re-run its analysis.
#[test]
fn high_durability_edit_still_revalidates() {
    let mut db = LoggingDatabase::new();

    let system = Source::builder("fun a fun b fun c".to_string())
        .text_durability(Durability::HIGH)
        .new(&db);
    assert_eq!(word_count(&db, system), 6);

    db.clear();
    system.set_text(&mut db)
        .with_durability(Durability::HIGH)
        .to("fun a fun b".to_string());

    assert_eq!(word_count(&db, system), 4);
    assert!(db.executed() > 0, "editing the durable source has to re-run its analysis");
}
