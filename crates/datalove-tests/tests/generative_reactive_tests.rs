//! Generated script sessions, modified at random, each step held to a fresh
//! run.
//!
//! A seed makes a session with worldgen: modules, and a script cut into
//! units. The units are entered, then changed a dozen times -- edited,
//! deleted, reordered, or put back after a delete -- and after every change the
//! session must be what a new engine running its current code from the top
//! leaves: each unit come to the same end and hold the same bindings. That is
//! the definition of a correct step, so no table is written, and any seed is
//! a test.
//!
//! About a third of the changes aim at a unit that takes or writes an
//! earlier unit's binding, which a session entered once to look says which
//! are: a shadow of the binding is put between its provider and the writer,
//! by an insert, an edit or a reorder, or added to the writer itself, or the
//! writer is reordered above its provider. Those are the arrangements the reach has been wrong about, and
//! random changes make them rarely.
//!
//! A unit can also take the session with it, by a runtime error or a crash,
//! which ends the case: a fresh run must then reset too, the same way.
//!
//! A failing seed is shrunk -- changes, then units, then lines of source
//! dropped for as long as it fails the same way -- and written out as a
//! worldfile in the reactive fixtures' format, from which a step-table case
//! can be made. A seed that runs longer than [`SEED_TIMEOUT`] fails too, and
//! is written out unshrunk.
//!
//! A shrunk session that once failed and was fixed is kept in
//! `tests/generative_reactive/`, and every one of them is run before the
//! seeds, and must agree with a fresh run. Its units need not compile as
//! entered: shrinking keeps a session that fails a unit and fixes it.
//!
//! The run also says how much it exercised, and fails if that is too little
//! to mean anything: every unit must compile when first entered, enough
//! units must still run after the changes, and every kind of change must have
//! been made. Those floors are for a run of [`DEFAULT_COUNT`] seeds or more.
//!
//! Environment:
//!
//! - `GENERATIVE_REACTIVE_SEED`: the first seed, by default [`DEFAULT_SEED`].
//!   The default is fixed, so that a failure is a change and not a draw.
//! - `GENERATIVE_REACTIVE_COUNT`: how many seeds, by default
//!   [`DEFAULT_COUNT`]. `just test-generative` runs more.
//! - `GENERATIVE_REACTIVE_WORLD`: a written session to run, rather than any
//!   seed, as a failing seed prints.
//! - `GENERATIVE_REACTIVE_NO_SHRINK`: write failing seeds out unshrunk, for
//!   counting the seeds a planted fault fails, where shrinking each would
//!   take minutes.

use rmx::prelude::*;
use std::path::PathBuf;
use std::cell::RefCell;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use rmx::rand::SeedableRng;
use rmx::rand_pcg::Pcg64;

use datalove_repl::Place;
use datalove_script_steps::{Change, Entry, Ran, Session, Workspace, read_changes, write_changes};
use datalove_worldgen::{WorldGenConfig, gen_session_seeded};

const DEFAULT_SEED: u64 = 0;
const DEFAULT_COUNT: u64 = 20;

/// How many changes a session gets after its units are entered.
const CHANGES: usize = 12;

/// How long one seed may take, shrinking aside, before it is taken to hang.
/// A seed takes well under a second in a debug build.
const SEED_TIMEOUT: Duration = Duration::from_secs(120);

/// How long a case may take while shrinking. A candidate that takes longer
/// is not the failure being shrunk; shrinking can make a loop that never
/// ends, though it never takes out a `break`.
const SHRINK_TIMEOUT: Duration = Duration::from_secs(10);

/// The least share of units, over a run, that may still run after the
/// changes. About two in three do; most of the rest no longer compile
/// because a change took away something they use.
const MIN_STILL_RUNNING: f64 = 0.5;

// ============================================================================
// Generating
// ============================================================================

/// A session to check: the modules and data its units require, as worldfile
/// text, and its changes, the first of which enters its units.
#[derive(Clone)]
struct Case {
    library: String,
    changes: Vec<Change>,
    /// How many of the changes were aimed, by kind; see [`AIMED_KINDS`].
    /// None for a session read from a file.
    aimed: [usize; 4],
}

