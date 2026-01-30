//! Benchmarks for parallel vs sequential compilation phases.

use std::collections::BTreeMap;
use bct::input::Source;
use bct::module_graph::ModuleGraphBuilder;
use std::cell::RefCell;
use std::rc::Rc;
use datalove_datafun_compiler::{
    Database,
    module_graph::{ModuleGraph, parse_module_graph, parse_module_graph_with_mode, ParallelMode},
    tracked_ownership_analysis::analyze_module_graph_with_mode,
    tracked_lower::lower_module_graph_with_evaluator,
};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::{
    typecheck_module_graph, typecheck_module_graph_with_mode,
    resolve_all_names_with_mode,
    ParallelMode as TypecheckParallelMode,
    AutoAdaptMode,
};

// Enough modules for parallelism, small enough for fast benchmarks.
const NUM_MODULES: usize = 32;
const FUNCTIONS_PER_MODULE: usize = 30;
const TYPE_ALIASES_PER_MODULE: usize = 5;

fn main() {
    divan::main();
}

/// Generate source code for a module with type aliases and functions.
///
/// The generated code exercises:
/// - Type aliases (for parser/tycheck work)
/// - Owned types like `int` (bigint) and lists (for ownership analysis)
/// - Clone operations `$` (for ownership tracking)
/// - Complex control flow with loops and conditionals (for lowering)
/// - Mutable variables (for lowering)
/// - Checked arithmetic (for lowering)
fn generate_module_source(module_idx: usize, num_functions: usize) -> String {
    let mut source = String::new();

    // Generate type aliases.
    for alias_idx in 0..TYPE_ALIASES_PER_MODULE {
        source.push_str(&format!(
            "type Alias{}_{}: int\n",
            module_idx, alias_idx
        ));
    }
    source.push('\n');

    // Generate a struct type alias for ownership testing.
    source.push_str(&format!(
        "type Data{}: {{x: int, y: int}}\n\n",
        module_idx
    ));

    for func_idx in 0..num_functions {
        // Vary function signatures to exercise different code paths.
        let variant = func_idx % 4;

        match variant {
            0 => {
                // Function with owned int types, cloning, and loop.
                source.push_str(&format!(
                    "fun func_{}_{}(n: int, acc: int): !int\n",
                    module_idx, func_idx
                ));
                source.push_str("  var i: u32 = @0\n");
                source.push_str("  var result: int = acc$\n");
                source.push_str("  loop while i .< @10\n");
                source.push_str("    set result = result + n$\n");
                source.push_str("    set i = i +! @1\n");
                source.push_str("  end loop\n");
                source.push_str("  ret ok result\n");
                source.push_str("end fun\n\n");
            }
            1 => {
                // Function with ref parameter (ownership borrowing).
                source.push_str(&format!(
                    "fun func_{}_{}(ref x: int, y: int): int\n",
                    module_idx, func_idx
                ));
                source.push_str("  let a = x$ + y\n");
                source.push_str("  let b = a$ * :int/@2\n");
                source.push_str("  if b .> :int/@1000\n");
                source.push_str("    ret b - :int/@500\n");
                source.push_str("  else\n");
                source.push_str("    ret b + :int/@500\n");
                source.push_str("  end if\n");
                source.push_str("end fun\n\n");
            }
            2 => {
                // Function with option type and early return.
                source.push_str(&format!(
                    "fun func_{}_{}(val: ?int): ?int\n",
                    module_idx, func_idx
                ));
                source.push_str("  let x = val?\n");
                source.push_str("  let y = x$ * :int/@3\n");
                source.push_str("  let z = y$ + :int/@100\n");
                source.push_str("  if z .> :int/@500\n");
                source.push_str("    ret some z - :int/@200\n");
                source.push_str("  else\n");
                source.push_str("    ret some z\n");
                source.push_str("  end if\n");
                source.push_str("end fun\n\n");
            }
            3 => {
                // Function with result type, nested conditionals, mutable state.
                source.push_str(&format!(
                    "fun func_{}_{}(a: int, b: int, cond: bool): !int\n",
                    module_idx, func_idx
                ));
                source.push_str("  var result: int = a$\n");
                source.push_str("  if cond\n");
                source.push_str("    set result = result + b$\n");
                source.push_str("    if result .> :int/@100\n");
                source.push_str("      set result = result * :int/@2\n");
                source.push_str("    else\n");
                source.push_str("      set result = result + :int/@50\n");
                source.push_str("    end if\n");
                source.push_str("  else\n");
                source.push_str("    set result = result - b\n");
                source.push_str("  end if\n");
                source.push_str("  ret ok result\n");
                source.push_str("end fun\n\n");
            }
            _ => unreachable!(),
        }
    }

    source
}

