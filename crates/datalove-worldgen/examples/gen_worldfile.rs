//! Generate a worldfile from a seed.
//!
//! Usage: cargo run -p datalove-worldgen --example gen_worldfile -- [SEED]

use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);

    let config = WorldGenConfig::default();
    let worldfile = gen_worldfile_seeded(seed, config);
    println!("{}", worldfile);
}
