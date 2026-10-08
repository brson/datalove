//! Evaluated consts are kept from one compile to the next, and only as long as
//! they are right.
//!
//! The const cache keys a module's consts on its source and the source of
//! everything it requires, transitively; see `const_cache` in the compiler. That
//! makes two promises, and these hold it to both:
//!
//! - **It is never stale.** After any edit, every const has the value a
//!   pipeline that evaluates everything gives it. Every test here runs with
//!   the cache verifying each hit against an evaluation besides, so a stale hit
//!   is a panic even where a test does not read the value back.
//! - **It is worth having.** An edit evaluates the consts of the module it
//!   changed and of the modules that require that one, transitively, and
//!   nothing else; a recompile with no edit evaluates none. A key that widened
//!   would still give right values, so only the report can see this one.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet};

use datalove_datafun as datafun;
use datafun::pipeline::{CompilerOptions, ModuleCompilationPipeline};

/// A world of `local/pkg` modules and data, compiled again after each change.
struct World {
    db: datafun::Database,
    pipeline: ModuleCompilationPipeline,
    modules: BTreeMap<String, String>,
    data: BTreeMap<String, String>,
}

/// Which modules a compile evaluated consts for, by name within `local/pkg`.
fn names(paths: &BTreeSet<String>) -> BTreeSet<String> {
    paths.iter()
        .map(|path| path.strip_prefix("local/pkg/").unwrap_or_else(|| panic!("{} is not local/pkg", path)).to_string())
        .collect()
}

fn set<'a>(names: &[&'a str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

impl World {
    fn new() -> World {
        World::with_options(CompilerOptions::default())
    }

    fn with_options(options: CompilerOptions) -> World {
        let mut pipeline = ModuleCompilationPipeline::new(options);
        pipeline.set_verify_const_cache(true);
        World {
            db: datafun::Database::default(),
            pipeline,
            modules: BTreeMap::new(),
            data: BTreeMap::new(),
        }
    }

    /// Add or replace a module.
    fn module(&mut self, name: &str, source: &str) {
        if self.modules.contains_key(name) {
            self.pipeline.update_source(&mut self.db, "local", "pkg", name, source);
        } else {
            self.pipeline.add_module(&self.db, "local", "pkg", name, source);
        }
        self.modules.insert(name.to_string(), source.to_string());
    }

    /// Add or replace a data file.
    fn data(&mut self, name: &str, text: &str) {
        if self.data.contains_key(name) {
            self.pipeline.update_data(&mut self.db, "local", "pkg", name, text);
        } else {
            self.pipeline.add_data(&self.db, "local", "pkg", name, text);
        }
        self.data.insert(name.to_string(), text.to_string());
    }

    /// Compile, then evaluate each of `probes` in a script requiring every module.
    ///
    /// Returns the modules whose consts the compile evaluated rather than took
    /// from the cache, and what each probe came to.
    fn compile(&mut self, probes: &[&str]) -> (BTreeSet<String>, Vec<String>) {
        let (compiled, db) = self.pipeline.compile(&mut self.db);
        assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
        let evaluated = names(&self.pipeline.const_cache_report().evaluated);

        let mut compiler = compiled.script_compiler_default(db).expect("a script compiler");
        let mut executor = compiled
            .script_executor(datafun::DebugOutputMode::Disabled, None)
            .expect("a script executor");
        let requires: String = self.modules.keys()
            .map(|name| format!("require module local/pkg/{}\n", name))
            .collect();
        let result = compiler.compile_fragment(&requires);
        let ir_unit = result.ir_unit.as_ref().unwrap_or_else(|| panic!("the requires lower: {:?}", result.first_error()));
        executor.execute_fragment(ir_unit);

        let values = probes.iter()
            .map(|probe| {
                let result = compiler.compile_expr(probe);
                let ir_unit = result.ir_unit.as_ref().unwrap_or_else(|| panic!("{} lowers: {:?}", probe, result.first_error()));
                executor.execute_expr(ir_unit).1
            })
            .collect();
        executor.destroy_live_values();
        (evaluated, values)
    }

    /// What `probes` come to in a world of the same sources that caches nothing.
    fn uncached(&self, probes: &[&str]) -> Vec<String> {
        let mut fresh = World::with_options(CompilerOptions { cache_consts: false, ..CompilerOptions::default() });
        for (name, text) in &self.data {
            fresh.data(name, text);
        }
        for (name, source) in &self.modules {
            fresh.module(name, source);
        }
        fresh.compile(probes).1
    }
}

/// A module whose const calls `dep.f`, read back through `get`.
fn caller(dep: &str) -> String {
    format!(
        "require module local/pkg/{dep}\n\
         const K: int = {dep}.f(1)\n\
         fun get(): int\n  ret K@\nend fun\n"
    )
}

