//! Benchmarks comparing interpreter execution with and without JIT.

use datalove_datafun as datafun;
use datalove_datafun_jit::JitEngine;
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

/// Run the benchmark without JIT (interpreter only).
fn run_without_jit() {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);

    let mut ctx = compiled.script_context(
        &db,
        datafun::DebugOutputMode::Disabled,
        None, // No JIT
    );

    let _result = ctx.eval_fragment(JITBENCH_SOURCE);
    ctx.destroy_all();
}

/// Run the benchmark with JIT enabled.
///
/// Must be called from a spawned thread due to Cranelift JIT limitations
/// with PIE binaries.
fn run_with_jit() {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new();
    let compiled = pipeline.compile_fresh(&db);

    let jit = JitEngine::new(1).expect("JitEngine creation failed");
    let mut ctx = compiled.script_context(
        &db,
        datafun::DebugOutputMode::Disabled,
        Some(Box::new(jit)),
    );

    let _result = ctx.eval_fragment(JITBENCH_SOURCE);
    ctx.destroy_all();
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
