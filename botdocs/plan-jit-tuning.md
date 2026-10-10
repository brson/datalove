# Tuning the JIT

A plan for making the JIT's decisions -- what to compile and when -- good on a
program shaped like an application, using the store demo (`demos/store`) as
the workload, and for adding on-stack replacement (OSR), which the demo shows
the JIT needs.

> **This is a plan.** Steps 0 to 3 are done (October 2026); the rest is not
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
| Interpreter (bytecode) | 866 ms | - | - |
| `--jit` (threshold 1) | 571 ms | 86 | 63 ms |
| `--jit-threshold 2` | 555 ms | 65 | 43 ms |
| `--jit-threshold 100` | 550 ms | 43 | 34 ms |
| AOT executable | 228 ms | - | - |

At 20,000 orders; the [grid](#user-content-the-baseline-grid) has the rest. Compiling the program, which reads the data and evaluates
the id maps under CTFE, is about 300 ms of every `script` run (`--time`
prints it), so the threshold-1 JIT runs the report in about 300 ms, of which
about 70 ms is codegen and some of the rest is `JitEngine::new`. That is within
reach of AOT, at the price of compiling every function called even once.

**A threshold of 100 is 1.57x the interpreter, ahead of threshold 1 at
1.52x,** since [OSR](#user-content-osr) enters the queries' loops, called once,
in compiled code. Before OSR it was 1.28x, and before calls into compiled code
were planned it was slower than having no JIT at all; see [Making the
crossings cheap](#user-content-making-the-crossings-cheap). Codegen now
includes the entries compiled at loops.

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

**2. Make every threshold at least as fast as the interpreter (done).** See
[Making the crossings cheap](#user-content-making-the-crossings-cheap). Every
threshold from 2 up now beats the interpreter at every size; threshold 1
still loses at 2,000 orders, to its codegen rather than to crossings.

**3. OSR (done),** [below](#user-content-osr). The goal was a threshold of 100
with OSR running the report close to the threshold-1 time while compiling far
fewer than 86 functions; it runs it faster than threshold 1, compiling 39.

**4. Tune.** Sweep the call threshold, the back-edge threshold -- or one
counter of calls plus back-edges over some k, as HotSpot does -- and
Cranelift's `opt_level` (`none` against `speed`, since codegen is now a
measurable share of the run). Score total wall time across all three data
sizes and pick what does well across them, not what wins at 20,000.

**5. Check against other programs.** `benchvs`, the `jit` bench in
`datalove-bench` and `botdocs/learn.dfs`, so the result is not fitted to one
program, then record it in the compiler guide's performance notes.

## The baseline grid

From `just grid` with OSR (step 3) at its default threshold of 1000
iterations, on a quiet machine. Times in ms, medians of 7 rounds (3 at
200,000). Wall is the whole process; compile and run are what `--time`
reports; the last three columns are from one `--jit-stats` run, codegen
including the entries compiled at loops.

**2,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 148 | 62 | 76 | 1.00x | - | - | - |
| jit t=1 | 162 | 62 | 87 | 0.91x | 86 | 57 | 122 |
| jit t=2 | 150 | 62 | 76 | 0.99x | 65 | 40 | 72,205 |
| jit t=10 | 148 | 63 | 74 | 1.00x | 53 | 38 | 77,309 |
| jit t=100 | 144 | 63 | 70 | 1.03x | 42 | 33 | 76,163 |
| jit t=1000 | 146 | 63 | 72 | 1.01x | 33 | 31 | 68,257 |
| aot | 34 | - | - | 4.37x | - | - | - |

**20,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 866 | 252 | 582 | 1.00x | - | - | - |
| jit t=1 | 571 | 249 | 284 | 1.52x | 86 | 63 | 123 |
| jit t=2 | 555 | 249 | 273 | 1.56x | 65 | 43 | 71,518 |
| jit t=10 | 555 | 248 | 271 | 1.56x | 54 | 39 | 76,836 |
| jit t=100 | 550 | 248 | 266 | 1.57x | 43 | 34 | 75,717 |
| jit t=1000 | 556 | 249 | 272 | 1.56x | 36 | 33 | 68,138 |
| aot | 228 | - | - | 3.80x | - | - | - |

**200,000 orders**

| Engine | Wall | Compile | Run | vs interp | Compiled | Codegen | Interp->JIT calls |
|---|---|---|---|---|---|---|---|
| interp | 7120 | 2218 | 4636 | 1.00x | - | - | - |
| jit t=1 | 4373 | 2258 | 1805 | 1.63x | 86 | 95 | 123 |
| jit t=2 | 4264 | 2236 | 1771 | 1.67x | 65 | 43 | 71,144 |
| jit t=10 | 4230 | 2205 | 1767 | 1.68x | 54 | 40 | 76,581 |
| jit t=100 | 4232 | 2212 | 1759 | 1.68x | 44 | 35 | 75,486 |
| jit t=1000 | 4264 | 2220 | 1775 | 1.67x | 36 | 33 | 67,964 |
| aot | 1714 | - | - | 4.15x | - | - | - |

What it says:

- **Every threshold from 2 up runs the report faster than compiling
  everything,** 1.56-1.68x the interpreter past 2,000 orders against 1.52-1.63x
  for threshold 1, compiling 33-54 functions rather than 86. Before OSR they
  were 1.28-1.46x.
- **The thresholds are all alike now,** because the loops decide it: a query
  is entered at its loop whatever the threshold for calls.
- **Calls into compiled code fell from 3.2 million to about 75,000,** since
  the loops making them are compiled too.
- **At 2,000 orders it is about even with the interpreter,** and threshold 1
  still loses: codegen, 31-57 ms, is a large share of a 75 ms run. The OSR
  threshold and `opt_level` are what step 4 has to tune for that.
- **AOT is the ceiling, at 3.8-4.4x.** At 200,000 orders the JIT's running
  phase, 1.76 s, is about what AOT's whole run is, 1.71 s; what is left between
  them is the front end, which compiles the program for the JIT on every run.

Before OSR, at 20,000 orders: threshold 100 ran at 1.28x and threshold 1 at
1.47x. Before step 2, at 20,000 orders: thresholds 10, 100 and 1000 ran at 0.93x,
0.91x and 0.95x the interpreter, and threshold 2 at 1.19x.

Two things the first run of the grid found, both fixed before step 2:

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

**What was done: B and C.** A site planning a call to a body under a
dispatcher asks it for a `SitePolicy` (`CallDispatcher::site_policy`) and
follows it: `Enter` gives a `Plan::Jit`, made from the argument recipes a
native plan uses and entered through `CallDispatcher::call_compiled`; `Count`
gives a body plan with a countdown, whose last call goes by the general path
with `DispatchCallContext::weight` saying how many it stands for, after which
the site asks again; `Interpret` gives a plain body plan; `EveryCall` plans
nothing, as before. `JitEngine` enters compiled code, interprets what it will
not compile, and otherwise counts for as many calls as the function lacks of
the threshold, so a function called from one site is compiled on the call it
would have been. Chaos mode draws at each asking between the engine's policy
and `EveryCall`. A plan made under one dispatcher is not trusted under
another (`IrInterpreter::dispatcher_epoch`). Calls to natives keep their plans
under a dispatcher, since one is never offered those.

After it, the same profile has no `execute_call_site` or `get_unit` in it; a
crossing is `call_planned_jit` (4.7%), `JitEngine::call_compiled` (1.5%), the
dispatcher's forwarding (0.9%) and `call_words` (0.5%), about 7.5% of the run
against 30%. `call_planned_jit` takes the dispatcher out of its `RefCell` and
puts it back each call, so that compiled code can call back into the
interpreter, and builds the words afresh; how the 4.7% divides between those
has not been measured. A is not done.

B and C together were the change that mattered: B alone still pays step 1 for
every call that is not into compiled code, and C alone makes the interpreted
calls cheap but leaves the crossings as they are. A is worth doing anyway, for
the paths that remain, and is small enough to measure first. The other
direction, compiled code calling into the interpreter through
`__jit_dispatch_call`, builds two `Vec`s and probes the layout cache per call;
the demo makes almost none of those, but OSR will, since a compiled loop calls
whatever its callees are.

## OSR

Done (step 3). Two facts about this codebase made it smaller than OSR usually
is.

**No way back.** The JIT does not speculate, so compiled code never needs to
leave for the interpreter. OSR goes one way, interpreter to compiled code, and
the hard half of OSR elsewhere -- materializing an interpreter frame from
compiled state -- does not arise.

**One frame layout.** Both sides lay a function's frame out with
`FrameLayout::compute` from the same types: values, slots and tracking bytes
at the same offsets. A loop's carried values are block parameters, which have
fixed places in the frame, so at a loop header the loop's state is in the
frame bytes.

How it works:

- **Counting.** The bytecode starts every loop header with an
  `Op::LoopHead`, which counts down a `LoopCounter` in the body's
  `BcFunction`, shared by every call running it. A header is an in-loop block
  entered by an edge from a block at or after it. When a header is copied
  into its latch by `duplicate_short_branches`, the op is copied with it, and
  the frame is as entering the header wherever it runs. When the count runs
  out, `IrInterpreter::loop_hot` asks the dispatcher for a `LoopPolicy` --
  count so many more, interpret, or enter -- reporting how many iterations it
  counted. With no dispatcher it asks again every 65,536.
- **Deciding.** `JitEngine::loop_policy` sums iterations per `(function,
  header)` against `osr_threshold` (`--jit-osr-threshold`, default 1000) and
  then compiles an entry there for the frame that asked; later frames to ask
  are given the same entry at once. Only a function's frame is entered, not a
  script unit's.
- **Compiling.** `FunctionCompiler::compile_osr_as` compiles the blocks
  reachable from the header, entered by a block of its own taking `(rt,
  [sret], frame)`. The frame is a `FrameBase::Ptr`: every frame address is
  formed off the interpreter's frame instead of the code's own stack slot, so
  references into it stay good. The entry block loads the parameters'
  pointers and descriptors and the shape descriptors from where the
  interpreter keeps them (`OsrSpec`), and the header's block parameters from
  their places, and jumps to the header. A value defined before the loop is
  read from its place in the frame the first time compiled code asks for it,
  inserted before the entry's jump; a scalar constant is made again instead,
  since the bytecode folds `u32` constants into what reads them and may never
  write them to the frame.
- **Refusing.** If the code reached defines a value the header needs -- an
  inner loop's header, needing what the enclosing loop's body defines -- there
  would be two places it comes from with nothing to choose between them, so
  `osr_check` refuses the header (a liveness pass over the reached blocks,
  with what each block's compilation defined). The outer loop's header is
  then the one entered, which compiles both loops. A reference whose
  descriptor only its projection works out is refused too.
- **Entering.** The interpreter calls the entry with its frame and the call's
  result destination; the code runs the rest of the call, and the
  interpreter returns from the frame as it does after a `Return`.

What it took beyond the sketch:

- **A dispatcher for compiled code's callees.** A call the dispatcher makes
  takes it out of the interpreter for as long as the call runs, so compiled
  code calling an interpreted function through `__jit_dispatch_call` used to
  run it with no JIT at all: nothing it called was counted, entered or
  compiled. With the query loops compiled, `ord.sorted` went that way, and
  the 613,000 calls it makes to `ord.greater_at` ran interpreted instead of
  compiled; OSR made the demo slower until the trampoline lent the engine to
  the interpreter for the call (`IrInterpreter::lend_dispatcher`,
  `LentEngine`). That made the trampoline's thread-local context nest, so
  `set_dispatch_context` now returns the context it replaced for
  `restore_dispatch_context` to put back.
- **Not done:** compiling the function's ordinary entry at the same time.
  The function is compiled by its own call count, as before.

Tested by an `osr` engine in `engine_tests` (no function compiled by its
calls, every function's loops entered after one to three iterations, by the
fixture's seed), the same at two iterations in `std_engine_tests`, where the
library's loops are, and `std_tests/176_osr_loops`: carried scalars, an
inner loop refused and one entered, owned values made in the loop, a
borrowed parameter, an error from inside the loop, a generic function (one
entry for both instantiations, its descriptors from the frame) and a
`break`. Chaos mode enters loops at random. The engine fixtures have few
loops in functions -- eight -- so the library is where the coverage is.

Known rough edges:

- An aggregate parameter of the header is passed as its own address, which
  the header then copies onto itself.
- Compiled code's entries, ordinary and OSR, are kept by `FuncIdentity`, and
  nothing forgets them when bodies are replaced.
- Every call from compiled code into the interpreter allocates two `Vec`s and
  probes the layout cache; with OSR there are many more of those.
