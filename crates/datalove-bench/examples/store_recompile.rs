//! Recompiles of the store demo, for timing what an edit costs in a real world.
//!
//! The synthetic worlds have no consts worth the name, so they cannot say what
//! evaluating consts costs. The store has two built from twenty thousand orders
//! at compile time, under a module the rest of the store imports.
//!
//! Scratch tool, not a test. Generate the demo's data first (`just gen` in
//! `demos/store`), then `cargo run --release -p datalove-bench --example
//! store_recompile -- [--no-cache] [iterations] [noop|edit:<module>] [local
//! dir]`. `--no-cache` evaluates every const on every compile, which is what
//! the const cache saves.
//!
//! An edit appends a function to `local/store/<module>`, a different one each
//! time, so it is a real change to that module and to nothing else.

use std::time::{Duration, Instant};

use datalove_datafun as datafun;
use datafun::pipeline::rider_load::RiderNatives;
use datafun::pipeline::{load_local_library, WorkspaceDescriptor};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let no_cache = args.iter().any(|a| a == "--no-cache");
    args.retain(|a| a != "--no-cache");
    let mut args = args.into_iter();
    let iterations: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(20);
    let mode = args.next().unwrap_or_else(|| "noop".to_string());
    let dir = args.next().unwrap_or_else(|| {
        format!("{}/../../demos/store/local", env!("CARGO_MANIFEST_DIR"))
    });
    let edited = mode.strip_prefix("edit:").map(str::to_string);

    let mut db = datafun::Database::default();
    let sys = datalove_sys_packages::system_library();
    let local = load_local_library(dir.as_ref()).expect("the store's local library loads");
    let mut descriptor = WorkspaceDescriptor::from_system_library(&sys)
        .with_user_library(local)
        .expect("the store's library fits the workspace");
    descriptor.options.cache_consts = !no_cache;
    let mut pipeline = descriptor.to_pipeline(&db);
    pipeline.set_natives(std::sync::Arc::new(RiderNatives::for_workspace(&descriptor, &sys.natives)));

    let original = edited.as_ref().map(|module| {
        let path = format!("{}/store/{}.dfm", dir, module);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {}", path, e))
    });
    let probe = |salt: usize| format!(
        "{}\nfun probe_edit(): u32\n  ret : u32 / {}\nend fun\n",
        original.as_ref().expect("only an edit probes"), salt,
    );

    let start = Instant::now();
    let compiled = pipeline.compile_fresh(&db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);
    println!("first compile: {:?}", start.elapsed());

    let mut times: Vec<Duration> = Vec::with_capacity(iterations);
    for salt in 1..=iterations {
        if let Some(module) = &edited {
            pipeline.update_source(&mut db, "local", "store", module, &probe(salt));
        }
        let start = Instant::now();
        let (compiled, _) = pipeline.compile(&mut db);
        times.push(start.elapsed());
        assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    }

    times.sort();
    let total: Duration = times.iter().sum();
    println!(
        "{} {} recompiles: median {:?}, mean {:?}, min {:?}, max {:?}",
        iterations,
        mode,
        times[times.len() / 2],
        total / iterations as u32,
        times[0],
        times[times.len() - 1],
    );
}
