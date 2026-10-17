//! Each front-end phase of a cold `sys/std` compile, timed sequential and parallel.
//!
//! Parsing happens in the first row, package resolution and the dependency
//! walk, which parses every module it reaches. The parse row is phase 1's
//! warm-up of memos that are already there, and what is left of it is the
//! sequential aggregator; see `botdocs/compile-time.md`.
//!
//! Scratch tool. `cargo run --release -p datalove-bench --example par_phases -- [iterations]`.

use datalove_datafun_compiler::instances::GenericsPolicy;
use rmx::prelude::*;

use datalove_datafun as datafun;
use datalove_datafun::incremental::{IncrementalModuleWorld, Roots, extract_dependencies_with_mode};
use datalove_datafun::pipeline::WorkspaceDescriptor;
use datalove_datafun_compiler::module_graph::parse_module_graph_with_mode;
use datalove_datafun_compiler::tracked_ownership_analysis::analyze_module_graph_with_mode;
use datalove_datafun_compiler::tracked_lower::lower_module_graph_with_evaluator;
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_resolve::resolve_all_names_with_mode;
use datalove_datafun_tycheck::{AutoAdaptMode, ParallelMode, typecheck_module_graph_with_mode};
use std::time::{Duration, Instant};

const N: usize = 6;

fn one(modules: &[(String, String)], riders: &[(String, String)], mode: ParallelMode) -> [Duration; N] {
    let db = datafun::Database::default();
    let mut world = IncrementalModuleWorld::new();
    for (path, source) in modules {
        world.add_module(&db, path, source);
    }
    let mut d = [Duration::ZERO; N];
    let t = Instant::now();
    let deps = extract_dependencies_with_mode(&world, &db, &Roots::All, mode);
    let (graph, rr) = world.build_graph(&db, deps, &Roots::All);
    d[0] = t.elapsed();
    let riders = riders.iter()
        .map(|(n, s)| (n.clone(), bct::input::Source::new(&db, s.clone())))
        .collect();
    let t = Instant::now();
    let parsed = parse_module_graph_with_mode(&db, graph, rr, riders, mode);
    d[1] = t.elapsed();
    let t = Instant::now();
    let names = resolve_all_names_with_mode(&db, parsed, mode);
    d[2] = t.elapsed();
    let t = Instant::now();
    let tc = typecheck_module_graph_with_mode(&db, parsed, names, mode, AutoAdaptMode::Disabled);
    d[3] = t.elapsed();
    let t = Instant::now();
    let own = analyze_module_graph_with_mode(&db, parsed, tc, mode, AutoAdaptMode::Disabled);
    d[4] = t.elapsed();
    let t = Instant::now();
    let mut ev = InterpCtfeEvaluator::new();
    let _l = lower_module_graph_with_evaluator(&db, parsed, tc, own, mode, &mut ev, false, false, GenericsPolicy::Off, None);
    d[5] = t.elapsed();
    std::mem::forget(db);
    d
}

fn main() {
    let iters: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(30);
    let sys = datalove_sys_packages::system_library();
    let descriptor = WorkspaceDescriptor::from_system_library(&sys);
    let library = descriptor.system_library.as_ref().unwrap();
    let mut modules = Vec::new();
    let mut riders = Vec::new();
    for (pn, p) in &library.packages {
        for (mn, m) in &p.modules {
            modules.push((format!("sys/{}/{}", pn, mn), m.source.to_string()));
        }
        if let Some(r) = &p.rider {
            riders.push((pn.C(), r.interface_source.to_string()));
        }
    }
    println!("{} modules, rayon threads {}", modules.len(), rmx::rayon::current_num_threads());
    let names = ["deps+parse", "parse", "resolve", "typecheck", "ownership", "lower(5)"];
    let mut res = Vec::new();
    for mode in [ParallelMode::Sequential, ParallelMode::Parallel] {
        let _ = one(&modules, &riders, mode);
        let mut best = [Duration::MAX; N];
        for _ in 0..iters {
            let d = one(&modules, &riders, mode);
            for i in 0..N { best[i] = best[i].min(d[i]); }
        }
        res.push(best);
    }
    println!("{:<10} {:>9} {:>9} {:>7}", "phase", "seq ms", "par ms", "speedup");
    let (mut ts, mut tp) = (0.0, 0.0);
    for i in 0..N {
        let s = res[0][i].as_secs_f64() * 1e3;
        let p = res[1][i].as_secs_f64() * 1e3;
        ts += s; tp += p;
        println!("{:<10} {:>9.2} {:>9.2} {:>6.2}x", names[i], s, p, s / p);
    }
    println!("{:<10} {:>9.2} {:>9.2} {:>6.2}x", "total", ts, tp, ts / tp);
}
