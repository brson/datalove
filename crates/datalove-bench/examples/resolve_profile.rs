//! Where package resolution's time goes.
//!
//! Resolution is 28% of a cold compile and 92% of one that compiles only what
//! the roots reach (`cold_phases`), so it is the floor for any invocation. All it
//! is supposed to do is read `require` lines and work out which module each one
//! names, which should not cost what it costs.
//!
//! The last row is the one to look at: it parses every module *again*, the way
//! phase 1 does, after resolution has already parsed them all. If that row is
//! about as expensive as resolution's own parse, the world is being parsed twice
//! and resolution's share is mostly a duplicate.
//!
//! Scratch tool. `cargo run --release -p datalove-bench --example resolve_profile -- [iterations]`.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
use datalove_datafun as datafun;
use datalove_datafun::incremental::{IncrementalModuleWorld, Roots};
use datalove_datafun::pipeline::WorkspaceDescriptor;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Steps {
    /// Interning the packages and modules into a `PackageWorld`.
    intern: Duration,
    /// `import_demands`: finding each module's `require` lines.
    demands: Duration,
    /// Resolving each demand to a module, and the validation that goes with it.
    resolve: Duration,
    /// `to_module_graph` and the path walk after it.
    to_graph: Duration,
    /// `build_graph`: the topological sort and the `ModuleGraph`.
    build: Duration,
    /// Parsing every module again, the way phase 1 does.
    reparse: Duration,
}

fn one_run(modules: &[(String, String)]) -> Steps {
    let db = datafun::Database::default();
    let mut world = IncrementalModuleWorld::new();
    for (path, source) in modules {
        world.add_module(&db, path, source);
    }

    let mut steps = Steps::default();

    // Split the libraries the way `extract_dependencies` does.
    let mut pkglib_system: BTreeMap<String, BTreeMap<String, Source>> = BTreeMap::new();
    let mut pkglib_local: BTreeMap<String, BTreeMap<String, Source>> = BTreeMap::new();
    for (path, source) in world.sources() {
        let parts: Vec<&str> = path.split('/').collect();
        let pkglib = match parts[0] {
            "sys" => &mut pkglib_system,
            _ => &mut pkglib_local,
        };
        pkglib.entry(parts[1].S()).or_default().insert(parts[2].S(), *source);
    }

    let t = Instant::now();
    let package_world = datafun::package::import_with_sources(&db, pkglib_system, pkglib_local);
    let map = datafun::package_world_map(&db, package_world);
    steps.intern = t.elapsed();

    let t = Instant::now();
    let demands = datafun::import_demands::import_demands(&db, map);
    steps.demands = t.elapsed();

    let t = Instant::now();
    let resolution = datafun::package_resolve::resolve_package_world_with_imports(
        &db, package_world);
    let _ = demands;
    steps.resolve = t.elapsed();

    let t = Instant::now();
    let pkg_graph = resolution.result(&db).expect("the system library resolves");
    let _with_requires = datafun::to_module_graph(&db, package_world, pkg_graph);
    steps.to_graph = t.elapsed();

    // `dependencies_of` is tracked and sits over the three steps above, so ask
    // for it now: everything under it is warm and this is the aggregation only.
    let t = Instant::now();
    let deps = datalove_datafun::incremental::extract_dependencies(&world, &db);
    let _ = world.build_graph(&db, deps, &Roots::All);
    steps.build = t.elapsed();

    // And now the parse phase 1 does. If resolution had shared its parse, this
    // would be a memo hit.
    let t = Instant::now();
    let (graph, _) = world.build_graph(&db, deps, &Roots::All);
    for module in graph.iter_modules(&db) {
        let _ = datalove_datafun_parser::parse_module_full(&db, module);
    }
    steps.reparse = t.elapsed();

    steps
}

fn main() {
    let iterations: usize = std::env::args().nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(15);

    let sys = datalove_stdlib::system_library();
    let descriptor = WorkspaceDescriptor::from_system_library(&sys);
    let library = descriptor.system_library.as_ref().expect("the system library is there");

    let mut modules: Vec<(String, String)> = Vec::new();
    for (package_name, package) in &library.packages {
        for (module_name, module) in &package.modules {
            modules.push((
                format!("sys/{}/{}", package_name, module_name),
                module.source.to_string(),
            ));
        }
    }
    modules.push(("local/app/main".S(),
        "fun local_a(x: int): int\n  ret x + 1\nend fun\n".S()));

    println!("{} modules, {} iterations", modules.len(), iterations);

    let _ = one_run(&modules);

    let mut best = Steps {
        intern: Duration::MAX, demands: Duration::MAX, resolve: Duration::MAX,
        to_graph: Duration::MAX, build: Duration::MAX, reparse: Duration::MAX,
    };
    for _ in 0..iterations {
        let s = one_run(&modules);
        best.intern = best.intern.min(s.intern);
        best.demands = best.demands.min(s.demands);
        best.resolve = best.resolve.min(s.resolve);
        best.to_graph = best.to_graph.min(s.to_graph);
        best.build = best.build.min(s.build);
        best.reparse = best.reparse.min(s.reparse);
    }

    let resolution_total = best.intern + best.demands + best.resolve + best.to_graph + best.build;
    let row = |name: &str, d: Duration| {
        println!("  {name:<34} {:>8.2}ms   {:>5.1}% of resolution",
            d.as_secs_f64() * 1e3,
            d.as_secs_f64() / resolution_total.as_secs_f64() * 100.0);
    };
    println!("\npackage resolution, best of {iterations}:");
    row("intern packages", best.intern);
    row("import_demands (parses the world)", best.demands);
    row("resolve demands to modules", best.resolve);
    row("to_module_graph + path walk", best.to_graph);
    row("build_graph (sort + intern)", best.build);
    println!("  {:<34} {:>8.2}ms", "resolution total", resolution_total.as_secs_f64() * 1e3);
    println!();
    row("phase 1 parse, after all that", best.reparse);
}
