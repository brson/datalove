//! Evaluated consts, kept from one compile to the next.
//!
//! Const evaluation runs the interpreter, and the interpreter cannot run inside
//! a tracked function -- see "What is not tracked" in
//! `botdocs/compiler-guide.md` -- so salsa does not memoize it. This does,
//! per module, outside salsa, for a pipeline that compiles the same world again
//! and again.
//!
//! **The key is the module's source and the source of everything it requires,
//! transitively**, with the world's data files besides. A const can only
//! call into the modules its module requires, and those into theirs, so a
//! module whose closure is textually unchanged evaluates its consts to what it
//! did last time. Content rather than salsa identity, so that nothing about how
//! the database was driven between two compiles can make a stale entry look
//! fresh. What the closure cannot see -- the riders, and the evaluator itself --
//! is the owner's to report: [`ConstCache::set_world`] takes a fingerprint of
//! it, and anything that changes the natives clears the cache.
//!
//! Only a module whose consts all evaluated is kept. One with an error is
//! evaluated every time, so that the error is reported every time.
//!
//! Set `DATALOVE_VERIFY_CONST_CACHE` and every hit is evaluated as well, and
//! the two compared; a difference is a panic, which is how the test suite
//! checks that the key is not missing anything.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use bct::input::Source;
use bct::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_ir::{ConstValue, IrType, SharedConst};

/// A 256-bit content hash.
pub type Fingerprint = [u8; 32];

/// A module's consts as the last compile left them.
struct CachedModule {
    fingerprint: Fingerprint,
    /// The module-level consts, by name.
    module_level: Option<HashMap<String, (IrType, Arc<ConstValue>)>>,
    /// The consts in function bodies, under `func::name`, sorted.
    body: Option<Vec<(String, IrType, SharedConst)>>,
}

/// Which modules the last compile evaluated, and which it took from the cache.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConstCacheReport {
    /// Modules evaluated in at least one of the two const phases.
    pub evaluated: BTreeSet<String>,
    /// Modules every phase that ran took from the cache.
    pub reused: BTreeSet<String>,
}

/// Evaluated consts by module path; see the module docs.
pub struct ConstCache {
    modules: HashMap<String, CachedModule>,
    /// What the closures cannot see. A change empties the cache.
    world: Option<Fingerprint>,
    /// This compile's key for each module in the graph.
    keys: HashMap<String, Fingerprint>,
    report: ConstCacheReport,
    verify: bool,
}

impl Default for ConstCache {
    fn default() -> Self {
        ConstCache {
            modules: HashMap::new(),
            world: None,
            keys: HashMap::new(),
            report: ConstCacheReport::default(),
            verify: std::env::var_os("DATALOVE_VERIFY_CONST_CACHE").is_some(),
        }
    }
}

impl ConstCache {
    /// Forget everything.
    pub fn clear(&mut self) {
        self.modules.clear();
        self.world = None;
    }

    /// Say what the world outside the modules is, emptying the cache if it moved.
    ///
    /// The riders' interfaces, for one; whatever an owner knows a const's
    /// value depends on that is not in a module's source.
    pub fn set_world(&mut self, world: Fingerprint) {
        if self.world != Some(world) {
            self.modules.clear();
            self.world = Some(world);
        }
    }

    /// Evaluate every hit as well, and panic if the two differ.
    ///
    /// What `DATALOVE_VERIFY_CONST_CACHE` turns on, for a caller that wants it
    /// regardless.
    pub fn set_verify(&mut self, verify: bool) {
        self.verify = verify;
    }

    /// What the last compile evaluated and reused.
    pub fn last_report(&self) -> &ConstCacheReport {
        &self.report
    }

