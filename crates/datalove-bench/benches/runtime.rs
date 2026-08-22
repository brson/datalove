//! Benchmarks for interpreter and runtime hot paths.
//!
//! Each workload is chosen to load one path heavily:
//!
//! - `calls` - function call overhead, which pays for a frame layout and a
//!   zeroed frame allocation per call.
//! - `bigint` - allocator traffic, since every bigint operation allocates and
//!   frees a limb buffer.
//! - `map_insert` - key comparison, which reaches the runtime's `cmp` entry
//!   point once per B-tree probe.
//!
//! Each timing includes roughly 1.3ms of compiling the fragment, so treat the
//! numbers as relative rather than as pure execution cost.

use datalove_datafun as datafun;
use datafun::pipeline::ModuleCompilationPipeline;

fn main() {
    divan::main();
}

/// Call a small function in a loop.
const CALLS_SOURCE: &str = r#"
fun small(x: u32): u32
  ret x
end fun

fun many_calls(n: u32): !u32
  var i: u32 = 0
  var acc: u32 = 0
  loop while i .< n
    set acc = acc +! small(i)
    set i = i +! 1
  end loop
  ret ok acc
end fun

debuglog many_calls(20000)
"#;

/// Call a function with many locals, where the frame layout is larger and so
/// is the cost of computing it.
const CALLS_WIDE_SOURCE: &str = r#"
fun wide(x: u32): u32
  let v0: u32 = x
  let v1: u32 = x
  let v2: u32 = x
  let v3: u32 = x
  let v4: u32 = x
  let v5: u32 = x
  let v6: u32 = x
  let v7: u32 = x
  let v8: u32 = x
  let v9: u32 = x
  let v10: u32 = x
  let v11: u32 = x
  let v12: u32 = x
  let v13: u32 = x
  let v14: u32 = x
  let v15: u32 = x
  let v16: u32 = x
  let v17: u32 = x
  let v18: u32 = x
  let v19: u32 = x
  let v20: u32 = x
  let v21: u32 = x
  let v22: u32 = x
  let v23: u32 = x
  ret v23
end fun

fun many_wide(n: u32): !u32
  var i: u32 = 0
  var acc: u32 = 0
  loop while i .< n
    set acc = acc +! wide(i)
    set i = i +! 1
  end loop
  ret ok acc
end fun

debuglog many_wide(20000)
"#;

/// Accumulate into a bigint, allocating and freeing a limb buffer each step.
const BIGINT_SOURCE: &str = r#"
fun sum_int(n: u32): !int
  var acc: int = 0
  var i: u32 = 0
  loop while i .< n
    set acc = acc + 1
    set i = i +! 1
  end loop
  ret ok acc
end fun

debuglog sum_int(20000)
"#;

/// Insert into a map with a deeply structured key.
///
/// Key comparison walks both type descriptors structurally, so the cost of
/// that walk scales with the key type's depth rather than the value's.
const MAP_DEEP_SOURCE: &str = r#"
type K: ((u32, u32), (u32, u32))

fun build_deep(n: u32): !u32
  var m: %{K = u32} = %{}
  var i: u32 = 0
  loop while i .< n
    set m[((i, i), (i, i))] = i
    set i = i +! 1
  end loop
  ret ok i
end fun

debuglog build_deep(4000)
"#;

/// Insert into a map, comparing keys on every B-tree probe.
const MAP_SOURCE: &str = r#"
fun build_map(n: u32): !u32
  var m: %{u32 = u32} = %{}
  var i: u32 = 0
  loop while i .< n
    set m[i] = i
    set i = i +! 1
  end loop
  ret ok i
end fun

debuglog build_map(20000)
"#;

/// Compile and execute a script fragment with the interpreter.
fn run(source: &str) {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::default();
    let compiled = pipeline.compile_fresh(&db);

    let mut compiler = compiled.script_compiler_default(&db).unwrap();
    let mut executor = compiled
        .script_executor(datafun::DebugOutputMode::Disabled, None)
        .unwrap();

    let unit = compiler.compile_fragment(source);
    let ir_unit = unit.ir_unit.as_ref().expect("benchmark source must compile");
    executor.execute_fragment(ir_unit);
    executor.destroy_live_values();
}

#[divan::bench]
fn calls(bencher: divan::Bencher) {
    bencher.bench_local(|| run(CALLS_SOURCE));
}

#[divan::bench]
fn calls_wide(bencher: divan::Bencher) {
    bencher.bench_local(|| run(CALLS_WIDE_SOURCE));
}

#[divan::bench]
fn bigint(bencher: divan::Bencher) {
    bencher.bench_local(|| run(BIGINT_SOURCE));
}

#[divan::bench]
fn map_insert(bencher: divan::Bencher) {
    bencher.bench_local(|| run(MAP_SOURCE));
}

#[divan::bench]
fn map_insert_deep_key(bencher: divan::Bencher) {
    bencher.bench_local(|| run(MAP_DEEP_SOURCE));
}