/// Pre-generate module sources (expensive, done once outside benchmark).
fn generate_sources() -> Vec<(String, String)> {
    (0..NUM_MODULES)
        .map(|i| {
            let path = format!("bench/module_{}", i);
            let source = generate_module_source(i, FUNCTIONS_PER_MODULE);
            (path, source)
        })
        .collect()
}

/// Set up a module graph for benchmarking.
fn setup_graph(db: &Database, sources: &[(String, String)]) -> ModuleGraph {
    let mut builder = ModuleGraphBuilder::new(db);
    for (path, source_text) in sources {
        let src = Source::new(db, source_text.clone());
        builder.add_module(path.clone(), src);
    }
    builder.build()
}

// Benchmarks prime prior phases in setup, then re-call them (memoized) during measurement.
// This isolates the target phase timing since memoized calls are essentially free.

#[divan::bench]
fn parse_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let result = parse_module_graph_with_mode(&db, graph, BTreeMap::new(), ParallelMode::Sequential);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn parse_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let result = parse_module_graph_with_mode(&db, graph, BTreeMap::new(), ParallelMode::Parallel);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn resolve_names_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            // Prime: parse is memoized after this.
            let _ = parse_module_graph(&db, graph, BTreeMap::new());
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new()); // Memoized.
            let result = resolve_all_names_with_mode(&db, parsed, TypecheckParallelMode::Sequential);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn resolve_names_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            let _ = parse_module_graph(&db, graph, BTreeMap::new());
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let result = resolve_all_names_with_mode(&db, parsed, TypecheckParallelMode::Parallel);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn typecheck_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            // Prime: parse is memoized after this.
            let _ = parse_module_graph(&db, graph, BTreeMap::new());
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new()); // Memoized.
            let result = typecheck_module_graph_with_mode(&db, parsed, TypecheckParallelMode::Sequential);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn typecheck_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            let _ = parse_module_graph(&db, graph, BTreeMap::new());
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let result = typecheck_module_graph_with_mode(&db, parsed, TypecheckParallelMode::Parallel);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn ownership_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let _ = typecheck_module_graph(&db, parsed);
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let typechecked = typecheck_module_graph(&db, parsed);
            let result = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn ownership_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let _ = typecheck_module_graph(&db, parsed);
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let typechecked = typecheck_module_graph(&db, parsed);
            let result = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Parallel);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn lower_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let typechecked = typecheck_module_graph(&db, parsed);
            let _ = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential);
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let typechecked = typecheck_module_graph(&db, parsed);
            let ownership = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential);
            let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
            let result = lower_module_graph_with_evaluator(&db, parsed, typechecked, ownership, ParallelMode::Sequential, evaluator);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn lower_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let graph = setup_graph(&db, &sources);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let typechecked = typecheck_module_graph(&db, parsed);
            let _ = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential);
            (db, graph)
        })
        .bench_local_values(|(db, graph)| {
            let parsed = parse_module_graph(&db, graph, BTreeMap::new());
            let typechecked = typecheck_module_graph(&db, parsed);
            let ownership = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential);
            let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
            let result = lower_module_graph_with_evaluator(&db, parsed, typechecked, ownership, ParallelMode::Parallel, evaluator);
            let _ = divan::black_box(result);
        });
}
