//! Compile the system library and run a script on the bytecode, in wasm,
//! under wasmi, with no threads.
//!
//! The module is this crate built for `wasm32-unknown-unknown`. Each mode's
//! report has to match what the same code reports natively, and the parallel
//! one has to have run on a rayon pool of one thread, the caller's.
//!
//! Ignored by default because building the compiler for wasm takes minutes
//! from cold; `just test-wasm` runs it.

use std::path::PathBuf;
use std::process::Command;

use datalove_wasm_smoke::{ParallelMode, run};
use wasmi::{Engine, ExternType, Instance, Linker, Module, Store};

/// Build this crate for wasm and return the module's path.
///
/// Into a target directory of its own, since the one `cargo test` is using is
/// locked for as long as it runs.
fn build_module() -> PathBuf {
    let target_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("wasm-smoke");
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-unknown-unknown", "-p", "datalove-wasm-smoke"])
        .arg("--target-dir").arg(&target_dir)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "building the wasm module failed");
    target_dir.join("wasm32-unknown-unknown/release/datalove_wasm_smoke.wasm")
}

/// The module, instantiated with no host functions it can usefully call.
///
/// What it imports is wasm-bindgen's, there because rustmax's portable profile
/// turns on `getrandom/js` and chrono's `wasmbind` for every wasm target, and
/// `datalove-datalit` asks for `getrandom/js` itself. Nothing here processes
/// the module with wasm-bindgen, so those imports are bound to functions that
/// trap: the compile and the run have to get by without them. Anything else
/// imported is a host the module needs, and fails the test.
fn instantiate(path: &PathBuf) -> (Store<()>, Instance) {
    let engine = Engine::default();
    let bytes = std::fs::read(path).expect("the module was built");
    let module = Module::new(&engine, &bytes[..]).expect("the module validates");
    let mut linker = Linker::<()>::new(&engine);
    for import in module.imports() {
        let (module_name, name) = (import.module().to_string(), import.name().to_string());
        assert!(module_name.starts_with("__wbindgen"), "the module imports {module_name}::{name}");
        let ExternType::Func(ty) = import.ty() else {
            panic!("the module imports {module_name}::{name}, which is not a function");
        };
        let message = format!("the module called {module_name}::{name}, which is JavaScript's");
        linker.func_new(&module_name, &name, ty.clone(), move |_, _, _| Err(wasmi::Error::new(message.clone())))
            .expect("each import is bound once");
    }
    let mut store = Store::new(&engine, ());
    let instance = linker.instantiate_and_start(&mut store, &module).expect("the module instantiates");
    (store, instance)
}

/// What the last run left in the module's output buffer.
fn output(store: &mut Store<()>, instance: &Instance) -> String {
    let ptr = instance.get_typed_func::<(), i32>(&*store, "smoke_output_ptr").unwrap()
        .call(&mut *store, ()).unwrap() as usize;
    let len = instance.get_typed_func::<(), i32>(&*store, "smoke_output_len").unwrap()
        .call(&mut *store, ()).unwrap() as usize;
    let memory = instance.get_memory(&*store, "memory").expect("the module exports its memory");
    String::from_utf8(memory.data(&*store)[ptr..ptr + len].to_vec()).expect("the output is text")
}

/// Run the script in the module in one mode, on a fresh instance.
fn run_in_wasm(path: &PathBuf, mode: ParallelMode) -> String {
    let (mut store, instance) = instantiate(path);
    let parallel = match mode {
        ParallelMode::Sequential => 0,
        ParallelMode::Parallel => 1,
    };
    let result = instance.get_typed_func::<i32, i32>(&store, "smoke_run").unwrap()
        .call(&mut store, parallel);
    let report = output(&mut store, &instance);
    match result {
        Ok(0) => {}
        Ok(_) => panic!("{mode:?} run failed in wasm: {report}"),
        Err(trap) => panic!("{mode:?} run trapped in wasm: {trap}; {report}"),
    }
    if mode == ParallelMode::Parallel {
        let threads = instance.get_typed_func::<(), i32>(&store, "smoke_rayon_threads").unwrap()
            .call(&mut store, ()).unwrap();
        assert_eq!(threads, 1, "rayon's pool in wasm has more than the calling thread");
    }
    report
}

#[test]
#[ignore = "builds the compiler for wasm, which takes minutes; run by `just test-wasm`"]
fn bytecode_pipeline_runs_in_wasm_on_one_thread() {
    let path = build_module();
    for mode in [ParallelMode::Sequential, ParallelMode::Parallel] {
        let native = run(mode).expect("the script runs natively");
        assert_eq!(native, "6765\n[1, 2]\n255\n", "the native {mode:?} report");
        assert_eq!(run_in_wasm(&path, mode), native, "the wasm {mode:?} report");
    }
}
