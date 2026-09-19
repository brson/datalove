//! Interpreter, inliner and jit measured against each other on one workload.
//!
//! Four configurations per workload, so that the jit's contribution and the
//! inliner's can be read apart:
//!
//! - `interp` -- no dispatcher at all, which is what every entry point but
//!   `datalove script --jit` uses today.
//! - `inline` -- `OptimizingDispatcher` with the jit off. Measures whether
//!   inlining pays for itself under the interpreter alone.
//! - `jit` -- `OptimizingDispatcher` with inlining off.
//! - `jit_inline` -- both on.
//!
//! Only execution is timed. Building the database, compiling the fragment and
//! constructing the dispatcher all happen in `with_inputs`, which divan does
//! not count -- constructing a `JitEngine` alone reserves a 64MB arena and
//! costs more than some of these workloads.
//!
//! `floor` is an empty workload in each configuration. Subtract it to get the
//! cost of the work rather than of the harness.
//!
//! A workload that fails to compile panics rather than being skipped. The
//! previous version of this file ran its source through `if let Some(ir_unit)`,
//! the source had a type error, and so both of its benchmarks measured an
//! empty execution for as long as the file existed.

use divan::Bencher;

use datalove_datafun as datafun;
use datafun::pipeline::{WorkspaceDescriptor, rider_load};
use datalove_datafun_cranelift_jit::{DispatcherConfig, DispatcherMode, OptimizingDispatcher};
use datalove_datafun_interp::CallDispatcher;
use datalove_datafun_ir::IrCodeUnit;

fn main() {
    divan::main();
}

/// Which optimizations a run has available.
#[derive(Clone, Copy)]
enum Config {
    Interp,
    Inline,
    Jit,
    JitInline,
}

impl Config {
    /// The dispatcher this configuration runs with, if any.
    ///
    /// Thresholds are 1 rather than production's 100 and 50. At production
    /// thresholds most of these workloads never reach either optimization, so
    /// the four configurations would measure the same thing; what the
    /// thresholds cost is a separate question from what the optimizations are
    /// worth, and mixing them measures neither.
    fn dispatcher(self) -> Option<Box<dyn CallDispatcher>> {
        let (jit_enabled, inlining_enabled) = match self {
            Config::Interp => return None,
            Config::Inline => (false, true),
            Config::Jit => (true, false),
            Config::JitInline => (true, true),
        };
        let config = DispatcherConfig {
            jit_enabled,
            inlining_enabled,
            mode: DispatcherMode::Tuned { jit_threshold: 1, inline_threshold: 1 },
            // Off: the collector times the dispatcher's own bookkeeping rather
            // than the call, and its per-call `Instant::now` would land inside
            // what is being measured.
            metrics_enabled: false,
            metrics_config: Default::default(),
        };
        let dispatcher = OptimizingDispatcher::with_config(config)
            .expect("dispatcher construction failed");
        Some(Box::new(dispatcher))
    }
}

/// Everything a timed run needs, built outside the timer.
struct Prepared {
    executor: datafun::pipeline::ScriptExecutor,
    unit: IrCodeUnit,
}

/// The database and compiled system library, built once per benchmark thread.
///
/// Leaked rather than owned because a `CompiledModules` borrows both the
/// database and the pipeline it came from, and every iteration wants one. Built
/// once because compiling `sys/std` per iteration was both the slowest part of
/// the setup and enough leaked salsa state to exhaust memory. Thread-local
/// because a salsa `Database` is not `Sync`.
type World = (
    &'static datafun::Database,
    &'static datafun::pipeline::CompiledModules<'static>,
    &'static datafun::pipeline::SystemLibrary,
);

thread_local! {
    static WORLD: std::cell::OnceCell<World> = const { std::cell::OnceCell::new() };
}

fn with_world<R>(f: impl FnOnce(World) -> R) -> R {
    WORLD.with(|cell| {
        let world = *cell.get_or_init(|| {
            let db: &'static datafun::Database =
                Box::leak(Box::new(datafun::Database::default()));
            let sys: &'static datafun::pipeline::SystemLibrary =
                Box::leak(Box::new(datalove_stdlib::system_library()));
            let descriptor = WorkspaceDescriptor::from_system_library(sys);
            let pipeline: &'static mut datafun::pipeline::ModuleCompilationPipeline =
                Box::leak(Box::new(descriptor.to_pipeline(db)));
            let compiled = pipeline.compile_fresh(db);
            assert!(!compiled.has_errors(), "the system library must compile");
            (db, &*Box::leak(Box::new(compiled)), sys)
        });
        f(world)
    })
}