const AIMED_KINDS: [&str; 4] = [
    "shadows inserted", "shadows edited in", "shadows reordered in", "writers reordered above their providers",
];

/// A unit that takes or writes a binding an earlier unit provides, by
/// name.
struct Access {
    writer: String,
    provider: String,
    /// The binding's name.
    name: String,
}

/// What each unit, entered, takes or writes of earlier units' bindings;
/// nothing if entering the units panics or hangs, which checking the case
/// then reports.
fn accesses(library: &str, entries: &[Entry]) -> Vec<Access> {
    let library = library.S();
    let enter = Change::Enter(entries.to_vec());
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(CHECKING.S())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let found = std::panic::catch_unwind(|| {
                let workspace = Workspace::from_worldfile(&library);
                let mut session = Session::start(workspace.sys, workspace.descriptor, workspace.also_roots);
                session.apply(&enter);
                let mut found = Vec::new();
                if session.reset().is_some() {
                    return found;
                }
                for (index, writer) in session.units().iter().enumerate() {
                    if session.has_failed(writer) {
                        continue;
                    }
                    // Each says `<verb> `<name>` (<provider>)`.
                    for used in session.uses(index) {
                        if used.starts_with("reads") {
                            continue;
                        }
                        let (_, rest) = used.split_once('`').X();
                        let (name, rest) = rest.split_once('`').X();
                        let provider = rest.trim().trim_start_matches('(').trim_end_matches(')');
                        found.push(Access { writer: writer.C(), provider: provider.S(), name: name.S() });
                    }
                }
                found
            });
            send.send(found.unwrap_or_default()).ok();
        })
        .X();
    receive.recv_timeout(SEED_TIMEOUT).unwrap_or_default()
}

/// The units of a session as its changes leave them, by name, in order.
#[derive(Default)]
struct Order {
    units: Vec<String>,
}

impl Order {
    /// Make `change`, if it can be made: whether every unit it names is in
    /// the session, a new one is not, and a reorder changes the order.
    fn apply(&mut self, change: &Change) -> bool {
        let at = |units: &[String], unit: &str| units.iter().position(|name| name == unit);
        match change {
            Change::Enter(entries) => {
                for entry in entries {
                    if at(&self.units, &entry.unit).is_some() {
                        return false;
                    }
                    self.units.push(entry.unit.C());
                }
                true
            }
            Change::Edit { unit, .. } => at(&self.units, unit).is_some(),
            Change::Delete { unit } => {
                let Some(index) = at(&self.units, unit) else {
                    return false;
                };
                self.units.remove(index);
                true
            }
            Change::Insert { entry, place, beside } => {
                let Some(index) = at(&self.units, beside) else {
                    return false;
                };
                if at(&self.units, &entry.unit).is_some() {
                    return false;
                }
                self.units.insert(place.index(index), entry.unit.C());
                true
            }
            Change::Move { unit, place, beside } => {
                let Some(from) = at(&self.units, unit) else {
                    return false;
                };
                let name = self.units.remove(from);
                let Some(index) = at(&self.units, beside) else {
                    return false;
                };
                // The engine refuses to reorder a unit to where it is.
                let to = place.index(index);
                if to == from {
                    return false;
                }
                self.units.insert(to, name);
                true
            }
            Change::EditModule { .. } | Change::EditData { .. } => todo!("module and data edits"),
        }
    }

    /// Whether `changes` can be made in turn, the first entering units.
    fn admits(changes: &[Change]) -> bool {
        matches!(changes.first(), Some(Change::Enter(_)))
            && changes.iter().try_fold(Order::default(), |mut order, change| {
                order.apply(change).then_some(order)
            }).is_some()
    }
}

