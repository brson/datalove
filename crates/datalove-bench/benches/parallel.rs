//! Benchmarks for parallel vs sequential compilation phases.

use std::collections::BTreeMap;
use bct::input::Source;
use bct::module_graph::ModuleGraphBuilder;
use std::cell::RefCell;
use std::rc::Rc;
use datalove_datafun_compiler::{
    Database,
    compile::{compile_modules, ModuleCompilationInput},
    module_graph::{ModuleGraph, parse_module_graph, parse_module_graph_with_mode, ParallelMode},
    tracked_ownership_analysis::analyze_module_graph_with_mode,
    tracked_lower::lower_module_graph_with_evaluator,
};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_resolve::{
    resolve_all_names_with_mode, resolve_all_exports, build_all_function_ast_maps,
};
use datalove_datafun_tycheck::{
    typecheck_module_graph, typecheck_module_graph_with_mode,
    ParallelMode as TypecheckParallelMode,
    AutoAdaptMode,
};

// Enough modules for parallelism, small enough for fast benchmarks.
const NUM_MODULES: usize = 32;
const FUNCTIONS_PER_MODULE: usize = 30;
const TYPE_ALIASES_PER_MODULE: usize = 5;

fn main() {
    // The generated source must actually compile, or every phase below is
    // timing an error path instead of the work it claims to measure.
    verify_generated_source_compiles();
    divan::main();
}

/// Compile the benchmark source once and panic if it produces any diagnostic.
fn verify_generated_source_compiles() {
    let sources = generate_sources();
    let db = Database::default();
    let srcs = setup_sources(&db, &sources);
    let graph = setup_graph(&db, &srcs);
    let input = ModuleCompilationInput {
        graph,
        resolved_requires: BTreeMap::new(),
    };
    let output = compile_modules(&db, input, Vec::new(), ParallelMode::Sequential);
    if output.is_successful() {
        return;
    }

    let mut report = String::from("benchmark source does not compile:\n");
    let phases = [
        ("parse", &output.parse_errors),
        ("typecheck", &output.typecheck_errors),
        ("ownership", &output.ownership_errors),
    ];
    for (phase, errors) in phases {
        for (path, messages) in errors.iter().filter(|(_, m)| !m.is_empty()) {
            report.push_str(&format!("  {} {}: {:?}\n", phase, path, messages));
        }
    }
    panic!("{}", report);
}

/// Generate source code for a module with type aliases and functions.
///
/// The generated code exercises:
/// - Type aliases (for parser/tycheck work)
/// - Linear types like `int` (bigint) (for ownership analysis)
/// - Clone operations `@` (for ownership tracking)
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
                source.push_str("  var i: u32 = 0\n");
                source.push_str("  var result: int = acc\n");
                // Comparison does not propagate an expected type to a bare
                // literal the way checked arithmetic does, so the bound is
                // hinted while `i +! 1` below is not.
                source.push_str("  loop while i .< :u32/10\n");
                source.push_str("    set result = result + n\n");
                source.push_str("    set i = i +! 1\n");
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
                source.push_str("  let a = x + y\n");
                source.push_str("  let b = a * 2\n");
                source.push_str("  if b .> 1000\n");
                source.push_str("    ret b - 500\n");
                source.push_str("  else\n");
                source.push_str("    ret b + 500\n");
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
                source.push_str("  let y = x * 3\n");
                source.push_str("  let z = y + 100\n");
                source.push_str("  if z .> 500\n");
                source.push_str("    ret some(z - 200)\n");
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
                source.push_str("  var result: int = a\n");
                source.push_str("  if cond\n");
                source.push_str("    set result = result + b\n");
                source.push_str("    if result .> 100\n");
                source.push_str("      set result = result * 2\n");
                source.push_str("    else\n");
                source.push_str("      set result = result + 50\n");
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

/// Make the `Source` inputs for a set of module texts.
///
/// A `Source` is an input, so making one twice from the same text gives two
/// different inputs and nothing built over the first is reused. These are made
/// once, in setup; the graph is built from them inside the measured closure,
/// where - the graph being interned - it comes back the same graph, and so
/// still primed.
fn setup_sources(db: &Database, sources: &[(String, String)]) -> Vec<(String, Source)> {
    sources.iter()
        .map(|(path, text)| (path.clone(), Source::new(db, text.clone())))
        .collect()
}

/// Set up a module graph for benchmarking.
fn setup_graph<'db>(db: &'db Database, sources: &[(String, Source)]) -> ModuleGraph<'db> {
    let mut builder = ModuleGraphBuilder::new(db);
    for (path, src) in sources {
        builder.add_module(path.clone(), *src);
    }
    builder.build()
}