/// Compile `source` and build an executor for it in `config`.
fn prepare(source: &str, config: Config) -> Prepared {
    with_world(|(db, compiled, sys)| {
    let mut compiler = compiled
        .script_compiler_default(db)
        .expect("module compilation failed");
    let mut executor = compiled
        .script_executor(datafun::DebugOutputMode::Disabled, config.dispatcher())
        .expect("executor construction failed");

    // A native the jit reaches without having been told its address aborts the
    // process, so both tables are filled whether the jit is on or not. The cli
    // does this only for a bare `JitEngine`; an `OptimizingDispatcher` holds
    // its engine behind `jit()` and nothing in the tree hands it the symbols.
    let native_fn_ptrs = rider_load::register_linked_natives(
        &compiled.native_symbols(),
        &sys.natives,
        executor.native_table_mut(),
    )
    .expect("linked natives must resolve");
    if let Some(dispatcher) = executor.take_dispatcher() {
        if let Some(opt) = dispatcher.as_any().downcast_ref::<OptimizingDispatcher>() {
            for (symbol, ptr) in &native_fn_ptrs {
                opt.jit().register_native_symbol(symbol, *ptr);
            }
        }
        executor.set_dispatcher(dispatcher);
    }

    let unit = compiler.compile_fragment(source);
    let unit = unit
        .ir_unit
        .clone()
        .expect("benchmark source must compile");

    Prepared { executor, unit }
    })
}

/// Run one prepared workload. This is the only part that is timed.
fn execute(mut prepared: Prepared) {
    prepared.executor.execute_fragment(&prepared.unit);
    prepared.executor.destroy_live_values();
}

/// Declare the four configurations of one workload.
macro_rules! workload {
    ($name:ident, $source:expr) => {
        mod $name {
            use super::*;

            const SOURCE: &str = $source;

            fn run(bencher: Bencher, config: Config) {
                bencher
                    .with_inputs(|| prepare(SOURCE, config))
                    .bench_local_values(execute);
            }

            #[divan::bench]
            fn interp(bencher: Bencher) { run(bencher, Config::Interp) }
            #[divan::bench]
            fn inline(bencher: Bencher) { run(bencher, Config::Inline) }
            #[divan::bench]
            fn jit(bencher: Bencher) { run(bencher, Config::Jit) }
            #[divan::bench]
            fn jit_inline(bencher: Bencher) { run(bencher, Config::JitInline) }
        }
    };
}

// What the harness itself costs, with nothing to run.
workload!(floor, "debuglog 1");

// A tight arithmetic loop inside one call. Nothing to inline, and the jit's
// best case: no call crosses the boundary while the loop runs.
workload!(
    loop_arith,
    r#"
fun run_one(n: u32): !u32
  var i: u32 = 0
  var accum: u32 = 0
  loop while i .< n
    set accum = accum +! 1
    set i = i +! 1
  end loop
  ret ok accum
end fun

fun run_many(repeat: u32, n: u32): !u32
  var i: u32 = 0
  var accum: u32 = 0
  loop while i .< repeat
    set accum = accum +! run_one(n)!
    set i = i +! 1
  end loop
  ret ok accum
end fun

debuglog run_many(20, 100000)
"#
);

// A small callee in a hot loop: what inlining is for, and what the jit is
// worst at, since every call in compiled code leaves through the trampoline.
workload!(
    small_callee,
    r#"
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

debuglog many_calls(200000)
"#
);

// Recursion, where inlining a call site cannot remove the call and the jit
// pays the trampoline on every level.
workload!(
    recursive,
    r#"
fun fib(n: int): int
    if n .< 2
        ret n
    else
        ret fib(n - 1) + fib(n - 2)
    end if
end fun

debuglog fib(24)
"#
);

// A chain of small calls, so that inlining has somewhere to go transitively.
workload!(
    call_chain,
    r#"
fun leaf(x: u32): u32
  ret x
end fun

fun mid(x: u32): u32
  ret leaf(x)
end fun

fun top(x: u32): u32
  ret mid(x)
end fun

fun drive(n: u32): !u32
  var i: u32 = 0
  var acc: u32 = 0
  loop while i .< n
    set acc = acc +! top(i)
    set i = i +! 1
  end loop
  ret ok acc
end fun

debuglog drive(100000)
"#
);

// Calls into `sys/std`, which is where a real program spends its time. A
// generic that builds a collection declares a shape, and both the jit and the
// inliner refuse such a callee, so this measures how much of a stdlib-shaped
// workload either can reach at all.
workload!(
    stdlib_list,
    r#"
require module sys/std/list

import list.push
import list.len

fun build(n: int): [int]
    var out: [int] = []
    var i: int = 0
    loop while i .< n
      push(mut out, 1)
      set i = i + 1
    end loop
    ret out
end fun

fun drive(reps: int, n: int): int
    var acc: int = 0
    var k: int = 0
    loop while k .< reps
      let xs = build(n@)
      set acc = acc + 1
      set k = k + 1
    end loop
    ret acc
end fun

debuglog drive(200, 200)
"#
);
