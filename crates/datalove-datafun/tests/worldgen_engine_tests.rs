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
//! Seeds are checked in worker processes, this test run again, one for each
//! CPU, each sent a seed at a time, so that a seed that crashes the process
//! or never ends fails alone and by name; a new worker takes the next seed. What the
//! engines did is counted in each worker and added up here, and on the
//! default seeds held to floors: see [`Shown::floors`] and
//! [`engines::report`].
//!
//! A failing world is written out under the target directory, shrunk --
//! sections, then blocks, then lines dropped while it fails the same way,
//! each candidate checked in a process of its own -- and written out again.
//! Either can be run on its own, and the shrunk one, with a comment saying
//! what it shows, made a fixture of `engine_tests`. See
//! `botdocs/plan-worldgen-engines.md` for where this suite is going.
//!
//! Environment:
//!
//! - `WORLDGEN_ENGINES_SEED`: the first seed, by default [`DEFAULT_SEED`], or
//!   `random`. The default is fixed, so that a failure is a change and not a
//!   draw.
//! - `WORLDGEN_ENGINES_COUNT`: how many seeds, by default [`DEFAULT_COUNT`].
//!   `just test-worldgen-engines` runs more.
//! - `WORLDGEN_ENGINES_WORLD`: a worldfile to check, rather than any seed, as
//!   a failing seed prints. Checked in this process.
//! - `WORLDGEN_ENGINES_WRITE`: when set, every world is written out where a
//!   failing one is, passing or not, to be read.
//! - `WORLDGEN_ENGINES_NO_SHRINK`: write failing worlds out unshrunk.
//! - `DATALOVE_TEST_ENGINES`: the engines to run, as for `engine_tests`.

mod engines;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use rmx::prelude::*;
use rmx::rayon::prelude::*;
use serde::{Deserialize, Serialize};

use bct::random::{SeededRng, env_seeds};
use datalove_datafun::driver::{LoweringResult, OwnershipResult, TypecheckResult};
use datalove_datafun::worldfile_analysis::Analysis;
use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};
use engines::Tally;

/// The first seed of a default run.
const DEFAULT_SEED: u64 = 0;

/// How many seeds a default run checks.
const DEFAULT_COUNT: u64 = 40;

/// One seed in this many writes its script as one fragment.
const ONE_FRAGMENT_EVERY: u64 = 3;

/// How long a worker may take over one seed before it is killed and the
/// seed failed. Most take well under a second.
const SEED_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a shrink candidate may take; one that takes longer does not
/// fail the same way.
const SHRINK_TIMEOUT: Duration = Duration::from_secs(30);

/// The most failing worlds a run shrinks. A change that breaks many seeds
/// breaks them alike, more often than not.
const MAX_SHRUNK: usize = 3;

/// The most candidates one shrink checks.
const MAX_SHRINK_RUNS: usize = 2000;

/// What a worker's line starts with, telling it from anything else the
/// process prints.
const PROTOCOL: &str = "@worldgen ";

/// What a world checked on its own is said to have done, at the start of
/// a line, which [`check_apart`] looks for.
const PASS: &str = "\x1b[32mpass\x1b[0m";
const FAIL: &str = "\x1b[31mFAIL\x1b[0m";

/// The variable that makes this process a worker, checking each seed it
/// is sent on stdin, a line each, until stdin ends.
const WORKER: &str = "WORLDGEN_ENGINES_WORKER";

/// The generator's configuration for a seed: [`WorldGenConfig::engines`],
/// written as one fragment for one seed in [`ONE_FRAGMENT_EVERY`].
fn config(seed: u64) -> WorldGenConfig {
    WorldGenConfig {
        one_fragment: SeededRng::derive(seed, "one-fragment").random_range(0..ONE_FRAGMENT_EVERY) == 0,
        ..WorldGenConfig::engines()
    }
}

