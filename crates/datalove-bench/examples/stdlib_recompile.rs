//! The same as `recompile_profile`, over the real system library.
//!
//! The synthetic world has no generics and no consts, so it exercises the
//! paths that early out. This one is what the compiler actually compiles.

use datalove_datafun as datafun;
use datafun::pipeline::WorkspaceDescriptor;

fn main() {
    let mut args = std::env::args().skip(1);
    let iterations: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(200);
    let edit = args.next().map(|a| a == "edit").unwrap_or(false);

    let mut db = datafun::Database::default();
    let sys = datalove_stdlib::system_library();
    let descriptor = WorkspaceDescriptor::from_system_library(&sys);
    let mut pipeline = descriptor.to_pipeline(&db);

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
    println!("editing sys/{}/{}", package, module);
    let probe = |salt: usize| format!("{}\nfun probe_edit(): int\n  ret {}\nend fun\n", text, salt);

    pipeline.update_source(&mut db, "sys", &package, &module, &probe(0));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    drop(compiled);

    let start = std::time::Instant::now();
    for salt in 1..=iterations {
        if edit {
            pipeline.update_source(&mut db, "sys", &package, &module, &probe(salt));
        }
        let (compiled, _) = pipeline.compile(&mut db);
        assert!(!compiled.has_errors(), "{:?}", compiled.all_errors());
    }
    let elapsed = start.elapsed();

    println!(
        "{} {} recompiles in {:?} ({:?} each)",
        iterations,
        if edit { "edit" } else { "no-op" },
        elapsed,
        elapsed / iterations as u32,
    );
}
