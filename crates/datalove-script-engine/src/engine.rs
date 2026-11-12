//! The engine, and how it settles the modules a session compiles against.

use rmx::prelude::*;
use salsa::Setter as _;
use serde::Serialize;
use std::cell::{Ref, RefCell};
use std::sync::Arc;

use crate::{Unit, Eval, EvalBinding, EvalExpr, EnvBinding};
use datalove_datafun as datafun;
use datafun::driver::{CompiledModules, ScriptCompiler, ScriptCompilationResult, ScriptExecutor, ScriptSession, ModuleCompiler, PackageLibrary, SystemLibrary, UnitStatus, WorkspaceDescriptor};
use datafun::driver::rider_load::{RegisteredNatives, RiderNatives, register_natives};
use datalove_datafun_interp::CallDispatcher;
use datalove_datafun_ir::{ExportBinding, IrCodeUnit};
use bct::input::Source;
use datafun::incremental::Roots;
use bct::diagnostic::StoredDiagnostic;
use rmx::std::collections::{BTreeMap, BTreeSet};
use datalove_ct::unit_analysis::{self, Pass};

/// The engine, which owns the database its sessions are compiled in, and the
/// sessions.
///
/// **A session is a list of script units and the runtime they run in**, and
/// the engine holds any number of them over one compilation of the modules:
/// each has its own compiled units, its own executor and its own runtime, and
/// they share the database, the module compiler and the natives. The engine starts
/// with one, which the methods on `Engine` itself act on;
/// [`Engine::open_session`] opens more, and [`Engine::session`] acts on any.
///
/// **It owns the database rather than borrowing it**, which is what lets a
/// unit be edited: an edit is `set_text` on the unit's `Source` and needs
/// `&mut db`, and a `ScriptCompiler` holding `&'db dyn Database` cannot be
/// alive across that. So no compiler is kept between calls; [`ScriptSession`]
/// is kept instead and a compiler is built over it per operation. What that
/// costs is re-deriving the module compilation each time, which is salsa
/// verifying what it already has -- the same thing a crash reset has always
/// relied on. See `botdocs/plan-script-reactivity.md`.
pub struct Engine {
    shared: Shared,
    sessions: BTreeMap<SessionId, SessionState>,
    /// The session the engine started with, which is never closed.
    first: SessionId,
    /// The number the next session opened is given.
    next_session: u32,
}

/// A session of an engine's, by the number it was opened under.
///
/// Numbers count from 0, the session the engine started with, and are not
/// given twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SessionId(u32);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What the sessions share: the database, and the modules compiled in it.
///
/// **A step on a session writes, then compiles, then runs, and only the
/// write needs this mutably.** The write is what needs `&mut db`: `set_text`
/// for an edit, `set_prev` for a splice, a module's new source. Compiling
/// reads the database, and settles and compiles the modules through the cell
/// they are in; running reaches the session's executor and nothing here. So
/// everything after the write is done through `&Shared`, which is the line a
/// session on a thread of its own will be drawn along: see
/// `botdocs/plan-multi-session.md`.
struct Shared {
    db: datafun::Database,
    /// The workspace the sessions compile against.
    workspace: WorkspaceDescriptor,
    /// The natives the module compiler was given, which each session's executor
    /// registers the ones it calls from.
    ///
    /// The same for the life of the module compiler, so that neither a reset nor a
    /// new session rebuilds riders or reloads them.
    natives: Arc<RiderNatives>,
    /// Where `debuglog` goes, kept so every session sends it to the same place.
    debug_output: datafun::DebugOutputMode,
    modules: RefCell<Modules>,
    /// What makes the dispatcher each session's executor runs calls through,
    /// one a session, kept across its edits; see [`Engine::use_dispatcher`].
    dispatcher: Option<Box<dyn Fn() -> Box<dyn CallDispatcher>>>,
}

/// The modules the sessions compile against, and how they were settled.
///
/// **Borrowed only for as long as settling and compiling them takes.** The
/// `CompiledModules` a compile returns borrows the database and not these, so
/// nothing that compiles a unit or runs one holds them, and a session's step
/// takes them twice at most: to compile, and to say why a unit failed.
struct Modules {
    /// The module compiler that compiled the modules, kept rather than rebuilt.
    ///
    /// A module compiler owns the `Source` inputs salsa keys its queries on, so throwing
    /// one away and making another means recompiling the library from nothing. A
    /// reset happens on every panic recovery, and the library it recompiles cannot
    /// have changed -- it is the copy embedded in the binary -- so keeping the
    /// module compiler turns a reset from a full compile into salsa verifying what it
    /// already has.
    ///
    /// **Its roots are what every open session's units require**, plus
    /// `also_roots`, so one session's units compile against modules only
    /// another requires. That changes nothing a unit means, since a unit sees
    /// only the modules it requires.
    module_compiler: ModuleCompiler,
    /// Modules compiled whether or not a unit requires them.
    ///
    /// The rest of the roots are what the sessions' units require, worked out
    /// again before every step; see [`Compiling::compile_units`].
    also_roots: Vec<String>,
    /// The required modules that do not compile, as the last settling of the
    /// roots found them; see [`settle_roots`].
    errors: ModuleErrors,
    /// How many times the modules compiled have changed, by the roots moving
    /// or a module being edited.
    ///
    /// Each session's executor holds the modules as they were when it last
    /// took them, and takes them again when this has moved on; see
    /// [`SessionState::taken`].
    changes: u64,
}

impl Modules {
    /// Settle the roots on what `units` require, counting a change if the
    /// roots moved.
    ///
    /// `units` are every open session's units, as they will stand once the
    /// step in hand is done.
    fn settle(&mut self, db: &datafun::Database, units: &[Source]) {
        let previous = self.module_compiler.roots().C();
        self.errors = settle_roots(db, &mut self.module_compiler, units, &self.also_roots)
            .expect("errors no root reaches are a rider's, and the riders compiled at startup");
        if *self.module_compiler.roots() != previous {
            self.changes += 1;
        }
    }
}

