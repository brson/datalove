//! Seeded random numbers, the only ones the workspace uses.
//!
//! Nothing here needs strong randomness: random numbers pick test programs,
//! edits and JIT decisions, and what matters is that a seed picks the same
//! ones again, in any run, on any machine and toolchain, so that a failure can
//! be repeated. So every generator is a [`SeededRng`], which is Pcg64, an
//! algorithm fixed by name, and `clippy.toml` keeps out the others: `StdRng`,
//! whose algorithm rand may replace in any version, `ThreadRng` and the rest.
//!
//! Seeds come from three places:
//!
//! - a number, written in a test or counted from a start, for
//!   [`SeededRng::new`];
//! - content, such as a fixture's source, through [`seed_of`];
//! - the OS, through [`fresh_seed`] and [`env_seed`], which print what they
//!   drew so that the run can be repeated with it.
//!
//! Two consumers of one seed must not draw the same stream, as a generated
//! program and the edits made to it would if both took `SeededRng::new(seed)`.
//! So the second takes [`SeededRng::derive`] with a key of its own. A key is
//! also how to make a choice that must not depend on the order choices are
//! made in, such as one per call site in a compile salsa may run in any order:
//! derive a generator from the site and draw once.
//!
//! Seeds are hashed with [`SeedHasher`], which is defined here and must not
//! change, rather than `DefaultHasher`, whose algorithm std may change in any
//! release, or FxHash, which `BCTS_LOOKUP_SEED` varies on purpose.
//!
//! The values drawn still come from rand's sampling, `random_range` and the
//! like, which a major version of rand may change. A rand upgrade can so
//! change what a seed makes, and tests tuned to a seed may need it moved.

#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use rmx::prelude::*;
use rmx::rand::rand_core::{Infallible, TryRng};
use rmx::rand_pcg::Pcg64;
use rmx::std::hash::{Hash, Hasher};

use crate::lookup::mix;

/// A deterministic random number generator.
///
/// It is a rand [`Rng`](rmx::rand::Rng), so the prelude's `RngExt` gives it
/// `random_range`, `random_bool` and the rest, and `SliceRandom` gives
/// `shuffle`.
#[derive(Clone, Debug)]
pub struct SeededRng(Pcg64);

impl SeededRng {
    /// The generator for `seed`.
    pub fn new(seed: u64) -> SeededRng {
        SeededRng(Pcg64::seed_from_u64(seed))
    }

    /// A generator for `seed` and `key`, independent of `new(seed)` and of
    /// the generator for any other key.
    pub fn derive(seed: u64, key: impl Hash) -> SeededRng {
        SeededRng::new(derive_seed(seed, key))
    }
}

impl TryRng for SeededRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        self.0.try_next_u32()
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        self.0.try_next_u64()
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        self.0.try_fill_bytes(dst)
    }
}

/// The seed `SeededRng::derive(seed, key)` uses, for a consumer that takes a
/// seed rather than a generator.
pub fn derive_seed(seed: u64, key: impl Hash) -> u64 {
    seed_of((seed, key))
}

/// A seed made from content, the same for the same content.
pub fn seed_of(content: impl Hash) -> u64 {
    let mut hasher = SeedHasher::default();
    content.hash(&mut hasher);
    hasher.finish()
}

/// A seed drawn from the OS, printed to stderr as `{name}={seed}`.
///
/// `name` is what the seed is given back through, an environment variable or
/// an option, so that the line printed says how to repeat the run.
pub fn fresh_seed(name: &str) -> u64 {
    let seed: u64 = rmx::rand::rng().random();
    eprintln!("{name}={seed}");
    seed
}

/// The seed in the environment variable `var`, or a fresh one if it is unset.
pub fn env_seed(var: &str) -> u64 {
    match rmx::std::env::var(var) {
        Ok(s) => s.parse().unwrap_or_else(|_| panic!("{var} is not a number: {s:?}")),
        Err(_) => fresh_seed(var),
    }
}

/// The hasher seeds are made with: FNV-1a, finished with splitmix64.
///
/// Integers are fed little-endian and `usize` as 64 bits, so that a seed is
/// the same on every target. Changing any of this changes every seed made
/// from content or a key.
#[derive(Clone, Debug)]
pub struct SeedHasher(u64);

impl Default for SeedHasher {
    fn default() -> SeedHasher {
        SeedHasher(0xcbf29ce484222325)
    }
}

impl Hasher for SeedHasher {
    fn finish(&self) -> u64 {
        mix(self.0)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ *byte as u64).wrapping_mul(0x100000001b3);
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.write(&[i]);
    }

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_u128(&mut self, i: u128) {
        self.write(&i.to_le_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    fn write_i8(&mut self, i: i8) {
        self.write_u8(i as u8);
    }

    fn write_i16(&mut self, i: i16) {
        self.write_u16(i as u16);
    }

    fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32);
    }

    fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64);
    }

    fn write_i128(&mut self, i: i128) {
        self.write_u128(i as u128);
    }

    fn write_isize(&mut self, i: isize) {
        self.write_u64(i as i64 as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draws(rng: &mut SeededRng) -> Vec<u64> {
        (0..4).map(|_| rng.random()).collect()
    }

    /// The streams and seeds are fixed: these values must never change.
    #[test]
    fn streams_are_pinned() {
        assert_eq!(draws(&mut SeededRng::new(0)), [
            0x20ae25dad4e2bc53, 0x58f938e12bbf69fb, 0xb5b235d60cf8e231, 0x09f71ee2b019a7da,
        ]);
        assert_eq!(seed_of("fixture"), 0xa6ef8fa5f19a257d);
        assert_eq!(derive_seed(7, "changes"), 0xb1b20c8128f4f1a0);
    }

    #[test]
    fn same_seed_same_stream() {
        assert_eq!(draws(&mut SeededRng::new(42)), draws(&mut SeededRng::new(42)));
        assert_eq!(draws(&mut SeededRng::derive(42, "a")), draws(&mut SeededRng::derive(42, "a")));
    }

    #[test]
    fn derived_streams_are_independent() {
        let base = draws(&mut SeededRng::new(42));
        let a = draws(&mut SeededRng::derive(42, "a"));
        let b = draws(&mut SeededRng::derive(42, "b"));
        assert_ne!(base, a);
        assert_ne!(base, b);
        assert_ne!(a, b);
    }
}
