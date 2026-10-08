//! What an edit costs, against what a first compile costs.
//!
//! The suite has fixtures for whether memoization happens; this measures what
//! it is worth. A no-op recompile is the floor every edit pays -- the untracked
//! glue around the queries -- and the gap to an edit is what one module's work
//! actually costs. Both were around 15ms of a 53ms first compile before phase
//! 5a was memoized, which is to say an edit cost what compiling everything did.
//!
//! Held together by the pipeline: the `Source` inputs it owns are the unit of
//! reuse (see "Reusing a Compiled World" in `botdocs/compiler-guide.md`), and
//! rebuilding it loses everything, so these all drive one.

use std::cell::RefCell;

use divan::Bencher;

use datalove_datafun::pipeline::ModuleCompilationPipeline;
use datalove_datafun_compiler::Database;

// Enough modules that per-module work is visible next to the fixed floor.
const NUM_MODULES: usize = 32;
const FUNCTIONS_PER_MODULE: usize = 30;

fn main() {
    divan::main();
}

/// One module of `FUNCTIONS_PER_MODULE` functions, `salt` changing every body.
fn module_source(module_idx: usize, salt: usize) -> String {
    let mut source = String::new();
    for func_idx in 0..FUNCTIONS_PER_MODULE {
        source.push_str(&format!(
            "fun func_{}_{}(a: int, b: int): !int\n",
            module_idx, func_idx
        ));
        source.push_str("  var result: int = a\n");
        source.push_str(&format!("  set result = result + b + {}\n", salt));
        source.push_str("  if result .> 100\n");
        source.push_str("    set result = result * 2\n");
        source.push_str("  end if\n");
        source.push_str("  ret ok result\n");
        source.push_str("end fun\n\n");
    }
    source
}

/// A pipeline holding every module, with nothing compiled yet.
fn pipeline(db: &Database) -> ModuleCompilationPipeline {
    let mut pipeline = ModuleCompilationPipeline::default();
    for i in 0..NUM_MODULES {
        pipeline.add_module(db, "local", "pkg", &format!("m{}", i), &module_source(i, 0));
    }
    pipeline
}

/// Compile once and check it worked, so no case times an error path.
fn compile_fresh(db: &Database, pipeline: &mut ModuleCompilationPipeline) {
    let compiled = pipeline.compile_fresh(db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
}

/// Everything cold: a new database, new inputs, nothing memoized.
#[divan::bench(sample_count = 20)]
fn first_compile(bencher: Bencher) {
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let pipeline = pipeline(&db);
            (db, pipeline)
        })
        .bench_local_values(|(db, mut pipeline)| {
            compile_fresh(&db, &mut pipeline);
            divan::black_box(&pipeline);
        });
}

/// Recompiling a world that did not change: the floor an edit cannot get under.
///
/// Set up once and measured many times, because the point is what a warm
/// pipeline costs; building a cold one per sample would drown it.
#[divan::bench(sample_count = 50)]
fn unchanged_recompile(bencher: Bencher) {
    let db = Database::default();
    let mut pipeline = pipeline(&db);
    compile_fresh(&db, &mut pipeline);
    let state = RefCell::new((db, pipeline));

    bencher.bench_local(|| {
        let mut held = state.borrow_mut();
        let (db, pipeline) = &mut *held;
        let (compiled, _) = pipeline.compile(db);
        assert!(compiled.is_successful());
    });
}

/// One module of `NUM_MODULES` edited, then recompiled.
#[divan::bench(sample_count = 50)]
fn one_module_edit(bencher: Bencher) {
    let db = Database::default();
    let mut pipeline = pipeline(&db);
    compile_fresh(&db, &mut pipeline);
    let state = RefCell::new((db, pipeline));
    let salt = RefCell::new(0usize);

    bencher.bench_local(|| {
        let mut held = state.borrow_mut();
        let (db, pipeline) = &mut *held;
        // A new body every iteration, or all but the first are no-op recompiles.
        *salt.borrow_mut() += 1;
        pipeline.update_source(db, "local", "pkg", "m0", &module_source(0, *salt.borrow()));
        let (compiled, _) = pipeline.compile(db);
        assert!(compiled.is_successful());
    });
}

