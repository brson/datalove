//! One cold compile, then many recompiles, for profiling the recompile path.
//!
//! Profiling the benchmark binary points at the cold compile instead, because
//! its setup does one and that swamps everything after it. Run this under
//! `perf record` and the profile is the recompile.
//!
//! Scratch tool, not a test. `cargo run --release -p datalove-bench --example
//! recompile_profile -- [iterations] [edit|noop] [modules]`.
//!
//! The module count is a knob because the interesting question about an edit is
//! not what it costs but how that cost grows: the passes that run over the whole
//! program on every edit make it linear in the size of the world, and the size
//! of the world in the fixtures is not the size of anybody's project.

use datalove_datafun::pipeline::ModuleCompilationPipeline;
use datalove_datafun_compiler::Database;

const FUNCTIONS_PER_MODULE: usize = 30;

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

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(500);
    let edit = args.next().map(|a| a == "edit").unwrap_or(false);
    let num_modules: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(32);

    let mut db = Database::default();
    let mut pipeline = ModuleCompilationPipeline::default();
    for i in 0..num_modules {
        pipeline.add_module(&db, "local", "pkg", &format!("m{}", i), &module_source(i, 0));
    }

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());

    let start = std::time::Instant::now();
    for salt in 1..=iterations {
        if edit {
            pipeline.update_source(&mut db, "local", "pkg", "m0", &module_source(0, salt));
        }
        let (compiled, _) = pipeline.compile(&mut db);
        assert!(compiled.is_successful());
    }
    let elapsed = start.elapsed();

    println!(
        "{} modules, {} {} recompiles in {:?} ({:?} each), peak RSS {} MB",
        num_modules,
        iterations,
        if edit { "edit" } else { "no-op" },
        elapsed,
        elapsed / iterations as u32,
        peak_rss_mb(),
    );
}

/// Peak resident set, in megabytes.
///
/// Memoizing trades memory for time -- the module registry and the function id
/// lookup are both held in memos now rather than rebuilt -- so a reading that
/// does not say what it cost in memory is only half of the answer.
fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines()
                .find(|line| line.starts_with("VmHWM:"))?
                .split_whitespace()
                .nth(1)?
                .parse::<u64>()
                .ok()
        })
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}