/// The session seed `seed` makes, and the changes it gets.
fn generate(seed: u64) -> Case {
    let mut config = WorldGenConfig::default();
    config.script_statements = (6, 14);
    config.binding_access_probability = 50;
    let session = gen_session_seeded(seed, config);
    let mut rng = Pcg64::seed_from_u64(seed);

    let entries: Vec<Entry> = session.units.iter().enumerate()
        .map(|(index, source)| Entry { unit: (index + 1).to_string(), source: source.C(), expression: false })
        .collect();
    // The units in session order, each with the source it was generated
    // with, which an edit starts from; and the deleted ones, for an insert
    // to put back.
    let mut live: Vec<(String, String)> = entries.iter().map(|entry| (entry.unit.C(), entry.source.C())).collect();
    let mut deleted: Vec<String> = Vec::new();
    let mut next = entries.len() + 1;

    let aims = accesses(&session.library, &entries);
    let mut aimed = [0; 4];

    let mut changes = vec![Change::Enter(entries)];
    while changes.len() <= CHANGES {
        if !aims.is_empty() && rng.random_range(0..3) == 0 {
            let access = &aims[rng.random_range(0..aims.len())];
            if let Some((kind, made)) = aim(&mut rng, &mut live, &mut next, &session.shadows, access) {
                aimed[kind] += 1;
                changes.extend(made);
                continue;
            }
        }
        let count = live.len();
        let pick = rng.random_range(0..count);
        let (unit, original) = live[pick].C();
        let change = match rng.random_range(0..10) {
            0..=3 => Change::Edit { unit, source: perturb(&mut rng, &original) },
            4 => Change::Edit { unit, source: original },
            5 => Change::Edit { unit, source: live[rng.random_range(0..count)].1.C() },
            6 if count > 1 => {
                live.remove(pick);
                deleted.push(original);
                Change::Delete { unit }
            }
            7 if count > 1 => {
                // Anywhere but where it is: an index among the units left
                // once it is taken out, other than its own.
                let entry = live.remove(pick);
                let to = (0..count).filter(|to| *to != pick).nth(rng.random_range(0..count - 1)).X();
                let (place, beside) = match to {
                    0 => (Place::Before, live[0].0.C()),
                    _ => (Place::After, live[to - 1].0.C()),
                };
                live.insert(to, entry);
                Change::Move { unit, place, beside }
            }
            8 | 9 if !deleted.is_empty() => {
                let source = deleted.remove(rng.random_range(0..deleted.len()));
                let name = next.to_string();
                next += 1;
                live.insert(pick + 1, (name.C(), source.C()));
                Change::Insert {
                    entry: Entry { unit: name, source, expression: false },
                    place: Place::After,
                    beside: unit,
                }
            }
            _ => Change::Edit { unit, source: perturb(&mut rng, &original) },
        };
        changes.push(change);
    }
    assert!(Order::admits(&changes), "seed {seed} makes changes that cannot be made");
    Case { library: session.library, changes, aimed }
}

/// Changes aimed at `access`, and which kind of [`AIMED_KINDS`] they are;
/// `None` if its writer or provider is gone or out of order.
///
/// `live` is the units in session order, with the source each was
/// generated with, and `next` the number the next unit takes.
fn aim(
    rng: &mut Pcg64,
    live: &mut Vec<(String, String)>,
    next: &mut usize,
    shadows: &[(String, String)],
    access: &Access,
) -> Option<(usize, Vec<Change>)> {
    let at = |live: &[(String, String)], unit: &str| live.iter().position(|(name, _)| name == unit);
    let provider = at(live, &access.provider)?;
    let writer = at(live, &access.writer)?;
    if provider >= writer {
        return None;
    }
    let shadow = &shadows.iter().find(|(name, _)| *name == access.name)?.1;
    let mut new_unit = |live: &mut Vec<(String, String)>, after: usize| {
        let unit = next.to_string();
        *next += 1;
        live.insert(after + 1, (unit.C(), shadow.C()));
        let entry = Entry { unit, source: shadow.C(), expression: false };
        Change::Insert { entry, place: Place::After, beside: live[after].0.C() }
    };
    let made = match rng.random_range(0..4) {
        0 => {
            let after = rng.random_range(provider..writer);
            (0, vec![new_unit(live, after)])
        }
        // A unit after the provider, up to the writer itself, which then
        // binds again a name it takes or writes.
        1 => {
            let (unit, original) = live[rng.random_range(provider + 1..=writer)].C();
            (1, vec![Change::Edit { unit, source: fmt!("{original}\n{shadow}") }])
        }
        2 => {
            let last = live.len() - 1;
            let insert = new_unit(live, last);
            let entry = live.pop().X();
            let unit = entry.0.C();
            let after = rng.random_range(provider..writer);
            let beside = live[after].0.C();
            live.insert(after + 1, entry);
            (2, vec![insert, Change::Move { unit, place: Place::After, beside }])
        }
        _ => {
            let entry = live.remove(writer);
            let unit = entry.0.C();
            let beside = live[provider].0.C();
            live.insert(provider, entry);
            (3, vec![Change::Move { unit, place: Place::Before, beside }])
        }
    };
    Some(made)
}