    /// Key every module in `graph`, and drop what no longer matches.
    pub(crate) fn begin<'db>(&mut self, db: &'db dyn salsa::Database, graph: ModuleGraph<'db>) {
        let data = data_fingerprint(db, graph);
        self.keys = graph.modules(db).iter()
            .map(|module| {
                let mut hasher = rmx::blake3::Hasher::new();
                hasher.update(&closure_fingerprint(db, graph, module.id(db)));
                hasher.update(&data);
                (module.id(db).path(db).clone(), *hasher.finalize().as_bytes())
            })
            .collect();
        let keys = &self.keys;
        self.modules.retain(|path, cached| keys.get(path) == Some(&cached.fingerprint));
        self.report = ConstCacheReport::default();
    }

    /// Start a compile that evaluates no consts, so keys nothing.
    pub(crate) fn begin_without_consts(&mut self) {
        self.keys.clear();
        self.report = ConstCacheReport::default();
    }

    /// Whether every hit is checked against an evaluation.
    pub(crate) fn verifying(&self) -> bool {
        self.verify
    }

    /// The module-level consts of `path` from an earlier compile.
    pub(crate) fn module_level(&self, path: &str) -> Option<&HashMap<String, (IrType, Arc<ConstValue>)>> {
        self.modules.get(path)?.module_level.as_ref()
    }

    /// The function-body consts of `path` from an earlier compile.
    pub(crate) fn body(&self, path: &str) -> Option<&Vec<(String, IrType, SharedConst)>> {
        self.modules.get(path)?.body.as_ref()
    }

    /// Keep the module-level consts of `path`, which all evaluated.
    pub(crate) fn store_module_level(&mut self, path: &str, consts: HashMap<String, (IrType, Arc<ConstValue>)>) {
        self.entry(path).module_level = Some(consts);
    }

    /// Keep the function-body consts of `path`, which all evaluated.
    pub(crate) fn store_body(&mut self, path: &str, consts: Vec<(String, IrType, SharedConst)>) {
        self.entry(path).body = Some(consts);
    }

    /// Record that a phase evaluated `path` rather than reusing it.
    pub(crate) fn note_evaluated(&mut self, path: &str) {
        self.report.reused.remove(path);
        self.report.evaluated.insert(path.to_owned());
    }

    /// Record that a phase reused `path`.
    pub(crate) fn note_reused(&mut self, path: &str) {
        if !self.report.evaluated.contains(path) {
            self.report.reused.insert(path.to_owned());
        }
    }

    fn entry(&mut self, path: &str) -> &mut CachedModule {
        let fingerprint = *self.keys.get(path).expect("every module evaluated was keyed by `begin`");
        let cached = self.modules.entry(path.to_owned()).or_insert_with(|| CachedModule {
            fingerprint,
            module_level: None,
            body: None,
        });
        assert_eq!(cached.fingerprint, fingerprint, "`begin` drops entries that do not match");
        cached
    }
}

/// A source's text, hashed.
///
/// Tracked, so an unchanged source is not hashed again; a data file can be
/// megabytes.
#[salsa::tracked(returns(copy))]
fn source_fingerprint(db: &dyn salsa::Database, source: Source) -> Fingerprint {
    *rmx::blake3::hash(source.text(db).as_bytes()).as_bytes()
}

/// A module's path and source, and the closure fingerprints of what it requires.
///
/// So it covers every module the module reaches. The requires are hashed in
/// path order, so the fingerprint is a function of content alone.
#[salsa::tracked(returns(copy))]
fn closure_fingerprint<'db>(
    db: &'db dyn salsa::Database,
    graph: ModuleGraph<'db>,
    module_id: ModuleId<'db>,
) -> Fingerprint {
    let module = graph.module_by_id(db)[&module_id];
    let mut hasher = rmx::blake3::Hasher::new();
    let path = module_id.path(db);
    hasher.update(&(path.len() as u64).to_le_bytes());
    hasher.update(path.as_bytes());
    hasher.update(&source_fingerprint(db, module.source(db)));
    let mut requires: Vec<(&String, Fingerprint)> = graph.dependencies(db)
        .get(&module_id)
        .into_iter()
        .flatten()
        .map(|dep| (dep.path(db), closure_fingerprint(db, graph, *dep)))
        .collect();
    requires.sort();
    for (_, fingerprint) in requires {
        hasher.update(&fingerprint);
    }
    *hasher.finalize().as_bytes()
}

/// Every data file in the world, by path.
///
/// The whole world's rather than what each module requires: data is edited
/// rarely, and a const reading the wrong version of it would be hard to see.
#[salsa::tracked(returns(copy))]
fn data_fingerprint<'db>(db: &'db dyn salsa::Database, graph: ModuleGraph<'db>) -> Fingerprint {
    let mut hasher = rmx::blake3::Hasher::new();
    for (path, source) in graph.data(db) {
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&source_fingerprint(db, *source));
    }
    *hasher.finalize().as_bytes()
}
