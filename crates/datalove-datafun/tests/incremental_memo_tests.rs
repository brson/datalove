//! An incremental rebuild costs what changed, not what exists.
//!
//! `extract_dependencies` runs after every edit. It used to read the text back
//! out of each module and hand it to `import_from_loader`, which makes a
//! `Source` per module - and a `Source` is an input, so those were new inputs
//! every time, carrying a new `PackageModule`, `Package` and `PackageWorld`
//! with them. Package resolution and everything under it started from nothing
//! on every call, and the discarded inputs stayed in the database, which does
//! not collect them.
//!
//! Passing the sources the world already holds means an unchanged world really
//! is the same world. Measured against the commit before that change:
//!
//! ```text
//!                    no edit          one module edited
//! modules   before    after     before        after
//!       4      29         0         29            6
//!      12      77         0         77            6
//!      24     149         0        149            6
//! ```

use rmx::prelude::*;

use std::sync::{Arc, Mutex};

use datalove_datafun::incremental::{IncrementalModuleWorld, extract_dependencies};

#[salsa::db]
#[derive(Clone)]
struct LoggingDatabase {
    storage: salsa::Storage<Self>,
    events: Arc<Mutex<Vec<salsa::Event>>>,
}

#[salsa::db]
impl salsa::Database for LoggingDatabase {}

impl datalove_datafun_tycheck::DbClone for LoggingDatabase {
    fn dyn_clone(&self) -> Box<dyn datalove_datafun_tycheck::DbClone + Send> {
        Box::new(self.clone())
    }

    fn as_salsa_db(&self) -> &dyn salsa::Database {
        self
    }
}

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

fn module_source(index: usize, value: usize) -> String {
    format!("fun f{}(): i32\n  ret {}\nend fun\n", index, value)
}

/// Build a world of `modules` modules and report what each step costs.
struct Costs {
    first_build: usize,
    rebuild_unchanged: usize,
    after_one_edit: usize,
}

fn costs_for(modules: usize) -> Costs {
    let mut db = LoggingDatabase::new();
    let mut world = IncrementalModuleWorld::new();
    for index in 0..modules {
        world.add_module(&db, &format!("local/test/m{}", index), &module_source(index, index));
    }

    db.clear();
    let _ = extract_dependencies(&world, &db);
    let first_build = db.executed();

    db.clear();
    let _ = extract_dependencies(&world, &db);
    let rebuild_unchanged = db.executed();

    world.update_source(&mut db, "local/test/m0", &module_source(0, 99));
    db.clear();
    let _ = extract_dependencies(&world, &db);
    let after_one_edit = db.executed();

    Costs { first_build, rebuild_unchanged, after_one_edit }
}

/// Asking again without changing anything runs nothing.
#[test]
fn rebuilding_an_unchanged_world_runs_nothing() {
    for modules in [4, 12, 24] {
        let costs = costs_for(modules);
        assert!(
            costs.first_build > 0,
            "the first build of {} modules has to do some work",
            modules,
        );
        assert_eq!(
            costs.rebuild_unchanged, 0,
            "asking again for the dependencies of {} unchanged modules should \
             run nothing, but it ran {} queries",
            modules, costs.rebuild_unchanged,
        );
    }
}

/// Editing one module costs the same however many modules there are.
///
/// This is the property that matters for an editing session: the cost tracks
/// what was touched, not how big the world is.
#[test]
fn an_edit_costs_the_same_whatever_the_world_size() {
    let small = costs_for(4);
    let large = costs_for(24);

    assert_eq!(
        small.after_one_edit, large.after_one_edit,
        "editing one module ran {} queries in a world of 4 and {} in a world \
         of 24; the cost should follow the edit, not the world",
        small.after_one_edit, large.after_one_edit,
    );

    // And the first build does grow with the world, so the comparison above is
    // not just measuring something that never moves.
    assert!(
        large.first_build > small.first_build,
        "building 24 modules should cost more than building 4",
    );
}

/// An edit costs a fraction of a build from scratch.
#[test]
fn an_edit_costs_much_less_than_a_first_build() {
    let costs = costs_for(24);
    assert!(
        costs.after_one_edit * 4 < costs.first_build,
        "editing one of 24 modules ran {} queries against {} for the first \
         build; it used to run essentially the whole build",
        costs.after_one_edit, costs.first_build,
    );
}
