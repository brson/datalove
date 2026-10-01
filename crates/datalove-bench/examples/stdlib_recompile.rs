//! The same as `recompile_profile`, over the real system library.
//!
//! The synthetic world has no generics and no consts, so it exercises the
//! paths that early out. This one is what the compiler actually compiles.

use datalove_datafun as datafun;
use datafun::pipeline::WorkspaceDescriptor;

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    let mode = args.next().unwrap_or_else(|| "noop".to_string());
    let edit = mode == "edit" || mode == "local";
    // `local` edits a module of one's own that calls into the system library,
    // which is what an editing session does. `edit` edits the system library
    // itself, which is the worst case and not the common one.
    let local = mode == "local";

    let mut db = datafun::Database::default();
    let sys = datalove_sys_packages::system_library();
    let descriptor = WorkspaceDescriptor::from_system_library(&sys);
    let mut pipeline = descriptor.to_pipeline(&db);
    let local_source = |salt: usize| format!(
        "fun local_a(x: int): int\n  ret x + {}\nend fun\n\
         fun local_b(x: int): int\n  ret local_a(x) * 2\nend fun\n",
        salt,
    );
    pipeline.add_module(&db, "local", "app", "main", &local_source(0));

    let compiled = pipeline.compile_fresh(&db);
    assert!(!compiled.has_errors(), "the system library must compile");
    drop(compiled);

    // A module to edit: the first one, whatever it is, with a probe function
    // appended whose body changes every iteration.
    //
    // A comment would not do. It moves spans and nothing else, so lowering
    // backdates and the edit never reaches phase 5 at all -- which reads as a
    // very fast edit and measures nothing. Adding the probe once and then
    // changing its body keeps the function ids where they are, so this is one
    // function's worth of real change, which is what an edit usually is.
    let (package, module, text) = sys.library.packages.iter()
        .flat_map(|(p, pkg)| pkg.modules.iter().map(move |(m, d)| (p.clone(), m.clone(), d.source.to_string())))
        .next()
        .expect("the system library has modules");
    if local {
        println!("editing local/app/main");
    } else {
        println!("editing sys/{}/{}", package, module);
    }
    let probe = |salt: usize| format!("{}\nfun probe_edit(): int\n  ret {}\nend fun\n", text, salt);

    pipeline.update_source(&mut db, "sys", &package, &module, &probe(0));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);

    let start = std::time::Instant::now();
    for salt in 1..=iterations {
        if local {
            pipeline.update_source(&mut db, "local", "app", "main", &local_source(salt));
        } else if edit {
            pipeline.update_source(&mut db, "sys", &package, &module, &probe(salt));
        }
        let (compiled, _) = pipeline.compile(&mut db);
        assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    }
    let elapsed = start.elapsed();

    println!(
        "{} {} recompiles in {:?} ({:?} each), peak RSS {} MB",
        iterations,
        &mode,
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
