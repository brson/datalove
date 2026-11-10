//! Generated script sessions, modified at random, each step held to a fresh
//! run.
//!
//! A seed makes a session with worldgen: modules, and a script cut into
//! units, some of them expressions or declaring functions that later units
//! call, and data files some of them require. The units are entered, then changed a dozen times -- edited,
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
//! writer is reordered above its provider. Those are the arrangements the
//! reach has been wrong about, and random changes make them rarely. Other
//! changes set a divisor worldgen made to 0, so that a unit dividing by it
//! returns early and the units reading what that unit binds are blocked.
//! Others revive a `var` a unit takes, by a unit put right after it, or put
//! a unit reading a quotient right after the unit that divides, and set the
//! divisor to 0, so that the new unit is blocked.
//! And some edit a module the units import from, or a data file they
//! require: a number in it changed, or put back as generated, or, for a
//! module, a function renamed or one that does not compile added, and for
//! data, text of another type or text that does not parse.
//!
//! Each seed also makes a dense session, with no worldgen: a few units over
//! two names, `x` and `y`, each a line or two of a dozen kinds -- bind, take,
//! clone, read, set, update, divide, declare and call a function, import --
//! and changes made among them alone; see [`dense`]. A unit there may name
//! what nothing binds yet, and fail when entered.
//!
//! A unit can also take the session with it, by a runtime error, which ends
//! the case: a fresh run must then reset too, the same way. Or by a crash, the
//! engine panicking, which fails the case even when a fresh run crashes too.
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
//! been made. Those floors are for the default seeds alone.
//!
//! Environment:
//!
//! - `GENERATIVE_REACTIVE_SEED`: the first seed, by default [`DEFAULT_SEED`],
//!   or `random`. The default is fixed, so that a failure is a change and not
//!   a draw.
//! - `GENERATIVE_REACTIVE_COUNT`: how many seeds, by default
//!   [`DEFAULT_COUNT`]. `just test-generative-reactive` runs more.
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

use bct::random::{SeededRng, env_seeds};

use datalove_repl::Place;
use datalove_script_engine::Eval;
use datalove_script_steps::{Change, Difference, Entry, Ran, Session, Workspace, read_changes, write_changes};
use datalove_worldgen::{WorldGenConfig, gen_session_seeded};

/// The first seed of a default run.
///
/// The floors are not met by every run of [`DEFAULT_COUNT`] seeds, revives
/// and blocked units being rare, so this is a start that meets them. It
/// moves when the generator or its random numbers change.
const DEFAULT_SEED: u64 = 60;
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
    aimed: [usize; 7],
}

const AIMED_KINDS: [&str; 7] = [
    "shadows inserted", "shadows edited in", "shadows reordered in", "writers reordered above their providers",
    "divisors zeroed", "revives inserted after a take", "quotients read and their divisors zeroed",
];

/// `source` with every divisor worldgen declared or set set to 0, or `None`
/// if it has none: a top-level `var dN: int = <digits>` or `set dN = <digits>`.
fn zero_divisors(source: &str) -> Option<String> {
    let zeroed = |line: &str| {
        let rest = line.strip_prefix("var d").or_else(|| line.strip_prefix("set d"))?;
        let (name, value) = rest.split_once(" = ")?;
        let name_ok = name.strip_suffix(": int").unwrap_or(name).chars().all(|c| c.is_ascii_digit());
        (name_ok && !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) && value != "0")
            .then(|| fmt!("{}0", &line[..line.len() - value.len()]))
    };
    let mut any = false;
    let lines: Vec<String> = source.lines()
        .map(|line| match zeroed(line) {
            Some(zero) => {
                any = true;
                zero
            }
            None => line.S(),
        })
        .collect();
    any.then(|| lines.join("\n"))
}