impl Shared {
    /// Settle the roots on what `units` require and compile the modules they
    /// come to.
    fn compile_modules(&self, units: &[Source]) -> CompiledModules<'_> {
        let mut modules = self.modules.borrow_mut();
        modules.settle(&self.db, units);
        modules.module_compiler.compile(&self.db)
    }

    /// A session with no units, compiling against the modules `units`
    /// require, which are the other sessions'.
    fn start_session(&self, units: &[Source]) -> AnyResult<SessionState> {
        let compiled = self.compile_modules(units);
        assert!(!compiled.has_errors(), "the roots were settled on modules that compile");

        let session = compiled.script_compiler_default(&self.db)
            .expect("script_compiler should succeed after error check")
            .into_session();
        let mut executor = compiled.script_executor(self.debug_output, self.dispatcher.as_ref().map(|make| make()))
            .expect("script_executor should succeed after error check");
        let natives = register_natives(&self.natives, &compiled, &mut executor)?;

        Ok(SessionState {
            session,
            executor,
            natives,
            taken: self.modules.borrow().changes,
            last_step: Vec::new(),
        })
    }

    /// Keep the descriptor in step with the module compiler.
    ///
    /// A reset rebuilds from the module compiler rather than from the descriptor, so
    /// nothing reads this today. It is what a `WorkspaceDelta` would be taken
    /// against, and a descriptor saying one thing while the module compiler compiles
    /// another is the kind of disagreement that is found much later.
    fn set_workspace_module(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) {
        let libraries = self.workspace.system_library.iter_mut()
            .chain(self.workspace.user_libraries.iter_mut());
        let held = libraries
            .filter(|held| held.name == library)
            .filter_map(|held| held.packages.get_mut(package))
            .filter_map(|held| held.modules.get_mut(module))
            .next()
            .unwrap_or_else(|| panic!(
                "the module compiler has {library}/{package}/{module} and the workspace \
                 descriptor it was built from does not",
            ));
        held.source = source.into();
    }

    /// Keep the descriptor in step with the module compiler's data, as
    /// [`Self::set_workspace_module`] does for its modules.
    fn set_workspace_data(&mut self, library: &str, package: &str, name: &str, text: &str) {
        let libraries = self.workspace.system_library.iter_mut()
            .chain(self.workspace.user_libraries.iter_mut());
        let held = libraries
            .filter(|held| held.name == library)
            .filter_map(|held| held.packages.get_mut(package))
            .filter_map(|held| held.data.get_mut(name))
            .next()
            .unwrap_or_else(|| panic!(
                "the module compiler has data {library}/{package}/{name} and the workspace \
                 descriptor it was built from does not",
            ));
        held.source = text.into();
    }
}

/// One session: its units, and the executor that runs them.
struct SessionState {
    /// The units submitted so far and what each one's compilation produced.
    session: ScriptSession,
    /// Script executor for running compiled units.
    executor: ScriptExecutor,
    /// The natives `executor`'s native table points to.
    ///
    /// Holding it here decides nothing about lifetimes: the table and the jit
    /// each took a share when the symbols were registered, so neither can be
    /// reached after its code has gone whatever order these fields go in.
    natives: RegisteredNatives,
    /// The [`Modules::changes`] the executor holds the modules of.
    taken: u64,
    /// What the last operation on the units did to each unit it compiled or
    /// re-derived; see [`SessionMut::last_step`].
    last_step: Vec<UnitStep>,
}

impl SessionState {
    fn unit_sources(&self) -> Vec<Source> {
        self.session.unit_sources()
    }

    /// Compile `work` over the modules `compiled`, giving the executor them
    /// first if it holds older ones.
    ///
    /// **It compiles a copy of the session and keeps the copy only once
    /// `work` is done.** A compile that does not finish leaves the session as
    /// the step's write left it, and the compile can be done again: nothing
    /// it did before it stopped is anywhere but in salsa's memos, and the
    /// executor holding newer modules is what the compile again would want.
    /// That is what a session's step needs when another session's write
    /// cancels it.
    fn resume<R>(
        &mut self,
        db: &datafun::Database,
        compiled: &CompiledModules<'_>,
        changes: u64,
        work: impl FnOnce(&mut ScriptCompiler<'_>) -> R,
    ) -> R {
        if self.taken != changes {
            take_modules(&mut self.executor, &mut self.natives, compiled);
            self.taken = changes;
        }
        let mut compiler = compiled
            .script_compiler_resumed(db, self.session.C())
            .expect("the roots were settled on modules that compile");
        let result = work(&mut compiler);
        self.session = compiler.into_session();
        result
    }
}

impl Drop for SessionState {
    fn drop(&mut self) {
        self.executor.destroy_live_values();
    }
}

/// One of an engine's sessions, to act on; see [`Engine::session`].
pub struct SessionMut<'e> {
    shared: &'e mut Shared,
    state: &'e mut SessionState,
    /// The units of every other open session, which the roots are settled on
    /// as well as this one's.
    ///
    /// Nothing can change them while this borrows the engine.
    others: Vec<Source>,
}

/// A session's step past its write: what compiling and running reach.
///
/// Holds the shared half of the engine by `&`, so nothing done through it
/// can write the database.
struct Compiling<'a> {
    shared: &'a Shared,
    state: &'a mut SessionState,
    others: &'a [Source],
}

/// The modules a session requires that do not compile, and what they said.
///
/// **A required module that does not compile is left out of the compile**,
/// with every required module that reaches it, and the rest stay; see
/// [`settle_roots`]. So the engine holds a session whose modules have errors
/// by holding them outside it, and a unit requiring one fails on that require
/// until the module compiles again.
#[derive(Debug, Default)]
pub struct ModuleErrors {
    /// Each required module left out, by path, with the errors of the modules
    /// it reaches that explain why.
    pub broken: BTreeMap<String, Vec<String>>,
    /// The parse, type and ownership diagnostics of the compile that found
    /// them, in the order it reported them, which a caller renders against
    /// [`Engine::db`].
    pub diagnostics: Vec<StoredDiagnostic>,
    /// The lowering errors of that compile, which carry no spans.
    pub lowering_errors: Vec<String>,
}

impl ModuleErrors {
    /// Whether every required module compiles.
    pub fn is_empty(&self) -> bool {
        self.broken.is_empty()
    }
}

/// Put modules that changed in front of the executor, before anything runs.
///
/// A script unit names a module function by `CodeRef::Module`, resolved
/// against the registry the executor holds, so recompiled modules change
/// nothing until it holds theirs. A module that joined may call natives
/// nothing called before. A module keeps its `IrModuleId` across the change,
/// so the units already lowered still call the modules they named.
fn take_modules(
    executor: &mut ScriptExecutor,
    natives: &mut RegisteredNatives,
    compiled: &datafun::driver::CompiledModules<'_>,
) {
    executor.set_module_registry(compiled.shared.module_registry.C());
    *natives = register_natives(&natives.natives, compiled, executor)
        .expect("the natives of modules that compiled are found");
}