/// `source` with each integer literal, by chance, replaced by a small other
/// one, so that an edit changes values and keeps types.
fn perturb(rng: &mut Pcg64, source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // A digit after a letter is part of a name, like `v12`.
        let in_name = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if !chars[i].is_ascii_digit() || in_name {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        match rng.random_bool(0.5) {
            true => out.push_str(&rng.random_range(0..20).to_string()),
            false => out.extend(&chars[start..i]),
        }
    }
    out
}

// ============================================================================
// Checking
// ============================================================================

/// How a case came out.
enum Verdict {
    Agrees(Tally),
    /// The ways some step left the session other than a fresh run would.
    Disagrees(Vec<String>),
    Panics { at: String, message: String },
    Hangs,
}

impl Verdict {
    /// Whether `self` is the failure `other` is, for shrinking: the same
    /// panic, by where it was raised, or a disagreement of any kind.
    fn same_failure(&self, other: &Verdict) -> bool {
        match (self, other) {
            (Verdict::Disagrees(_), Verdict::Disagrees(_)) => true,
            (Verdict::Panics { at, .. }, Verdict::Panics { at: other, .. }) => at == other,
            _ => false,
        }
    }

    fn describe(&self) -> String {
        match self {
            Verdict::Agrees(tally) if tally.resets > 0 => "agrees with a fresh run, both resetting".S(),
            Verdict::Agrees(_) => "agrees with a fresh run".S(),
            Verdict::Disagrees(failures) => failures.join("\n"),
            Verdict::Panics { at, message } => fmt!("panicked at {at}: {message}"),
            Verdict::Hangs => fmt!("ran longer than {SEED_TIMEOUT:?}"),
        }
    }
}

/// What a run exercised.
#[derive(Default)]
struct Tally {
    /// Units entered, and of those, the ones that did not compile, by name.
    entered: usize,
    failed_on_entry: Vec<String>,
    /// Units in the sessions after their changes, and of those, the ones
    /// that ran to their end.
    remaining: usize,
    still_running: usize,
    /// Cases that ended in a unit taking the session with it.
    resets: usize,
    /// Changes made, by kind: edits, deletes, reorders, inserts.
    changes: [usize; 4],
    /// Of those, the aimed ones, by kind; see [`AIMED_KINDS`].
    aimed: [usize; 4],
    /// What the units do to earlier units' bindings as they are entered, by
    /// verb: reads, takes, overwrites, updates, revives.
    uses: [usize; 5],
}