/// Run parse and name resolution so their memoized results are in place.
///
/// The typecheck benchmarks need this in setup: name resolution feeds
/// typecheck, so without priming it runs unmemoized inside the measured
/// closure, and always sequentially, which both inflates the timings and
/// shrinks the gap the benchmark exists to show.
fn prime_through_resolve<'db>(db: &'db Database, graph: ModuleGraph<'db>) {
    let parsed = parse_module_graph(db, graph, BTreeMap::new(), Vec::new());
    let _ = resolve_all_names_with_mode(db, parsed, TypecheckParallelMode::Sequential);
    let _ = resolve_all_exports(db, parsed);
    let _ = build_all_function_ast_maps(db, parsed);
}

/// Run parse and typecheck sequentially, mirroring the ordering in
/// `compile_modules`.
///
/// Benchmarks for later phases call this both to prime memoization in setup and
/// to obtain their inputs during measurement, where it is essentially free.
fn typecheck_through<'db>(
    db: &'db Database,
    graph: ModuleGraph<'db>,
) -> (
    datalove_datafun_tycheck::ParsedModuleGraph<'db>,
    datalove_datafun_tycheck::ModuleGraphTypecheckResult<'db>,
) {
    let parsed = parse_module_graph(db, graph, BTreeMap::new(), Vec::new());
    let names = resolve_all_names_with_mode(db, parsed, TypecheckParallelMode::Sequential);
    let exports = resolve_all_exports(db, parsed);
    let function_asts = build_all_function_ast_maps(db, parsed);
    let typechecked = typecheck_module_graph(
        db, parsed, names, exports, function_asts, AutoAdaptMode::Disabled,
    );
    (parsed, typechecked)
}

// Benchmarks prime prior phases in setup, then re-call them (memoized) during measurement.
// This isolates the target phase timing since memoized calls are essentially free.

#[divan::bench]
fn parse_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let result = parse_module_graph_with_mode(&db, graph, BTreeMap::new(), Vec::new(), ParallelMode::Sequential);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn parse_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let result = parse_module_graph_with_mode(&db, graph, BTreeMap::new(), Vec::new(), ParallelMode::Parallel);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn resolve_names_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            // Prime: parse is memoized after this.
            let _ = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new()); // Memoized.
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
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            let _ = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
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
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            // Prime: parse and name resolution are memoized after this.
            prime_through_resolve(&db, graph);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new()); // Memoized.
            let names = resolve_all_names_with_mode(&db, parsed, TypecheckParallelMode::Sequential);
            let exports = resolve_all_exports(&db, parsed);
            let function_asts = build_all_function_ast_maps(&db, parsed);
            let result = typecheck_module_graph_with_mode(
                &db, parsed, names, exports, function_asts,
                TypecheckParallelMode::Sequential, AutoAdaptMode::Disabled,
            );
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn typecheck_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            prime_through_resolve(&db, graph);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let parsed = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
            let names = resolve_all_names_with_mode(&db, parsed, TypecheckParallelMode::Sequential);
            let exports = resolve_all_exports(&db, parsed);
            let function_asts = build_all_function_ast_maps(&db, parsed);
            let result = typecheck_module_graph_with_mode(
                &db, parsed, names, exports, function_asts,
                TypecheckParallelMode::Parallel, AutoAdaptMode::Disabled,
            );
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn ownership_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            let _ = typecheck_through(&db, graph);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let (parsed, typechecked) = typecheck_through(&db, graph);
            let result = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential, AutoAdaptMode::Disabled);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn ownership_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            let _ = typecheck_through(&db, graph);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let (parsed, typechecked) = typecheck_through(&db, graph);
            let result = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Parallel, AutoAdaptMode::Disabled);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn lower_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            let (parsed, typechecked) = typecheck_through(&db, graph);
            let _ = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential, AutoAdaptMode::Disabled);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let (parsed, typechecked) = typecheck_through(&db, graph);
            let ownership = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential, AutoAdaptMode::Disabled);
            let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
            let result = lower_module_graph_with_evaluator(&db, parsed, typechecked, ownership, ParallelMode::Sequential, evaluator, false, false);
            let _ = divan::black_box(result);
        });
}

#[divan::bench]
fn lower_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher
        .with_inputs(|| {
            let db = Database::default();
            let srcs = setup_sources(&db, &sources);
            let graph = setup_graph(&db, &srcs);
            let (parsed, typechecked) = typecheck_through(&db, graph);
            let _ = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential, AutoAdaptMode::Disabled);
            (db, srcs)
        })
        .bench_local_values(|(db, srcs)| {
            let graph = setup_graph(&db, &srcs);
            let (parsed, typechecked) = typecheck_through(&db, graph);
            let ownership = analyze_module_graph_with_mode(&db, parsed, typechecked, ParallelMode::Sequential, AutoAdaptMode::Disabled);
            let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
            let result = lower_module_graph_with_evaluator(&db, parsed, typechecked, ownership, ParallelMode::Parallel, evaluator, false, false);
            let _ = divan::black_box(result);
        });
}