/// Narrow the roots to what `units` require, plus `also`, leaving out what does
/// not compile.
///
/// **A required module that does not compile is left out, along with every
/// required module that reaches it**, and the rest stay, so one broken module
/// costs only the units that require it, the way one failed unit costs only
/// the units that read it. A unit requiring a left-out module fails on its
/// `require`, and the engine says why with what this reports.
///
/// The one error no root's leaving fixes is a rider interface's, since every
/// rider is compiled whether or not anything requires it, and that is an error.
fn settle_roots(
    db: &datafun::Database,
    module_compiler: &mut ModuleCompiler,
    units: &[Source],
    also: &[String],
) -> AnyResult<ModuleErrors> {
    module_compiler.narrow_roots_to_sources(db, units, also);
    let mut found = ModuleErrors::default();
    loop {
        let compiled = module_compiler.compile(db);
        if !compiled.has_errors() {
            return Ok(found);
        }
        let Roots::From(roots) = module_compiler.roots().C() else {
            unreachable!("the engine always narrows");
        };

        // What the first compile said, since it holds every module the
        // later ones hold and the ones they left out.
        if found.is_empty() {
            let parse = compiled.get_module_parse_diagnostics(db).into_iter()
                .map(|diagnostic| diagnostic.to_diagnostic(db));
            let types = compiled.get_module_type_diagnostics(db).into_iter()
                .map(|diagnostic| diagnostic.to_diagnostic(db));
            let ownership = compiled.get_module_ownership_diagnostics(db).into_iter()
                .map(|diagnostic| diagnostic.to_diagnostic(db));
            found.diagnostics = parse.chain(types).chain(ownership)
                .map(|diagnostic| diagnostic.to_stored(db))
                .collect();
            found.lowering_errors = compiled.all_lowering_errors();
        }

        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for by_path in [&compiled.parse_errors, &compiled.path_to_errors,
                        &compiled.ownership_errors, &compiled.lowering_errors] {
            for (path, messages) in by_path.iter().filter(|(_, messages)| !messages.is_empty()) {
                errors.entry(path.C()).or_default().extend(messages.iter().cloned());
            }
        }

        let reach = module_reach(db, &compiled);
        let failing: Vec<String> = roots.iter()
            .filter(|root| reach.get(*root).is_some_and(|reached| reached.iter().any(|m| errors.contains_key(m))))
            .cloned()
            .collect();
        if failing.is_empty() {
            bail!("modules no root requires do not compile: {}", compiled.all_errors().join("; "));
        }

        for root in &failing {
            let messages = reach[root].iter()
                .filter_map(|m| errors.get(m))
                .flatten()
                .cloned()
                .collect();
            found.broken.insert(root.C(), messages);
        }
        let kept = roots.into_iter().filter(|root| !failing.contains(root)).collect();
        module_compiler.set_roots(Roots::From(kept));
    }
}

/// Every module and rider each module of a compile reaches by `require`,
/// itself included, by path.
fn module_reach(db: &datafun::Database, compiled: &datafun::driver::CompiledModules<'_>) -> BTreeMap<String, BTreeSet<String>> {
    let parsed = compiled.shared.parsed_graph;
    let requires = parsed.resolved_requires(db);
    let path = |module: bct::module_graph::ModuleId<'_>| module.path(db).C();

    let mut reach = BTreeMap::new();
    for module in parsed.graph(db).iter_modules(db) {
        let mut seen = BTreeSet::new();
        let mut stack = vec![module.id(db)];
        while let Some(next) = stack.pop() {
            if seen.insert(path(next)) {
                stack.extend(requires.get(&next).into_iter().flatten().map(|(_, target)| *target));
                // A rider requires nothing, so it is reached and ends there.
                seen.extend(parsed.resolved_riders(db).get(&next).into_iter().flatten()
                    .map(|(_, rider)| path(rider.module_id)));
            }
        }
        reach.insert(path(module.id(db)), seen);
    }
    reach
}

/// What a caller is shown of a unit that did not compile, while the compiler
/// that compiled it is still alive.
///
/// The compiler's diagnostics are those of the last unit it compiled and do
/// not outlive it, and the engine lets each compiler go once its operation is
/// over, so a caller that renders them has to be handed them here.
pub type OnCompileError<'a> =
    &'a mut dyn FnMut(&ScriptCompiler<'_>, &datafun::driver::ScriptCompilationResult);

/// What one step did to one unit: which of the passes that compile it ran
/// again, and what re-deriving it came to, if it was re-derived.
///
/// A step is any operation on the units, an append included.
#[derive(Debug, Clone, Serialize)]
pub struct UnitStep {
    /// The unit's id.
    pub unit: u32,
    /// The passes salsa ran for the unit in this step.
    pub compiled: Compiled,
    /// What the unit came to, if the step re-derived it: lowered it again
    /// and ran it, or found it failed or blocked. `None` for a unit whose
    /// passes ran and whose lowering did not change, so it was left as it
    /// was.
    pub eval: Option<Eval>,
}

impl UnitStep {
    /// A unit re-derived, before what ran for it is merged in.
    fn rederived(unit: u32, eval: Eval) -> UnitStep {
        UnitStep { unit, compiled: Compiled::default(), eval: Some(eval) }
    }
}

/// Which of the passes that compile a unit ran for it in a step.
///
/// A unit is compiled again when any of them ran: parse and name resolution,
/// typecheck, and ownership. Lowering is not one of them; it is what
/// re-deriving a unit does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Compiled {
    pub parse: bool,
    pub typecheck: bool,
    pub ownership: bool,
}

impl Compiled {
    /// Whether any pass ran.
    pub fn any(&self) -> bool {
        self.parse || self.typecheck || self.ownership
    }
}


impl Engine {
    pub fn new(sys: SystemLibrary) -> AnyResult<Engine> {
        let workspace = WorkspaceDescriptor::from_system_library(&sys);
        Engine::with_workspace(sys, workspace)
    }

    /// The same, with the workspace's own library, if it has one.
    pub fn with_local(sys: SystemLibrary, local: Option<PackageLibrary>) -> AnyResult<Engine> {
        let workspace = WorkspaceDescriptor::from_system_library(&sys);
        let workspace = match local {
            Some(local) => workspace.with_user_library(local)?,
            None => workspace,
        };
        Engine::with_workspace(sys, workspace)
    }

    /// The same, over a workspace that holds more than the system library.
    ///
    /// The library still comes separately because it carries the addresses of
    /// the native rider functions linked into this binary, which a descriptor
    /// has no way to say.
    pub fn with_workspace(
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
    ) -> AnyResult<Engine> {
        Engine::with_options(sys, workspace, datafun::DebugOutputMode::Disabled, Vec::new())
    }

    /// The same, saying where `debuglog` goes and which modules to compile
    /// whether or not a unit requires them.
    ///
    /// **The engine decides the module compiler's roots**: `also_roots`, and whatever
    /// the sessions' units require, which starts as nothing. Any of them that
    /// does not compile is left out, and [`Self::module_errors`] says so; the
    /// engine starts regardless.
    pub fn with_options(
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
        debug_output: datafun::DebugOutputMode,
        also_roots: Vec<String>,
    ) -> AnyResult<Engine> {
        Engine::with_options_in(datafun::Database::default(), sys, workspace, debug_output, also_roots)
    }

    /// The same as [`Engine::with_options`], in a database the caller made.
    ///
    /// For a database that records what salsa runs
    /// (`datafun::Database::recording`), which is how a test sees which units
    /// were typechecked again.
    pub fn in_database(
        db: datafun::Database,
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
        also_roots: Vec<String>,
    ) -> AnyResult<Engine> {
        Engine::with_options_in(db, sys, workspace, datafun::DebugOutputMode::Disabled, also_roots)
    }

    fn with_options_in(
        db: datafun::Database,
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
        debug_output: datafun::DebugOutputMode,
        also_roots: Vec<String>,
    ) -> AnyResult<Engine> {
        // An engine needs somewhere to build riders, so a caller that did not
        // say gets this installation's work dir rather than an engine that
        // cannot start. It only matters with `DATALOVE_BUILD_SYS_RIDERS` set,
        // where the riders are built rather than taken from this binary --
        // which is exactly the arrangement a caller is least likely to have
        // thought about.
        let workspace = match workspace.work_dir {
            Some(_) => workspace,
            None => workspace.with_work_dir(datalove_paths::work_dir()?),
        };

        let (mut module_compiler, natives) = workspace.to_compiler_with_natives(&db, &sys.natives);
        // Settled here rather than by the first session, because a rider
        // interface that does not compile is the one error that stops the
        // engine starting, and this is where it is reported.
        let errors = settle_roots(&db, &mut module_compiler, &[], &also_roots)?;

        let modules = Modules { module_compiler, also_roots, errors, changes: 0 };
        let shared = Shared { db, workspace, natives, debug_output, modules: RefCell::new(modules), dispatcher: None };
        let first = SessionId(0);
        let sessions = BTreeMap::from([(first, shared.start_session(&[])?)]);
        Ok(Engine { shared, sessions, first, next_session: 1 })
    }