/// What a run of worlds showed, over every world checked.
#[derive(Default, Serialize, Deserialize)]
struct Shown {
    worlds: usize,
    units: usize,
    /// Fragments run to their end.
    completed: usize,
    /// Fragments that ended otherwise, by the first two words of what they
    /// ended with: `er error` for one that returned early, `(not run:` for
    /// one blocked by such a one.
    ended: BTreeMap<String, usize>,
    /// Lines of debug log.
    logged: usize,
}

impl Shown {
    fn of(reference: &Analysis) -> Shown {
        let mut shown = Shown { worlds: 1, ..Shown::default() };
        for section in reference.sections.iter().filter(|s| s.section_type != "module") {
            shown.units += 1;
            shown.logged += section.debug_output.as_deref().map_or(0, |d| d.lines().count());
            if section.section_type != "scriptunit-fragment" {
                continue;
            }
            if section.output == "(fragment executed)" {
                shown.completed += 1;
            } else {
                let how = section.output.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
                *shown.ended.entry(how).or_insert(0) += 1;
            }
        }
        shown
    }

    fn add(&mut self, other: &Shown) {
        self.worlds += other.worlds;
        self.units += other.units;
        self.completed += other.completed;
        self.logged += other.logged;
        for (how, n) in &other.ended {
            *self.ended.entry(how.C()).or_insert(0) += n;
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

    /// What the default seeds have to have shown, besides what
    /// [`engines::report`] asks of the engines: fragments that ran to the
    /// end, fragments that returned early with an error and fragments
    /// blocked by one, and enough worlds of one fragment for the AOT
    /// backends to have run on a tenth of them.
    fn floors(&self, tally: &Tally) -> Result<(), String> {
        if self.completed == 0 {
            return Err("no fragment ran to its end".S());
        }
        if !self.ended.contains_key("er error") {
            return Err("no fragment returned early with an error".S());
        }
        if !self.ended.contains_key("(not run:") {
            return Err("no fragment was blocked by one that returned early".S());
        }
        for aot in ["aot", "c"] {
            if datalove_exampletest::engine_enabled(aot) && (tally.ran(aot) as usize) * 10 < self.worlds {
                return Err(format!("the {aot} backend ran on {} of {} worlds, under a tenth", tally.ran(aot), self.worlds));
            }
        }
        Ok(())
    }
}

/// Check a worldfile on every engine, and that every unit of it compiled,
/// returning what the reference made of it.
///
/// A panic is a failure like any other, so that the world is written out.
fn check(source: &str) -> Result<Analysis, String> {
    let (reference, _) = caught(|| engines::check(source), "panicked")??;
    if !engines::compiled_cleanly(&reference) {
        return Err(format!("a unit did not compile:\n{}", compile_errors(&reference)));
    }
    Ok(reference)
}

/// `f`'s answer, or what it panicked with, after `what`.
fn caught<T>(f: impl FnOnce() -> T, what: &str) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|panic| {
        let message = panic.downcast_ref::<&str>().map(|s| s.S())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        format!("{what}: {message}")
    })
}

/// The world a seed makes, or how the generator failed to make it.
fn generate(seed: u64) -> Result<String, String> {
    caught(|| gen_worldfile_seeded(seed, config(seed)), "the generator panicked")
}

/// What went wrong compiling each unit that did not compile.
///
/// Only those that failed a phase, and not those whose later phases were
/// skipped for it, which every unit's are where one fails: the first line is
/// then the first failure, which a shrink keeps to.
fn compile_errors(reference: &Analysis) -> String {
    reference.sections.iter()
        .enumerate()
        .filter(|(_, s)| {
            matches!(s.typecheck, TypecheckResult::Error { .. })
                || matches!(s.ownership, OwnershipResult::Error { .. })
                || matches!(s.lowering, LoweringResult::Error { .. })
                || matches!(s.typecheck, TypecheckResult::ParseError { .. })
        })
        .map(|(i, s)| format!("  section {} ({}): {:?} {:?} {:?}\n", i + 1, s.section_type, s.typecheck, s.ownership, s.lowering))
        .collect()
}

/// Where failing worlds, and every world with `WORLDGEN_ENGINES_WRITE`, are
/// written.
fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("worldgen-engines");
    std::fs::create_dir_all(&dir).expect("a directory for failing worlds");
    dir
}

