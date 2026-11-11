//! Generated worldfiles on every engine, each held to the IR walker.
//!
//! A seed makes a worldfile with worldgen, and [`engines::check`] runs it as
//! it runs a fixture of `engine_tests`: on the reference, then on every other
//! engine that applies, each of which must observe what the reference did.
//! Every unit of a generated world must also compile, since the generator
//! means to write only programs that do; one that does not is a generator
//! bug, or a compiler one.
//!
//! One seed in [`ONE_FRAGMENT_EVERY`] writes its script as one fragment, which
//! the AOT backends take; the rest cut it into units, some of them
//! expressions, as a session would.
//!
//! A failing world is written out under the target directory, and can be run
//! again on its own. See `botdocs/plan-worldgen-engines.md` for where this
//! suite is going.
//!
//! Environment:
//!
//! - `WORLDGEN_ENGINES_SEED`: the first seed, by default [`DEFAULT_SEED`], or
//!   `random`. The default is fixed, so that a failure is a change and not a
//!   draw.
//! - `WORLDGEN_ENGINES_COUNT`: how many seeds, by default [`DEFAULT_COUNT`].
//!   `just test-worldgen-engines` runs more.
//! - `WORLDGEN_ENGINES_WORLD`: a worldfile to check, rather than any seed, as
//!   a failing seed prints.
//! - `WORLDGEN_ENGINES_WRITE`: when set, every world is written out where a
//!   failing one is, passing or not, to be read.
//! - `DATALOVE_TEST_ENGINES`: the engines to run, as for `engine_tests`.

mod engines;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rmx::prelude::*;
use rmx::rayon::prelude::*;

use bct::random::{SeededRng, env_seeds};
use datalove_datafun::worldfile_analysis::Analysis;
use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};

/// The first seed of a default run.
const DEFAULT_SEED: u64 = 0;

/// How many seeds a default run checks.
const DEFAULT_COUNT: u64 = 20;

/// One seed in this many writes its script as one fragment.
const ONE_FRAGMENT_EVERY: u64 = 3;

/// The generator's configuration for a seed.
fn config(seed: u64) -> WorldGenConfig {
    let mut config = WorldGenConfig::default();
    config.one_fragment = SeededRng::derive(seed, "one-fragment").random_range(0..ONE_FRAGMENT_EVERY) == 0;
    config.expression_unit_probability = 15;
    config.counted_loop_probability = 70;
    config.loop_probability = 20;
    config.observe_bindings = true;
    config.call_every_function = true;
    config.fallible_script = true;
    config.qualified_calls = true;
    config.compute_from_variables = true;
    config.recursion_probability = 30;
    config.script_statements = (4, 12);
    config.arithmetic_probability = 50;
    config
}

/// What a run of worlds showed, over every world checked.
#[derive(Default)]
struct Shown {
    worlds: usize,
    units: usize,
    /// Fragments run to their end.
    completed: usize,
    /// Fragments that ended otherwise, by what they ended with.
    ended: BTreeMap<String, usize>,
    /// Lines of debug log.
    logged: usize,
}

impl Shown {
    fn add(&mut self, reference: &Analysis) {
        self.worlds += 1;
        for section in reference.sections.iter().filter(|s| s.section_type != "module") {
            self.units += 1;
            self.logged += section.debug_output.as_deref().map_or(0, |d| d.lines().count());
            if section.section_type != "scriptunit-fragment" {
                continue;
            }
            if section.output == "(fragment executed)" {
                self.completed += 1;
            } else {
                let how = section.output.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
                *self.ended.entry(how).or_insert(0) += 1;
            }
        }
    }

    fn print(&self) {
        println!(
            "shown: {} worlds, {} units, {} fragments run to the end, {} lines of debug log",
            self.worlds, self.units, self.completed, self.logged,
        );
        for (how, n) in &self.ended {
            println!("  {n} fragments ended with: {how}");
        }
    }
}

/// Check a worldfile on every engine, and that every unit of it compiled,
/// returning what the reference made of it.
///
/// A panic is a failure like any other, so that the world is written out.
fn check(source: &str) -> Result<Analysis, String> {
    let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engines::check(source)))
        .map_err(|panic| {
            let message = panic.downcast_ref::<&str>().map(|s| s.S())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            format!("panicked: {message}")
        })?;
    let (reference, _) = checked?;
    if !engines::compiled_cleanly(&reference) {
        return Err(format!("a unit did not compile:\n{}", compile_errors(&reference)));
    }
    Ok(reference)
}

/// What went wrong compiling each unit that did not compile.
fn compile_errors(reference: &Analysis) -> String {
    reference.sections.iter()
        .enumerate()
        .filter(|(_, s)| !engines::section_compiled(s))
        .map(|(i, s)| format!("  section {} ({}): {:?} {:?} {:?}\n", i + 1, s.section_type, s.typecheck, s.ownership, s.lowering))
        .collect()
}

/// Write a failing world where it can be read and run again.
fn write_out(seed: u64, world: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("worldgen-engines");
    std::fs::create_dir_all(&dir).expect("a directory for failing worlds");
    let path = dir.join(format!("seed-{seed}.world"));
    std::fs::write(&path, world).expect("a failing world written out");
    path
}

fn main() {
    if let Some(path) = std::env::var_os("WORLDGEN_ENGINES_WORLD") {
        let source = std::fs::read_to_string(&path).expect("a readable worldfile");
        match check(&source) {
            Ok(_) => println!("\x1b[32mpass\x1b[0m {}", path.display()),
            Err(e) => {
                println!("\x1b[31mFAIL\x1b[0m {}\n{e}", path.display());
                std::process::exit(1);
            }
        }
        return;
    }

    let seeds = env_seeds("WORLDGEN_ENGINES", DEFAULT_SEED, DEFAULT_COUNT);
    let write_all = std::env::var_os("WORLDGEN_ENGINES_WRITE").is_some();
    let shown = Mutex::new(Shown::default());
    let mut failures: Vec<(u64, String)> = seeds.iter().collect::<Vec<_>>()
        .into_par_iter()
        .filter_map(|seed| {
            let world = gen_worldfile_seeded(seed, config(seed));
            if write_all {
                write_out(seed, &world);
            }
            match check(&world) {
                Ok(reference) => {
                    shown.lock().X().add(&reference);
                    None
                }
                Err(e) => Some((seed, e)),
            }
        })
        .collect();
    failures.sort_by_key(|(seed, _)| *seed);

    let mut failed = false;
    for (seed, error) in &failures {
        let path = write_out(*seed, &gen_worldfile_seeded(*seed, config(*seed)));
        println!("\x1b[31mFAIL\x1b[0m seed {seed}: {error}");
        println!("  WORLDGEN_ENGINES_WORLD={}", path.display());
        failed = true;
    }
    println!(
        "worldgen engines: {} of {} seeds from {} passed",
        seeds.count - failures.len() as u64, seeds.count, seeds.start,
    );
    shown.into_inner().X().print();
    if let Err(e) = engines::report(seeds.is_default()) {
        println!("\x1b[31mFAIL\x1b[0m {e}");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