    /// Run the calls the sessions' units make through a dispatcher from
    /// `make`, which may compile them, as the JIT does: one for each session,
    /// made when the session starts and kept across every change to its
    /// units, so that what it compiled for code since replaced is the
    /// dispatcher's to drop. A reset starts the session with a new one.
    ///
    /// Before any unit is entered, since the sessions start again with it.
    pub fn use_dispatcher(&mut self, make: impl Fn() -> Box<dyn CallDispatcher> + 'static) -> AnyResult<()> {
        assert!(
            self.sessions.values().all(|state| state.session.unit_ids().is_empty()),
            "a dispatcher is installed before any unit is entered",
        );
        self.shared.dispatcher = Some(Box::new(make));
        for state in self.sessions.values_mut() {
            *state = self.shared.start_session(&[])?;
        }
        Ok(())
    }

    /// The dispatcher the first session's calls ran through, taken out of
    /// it, for what it says it did; the session runs on without one.
    pub fn take_dispatcher(&mut self) -> Option<Box<dyn CallDispatcher>> {
        let first = self.first;
        self.sessions.get_mut(&first).X().executor.take_dispatcher()
    }

    /// The required modules that do not compile; see [`ModuleErrors`].
    ///
    /// Required by any open session: the sessions share the modules, so a
    /// module one of them requires and that does not compile is left out for
    /// all of them.
    pub fn module_errors(&self) -> Ref<'_, ModuleErrors> {
        Ref::map(self.shared.modules.borrow(), |modules| &modules.errors)
    }

    /// The database the sessions are compiled in, which is what the stored
    /// diagnostics of [`Self::module_errors`] are read back against.
    pub fn db(&self) -> &datafun::Database {
        &self.shared.db
    }

    /// The workspace the sessions compile against.
    pub fn workspace(&self) -> &WorkspaceDescriptor {
        &self.shared.workspace
    }

    /// The module compiler the modules are compiled by, for finding which file a
    /// diagnostic points into.
    pub fn module_compiler(&self) -> Ref<'_, ModuleCompiler> {
        Ref::map(self.shared.modules.borrow(), |modules| &modules.module_compiler)
    }

    /// The session the engine started with, which the methods on `Engine`
    /// itself act on.
    pub fn first_session(&self) -> SessionId {
        self.first
    }

    /// The open sessions, in the order they were opened.
    pub fn session_ids(&self) -> Vec<SessionId> {
        self.sessions.keys().copied().collect()
    }

    /// Every open session's units.
    fn all_units(&self) -> Vec<Source> {
        self.sessions.values().flat_map(SessionState::unit_sources).collect()
    }

    /// Open a session with no units, over the modules the engine's sessions
    /// compile against.
    pub fn open_session(&mut self) -> SessionId {
        let id = SessionId(self.next_session);
        self.next_session += 1;
        let state = self.shared.start_session(&self.all_units())
            .expect("the natives registered when the engine started are found");
        self.sessions.insert(id, state);
        id
    }

    /// Close session `id`, destroying the values its units hold.
    ///
    /// The modules only it required leave the compile, and the errors of
    /// those that did not compile leave [`Self::module_errors`]. The session
    /// the engine started with is never closed.
    pub fn close_session(&mut self, id: SessionId) {
        assert_ne!(id, self.first, "the session the engine started with is never closed");
        self.sessions.remove(&id).unwrap_or_else(|| panic!("no session {id} is open"));
        let units = self.all_units();
        self.shared.modules.get_mut().settle(&self.shared.db, &units);
    }

    /// Session `id`, to act on.
    pub fn session(&mut self, id: SessionId) -> SessionMut<'_> {
        let others = self.sessions.iter()
            .filter(|(other, _)| **other != id)
            .flat_map(|(_, state)| state.unit_sources())
            .collect();
        let state = self.sessions.get_mut(&id).unwrap_or_else(|| panic!("no session {id} is open"));
        SessionMut { shared: &mut self.shared, state, others }
    }

    fn first_state(&self) -> &SessionState {
        &self.sessions[&self.first]
    }

    fn first_mut(&mut self) -> SessionMut<'_> {
        self.session(self.first)
    }

    /// Change one module's source and re-derive the script units it reaches,
    /// in every session.
    ///
    /// The module-shaped half of [`SessionMut::edit_unit`], and it needs one
    /// step that unit editing does not: an executor calls a module function
    /// through the registry it was handed, so the recompiled modules have to
    /// be put in front of every session's before anything is run again.
    ///
    /// A unit is re-derived when one of its imports resolves into this module,
    /// when it requires a module that requires this one, when it requires a
    /// module the edit broke or fixed, or when it uses what such a unit
    /// provides. Returns what the step did to each session's units, by
    /// session; each session's [`SessionMut::last_step`] is its own of them.
    ///
    /// **An edit that does not compile is kept.** The module is left out of
    /// the compile, with the modules that require it; the units requiring any
    /// of them fail, saying why; [`Self::module_errors`] has its errors; and an
    /// edit that makes it compile brings it and them back. A module no unit
    /// requires is not compiled, so an edit to one reports nothing.
    pub fn edit_module(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) -> BTreeMap<SessionId, Vec<UnitStep>> {
        let (mut reports, ran) = unit_analysis::collect(|| self.edit_module_in_step(library, package, module, source));
        self.session_ids().into_iter()
            .map(|id| {
                let mut session = self.session(id);
                session.record_step(reports.remove(&id).expect("every session is re-derived"), ran.C());
                (id, session.last_step().to_vec())
            })
            .collect()
    }

    fn edit_module_in_step(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) -> BTreeMap<SessionId, Vec<UnitStep>> {
        // The write.
        let Shared { db, modules, .. } = &mut self.shared;
        let module_compiler = &mut modules.get_mut().module_compiler;
        assert!(
            module_compiler.contains_module(library, package, module),
            "no module {library}/{package}/{module} in this engine",
        );
        module_compiler.update_source(db, library, package, module, source);
        self.shared.set_workspace_module(library, package, module, source);
        let path = format!("{library}/{package}/{module}");

        // The compile.
        let shared = &self.shared;
        let broken_before: BTreeSet<String> = shared.modules.borrow().errors.broken.keys().cloned().collect();
        let compiled = shared.compile_modules(&self.all_units());
        let mut modules = shared.modules.borrow_mut();
        // The roots may not have moved, and the modules have changed anyway.
        modules.changes += 1;
        let changes = modules.changes;

        // The modules whose units the edit reaches: this one, the ones that
        // reach it, and the ones it broke or fixed.
        let mut reached: BTreeSet<String> = module_reach(&shared.db, &compiled).into_iter()
            .filter(|(_, reach)| reach.contains(&path))
            .map(|(module, _)| module)
            .collect();
        reached.insert(path);
        let broken_after: BTreeSet<String> = modules.errors.broken.keys().cloned().collect();
        let broken_or_fixed: Vec<String> = broken_before.symmetric_difference(&broken_after).cloned().collect();
        reached.extend(broken_or_fixed.iter().cloned());
        drop(modules);

        // A unit whose require failed is found here too: its imports are read
        // as they now resolve, and they resolve into a module that compiles.
        let reached: Vec<String> = reached.into_iter().collect();

        let rederived: BTreeMap<SessionId, _> = self.sessions.iter_mut()
            .map(|(id, state)| {
                let rederived = state.resume(&shared.db, &compiled, changes, |compiler| {
                    compiler.relower_module_reach(&reached, &broken_or_fixed)
                });
                (*id, rederived)
            })
            .collect();

        // The run.
        rederived.into_iter()
            .map(|(id, rederived)| {
                let mut session = self.session(id);
                (id, session.compiling().rerun_and_explain(rederived))
            })
            .collect()
    }

    /// Change one data file's text and re-derive the script units it reaches,
    /// in every session.
    ///
    /// The data-shaped sibling of [`Self::edit_module`]. A unit is re-derived
    /// when it requires the data, when it imports from a module whose consts
    /// reach it, or when it uses what such a unit provides; the units that
    /// require it are recompiled, since the data is compiled into them as a
    /// const. Returns what the step did to each session's units, by session.
    pub fn edit_data(
        &mut self,
        library: &str,
        package: &str,
        name: &str,
        text: &str,
    ) -> BTreeMap<SessionId, Vec<UnitStep>> {
        let (mut reports, ran) = unit_analysis::collect(|| self.edit_data_in_step(library, package, name, text));
        self.session_ids().into_iter()
            .map(|id| {
                let mut session = self.session(id);
                session.record_step(reports.remove(&id).expect("every session is re-derived"), ran.C());
                (id, session.last_step().to_vec())
            })
            .collect()
    }

    fn edit_data_in_step(
        &mut self,
        library: &str,
        package: &str,
        name: &str,
        text: &str,
    ) -> BTreeMap<SessionId, Vec<UnitStep>> {
        // The write.
        let path = format!("{library}/{package}/{name}");
        let Shared { db, modules, .. } = &mut self.shared;
        let module_compiler = &mut modules.get_mut().module_compiler;
        assert!(module_compiler.data_source(&path).is_some(), "no data {path} in this engine");
        module_compiler.update_data(db, library, package, name, text);
        self.shared.set_workspace_data(library, package, name, text);

        // The compile.
        let shared = &self.shared;
        let compiled = shared.compile_modules(&self.all_units());
        let db = &shared.db;
        // The modules whose consts name the data, and the modules that reach
        // one of them.
        let typecheck = compiled.shared.graph_typecheck;
        let requiring: BTreeSet<String> = typecheck.module_results(db).iter_unordered()
            .filter(|(_, result)| result.data_files(db).contains_key(&path))
            .map(|(module, _)| module.path(db).C())
            .collect();
        let reached: Vec<String> = module_reach(db, &compiled).into_iter()
            .filter(|(_, reach)| !reach.is_disjoint(&requiring))
            .map(|(module, _)| module)
            .collect();
        let changes = {
            let mut modules = shared.modules.borrow_mut();
            // A module's consts hold the data, so the modules have changed.
            if !reached.is_empty() {
                modules.changes += 1;
            }
            modules.changes
        };

        let rederived: BTreeMap<SessionId, _> = self.sessions.iter_mut()
            .map(|(id, state)| {
                let rederived = state.resume(&shared.db, &compiled, changes, |compiler| {
                    compiler.relower_data_reach(&path, &reached)
                });
                (*id, rederived)
            })
            .collect();

        // The run.
        rederived.into_iter()
            .map(|(id, rederived)| {
                let mut session = self.session(id);
                (id, session.compiling().rerun_and_explain(rederived))
            })
            .collect()
    }

    // What follows acts on the session the engine started with; see
    // `SessionMut` for each.

    pub fn reset(&mut self) {
        self.first_mut().reset()
    }

    pub fn unit_sources(&self) -> Vec<Source> {
        self.first_state().unit_sources()
    }

    pub fn unit_ids(&self) -> Vec<u32> {
        self.first_state().session.unit_ids()
    }

    pub fn next_id(&self) -> u32 {
        self.first_state().session.next_id()
    }

    pub fn set_next_id(&mut self, next: u32) {
        self.first_mut().set_next_id(next)
    }

    pub fn eval(&mut self, unit: Unit) -> Eval {
        self.first_mut().eval(unit)
    }

    pub fn eval_reporting(&mut self, unit: Unit, on_error: OnCompileError<'_>) -> Eval {
        self.first_mut().eval_reporting(unit, on_error)
    }

    pub fn last_step(&self) -> &[UnitStep] {
        &self.first_state().last_step
    }

    pub fn edit_unit(&mut self, id: u32, source: &str) -> Vec<UnitStep> {
        self.first_mut().edit_unit(id, source)
    }

    pub fn truncate_units(&mut self, n: usize) -> Vec<UnitStep> {
        self.first_mut().truncate_units(n)
    }

    pub fn delete_unit(&mut self, id: u32) -> Vec<UnitStep> {
        self.first_mut().delete_unit(id)
    }

    pub fn insert_unit(&mut self, at: usize, source: &str, is_expr: bool) -> Vec<UnitStep> {
        self.first_mut().insert_unit(at, source, is_expr)
    }

    pub fn move_unit(&mut self, id: u32, to: usize) -> Vec<UnitStep> {
        self.first_mut().move_unit(id, to)
    }

    pub fn restore_unit(&mut self, at: usize, id: u32, source: &str, is_expr: bool) -> Vec<UnitStep> {
        self.first_mut().restore_unit(at, id, source, is_expr)
    }

    pub fn get_environment(&mut self) -> Vec<EnvBinding> {
        self.first_mut().get_environment()
    }

    pub fn unit_diagnostics(&mut self, id: u32, name: &str, style: bct::render::TextStyle) -> String {
        self.first_mut().unit_diagnostics(id, name, style)
    }

    pub fn unit_count(&self) -> usize {
        self.first_state().session.unit_ids().len()
    }

    pub fn unit_accesses(&self, id: u32) -> datafun::driver::UnitAccesses {
        self.first_state().session.unit_accesses(id)
    }

    pub fn unit_bindings(&mut self, id: u32) -> Vec<(String, Option<String>)> {
        self.first_mut().unit_bindings(id)
    }
}

