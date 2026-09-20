# The JIT and the Inliner: State, Measurements, and What to Do

Both were built to show the shape could work, and neither has been tuned or turned on.
This is what they do today, measured rather than reasoned about, what current practice
does instead, and a staged plan with a measurement attached to each stage.

The short version, in case nothing else is read:

- **The jit is worth turning on.** It is 85x on a tight arithmetic loop and 2-3.5x on
  call-heavy code. It costs a fixed 14ms of process startup and a synchronous compile
  pause per function.
- **The inliner was a regression, by up to 2.8x, and the cause was not the inlining.**
  Every entry to a function with an inlined body deep-cloned the body; every call
  recomputed a frame layout that never changes; and every call allocated a frame out of
  five to seven separate pieces. Fixing those three made the interpreter **1.5-1.8x
  faster on call-heavy code on its own**, and the jit up to 1.86x with none of its own code
  touched. Inlining went from 0.35x to roughly break-even -- which is the point: making
  calls cheap is what makes inlining worth less.
- **The jit never compiles an inlined body.** The combination the whole
  `OptimizingDispatcher` exists for is not wired up; only the metric label is.
- **The C AOT backend compiles its output at `-O0`.** Changing that one flag to `-O2`
  made it 8.4x faster and moved it past the cranelift AOT backend.
- **Cranelift has had an inliner since 0.123**, and the tree is on 0.135. For the two
  compiled backends that is a better place to inline than the datalove IR.
