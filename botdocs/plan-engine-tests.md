# Testing the Engines Against One Reference

A plan for how every execution engine is tested: the IR walker as the one
reference, every other engine compared against it in the same process, one
corpus and one runner, and the bytecode on by default.

## Contents

- [Where things stand](#user-content-where-things-stand)
- [The design](#user-content-the-design)
- [Order of work](#user-content-order-of-work)
- [What goes](#user-content-what-goes)

## Where things stand

Surveyed in October 2026.

**Two oracles, inconsistently applied.** Some suites compare a backend with
the IR walker at run time -- `dual_tests` (Cranelift AOT), `c_dual_tests` (C
AOT), `std_all_tests` (JIT, both AOTs), the chaos dispatcher suite, the cli's
`backend_tests`. Others compare against checked-in expected files --
`interp_tests`, `module_interp_tests`, `interp_constlet_tests`,
`interp_jit_tests`, `aot_tests`, `std_tests`, and the whole of `test-bc`. The
bytecode is never compared with the IR walker in one process: under
`DATALOVE_INTERP=bc` both sides of every differential suite are the bytecode.

**Expected files duplicated per backend.** The 394 `.c.out.expected` files are
byte-identical to their `.out.expected` twins. `c_dual_tests` runs only under
`slow_tests`, so its copies go stale whenever lowering changes; sixteen commits
since June have re-blessed them.

**Configurations standing in for test structure.** `test-bc` is a CI job to set
one environment variable. `test-slow` reruns all of `datalove-datafun` to add
one binary. `test-sys-riders` is in `test-ci` and not in CI.
`test-sanitizers-all` names recipes that do not exist, and `test-miri-interp`
stops in the harness's crossbeam before reaching the interpreter.

**Overlapping suites.** `std_tests` is a subset of `std_all_tests`;
`interp_tests` of `interp_jit_tests`, the two overwriting each other's
`.out.actual`. The tuned dispatcher suite seldom reaches its thresholds. The
specialization suite fails only because its expected files say `PASS`.

**Corpora tied to one or two engines.** `interp/` (420 worldfiles), `dual/`
(394), `std_tests/` (165), `aot/` (76), `module_interp/` (67),
`specialize_differential/` (29), `interp_constlet/` (15): most programs run on
one engine or two.

**Gaps.** Worldgen fuzzing never runs in CI. The dynamic inliner has no direct
test. The REPL engine tests and the JIT crate's unit tests are outside
`test-bc`.

## The design

**One reference.** The IR walker -- with its debug liveness checks, the checked
engine -- is the only engine whose results are compared with expected files.
An expected file records the reference's behavior and the front end's
(typecheck, ownership, IR); never one engine's copy of another's.

**Every other engine compared with it, in process.** For each program the
runner runs the reference, checks it against the expected file, then runs each
engine that applies and compares what it observes -- each script unit's output
and debug log -- with the reference's. A new engine adds no expected files.

The engines:

| engine | how |
|---|---|
| IR walker | the reference |
| bytecode | `Engine::Bytecode` on the executor |
| JIT | a `JitEngine` compiling on first call |
| chaos | an `OptimizingDispatcher` with a chaos config seeded from the fixture; asserting the JIT or the inliner engaged somewhere in the run |
| Cranelift AOT | a single-fragment program compiled, linked and run |
| C AOT | the same through the C backend (in the slow job) |

The engine is chosen by the runner, not by an environment variable, so the
reference can never silently be something else. An engine applies to a
fixture unless the fixture's shape rules it out (the AOT backends take exactly
one script fragment) or the fixture opts out by a header line, for what a
backend does not support yet.

**One corpus, one runner.** The execution corpora merge into one directory, read
by one harness. Lowered-IR snapshots belong to the lowering suites, not the
execution corpus.

**The bytecode by default.** `IrInterpreter` defaults to the bytecode;
`DATALOVE_INTERP=ir` forces the IR walker, for debugging. The runner sets each
engine explicitly. `test-bc` goes: the runner always compares the bytecode
with the IR walker.

**Four CI configurations.**

| job | runs |
|---|---|
| default | every crate; the runner's engines but C AOT |
| slow | the runner with C AOT; property tests; wasm check; sys riders built; a fixed-seed worldgen run |
| index-64 | default under `index-64` |
| parallel | default under `DATALOVE_PARALLEL=1` |

## Order of work

1. **The runner** over the existing corpora, beside the current suites, with
   the engine matrix and in-process comparison. Confirm it catches what they
   catch.
2. **The bytecode by default**, and `test-bc` gone.
3. **The corpora merged and the superseded suites deleted**, one at a time.
4. **CI rebalanced** to the four jobs, with worldgen in the slow one.

## What goes

`interp_tests`, `interp_jit_tests`, `dual_tests`, `c_dual_tests`, `aot_tests`,
`std_tests`, `module_interp_tests`, `interp_constlet_tests`, the tuned
dispatcher suite, and the chaos suite as a suite of its own; the 394
`.c.out.expected` files and the two `.disabled` expected files; the untracked
`interp3/`, `module_interp3/` and `analysis/` directories; the `test-bc`,
broken sanitizer and broken Miri recipes. The cli's `backend_tests` stays, for
the command line itself.
