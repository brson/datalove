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

use datalove_datafun::incremental::{IncrementalModuleWorld, Roots, extract_dependencies};
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
    let (graph, requires) = world.build_graph(&db, &deps, &Roots::All);
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

/// Tokens cost what values cost, not what tracked structs cost.
///
/// As a tracked struct per token this was 1032 structs and about 78 KB counting
/// the vector of ids; as values in the `Vec` a `ChunkLex` already held it is
/// about 50 KB. The ceiling is the old figure, so the test fails if they go back
/// to being structs -- which is what its sibling above checks directly, and this
/// checks by the bill.
///
/// **This asserted a share of the database and that was wrong.** Tokens were 43%
/// of all tracked-struct bytes when they were structs, so the test asked for
/// under 40% of the total -- and then removing a duplicate parse of the world cut
/// `ExprFun` and `StmtFun` in half, took the total from 135224 bytes to 107000,
/// and pushed tokens to 47% without a byte of them moving. A ratio against a
/// denominator that legitimately shrinks is a guard that fails on improvement.
/// Tokens are in fact still the largest single ingredient; they are just cheaper
/// than they were, which is what was actually meant.
#[test]
fn tokens_cost_what_values_cost() {
    let held = ingredients(24);
    let chunk_lex = held.iter()
        .find(|(name, _, _)| name.ends_with("ChunkLex"))
        .map(|(_, _, bytes)| *bytes)
        .expect("the database should hold a ChunkLex per module");

    assert!(
        chunk_lex < 66 * 1024,
        "tokens cost {chunk_lex} bytes; as tracked structs they cost about \
         78 KB and the structs alone were 66 KB, so this has gone backwards",
    );
}
