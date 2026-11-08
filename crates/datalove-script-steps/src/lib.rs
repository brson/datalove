//! Script sessions driven a change at a time, and held to a fresh run.
//!
//! A change is units appended, one unit edited, deleted, inserted or reordered,
//! or a module or data file edited. [`Session::apply`] makes one and says
//! what each unit did: whether it recompiled, counted from salsa, and how
//! it ran. [`Session::differs_from_fresh`] then compares the session with a
//! new engine that runs its current units in order, which is the definition
//! of a correct step: each unit must come to the same end and hold the same
//! bindings.
//!
//! The step tables of `datalove-doc-fixtures` are checked on top of this,
//! and the generated sessions of `datalove-tests` against it alone.

use rmx::prelude::*;

use bct::input::Source;
use datalove_ct::query_events::QueryRecorder;
use datalove_ct::unit_analysis::{self, Pass};
use datalove_datafun::driver::WorkspaceDescriptor;
use datalove_repl::Place;
use datalove_script_engine::{Engine, Eval, SystemLibrary, Unit};

// ============================================================================
// Changes
// ============================================================================

/// A unit entered, by appending or inserting.
#[derive(Clone)]
pub struct Entry {
    pub unit: String,
    pub source: String,
    pub expression: bool,
}

impl Entry {
    fn code(&self) -> Unit {
        match self.expression {
            true => Unit::Expression(self.source.C()),
            false => Unit::Fragment(self.source.C()),
        }
    }
}

#[derive(Clone)]
pub enum Change {
    /// Units appended, one at a time.
    Enter(Vec<Entry>),
    Edit { unit: String, source: String },
    /// A data file's text replaced, by its `library/package/name` path.
    EditData { path: String, text: String },
    /// A module's source replaced, by its `library/package/module` path.
    EditModule { path: String, source: String },
    Delete { unit: String },
    /// A unit put before or after the unit `beside`.
    Insert { entry: Entry, place: Place, beside: String },
    /// A unit reordered to before or after the unit `beside`.
    Move { unit: String, place: Place, beside: String },
}

// ============================================================================
// Outcomes
// ============================================================================

/// What became of a unit's run in one step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ran {
    /// Not run.
    No,
    /// Ran to its end.
    Runs,
    /// Did not compile, so did not run.
    Fails,
    ReturnsEarly,
    /// Compiled, and not run because something it reads did not finish.
    Blocked,
    Deleted,
}

/// What a unit did in one step.
///
/// _Recompiles_ is salsa running the unit's typecheck or its ownership
/// analysis again. The typecheck alone misses a unit that names a binding an
/// earlier unit now gives away or gives back: whether it may use the name is
/// decided in ownership analysis. Lowering is not memoized and is redone for
/// every unit the engine reaches, so it is not what tells a recompile from a
/// rerun.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub recompiles: bool,
    pub ran: Ran,
}

fn ran(eval: &Eval) -> Ran {
    match eval {
        Eval::NotCompiled(_) => Ran::Fails,
        Eval::ReturnedEarly(_) => Ran::ReturnsEarly,
        Eval::Blocked { .. } => Ran::Blocked,
        Eval::Ran(_) | Eval::Value(_) => Ran::Runs,
        Eval::Crashed(_) | Eval::RuntimeError(_) => unreachable!("a reset ends the session; see `Session::reset`"),
    }
}

