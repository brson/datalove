# Tuning the JIT

A plan for making the JIT's decisions -- what to compile and when -- good on a
program shaped like an application, using the store demo (`demos/store`) as
the workload, and for adding on-stack replacement (OSR), which the demo shows
the JIT needs.

> **This is a plan.** Step 0 is done (October 2026); the rest is not started.
> The numbers are from a release build on 4 CPUs at 20,000 orders.

## Contents

- [Why the store demo](#user-content-why-the-store-demo)
- [Where things stand](#user-content-where-things-stand)
- [The steps](#user-content-the-steps)
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

Compiling the program, which reads the data and evaluates the id maps under
CTFE, is about 330 ms of every `script` run (`--jit-stats` prints it as
`compiling`), so the threshold-1 JIT runs the report in about 300 ms, of which
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

**1. A baseline grid.** Data at 2,000, 20,000 and 200,000 orders (`just gen
N`), since the right threshold moves with the size of the run: a short run
rewards compiling little and a long one compiling a lot. Engines: interpreter,
thresholds 1, 2, 10, 100 and 1000, and AOT as the ceiling. `hyperfine` with
the variants alternating, and the compile phase taken out using the time
`--jit-stats` reports. Kept as a script beside the demo so the grid can be
rerun after each later step.

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
