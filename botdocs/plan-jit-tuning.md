# Tuning the JIT

A plan for making the JIT's decisions -- what to compile and when -- good on a
program shaped like an application, using the store demo (`demos/store`) as
the workload, and for adding on-stack replacement (OSR), which the demo shows
the JIT needs.

> **This is a plan.** Steps 0 and 1 are done (October 2026); the rest is not
> started. The numbers are from a release build on 4 CPUs.

## Contents

- [Why the store demo](#user-content-why-the-store-demo)
- [Where things stand](#user-content-where-things-stand)
- [The steps](#user-content-the-steps)
- [The baseline grid](#user-content-the-baseline-grid)
- [OSR](#user-content-osr)

## Why the store demo

The JIT tiers by call count: a function is compiled on the call that reaches
`jit_threshold`. Every query in `sales.dfm`, `people.dfm` and `basket.dfm` is
called once and loops over all the orders, most with a loop over each order's
lines inside, so under any threshold above 1 the loops that hold the run's time
are never compiled. Their callees -- `db.line_product`, `db.is_sale`, `sales.add`
-- are called millions of times and are. That is the ordinary shape of a
program, and the benchmarks in `benchvs` and `datalove-bench`, each a hot
function or a hot loop, do not have it.

The loops worth entering partway through:

- `basket.top_pairs`: three nested loops, and the heaviest query.
- `sales.by_category`, `sales.top_products`: two levels, a map insert per line.
- `people.loyalty`: a scan of a sorted list that calls nothing in `db`.

Not worth it: the script's own loop in `report.dfs`, about a hundred
iterations, and `db.build_*_slots`, which run under CTFE at compile time.

## Where things stand

| Engine | Wall time | Compiled | Codegen |
|---|---|---|---|
| Interpreter (bytecode) | 925 ms | - | - |
| `--jit` (threshold 1) | 619-634 ms | 86 | 66-71 ms |
| `--jit-threshold 2` | 788 ms | 66 | 20 ms |
| `--jit-threshold 100` | 993 ms | 44 | 12 ms |
| AOT executable | 239 ms | - | - |

At 20,000 orders. Compiling the program, which reads the data and evaluates
the id maps under CTFE, is about 300 ms of every `script` run (`--time`
prints it), so the threshold-1 JIT runs the report in about 300 ms, of which
about 70 ms is codegen and some of the rest is `JitEngine::new`. That is within
reach of AOT, at the price of compiling every function called even once.

**A threshold of 100 is slower than not having the JIT.** `--jit-stats` says
why: 3.4 million calls cross from the interpreter into compiled code, each
through `JitEngine::dispatch_with`, which hashes the `FuncIdentity`, resolves the
callee's context, sets the thread's dispatch context and calls through
`bridge::call_jit`. On top of that, the bytecode gives up its `Plan` fast path
for every call whenever a dispatcher is installed (`bytecode.rs`, the epoch and
dispatcher check in the plan lookup), so installing the JIT taxes calls it
never compiles.

**Codegen of the first function costs about 4 ms** whatever it is
(`db.order_count`, 72 bytes, is the most expensive function to compile in the
threshold-1 run), which looks like one-time setup in the compiler being
charged to it.

## The steps

**0. Instrumentation (done).** `script --jit` runs under an
`OptimizingDispatcher` rather than a bare `JitEngine`, so the dispatcher being
tuned is the one that runs. `--jit-threshold N` sets the threshold, defaulting
to 1, which is what `--jit` always did. `--jit-stats` prints the time spent
compiling and running, then `JitStats`: totals, the functions with the most
calls through the dispatcher -- run by the interpreter, entered from the
interpreter, and calling back into it from compiled code -- and the functions
that took longest to compile, by module path. `just jit-stats N` in the demo
runs it. Counting calls is a second table probe per call, so it is off unless
asked for. This replaced `OptimizingDispatcher`'s `MetricsCollector`, which
nothing read, saw only calls from the interpreter, and timed an interpreted
call before the interpreter ran it.

Back-edge counts belong here too but wait for step 3, which adds the counters.

**1. A baseline grid (done).** `demos/store/grid.py` (`just grid`) times the
interpreter, thresholds 1, 2, 10, 100 and 1000, and AOT as the ceiling, at
2,000, 20,000 and 200,000 orders, since the right threshold moves with the
size of the run. The engines alternate round by round and the medians are
reported; `script --time` splits each run into compiling and running, and
one untimed `--jit-stats` run per threshold gives what was compiled and the
crossings. It writes `target/store-grid/grid.json`, and is what to rerun after
each later step. The results are [below](#user-content-the-baseline-grid).

**2. Make every threshold at least as fast as the interpreter.** Profile
`--jit-threshold 100` with `perf`. The candidates are the two above: keep the
bytecode's `Plan` path for a callee the JIT has not compiled, and make a
crossing into compiled code cheaper than a hash, a context resolution and a
thread-local write. Done when no threshold loses to the interpreter.

**3. OSR,** below. Checked by `just check` in the demo, which diffs the report
against `check.py`, and by chaos mode taking OSR at random back-edges in
`engine_tests`. Done when a threshold of 100 with OSR runs the report close to
the threshold-1 time while compiling far fewer than 86 functions.

**4. Tune.** Sweep the call threshold, the back-edge threshold -- or one
counter of calls plus back-edges over some k, as HotSpot does -- and
Cranelift's `opt_level` (`none` against `speed`, since codegen is now a
measurable share of the run). Score total wall time across all three data
sizes and pick what does well across them, not what wins at 20,000.

**5. Check against other programs.** `benchvs`, the `jit` bench in
`datalove-bench` and `botdocs/learn.dfs`, so the result is not fitted to one
program, then record it in the compiler guide's performance notes.

## The baseline grid

From `just grid` at commit `44101c96` plus `--time`. Times in ms, medians of 7
rounds (3 at 200,000). Wall is the whole process; compile and run are what
`--time` reports; the last three columns are from one `--jit-stats` run.

**2,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 159 | 68 | 78 | 1.00x | - | - | - |
| jit t=1 | 188 | 78 | 96 | 0.85x | 86 | 72 | 122 |
| jit t=2 | 171 | 77 | 78 | 0.93x | 66 | 23 | 266,020 |
| jit t=10 | 199 | 78 | 106 | 0.80x | 57 | 17 | 402,770 |
| jit t=100 | 193 | 79 | 100 | 0.83x | 44 | 13 | 403,227 |
| jit t=1000 | 196 | 77 | 104 | 0.81x | 37 | 11 | 389,285 |
| aot | 37 | - | - | 4.28x | - | - | - |

**20,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 899 | 270 | 596 | 1.00x | - | - | - |
| jit t=1 | 630 | 290 | 299 | 1.43x | 86 | 70 | 123 |
| jit t=2 | 810 | 309 | 472 | 1.11x | 66 | 23 | 2,363,019 |
| jit t=10 | 1040 | 299 | 701 | 0.86x | 58 | 17 | 3,383,535 |
| jit t=100 | 992 | 292 | 660 | 0.91x | 44 | 12 | 3,384,049 |
| jit t=1000 | 978 | 288 | 649 | 0.92x | 37 | 10 | 3,370,008 |
| aot | 240 | - | - | 3.75x | - | - | - |

**200,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 7115 | 2307 | 4555 | 1.00x | - | - | - |
| jit t=1 | 4746 | 2615 | 1811 | 1.50x | 86 | 97 | 123 |
| jit t=2 | 6412 | 2560 | 3545 | 1.11x | 66 | 21 | 22,578,685 |
| jit t=10 | 8141 | 2623 | 5018 | 0.87x | 58 | 17 | 29,446,718 |
| jit t=100 | 7891 | 2584 | 4984 | 0.90x | 44 | 12 | 29,447,225 |
| jit t=1000 | 7855 | 2621 | 4903 | 0.91x | 37 | 10 | 29,433,391 |
| aot | crashed | | | | | | |

What it says:

- **No threshold above 2 beats the interpreter at any size,** and they lose
  by about the same at every size, 8-20%. The crossings grow with the data,
  about 150 per order, so the loss is per call rather than a fixed cost. Step
  2 is the first thing to do; nothing tuned on top of today's crossings means
  much.
- **Compiling everything wins once the run is long enough,** and only then:
  2x on the running phase at 20,000 and 2.5x at 200,000, but at 2,000 its 72
  ms of codegen is more than the whole interpreted run of 78 ms. That is the
  trade a threshold exists to make, and today no threshold makes it, because
  the code worth compiling is in loops. Cheaper codegen (`opt_level`) would
  move the break-even down.
- **Threshold 2 sits between** because it catches the queries the report
  calls twice and the functions they call, and leaves the rest crossing.
- **AOT is the ceiling, at 3.75-4.3x,** and threshold 1 is about 60 ms of
  running from it at 20,000 once codegen is taken out.
- **The compile phase is 13% slower under `--jit` at 200,000 orders,** 2,615
  against 2,307 ms, which is not the jit: `--jit` runs the whole command on a
  spawned thread, and glibc gives that thread its own malloc arena, which the
  allocation-bound front end does worse in. `MALLOC_ARENA_MAX=1` takes the
  difference away. The grid's compile column carries it; the run column does
  not.
- **The AOT executable crashes at 200,000 orders,** overflowing its stack in
  `__dtlv_statics_init`, whose frame grows with the data; see
  [issues](issues.md#user-content-the-aot-statics-initializer-takes-a-frame-the-size-of-the-data).

## OSR

Two facts about this codebase make OSR smaller than it usually is.

**No way back.** The JIT does not speculate, so compiled code never needs to
leave for the interpreter. OSR goes one way, interpreter to compiled code, and
the hard half of OSR elsewhere -- materializing an interpreter frame from
compiled state -- does not arise.

**One frame layout.** `IrLayout` lays out the interpreter's frames and
compiled code's frame slot alike: values, slots and tracking bytes at the same
offsets. A loop's carried values are block parameters, which have fixed places
in the frame, so at a loop header the whole of the loop's state is in the
frame bytes.

The sketch:

- **Count back-edges.** In the bytecode, a `Jump` to a lower pc; in the IR
  walker, a `Goto` or `Branch` to a block that dominates it. Keyed by
  `(FuncIdentity, header block)`.
- **Compile an entry at the header.** At the threshold, compile a variant of
  the function taking `(rt, frame, [sret])`, whose entry block loads the
  header's block parameters from `frame` and jumps to the header. It uses the
  interpreter's frame bytes in place rather than its own stack slot, so a
  reference into the frame stays good; that means codegen's
  `stack_addr(frame_slot, offset)` becomes an address off a frame base that is
  either the stack slot or the parameter.
- **Return as the function.** The variant returns into the caller's
  destination, and the interpreter pops the frame as if the call had returned.
  Compile the ordinary entry at the same time, so the next call starts native.

What to test, beyond the demo: a reference live across the header, owned
values with tracking bytes, nested loops where only the inner one is hot, and
a `!` return after entering.
