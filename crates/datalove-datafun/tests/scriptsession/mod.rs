// Each test target that includes this module compiles its own copy and uses
// the part it needs, so anything the other target uses looks dead here.
#![allow(dead_code)]

//! A script session a test can append to, edit and measure.
//!
//! Shared by `script_exec_reactivity_tests` and `script_scenario_tests`, which
//! ask the same questions of different fixtures: which units an edit reaches
//! through lowering and execution, and which it leaves alone.
//!
//! **A `ScriptCompiler` borrows the database for as long as it lives** and an
//! edit is `set_text`, which needs `&mut db`, so the compiler is built, used and
//! let go within each step. [`ScriptSession`] is what crosses in between. The
//! executor holds no salsa data at all, so it simply stays -- except for the
//! module registry, which is the one thing a module edit replaces.
//!
//! **Every expectation here is derived from the typechecker's own record** --
//! `new_vars`, `new_fns` and `asked_names` from stage A, and
//! `imported_modules` for the module edge -- together with the fixture text the
//! test itself wrote. None of it is read back from the reach the compiler
//! computes, which is the thing under test. See
//! `botdocs/plan-script-reactivity.md`.

use rmx::prelude::*;
use rmx::std::collections::BTreeSet;
use salsa::Setter as _;

use datalove_datafun as datafun;
use datafun::pipeline::{
    CompilerOptions, ModuleCompilationPipeline, ScriptCompilationResult, ScriptExecutor,
    ScriptSession,
};

/// The mark unit `index` leaves when it runs.
///
/// Every fixture unit ends in a `debuglog` of one of these, so the debug buffer
/// says which units ran. `debuglog` is the only effect a `fun` can have, which
/// is what makes the buffer a complete account of execution.
pub fn mark(index: usize) -> String {
    format!("ran {}", unit_letter(index))
}

/// The letter a unit is named by, for `A` through `Z`.
pub fn unit_letter(index: usize) -> char {
    let letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    letters.as_bytes()[index] as char
}

/// A module, as the pipeline names one.
pub struct Module<'a> {
    pub library: &'a str,
    pub package: &'a str,
    pub module: &'a str,
    pub source: &'a str,
}

impl Module<'_> {
    /// The path an import of this module resolves to.
    pub fn path(&self) -> String {
        format!("{}/{}/{}", self.library, self.package, self.module)
    }
}

/// A script session that can be edited part way through.
pub struct Session {
    pub db: datafun::Database,
    pub pipeline: ModuleCompilationPipeline,
    pub script: Option<ScriptSession>,
    pub executor: ScriptExecutor,
    /// Each unit's text as it now stands, which is where the import edges the
    /// typechecker does not record in `asked_names` are read from.
    texts: Vec<String>,
}

impl Session {
    pub fn new() -> Session {
        Session::with_db(datafun::Database::default())
    }

    pub fn with_db(db: datafun::Database) -> Session {
        Session::build(db, ModuleCompilationPipeline::new(CompilerOptions::default()))
    }