/// Write a world out as `name`, returning where.
fn write_out(name: &str, world: &str) -> PathBuf {
    let path = out_dir().join(name);
    std::fs::write(&path, world).expect("a world written out");
    path
}

// ============================================================================
// Workers
// ============================================================================

/// What a worker says of a seed, on a line.
#[derive(Serialize, Deserialize)]
enum Said {
    /// The seed passed, showing this, and the engines did that over it.
    Pass(Shown, Tally),
    /// The seed failed, so, and the engines did that over it.
    Fail(String, Tally),
}

/// Check each seed sent, saying how it went.
fn work() {
    let write_all = std::env::var_os("WORLDGEN_ENGINES_WRITE").is_some();
    for line in std::io::stdin().lines() {
        let seed: u64 = line.X().parse().X();
        let checked = generate(seed).and_then(|world| {
            if write_all {
                write_out(&format!("seed-{seed}.world"), &world);
            }
            check(&world)
        });
        let said = match checked {
            Ok(reference) => Said::Pass(Shown::of(&reference), Tally::take()),
            Err(e) => Said::Fail(e, Tally::take()),
        };
        println!("{PROTOCOL}{}", ron::to_string(&said).X());
    }
}

/// A child process of this test, and its stdout a line at a time.
struct Spawned {
    child: Child,
    lines: Receiver<String>,
}