const CHANGE_KINDS: [&str; 4] = ["edits", "deletes", "reorders", "inserts"];
const VERBS: [&str; 5] = ["reads", "takes", "overwrites", "updates", "revives"];

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.entered += other.entered;
        self.failed_on_entry.extend(other.failed_on_entry.iter().cloned());
        self.remaining += other.remaining;
        self.still_running += other.still_running;
        self.resets += other.resets;
        for (sum, count) in self.changes.iter_mut().zip(other.changes) {
            *sum += count;
        }
        for (sum, count) in self.aimed.iter_mut().zip(other.aimed) {
            *sum += count;
        }
        for (sum, count) in self.uses.iter_mut().zip(other.uses) {
            *sum += count;
        }
    }

    /// Every way this run exercised too little to mean anything.
    fn shortfalls(&self) -> Vec<String> {
        let mut shortfalls = Vec::new();
        let share = self.still_running as f64 / self.remaining.max(1) as f64;
        if share < MIN_STILL_RUNNING {
            shortfalls.push(fmt!(
                "{} of {} units still run after the changes, fewer than {MIN_STILL_RUNNING}",
                self.still_running, self.remaining,
            ));
        }
        for (kind, count) in CHANGE_KINDS.iter().zip(self.changes).chain(AIMED_KINDS.iter().zip(self.aimed)) {
            if count == 0 {
                shortfalls.push(fmt!("{kind}: none"));
            }
        }
        for (verb, count) in VERBS.iter().zip(self.uses).skip(1) {
            if count == 0 {
                shortfalls.push(fmt!("no unit {verb} an earlier unit's binding"));
            }
        }
        shortfalls
    }

    fn describe(&self) -> String {
        let changes: Vec<String> = CHANGE_KINDS.iter().zip(self.changes)
            .map(|(kind, count)| fmt!("{count} {kind}"))
            .collect();
        let aimed: Vec<String> = AIMED_KINDS.iter().zip(self.aimed)
            .map(|(kind, count)| fmt!("{count} {kind}"))
            .collect();
        let uses: Vec<String> = VERBS.iter().zip(self.uses)
            .map(|(verb, count)| fmt!("{count} {verb}"))
            .collect();
        fmt!(
            "{} units entered, {} still run of {} after {} ({}), {} sessions reset; as entered, units use earlier units' bindings: {}",
            self.entered, self.still_running, self.remaining, changes.join(", "), aimed.join(", "), self.resets, uses.join(", "),
        )
    }
}

/// The name of the threads [`check`] runs cases on.
const CHECKING: &str = "check";

thread_local! {
    /// Where the last panic on this thread was raised, and what it said.
    static LAST_PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Keep each panic on a thread checking a case for the verdict to report,
/// rather than printing it: a shrink raises hundreds. Any other panic is
/// printed as usual.
fn catch_panics() {
    let usual = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().name() != Some(CHECKING) {
            return usual(info);
        }
        let at = info.location().map_or("an unknown place".S(), |location| location.to_string());
        LAST_PANIC.set(Some((at, payload_message(info.payload()))));
    }));
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    match payload.downcast_ref::<&str>() {
        Some(message) => message.S(),
        None => payload.downcast_ref::<String>().cloned().unwrap_or_default(),
    }
}

/// Make a case's changes in turn, on a thread of its own so that a hang is
/// reported rather than waited on. A thread that hangs is left running.
fn check(case: &Case, timeout: Duration) -> Verdict {
    let case = case.C();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(CHECKING.S())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            LAST_PANIC.set(None);
            let verdict = match std::panic::catch_unwind(|| run(&case)) {
                Ok(verdict) => verdict,
                // A panic raised on another thread and carried here, as
                // rayon carries one, did not pass this thread's hook.
                Err(payload) => match LAST_PANIC.take() {
                    Some((at, message)) => Verdict::Panics { at, message },
                    None => Verdict::Panics { at: "another thread".S(), message: payload_message(&*payload) },
                },
            };
            // The receiver is gone when this thread outlived its timeout.
            send.send(verdict).ok();
        })
        .X();
    match receive.recv_timeout(timeout) {
        Ok(verdict) => verdict,
        Err(RecvTimeoutError::Timeout) => Verdict::Hangs,
        Err(RecvTimeoutError::Disconnected) => panic!("the thread checking a case ended without a verdict"),
    }
}

fn run(case: &Case) -> Verdict {
    let workspace = Workspace::from_worldfile(&case.library);
    let mut session = Session::start(workspace.sys, workspace.descriptor, workspace.also_roots);
    let mut tally = Tally { aimed: case.aimed, ..Tally::default() };
    let mut failures = Vec::new();
    for (number, change) in case.changes.iter().enumerate() {
        let touched = session.apply(change);
        match change {
            Change::Enter(_) => {
                tally.entered += touched.len();
                tally.failed_on_entry.extend(touched.iter()
                    .filter(|(_, outcome)| outcome.ran == Ran::Fails)
                    .map(|(unit, _)| unit.C()));
                if session.reset().is_none() {
                    for index in 0..session.units().len() {
                        for used in session.uses(index) {
                            let verb = VERBS.iter().position(|verb| used.starts_with(verb)).X();
                            tally.uses[verb] += 1;
                        }
                    }
                }
            }
            Change::Edit { .. } => tally.changes[0] += 1,
            Change::Delete { .. } => tally.changes[1] += 1,
            Change::Move { .. } => tally.changes[2] += 1,
            Change::Insert { .. } => tally.changes[3] += 1,
            Change::EditModule { .. } | Change::EditData { .. } => todo!("module and data edits"),
        }
        failures.extend(session.differs_from_fresh(&fmt!("step {}", number + 1)));
        if session.reset().is_some() {
            break;
        }
    }
    if !failures.is_empty() {
        return Verdict::Disagrees(failures);
    }
    if session.reset().is_some() {
        tally.resets = 1;
        return Verdict::Agrees(tally);
    }

    tally.remaining = session.states().len();
    tally.still_running = session.states().iter().filter(|ran| **ran == Ran::Runs).count();
    Verdict::Agrees(tally)
}