    /// A session over a set of modules the script may require and import from.
    pub fn with_modules(modules: &[Module<'_>]) -> Session {
        Session::with_modules_in(datafun::Database::default(), modules)
    }

    /// The same, over a database the caller built -- a recording one, usually.
    pub fn with_modules_in(db: datafun::Database, modules: &[Module<'_>]) -> Session {
        let mut pipeline = ModuleCompilationPipeline::new(CompilerOptions::default());
        for module in modules {
            pipeline.add_module(
                &db, module.library, module.package, module.module, module.source);
        }
        Session::build(db, pipeline)
    }

    fn build(db: datafun::Database, mut pipeline: ModuleCompilationPipeline) -> Session {
        let (script, executor) = {
            let compiled = pipeline.compile_fresh(&db);
            assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
            let script = compiled
                .script_compiler_default(&db)
                .expect("a script compiler")
                .into_session();
            let executor = compiled
                .script_executor(datalove_rt::c::DebugOutputMode::Buffer, None)
                .expect("a script executor");
            (script, executor)
        };
        Session { db, pipeline, script: Some(script), executor, texts: Vec::new() }
    }

    /// How many units the script holds.
    pub fn unit_count(&self) -> usize {
        self.texts.len()
    }

    /// Compile one more unit on the end of the script, without running it.
    pub fn compile_append(&mut self, text: &str) -> ScriptCompilationResult {
        let compiled = self.pipeline.compile_fresh(&self.db);
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("a script compiler");
        let result = compiler.compile_fragment(text);
        self.script = Some(compiler.into_session());
        if result.ir_unit.is_some() {
            self.texts.push(text.S());
        }
        result
    }

    /// Compile and run one more unit on the end of the script.
    pub fn append(&mut self, text: &str) {
        let result = self.compile_append(text);
        let ir_unit = result.ir_unit.as_ref().unwrap_or_else(|| {
            panic!("appending {text:?} should compile: {:?} {:?}",
                result.typecheck, result.lowering)
        });
        let output = self.executor.execute_fragment(ir_unit);
        assert!(!output.starts_with("Error:"), "running {text:?}: {output}");
    }

    /// Compile and run one more unit, submitted as a bare expression.
    ///
    /// Returns the result type and the printed value, which is what the REPL
    /// shows for an expression line.
    pub fn append_expr(&mut self, text: &str) -> (Option<String>, String) {
        let compiled = self.pipeline.compile_fresh(&self.db);
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("a script compiler");
        let result = compiler.compile_expr(text);
        self.script = Some(compiler.into_session());

        let ir_unit = result.ir_unit.as_ref().unwrap_or_else(|| {
            panic!("the expression {text:?} should compile: {:?}", result.lowering)
        });
        self.texts.push(text.S());
        self.executor.execute_expr(ir_unit)
    }

    /// Change unit `index`'s text, which is `set_text` on its `Source`.
    pub fn edit(&mut self, index: usize, text: &str) {
        let source = self.script.as_ref().expect("a session").unit_sources()[index];
        source.set_text(&mut self.db).to(text.S());
        self.texts[index] = text.S();
    }

    /// Re-lower what an edit to unit `edited` reaches, without running any of it.
    ///
    /// For the cases where a unit is supposed to stop compiling, which
    /// [`Self::rederive`] refuses to see.
    pub fn relower(&mut self, edited: usize) -> Vec<(usize, ScriptCompilationResult)> {
        let compiled = self.pipeline.compile_fresh(&self.db);
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("a script compiler");
        let redone = compiler.relower_reach(edited);
        self.script = Some(compiler.into_session());
        redone
    }

    /// Re-lower and re-execute what an edit to unit `edited` reaches.
    ///
    /// Returns the units re-lowered, which is also the units re-executed:
    /// a unit whose lowering the edit reaches has to run again, since it is its
    /// values the edit changed.
    pub fn rederive(&mut self, edited: usize) -> BTreeSet<usize> {
        let redone = self.relower(edited);
        self.rerun(redone)
    }

    /// Add a module the session did not start with.
    ///
    /// The module *set* changes, which is a different world rather than an edit
    /// to one: `ScriptEnv` interns over the modules it may import from, so the
    /// env moves and every unit is re-keyed. Nothing is re-derived here -- the
    /// caller asks the units whatever it wants to know afterwards.
    pub fn add_module(&mut self, module: &Module<'_>) {
        self.pipeline.add_module(
            &self.db, module.library, module.package, module.module, module.source);
    }

    /// Take a module out of the session, the other half of [`Self::add_module`].
    pub fn remove_module(&mut self, module: &Module<'_>) {
        self.pipeline.remove_module(module.library, module.package, module.module);
    }

    /// Change a module's text and re-derive what that reaches.
    ///
    /// Two steps rather than one, and the second is the one a unit edit does not
    /// need: the executor calls a module function through the registry it holds,
    /// so recompiling the modules changes nothing until the new registry is put
    /// in front of it. A script unit's own IR names a module function by
    /// `CodeRef::Module`, so re-lowering the unit does not carry the change.
    pub fn relower_module(
        &mut self,
        module: &Module<'_>,
    ) -> Vec<(usize, ScriptCompilationResult)> {
        self.pipeline.update_source(
            &mut self.db, module.library, module.package, module.module, module.source);

        let compiled = self.pipeline.compile_fresh(&self.db);
        assert!(
            compiled.is_successful(),
            "the edited module should compile: {:?}", compiled.all_errors(),
        );
        self.executor.set_module_registry(compiled.shared.module_registry.clone());
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("a script compiler");
        let redone = compiler.relower_module_reach(&[module.path()]);
        self.script = Some(compiler.into_session());
        redone
    }

    /// Change a module's text and re-derive and re-execute what it reaches.
    pub fn rederive_module(&mut self, module: &Module<'_>) -> BTreeSet<usize> {
        let redone = self.relower_module(module);
        self.rerun(redone)
    }

    /// Run each re-lowered unit again in place, in the order given.
    fn rerun(&mut self, redone: Vec<(usize, ScriptCompilationResult)>) -> BTreeSet<usize> {
        let mut order = Vec::new();
        for (index, result) in redone {
            let ir_unit = result.ir_unit.as_ref().unwrap_or_else(|| {
                panic!("unit {index} should re-lower: {:?} {:?}",
                    result.typecheck, result.lowering)
            });
            let (_, output) = self.executor.reexecute_unit(index, ir_unit);
            assert!(!output.starts_with("Error:"), "re-running unit {index}: {output}");
            order.push(index);
        }

        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted, "units must be re-derived in index order");
        order.into_iter().collect()
    }

    /// What each unit provides, what it uses and what it imports, as owned text.
    ///
    /// Owned because it is read before the database is borrowed mutably for the
    /// edit, and everything salsa hands back is tied to that borrow.
    pub fn graph(&mut self) -> Graph {
        let compiled = self.pipeline.compile_fresh(&self.db);
        let compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("a script compiler");
        let db = &self.db;
        let mut graph = Graph {
            provides: Vec::new(),
            signatures: Vec::new(),
            uses: Vec::new(),
            imports: Vec::new(),
            failed: Vec::new(),
        };
        for (index, output) in compiler.unit_typecheck_outputs().iter().enumerate() {
            graph.provides.push(
                output.new_vars(db).iter().map(|(name, _, _)| name.as_str(db).S())
                    .chain(output.new_fns(db).iter().map(|(name, _)| name.as_str(db).S()))
                    .chain(output.new_module_aliases(db).iter()
                        .map(|(alias, _)| alias.as_str(db).S()))
                    .collect(),
            );
            // What `binding_at` would answer about each name, as a hash, so
            // that an edit which leaves a binding alone can be told from one
            // that moves it without holding anything borrowed from the
            // database. A type and a mutability, because those are what
            // `ScriptBinding` carries and so what a later unit's memo turns on.
            let mut signatures: Vec<(String, u64)> = output.new_vars(db).iter()
                .map(|(name, ty, is_mutable)| {
                    (name.as_str(db).S(), hash_of(&(ty, is_mutable)))
                })
                .chain(output.new_fns(db).iter()
                    .map(|(name, func)| (name.as_str(db).S(), hash_of(func))))
                .collect();
            if output.result(db).errors(db).is_empty() {
                signatures.sort();
            } else {
                // A unit that failed provides nothing, which is the answer
                // `unit_provides` gives and so what the units after it see.
                signatures.clear();
            }
            graph.signatures.push(signatures);
            let mut uses: Vec<String> =
                output.asked_names(db).iter().map(|name| name.as_str(db).S()).collect();
            // The alias an `import` names does not go through the lookups
            // `asked_names` records, so it is read off the fixture text -- which
            // this test wrote and the compiler's own derivation has no part in.
            uses.extend(imported_aliases(&self.texts[index]));
            graph.uses.push(uses);
            graph.imports.push(output.imported_modules(db).C());
            graph.failed.push(!output.result(db).errors(db).is_empty());
        }
        self.script = Some(compiler.into_session());
        graph
    }

    /// The marks left since the buffer was last cleared, as unit indices.
    pub fn units_run(&mut self) -> BTreeSet<usize> {
        let buffer = self.executor.get_debug_buffer();
        self.executor.clear_debug_buffer();
        (0..self.texts.len()).filter(|index| buffer.contains(&mark(*index))).collect()
    }

    pub fn binding(&mut self, name: &str) -> String {
        self.executor
            .get_binding(name)
            .unwrap_or_else(|| panic!("no binding named {name}"))
            .1
    }

    /// Whether a name is bound at all, which a removed binding is not.
    pub fn has_binding(&mut self, name: &str) -> bool {
        self.executor.get_binding(name).is_some()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Before the interpreter's runtime is shut down, which is where the
        // leak checker has its say.
        self.executor.destroy_live_values();
    }
}

/// The module aliases the `import` statements in one unit's text name.
fn imported_aliases(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("import "))
        .map(|rest| rest.split('.').next().expect("an import names a module").trim().S())
        .collect()
}

