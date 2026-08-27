//! What the database costs to hold, measured rather than assumed.
//!
//! Tokens used to be a `#[salsa::tracked]` struct, which gave every token in
//! every module a salsa id, a page slot and revision metadata. For 24 four-line
//! modules that was 1032 tracked structs at 66 KB, the largest single thing in
//! the database - 43% of all tracked-struct bytes, for a value that is an
//! interned string, a byte range and a one-byte kind, and that nothing ever
//! looks up by identity.
//!
//! As plain values in the `Vec` a `ChunkLex` already held, the same tokens cost
//! 50 KB of heap. Counting the vector of ids the old arrangement also needed,
//! the change is 78 KB to 50 KB.
//!
//! The measurement only works because `ChunkLex` declares a `heap_size`:
//! salsa sizes fields by their stack size, and a `Vec` is three words whatever
//! it holds, so without that the tokens would appear to cost nothing at all and
//! this file would be measuring its own blind spot.

use rmx::prelude::*;

use datalove_datafun::incremental::{IncrementalModuleWorld, extract_dependencies};
use datalove_datafun_compiler::Database;
use datalove_datafun_compiler::module_graph::parse_module_graph;

/// Compile a world of `modules` modules and report what the database holds.
fn ingredients(modules: usize) -> Vec<(String, usize, usize)> {
    let db = Database::default();
    let mut world = IncrementalModuleWorld::new();
    for index in 0..modules {
        world.add_module(
            &db,
            &format!("local/test/m{}", index),
            &format!(
                "fun f{}(a: i32, b: i32): i32\n  let c = a + b * {}\n  ret c\nend fun\n",
                index, index,
            ),
        );
    }

    let deps = extract_dependencies(&world, &db);
    let (graph, requires) = world.build_fresh(&db, &deps);
    let parsed = parse_module_graph(&db, graph, requires, Vec::new());
    let _ = datalove_datafun_resolve::resolve_all_names(&db, parsed);

    <dyn salsa::Database>::memory_usage(&db)
        .structs
        .iter()
        .map(|i| {
            let bytes = i.size_of_fields()
                + i.size_of_metadata()
                + i.heap_size_of_fields().unwrap_or(0);
            (i.debug_name().S(), i.count(), bytes)
        })
        .filter(|(_, count, _)| *count > 0)
        .collect()
}

/// Tokens are values, so salsa should not be holding any.
#[test]
fn tokens_are_not_tracked_structs() {
    let held = ingredients(24);
    let token = held.iter().find(|(name, _, _)| name.ends_with("Token"));
    assert!(
        token.is_none(),
        "the database is holding {:?}; tokens are plain values in a ChunkLex \
         and should not be salsa structs",
        token,
    );
}

/// The token bytes are counted, not hidden behind a `Vec`.
///
/// Without this the test above would pass just as well if the tokens had been
/// moved somewhere salsa cannot see, which would look like a saving and be
/// nothing of the kind.
#[test]
fn the_tokens_are_still_accounted_for() {
    let held = ingredients(24);
    let (_, count, bytes) = held.iter()
        .find(|(name, _, _)| name.ends_with("ChunkLex"))
        .expect("the database should hold a ChunkLex per module")
        .C();

    assert_eq!(count, 24, "one ChunkLex per module");
    assert!(
        bytes > 24 * 1024,
        "24 modules of tokens should be tens of kilobytes, but ChunkLex \
         reports {} bytes; has the heap_size on ChunkLex been dropped?",
        bytes,
    );
}

/// Tokens should not be the biggest thing in the database any more.
#[test]
fn tokens_are_no_longer_the_largest_cost() {
    let held = ingredients(24);
    let total: usize = held.iter().map(|(_, _, bytes)| bytes).sum();

    let chunk_lex = held.iter()
        .find(|(name, _, _)| name.ends_with("ChunkLex"))
        .map(|(_, _, bytes)| *bytes)
        .unwrap_or(0);

    // Tokens were 43% of the database when each one was a tracked struct.
    assert!(
        chunk_lex * 100 / total < 40,
        "tokens are {}% of {} bytes; they were 43% before they became values, \
         so this has gone backwards",
        chunk_lex * 100 / total,
        total,
    );
}