// ============================================================================
// Shrinking
// ============================================================================

/// The smallest case found that fails as `verdict` says `case` does: with
/// changes, then units, then lines of source taken out, for as long as one
/// can be.
fn shrink(case: Case, verdict: &Verdict) -> Case {
    let mut case = case;
    let fails = |candidate: &Case| {
        Order::admits(&candidate.changes) && check(candidate, SHRINK_TIMEOUT).same_failure(verdict)
    };
    loop {
        let mut smaller = false;

        // Each change after the units are entered, the last first.
        for index in (1..case.changes.len()).rev() {
            let mut candidate = case.C();
            candidate.changes.remove(index);
            if fails(&candidate) {
                case = candidate;
                smaller = true;
            }
        }

        // Each unit entered.
        let entered = match &case.changes[0] {
            Change::Enter(entries) => entries.len(),
            _ => unreachable!("the first change enters the units"),
        };
        for index in (0..entered).rev() {
            let mut candidate = case.C();
            let Change::Enter(entries) = &mut candidate.changes[0] else { unreachable!() };
            if entries.len() == 1 {
                continue;
            }
            entries.remove(index);
            if fails(&candidate) {
                case = candidate;
                smaller = true;
            }
        }

        // Each line of each source, but a `break`, without which the loop it
        // ends may never end.
        for at in sources(&case.changes) {
            let lines = source(&mut case.C().changes, at).lines().count();
            for line in (0..lines).rev() {
                let mut candidate = case.C();
                let text = source(&mut candidate.changes, at);
                if text.lines().nth(line).X().trim() == "break" {
                    continue;
                }
                let kept: Vec<&str> = text.lines().enumerate()
                    .filter(|(index, _)| *index != line)
                    .map(|(_, text)| text)
                    .collect();
                *text = kept.join("\n");
                if fails(&candidate) {
                    case = candidate;
                    smaller = true;
                }
            }
        }

        if !smaller {
            return case;
        }
    }
}

/// Where a source is among a case's changes: a change, and an entry of it
/// when it enters several.
type SourceAt = (usize, Option<usize>);

fn sources(changes: &[Change]) -> Vec<SourceAt> {
    let mut found = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        match change {
            Change::Enter(entries) => found.extend((0..entries.len()).map(|entry| (index, Some(entry)))),
            Change::Edit { .. } | Change::Insert { .. } => found.push((index, None)),
            _ => {}
        }
    }
    found
}

fn source(changes: &mut [Change], (index, entry): SourceAt) -> &mut String {
    match (&mut changes[index], entry) {
        (Change::Enter(entries), Some(entry)) => &mut entries[entry].source,
        (Change::Edit { source, .. }, None) => source,
        (Change::Insert { entry, .. }, None) => &mut entry.source,
        _ => unreachable!("`sources` says where the sources are"),
    }
}

// ============================================================================
// Running
// ============================================================================

fn env_u64(name: &str, default: u64) -> u64 {
    match std::env::var(name) {
        Ok(text) => text.parse().unwrap_or_else(|_| panic!("{name} must be a u64")),
        Err(_) => default,
    }
}

