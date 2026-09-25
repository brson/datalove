//! What salsa actually runs, per compile, on the system library plus a local
//! module.
//!
//! The recompile examples say how long a compile took. This says what it did:
//! every query salsa executed, counted by name. A phase that should be
//! memoized and is not shows up here as a row, which no timing does.
//!
//! Scratch tool. `cargo run --release -p datalove-bench --example query_census`.

use rmx::std::collections::BTreeMap;

use datalove_ct::query_events::QueryRecorder;
use datalove_datafun as datafun;
use datafun::pipeline::WorkspaceDescriptor;

/// A local module that calls into the system library, as a real one would.
fn local_source(salt: usize) -> String {
    format!(
        "fun local_a(x: int): int\n  ret x + {}\nend fun\n\
         fun local_b(x: int): int\n  ret local_a(x) * 2\nend fun\n",
        salt,
    )
}

fn counts(recorder: &QueryRecorder) -> BTreeMap<String, usize> {
    let mut by_query: BTreeMap<String, usize> = BTreeMap::new();
    for executed in recorder.take() {
        *by_query.entry(executed.query).or_default() += 1;
    }
    by_query
}

fn report(label: &str, by_query: BTreeMap<String, usize>) {
    let total: usize = by_query.values().sum();
    println!("\n=== {label}: {total} queries");
    let mut rows: Vec<_> = by_query.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (query, count) in rows.iter().take(18) {
        println!("  {count:>5}  {query}");
    }
}

fn main() {
    let recorder = QueryRecorder::new();
    let mut db = datafun::Database::recording(&recorder);

    let sys = datalove_stdlib::system_library();
    let descriptor = WorkspaceDescriptor::from_system_library(&sys);
    let mut pipeline = descriptor.to_pipeline(&db);
    pipeline.add_module(&db, "local", "app", "main", &local_source(0));

    let compiled = pipeline.compile_fresh(&db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);
    report("cold compile", counts(&recorder));

    // Nothing changed.
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors());
    drop(compiled);
    report("unchanged recompile", counts(&recorder));

    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors());
    drop(compiled);
    report("second unchanged recompile", counts(&recorder));

    // A body edit in the local module, which is what an editing session does.
    pipeline.update_source(&mut db, "local", "app", "main", &local_source(1));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);
    report("edit one local module", counts(&recorder));

    // A body edit deep in the system library, which everything depends on.
    let (package, module, text) = sys.library.packages.iter()
        .flat_map(|(p, pkg)| pkg.modules.iter().map(move |(m, d)| (p.clone(), m.clone(), d.source.to_string())))
        .next()
        .expect("the system library has modules");
    let probe = |salt: usize| format!("{}\nfun probe_edit(): int\n  ret {}\nend fun\n", text, salt);
    pipeline.update_source(&mut db, "sys", &package, &module, &probe(0));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();

    pipeline.update_source(&mut db, "sys", &package, &module, &probe(1));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);
    report(&format!("edit sys/{package}/{module}"), counts(&recorder));
}