/// One value's hash, which is how a binding is compared across a mutation.
fn hash_of<T: std::hash::Hash>(value: &T) -> u64 {
    use std::hash::Hasher as _;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// What each unit provides, what it uses and which modules it imports from.
pub struct Graph {
    pub provides: Vec<Vec<String>>,
    /// What each unit provides, name and binding together.
    ///
    /// `provides` says which names moved between units; this says whether the
    /// binding behind a name is the same one, which is what decides whether a
    /// unit that reads it has to be typechecked again.
    pub signatures: Vec<Vec<(String, u64)>>,
    pub uses: Vec<Vec<String>>,
    pub imports: Vec<Vec<String>>,
    pub failed: Vec<bool>,
}

impl Graph {
    /// Both graphs' edges at once, which is what a reach has to be walked over.
    ///
    /// Either alone would miss a case: a binding the edit *removes* is absent
    /// from the new graph although the unit that read it has to be told, and a
    /// name the edit *introduces* is absent from the old one although a later
    /// unit that asked for it in vain now finds it.
    pub fn union(&self, other: &Graph) -> Graph {
        let units = self.provides.len().max(other.provides.len());
        let join = |mine: &Vec<Vec<String>>, theirs: &Vec<Vec<String>>| -> Vec<Vec<String>> {
            (0..units)
                .map(|unit| {
                    let mut joined: Vec<String> = mine.get(unit).cloned().unwrap_or_default();
                    joined.extend(theirs.get(unit).cloned().unwrap_or_default());
                    joined.sort();
                    joined.dedup();
                    joined
                })
                .collect()
        };
        Graph {
            provides: join(&self.provides, &other.provides),
            // The binding behind a name is whatever it is *now*, since the
            // question a signature answers is what a unit would see today.
            signatures: other.signatures.clone(),
            uses: join(&self.uses, &other.uses),
            imports: join(&self.imports, &other.imports),
            failed: other.failed.clone(),
        }
    }

    /// The units reachable from `seeds`, in index order, the seeds included.
    ///
    /// A name a unit uses resolves to the nearest earlier unit that provides
    /// it, so the reach extends to a unit when one of the names it uses
    /// resolves to a unit already reached. Transitive, and one forward pass
    /// gives all of it, because a unit's providers all sit before it.
    ///
    /// An edited unit is always in its own reach: `asked_names` records a name
    /// the unit binds itself, deliberately, and that over-reporting is what
    /// makes it sound. See `script_graph_tests`.
    pub fn reach_from(&self, seeds: &BTreeSet<usize>) -> BTreeSet<usize> {
        let mut reached = vec![false; self.uses.len()];
        for unit in 0..self.uses.len() {
            reached[unit] = seeds.contains(&unit)
                || self.uses[unit].iter().any(|name| {
                    (0..unit)
                        .rev()
                        .find(|earlier| self.provides[*earlier].contains(name))
                        .is_some_and(|provider| reached[provider])
                });
        }
        (0..reached.len()).filter(|unit| reached[*unit]).collect()
    }

    /// The units an edit to unit `edited` reaches.
    pub fn reach(&self, edited: usize) -> BTreeSet<usize> {
        self.reach_from(&BTreeSet::from([edited]))
    }

    /// The binding `name` resolves to for a unit sitting at `before`.
    ///
    /// The nearest earlier unit that provides it wins, which is what
    /// `binding_at` walks back for.
    fn resolved(&self, before: usize, name: &str) -> Option<u64> {
        (0..before).rev().find_map(|unit| {
            self.signatures[unit].iter()
                .rev()
                .find(|(provided, _)| provided == name)
                .map(|(_, signature)| *signature)
        })
    }

    /// The units an edit to the module at `path` reaches.
    ///
    /// Seeded by the units whose imports resolve into that module, and then
    /// carried on through the same name edges: a unit that reads what an
    /// importing unit computed is as stale as the importing unit.
    pub fn module_reach(&self, path: &str) -> BTreeSet<usize> {
        self.reach_from(&self.module_importers(path))
    }

    /// The units whose imports resolve into the module at `path`.
    pub fn module_importers(&self, path: &str) -> BTreeSet<usize> {
        (0..self.imports.len())
            .filter(|unit| self.imports[*unit].iter().any(|held| held == path))
            .collect()
    }
}

/// The units that have to be typechecked again, derived from the two graphs.
///
/// **Analysis is not lowering and the reaches differ**, which is the whole
/// distinction stage C exists for: a unit is typechecked again only when one of
/// the names it asked about answers differently, because its memo depends on
/// the `binding_at` reads and nothing else. A value-only edit moves no binding,
/// so it reaches nobody but the unit it is in, where the *execution* reach
/// carries on to everything downstream.
///
/// `seeded` are the units whose own text changed, which re-run whatever their
/// environment says, because `unit_ast` is keyed on the unit.
pub fn analysis_reach(
    before: &Graph,
    after: &Graph,
    seeded: &BTreeSet<usize>,
) -> BTreeSet<usize> {
    let units = before.uses.len().max(after.uses.len());
    (0..units)
        .filter(|unit| {
            seeded.contains(unit)
                || after.uses[*unit].iter().any(|name| {
                    before.resolved(*unit, name) != after.resolved(*unit, name)
                })
        })
        .collect()
}