impl Spawned {
    fn new(env: &[(&str, &str)]) -> Spawned {
        let mut command = Command::new(std::env::current_exe().X());
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("this test run again");
        let stdout = child.stdout.take().X();
        let (send, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Spawned { child, lines }
    }

    /// Kill it, if it has not ended, and say how it ended.
    fn end(mut self) -> String {
        let _ = self.child.kill();
        self.child.wait().map_or_else(|e| e.S(), |status| status.S())
    }
}

/// What the workers made of some seeds.
#[derive(Default)]
struct Checked {
    shown: Shown,
    tally: Tally,
    failures: Vec<(u64, String)>,
}

impl Checked {
    fn add(&mut self, other: Checked) {
        self.shown.add(&other.shown);
        self.tally.add(&other.tally);
        self.failures.extend(other.failures);
    }
}

/// Check seeds in a worker, taking the next from `next` until it reaches
/// `end`, and starting another worker after any that dies on a seed or
/// never finishes it.
fn check_in_worker(next: &AtomicU64, end: u64) -> Checked {
    let mut checked = Checked::default();
    let mut worker: Option<Spawned> = None;
    loop {
        let seed = next.fetch_add(1, Ordering::Relaxed);
        if seed >= end {
            break;
        }
        let spawned = worker.get_or_insert_with(|| Spawned::new(&[(WORKER, "")]));
        let mut stdin = spawned.child.stdin.as_ref().X();
        // A worker that died has no stdin to write to; its end says why.
        let sent = writeln!(stdin, "{seed}").and_then(|()| stdin.flush()).is_ok();
        loop {
            let line = match sent {
                true => spawned.lines.recv_timeout(SEED_TIMEOUT),
                false => Err(RecvTimeoutError::Disconnected),
            };
            match line {
                Ok(line) => {
                    let Some(said) = line.strip_prefix(PROTOCOL) else {
                        println!("{line}");
                        continue;
                    };
                    let tally = match ron::from_str::<Said>(said).X() {
                        Said::Pass(shown, tally) => {
                            checked.shown.add(&shown);
                            tally
                        }
                        Said::Fail(e, tally) => {
                            checked.failures.push((seed, e));
                            tally
                        }
                    };
                    checked.tally.add(&tally);
                }
                Err(timeout_or_end) => {
                    let how = match timeout_or_end {
                        RecvTimeoutError::Timeout => format!("timed out after {SEED_TIMEOUT:?}"),
                        RecvTimeoutError::Disconnected => "the worker ended".S(),
                    };
                    let status = worker.take().X().end();
                    checked.failures.push((seed, format!("{how}: {status}")));
                }
            }
            break;
        }
    }
    if let Some(mut spawned) = worker {
        drop(spawned.child.stdin.take());
        spawned.child.wait().X();
    }
    checked
}

// ============================================================================
// Shrinking
// ============================================================================

/// What a failure is, for a shrink to keep to: the first two lines of what
/// it said, with numbers left out, or for an engine that differed from the
/// reference, which one.
fn signature(error: &str) -> String {
    let error = match error.find(" differs from the reference") {
        Some(at) => &error[..at],
        None => error,
    };
    error.lines().take(2).collect::<Vec<_>>().join("\n")
        .chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect()
}

/// How a world fares checked in a process of its own: what it failed with,
/// none if it passed.
fn check_apart(path: &Path) -> Option<String> {
    let checker = Spawned::new(&[("WORLDGEN_ENGINES_WORLD", &path.to_string_lossy())]);
    let deadline = Instant::now() + SHRINK_TIMEOUT;
    let mut lines = Vec::new();
    loop {
        match checker.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => lines.push(line),
            Err(RecvTimeoutError::Timeout) => {
                checker.end();
                return Some(format!("timed out after {SHRINK_TIMEOUT:?}"));
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = checker.end();
    if lines.iter().any(|line| line.starts_with(PASS)) {
        return None;
    }
    // A failure is said on the lines after the one saying it failed.
    match lines.iter().position(|line| line.starts_with(FAIL)) {
        Some(at) => Some(lines[at + 1..].join("\n")),
        None => Some(format!("the worker ended: {status}")),
    }
}

/// The world split into its sections, each with its header.
fn sections(world: &str) -> Vec<String> {
    let lines: Vec<&str> = world.split_inclusive('\n').collect();
    let mut sections: Vec<String> = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let header = lines[at].starts_with("----------")
            && lines.get(at + 2).is_some_and(|l| l.starts_with("----------"));
        if header || sections.is_empty() {
            sections.push(String::new());
        }
        let take = if header { 3 } else { 1 };
        for line in &lines[at..(at + take).min(lines.len())] {
            sections.last_mut().X().push_str(line);
        }
        at += take;
    }
    sections
}

/// Whether a line is one a shrink keeps: a section's header, or a `break`,
/// without which the loop it ends may never end.
fn kept_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("----------") || line.starts_with("scriptunit") || line.starts_with("module ") || line == "break"
}

/// The line ending the block `lines[at]` opens, if it opens one.
fn block_end(lines: &[&str], at: usize) -> Option<usize> {
    let line = lines[at];
    let indent = &line[..line.len() - line.trim_start().len()];
    let opens = ["fun ", "loop", "if ", "for ", "match "].iter().any(|word| line.trim_start().starts_with(word));
    if !opens {
        return None;
    }
    let end = format!("{indent}end");
    (at + 1..lines.len()).find(|&later| lines[later].starts_with(&end))
}

/// Lines joined back into a world.
fn join_lines<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    lines.map(|line| format!("{line}\n")).collect()
}

