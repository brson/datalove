//! Tests for datalove-rt.
//!
//! This is a separate crate because testing the runtime requires creating
//! test values via datalove-datalit, but the runtime itself should not
//! depend on the compiler/datalit crate.
//!
//! Put tests in the tests directory.

/// The collection sizes a test runs, without the largest under Miri.
///
/// Miri takes most of an hour over a tree of a thousand elements. Sizes up to
/// 300 still reach three levels of B-tree, which is every shape the larger
/// ones exercise.
pub fn sizes<const N: usize>(all: [u32; N]) -> impl Iterator<Item = u32> {
    all.into_iter().filter(|&n| !cfg!(miri) || n <= 300)
}

/// A collection size, held to 300 under Miri; see [`sizes`].
pub const fn size(n: u32) -> u32 {
    if cfg!(miri) && n > 300 { 300 } else { n }
}
