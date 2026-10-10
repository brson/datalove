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
- [Making the crossings cheap](#user-content-making-the-crossings-cheap)
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
| Interpreter (bytecode) | 898 ms | - | - |
| `--jit` (threshold 1) | 608 ms | 86 | 68 ms |
| `--jit-threshold 2` | 758 ms | 66 | 21 ms |
| `--jit-threshold 100` | 982 ms | 44 | 12 ms |
| AOT executable | 235 ms | - | - |

At 20,000 orders; the [grid](#user-content-the-baseline-grid) has the rest. Compiling the program, which reads the data and evaluates
the id maps under CTFE, is about 300 ms of every `script` run (`--time`
prints it), so the threshold-1 JIT runs the report in about 300 ms, of which
about 70 ms is codegen and some of the rest is `JitEngine::new`. That is within
reach of AOT, at the price of compiling every function called even once.

**A threshold of 100 is slower than not having the JIT,** because 3.4 million
calls cross from the interpreter into compiled code and each costs about ten
times what the compiled function then does; see [Making the crossings
cheap](#user-content-making-the-crossings-cheap).

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

**2. Make every threshold at least as fast as the interpreter.** Profiled
(see [Making the crossings cheap](#user-content-making-the-crossings-cheap));
not started. Done when no threshold loses to the interpreter.

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

From `just grid` at commit `53f839f3`, on a quiet machine. Times in ms,
medians of 7 rounds (3 at 200,000). Wall is the whole process; compile and run
are what `--time` reports; the last three columns are from one `--jit-stats`
run.

**2,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 154 | 66 | 75 | 1.00x | - | - | - |
| jit t=1 | 170 | 67 | 91 | 0.90x | 86 | 60 | 122 |
| jit t=2 | 155 | 67 | 74 | 1.00x | 66 | 20 | 266,020 |
| jit t=10 | 180 | 67 | 98 | 0.86x | 57 | 16 | 402,770 |
| jit t=100 | 178 | 69 | 94 | 0.87x | 44 | 11 | 403,227 |
| jit t=1000 | 178 | 68 | 95 | 0.87x | 37 | 9 | 389,285 |
| aot | 35 | - | - | 4.45x | - | - | - |

**20,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 898 | 266 | 595 | 1.00x | - | - | - |
| jit t=1 | 608 | 269 | 297 | 1.48x | 86 | 68 | 123 |
| jit t=2 | 758 | 269 | 449 | 1.19x | 66 | 21 | 2,363,019 |
| jit t=10 | 969 | 268 | 656 | 0.93x | 58 | 17 | 3,383,535 |
| jit t=100 | 982 | 273 | 664 | 0.91x | 44 | 12 | 3,384,049 |
| jit t=1000 | 949 | 271 | 639 | 0.95x | 37 | 11 | 3,370,008 |
| aot | 235 | - | - | 3.83x | - | - | - |

**200,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 7315 | 2352 | 4686 | 1.00x | - | - | - |
| jit t=1 | 4622 | 2418 | 1884 | 1.58x | 86 | 108 | 123 |
| jit t=2 | 6385 | 2379 | 3657 | 1.15x | 66 | 22 | 22,578,685 |
| jit t=10 | 7640 | 2371 | 4959 | 0.96x | 58 | 17 | 29,446,718 |
| jit t=100 | 7886 | 2394 | 5164 | 0.93x | 44 | 12 | 29,447,225 |
| jit t=1000 | 7764 | 2401 | 4974 | 0.94x | 37 | 10 | 29,433,391 |
| aot | 1831 | - | - | 4.00x | - | - | - |

What it says:

- **No threshold of 10 or more beats the interpreter at any size,** and they
  lose by about the same at every size, 4-14%. The crossings grow with the
  data, about 150 per order, so the loss is per call rather than a fixed cost.
  Step 2 is the first thing to do; nothing tuned on top of today's crossings
  means much.
- **Compiling everything wins once the run is long enough,** and only then:
  2x on the running phase at 20,000 and 2.5x at 200,000, but at 2,000 its 60
  ms of codegen is most of the whole interpreted run of 75 ms. That is the
  trade a threshold exists to make, and today no threshold makes it, because
  the code worth compiling is in loops. Cheaper codegen (`opt_level`) would
  move the break-even down.
- **Threshold 2 sits between** because it catches the queries the report
  calls twice and the functions they call, and leaves the rest crossing.
- **AOT is the ceiling, at 3.8-4.5x,** and threshold 1 is about 60 ms of
  running from it at 20,000 once codegen is taken out.

Two things the first run of the grid found, both fixed before this one:

- **The compile phase was 13% slower under `--jit` at 200,000 orders,** 2,615
  against 2,307 ms, which was not the jit: `--jit` ran the whole command on a
  spawned thread, and glibc gave that thread its own malloc arena, which the
  allocation-bound front end does worse in. `MALLOC_ARENA_MAX=1` took the
  difference away. The thread had been added for "Cranelift limitations with
  PIE binaries", which a thread cannot affect, and the JIT runs on the main
  thread without it; it now does, and the compile phases match.
- **The AOT executable crashed at 200,000 orders,** overflowing its stack in
  `__dtlv_statics_init`, whose frame grew with the data: every collection
  constant took a stack slot of its own for its elements, 13 MB of them. They
  now share one slot used as a stack, with anything over 4 KB built on the
  heap (`ConstScratchArea` in the Cranelift codegen), and the frame is 320
  bytes.

## Making the crossings cheap

`perf record` of `--jit-threshold 100` at 20,000 orders, by self time, against
the same report under the interpreter alone:

| Symbol | Interpreter | Threshold 100 |
|---|---|---|
| `run_body` (the bytecode loop, planned calls inlined) | 32.4% | 5.4% |
| `execute_call_site` | - | 7.7% |
| `ExecutionContext::get_unit` | - | 5.6% |
| `OptimizingDispatcher::dispatch_call` | - | 3.0% |
| `run_fast_call`, `fast_call`, `run_general_call` | - | 6.2% |
| `resolve_arg` | - | 2.0% |
| `JitEngine::record_call` | - | 1.9% |
| `LayoutCache::get_or_compute` | - | 1.6% |
| `execute_native_call` | - | 1.5% |
| `bridge::call_jit` | - | 1.2% |
| compiled code (`[JIT]`) | - | 2.9% |

About 30% of the run is the general call path, against 3% in the code it
calls: entering a compiled function costs about ten times what the function
then does, roughly 80 ns a crossing over 3.4 million. Where it goes, for one
call from bytecode to a compiled `db.line_count`:

1. `valid_plan` refuses every plan while a dispatcher is installed, so the
   call leaves the loop for `run_general_call` and `execute_call`.
2. `get_unit` finds the callee again, in two `BTreeMap`s
   (`ModuleFunctionRegistry::modules`), which is the 5.6%.
3. `execute_call_site` finds the layout in the `LayoutCache`, pushes a whole
   interpreter frame for the callee, resolves each argument into a `Value` in
   it and fills in the shape descriptors -- all before offering the call to the
   dispatcher, which, taking it, uses none of the frame but the argument
   pointers.
4. `try_dispatch_call` takes the dispatcher out of its `RefCell` and back.
5. `dispatch_with` works out the `FuncIdentity` again, the callee's context
   (`for_callee`), whether it is `enterable`, probes `states`, sets the
   thread's dispatch context, and only then is `call_jit` a few stores and an
   indirect call.

And the refusal in step 1 applies to every call, not just those into compiled
code: calls to rider natives and to functions still interpreted go the long
way too (`execute_native_call` is 1.5% by itself), so installing the JIT taxes
calls it never compiles.

The avenues, cheapest first:

**A. Trim the general path.** Still worth having for the IR walker and the
calls the fast path does not suit. A dense index or an `FxHashMap` for the
module registry rather than nested `BTreeMap`s; ask the dispatcher before
pushing the callee's frame, from arguments resolved into a small array, and
push it only when the call stays in the interpreter; carry the identity and
context the call site has already worked out into `dispatch_with` rather than
working them out again; check `enterable` once, at compile time, which it
already is for the decision that matters. Each is local and the gain is a
fraction of the 30%; none removes the leaving of the loop.

**B. A planned call into compiled code.** The bytecode already makes a call to
a rider native without leaving the loop: a `NativePlan` holds the function
pointer and how to read each C word of its arguments, and the loop builds the
words and calls. A compiled function is entered the same way -- the runtime
handle, the `sret` pointer, a pointer per argument, then the descriptors -- so a
`Plan::Jit` holding the code pointer, whether it takes `sret`, and the
argument recipes is the same machinery. Compiled code never changes or goes
away once published (there is no deoptimization), so the plan holds for the
code epoch as a body plan does. The dispatch context the trampoline needs, for
calls out of compiled code into functions not yet compiled, can be set once
when the loop starts rather than per call. It should take a crossing from about
80 ns to about what a planned native call costs, which has not been measured.

**C. Count in the call site, not in the dispatcher.** What makes the plans
invalid under a dispatcher is that the JIT has to see each call to count it.
If a body plan carries a countdown instead, a planned call costs one decrement
more than it does now, and only when it reaches zero does the call take the
slow path and tell the dispatcher how many calls it has seen -- which may
compile the callee, and if it does, the site's next refresh makes the plan a
`Plan::Jit`. Native calls keep their plans whatever happens. Chaos mode, which
decides at random per call, decides per batch instead, with a countdown of 1
to keep its present coverage. This is what takes the tax off calls the JIT
never compiles, and with B it makes the dispatcher something the loop asks
rarely rather than on every call.

B and C together are the change that matters: B alone still pays step 1 for
every call that is not into compiled code, and C alone makes the interpreted
calls cheap but leaves the crossings as they are. A is worth doing anyway, for
the paths that remain, and is small enough to measure first. The other
direction, compiled code calling into the interpreter through
`__jit_dispatch_call`, builds two `Vec`s and probes the layout cache per call;
the demo makes almost none of those, but OSR will, since a compiled loop calls
whatever its callees are.

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
