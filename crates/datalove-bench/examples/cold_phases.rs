//! Where a cold compile's time goes, split at the seam reachability would use.
//!
//! Compiling only what is reachable from a program's entry points changes what a
//! cold compile costs, and the question is where to draw the line. Checking the
//! whole world and lowering only what is reachable keeps a type error in an
//! unimported module an error, which pruning the frontend too would not -- but it
//! is only worth doing if phase 5 onward is a large enough share to pay for
//! itself.
//!
//! The pipeline already has that seam: `compile_modules` returns after phase 4,
//! and `lower_module_graph_with_evaluator` is a separate call. This drives the
//! two halves directly, on a fresh database each iteration so nothing is a memo
//! hit, and reports the split.
//!
//! Scratch tool. `cargo run --release -p datalove-bench --example cold_phases -- [iterations]`.

use rmx::prelude::*;

use datalove_datafun as datafun;
use datalove_datafun::incremental::{IncrementalModuleWorld, Roots, extract_dependencies};
use datalove_datafun::pipeline::WorkspaceDescriptor;
use datalove_datafun_compiler::compile::{ModuleCompilationInput, compile_modules};
use datalove_datafun_compiler::tracked_lower::lower_module_graph_with_evaluator;
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::ParallelMode;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A local module that calls into the system library, as a real one would.
const LOCAL: &str = "fun local_a(x: int): int\n  ret x + 1\nend fun\n\
                     fun local_b(x: int): int\n  ret local_a(x) * 2\nend fun\n";

#[derive(Default)]
struct Split {
    /// Package resolution and building the module graph.
    resolve: Duration,
    /// Phases 1 to 4: parse, name resolution, typecheck, ownership.
    check: Duration,
    /// Phase 5: lowering, const evaluation, specialization, assembly.
    lower: Duration,
}

fn one_cold_compile(
    modules: &[(String, String)],
    rider_sources: &[(String, String)],
    roots: &Roots,
) -> Split {
    let db = datafun::Database::default();
    let mut world = IncrementalModuleWorld::new();
    for (path, source) in modules {
        world.add_module(&db, path, source);
    }

    let mut split = Split::default();

    let t = Instant::now();
    let deps = extract_dependencies(&world, &db, roots);
    let (graph, resolved_requires) = world.build_graph(&db, deps, roots);
    split.resolve = t.elapsed();

    let t = Instant::now();
    let output = compile_modules(
        &db,
        ModuleCompilationInput { graph, resolved_requires },
        rider_sources.to_vec(),
        ParallelMode::Sequential,
    );
    assert!(output.is_successful(), "{:?} {:?}", output.typecheck_errors, output.ownership_errors);
    split.check = t.elapsed();

    let t = Instant::now();
    let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
    let _lowering = lower_module_graph_with_evaluator(
        &db,
        output.parsed_graph,
        output.typecheck_result,
        output.ownership_analysis,
        ParallelMode::Sequential,
        evaluator,
        false,
        false,
    );
    split.lower = t.elapsed();

    split
}

fn main() {
    let iterations: usize = std::env::args().nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(10);

    let sys = datalove_stdlib::system_library();
    let descriptor = WorkspaceDescriptor::from_system_library(&sys);

    let library = descriptor.system_library.as_ref().expect("the system library is there");
    let mut modules: Vec<(String, String)> = Vec::new();
    let mut rider_sources: Vec<(String, String)> = Vec::new();
    for (package_name, package) in &library.packages {
        for (module_name, module) in &package.modules {
            modules.push((
                format!("sys/{}/{}", package_name, module_name),
                module.source.to_string(),
            ));
        }
        if let Some(rider) = &package.rider {
            rider_sources.push((package_name.C(), rider.interface_source.to_string()));
        }
    }
    modules.push(("local/app/main".S(), LOCAL.S()));

    println!(
        "{} modules, {} rider(s), {} iterations",
        modules.len(), rider_sources.len(), iterations,
    );

    // The first is discarded: it pays for lazily-initialised statics and for
    // whatever the allocator has to ask the kernel for, and it is not the
    // number anyone would act on.
    let _ = one_cold_compile(&modules, &rider_sources, &Roots::All);

    // `all` compiles the world; `reachable` compiles what the local module
    // reaches, which for a program that requires nothing is the program.
    let scenarios: Vec<(&str, Roots)> = vec![
        ("whole world", Roots::All),
        ("reachable from local/app/main", Roots::From(
            std::iter::once("local/app/main".S()).collect())),
    ];

    for (label, roots) in scenarios {
    let mut best = Split { resolve: Duration::MAX, check: Duration::MAX, lower: Duration::MAX };
    let mut total = Split::default();
    for _ in 0..iterations {
        let split = one_cold_compile(&modules, &rider_sources, &roots);
        best.resolve = best.resolve.min(split.resolve);
        best.check = best.check.min(split.check);
        best.lower = best.lower.min(split.lower);
        total.resolve += split.resolve;
        total.check += split.check;
        total.lower += split.lower;
    }

    let n = iterations as u32;
    let row = |name: &str, best: Duration, mean: Duration, whole: Duration| {
        println!(
            "  {name:<26} best {:>8.2}ms   mean {:>8.2}ms   {:>5.1}%",
            best.as_secs_f64() * 1e3,
            mean.as_secs_f64() * 1e3,
            mean.as_secs_f64() / whole.as_secs_f64() * 100.0,
        );
    };
    let whole = (total.resolve + total.check + total.lower) / n;
    println!("\ncold compile, {label}, by phase:");
    row("package resolve + graph", best.resolve, total.resolve / n, whole);
    row("phases 1-4 (check)", best.check, total.check / n, whole);
    row("phase 5 (lower)", best.lower, total.lower / n, whole);
    println!("  {:<26} best {:>8.2}ms   mean {:>8.2}ms",
        "whole",
        (best.resolve + best.check + best.lower).as_secs_f64() * 1e3,
        whole.as_secs_f64() * 1e3);
    }
}