/// A unit that takes or writes a binding an earlier unit provides, by
/// name.
struct Access {
    writer: String,
    provider: String,
    /// The binding's name.
    name: String,
    /// What the writer does to it: takes, overwrites, updates or revives.
    verb: String,
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
                        let (verb, rest) = used.split_once('`').X();
                        let (name, rest) = rest.split_once('`').X();
                        let provider = rest.trim().trim_start_matches('(').trim_end_matches(')');
                        found.push(Access {
                            writer: writer.C(), provider: provider.S(), name: name.S(), verb: verb.trim().S(),
                        });
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
            Change::EditModule { .. } | Change::EditData { .. } => true,
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
    config.expression_unit_probability = 15;
    let session = gen_session_seeded(seed, config);
    let mut rng = SeededRng::derive(seed, "changes");

    let entries: Vec<Entry> = session.units.iter().enumerate()
        .map(|(index, unit)| Entry { unit: (index + 1).S(), source: unit.source.C(), expression: unit.expression })
        .collect();
    // The units in session order, each with the source it was generated
    // with, which an edit starts from; and the deleted ones, for an insert
    // to put back, as entries. The expression units, by name, keep their
    // kind through all of it.
    let mut live: Vec<(String, String)> = entries.iter().map(|entry| (entry.unit.C(), entry.source.C())).collect();
    let mut expressions: Vec<String> = entries.iter().filter(|entry| entry.expression).map(|entry| entry.unit.C()).collect();
    let mut deleted: Vec<Entry> = Vec::new();
    let mut next = entries.len() + 1;

    let aims = accesses(&session.library, &entries);
    let mut aimed = [0; 7];