- **The three backends disagreed about uninitialized frame memory.** Neither compiled
  backend zeroes a frame -- the cranelift one poisons it on purpose -- while the
  interpreter zeroed and depended on it, because it had no liveness bit for a frame value
  where the other two carry a tracking byte. It has one now
  ([why](#user-content-why-the-interpreter-zeroed-its-frames-and-why-it-no-longer-does)).

## Contents

- [How these were measured](#user-content-how-these-were-measured)
- [The numbers](#user-content-the-numbers)
- [What Stage 1 did](#user-content-what-stage-1-did)
- [What Stage 2 did](#user-content-what-stage-2-did)
- [Why the interpreter zeroed its frames](#user-content-why-the-interpreter-zeroed-its-frames-and-why-it-no-longer-does)
- [What Stage 3 did](#user-content-what-stage-3-did)
- [State of the jit](#user-content-state-of-the-jit)
- [State of the inliner](#user-content-state-of-the-inliner)
- [What current practice does](#user-content-what-current-practice-does)
- [Is inlining the interpreter alone worthwhile?](#user-content-is-inlining-the-interpreter-alone-worthwhile)
- [Is inlining either AOT worthwhile?](#user-content-is-inlining-either-aot-worthwhile)
- [The plan](#user-content-the-plan)
- [Faults found on the way](#user-content-faults-found-on-the-way)

## How these were measured

Three instruments, because no single one answers the question.

**`crates/datalove-bench/benches/jit.rs`** is the in-process harness and the one to reach
for. It runs six workloads in four configurations -- `interp` with no dispatcher,
`inline` with the jit off, `jit` with inlining off, and `jit_inline` with both -- and times
**only execution**. Building the database, compiling the fragment and constructing the
dispatcher happen in divan's `with_inputs`, which is not counted; constructing a
`JitEngine` alone reserves a 64MB arena and costs more than several of these workloads.
The `floor` workload is `debuglog 1` in each configuration, so the harness's own cost can
be subtracted.

Thresholds in the harness are 1, not production's 100 and 50. At production thresholds
most of these workloads never reach either optimization and all four configurations
measure the same thing. What the thresholds cost is a separate question from what the
optimizations are worth, and mixing them measures neither.

This file replaced a previous version which measured nothing at all. Its benchmark source
did not typecheck -- `let repeat = 10` infers `int` where `run_many` wanted `u32` -- and
it ran the result through `if let Some(ir_unit)`, so both of its benchmarks executed an
empty program for as long as the file existed. Its reported `interp_only` figure of 265us
for what should have been a million loop iterations was the compiler front end and nothing
else, and its `interp_with_jit` figure of 3ms was that plus a thread spawn and a 64MB
arena. The new harness `expect`s the compile, so it cannot silently measure nothing again.

**hyperfine over the release CLI** measures what a user experiences, startup included:

```
hyperfine -w 2 -m 8 "datalove script W.dfs" "datalove script --jit W.dfs"
```

Process startup and front-end compilation are about 88ms of every one of these, so an
empty script is measured as the floor and subtracted.

**`perf record` over the bench binary** attributes the time:

```
cargo bench -q -p datalove-bench --bench jit -- 'call_chain::inline'   # build it
perf record -g -o /tmp/p.data -- target/release/deps/jit-* 'call_chain::inline' --bench
perf report -i /tmp/p.data --stdio --no-children -g none --percent-limit 1.2
```

Machine: 4 CPUs, 16GB, inside the sandbox, `SANDBOX_LIMITS` governed. Ratios are what
matter here, not absolute times; the fleet shares a CPU cap and absolute numbers will move.

## The numbers

### Execution only, in process

**These are the figures this report was written against, before Stage 1 landed.** The
numbers after it are in [What Stage 1 did](#user-content-what-stage-1-did); this table is
kept because the rest of the report reasons from it and because the ratios between the
four configurations are what motivated the plan.

Means over 100 samples. Ratio is against `interp`.

| Workload | `interp` | `inline` | `jit` | `jit_inline` |
|---|---|---|---|---|
| `floor` | 13.8us | 32.6us | 28.5us | 29.2us |
| `loop_arith` -- 2M loop iterations, 20 calls | 188.6ms | 188.8ms **1.00x** | 2.20ms **86x** | 2.10ms **90x** |
| `small_callee` -- 200k calls to a one-line callee | 29.8ms | 38.7ms **0.77x** | 8.49ms **3.5x** | 8.34ms **3.6x** |
| `call_chain` -- 100k calls three deep | 72.0ms | 103.3ms **0.70x** | 26.3ms **2.7x** | 26.7ms **2.7x** |
| `recursive` -- `fib(24)`, ~75k calls | 106.0ms | 304.9ms **0.35x** | 49.9ms **2.1x** | 49.9ms **2.1x** |
| `stdlib_list` -- 40k `list.push` through `sys/std` | 28.4ms | 18.9ms **1.50x** | 15.8ms **1.8x** | 15.8ms **1.8x** |

Three things are visible at a glance.

The jit is a large win everywhere, and an enormous one where a loop runs without crossing
a call boundary.

Inlining alone is a **regression in three of the four workloads that have calls to
inline**, badly so on recursion, and a 1.5x win in exactly one. That one is the only
workload where the caller being rewritten is entered a few hundred times and then does a
few hundred iterations of real work per entry; the others enter the rewritten caller
100,000 times and do almost nothing per entry.

`jit` and `jit_inline` are identical within noise in every workload. That is not inlining
failing to help -- it is inlining not reaching the jit at all.

### Process level, startup included

| Workload | `script` | `script --jit` |
|---|---|---|
| `debuglog 1` (floor) | 88.0 +- 5.7ms | 101.6 +- 6.4ms |
| `loop_arith`, 20M iterations | 2.40s | 107ms |
| `loop_arith`, 200M iterations | ~23s (extrapolated) | 182ms |
| `fib(30)`, ~2.7M calls | 2.022s | 966ms |

`--jit` costs a flat **13.6ms** more than not passing it, on a script that does nothing.
That is `JitEngine::new`: reserving the arena, building the ISA, and registering every
runtime symbol. For a CLI or a REPL prompt that is the whole latency budget.

Net of the floor, `fib(30)` is 715ns per call interpreted and 320ns per call jitted. A
native call should be a handful of cycles. 320ns at this clock is roughly a thousand.

### AOT

| Workload | cranelift AOT | C AOT `-O0` (today) | C AOT `-O2` |
|---|---|---|---|
| `loop_arith`, 20M | 10.4ms | 60.1ms | **7.1ms** |
| `chain`, 20M calls three deep | 117.7ms | -- | **0.79ms** |
| `fib(30)` | 409.7ms | -- | 399.9ms |

The `-O2` column was produced by changing one string in
`crates/datalove-datafun/src/pipeline/c_aot.rs` and rebuilding; the change was reverted
and is not in the tree.

`chain` at 0.79ms is the C compiler inlining the three-deep chain, seeing that the loop's
result is overwritten every iteration, and deleting the loop. That is an extreme case
chosen to make the point rather than a typical speedup, and the point it makes is real:
the host C compiler brings an inliner, LICM and dead-code elimination that neither of the
other two backends has at all.

`fib(30)` is where the two AOT backends agree, because recursion is where an inliner has
the least to give. It is also 2.4x faster than the jit on the same program and 5x faster
than the interpreter, which bounds what jit tuning can be expected to recover.

### Where the time goes

`call_chain`, percentages of cycles, symbols folded.

**`interp`** -- the interpreter's own loop dominates, as it should:

| | |
|---|---|
| `execute_instruction` | 7.3% |
| `execute_blocks` | 4.9% |
| `malloc` + `cfree` + `calloc` | 11.1% |
| `call_in_context_with_shapes` | 4.5% |
| hashing an `IrType` with SipHash | 4.1% |
| `Frame::new` + `IrLayout::compute` | 6.5% |
| `IrTyDescTable::get_or_create` | 3.0% |

**`inline`** -- the interpreter is no longer what dominates:

| | |
|---|---|
| SipHash, total, over `FuncIdentity` / `IrType` / `CallSiteKey` | **~13.8%** |
| cloning the inlined body (`Vec<IrBlock>::clone`, `Instruction::to_vec`, `Option::cloned`) | **6.4%** |
| `malloc` + `cfree` + `RawVec::deallocate` | 9.7% |
| `execute_instruction` | 4.5% |
| `OptimizingDispatcher::dispatch_call` + `get_inlined_function` | 3.5% |

**`jit`** -- more than half the time is the trampoline's bookkeeping:

| | |
|---|---|
| hashing, total (`IrType` 9.1%, Sip13 write 8.8%, `FunctionKey` 5.9%, `IrType::hash` 2.8%) | **~26%** |
| `__jit_dispatch_call` | 9.2% |
| `IrTyDescTable::get_or_create` | 5.8% |
| HashMap key `eq` | 4.6% |
| `bridge::call_jit` | 2.7% |
| `JitEngine::record_call_with_context` | 2.2% |

The single largest cost in the jit configuration is hashing an `IrType` tree with SipHash
to find a type descriptor, once per argument per call, for a type the stub knew at compile
time.

### What Stage 1 did

Landed: the layout cache, the `Rc` in place of the clone, and `FxHashMap` on the four hot
maps. Means over 100 samples, same machine, same session.

| Workload / config | before | after | |
|---|---|---|---|
| `call_chain` `interp` | 72.3ms | 58.3ms | **1.24x** |
| `call_chain` `inline` | 106.3ms | 51.8ms | **2.05x** |
| `call_chain` `jit` | 27.5ms | 15.4ms | **1.79x** |
| `recursive` `interp` | 108.4ms | 77-87ms | **1.25x** |
| `recursive` `inline` | 299.6ms | 73-80ms | **4.0x** |
| `recursive` `jit` | 48.8ms | 45.0ms | 1.08x |
| `small_callee` `interp` | 29.9ms | 25.1ms | **1.19x** |
| `small_callee` `inline` | 39.4ms | 29.4ms | **1.34x** |
| `small_callee` `jit` | 8.52ms | 5.19ms | **1.64x** |
| `stdlib_list` `jit` | 16.0ms | 14.6ms | 1.10x |
| `loop_arith` (all) | -- | -- | unchanged |

`loop_arith` is unchanged because it makes 20 calls in total; none of this touches the
instruction loop.

And the answer to the question the stage was there to settle -- **is inlining a win or a
regression under the interpreter alone?** It flipped, in three of the four workloads that
have calls to inline:

| Workload | `inline` vs `interp` before | after |
|---|---|---|
| `recursive` | 0.35x | **1.18x** |
| `call_chain` | 0.70x | **1.12x** |
| `stdlib_list` | 1.50x | **1.47x** |
| `small_callee` | 0.77x | 0.85x |

`small_callee` is the case where a caller is entered 200,000 times and does one addition
per entry, so what is left of the per-entry cost still exceeds what removing one call
saves. That is the honest remaining limit, and it is Stage 2's and the frame-pooling
item's to fix, not the clone's.

The `jit` improvement is a side effect worth noting: it never touched the jit's own code
paths. Its trampoline calls `get_or_create` per argument per call, and making that lookup
cheap was worth 1.64x on `small_callee` by itself.

**What the profile looks like now.** `call_chain`, percentages of cycles. A dash is below
the 2.5% cutoff rather than absent, except on the rows marked **gone**, which no longer
appear at any percentage.

| | `interp` | `inline` | `jit` |
|---|---|---|---|
| `execute_instruction` | 9.7% | 8.6% | -- |
| `execute_blocks` | 5.1% | 5.2% | -- |
| `call_in_context_with_shapes` | 5.0% | 3.5% | -- |
| `malloc` + `cfree` + `calloc` | 10.9% | ~6% | 2.7% |
| `Frame::new` + `prepare_call_args` | 6.5% | -- | -- |
| `__jit_dispatch_call` | -- | -- | 12.6% |
| `IrTyDescTable::get_or_create` | -- | -- | 7.8% |
| SipHash | **gone** | **gone** | **gone** |
| cloning the body | -- | **gone** | -- |
| `IrLayout::compute` | **gone** | **gone** | **gone** |

For comparison, before Stage 1 the same three columns had SipHash at 4.1%, 13.8% and 26%,
the clone at 6.4% of `inline`, and `IrLayout::compute` at 3.0% of `interp`.

The interpreter's own loop now dominates the interpreter, which is what a profile should
look like. What remains above it is the allocator, which is
[Stage 2](#user-content-what-stage-2-did).

In the jit configuration the two remaining items are the trampoline itself and the
per-argument `get_or_create` it does -- the lookup is cheap now but still happening, for a
type the stub knew when it was compiled. That is Stage 3.

### What Stage 2 did

Landed: a frame pool, and the parameter descriptors moved into the cached layout.

`Frame::new` allocated five to seven vectors plus a zeroed data block per call. Function
frames are strictly nested -- a call returns before its caller goes on -- so a `FramePool`
holds one frame per level of call depth and a frame handed back is free for the next call
at that depth. `reset` reuses the data block when it is large enough and aligned well
enough and the capacity of every side table either way. And `IrLayout` now carries
`param_tydescs`, which removes the last `get_or_create` on the interpreter's call path: it
was called once per owned parameter per call for a type the signature fixes.

| Workload / config | after Stage 1 | after Stage 2 | |
|---|---|---|---|
| `call_chain` `interp` | 58.3ms | 40.9ms | **1.43x** |
| `call_chain` `inline` | 51.8ms | 41.8ms | **1.24x** |
| `recursive` `interp` | ~80ms | 65.5ms | **1.22x** |
| `recursive` `inline` | ~76ms | 63.5ms | **1.20x** |
| `small_callee` `interp` | 25.1ms | 20.3ms | **1.24x** |
| `small_callee` `inline` | 29.4ms | 23.7ms | **1.24x** |
| `small_callee` `jit` | 5.19ms | 5.13ms | 1.01x |
| `stdlib_list` `interp` | 26.4ms | 23.1ms | **1.14x** |
| `call_chain` `jit` | 15.4ms | 14.8ms | 1.04x |
| `loop_arith` (all) | -- | -- | unchanged |

**Cumulative, against the figures this report was written from:**

| Workload / config | before | now | |
|---|---|---|---|
| `call_chain` `interp` | 72.3ms | 40.9ms | **1.77x** |
| `call_chain` `inline` | 106.3ms | 41.8ms | **2.54x** |
| `call_chain` `jit` | 27.5ms | 14.8ms | **1.86x** |
| `recursive` `interp` | 108.4ms | 65.5ms | **1.66x** |
| `recursive` `inline` | 299.6ms | 63.5ms | **4.72x** |
| `recursive` `jit` | 48.8ms | 43.4ms | **1.13x** |
| `small_callee` `interp` | 29.9ms | 20.3ms | **1.47x** |
| `small_callee` `inline` | 39.4ms | 23.7ms | **1.66x** |
| `small_callee` `jit` | 8.52ms | 5.13ms | **1.66x** |
| `stdlib_list` `interp` | 27.8ms | 23.1ms | **1.20x** |
| `stdlib_list` `jit` | 16.0ms | 14.1ms | **1.14x** |

**And it changed the answer about the inliner again, in the other direction.** Making a
call cheap is making inlining worth less, which is exactly what should happen:

| Workload | `inline` vs `interp`: before | after Stage 1 | after Stage 2 |
|---|---|---|---|
| `recursive` | 0.35x | 1.18x | 1.03x |
| `call_chain` | 0.70x | 1.12x | 0.98x |
| `stdlib_list` | 1.50x | 1.47x | 1.33x |
| `small_callee` | 0.77x | 0.85x | 0.86x |

So the IR-level inliner is now roughly break-even under the interpreter on three of these
and a 1.33x win on the stdlib-shaped one, where what it removes is a cross-module call
rather than a local one. That is the conclusion to carry forward: **optimizing the call
path undercuts the case for inlining in the interpreter**, and the inliner's justification
is what it can feed the jit, not what it is worth alone. There is no reason to turn it on
for the interpreter by itself.

**The profile after Stage 2**, `call_chain` `interp`:

| | |
|---|---|
| `execute_instruction` | 14.3% |
| `execute_blocks` | 7.6% |
| `call_in_context_with_shapes` | 7.5% |
| `prepare_call_args` | 5.6% |
| `FramePool::take` | 5.0% |
| `read_operand` | 4.3% |
| `execute_call` | 2.9% |
| `LayoutCache::get_or_compute` | 2.1% |
| `malloc` / `cfree` / `calloc` | **gone** |
| `Frame::new` | **gone** |

The allocator is gone from the profile entirely, from 10.9% before. `FramePool::take` at
5.0% is what replaced it: a `memset` of the frame prefix and four `Vec` refills. The memset
went away afterwards, for a different
reason ([below](#user-content-why-the-interpreter-zeroed-its-frames-and-why-it-no-longer-does)).

What is left is the interpreter interpreting. Roughly 23% is still call machinery
(`call_in_context_with_shapes`, `prepare_call_args`, `FramePool::take`, `execute_call`,
`LayoutCache::get_or_compute`) and the rest is the instruction loop and operand reads.
Getting further into that means a contiguous frame stack rather than a pool of separate
frames, or specializing the instruction dispatch -- both of which are larger changes than
anything done so far.

### Why the interpreter zeroed its frames, and why it no longer does

Asked as a question about whether the three backends agree, and they did not.

**Neither compiled backend zeroes a frame.** The C backend emits a bare
`_Alignas(N) uint8_t __frame[N];` and initializes only the tracking-byte region to
`TRACK_UNINIT`; the frame holds whatever the C stack left. The cranelift backend allocates
an `ExplicitSlot`, which is not zeroed, and then deliberately fills aggregate slots with
`0xFF` -- its own comment is *"0xFF poison makes uninitialized reads obvious"* -- and
zeroes only the tracking bytes. Both carry a liveness bit per slot and per parameter
(`tracking_byte` in `cranelift/src/layout.rs`, `__frame[off] == TRACK_LIVE` in the C
backend's `emit_erasure_tracked`) and consult it rather than asking the memory.

**The interpreter zeroed, and that was load-bearing.** `prepare_call_args` destroys an
`out` argument's destination before the call, because the callee is about to overwrite
whatever is there. When the destination is a frame value that has never been written there
is nothing to free -- and a function frame had no liveness bit for a value
(`value_initialized` was `Some` only for script frames), so "all zeros" was doing the work
of "nothing here yet": the destroy read a null pointer and did nothing.

Filling the frame with `0xFF` on reuse instead proved it in one run. All 409 interp
fixtures and both dispatch suites still passed; `std_all_tests` failed with

```
free() called on untracked pointer: 0xffffffffffffffff
  at string_destroy_local
  at IrInterpreter::prepare_call_args  (lib.rs:2192)
```

which is precisely the signal the cranelift backend's poison exists to produce.

So the interpreter was the odd one out, and in the weaker direction: zeroing makes
"uninitialized" and "empty" the same value, which is the distinction the compiled backends
found they had to track. It now tracks it too. Function frames carry `value_initialized`
like script frames do, `prepare_call_args` asks the frame rather than the memory, and the
frame data is left undefined -- filled with `0xFF` in a debug build, untouched in release,
which is at least as loud as either compiled backend in debug and as cheap as the C one in
release.

Marking was already complete, which is what made this cheap: all 45 `Instruction` variants
that write a value dest mark it, `pass_block_args` marks a block parameter, and
`mark_out_params_initialized` marks an `out` destination after the call.

Two cases got stricter rather than merely equivalent. A `Slot` or an external destination
was also destroyed unconditionally before, relying on the same zeroing; both now consult
the bit they already had. And a destination is marked empty as soon as it is destroyed, so
nothing can free it twice if the callee errors before writing.

**It is not a speed change.** The `memset` that went away is offset by refilling one more
side table and setting a bool per value write; across four workloads the difference did not
resolve above the machine's noise, which was about +-10% at the time. It was done because
the backends should agree, and the measurement is only here to say it cost nothing.

### What Stage 3 did

The trampoline described its callee's parameters and return by looking each type up in the
descriptor table -- `arg_count + 1` probes of a tree-shaped `IrType` key, on every call out
of compiled code. All of it is fixed by the callee's signature, and the interpreter now
keeps a layout per body, so `IrLayout` carries `return_tydesc` beside `param_tydescs` and
the trampoline reads them off one cached layout.

Measured against the previous commit by interleaving the two builds twice, taking the
`fastest` column, with `loop_arith::jit` as a control since it makes twenty calls in total:

| Workload | round 1 | round 2 | control moved |
|---|---|---|---|
| `call_chain` `jit` | **12.9% faster** | **8.8% faster** | +1.6% / +3.4% |
| `small_callee` `jit` | **11.8% faster** | **13.2% faster** | as above |
| `recursive` `jit` | flat | flat | as above |

`recursive` is flat because `fib` does bigint arithmetic per call, so the trampoline is a
smaller share of it; `call_chain` and `small_callee` do almost nothing per call and are
most of trampoline.

Cumulative for the jit on call-bound work: `call_chain` 27.5ms to ~13.0ms (**2.1x**),
`small_callee` 8.52ms to ~4.3ms (**2.0x**).

**The profile after Stage 3**, `call_chain` `jit`:

| | |
|---|---|
| `__jit_dispatch_call` | 16.7% |
| `LayoutCache::get_or_compute` | 4.8% |
| `bridge::call_jit` | 3.7% |
| `JitEngine::record_call_with_context` | 3.1% |
| `malloc` + `cfree` | 5.5% |
| `IrTyDescTable::get_or_create` | **gone** |
| SipHash | **gone** |

`__jit_dispatch_call` grew as a share because the total shrank. What is left inside it is
three lookups of the same callee -- `get_unit` to find the IR, `layout_for` to find the
layout, `record_call_with_context` to find the compiled code -- plus a `Vec` for the
argument values.

All three lookups are avoidable, and that is the endgame for this trampoline: a stub is
compiled per call site with the callee's key as a constant, so it could equally carry a
constant pointer to a record holding the resolved unit, layout and code pointer. That
removes the remaining hash probes and leaves the argument vector as the only per-call cost.
It is a larger change than anything above, because that record has to outlive the compiled
code that points at it.


## State of the jit

### It has no idea a loop exists

`JitEngine::record_call` counts **calls and only calls**. There is no back-edge counter
and no on-stack replacement. A function called once that then loops ten million times is
never compiled, and a function compiled while it is running does not benefit that
invocation, because compiled code is only ever entered at a call.

This is the largest single gap against current practice, where counting loop back edges
and OSR-entering mid-loop is the standard mechanism and has been for twenty years.
`loop_arith` only shows an 86x win because its inner loop happens to be in a function
called twenty times; move the loop up one level and the jit sees nothing.

### Every call from compiled code leaves through a trampoline, forever

`define_stub` emits, for each call site, a stub that spills the arguments to a stack slot,
spills the descriptors to another, and calls `__jit_dispatch_call`. That function then, on
**every call**, reads a thread-local through a `RefCell`, decodes the key, looks the callee
IR up, allocates a `Vec` for the argument values, calls `get_or_create` for each argument's
type descriptor, allocates a second `Vec` for the shape descriptors, does a `FunctionKey`
HashMap lookup in `record_call_with_context`, and only then calls the callee.

Nothing is ever patched. A compiled function calling another compiled function pays all of
that, every time. There is no direct call and no inline cache. This is why the jit is 86x
on a loop and 2.1x on recursion, and it is the mechanism by which the inliner would earn
its keep: inlining removes calls, and calls are exactly what the jit is worst at.

### The compile is synchronous, at the top optimization level, on the critical path

`opt_level` is set to `"speed"` in `JitCompiler::new`, and compilation happens inline in
the dispatch that crossed the threshold. There is no cheap first tier and no background
thread. Current practice puts a fast tier first precisely because a slow optimizing tier
on the critical path is the latency hazard; V8 added Maglev because for a function that
runs hundreds rather than thousands of times, TurboFan's ~1ms compile costs more than it
recovers.

### Nothing evicts, nothing recompiles, nothing deoptimizes

`unique_symbol` and the stub counter both use global atomics, so recompiling a function
produces a new symbol and every caller's stub is emitted afresh per caller. Nothing is
freed. The arena is a fixed 64MB and the failure mode when it fills is a compile error
mentioning `JIT_ARENA_SIZE`. For a batch script this does not matter. For a REPL, or
anything long-running, it is a leak with a hard ceiling.

### What cannot be jitted, counted

Instrumented over the whole corpus -- 409 interp worldfiles, 143 `std_all_tests` fixtures,
21 four-backend fixtures -- there are exactly three reasons a function is not compiled, and
they are nothing like equal in weight.

| Reason | Calls refused | Distinct functions |
|---|---|---|
| The callee declares shapes | **522** | **59** |
| The callee is reached by `CodeRef::External` | 2-3 | 1 |
| The other twenty-three `Unsupported` sites in the codegen | **0** | 0 |

**A generic that builds a collection.** This is the whole of it. `bridge::dispatchable`
refuses any callee whose `descriptor_shapes` is non-empty, and the interpreter keeps such a
call before the dispatcher is even asked (`!supplied.is_empty()`), so it never appears as an
error -- the instrumented run never reached `dispatchable` at all, which is belt-and-braces
for a gate that already fired. The 59 are essentially `sys/std`'s generic builders: `zero`,
`one`, `singleton`, `to_list`, `reversed`, `sorted`, `keys`, `extend`, `sum`, `product`,
`min_value`, `max_value`, `pair`, `set_of`, `flattened`. It is why `stdlib_list` gets 1.8x
rather than more.

The reason given for the gate -- that a `CallDispatcher` is handed the argument values and
nothing else -- is the only part still true, and it is now the *only* missing piece.
`bridge::call_jit` takes `shape_descriptors` already, `__jit_dispatch_call` reads them off
the back of the descriptor array already, and `build_signature` lays them out already; all
of that landed with the two jit boundaries. The interpreter has the resolved `supplied`
vector in its hand at the gate. So lifting this is: carry the shapes on
`DispatchCallContext`, offer the call to the dispatcher before keeping it, forward them to
`call_jit`, and fall back to `execute_call_with_shapes` when the dispatcher declines. See
[What each backend carries](../generics.md#user-content-what-each-backend-carries).

**A call into an earlier script unit.** `resolve_code_ref`'s `External` arm errors
unconditionally with "not yet implemented" -- but `compile_function_with_context` does build
a stub for an external callee, and registers it in `local_funcs`, and
`callee_descriptor_params` and `callee_param_modes` both already read External out of that
same map with `Local(id) | External { id, .. }`. So the metadata path handles External and
only the `FuncId` lookup does not. `interp/053_script_to_prior_script_call` is the fixture.

Three lines would fix the symptom. They should not be the fix, because `local_funcs` is
keyed by `CodeUnitId` alone: a body calling both `Local(0)` and `External { unit: n, id: 0 }`
puts two stubs under one key and the later insert wins, silently. That is the hazard
`interp/947_crossunit_local_id_reuse` exists for, one level down. The keyspace wants
splitting first.

**The twenty-three that never fire.** Worth knowing what they are, because the list reads
alarming and mostly is not:

- Guards on IR that lowering does not emit: `scalar pack with multiple fields`,
  `scalar get_field with field_index n (max 0)` -- a `Pack` or `GetField` whose destination
  is a scalar but which names more than one field.
- Script-only operands: `external slot store`, `set_field on external slot`. A function body
  cannot name another unit's slots.
- Operation and type combinations the surface language cannot express.
  `Int binop not yet supported` covers `Div` and `Mod`, and `int / int` is refused by the
  typechecker outright (F026 `invalid operand type 'int' for operator '/'`) while `%` is not
  an operator at all. What is left of float's `_` arm is the bit and logic operations, which
  do not typecheck on a float.
- Assertions: `local function not yet declared`, `module function not pre-compiled`. The
  stub pass fills both maps before compiling.

So most of them are assertions wearing a recoverable error's clothing, and that is itself a
problem: a *new* codegen bug that phrases itself as `unsupported:` silently turns the jit off
for that function and nothing reports it. The three-string `contains` match in both
dispatchers is doing load-bearing control flow on error prose.

**And one thing that is not a restriction but should be.** More than eight arguments --
counting descriptors -- aborts instead of falling back. See
[issues.md](../issues.md#user-content-a-function-of-more-than-eight-parameters-aborts-under---jit),
and note that lifting the shape gate enlarges the set of programs that reach it, since a
generic's declared shapes count towards the eight.

### The metrics cannot answer the question they exist for

`MetricsCollector::record_call` is handed a `start_time` captured when the dispatcher was
entered. For a jitted call the elapsed time spans the call, which is right. For an
interpreted call the dispatcher returns `NotHandled` and the interpreter runs the function
*after* the measurement is taken, so the recorded "time" is the dispatcher's own
bookkeeping. Interpreted and jitted times in the same collector are not comparable, and
`avg_call_time_ns` mixes them.

`record_jit_compile` is also called on every jit dispatch where the state is already
`Compiled`, not once per compilation, so `jit_compiled_count` and `total_jit_code_size`
count executions.

`DispatcherConfig::production()` turns metrics on. With `timing_sample_rate: 1` that is
two `Instant::now` calls and a HashMap entry per dispatched call, in the configuration
named for production.

## State of the inliner

### The clone is the whole regression

```rust
fn get_optimized_function(&self, func: dispatch::FuncIdentity) -> Option<IrCodeUnit> {
    let dispatcher = self.call_dispatcher.borrow();
    dispatcher.as_ref().and_then(|d| d.get_optimized_function(func).cloned())
}
```

`execute_call` calls this on every call. When the callee has an inlined body, `.cloned()`
deep-copies the entire `IrCodeUnit` -- every block, every instruction, the value types, the
symbol table -- to escape the `RefCell` borrow, and then throws it away when the call
returns. The clone happens once per **entry**, so its cost is divided by the work done per
entry, which is exactly the pattern the measurements show: a 2.8x regression on `fib`,
which does nothing per entry, and a 1.5x win on `stdlib_list`, where each entry runs a
few hundred loop iterations.

The profile confirms it directly: `Vec<IrBlock>::clone`, `Instruction::to_vec` and
`Option::cloned` are 6.4% of cycles between them and account for most of a further 9.7%
of allocator traffic.

This is the first thing to fix and it is not a design question. An `Rc<IrCodeUnit>` in the
inliner's map and an `Rc` out of the trait removes it.

### The inlined body never reaches the jit

`OptimizingDispatcher::dispatch_call` has the comment

> Note: We always pass the original function to JIT. The inliner modifies the caller, not
> the callee, so the callee IR is the same either way.

and `try_jit_execution` is handed the `callee` that `execute_instruction` resolved out of
the registry -- the original. The inlined caller is only picked up in `execute_call`, which
runs after dispatch has already declined. So the caller with the inlined body is compiled
from its un-inlined IR whenever it is itself called, and `is_inlined` -- which is computed
from whether the *callee being entered* happens to have an inlined version of itself
stored -- only labels the metric `InlinedJit`. The combination is a label.

That is the whole premise of `plan-fancy-jit.md`: "inline hot call sites -> JIT compile the
optimized IR". It is the one part of that plan that was not built, and the measurements
show it: `jit` and `jit_inline` agree to within noise everywhere.

### No cost model at all

`DynamicInliner` inlines when a call site's count crosses a threshold. There is no size
limit, no code-growth budget, no depth limit, and -- unlike the directive-driven path in
`resolve_directives`, which does check -- **no recursion check**. A self-recursive hot
function inlines into itself; `remap_call_site` gives the moved call sites fresh ids, those
ids are counted separately, and they cross the threshold in turn. The `recursive` workload
is measurably worse than the interpreter partly for this reason.

Current practice budgets in units the compiler can see: Truffle counts Graal nodes after
partial evaluation and holds two budgets, an exploration budget and an inlining budget;
LLVM and HotSpot carry size thresholds with separate, larger ones for call sites known to
be hot. Any of those is more than nothing, which is what is here.

### Per-call bookkeeping on a path that can no longer do anything

`record_call` inserts into a `HashMap<CallSiteKey, CallSiteState>` on every dispatched
call, and keeps doing so after the site has been inlined and `inlined` is `true`. The key
hashes a `FuncIdentity` and a `CallSiteId` with SipHash. Together with the `FuncIdentity`
lookup in `get_optimized_function` and the `IrType` lookups in `get_or_create`, hashing is
~13.8% of the `inline` configuration.

### Two keys for one concept

`FuncIdentity` in the interpreter and `FunctionKey` in the jit crate are the same idea
written twice, both hashed per call, both with the default hasher. `FunctionKey::of` and
`FuncIdentity::of` have the same body modulo the type.

### The other inliner is not wired to anything

`parse_inline_directives`, `inline_module`, `inline_cross_module` and the
`inline-directives` worldfile section are reached only from `ir_inline_tests`, which
compares printed IR before against after and never runs the result. 21 fixtures, none
generic.

## What current practice does

Enough has changed since this code was written that two of its assumptions are now out of
date.

**Cranelift has an inliner.** It landed in Wasmtime 36 (cranelift 0.123) and the tree is
on **0.135**, so it is already available:
`cranelift_codegen::Context::inline(impl Inline)` exists in
`~/.cargo/registry/.../cranelift-codegen-0.135.1/src/inline.rs`. It is deliberately
"inlining as a library": Cranelift provides the mechanics and the embedder provides an
`Inline` impl returning `InlineCommand::KeepCall` or `InlineCommand::Inline { callee,
visit_callee }` per direct call. Cranelift's own heuristics are described by its author as
"extremely naive" and size-only, so the policy is ours to write either way -- but the
*transformation*, and the fact that everything downstream of it (the egraph optimizer, GVN,
LICM) then sees through the call, comes free.
([Fitzgerald](https://fitzgen.com/2025/11/19/inliner.html),
[Bytecode Alliance](https://bytecodealliance.org/articles/inliner))

**Tier-up is driven by back edges, not just calls, and uses OSR.** JavaScriptCore's
baseline tier triggers at roughly 6 invocations *or* 100 loop iterations, or a combination;
the low-level interpreter OSRs into the jit when stuck in a loop and relinks every caller
to the compiled entry. V8's Maglev tiers up at ~500 invocations with stable feedback and
resets the counter when feedback changes. HotSpot's non-tiered `CompileThreshold` default
is 10000, with separate per-tier thresholds when tiered.
([WebKit](https://docs.webkit.org/Deep%20Dive/JSC/JavaScriptCore.html),
[Deegen](https://arxiv.org/pdf/2411.11469))

**A cheap first tier is the standard answer to compile latency, and the gap is large.**
Copy-and-patch generates code two orders of magnitude faster than LLVM `-O0` and 4.9-6.5x
faster than Wasmtime's Liftoff, while producing code 14% faster than LLVM `-O0` and only a
few percent off Cranelift's. Cranelift itself sits about 10x faster to compile than LLVM
but roughly 100x slower than copy-and-patch. The motivating numbers for tiering at all are
stark: TurboFan needs 49 CPU seconds for the AutoCAD Web App's Wasm module.
([Copy-and-Patch](https://arxiv.org/pdf/2011.13127),
[TPDE](https://arxiv.org/pdf/2505.22610))

**Specialization beats dispatch tricks in interpreters.** CPython's PEP 659 reports 10-60%
from quickening and specialization, with most of it from specialization -- attribute
lookup, globals, calls -- and "a small, but useful, fraction" from superinstructions.
Brunthaler's inline-caching-without-dynamic-translation work reports up to 1.71x.
([PEP 659](https://peps.python.org/pep-0659/),
[Brunthaler](https://publications.sba-research.org/publications/ecoop10.pdf))

**And a caution worth taking seriously.** CPython spent roughly 30 months on a
copy-and-patch jit and, as of mid-2025, its own core developers reported it ranging from
slower than the interpreter to about equal, with results that flip depending on which C
compiler built the interpreter. Two of their stated measurement traps apply directly here:
comparing against the wrong baseline, and not controlling the build. This is the fate the
broken `jit.rs` benchmark was quietly arranging.
([devclass](https://devclass.com/2025/07/09/despite-30-months-work-core-developer-says-pythons-jit-compiler-is-often-slower-than-the-interpreter/),
[Ken Jin](https://fidget-spinner.github.io/posts/faster-jit-plan.html))

## Is inlining the interpreter alone worthwhile?

**Before Stage 1: no.** 0.35x on recursion, 0.70x on a call chain, 0.77x on a small
callee, 1.50x on the one stdlib-shaped workload. Net harmful.

**After Stage 1: yes, but modestly, and still only with a cost model.** 1.18x on
recursion, 1.12x on a call chain, 1.47x on stdlib-shaped code, and still 0.85x on a
caller entered 200,000 times that does one addition per entry. So it pays on three of
four, and the one it does not pay on is the shape a size budget would decline anyway.

The ceiling was, and remains, arithmetic. In the `interp` profile of `call_chain`, the
per-call machinery inlining removes -- `call_in_context_with_shapes`, `Frame::new`,
`prepare_call_args`, and the share of `malloc`/`calloc` that is frame allocation -- is
somewhere around 20-25% of cycles. That is worth having and it is not transformative, and
it is a fraction of what the jit gives on the same code.

The reason to do it anyway is that it is the only optimization that reaches code the jit
cannot compile, and it is the input the jit most wants. So the honest framing is: inlining
in the interpreter is worth doing *as the feeder for the jit* and as a fallback for
shape-declaring generics, not as a standalone interpreter optimization.

If the goal is purely to make the interpreter faster, the profile says to spend the effort
elsewhere first, and said so loudly. Two of the three items it named are now done and were
indeed larger than inlining:

- ~~`IrTyDescTable::get_or_create` plus `IrType` hashing, ~7% of the `interp` profile and
  ~26% of the `jit` profile~~ -- the hashing is gone; the remaining lookups in the jit's
  trampoline are Stage 3.
- ~~`IrLayout::compute`, recomputed per call for a layout that depends only on the code
  unit~~ -- cached.
- **The allocator is still ~11%, most of it frames.** `Frame::new` allocates five to seven
  `Vec`s per call. A frame arena or reuse pool removes it, and it is now the largest single
  item in the interpreter profile.

That last one is larger than inlining, cheaper, and carries no code-growth risk. It is
also what current practice would do first: specialize and quicken the interpreter before
adding a compiler tier.

## Is inlining either AOT worthwhile?

**The C backend: it already has an inliner and is being told not to use it.** `cc` is
invoked with `-O0` in `link_sources_to_path`. Passing `-O2` made `loop_arith` 8.4x faster,
`chain` 150x faster, and moved the backend from 5.8x *slower* than cranelift AOT to 1.46x
faster. There is no reason to write an inliner for this path; there is a reason to stop
suppressing the one that ships with the host compiler. This is one string.

Two things to check before doing it. The generated C is emitted as several files
(`sources.files`), so cross-function inlining only reaches within a translation unit unless
`-flto` is added; and `-O2` will expose any undefined behaviour the emitted C has been
getting away with, which is the real work in this item and the reason it is not simply a
one-line commit. Both AOT backends are covered by the four-way differential suite in
`crates/datalove-cli/tests/fixtures/backend/`, which is the right place to find out.

**The cranelift backend: yes, and through `Context::inline` rather than the IR inliner.**
It runs at `opt_level = "speed"`, so it already has the egraph optimizer, GVN and LICM --
all of which are blocked at call boundaries and all of which start working once a call is
inlined. Inlining at the CLIF level is also strictly easier than at the datalove IR level
for this codebase, because by then the descriptors are ordinary pointer arguments and the
whole `descriptor_shapes` renumbering problem that makes `inline_call_site` refuse
shape-declaring callees does not exist. An AOT build has the whole call graph, so the
policy can be a simple size-and-call-count rule.

**And the same hook fixes the jit's worst problem.** If the jit inlines at the CLIF level,
the inlined call is not a trampoline call any more -- it is gone. That addresses the ~26%
hashing and 9% `__jit_dispatch_call` in one move, without touching the IR-level inliner at
all.

So: the datalove-IR inliner's remaining justification is the interpreter. For both
compiled backends, `Context::inline` is the better tool, and it is already in the
dependency tree.

## The plan

Staged so that each stage is independently worth landing and has a measurement that says
whether it was. `cargo bench -p datalove-bench --bench jit` is the measurement unless
stated otherwise; record the table before and after.

### Stage 0 -- make the thing measurable and reachable

1. The harness in `crates/datalove-bench/benches/jit.rs` is in place. Add the three
   workloads it cannot express yet: a loop in a function called *once* (which no counter
   currently sees), a generic that builds a collection (which nothing compiles), and a
   REPL-shaped sequence of small units.
2. Expose the `OptimizingDispatcher` from the CLI: `--jit` should take a mode, and
   `--inline` should exist. Today the only reachable dispatcher is a bare `JitEngine` and
   the inliner cannot be run by a user at all, which is why it has never been measured.
3. Register native symbols with the jit inside an `OptimizingDispatcher`. The CLI's
   `register_natives` downcasts to `JitEngine`, so with an `OptimizingDispatcher` no symbol
   is registered and the first `sys/std` call aborts the process on
   `native symbol not registered`. The harness works around this; the CLI cannot.
4. Fix the metrics so interpreted and jitted time are comparable, or stop reporting
   `avg_call_time_ns`. Turn metrics **off** in `production()`.

**Measures:** nothing yet. This is what makes the rest measurable.

### Stage 1 -- stop recomputing and stop copying -- **done**

1. **Cache the frame layout per body.** `IrLayout::compute` ran at every call, and it is a
   function of the unit's value and slot types alone: four allocations and a
   `get_or_create` per value and per slot, recomputing an identical answer. `LayoutCache`
   in `layout.rs` keys it by the same `FuncIdentity` the dispatcher uses and validates the
   entry against `value_count`, since the inliner is the only thing that replaces a body
   and only ever adds values. `Frame` holds an `Rc<IrLayout>`.
2. **`Rc<IrCodeUnit>` out of `CallDispatcher::get_optimized_function`,** so entering a
   function with an inlined body is a reference count rather than a deep copy of every
   block.
3. **`FxHashMap` for the hot maps** -- the `IrTyDescTable` cache, `CallSiteKey`,
   `inlined_functions`, `JitEngine::states`. An `IrType` is a tree and SipHash walked all
   of it.

Not done, and still worth doing: stop tracking a call site once it has been inlined, and
merge `FunctionKey` into `FuncIdentity`.

**Measured.** See [What Stage 1 did](#user-content-what-stage-1-did).

### Stage 2 -- stop allocating a frame per call -- **done**

1. **A frame pool.** Function frames are strictly nested, so `FramePool` holds one per
   level of call depth and `reset` reuses the data block and every side table's capacity.
2. **Parameter descriptors in the cached layout.** The last `get_or_create` on the
   interpreter's call path, once per owned parameter per call, for a type the signature
   fixes.

**Measured.** See [What Stage 2 did](#user-content-what-stage-2-did). The allocator left
the profile entirely.

### Stage 2b -- give the inliner a cost model

Renumbered, and now lower priority than it looked: with the call path this much cheaper
the inliner is break-even under the interpreter, so this is about making it safe to feed
the jit rather than about winning anything on its own.

1. Refuse self-recursion, as `resolve_directives` already does.
2. A size budget: a callee body size limit, a caller growth cap, and a global growth cap.
   Count IR instructions -- the analogue of Truffle counting Graal nodes -- rather than
   source size.
3. A depth limit on transitive inlining, since moved call sites currently get fresh ids
   and become candidates with nothing stopping them.

**Measures:** `recursive` must not regress. `call_chain` should improve, since a
three-deep chain of one-line functions is what a budget should say yes to.

### Stage 3 -- the descriptor lookups -- **done**

The interpreter's half went with Stages 1 and 2: the layout is cached per body and carries
the parameter descriptors, so nothing looks a type up per call. The jit's half is the
trampoline reading that same layout rather than probing the descriptor table once per
argument.

Not done: the three per-call hash probes still inside `__jit_dispatch_call`, which want the
stub to carry a pointer to a resolved callee record rather than a key to look one up by.

**Measured.** See [What Stage 3 did](#user-content-what-stage-3-did).

### Stage 4 -- count back edges and OSR

1. A back-edge counter, so a loop is visible at all. Current practice is a combined
   rule -- JSC uses roughly 6 invocations or 100 iterations.
2. OSR entry at loop headers, so a hot loop in a function called once is compiled and
   entered mid-loop rather than on the next call.
3. Hysteresis on the threshold so recompilation does not thrash.

**Measures:** a new workload with the loop in a function called once. Today the jit's
speedup on it is exactly 1.0x. This stage is the difference between `loop_arith`'s 86x
being available to real code and being an artifact of where the loop happened to sit.

### Stage 5 -- inline in Cranelift, for both compiled backends

Implement `cranelift_codegen::inline::Inline` over the function registry and call
`Context::inline` before `compile_as`. The policy is ours; start with size-and-hotness, and
for the jit feed it the call counts the dispatcher already keeps.

This is where the inliner and the jit actually combine, and it is a different combination
from the one `plan-fancy-jit.md` described: rather than feeding inlined datalove IR to the
jit, the jit inlines at the level where it can also delete the trampoline. Do it here
rather than fixing `try_jit_execution` to take the inlined body, because at the CLIF level
the `descriptor_shapes` renumbering problem that makes `inline_call_site` refuse
shape-declaring callees does not arise.

**Measures:** `small_callee`, `call_chain` and `recursive` under `jit`. The target is the
gap between the jit and the cranelift AOT backend on `fib(30)` -- 966ms against 410ms.

### Stage 6 -- latency, and turning it on by default

1. Move `JitEngine` construction off the startup path, or make the arena lazy. 13.6ms of
   fixed cost per process is the reason `--jit` cannot become the default for a CLI or a
   REPL.
2. A cheap first tier, or `opt_level = "none"` for the first compile with a promotion to
   `"speed"` on a second threshold. The point is to stop paying full Cranelift latency
   synchronously at the moment a function gets hot.
3. Background compilation, so the compile is not on the critical path at all. This is what
   makes a slow optimizing tier tolerable and is why V8 and Wasmtime can afford theirs.
4. Code cache accounting: dedupe stubs, reuse symbols across recompiles, and decide what
   happens when the arena fills other than a compile error.

**Measures:** the `floor` row, and a REPL-shaped workload of many small units. Also worst
case per-unit latency, not just the mean -- that is the number that decides whether this
is on by default.

### Stage 7 -- the C backend's optimization level

Pass `-O2` (and consider `-flto`, since the C is emitted as several files). Run the
four-way differential suite in `crates/datalove-cli/tests/fixtures/backend/` and fix what
`-O2` exposes. Placed last because it is the item most likely to surface undefined
behaviour in the emitted C, not because it is small -- on the measurements it is the single
largest speedup in this report.

**Measures:** 8.4x on `loop_arith`, and cranelift AOT as the baseline to beat.

### What to do next

Stages 1 to 3 are done, and with them all the work that was memoization or a type
substitution. The interpreter is 1.5-1.8x faster on call-heavy code and the jit about 2x,
and nothing left in either profile is a lookup of something already known.

What remains is a choice between three things that are not cheap, and the measurements say
to weigh them like this:

**Stage 7, `-O2` for the C backend, is still the largest measured number in this report**
-- 8.4x on `loop_arith`, and it moves that backend past the cranelift one. It is one flag
and then however much undefined behaviour `-O2` exposes in the emitted C, which is the real
work and is not estimable from here. The four-backend differential suite is the instrument.

**Stage 4, back-edge counting and OSR**, decides whether the jit's 86x on a loop is
available to real code or an artifact of where the loop happened to sit. Today a function
called once that loops ten million times is invisible to the jit. Nothing else in the plan
changes that, and it is the difference between the jit being worth turning on for some
programs and for most.

**Stage 5, `Context::inline`**, is where the inliner and the jit finally combine, and it
attacks the trampoline by deleting calls rather than making them cheaper --
`__jit_dispatch_call` is still 16.7% of the jit profile and an inlined call is not in it at
all. It also gives the cranelift AOT backend an inliner it does not have.

Going further in the *interpreter* means a contiguous frame stack instead of a pool of
separate frames, or specializing instruction dispatch. Both are larger than everything
above, and current practice says specialization is where the remaining interpreter wins
are.

If the question is instead "should the jit be on by default", the answer is that Stage 6
decides it and nothing else does. The jit's speed is not in doubt; its 13.6ms of startup
and its synchronous compile pauses are.

## Faults found on the way

Not part of the review, found while doing it, and recorded rather than fixed.

**The ownership pass panics on a clone returned out of a loop.** Reproduced:

```datalove
fun build(n: u32): [u32]
    var out: [u32] = []
    loop
      if n == 0
        ret out@
      end if
      break
    end loop
    ret []
end fun
```

panics at `crates/datalove-datafun-ownership/src/lib.rs:1871` on
`ctx.get_moved_at(*id).X()`, whose comment reads "The binding is Moved, so `mark_moved`
recorded where". That does not hold when the move is a clone-through inside a loop. Without
the `@` the same program is correctly refused with D007 and D008, so the panic is on the
path that should have accepted it. Added to [issues.md](../issues.md).

**`register_natives` in the CLI cannot see an `OptimizingDispatcher`.** It downcasts to
`JitEngine`, so any dispatcher other than a bare one leaves the jit's symbol table empty
and the first native call aborts the process from
`crates/datalove-datafun-cranelift-jit/src/compiler.rs`. Folded into Stage 0 above rather
than filed separately, since nothing can currently construct that situation from the CLI.