/// A unit's bindings as a step table's `holds after` writes them, and as
/// the fresh run compares them.
fn holds(bindings: &[(String, Option<String>)]) -> String {
    if bindings.is_empty() {
        return "-".S();
    }
    bindings.iter()
        .map(|(name, value)| match value {
            Some(value) => fmt!("`{name}` = {value}"),
            None => fmt!("`{name}` taken"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ============================================================================
// Running
// ============================================================================

/// A session in progress: the engine and the names of its units, in order.
pub struct Session {
    engine: Engine,
    units: Vec<String>,
    /// Each unit's current source, in the same order.
    code: Vec<Unit>,
    /// What each unit came to when it last ran or failed to, in the same
    /// order.
    states: Vec<Ran>,
    /// The units whose last compile failed, which leaves no record of what
    /// they do.
    failed: Vec<String>,
    recorder: QueryRecorder,
    /// The units the engine says it typechecked or analyzed for ownership in
    /// the step just applied, which the recorder checks.
    analyzed: Vec<String>,
    also_roots: Vec<String>,
    /// What a unit came to that took the session with it, a crash or a
    /// runtime error, after which the engine holds no units.
    reset: Option<Eval>,
}

impl Session {
    pub fn start(sys: SystemLibrary, workspace: WorkspaceDescriptor, also_roots: Vec<String>) -> Session {
        let recorder = QueryRecorder::new();
        let db = datalove_datafun::Database::recording(&recorder);
        let engine = Engine::in_database(db, sys, workspace, also_roots.C()).expect("the engine starts");
        assert!(engine.module_errors().is_empty(), "the modules compile: {:?}", engine.module_errors().broken);
        recorder.clear();
        Session {
            engine, units: Vec::new(), code: Vec::new(), states: Vec::new(), failed: Vec::new(),
            recorder, analyzed: Vec::new(), also_roots, reset: None,
        }
    }

    /// What unit `index` does to earlier units' bindings, as a step table's
    /// `uses` column says it, one entry per binding.
    pub fn uses(&self, index: usize) -> Vec<String> {
        let ids = self.engine.unit_ids();
        let accesses = self.engine.unit_accesses(ids[index]);
        let mut uses: Vec<String> = accesses.uses.iter()
            .filter_map(|name| {
                let provider = (0..index).rev()
                    .find(|earlier| self.engine.unit_accesses(ids[*earlier]).provides.contains(name))?;
                let has = |names: &[String]| names.contains(name);
                let verb = if has(&accesses.revives) {
                    "revives"
                } else if has(&accesses.takes) {
                    "takes"
                } else if has(&accesses.updates) {
                    "updates"
                } else if has(&accesses.overwrites) {
                    "overwrites"
                } else {
                    "reads"
                };
                Some(fmt!("{verb} `{name}` ({})", self.units[provider]))
            })
            .collect();
        uses.sort();
        uses
    }

    pub fn index(&self, unit: &str) -> usize {
        self.units.iter().position(|name| name == unit)
            .unwrap_or_else(|| panic!("no unit `{unit}` in the session"))
    }

    /// What a unit came to that took the session with it, if one did.
    ///
    /// A runtime error or a crash empties the engine's session, so the
    /// session ends there: it takes no more changes, and what its units
    /// hold or do is not asked.
    pub fn reset(&self) -> Option<&Eval> {
        self.reset.as_ref()
    }

    /// The units' names, in session order.
    pub fn units(&self) -> &[String] {
        &self.units
    }

    /// Whether unit `unit`'s last compile failed.
    pub fn has_failed(&self, unit: &str) -> bool {
        self.failed.iter().any(|name| name == unit)
    }

    /// The units the engine says it typechecked or analyzed for ownership in
    /// the step just applied.
    pub fn analyzed(&self) -> &[String] {
        &self.analyzed
    }

    /// What each unit came to when it last ran or failed to, in session
    /// order.
    pub fn states(&self) -> &[Ran] {
        &self.states
    }

    /// What unit `index`'s bindings hold now, as [`holds`] writes them.
    pub fn holds(&mut self, index: usize) -> String {
        let id = self.engine.unit_ids()[index];
        holds(&self.engine.unit_bindings(id))
    }

    /// The engine's id for the unit named `unit`.
    fn id(&self, unit: &str) -> u32 {
        self.engine.unit_ids()[self.index(unit)]
    }

    /// The name of the unit the engine knows by `id`.
    fn name(&self, id: u32) -> String {
        let index = self.engine.unit_ids().iter().position(|held| *held == id)
            .unwrap_or_else(|| panic!("no unit {id} in the session"));
        self.units[index].C()
    }

    /// Make a step's change, and say what each unit did.
    pub fn apply(&mut self, change: &Change) -> Vec<(String, Outcome)> {
        assert!(self.reset.is_none(), "a change to a session that reset");
        self.analyzed.clear();
        let (ran, analyses) = unit_analysis::collect(|| {
            let mut ran: Vec<(String, Ran)> = Vec::new();
            match change {
                Change::Enter(entries) => {
                    for entry in entries {
                        match self.append(entry) {
                            Some(outcome) => ran.push(outcome),
                            None => break,
                        }
                    }
                }
                Change::Edit { unit, source } => {
                    let index = self.index(unit);
                    self.code[index] = match self.code[index].is_expression() {
                        true => Unit::Expression(source.C()),
                        false => Unit::Fragment(source.C()),
                    };
                    let reports = self.engine.edit_unit(self.id(unit), source);
                    ran.extend(self.reported(reports));
                }
                Change::EditData { path, text } => {
                    let [library, package, name] = library_path(path);
                    let first = self.engine.first_session();
                    let reports = self.engine.edit_data(library, package, name, text).remove(&first)
                        .expect("every session is re-derived");
                    ran.extend(self.reported(reports));
                }
                Change::EditModule { path, source } => {
                    let [library, package, module] = library_path(path);
                    let first = self.engine.first_session();
                    let reports = self.engine.edit_module(library, package, module, source).remove(&first)
                        .expect("every session is re-derived");
                    ran.extend(self.reported(reports));
                }
                Change::Delete { unit } => {
                    let id = self.id(unit);
                    let index = self.index(unit);
                    self.units.remove(index);
                    self.code.remove(index);
                    self.states.remove(index);
                    ran.push((unit.C(), Ran::Deleted));
                    let reports = self.engine.delete_unit(id);
                    ran.extend(self.reported(reports));
                }
                Change::Move { unit, place, beside } => {
                    let id = self.id(unit);
                    let from = self.index(unit);
                    let name = self.units.remove(from);
                    let code = self.code.remove(from);
                    let state = self.states.remove(from);
                    let to = place.index(self.index(beside));
                    self.units.insert(to, name);
                    self.code.insert(to, code);
                    self.states.insert(to, state);
                    let reports = self.engine.move_unit(id, to);
                    ran.extend(self.reported(reports));
                }
                Change::Insert { entry, place, beside } => {
                    let index = place.index(self.index(beside));
                    self.units.insert(index, entry.unit.C());
                    self.code.insert(index, entry.code());
                    self.states.insert(index, Ran::No);
                    let reports = self.engine.insert_unit(index, &entry.source, entry.expression);
                    ran.extend(self.reported(reports));
                }
            }
            ran
        });
        let recompiled = self.recompiled(&analyses);
        for (unit, ran) in &ran {
            self.failed.retain(|name| name != unit);
            if *ran == Ran::Fails {
                self.failed.push(unit.C());
            }
            if !matches!(ran, Ran::No | Ran::Deleted) {
                let index = self.index(unit);
                self.states[index] = *ran;
            }
        }

        let mut outcomes: Vec<(String, Outcome)> = ran.iter()
            .map(|(unit, ran)| (unit.C(), Outcome { recompiles: recompiled.contains(unit), ran: *ran }))
            .collect();
        for unit in &recompiled {
            if !ran.iter().any(|(name, _)| name == unit) {
                outcomes.push((unit.C(), Outcome { recompiles: true, ran: Ran::No }));
            }
        }
        outcomes
    }

    /// The units whose typecheck or ownership analysis salsa ran in this step.
    ///
    /// What salsa ran is what it reports, and `analyses` only says which unit
    /// each of those ran for.
    ///
    /// An ownership analysis is attributed to the unit the compiler was
    /// compiling when it ran, which it marks for this. Its key can't say:
    /// the analysis is keyed on what earlier units left behind, so it runs
    /// under a key no unit has had before exactly when that changes.
    ///
    /// A typecheck's key can't say either: it is keyed on the unit and the
    /// session's environment, which changes when a unit requires a module,
    /// and the earlier units are typechecked again under the new one as a
    /// later unit looks a name up through them. Nor can the unit being
    /// compiled, since one unit's typecheck runs inside another's. So the
    /// typecheck says itself which unit it is for, in `analyses`, and every
    /// typecheck salsa ran must have said so, once.
    fn recompiled(&mut self, analyses: &[(Pass, Source)]) -> Vec<String> {
        let events = self.recorder.take();
        // The engine hears which units it analyzed only on its own thread.
        for event in &events {
            let analysis = event.query == "typecheck_script_unit" || event.query.starts_with("analyze_script_");
            assert!(!(analysis && event.on_worker), "{} ran on a worker, where the engine cannot see it", event.query);
        }
        let typechecks = events.iter().filter(|event| event.query == "typecheck_script_unit").count();
        let typechecked: Vec<Source> = analyses.iter()
            .filter(|(pass, _)| *pass == Pass::Typecheck)
            .map(|(_, source)| *source)
            .collect();
        assert_eq!(typechecked.len(), typechecks, "every typecheck salsa ran says which unit it is for, once");

        // The units' sources after the step; a unit the step deleted has none
        // left to name.
        let sources = self.engine.unit_sources();
        let mut recompiled: Vec<String> = Vec::new();
        let mut note = |unit: &String| {
            if !recompiled.contains(unit) {
                recompiled.push(unit.C());
            }
        };
        for source in typechecked {
            if let Some(index) = sources.iter().position(|unit| *unit == source) {
                note(&self.units[index]);
            }
        }
        for event in &events {
            if !event.query.starts_with("analyze_script_") {
                continue;
            }
            let index = event.compiling.expect("an ownership analysis runs inside a unit's compile");
            note(&self.units[index]);
        }
        recompiled
    }

    /// What each unit the engine reports came to, and the units it says it
    /// analyzed, for the recorder to check.
    ///
    /// A unit analyzed and not re-derived did not run.
    fn reported(&mut self, reports: Vec<datalove_script_engine::UnitStep>) -> Vec<(String, Ran)> {
        // After a reset the ids are gone, and nothing is named by them.
        if let Some(eval) = reports.iter().filter_map(|report| report.eval.as_ref()).find(|eval| eval.resets()) {
            self.reset = Some(eval.C());
            return Vec::new();
        }
        self.note_analyzed();
        reports.iter()
            .map(|report| (self.name(report.unit), report.eval.as_ref().map_or(Ran::No, ran)))
            .collect()
    }

    /// Note the units the engine's last step says it typechecked or analyzed
    /// for ownership; the recorder sees only those two passes.
    fn note_analyzed(&mut self) {
        for report in self.engine.last_step() {
            if report.compiled.typecheck || report.compiled.ownership {
                let unit = self.name(report.unit);
                if !self.analyzed.contains(&unit) {
                    self.analyzed.push(unit);
                }
            }
        }
    }

    /// Append a unit, and say what it came to; `None` if it took the
    /// session with it.
    fn append(&mut self, entry: &Entry) -> Option<(String, Ran)> {
        self.units.push(entry.unit.C());
        self.code.push(entry.code());
        let eval = self.engine.eval(entry.code());
        if eval.resets() {
            self.states.push(Ran::No);
            self.reset = Some(eval);
            return None;
        }
        self.note_analyzed();
        self.states.push(ran(&eval));
        Some((entry.unit.C(), ran(&eval)))
    }

    /// Say every way this session differs from a fresh run of its current
    /// code: each unit, in order, must have come to the same end and hold the
    /// same bindings.
    ///
    /// This is what a step is for, so it is checked whether or not the step
    /// has a table, and it does not trust the tables' `holds after`.
    pub fn differs_from_fresh(&mut self, at: &str) -> Vec<String> {
        let sys = datalove_sys_packages::system_library();
        let mut fresh = Engine::with_options(
            sys, self.engine.workspace().C(), datalove_datafun::DebugOutputMode::Disabled, self.also_roots.C(),
        ).expect("the engine starts");
        let mut states: Vec<Ran> = Vec::new();
        let mut fresh_reset = None;
        for (index, unit) in self.code.iter().enumerate() {
            let eval = fresh.eval(unit.C());
            if eval.resets() {
                fresh_reset = Some((index, eval));
                break;
            }
            states.push(ran(&eval));
        }

        // A reset leaves nothing to compare but how it came: both must
        // reset, and the same way.
        let same_way = |a: &Eval, b: &Eval| std::mem::discriminant(a) == std::mem::discriminant(b);
        match (&self.reset, &fresh_reset) {
            (None, None) => {}
            (Some(reset), Some((_, fresh))) if same_way(reset, fresh) => return Vec::new(),
            (Some(reset), Some((index, fresh))) => return vec![fmt!(
                "{at}: the session came to {reset:?}, and a fresh run to {fresh:?} at unit {}", self.units[*index],
            )],
            (Some(reset), None) => return vec![fmt!(
                "{at}: the session came to {reset:?}, and a fresh run did not reset",
            )],
            (None, Some((index, fresh))) => return vec![fmt!(
                "{at}: a fresh run came to {fresh:?} at unit {}, and the session did not reset", self.units[*index],
            )],
        }

        let ids = self.engine.unit_ids();
        let fresh_ids = fresh.unit_ids();
        let mut failures = Vec::new();
        for (index, unit) in self.units.iter().enumerate() {
            if self.states[index] != states[index] {
                failures.push(fmt!(
                    "{at}: unit {unit} came to {:?}, and to {:?} in a fresh run",
                    self.states[index], states[index],
                ));
                continue;
            }
            let held = holds(&self.engine.unit_bindings(ids[index]));
            let fresh_held = holds(&fresh.unit_bindings(fresh_ids[index]));
            if held != fresh_held {
                failures.push(fmt!("{at}: unit {unit} holds {held}, and {fresh_held} in a fresh run"));
            }
        }
        failures
    }
}

/// A `library/package/name` path, taken apart.
fn library_path(path: &str) -> [&str; 3] {
    let parts: Vec<&str> = path.splitn(3, '/').collect();
    parts.try_into().unwrap_or_else(|_| panic!("`{path}` is not a library/package/name path"))
}

/// The workspace a session runs in.
pub struct Workspace {
    pub sys: SystemLibrary,
    pub descriptor: WorkspaceDescriptor,
    /// Modules compiled whether or not a unit requires them.
    pub also_roots: Vec<String>,
}

impl Workspace {
    /// The system library and nothing else.
    pub fn system() -> Workspace {
        let sys = datalove_sys_packages::system_library();
        let descriptor = WorkspaceDescriptor::from_system_library(&sys);
        Workspace { sys, descriptor, also_roots: Vec::new() }
    }

    /// The system library, and the `module` and `data` sections of
    /// `library`, a worldfile, read as any worldfile's are.
    pub fn from_worldfile(library: &str) -> Workspace {
        if library.is_empty() {
            return Workspace::system();
        }
        let fs = datalove_vio::MemFs::new();
        let path = std::path::Path::new("/library.world");
        fs.add_file(path, library);
        let world = datalove_repl::read_world(&fs, path).X();
        let sys = datalove_sys_packages::system_library();
        let (descriptor, also_roots) = world.workspace(&sys).X();
        Workspace { sys, descriptor, also_roots }
    }
}

// ============================================================================
// Worldfiles
// ============================================================================

pub const SEPARATOR: &str = "----------";

/// One section of a worldfile.
pub struct Section {
    pub header: String,
    pub content: String,
}

/// Split worldfile text into its sections.
///
/// Comment lines before the first section are skipped.
pub fn sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut sections = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_end() != SEPARATOR {
            let line = lines[i].trim();
            assert!(line.is_empty() || line.starts_with("//"), "text outside a section: {:?}", lines[i]);
            i += 1;
            continue;
        }
        let header = lines[i + 1].trim().S();
        assert_eq!(lines[i + 2].trim_end(), SEPARATOR, "a header is closed by a separator: {header}");
        let mut end = i + 3;
        while end < lines.len() && lines[end].trim_end() != SEPARATOR {
            end += 1;
        }
        let content = lines[i + 3..end].join("\n").trim().S();
        sections.push(Section { header, content });
        i = end;
    }
    sections
}

/// What one section of a session's worldfile is.
pub enum Read {
    /// A `module` or `data` section, added to the reader's library.
    Library,
    /// A `scriptunit-fragment`: a unit entered, by appending.
    Entry(Entry),
    /// Any other change: an edit, delete, insert or reorder of a unit, or an
    /// edit of a module or data.
    Change(Change),
    /// None of those, for the caller to read.
    Other,
}

/// Reads a session's worldfile a section at a time: what each section is,
/// with the units named by the order they are entered in, and the library
/// the `module` and `data` sections make.
pub struct SessionReader {
    /// Whose worldfile it is, in what goes wrong.
    name: String,
    /// The `module` and `data` sections so far, as worldfile text.
    pub library: String,
    /// The number the next unit entered, by fragment or by insert, takes.
    next: usize,
}

impl SessionReader {
    pub fn new(name: &str) -> SessionReader {
        SessionReader { name: name.S(), library: String::new(), next: 1 }
    }

    /// What `section` is.
    pub fn read(&mut self, section: &Section) -> Read {
        let name = &self.name;
        let mut words = section.header.split_whitespace();
        let kind = words.next().X();
        let args: Vec<&str> = words.collect();
        let content = section.content.C();
        let change = match kind {
            "module" | "data" => {
                self.library.push_str(&fmt!("{SEPARATOR}\n{}\n{SEPARATOR}\n\n{content}\n\n", section.header));
                return Read::Library;
            }
            "scriptunit-fragment" => {
                let unit = self.take_id(args.first().copied());
                return Read::Entry(Entry { unit, source: content, expression: false });
            }
            "module-edit" => Change::EditModule { path: args[0].S(), source: content },
            "data-edit" => Change::EditData { path: args[0].S(), text: content },
            "scriptunit-edit" => Change::Edit { unit: args[0].S(), source: content },
            "scriptunit-delete" => Change::Delete { unit: args[0].S() },
            "scriptunit-move" => {
                let place = match args.get(1) {
                    Some(&"before") => Place::Before,
                    Some(&"after") => Place::After,
                    _ => panic!("{name}: `scriptunit-move <unit> before|after <unit>`"),
                };
                Change::Move { unit: args[0].S(), place, beside: args[2].S() }
            }
            "scriptunit-insert" => {
                assert_eq!(args.get(1), Some(&"after"), "{name}: `scriptunit-insert <unit> after <unit>`");
                let entry = Entry { unit: self.take_id(Some(args[0])), source: content, expression: false };
                Change::Insert { entry, place: Place::After, beside: args[2].S() }
            }
            _ => return Read::Other,
        };
        Read::Change(change)
    }

    /// The next unit's name, which `id`, the name its section gives it, must
    /// be if there is one.
    fn take_id(&mut self, id: Option<&str>) -> String {
        let unit = self.next.to_string();
        if let Some(id) = id {
            assert_eq!(id, unit, "{}: a new unit takes the next id", self.name);
        }
        self.next += 1;
        unit
    }
}

/// A session as a worldfile of changes and nothing else: the `module` and
/// `data` sections it requires, as worldfile text, and its changes, units
/// entered together as one. `name` says whose it is in what goes wrong.
pub fn read_changes(name: &str, text: &str) -> (String, Vec<Change>) {
    let mut reader = SessionReader::new(name);
    let mut changes: Vec<Change> = Vec::new();
    let mut entered: Vec<Entry> = Vec::new();
    for section in sections(text) {
        match reader.read(&section) {
            Read::Library => {
                assert!(changes.is_empty() && entered.is_empty(), "{name}: a library section after a unit");
            }
            Read::Entry(entry) => entered.push(entry),
            Read::Change(change) => {
                if !entered.is_empty() {
                    changes.push(Change::Enter(std::mem::take(&mut entered)));
                }
                changes.push(change);
            }
            Read::Other => panic!("{name}: `{}` is not a change", section.header),
        }
    }
    if !entered.is_empty() {
        changes.push(Change::Enter(entered));
    }
    (reader.library, changes)
}

/// A session as a worldfile that [`read_changes`] reads back: the library
/// text, then each change as its section.
///
/// The units are renamed by the order they are entered in, since a
/// worldfile names each by the next number.
pub fn write_changes(library: &str, changes: &[Change]) -> String {
    let mut names: Vec<String> = Vec::new();
    for change in changes {
        match change {
            Change::Enter(entries) => names.extend(entries.iter().map(|entry| entry.unit.C())),
            Change::Insert { entry, .. } => names.push(entry.unit.C()),
            _ => {}
        }
    }
    let id = |unit: &str| (names.iter().position(|name| name == unit).X() + 1).to_string();
    let section = |header: String, content: &str| fmt!("{SEPARATOR}\n{header}\n{SEPARATOR}\n\n{content}\n\n");

    let mut text = library.trim().S();
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    for change in changes {
        let written = match change {
            Change::Enter(entries) => entries.iter()
                .map(|entry| {
                    assert!(!entry.expression, "an expression unit has no section");
                    section(fmt!("scriptunit-fragment {}", id(&entry.unit)), &entry.source)
                })
                .collect(),
            Change::Edit { unit, source } => section(fmt!("scriptunit-edit {}", id(unit)), source),
            Change::EditData { path, text } => section(fmt!("data-edit {path}"), text),
            Change::EditModule { path, source } => section(fmt!("module-edit {path}"), source),
            Change::Delete { unit } => section(fmt!("scriptunit-delete {}", id(unit)), ""),
            Change::Insert { entry, place, beside } => {
                assert!(matches!(place, Place::After), "a worldfile inserts after a unit");
                section(fmt!("scriptunit-insert {} after {}", id(&entry.unit), id(beside)), &entry.source)
            }
            Change::Move { unit, place, beside } => {
                let place = match place {
                    Place::Before => "before",
                    Place::After => "after",
                };
                section(fmt!("scriptunit-move {} {place} {}", id(unit), id(beside)), "")
            }
        };
        text.push_str(&written);
    }
    text
}