/// A session's steps.
///
/// **Each is a write, a compile and a run, in that order**, and only the
/// write is done here: everything after it goes through [`Compiling`], which
/// cannot write the database. A step that writes nothing -- an append, a
/// truncation -- goes straight to it. The executor is touched only once the
/// compile is done, so a compile done twice finds it as it was.
impl SessionMut<'_> {
    /// The step past its write.
    fn compiling(&mut self) -> Compiling<'_> {
        Compiling { shared: self.shared, state: self.state, others: &self.others }
    }

    /// Start the session again, forgetting every unit in it.
    ///
    /// The modules stay compiled, and the riders built and loaded. The other
    /// sessions are untouched.
    pub fn reset(&mut self) {
        self.compiling().reset()
    }

    /// The session's units' sources, in order, which is how
    /// [`datalove_ct::unit_analysis`] names them.
    pub fn unit_sources(&self) -> Vec<Source> {
        self.state.unit_sources()
    }

    /// The session's units' ids, in order.
    ///
    /// **A unit's id is its for as long as it is in the session**, wherever
    /// units are inserted or deleted around it, and every operation on a
    /// unit names it by its id. Ids count from 1 in the order units are made
    /// and are not given twice, unless a unit is put back under the one it
    /// had; see [`Self::restore_unit`]. Each session numbers its own.
    pub fn unit_ids(&self) -> Vec<u32> {
        self.state.session.unit_ids()
    }

    /// Where unit `id` sits in the session, which must hold it.
    fn position(&self, id: u32) -> usize {
        self.state.session.position_of(id).unwrap_or_else(|| panic!("no unit {id} in the session"))
    }

    /// The id the next unit made is given, by appending or inserting.
    pub fn next_id(&self) -> u32 {
        self.state.session.next_id()
    }

    /// Give the next unit made the id `next`, which no unit in the session
    /// has, nor any later one.
    ///
    /// For a driver that numbers units as it numbered them before, as the
    /// REPL does when it drops changes it took back: the ids the units those
    /// made had are given again.
    pub fn set_next_id(&mut self, next: u32) {
        self.state.session.set_next_id(next);
    }

    /// Compile and run a unit as the session's next.
    ///
    /// A panic resets the session, and is reported as [`Eval::Crashed`].
    pub fn eval(&mut self, unit: Unit) -> Eval {
        self.eval_reporting(unit, &mut |_, _| {})
    }

    /// The same, showing `on_error` a unit that does not compile.
    pub fn eval_reporting(&mut self, unit: Unit, on_error: OnCompileError<'_>) -> Eval {
        let is_expr = unit.is_expression();
        let source = match unit {
            Unit::Fragment(source) | Unit::Expression(source) => source,
        };
        let (result, ran) = unit_analysis::collect(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.compiling().eval_new_unit(source, is_expr, on_error)
            }))
        });

        let eval = match result {
            Ok(eval_result) => eval_result,
            Err(panic) => {
                self.reset();
                Eval::Crashed(fmt!("Eval panic: {}", crate::panic_message(&*panic)))
            }
        };
        // The unit appended is the last, unless it took the session with it.
        let step = match eval.resets() {
            true => Vec::new(),
            false => vec![UnitStep::rederived(self.state.session.last_unit_id(), eval.C())],
        };
        self.record_step(step, ran);
        eval
    }

    /// What the last operation on the units did to each unit it compiled or
    /// re-derived, in the session's order.
    ///
    /// The same as what an edit, a delete or an insert returns, and for an
    /// appended unit the unit itself and any earlier unit it made compile
    /// again. A unit is named by its id; a runtime error, which empties the
    /// session, leaves nothing to name.
    pub fn last_step(&self) -> &[UnitStep] {
        &self.state.last_step
    }

    /// Run an operation on the units, and say which passes it ran for each
    /// unit beside what it re-derived.
    fn step(&mut self, operation: impl FnOnce(&mut Self) -> Vec<UnitStep>) -> Vec<UnitStep> {
        let (reports, ran) = unit_analysis::collect(|| operation(self));
        self.record_step(reports, ran);
        self.state.last_step.C()
    }

    /// Merge the passes that ran into a step's reports, adding a report for
    /// each unit compiled again and not re-derived, and keep the result.
    ///
    /// The passes name units by source, and the sources are those after the
    /// step: an edited unit keeps its source, an inserted one is new, and a
    /// removed one names nothing any more. Another session's units are other
    /// sources, so a pass that ran for one of them names nothing here.
    fn record_step(&mut self, mut reports: Vec<UnitStep>, ran: Vec<(Pass, Source)>) {
        let sources = self.unit_sources();
        let ids = self.unit_ids();
        for (pass, source) in ran {
            let Some(unit) = sources.iter().position(|unit| *unit == source).map(|index| ids[index]) else {
                continue;
            };
            let index = match reports.iter().position(|report| report.unit == unit) {
                Some(index) => index,
                None => {
                    reports.push(UnitStep { unit, compiled: Compiled::default(), eval: None });
                    reports.len() - 1
                }
            };
            let compiled = &mut reports[index].compiled;
            match pass {
                Pass::Parse => compiled.parse = true,
                Pass::Typecheck => compiled.typecheck = true,
                Pass::Ownership => compiled.ownership = true,
            }
        }
        reports.sort_by_key(|report| ids.iter().position(|id| *id == report.unit));
        self.state.last_step = reports;
    }

    /// Change unit `id`'s text and re-derive what the edit reaches.
    ///
    /// The units the edit does not reach keep the lowering and the frame they
    /// already have, which is sound because a unit that uses nothing the
    /// edited unit provides holds no reference into it. See
    /// `botdocs/script-reactivity-architecture.md`.
    ///
    /// Returns what the step did to each unit it compiled or re-derived, in
    /// the session's order; see [`Self::last_step`]. A unit whose re-lowering
    /// failed is reported as an error and stays, with its frame destroyed and
    /// its bindings gone from the environment, until an edit makes it compile
    /// again.
    pub fn edit_unit(&mut self, id: u32, source: &str) -> Vec<UnitStep> {
        self.step(|session| session.edit_unit_in_step(id, source))
    }

    fn edit_unit_in_step(&mut self, id: u32, source: &str) -> Vec<UnitStep> {
        let at = self.position(id);
        let sources = self.unit_sources();
        sources[at].set_text(&mut self.shared.db).to(source.S());

        let mut step = self.compiling();
        let rederived = step.compile_units(&sources, |compiler| compiler.relower_reach(at));
        step.rerun_and_explain(rederived)
    }

    /// Drop the units from position `n` on.
    ///
    /// Nothing sits after what goes, so usually nothing is re-derived or
    /// re-executed. Their bindings leave the environment and the values they
    /// held are destroyed. A unit that goes having taken or written an
    /// earlier unit's binding reruns the unit that set the binding, to build
    /// it as it was, and the units after that one that took or wrote it too.
    pub fn truncate_units(&mut self, n: usize) -> Vec<UnitStep> {
        self.step(|session| session.truncate_units_in_step(n))
    }

    fn truncate_units_in_step(&mut self, n: usize) -> Vec<UnitStep> {
        let mut units = self.unit_sources();
        let count = units.len();
        assert!(n <= count, "cannot truncate to {n} units; the session has {count}");
        units.truncate(n);

        let mut step = self.compiling();
        let rederived = step.compile_units(&units, |compiler| compiler.truncate_units(n));
        step.state.executor.truncate_units(n);
        step.rerun_and_explain(rederived)
    }

    /// Take unit `id` out of the session, and re-derive what that reaches.
    ///
    /// **An edit to nothing, and its reach is an edit's**: the units after it
    /// that used what it provided, and the units that use what those
    /// provide. Every other unit keeps its lowering and its frame, since no
    /// unit's id moves. A unit that read what the removed unit provided stops
    /// compiling and stays, as any unit that fails does. And if the removed
    /// unit took or wrote an earlier unit's binding, the unit that set it is
    /// rerun to build it as it was.
    pub fn delete_unit(&mut self, id: u32) -> Vec<UnitStep> {
        self.step(|session| session.delete_unit_in_step(id))
    }

    fn delete_unit_in_step(&mut self, id: u32) -> Vec<UnitStep> {
        let at = self.position(id);
        let removed = self.state.session.delete_unit(&mut self.shared.db, at);
        let units = self.unit_sources();

        let mut step = self.compiling();
        let rederived = step.compile_units(&units, |compiler| compiler.relower_deleted(at, removed));
        step.state.executor.remove_unit(id);
        step.rerun_and_explain(rederived)
    }

    /// Move unit `id` to position `to`, counted in the session as it is
    /// after the move, and re-derive what that reaches.
    ///
    /// **A deletion and an insertion in one step**: the units its leaving
    /// reaches, as for [`Self::delete_unit`], and the unit itself and what
    /// its arriving reaches, as for [`Self::insert_unit`]. The session as it
    /// would stand between the two is never run, so a runtime error there
    /// cannot empty it. The unit keeps its id, and every other unit keeps
    /// its place relative to the rest.
    pub fn move_unit(&mut self, id: u32, to: usize) -> Vec<UnitStep> {
        self.step(|session| session.move_unit_in_step(id, to))
    }

    fn move_unit_in_step(&mut self, id: u32, to: usize) -> Vec<UnitStep> {
        let from = self.position(id);
        self.state.session.move_unit(&mut self.shared.db, from, to);
        let units = self.unit_sources();

        let mut step = self.compiling();
        let rederived = step.compile_units(&units, |compiler| compiler.relower_moved(from, to));
        step.state.executor.move_unit(id, to);
        step.rerun_and_explain(rederived)
    }

    /// Splice a new unit in at position `at`, and re-derive what it reaches.
    ///
    /// `is_expr` says whether the text is a bare expression or a fragment of
    /// statements; nothing in the text decides it, which is why the appending
    /// path has two entry points rather than one.
    ///
    /// **An edit from nothing, and its reach is an edit's**: the new unit, the
    /// units after it that use what it provides, and so on, and every other
    /// unit keeps its lowering and its frame. The new unit stays whether or
    /// not it compiles, and is given the next id.
    pub fn insert_unit(
        &mut self,
        at: usize,
        source: &str,
        is_expr: bool,
    ) -> Vec<UnitStep> {
        self.step(|session| session.insert_unit_in_step(at, source, is_expr, None))
    }

    /// The same, giving the new unit the id `id`, which no unit in the
    /// session has.
    ///
    /// What putting back a unit that was taken out needs, as undoing a
    /// delete does: under the id it had.
    pub fn restore_unit(
        &mut self,
        at: usize,
        id: u32,
        source: &str,
        is_expr: bool,
    ) -> Vec<UnitStep> {
        self.step(|session| session.insert_unit_in_step(at, source, is_expr, Some(id)))
    }

    fn insert_unit_in_step(&mut self, at: usize, source: &str, is_expr: bool, id: Option<u32>) -> Vec<UnitStep> {
        let source = Source::new(&self.shared.db, source.S());
        let id = self.state.session.insert_unit(&mut self.shared.db, at, source, is_expr, id);
        let units = self.unit_sources();

        let mut step = self.compiling();
        let rederived = step.compile_units(&units, |compiler| compiler.relower_inserted(at));
        step.state.executor.insert_unit(at, id);
        step.rerun_and_explain(rederived)
    }

    /// The bindings the session offers, the nearest of each name, sorted by
    /// name.
    pub fn get_environment(&mut self) -> Vec<EnvBinding> {
        self.compiling().get_environment()
    }

    /// Unit `id`'s parse, type and ownership diagnostics as they stand,
    /// rendered as text in `style` quoting the unit as `name`; empty for a
    /// unit with none.
    pub fn unit_diagnostics(&mut self, id: u32, name: &str, style: bct::render::TextStyle) -> String {
        let at = self.position(id);
        let units = self.unit_sources();
        self.compiling().compile_units(&units, |compiler| compiler.render_unit_diagnostics(at, name, style))
    }

    /// How many units the session has, failed ones included.
    pub fn unit_count(&self) -> usize {
        self.unit_ids().len()
    }

    /// What unit `id` provides and does to earlier units' bindings.
    pub fn unit_accesses(&self, id: u32) -> datafun::driver::UnitAccesses {
        self.state.session.unit_accesses(id)
    }

    /// What unit `id` binds and what each binding holds now, `None` for one
    /// given away.
    ///
    /// One unit's frame, where [`Self::get_environment`] is the nearest
    /// binding of each name.
    pub fn unit_bindings(&mut self, id: u32) -> Vec<(String, Option<String>)> {
        self.state.executor.unit_bindings(id)
    }
}