/// A module of one function, `f(x) = x + add`.
fn leaf(add: u32) -> String {
    format!("fun f(x: int): int\n  ret x + {add}\nend fun\n")
}

#[test]
fn a_recompile_with_no_edit_evaluates_nothing() {
    let mut world = World::new();
    world.module("b", &leaf(10));
    world.module("a", &caller("b"));

    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&["a", "b"]));
    assert_eq!(values, ["11"]);

    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&[]));
    assert_eq!(values, ["11"]);
    assert_eq!(names(&world.pipeline.const_cache_report().reused), set(&["a", "b"]));
}

#[test]
fn editing_a_const_evaluates_it_again() {
    let mut world = World::new();
    world.module("a", "const K: int = 3\nfun get(): int\n  ret K@\nend fun\n");
    assert_eq!(world.compile(&["a.get()"]).1, ["3"]);

    world.module("a", "const K: int = 4\nfun get(): int\n  ret K@\nend fun\n");
    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&["a"]));
    assert_eq!(values, ["4"]);
}

#[test]
fn editing_a_function_a_const_calls_evaluates_the_const_again() {
    let mut world = World::new();
    world.module("b", &leaf(10));
    world.module("a", &caller("b"));
    assert_eq!(world.compile(&["a.get()"]).1, ["11"]);

    world.module("b", &leaf(20));
    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&["a", "b"]));
    assert_eq!(values, ["21"]);
}

#[test]
fn an_edit_reaches_through_requires_transitively() {
    let mut world = World::new();
    world.module("c", &leaf(100));
    world.module("b", "require module local/pkg/c\nfun f(x: int): int\n  ret c.f(x) + 10\nend fun\n");
    world.module("a", &caller("b"));
    assert_eq!(world.compile(&["a.get()"]).1, ["111"]);

    world.module("c", &leaf(200));
    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&["a", "b", "c"]));
    assert_eq!(values, ["211"]);
}

#[test]
fn an_edit_leaves_the_modules_it_does_not_reach_alone() {
    let mut world = World::new();
    world.module("b", &leaf(10));
    world.module("a", &caller("b"));
    world.module("u", &leaf(5));
    world.compile(&[]);

    // Not required by anything with a const.
    world.module("u", &leaf(6));
    assert_eq!(world.compile(&["a.get()"]), (set(&["u"]), vec!["11".to_string()]));

    // Required by `b`'s requirer, not by `b`.
    world.module("a", &format!("{}fun extra(): int\n  ret 1\nend fun\n", caller("b")));
    assert_eq!(world.compile(&["a.get()"]), (set(&["a"]), vec!["11".to_string()]));
}

#[test]
fn a_const_in_a_function_body_is_kept_and_evaluated_again_like_a_module_const() {
    let mut world = World::new();
    world.module("b", &leaf(10));
    world.module("a", "require module local/pkg/b\nfun get(): int\n  const L: int = b.f(2)\n  ret L@\nend fun\n");
    assert_eq!(world.compile(&["a.get()"]).1, ["12"]);
    assert_eq!(world.compile(&["a.get()"]).0, set(&[]));

    world.module("b", &leaf(30));
    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&["a", "b"]));
    assert_eq!(values, ["32"]);
}

