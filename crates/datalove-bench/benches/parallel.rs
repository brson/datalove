//! Benchmarks for parallel vs sequential parsing and typechecking.

use std::collections::BTreeMap;
use bct::input::Source;
use bct::module_graph::ModuleGraphBuilder;
use datalove_datafun_compiler::{
    Database,
    module_graph::{ModuleGraph, parse_module_graph, parse_module_graph_with_mode, ParallelMode},
};
use datalove_datafun_tycheck::{
    typecheck_module_graph_with_mode,
    ParallelMode as TypecheckParallelMode,
};

const NUM_MODULES: usize = 20;
const FUNCTIONS_PER_MODULE: usize = 50;

fn main() {
    divan::main();
}

/// Generate source code for a module with many functions.
fn generate_module_source(module_idx: usize, num_functions: usize) -> String {
    let mut source = String::new();

    for func_idx in 0..num_functions {
        // Generate a function with some arithmetic and control flow.
        source.push_str(&format!(
            "fun func_{}_{}(a: @i32, b: @i32): @i32\n",
            module_idx, func_idx
        ));
        source.push_str("  let x = a +? b\n");
        source.push_str("  let y = x *? @2\n");
        source.push_str("  if y >? @100\n");
        source.push_str("    ret y -? @50\n");
        source.push_str("  else\n");
        source.push_str("    ret y +? @50\n");
        source.push_str("  end if\n");
        source.push_str("end fun\n\n");
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

#[divan::bench]
fn parse_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher.bench_local(|| {
        let db = Database::default();
        let graph = setup_graph(&db, &sources);
        divan::black_box(
            parse_module_graph_with_mode(&db, graph, BTreeMap::new(), ParallelMode::Sequential)
        );
    });
}

#[divan::bench]
fn parse_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher.bench_local(|| {
        let db = Database::default();
        let graph = setup_graph(&db, &sources);
        divan::black_box(
            parse_module_graph_with_mode(&db, graph, BTreeMap::new(), ParallelMode::Parallel)
        );
    });
}

#[divan::bench]
fn typecheck_sequential(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher.bench_local(|| {
        let db = Database::default();
        let graph = setup_graph(&db, &sources);
        let parsed = parse_module_graph(&db, graph, BTreeMap::new());
        divan::black_box(
            typecheck_module_graph_with_mode(&db, parsed, TypecheckParallelMode::Sequential)
        );
    });
}

#[divan::bench]
fn typecheck_parallel(bencher: divan::Bencher) {
    let sources = generate_sources();
    bencher.bench_local(|| {
        let db = Database::default();
        let graph = setup_graph(&db, &sources);
        let parsed = parse_module_graph(&db, graph, BTreeMap::new());
        divan::black_box(
            typecheck_module_graph_with_mode(&db, parsed, TypecheckParallelMode::Parallel)
        );
    });
}