// A world whose consts do some work. The one above declares none, so it never
// evaluates any and cannot show what the const cache saves.
const CONST_MODULES: usize = 16;

/// What every const module requires: a loop, for the consts to spend time in.
const SPIN_SOURCE: &str = "\
fun spin(n: int): int
  var acc: int = 0
  var i: int = 0
  loop while i .< n
    set acc = acc + i@
    set i = i + 1
  end loop
  ret acc
end fun
";

/// A module with module consts and a function-body const, each running `spin`.
fn const_module_source(module_idx: usize, salt: usize) -> String {
    let mut source = String::from("require module local/pkg/spin\n");
    for const_idx in 0..4 {
        source.push_str(&format!("const K{}: int = spin.spin({})\n", const_idx, 200 + const_idx));
    }
    source.push_str(&format!(
        "fun body_{}(): int\n  const L: int = spin.spin(300)\n  ret L@ + K0@ + {}\nend fun\n",
        module_idx, salt,
    ));
    source
}

/// A warm pipeline over the const world, caching consts or not.
fn const_pipeline(cache: bool) -> (Database, ModuleCompilationPipeline) {
    let db = Database::default();
    let options = datalove_datafun::pipeline::CompilerOptions {
        cache_consts: cache,
        ..Default::default()
    };
    let mut pipeline = ModuleCompilationPipeline::new(options);
    pipeline.add_module(&db, "local", "pkg", "spin", SPIN_SOURCE);
    for i in 0..CONST_MODULES {
        pipeline.add_module(&db, "local", "pkg", &format!("c{}", i), &const_module_source(i, 0));
    }
    compile_fresh(&db, &mut pipeline);
    (db, pipeline)
}

/// Recompiling the const world unchanged, with and without the const cache.
#[divan::bench(sample_count = 50, args = [false, true])]
fn consts_unchanged_recompile(bencher: Bencher, cache: bool) {
    let state = RefCell::new(const_pipeline(cache));

    bencher.bench_local(|| {
        let mut held = state.borrow_mut();
        let (db, pipeline) = &mut *held;
        let (compiled, _) = pipeline.compile(db);
        assert!(compiled.is_successful());
    });
}

/// One const module edited, which nothing requires, with and without the cache.
#[divan::bench(sample_count = 50, args = [false, true])]
fn consts_one_module_edit(bencher: Bencher, cache: bool) {
    let state = RefCell::new(const_pipeline(cache));
    let salt = RefCell::new(0usize);

    bencher.bench_local(|| {
        let mut held = state.borrow_mut();
        let (db, pipeline) = &mut *held;
        *salt.borrow_mut() += 1;
        pipeline.update_source(db, "local", "pkg", "c0", &const_module_source(0, *salt.borrow()));
        let (compiled, _) = pipeline.compile(db);
        assert!(compiled.is_successful());
    });
}

/// The module every const calls into edited, so every const is evaluated again.
///
/// The cache's worst case: everything misses, and what it costs to have asked is
/// all it adds.
#[divan::bench(sample_count = 50, args = [false, true])]
fn consts_shared_module_edit(bencher: Bencher, cache: bool) {
    let state = RefCell::new(const_pipeline(cache));
    let salt = RefCell::new(0usize);

    bencher.bench_local(|| {
        let mut held = state.borrow_mut();
        let (db, pipeline) = &mut *held;
        *salt.borrow_mut() += 1;
        let source = format!("{}fun probe(): int\n  ret {}\nend fun\n", SPIN_SOURCE, *salt.borrow());
        pipeline.update_source(db, "local", "pkg", "spin", &source);
        let (compiled, _) = pipeline.compile(db);
        assert!(compiled.is_successful());
    });
}