#[test]
fn a_const_reading_data_is_evaluated_again_when_the_data_changes() {
    let mut world = World::new();
    world.data("nums", "[7, 8, 9]");
    world.module("a", "\
require data local/pkg/nums: [u32]
fun at(i: index): ?u32
  ret some (nums[i]?)
end fun
fun first(): u32
  if at(: index / 0) |n|
    ret n
  end if
  ret 0
end fun
const FIRST: u32 = first()
fun get(): u32
  ret FIRST
end fun
");
    assert_eq!(world.compile(&["a.get()"]).1, ["7"]);
    assert_eq!(world.compile(&["a.get()"]).0, set(&[]));

    world.data("nums", "[40, 8, 9]");
    let (evaluated, values) = world.compile(&["a.get()"]);
    assert_eq!(evaluated, set(&["a"]));
    assert_eq!(values, ["40"]);
}

#[test]
fn a_module_whose_consts_failed_is_evaluated_every_time() {
    let source = |divisor: u32| format!("\
fun get(): !u32
  const A: u32 = 100
  const B: u32 = {divisor}
  const C: u32 = A /! B
  ret ok C
end fun
");
    let mut world = World::new();
    world.module("a", &source(0));
    for _ in 0..2 {
        let (compiled, _) = world.pipeline.compile(&mut world.db);
        assert!(compiled.has_errors(), "dividing by zero is an error");
        assert_eq!(names(&world.pipeline.const_cache_report().evaluated), set(&["a"]));
    }

    world.module("a", &source(4));
    assert_eq!(world.compile(&["a.get()!"]), (set(&["a"]), vec!["25".to_string()]));
    assert_eq!(world.compile(&["a.get()!"]).0, set(&[]));
}

#[test]
fn changing_the_riders_empties_the_cache() {
    let mut world = World::new();
    world.module("b", &leaf(10));
    world.module("a", &caller("b"));
    world.compile(&[]);

    world.pipeline.set_rider_sources(vec![(
        "extra".to_string(),
        "native fun twice(x: u32): u32\n".to_string(),
    )]);
    assert_eq!(world.compile(&["a.get()"]), (set(&["a", "b"]), vec!["11".to_string()]));
    assert_eq!(world.compile(&["a.get()"]).0, set(&[]));
}

#[test]
fn turning_the_cache_off_evaluates_everything_every_time() {
    let mut world = World::with_options(CompilerOptions { cache_consts: false, ..CompilerOptions::default() });
    world.module("b", &leaf(10));
    world.module("a", &caller("b"));
    world.compile(&[]);
    world.compile(&[]);
    let report = world.pipeline.const_cache_report();
    assert!(report.evaluated.is_empty() && report.reused.is_empty(), "{:?}", report);
}

/// A chain with a fork in it, edited at random, checked against a pipeline that
/// caches nothing after every edit.
///
/// `m0` is required by `m1` and `m2`, which are both required by `m3`, and `m4`
/// requires `m3`; `side` is required by nobody. Every module has a module const
/// and a function-body const, each calling down the chain. An edit changes one
/// module's addend, or adds a function nothing calls; either way the evaluated
/// set has to be that module and what requires it, and every value has to be
/// what evaluating from scratch says.
#[test]
fn random_edits_match_a_world_that_caches_nothing() {
    const MODULES: [(&str, &[&str]); 6] = [
        ("m0", &[]),
        ("m1", &["m0"]),
        ("m2", &["m0"]),
        ("m3", &["m1", "m2"]),
        ("m4", &["m3"]),
        ("side", &[]),
    ];
    let source = |name: &str, requires: &[&str], add: u32, padding: u32| {
        let mut text = String::new();
        for dep in requires {
            text.push_str(&format!("require module local/pkg/{}\n", dep));
        }
        let below: String = requires.iter().map(|dep| format!(" + {}.f(x@)", dep)).collect();
        text.push_str(&format!("fun f(x: int): int\n  ret x@ + {}{}\nend fun\n", add, below));
        text.push_str("const K: int = f(1)\n");
        text.push_str("fun get(): int\n  ret K@\nend fun\n");
        text.push_str("fun body(): int\n  const L: int = f(2)\n  ret L@\nend fun\n");
        for i in 0..padding {
            text.push_str(&format!("fun pad{}(): int\n  ret {}\nend fun\n", i, i));
        }
        let _ = name;
        text
    };
    // What requires each module, transitively, the module included.
    let reach = |edited: &str| -> BTreeSet<String> {
        let mut reached = set(&[edited]);
        loop {
            let more: BTreeSet<String> = MODULES.iter()
                .filter(|(_, requires)| requires.iter().any(|dep| reached.contains(*dep)))
                .map(|(name, _)| name.to_string())
                .collect();
            if more.is_subset(&reached) {
                return reached;
            }
            reached.extend(more);
        }
    };
    let probes: Vec<String> = MODULES.iter()
        .flat_map(|(name, _)| [format!("{}.get()", name), format!("{}.body()", name)])
        .collect();
    let probes: Vec<&str> = probes.iter().map(String::as_str).collect();

    let mut world = World::new();
    let mut state: BTreeMap<&str, (u32, u32)> = MODULES.iter().map(|(name, _)| (*name, (1, 0))).collect();
    for (name, requires) in MODULES {
        world.module(name, &source(name, requires, 1, 0));
    }
    let (_, values) = world.compile(&probes);
    assert_eq!(values, world.uncached(&probes));

    // A small LCG, so the sequence is the same every run.
    let mut seed: u64 = 0x5eed;
    let mut next = |bound: u64| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) % bound
    };
    for step in 0..24 {
        let (name, requires) = MODULES[next(MODULES.len() as u64) as usize];
        let (add, padding) = state[name];
        let edited = if next(3) == 0 { (add, padding + 1) } else { (add + 1 + next(5) as u32, padding) };
        state.insert(name, edited);
        world.module(name, &source(name, requires, edited.0, edited.1));

        let (evaluated, values) = world.compile(&probes);
        assert_eq!(evaluated, reach(name), "step {}: editing {}", step, name);
        assert_eq!(values, world.uncached(&probes), "step {}: editing {}", step, name);
    }
}
