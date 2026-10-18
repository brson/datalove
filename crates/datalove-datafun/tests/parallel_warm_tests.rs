//! The parallel compile computes every per-module query ahead of its step.
//!
//! Each step of the compile asks a query once per module, and under
//! `ParallelMode::Parallel` those queries are computed first on rayon's pool
//! (`datalove_datafun_common::warm`), from the same function the step asks
//! with, so that the step finds every one in its memo. If the two ever keyed a
//! query differently nothing would be wrong, only slower: the step would
//! compute its own on the thread that called the compile, and the warmed one
//! would go to waste. So that is what this watches for, by asking salsa which
//! thread each query ran on.

use rmx::prelude::*;

use datalove_ct::query_events::QueryRecorder;
use datalove_datafun::pipeline::ModuleCompilationPipeline;
use datalove_datafun_common::ParallelMode;
use datalove_datafun_compiler::Database;

const MODULES: usize = 8;

/// A module with something for every step to do: functions, one of them
/// inline and one naming a module const, a module const, and a const in a
/// function's body.
fn module_source(index: usize) -> String {
    format!(
        "const BASE{index}: int = {index} * 10\n\
         \n\
         fun twice{index}(x: int): int\n  inline\n  ret x + x\nend fun\n\
         \n\
         fun f{index}(x: int, y: int): int\n\
         \x20 const K: int = 3 + 4\n\
         \x20 let s = twice{index}(x) + y * K\n\
         \x20 if s .> 100\n    ret s - 50\n  end if\n\
         \x20 ret s + BASE{index}\n\
         end fun\n"
    )
}

/// The per-module queries the parallel compile computes ahead of their steps.
const WARMED: &[&str] = &[
    "module_import_demands",
    "parse_module_full",
    "resolve_module_names",
    "resolve_module_imports",
    "typecheck_module",
    "analyze_module",
    "lower_module_functions",
    "closure_fingerprint",
    "ctfe_module_units",
    "const_inlined_module",
    "module_has_comptime_calls",
    "lower_module",
];

#[test]
fn every_per_module_query_runs_ahead_of_its_step() {
    let recorder = QueryRecorder::new();
    let db = Database::recording(&recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    for index in 0..MODULES {
        pipeline.add_module(&db, "local", "test", &fmt!("m{index}"), &module_source(index));
    }
    let compiled = pipeline.compile_fresh_with_mode(&db, ParallelMode::Parallel);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    for query in WARMED {
        let runs: Vec<_> = executed.iter().filter(|q| q.query == *query).collect();
        assert!(!runs.is_empty(), "`{query}` never ran, so this proves nothing about it");
        let stepped: Vec<_> = runs.iter().filter(|q| !q.on_worker).collect();
        assert!(
            stepped.is_empty(),
            "`{query}` ran {} of {} times on the compiling thread rather than ahead of its step: \
             its warm-up keys it otherwise than its step does",
            stepped.len(), runs.len(),
        );
    }
}
