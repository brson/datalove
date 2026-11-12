//! Every program in `fixtures/engines/` on every engine.
//!
//! Only the IR walker's results are compared with a fixture's expected file;
//! every other engine is held to the IR walker by [`engines::check`], which
//! says which engines there are and what a fixture's preamble may ask of
//! them.

mod engines;

use std::path::Path;

/// Run every applicable engine on a fixture, returning the reference's
/// rendering for the expected file.
fn check(path: &Path) -> Result<String, String> {
    let source = std::fs::read_to_string(path).map_err(|e| format!("failed to read: {e}"))?;
    engines::check(&source).map(|(_, rendered)| rendered)
}

fn main() {
    let floors = !cfg!(miri) && datalove_exampletest::parse_test_filters().is_empty();
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), check)
        .fixture_subdir("engines")
        .file_extension("world")
        .allow_errors(true)
        .after_all(move || engines::report(&engines::Tally::take(), floors))
        .run();
}
