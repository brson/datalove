//! Benchmarks comparing interpreter execution with and without JIT.

use datalove_datafun as datafun;
use datalove_datafun_jit::JitEngine;
use datalove_datafun_interp::CallDispatcher;
use datafun::pipeline::ModuleCompilationPipeline;

fn main() {
    divan::main();
}

/// Benchmark script source (from jitbench.dfs).
const JITBENCH_SOURCE: &str = r#"
fun run_one(n: u32): !u32
  var i: u32 = 0
  var accum: u32 = 0
  loop
    if i == n
      ret ok accum
    end if
    set accum = accum +! 1
    set i = i +! 1
  end loop
end fun

fun run_many(repeat: u32, n: u32): !u32
  var i: u32 = 0
  var accum: u32 = 0
  loop
    if i == repeat
      ret ok accum
    end if
    set accum = accum +! run_one(n)!
    set i = i +! 1
  end loop
end fun

let repeat = 10
let n = 100000
debuglog run_many(repeat, n)
"#;

/// Run the benchmark with optional call dispatcher.
fn run_benchmark(call_dispatcher: Option<Box<dyn CallDispatcher>>) {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);

    let mut compiler = compiled.script_compiler(&db).unwrap();
    let mut executor = compiled.script_executor(
        datafun::DebugOutputMode::Disabled,
        call_dispatcher,
    ).unwrap();

    let compiled_unit = compiler.compile_fragment(JITBENCH_SOURCE, false);
    if let Some(ir_unit) = &compiled_unit.ir_unit {
        executor.execute_fragment(ir_unit);
    }
    executor.destroy_all();
}

/// Run the benchmark without JIT (interpreter only).
fn run_without_jit() {
    run_benchmark(None);
}

/// Run the benchmark with JIT enabled.
///
/// Must be called from a spawned thread due to Cranelift JIT limitations
/// with PIE binaries.
fn run_with_jit() {
    let jit = JitEngine::new(1).expect("JitEngine creation failed");
    run_benchmark(Some(Box::new(jit)));
}

#[divan::bench]
fn interp_only(bencher: divan::Bencher) {
    bencher.bench_local(|| {
        run_without_jit();
    });
}

#[divan::bench]
fn interp_with_jit(bencher: divan::Bencher) {
    // Run in spawned thread to work around Cranelift JIT PIE limitations.
    bencher.bench_local(|| {
        std::thread::spawn(run_with_jit).join().expect("JIT thread panicked");
    });
}