/// A failing world, smaller, failing the same way.
///
/// Whole sections, then whole blocks, then runs of lines, halving the run
/// down to one line, each dropped where what is left still fails with
/// `signature_of_failure`; until nothing more can be.
fn shrink(world: &str, signature_of_failure: &str, name: &str) -> String {
    let path = out_dir().join(format!("{name}.candidate.world"));
    let runs = std::cell::Cell::new(0);
    let fails = |candidate: &str| {
        runs.set(runs.get() + 1);
        if runs.get() > MAX_SHRINK_RUNS {
            return false;
        }
        std::fs::write(&path, candidate).X();
        check_apart(&path).is_some_and(|e| signature(&e) == signature_of_failure)
    };
    let mut world = world.S();
    loop {
        let before = world.C();

        let mut parts = sections(&world);
        let mut at = parts.len();
        while at > 0 {
            at -= 1;
            let candidate: String = parts.iter().enumerate()
                .filter(|(i, _)| *i != at)
                .map(|(_, part)| part.as_str())
                .collect();
            if fails(&candidate) {
                parts.remove(at);
                world = candidate;
            }
        }

        let mut at = 0;
        loop {
            let lines: Vec<&str> = world.lines().collect();
            if at >= lines.len() {
                break;
            }
            let Some(end) = block_end(&lines, at) else {
                at += 1;
                continue;
            };
            let candidate = join_lines(lines[..at].iter().chain(&lines[end + 1..]).copied());
            if fails(&candidate) {
                world = candidate;
            } else {
                at += 1;
            }
        }

        let mut run = world.lines().count() / 2;
        while run >= 1 {
            let mut at = 0;
            loop {
                let lines: Vec<&str> = world.lines().collect();
                if at >= lines.len() {
                    break;
                }
                let dropped = at..(at + run).min(lines.len());
                let candidate = join_lines(lines.iter().enumerate()
                    .filter(|(i, line)| !dropped.contains(i) || kept_line(line))
                    .map(|(_, line)| *line));
                if candidate.lines().count() < lines.len() && fails(&candidate) {
                    world = candidate;
                } else {
                    at += run;
                }
            }
            run /= 2;
        }

        if world == before || runs.get() > MAX_SHRINK_RUNS {
            let _ = std::fs::remove_file(&path);
            return world;
        }
    }
}

// ============================================================================
// Running
// ============================================================================

fn main() {
    if std::env::var_os(WORKER).is_some() {
        work();
        return;
    }
    if let Some(path) = std::env::var_os("WORLDGEN_ENGINES_WORLD") {
        let source = std::fs::read_to_string(&path).expect("a readable worldfile");
        match check(&source) {
            Ok(_) => println!("{PASS} {}", path.display()),
            Err(e) => {
                println!("{FAIL} {}\n{e}", path.display());
                std::process::exit(1);
            }
        }
        return;
    }

    let seeds = env_seeds("WORLDGEN_ENGINES", DEFAULT_SEED, DEFAULT_COUNT);
    let end = seeds.start + seeds.count;
    let next = AtomicU64::new(seeds.start);
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let Checked { shown, tally, mut failures } = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| scope.spawn(|| check_in_worker(&next, end)))
            .collect();
        let mut all = Checked::default();
        for handle in handles {
            all.add(handle.join().expect("a thread running a worker"));
        }
        all
    });
    failures.sort_by_key(|(seed, _)| *seed);

    let shrink_them = std::env::var_os("WORLDGEN_ENGINES_NO_SHRINK").is_none();
    let reports: Vec<String> = failures.par_iter().enumerate()
        .map(|(i, (seed, error))| {
            let mut report = format!("{FAIL} seed {seed}: {error}\n");
            // Nothing to write out, nor shrink, where there is no world.
            let Ok(world) = generate(*seed) else {
                return report;
            };
            let path = write_out(&format!("seed-{seed}.world"), &world);
            report.push_str(&format!("  WORLDGEN_ENGINES_WORLD={}\n", path.display()));
            if shrink_them && i < MAX_SHRUNK {
                let shrunk = shrink(&world, &signature(error), &format!("seed-{seed}"));
                let path = write_out(&format!("seed-{seed}.shrunk.world"), &shrunk);
                report.push_str(&format!(
                    "  shrunk to {} lines of {}: WORLDGEN_ENGINES_WORLD={}\n",
                    shrunk.lines().count(), world.lines().count(), path.display(),
                ));
            }
            report
        })
        .collect();
    for report in &reports {
        print!("{report}");
    }
    if shrink_them && failures.len() > MAX_SHRUNK {
        println!("only the first {MAX_SHRUNK} failing worlds were shrunk");
    }

    let mut failed = !failures.is_empty();
    println!(
        "worldgen engines: {} of {} seeds from {} passed",
        seeds.count - failures.len() as u64, seeds.count, seeds.start,
    );
    shown.print();
    let floors = seeds.is_default();
    let checked = engines::report(&tally, floors).and_then(|()| match floors {
        true => shown.floors(&tally),
        false => Ok(()),
    });
    if let Err(e) = checked {
        println!("{FAIL} {e}");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