/// The kept sessions, by file name, in name order.
fn corpus() -> Vec<(String, Case)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/generative_reactive");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir).X()
        .map(|entry| entry.X().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "world"))
        .collect();
    paths.sort();
    paths.into_iter()
        .map(|path| {
            let name = path.file_name().X().to_string_lossy().S();
            let (library, changes) = read_changes(&name, &std::fs::read_to_string(&path).X());
            (name, Case { library, changes, aimed: [0; 4] })
        })
        .collect()
}

/// Write a failing case where it can be run again, and say how.
fn write_out(seed: u64, case: &Case) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("generative-reactive");
    std::fs::create_dir_all(&dir).X();
    let path = dir.join(fmt!("seed-{seed}.world"));
    std::fs::write(&path, write_changes(&case.library, &case.changes)).X();
    path
}

fn main() {
    catch_panics();

    if let Ok(path) = std::env::var("GENERATIVE_REACTIVE_WORLD") {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
        let (library, changes) = read_changes(&path, &text);
        let verdict = check(&Case { library, changes, aimed: [0; 4] }, SEED_TIMEOUT);
        println!("{path}: {}", verdict.describe());
        if let Verdict::Agrees(tally) = &verdict && !tally.failed_on_entry.is_empty() {
            println!("units that do not compile when entered: {}", tally.failed_on_entry.join(", "));
        }
        if !matches!(verdict, Verdict::Agrees(_)) {
            std::process::exit(1);
        }
        return;
    }

    let mut broken = Vec::new();
    for (name, case) in corpus() {
        let verdict = check(&case, SEED_TIMEOUT);
        if !matches!(verdict, Verdict::Agrees(_)) {
            println!("{name}: \x1b[31mFAIL\x1b[0m\n  {}", verdict.describe().replace('\n', "\n  "));
            broken.push(name);
        }
    }

    let start = env_u64("GENERATIVE_REACTIVE_SEED", DEFAULT_SEED);
    let count = env_u64("GENERATIVE_REACTIVE_COUNT", DEFAULT_COUNT);
    println!("generative reactive tests: seeds {start} to {}", start + count.max(1) - 1);

    let mut total = Tally::default();
    let mut failed = Vec::new();
    for seed in start..start + count {
        let case = generate(seed);
        // A failing case is written out to be read back, so every case is.
        let written = write_changes(&case.library, &case.changes);
        let (library, changes) = read_changes(&fmt!("seed {seed}"), &written);
        assert_eq!(written, write_changes(&library, &changes), "seed {seed} reads back as it was written");
        let verdict = check(&case, SEED_TIMEOUT);
        let Verdict::Agrees(tally) = &verdict else {
            println!("seed {seed}: \x1b[31mFAIL\x1b[0m\n  {}", verdict.describe().replace('\n', "\n  "));
            let shrink_it = std::env::var("GENERATIVE_REACTIVE_NO_SHRINK").is_err();
            let case = match verdict {
                Verdict::Hangs => case,
                _ if !shrink_it => case,
                _ => {
                    println!("  shrinking...");
                    shrink(case, &verdict)
                }
            };
            let path = write_out(seed, &case);
            println!("  the session is {}", path.display());
            println!("  run it with GENERATIVE_REACTIVE_WORLD={} cargo test -p datalove-tests --test generative_reactive_tests", path.display());
            failed.push(seed);
            continue;
        };
        if !tally.failed_on_entry.is_empty() {
            let path = write_out(seed, &case);
            println!(
                "seed {seed}: \x1b[31mFAIL\x1b[0m\n  units {} of {} do not compile as generated, which is a worldgen bug; the session is {}",
                tally.failed_on_entry.join(", "), tally.entered, path.display(),
            );
            failed.push(seed);
            continue;
        }
        total.add(tally);
    }

    println!("{}", total.describe());
    let shortfalls = match count >= DEFAULT_COUNT {
        true => total.shortfalls(),
        false => Vec::new(),
    };
    for shortfall in &shortfalls {
        println!("\x1b[31mtoo little exercised\x1b[0m: {shortfall}");
    }
    if !broken.is_empty() {
        println!("failing kept sessions: {}", broken.join(", "));
    }
    if !failed.is_empty() {
        println!("failing seeds: {failed:?}");
    }
    if !broken.is_empty() || !failed.is_empty() || !shortfalls.is_empty() {
        std::process::exit(1);
    }
}