impl Compiling<'_> {
    /// Start the session again, with no units.
    fn reset(&mut self) {
        // An empty session requires nothing, so the roots go back to what
        // the other sessions require, less any a module edit has broken
        // since. The old executor destroys its values as it goes.
        *self.state = self.shared.start_session(self.others)
            .expect("the natives registered when the engine started are found");
    }

    /// Compile the modules and then `work` over them.
    ///
    /// Everything that needs a compiler goes through here, because a compiler
    /// borrows the database and the engine owns it.
    ///
    /// **The modules compiled are the ones `units` require**, `units` being
    /// the session's units as they will stand once `work` is done, so a unit
    /// that requires a module nothing required before compiles against it, and
    /// one that goes takes its modules with it -- unless another session
    /// requires them too. Required modules that do not compile are left out;
    /// see [`settle_roots`]. The session is compiled on a copy; see
    /// [`SessionState::resume`].
    fn compile_units<R>(
        &mut self,
        units: &[Source],
        work: impl FnOnce(&mut ScriptCompiler<'_>) -> R,
    ) -> R {
        let all: Vec<Source> = self.others.iter().chain(units).copied().collect();
        let compiled = self.shared.compile_modules(&all);
        let changes = self.shared.modules.borrow().changes;
        self.state.resume(&self.shared.db, &compiled, changes, work)
    }

    /// Say why units that failed on a module that does not compile failed.
    ///
    /// A unit requiring a left-out module fails on its `require` as though the
    /// module did not exist, which is true of the compile and misleading to
    /// read, so its error becomes the module's.
    fn explain(&self, unit: Source, eval: Eval) -> Eval {
        let Eval::NotCompiled(error) = eval else {
            return eval;
        };
        let modules = self.shared.modules.borrow();
        let reasons: Vec<String> = modules.module_compiler.required_modules(&self.shared.db, unit).into_iter()
            .filter_map(|path| modules.errors.broken.get(&path).map(|errors| {
                fmt!("module `{path}` does not compile: {}", errors.join("; "))
            }))
            .collect();
        if reasons.is_empty() {
            Eval::NotCompiled(error)
        } else {
            Eval::NotCompiled(reasons.join("\n"))
        }
    }

    /// Run the units a compile re-derived, and say why those that failed on
    /// a module failed.
    fn rerun_and_explain(&mut self, rederived: Vec<(usize, ScriptCompilationResult)>) -> Vec<UnitStep> {
        let reports = self.rerun(rederived);
        // A unit that failed at run time took the session with it, so the
        // units reported before it have no sources left to explain against.
        if reports.iter().any(|report| report.eval.as_ref().is_some_and(Eval::resets)) {
            return reports;
        }
        let sources = self.state.unit_sources();
        let session = &self.state.session;
        reports.into_iter()
            .map(|UnitStep { unit, compiled, eval }| {
                let at = session.position_of(unit).expect("a reported unit is in the session");
                UnitStep { unit, compiled, eval: eval.map(|eval| self.explain(sources[at], eval)) }
            })
            .collect()
    }

    /// Compile and run a fragment or an expression as the next unit.
    ///
    /// **A unit that fails stays in the session**, whether it did not compile,
    /// returned early, or read from a unit that did either, and offers no
    /// bindings until an edit makes it run to its end. Only a runtime error
    /// takes the session with it.
    fn eval_new_unit(&mut self, source: String, is_expr: bool, on_error: OnCompileError<'_>) -> Eval {
        let source = Source::new(&self.shared.db, source);
        let mut units = self.state.unit_sources();
        units.push(source);
        let compiled = self.compile_units(&units, |compiler| {
            let compiled = compiler.compile_source(source, is_expr);
            if compiled.first_error().is_some() {
                on_error(compiler, &compiled);
            }
            compiled
        });

        if let Some(error) = compiled.first_error() {
            let id = self.state.session.last_unit_id();
            self.state.executor.skip_unit(id);
            return self.explain(source, Eval::NotCompiled(error));
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("a unit that compiled without errors has ir");
        let (ty, output) = self.execute(ir_unit, is_expr);
        if is_runtime_error(&output) {
            return self.failed(output);
        }
        self.outcome(ir_unit, ty, output)
    }

    /// Run a unit as the session's next: an expression for its value, and a
    /// fragment for what it binds.
    fn execute(&mut self, ir_unit: &IrCodeUnit, is_expr: bool) -> (Option<String>, String) {
        if is_expr {
            self.state.executor.execute_expr(ir_unit)
        } else {
            (None, self.state.executor.execute_fragment(ir_unit))
        }
    }

    /// A unit failed at run time: the executor has freed everything the
    /// runtime allocated and emptied its session (see
    /// `ScriptExecutor::failed`), so the compiled session starts again too.
    fn failed(&mut self, output: String) -> Eval {
        self.reset();
        Eval::RuntimeError(output)
    }

    /// What a unit that has just been run, or not run, came to.
    ///
    /// `ty` is the type an expression unit's value came to, which is how one
    /// that ran is told from a fragment.
    fn outcome(&mut self, ir_unit: &IrCodeUnit, ty: Option<String>, output: String) -> Eval {
        let id = ir_unit.script_context()
            .and_then(|ctx| ctx.unit)
            .expect("a compiled unit is numbered");
        match self.state.executor.unit_status(id).C() {
            // The executor's type is the IR's; the value's type is the
            // language's, from the typechecker.
            UnitStatus::Ran => match ty {
                Some(_) => {
                    let ty = self.state.session.value_type(id).S();
                    Eval::Value(EvalExpr { ty, value: output })
                }
                None => self.report_bindings(ir_unit),
            },
            UnitStatus::ReturnedEarly => Eval::ReturnedEarly(output),
            UnitStatus::Blocked { on } => Eval::Blocked { on },
            UnitStatus::NotCompiled => unreachable!("unit {id} compiled"),
        }
    }

    /// Report the bindings a fragment provides, as the compiler recorded
    /// them, with what each holds in the fragment's own frame.
    ///
    /// Its own, and not the nearest binding of each name: a unit rerun
    /// before a later one that shadows a name would otherwise report the
    /// later unit's binding, which may no longer be the later unit's either,
    /// since it has not been rerun yet.
    fn report_bindings(&mut self, ir_unit: &IrCodeUnit) -> Eval {
        let ctx = ir_unit.script_context().expect("a compiled fragment is a script unit");
        let id = ctx.unit.expect("a compiled unit is numbered");
        let exports = ctx.exports.C();
        let held = self.state.executor.unit_bindings(id);
        let value = |name: &str| {
            held.iter().rev().find(|(bound, _)| bound == name)
                .map(|(_, value)| value.C())
                .expect("the unit's frame holds what it exports")
        };
        let ty = |name: &str| self.state.session.type_of(id, name).S();
        Eval::Ran(exports.iter()
            .map(|(name, binding)| match binding {
                ExportBinding::Function(_) => EvalBinding::Function { name: name.C() },
                ExportBinding::Value(_) => EvalBinding::Value { name: name.C(), ty: ty(name), value: value(name) },
                ExportBinding::Slot(_) => EvalBinding::Slot { name: name.C(), ty: ty(name), value: value(name) },
            })
            .collect())
    }

    /// Run each re-lowered unit again in place, reporting what it came to.
    ///
    /// `rederived` names the units by position, as the compiler does, and
    /// the reports by id.
    fn rerun(
        &mut self,
        rederived: Vec<(usize, ScriptCompilationResult)>,
    ) -> Vec<UnitStep> {
        let ids = self.state.session.unit_ids();
        let mut reports = Vec::new();
        for (index, compiled) in rederived {
            let id = ids[index];
            let eval = match (compiled.first_error(), &compiled.ir_unit) {
                (Some(error), _) => {
                    self.state.executor.clear_unit(id);
                    Eval::NotCompiled(error)
                }
                (None, None) => unreachable!("a unit that compiled without errors has ir"),
                (None, Some(ir_unit)) => {
                    let (ty, output) = self.state.executor.rerun_unit(id, ir_unit);
                    if is_runtime_error(&output) {
                        // The session is gone, so there is nothing left
                        // for the rest to run against.
                        reports.push(UnitStep::rederived(id, self.failed(output)));
                        break;
                    }
                    // An expression unit reports the value it now comes to;
                    // a fragment reports the bindings it defines.
                    self.outcome(ir_unit, ty, output)
                }
            };
            reports.push(UnitStep::rederived(id, eval));
        }
        reports
    }

    /// The type of the nearest binding of `name`, as datalove writes it.
    ///
    /// The executor holds the values, and knows only how the IR lays them
    /// out; what the language says they are is the typechecker's, kept with
    /// the unit that provides the name.
    fn type_of(&self, name: &str) -> String {
        let unit = self.state.executor.provider_of(name).expect("a binding the executor offers has a provider");
        self.state.session.type_of(unit, name).S()
    }

    /// The bindings the session offers, the nearest of each name, sorted by
    /// name.
    fn get_environment(&mut self) -> Vec<EnvBinding> {
        self.state.executor.get_environment()
            .into_iter()
            .map(|(name, _kind, _, value)| EnvBinding { ty: self.type_of(&name), name, value })
            .collect()
    }
}

/// Whether what running a unit printed is a runtime error, which ends the
/// session.
fn is_runtime_error(output: &str) -> bool {
    output.starts_with("Error:")
}
