# Testing the Engines Against One Reference

A plan for how every execution engine is tested: the IR walker as the one
reference, every other engine compared against it in the same process, one
corpus and one runner, and the bytecode on by default.

## Contents

- [Where things stand](#user-content-where-things-stand)
- [The design](#user-content-the-design)
- [Order of work](#user-content-order-of-work)
- [What goes](#user-content-what-goes)
- [As built](#user-content-as-built)

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
| C AOT | the same through the C backend |

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
| default | every crate, every engine |
| slow | property tests; wasm check; sys riders built; a fixed-seed worldgen run |
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

## As built

October 2026.

**`engine_tests`** runs `fixtures/engines/`: the 420 programs of `interp/`, and
the 523 of `dual/`, `aot/`, `module_interp/`, `interp_constlet/` and
`specialize_differential/` that were not byte-for-byte copies of another,
prefixed with where they came from. A `module_interp/` program gained a
fragment that requires its `main` module and debuglogs `main()`. The engines
are the bytecode, the JIT, the chaos dispatcher (on the bytecode), Cranelift
AOT and C AOT, and two that compile differently -- `noconst`, const inlining
off, and `nospec`, specialization off -- held to the reference's outputs and
debug logs only, and only for a program that compiled without error. The run
prints how many programs each engine took: of 972, the AOT backends take
about 700. A fixture opts out of an engine with a `// engines: -c` comment
before its first section, saying why; one does. Under Miri only the
engines that generate no code run.

**C AOT runs in the default job**, not only the slow one: over the whole
corpus it costs about as much again as the rest, which is cheap next to a
backend that drifts. Its first run found a fault no suite had reached: a table's
row descriptor left out of the file that builds one.

**`std_engine_tests`**, the old `std_all_tests`, is the standard library's
runner, with the IR walker named as its reference and the bytecode added.
It stays separate because its programs are scripts against the real `sys/`
library, built once per worker, rather than worldfiles.

**The expected files still carry the IR.** The design moves IR snapshots to
the lowering suites; they are still in the reference's rendering, so a
lowering change re-blesses the engine corpus as it did the old suites.

**CI is four jobs**, `just test-ci` the same four: default, index-64,
parallel, and slow, which is the `slow_tests` property and exhaustive tests,
the wasm build, the suite against sys riders built from source, and
`just test-worldgen` on a fixed seed, so that a failure is a change rather
than a draw; another seed is a command-line argument away. Its first run,
under leak checking, found const inlining's dead code pass removing the only
instruction that consumed a value, and two faults in the generator.