    let mut changes = vec![Change::Enter(entries)];
    while changes.len() <= CHANGES {
        if rng.random_range(0..10) == 0 {
            let (path, original) = session.modules[rng.random_range(0..session.modules.len())].C();
            changes.push(Change::EditModule { path, source: edit_module(&mut rng, &original) });
            continue;
        }
        if !session.data.is_empty() && rng.random_range(0..8) == 0 {
            let (path, original) = session.data[rng.random_range(0..session.data.len())].C();
            changes.push(Change::EditData { path, text: edit_data(&mut rng, &original) });
            continue;
        }
        if rng.random_range(0..8) == 0 && let Some(made) = block(&mut live, &mut next) {
            aimed[6] += 1;
            changes.extend(made);
            continue;
        }
        if rng.random_range(0..6) == 0 {
            let zeroable: Vec<(String, String)> = live.iter()
                .filter_map(|(unit, original)| Some((unit.C(), zero_divisors(original)?)))
                .collect();
            if !zeroable.is_empty() {
                let (unit, source) = zeroable[rng.random_range(0..zeroable.len())].C();
                aimed[4] += 1;
                changes.push(Change::Edit { unit, source });
                continue;
            }
        }
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
            // Another unit's source, of the same kind.
            5 => {
                let kind = expressions.contains(&unit);
                let alike: Vec<&(String, String)> = live.iter()
                    .filter(|(other, _)| expressions.contains(other) == kind)
                    .collect();
                Change::Edit { unit, source: alike[rng.random_range(0..alike.len())].1.C() }
            }
            6 if count > 1 => {
                live.remove(pick);
                deleted.push(Entry { unit: unit.C(), source: original, expression: expressions.contains(&unit) });
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
                let gone = deleted.remove(rng.random_range(0..deleted.len()));
                let name = next.S();
                next += 1;
                live.insert(pick + 1, (name.C(), gone.source.C()));
                if gone.expression {
                    expressions.push(name.C());
                }
                Change::Insert {
                    entry: Entry { unit: name, source: gone.source, expression: gone.expression },
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

// ============================================================================
// Dense sessions
// ============================================================================

/// The module a dense session's units may require and import from.
const DENSE_LIBRARY: &str = "----------\nmodule local/dense/m\n----------\n\nfun g(): int\n  ret 7\nend fun\n";

/// The names a dense session's units bind, read, take and write.
const DENSE_NAMES: [&str; 2] = ["x", "y"];

/// What a dense session's units are made of, so far: the temporaries and the
/// functions bound, which later lines may name.
#[derive(Default)]
struct DenseNames {
    temps: usize,
    functions: usize,
    /// Functions that recurse without end, named apart from `functions` so
    /// that no other line calls one.
    runaways: usize,
}

impl DenseNames {
    fn temp(&mut self) -> String {
        self.temps += 1;
        fmt!("t{}", self.temps - 1)
    }

    /// A temporary bound so far, or a binding name if none is.
    fn some_temp(&self, rng: &mut SeededRng) -> String {
        match self.temps {
            0 => DENSE_NAMES[rng.random_range(0..DENSE_NAMES.len())].S(),
            temps => fmt!("t{}", rng.random_range(0..temps)),
        }
    }

    /// A number, now and then 0, which makes a divisor return early.
    fn number(rng: &mut SeededRng) -> u32 {
        match rng.random_range(0..4) {
            0 => 0,
            _ => rng.random_range(1..200),
        }
    }

    /// One line of a fragment unit, or a few for a block or a function.
    fn line(&mut self, rng: &mut SeededRng) -> String {
        let name = DENSE_NAMES[rng.random_range(0..DENSE_NAMES.len())];
        let k = DenseNames::number(rng);
        let other = DENSE_NAMES.iter().find(|other| **other != name).X();
        match rng.random_range(0..27) {
            0..=3 => fmt!("var {name}: int = {k}"),
            4 => fmt!("let {name}: int = {k}"),
            5..=7 => fmt!("let {}: int = {name}", self.temp()),
            8 => fmt!("let {}: int = {name}@", self.temp()),
            9 => fmt!("debuglog {name}"),
            10 | 11 => fmt!("set {name} = {k}"),
            12 => fmt!("if true xor false\n  set {name} = {k}\nend if"),
            13 => fmt!("set {name} = {name}@ + {k}"),
            14 | 15 => fmt!("let {}: int = {} /! {name}", self.temp(), rng.random_range(1..200)),
            16 => {
                let read = self.some_temp(rng);
                fmt!("let {}: int = {read}", self.temp())
            }
            17 => {
                self.functions += 1;
                fmt!("fun f{}(): int\n  ret {k}\nend fun", self.functions - 1)
            }
            18 if self.functions > 0 => {
                let called = rng.random_range(0..self.functions);
                fmt!("let {}: int = f{called}()", self.temp())
            }
            19 => match rng.random_range(0..3) {
                0 => "require module local/dense/m".S(),
                1 => "import m.g".S(),
                _ => fmt!("let {}: int = g()", self.temp()),
            },
            // Bind a name and divide by it, so that the unit returns early,
            // not providing it, when it is 0, and the units after that read
            // it are blocked.
            20 | 21 => fmt!("var {name}: int = {k}\nlet {}: int = {} /! {name}", self.temp(), rng.random_range(1..200)),
            // Write one name from the other, which a unit blocked on the
            // other would not do.
            22 | 23 => fmt!("set {name} = {other}@ + {k}"),
            24 => fmt!("var {name}: int = {other}@"),
            // Overflow the stack when a name holds 0, a runtime error, which
            // empties the session: an edit to the name turns it on or off,
            // and the fresh run must reset as the session does.
            25 => {
                self.runaways += 1;
                let runaway = fmt!("down{}", self.runaways - 1);
                fmt!(
                    "fun {runaway}(n: int): int\n  ret {runaway}(n) + 1\nend fun\nif {name} == 0\n  let {}: int = {runaway}(0)\nend if",
                    self.temp(),
                )
            }
            _ => fmt!("if true xor false\n  set {name} = {k}\nend if"),
        }
    }

    /// A fragment unit's source: one to three lines, now and then none.
    fn fragment(&mut self, rng: &mut SeededRng) -> String {
        let lines = match rng.random_range(0..10) {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            _ => 3,
        };
        (0..lines).map(|_| self.line(rng)).collect::<Vec<String>>().join("\n")
    }

    /// An expression unit's source.
    fn expression(&mut self, rng: &mut SeededRng) -> String {
        let name = DENSE_NAMES[rng.random_range(0..DENSE_NAMES.len())];
        match rng.random_range(0..5) {
            0 => name.S(),
            1 => fmt!("{name}@"),
            2 => self.some_temp(rng),
            3 if self.functions > 0 => fmt!("f{}()", rng.random_range(0..self.functions)),
            _ => "g()".S(),
        }
    }

    fn source(&mut self, rng: &mut SeededRng, expression: bool) -> String {
        match expression {
            true => self.expression(rng),
            false => self.fragment(rng),
        }
    }
}

/// A dense session: a few units over two bindings, made of a dozen kinds of
/// line, and changes made among them alone.
///
/// A generated session is large and its units each compile as entered, so
/// that its changes reach far; and so a particular arrangement around one
/// binding -- a unit taking what a unit that fails ownership binds, a reader
/// after a unit that may be blocked, a shadow moved past a take -- turns up
/// in a few seeds in a thousand. Here every unit is about the same two names,
/// a unit may name what no unit binds yet, and a change may make a unit fail
/// or fix it, so those arrangements are common.
fn dense(seed: u64) -> Case {
    let mut rng = SeededRng::derive(seed, "dense");
    let mut names = DenseNames::default();
    // Most sessions start by binding the names, which the rest then use.
    let mut sources: Vec<(String, bool)> = Vec::new();
    for name in DENSE_NAMES {
        if rng.random_range(0..4) != 0 {
            sources.push((fmt!("var {name}: int = {}", DenseNames::number(&mut rng)), false));
        }
    }
    for _ in 0..rng.random_range(3..7) {
        let expression = rng.random_range(0..5) == 0;
        sources.push((names.source(&mut rng, expression), expression));
    }
    let entries: Vec<Entry> = sources.into_iter().enumerate()
        .map(|(index, (source, expression))| Entry { unit: (index + 1).S(), source, expression })
        .collect();
    let mut live: Vec<(String, String, bool)> = entries.iter()
        .map(|entry| (entry.unit.C(), entry.source.C(), entry.expression))
        .collect();
    let mut next = entries.len() + 1;

    let mut changes = vec![Change::Enter(entries)];
    while changes.len() <= CHANGES {
        let count = live.len();
        let pick = rng.random_range(0..count);
        let (unit, _, expression) = live[pick].C();
        let change = match rng.random_range(0..10) {
            0..=2 => {
                let source = names.source(&mut rng, expression);
                live[pick].1 = source.C();
                Change::Edit { unit, source }
            }
            // Another unit's source, of the same kind, which binds or takes
            // again what that one does.
            3 => {
                let alike: Vec<String> = live.iter()
                    .filter(|other| other.2 == expression)
                    .map(|other| other.1.C())
                    .collect();
                let source = alike[rng.random_range(0..alike.len())].C();
                live[pick].1 = source.C();
                Change::Edit { unit, source }
            }
            4 if count > 1 => {
                live.remove(pick);
                Change::Delete { unit }
            }
            5 | 6 if count > 1 => {
                let entry = live.remove(pick);
                let to = (0..count).filter(|to| *to != pick).nth(rng.random_range(0..count - 1)).X();
                let (place, beside) = match to {
                    0 => (Place::Before, live[0].0.C()),
                    _ => (Place::After, live[to - 1].0.C()),
                };
                live.insert(to, entry);
                Change::Move { unit, place, beside }
            }
            _ => {
                let expression = rng.random_range(0..5) == 0;
                let source = names.source(&mut rng, expression);
                let name = next.S();
                next += 1;
                // A worldfile inserts after a unit, so never first; a move
                // puts a unit first.
                let to = rng.random_range(1..=count);
                let beside = live[to - 1].0.C();
                live.insert(to, (name.C(), source.C(), expression));
                Change::Insert { entry: Entry { unit: name, source, expression }, place: Place::After, beside }
            }
        };
        changes.push(change);
    }
    assert!(Order::admits(&changes), "seed {seed} makes dense changes that cannot be made");
    Case { library: DENSE_LIBRARY.S(), changes, aimed: [0; 7] }
}

/// A generated module's source edited: a number changed, as generated, a
/// function renamed, which its importers then do not find, or a function
/// that does not compile added, which fails the module.
fn edit_module(rng: &mut SeededRng, original: &str) -> String {
    match rng.random_range(0..6) {
        0..=1 => perturb(rng, original),
        2 => original.S(),
        3 => {
            let declared: Vec<usize> = original.match_indices("fun ").map(|(at, _)| at + "fun ".len())
                .filter(|at| original[..*at - "fun ".len()].ends_with('\n') || *at == "fun ".len())
                .collect();
            assert!(!declared.is_empty(), "worldgen declares a function in every module");
            let at = declared[rng.random_range(0..declared.len())];
            let end = original[at..].find(|c: char| !c.is_alphanumeric() && c != '_').map_or(original.len(), |len| at + len);
            fmt!("{}_gone{}", &original[..end], &original[end..])
        }
        _ => fmt!("{original}\n\nfun broken(): int\n  ret \"no\"\nend fun"),
    }
}

/// A generated data file's text edited: a number changed, as generated,
/// text of another type, or text that does not parse.
fn edit_data(rng: &mut SeededRng, original: &str) -> String {
    match rng.random_range(0..5) {
        0..=1 => perturb(rng, original),
        2 => original.S(),
        3 if original.trim_start().starts_with('"') => "5".S(),
        3 => "\"text\"".S(),
        _ => "(".S(),
    }
}

/// A unit inserted right after one that divides by a divisor, reading the
/// quotient, and the divisor set to 0 where it is declared: the quotient's
/// unit returns early and the new unit is blocked. `None` if no live unit
/// divides by a divisor a live unit declares.
///
/// A unit is only blocked by reading a quotient whose unit returned early,
/// which the units as generated arrange too rarely to count on.
fn block(live: &mut Vec<(String, String)>, next: &mut usize) -> Option<Vec<Change>> {
    // `let vN: int = K /! dM`, as worldgen writes a quotient.
    let quotient = |line: &str| {
        let rest = line.strip_prefix("let ")?;
        let (name, rest) = rest.split_once(": int = ")?;
        let (_, divisor) = rest.split_once(" /! ")?;
        Some((name.S(), divisor.trim().S()))
    };
    let (at, name, divisor) = live.iter().enumerate()
        .find_map(|(at, (_, source))| source.lines().find_map(quotient).map(|(name, divisor)| (at, name, divisor)))?;
    let declared = live.iter().position(|(_, source)| {
        source.lines().any(|line| line.starts_with(&fmt!("var {divisor}: int = ")))
    })?;
    let (declarer, original) = live[declared].C();
    let zeroed = zero_divisors(&original)?;

    let unit = next.S();
    *next += 1;
    let source = fmt!("let w{unit}: int = {name} + 1");
    let beside = live[at].0.C();
    live.insert(at + 1, (unit.C(), source.C()));
    let entry = Entry { unit, source, expression: false };
    Some(vec![
        Change::Insert { entry, place: Place::After, beside },
        Change::Edit { unit: declarer, source: zeroed },
    ])
}

/// Changes aimed at `access`, and which kind of [`AIMED_KINDS`] they are;
/// `None` if its writer or provider is gone or out of order.
///
/// `live` is the units in session order, with the source each was
/// generated with, and `next` the number the next unit takes.
fn aim(
    rng: &mut SeededRng,
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
        let unit = next.S();
        *next += 1;
        live.insert(after + 1, (unit.C(), shadow.C()));
        let entry = Entry { unit, source: shadow.C(), expression: false };
        Change::Insert { entry, place: Place::After, beside: live[after].0.C() }
    };
    // A `var` taken is revived, by a unit put right after the taker that
    // sets it to the shadow's literal, half the time it can be: a revive
    // needs a take before it, which the units as generated rarely have.
    if access.verb == "takes" && shadow.starts_with("var ") && rng.random_bool(0.5) {
        let (_, value) = shadow.split_once(" = ").X();
        let unit = next.S();
        *next += 1;
        let source = fmt!("set {} = {value}", access.name);
        live.insert(writer + 1, (unit.C(), source.C()));
        let entry = Entry { unit, source, expression: false };
        return Some((5, vec![Change::Insert { entry, place: Place::After, beside: access.writer.C() }]));
    }
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
fn perturb(rng: &mut SeededRng, source: &str) -> String {
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
            true => out.push_str(&rng.random_range(0..20).S()),
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
    Disagrees(Vec<Difference>),
    Panics { at: String, message: String },
    /// The engine panicked on a step, which empties the session; the fresh
    /// run may have too, but a panic is wrong however many make it.
    Crashes(String),
    Hangs,
}

impl Verdict {
    /// Whether `self` is the failure `other` is, for shrinking: the same
    /// panic, by where it was raised, or a disagreement whose first
    /// difference is of the same kind at the same unit.
    ///
    /// Any disagreement would let a shrink drift from the bug it set out
    /// with to another, which the kept session would then pin instead.
    fn same_failure(&self, other: &Verdict) -> bool {
        match (self, other) {
            (Verdict::Disagrees(ours), Verdict::Disagrees(theirs)) => {
                let (ours, theirs) = (&ours[0], &theirs[0]);
                ours.kind == theirs.kind && ours.unit == theirs.unit
            }
            (Verdict::Panics { at, .. }, Verdict::Panics { at: other, .. }) => at == other,
            (Verdict::Crashes(message), Verdict::Crashes(other)) => message == other,
            _ => false,
        }
    }

    fn describe(&self) -> String {
        match self {
            Verdict::Agrees(tally) if tally.resets > 0 => "agrees with a fresh run, both resetting".S(),
            Verdict::Agrees(_) => "agrees with a fresh run".S(),
            Verdict::Disagrees(failures) => failures.iter().map(|failure| failure.message.as_str()).collect::<Vec<_>>().join("\n"),
            Verdict::Panics { at, message } => fmt!("panicked at {at}: {message}"),
            Verdict::Crashes(message) => fmt!("the engine crashed: {message}"),
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
    /// Changes made, by kind; see [`CHANGE_KINDS`].
    changes: [usize; 6],
    /// Units a module edit, and a data edit, ran again, summed over the
    /// edits.
    rerun_by_module: usize,
    rerun_by_data: usize,
    /// Units entered that declare a function, and that call one an earlier
    /// unit declares.
    functions_declared: usize,
    functions_called: usize,
    /// Expression units entered, and how often one ran again after.
    expressions_entered: usize,
    expressions_rerun: usize,
    /// Of those, the aimed ones, by kind; see [`AIMED_KINDS`].
    aimed: [usize; 7],
    /// Units that had returned early, and that were blocked, when a step
    /// ended, summed over the steps.
    returned_early: usize,
    blocked: usize,
    /// What the units do to earlier units' bindings as they are entered or
    /// inserted, by verb: reads, takes, overwrites, updates, revives.
    uses: [usize; 5],
}

const CHANGE_KINDS: [&str; 6] = ["edits", "deletes", "reorders", "inserts", "module edits", "data edits"];
const VERBS: [&str; 5] = ["reads", "takes", "overwrites", "updates", "revives"];

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.entered += other.entered;
        self.failed_on_entry.extend(other.failed_on_entry.iter().cloned());
        self.remaining += other.remaining;
        self.still_running += other.still_running;
        self.resets += other.resets;
        self.returned_early += other.returned_early;
        self.blocked += other.blocked;
        self.rerun_by_module += other.rerun_by_module;
        self.rerun_by_data += other.rerun_by_data;
        self.expressions_entered += other.expressions_entered;
        self.functions_declared += other.functions_declared;
        self.functions_called += other.functions_called;
        self.expressions_rerun += other.expressions_rerun;
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
        if self.functions_declared == 0 {
            shortfalls.push("no unit declared a function".S());
        }
        if self.functions_called == 0 {
            shortfalls.push("no unit called a function an earlier unit declared".S());
        }
        if self.expressions_entered == 0 {
            shortfalls.push("no expression unit was entered".S());
        }
        if self.expressions_rerun == 0 {
            shortfalls.push("no expression unit ran again".S());
        }
        if self.rerun_by_module == 0 {
            shortfalls.push("no module edit ran a unit again".S());
        }
        if self.rerun_by_data == 0 {
            shortfalls.push("no data edit ran a unit again".S());
        }
        if self.returned_early == 0 {
            shortfalls.push("no unit returned early".S());
        }
        if self.blocked == 0 {
            shortfalls.push("no unit was blocked".S());
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
            "{} units entered, {} of them expressions, {} still run of {} after {} ({}), {} sessions reset; \
             expression units ran again {} times; {} units declared functions and {} called an earlier unit's; \
             module edits ran {} units again and data edits {}; after a step, {} units had returned early and {} were blocked; \
             as entered or inserted, units use earlier units' bindings: {}",
            self.entered, self.expressions_entered, self.still_running, self.remaining, changes.join(", "),
            aimed.join(", "), self.resets, self.expressions_rerun, self.functions_declared, self.functions_called,
            self.rerun_by_module, self.rerun_by_data, self.returned_early, self.blocked, uses.join(", "),
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
        let at = info.location().map_or("an unknown place".S(), |location| location.S());
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

/// How many of `entries` declare a function at their top level, and how many
/// call one an earlier entry declares.
fn functions_across(entries: &[Entry]) -> (usize, usize) {
    let mut declared: Vec<String> = Vec::new();
    let (mut declaring, mut calling) = (0, 0);
    for entry in entries {
        let calls = |name: &String| entry.source.match_indices(&fmt!("{name}(")).any(|(at, _)| {
            !entry.source[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_')
        });
        if declared.iter().any(calls) {
            calling += 1;
        }
        let names: Vec<String> = entry.source.lines()
            .filter_map(|line| line.strip_prefix("fun "))
            .map(|rest| rest.split(['(', '<']).next().X().S())
            .collect();
        declaring += usize::from(!names.is_empty());
        declared.extend(names);
    }
    (declaring, calling)
}

fn run(case: &Case) -> Verdict {
    let workspace = Workspace::from_worldfile(&case.library);
    let mut session = Session::start(workspace.sys, workspace.descriptor, workspace.also_roots);
    let mut tally = Tally { aimed: case.aimed, ..Tally::default() };
    let mut failures = Vec::new();
    let mut expressions: Vec<&str> = Vec::new();
    for (number, change) in case.changes.iter().enumerate() {
        let touched = session.apply(change);
        if !matches!(change, Change::Enter(_)) {
            tally.expressions_rerun += touched.iter()
                .filter(|(unit, outcome)| outcome.ran != Ran::No && expressions.contains(&unit.as_str()))
                .count();
        }
        match change {
            Change::Enter(entries) => {
                let (declared, called) = functions_across(entries);
                tally.functions_declared += declared;
                tally.functions_called += called;
                expressions.extend(entries.iter().filter(|entry| entry.expression).map(|entry| entry.unit.as_str()));
                tally.expressions_entered += entries.iter().filter(|entry| entry.expression).count();
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
            Change::Insert { entry, .. } => {
                tally.changes[3] += 1;
                if entry.expression {
                    expressions.push(&entry.unit);
                }
                if session.reset().is_none() {
                    for used in session.uses(session.index(&entry.unit)) {
                        let verb = VERBS.iter().position(|verb| used.starts_with(verb)).X();
                        tally.uses[verb] += 1;
                    }
                }
            }
            Change::EditModule { .. } => {
                tally.changes[4] += 1;
                tally.rerun_by_module += touched.iter().filter(|(_, outcome)| outcome.ran != Ran::No).count();
            }
            Change::EditData { .. } => {
                tally.changes[5] += 1;
                tally.rerun_by_data += touched.iter().filter(|(_, outcome)| outcome.ran != Ran::No).count();
            }
        }
        failures.extend(session.differs_from_fresh(&fmt!("step {}", number + 1)));
        if session.reset().is_some() {
            break;
        }
        tally.returned_early += session.states().iter().filter(|ran| **ran == Ran::ReturnsEarly).count();
        tally.blocked += session.states().iter().filter(|ran| **ran == Ran::Blocked).count();
    }
    if !failures.is_empty() {
        return Verdict::Disagrees(failures);
    }
    match session.reset() {
        Some(Eval::Crashed(message)) => return Verdict::Crashes(message.C()),
        Some(_) => {
            tally.resets = 1;
            return Verdict::Agrees(tally);
        }
        None => {}
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
            Change::Edit { .. } | Change::Insert { .. } | Change::EditModule { .. } | Change::EditData { .. } => {
                found.push((index, None));
            }
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
        (Change::EditModule { source, .. }, None) => source,
        (Change::EditData { text, .. }, None) => text,
        _ => unreachable!("`sources` says where the sources are"),
    }
}

// ============================================================================
// Running
// ============================================================================

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
            (name, Case { library, changes, aimed: [0; 7] })
        })
        .collect()
}

/// Write a failing case where it can be run again, and say how; `label`
/// says which, as `seed 12` or `seed 12 dense`.
fn write_out(label: &str, case: &Case) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("generative-reactive");
    std::fs::create_dir_all(&dir).X();
    let path = dir.join(fmt!("{}.world", label.replace(' ', "-")));
    std::fs::write(&path, write_changes(&case.library, &case.changes)).X();
    path
}

fn main() {
    catch_panics();

    if let Ok(path) = std::env::var("GENERATIVE_REACTIVE_WORLD") {
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
        let (library, changes) = read_changes(&path, &text);
        let verdict = check(&Case { library, changes, aimed: [0; 7] }, SEED_TIMEOUT);
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

    let seeds = env_seeds("GENERATIVE_REACTIVE", DEFAULT_SEED, DEFAULT_COUNT);
    println!("generative reactive tests: {} seeds from {}", seeds.count, seeds.start);

    let mut total = Tally::default();
    let mut dense_total = Tally::default();
    let mut failed = Vec::new();
    for (seed, dense_case) in seeds.iter().flat_map(|seed| [(seed, false), (seed, true)]) {
        let (case, label) = match dense_case {
            false => (generate(seed), fmt!("seed {seed}")),
            true => (dense(seed), fmt!("seed {seed} dense")),
        };
        // A failing case is written out to be read back, so every case is.
        let written = write_changes(&case.library, &case.changes);
        let (library, changes) = read_changes(&label, &written);
        assert_eq!(written, write_changes(&library, &changes), "{label} reads back as it was written");
        let verdict = check(&case, SEED_TIMEOUT);
        let Verdict::Agrees(tally) = &verdict else {
            println!("{label}: \x1b[31mFAIL\x1b[0m\n  {}", verdict.describe().replace('\n', "\n  "));
            let shrink_it = std::env::var("GENERATIVE_REACTIVE_NO_SHRINK").is_err();
            let case = match verdict {
                Verdict::Hangs => case,
                _ if !shrink_it => case,
                _ => {
                    println!("  shrinking...");
                    shrink(case, &verdict)
                }
            };
            let path = write_out(&label, &case);
            println!("  the session is {}", path.display());
            println!("  run it with GENERATIVE_REACTIVE_WORLD={} cargo test -p datalove-tests --test generative_reactive_tests", path.display());
            failed.push(label);
            continue;
        };
        if dense_case {
            dense_total.add(tally);
            continue;
        }
        if !tally.failed_on_entry.is_empty() {
            let path = write_out(&label, &case);
            println!(
                "{label}: \x1b[31mFAIL\x1b[0m\n  units {} of {} do not compile as generated, which is a worldgen bug; the session is {}",
                tally.failed_on_entry.join(", "), tally.entered, path.display(),
            );
            failed.push(label);
            continue;
        }
        total.add(tally);
    }

    println!("{}", total.describe());
    println!("dense sessions: {}", dense_total.describe());
    let shortfalls = match seeds.is_default() {
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
