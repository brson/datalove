# Where Compile Time Goes

What a cold compile of the system library costs, phase by phase, and how to
measure it. Written October 2026, after a pass that took a script requiring
every `sys/std` module from 273M to 197M user instructions, 192M after the
parser read its tokens in place, and 184M after the bracer and lexer were
reworked; see [the parser plan](plan-parser.md).

## Contents

- [Measuring](#user-content-measuring)
- [The breakdown](#user-content-the-breakdown)
- [Page faults](#user-content-page-faults)
- [Parsing](#user-content-parsing)
- [Parallel compilation](#user-content-parallel-compilation)
- [What was fixed](#user-content-what-was-fixed)
- [What is left](#user-content-what-is-left)

## Measuring

The benchmark is a script that requires every `sys/std` module and calls
nothing: about 200KB of source in 24 modules, 970 functions. Wall time on a
shared machine moves by 10% from one minute to the next, so compare
instruction counts, which do not:

```
perf stat -e instructions:u,page-faults,task-clock -r 10 \
    target/release/datalove script allstd.dfs
```

The binary is `datalove-cli`'s, so build with `-p datalove-cli`; `-p
datalove` builds a library and leaves a stale binary in place.

For a profile, build with frame pointers into a separate target directory,
since DWARF unwinding through salsa's inlined frames mostly fails:

```
RUSTFLAGS="-C force-frame-pointers=yes" CARGO_PROFILE_RELEASE_DEBUG=line-tables-only \
    cargo build --release -p datalove-cli --target-dir /tmp/relfp
perf record -F 20000 --call-graph fp -- /tmp/relfp/release/datalove script allstd.dfs
perf report --no-inline --children --sort sym
```

`--no-inline` matters: without it one function is reported once per inlined
copy and the inclusive view is unreadable.

## The breakdown

Measured October 2026 at 132.9M user instructions, after the typechecker
began unifying the types of expressions that must agree; averaged over 15
profiles, inclusive:

| Phase | Share |
|---|---:|
| Parse, including the lexer (4.6%) and bracer | 25% |
| Name resolution and building the parsed graph | 6% |
| Typecheck, of which inference 8% | 19% |
| Ownership analysis | 10% |
| Lowering every function, in two strata | 14% |
| Assembly: inlining, of which `simplify` 9% | 18% |
| CTFE registry, consts, the rest of phase 5 | 2% |
| Process start and the empty script (5.2M) | 4% |

Inference kept its per-expression types and a generic call's parameter
variables in SipHash `HashMap`s keyed by `ExprKey`, and the typechecker
recorded every name it looked up in a `BTreeSet` that only script units read.
Both are fixed, and unification no longer copies the types it compares: the
compile went from 132.9M instructions to 124.1M, and typecheck from 19% of it
to 13%.

The unifier itself -- union by rank with path compression, the algorithm
the `ena` crate implements and rustc uses -- is under 1% of the run. What
`ena` adds beyond that is snapshots, an undo log for rolling back speculative
unification, and a persistent table, which nothing here needs: inference
runs once, reports nothing, and is not tried and abandoned. So it would be a
dependency for no gain. Snapshots are the piece to borrow if inference ever
has to try something and take it back.

Of the cycles, a fifth to a quarter are the kernel's, almost all page
faults; see [Page faults](#user-content-page-faults). `malloc`, `free` and
`realloc` are 8% of the user instructions between them.

Parsing has no hot spot; see [Parsing](#user-content-parsing). Typecheck and ownership analysis kept their
per-expression tables in `BTreeMap`s keyed by `ExprKey`, and inserting into
those was the largest item in each. They are dense vectors per function now
(`BodyTable`); see [plan-plain-ast.md](plan-plain-ast.md).

## Page faults

A fifth of the run is the kernel faulting in fresh heap pages: about 4,100
faults, so 16MB touched, at a few microseconds each. They are spread across
every phase, wherever memory is first touched -- vector growth, salsa's
tables, maps -- so no one allocation site is worth chasing.

Transparent huge pages remove most of them. glibc asks for them on its heap
when told to:

```
GLIBC_TUNABLES=glibc.malloc.hugetlb=1 target/release/datalove script allstd.dfs
```

which takes the faults from 4,087 to 671 and task-clock from 29.7ms to 22.9ms,
with the same instructions. The machine's THP setting is `madvise`, so nothing
gets huge pages unless it asks. glibc reads the tunable only from the
environment at startup, so a process cannot turn it on for itself; doing it
from inside means another allocator that can be told to, or re-executing with
the variable set. Neither is done.

## Parsing

Parsing on its own is counted by the `parse_only` example in `datalove-bench`,
which parses every `sys/std` module on a fresh database and nothing else. In
October 2026 it was 43.0M instructions a pass (218 a byte, 1,204 page faults)
before expressions moved into arenas and 34.7M (176 a byte, 1,070 faults)
after; see [plan-plain-ast.md](plan-plain-ast.md). That is still a fifth of a
compile, because the rest got cheaper too, and still several times what a
direct hand-written parser needs.

What follows was written before the parser rewrite. Parsing was about a
quarter of the run, some 250 instructions per byte of
source, where a direct hand-written parser needs tens. Measured by
instructions, top down: statement parsing 18% of the run, the lexer 4.8%
before the fix below, splitting the top level into lines 3.8%, the non-ASCII
name check 1.2%, the bracer's own query 0.5%.

Two guesses about it were measured and were wrong, and are worth not making
again. A branch token carried a boxed sub-iterator, an allocation per bracket
and per copy; making it a `Copy` handle (`BranchInner`) saved 0.4% of the
run. Nested parsers kept their own span tables and appended them to their
parent's, copying an entry once per level of nesting; moving one set of
tables in and out (`Filed`) saved 0.3%. Neither allocation was where the cost
was, because `sys/std` does not nest deeply.

What is left is constant factors at every layer, none above about 3%:

- An expression was a salsa tracked struct, created with an identity hash and
  a disambiguation, and `create_expr` was 3.1% of the run. Expressions are now
  in a per-function arena, which took the whole compile from 178M instructions
  to 159M, and functions are plain data since, which took it to 151M; see
  [plan-plain-ast.md](plan-plain-ast.md).
- Every name is interned as it is read.
- Every bracket starts a sub-parser, a struct of a few hundred bytes, and
  every line one more.
- Tokens move as `TreeToken`s, and `BracerIter::next` merges four sequences
  to produce each one.

Doing much better is a different parser: one cursor over the flat token array
with each bracket's match index precomputed, and no token trees or
sub-parsers. See [the plan for it](plan-parser.md), which has where the
allocation was -- 35,726 allocations and 12MB to parse `sys/std` -- and what
building it did: 26,189 and 5.6MB, and parsing from 27% of the run to 23%,
the rest of the allocation being the AST's own.

## Parallel compilation

`DATALOVE_PARALLEL=1` runs each phase's per-module work on rayon (see
[the compiler guide](compiler-guide.md#user-content-parallel-execution)), but
on the `sys/std` script it buys little: measured October 2026 on four cores,
24.2ms wall sequential and 17.4ms parallel, 1.4x. Two things cost most of the
rest, and both have been measured rather than guessed.

**Parsing was not parallel.** Package resolution parses every module it
reaches, so that resolution and phase 1 share one parse: `extract_dependencies`
walks a worklist calling `module_import_demands`, which reaches
`parse_module_full` (see `crates/datalove-datafun/src/import_demands.rs`). It
did that on the main thread, so by the time `parse_module_graph_parallel` warmed
the parse memos they were all hits, and the largest front-end phase ran
sequentially. The `parallel` bench did not see this, because it calls the
phase-1 function directly. Under `DATALOVE_PARALLEL` the pipeline now calls
`extract_dependencies_with_mode`
(`crates/datalove-datafun/src/incremental.rs`), which runs the same walk
outside any query, one frontier at a time, with each frontier's parses on
rayon. The tracked walk that follows finds every parse already done. That took
the parallel run from 16.9ms to 14.3ms. A script that requires one module
costs what it did, 0.7% more instructions, so a narrowed set of roots still
parses only what it reaches.

**The workers spend as much time in the kernel as in the compiler.** A
per-thread profile of a parallel run put the rayon workers at about half
compiler code, 15% page faults and 15% `mprotect`, with the rest scheduling
and spinning. The `mprotect` calls come from glibc growing each thread's
private arena a little at a time. Each one takes the process's `mmap_lock` for
writing, which stalls page faults on every other thread (`rwsem_spin_on_owner`
in the profile). The huge pages of [Page faults](#user-content-page-faults)
remove almost all of it, and they help the parallel run more than the
sequential one:

| | Wall | Task-clock |
|---|---:|---:|
| Sequential | 23.2ms | 21.4ms |
| Parallel | 17.1ms | 28.5ms |
| Sequential, `hugetlb=1` | 17.3ms | 16.1ms |
| Parallel, `hugetlb=1` | 12.4ms | 20.4ms |
| Parallel, `hugetlb=1`, parallel parse | 10.9ms | 22.8ms |

One shared arena (`glibc.malloc.arena_max=1`) is worse than either: 27.7ms,
on malloc's lock.

With both fixed, what remains on the main thread is phase 5's sequential
pieces (`ctfe_module_registry`, `add_native_rider_units`, const inlining and
evaluation, `compute_func_id_map`), the phase-1 aggregator with
`rider_interfaces`, and building the database, a few percent each, against a
fixed 3.6ms for a script that requires nothing. The other phases' aggregators
are under 1%.

The barriers between phases are not the problem. A prototype that ran
resolution, typecheck and ownership as one task per module, with no wait
between them, measured the same as running them phase by phase, 10.6ms against
10.5ms with huge pages (measured against the parallel-parse prototype), at the same instruction count, so it really did reuse
the memos. That is the shape ty and rust-analyzer use: start the last query per
unit and let salsa pull the earlier ones in, blocking when two threads want
the same memo. It is not worth restructuring for at 24 modules on four cores.
It might be on a bigger world, where one slow module holds up every phase
boundary.

Upstream salsa has no helper for any of this. `salsa::par_map` and
`salsa::join` were removed in 0.25 (salsa-rs/salsa#1013), because work they
ran inside a query was not tracked as that query's dependency. What upstream
recommends is what this compiler does: clone the database for each thread,
outside any query. ty's version is `crates/ty_project/src/parallel.rs` in the
ruff repository. It clones once per rayon split rather than once per item
(`map_with` over a `Clone` wrapper around `dyn_clone`), and returns owned
values from the workers, not branded ones.

`cargo run --release -p datalove-bench --example par_phases` times each phase
both ways on a fresh database. Parsing is in its first row, package resolution
with the dependency walk; its parse row is the warm-up described above. In
October 2026, after the walk went parallel, best of 40 on four threads:

| Phase | Sequential | Parallel | Speedup |
|---|---:|---:|---:|
| Dependency walk, with parsing | 5.18ms | 1.94ms | 2.7x |
| Phase 1 warm-up and aggregator | 0.57ms | 0.61ms | 0.9x |
| Name resolution | 0.54ms | 0.24ms | 2.3x |
| Typecheck | 2.13ms | 0.72ms | 3.0x |
| Ownership | 1.76ms | 0.58ms | 3.0x |
| Phase 5 | 7.59ms | 3.63ms | 2.1x |
| Total | 17.77ms | 7.72ms | 2.3x |

## What was fixed

- `reachable_func_ids` copied, for every module, every entry of the world's
  function-id table that module could reach, names and all, and interned the
  copy: 4% of the `sys/std` compile and quadratic in the module count. It
  holds a handle per reachable module now (`ModuleFuncIds`, interned once per
  module): 128.0M to 123.4M instructions, and a cold 512-module compile in
  `recompile_profile` from 4.25G to 2.19G.

- Maps keyed by dense ids or hashed with SipHash, October 2026, from 140.6M
  instructions to 128.0M: ownership's per-binding state as vectors indexed by
  `BindingId` (`BindingMap`, 2.9%), the typechecker's, lowering's and
  ownership's name-keyed maps on FxHash (4.0%), block scopes undoing their
  bindings rather than copying the map (0.4%), the IR passes' per-value tables
  as vectors indexed by `ValueId` (`ValueMap`, `ValueSet`) and the inliner on
  FxHash (1.3%), and the function-id lookup on FxHash (0.8%). What is left of
  map and hashing cost is spread thin, much of it salsa hashing memo values.

- `compute_func_id_map` checked each rider function against every entry so
  far, to skip a rider required by several modules: quadratic, and 5.7% of the
  run. It now skips a rider it has seen.
- `lower_module_functions` and `lower_module` each built a `HashMap` of every
  function the module can reach, cloning every name, once per module per
  stratum; `lower_module` built its for a path it rarely takes. Lowering now
  searches the sorted `ReachableFuncIds` entries (the `FuncIds` trait in the
  lower crate).
- The same two built `String`-keyed maps of the exports' parameter and return
  types; they look signatures up by interned name now (`export_signatures`).
- `simplify` and dead code elimination hashed value and slot ids with SipHash;
  they use `FxHashMap` and `FxHashSet`.
- The lexer stepped through words and whitespace a `peek` and an `eat_char`
  per character, with the bounds and overflow checks each makes; it scans
  bytes now, decoding only where one is not ASCII. And the non-ASCII name
  check, a pass over every token, is skipped for a source that is all ASCII.
  Together 1.9% of the run.
- The CLI's `script` command dropped the salsa database table by table on its
  way out, 5% of the run. It forgets it instead, after dropping the
  interpreters, which check for leaks as they go.

## What is left

- Huge pages, above: the largest single item, and a decision about the
  allocator. It matters more under `DATALOVE_PARALLEL`; see
  [Parallel compilation](#user-content-parallel-compilation).
- Parsing: no single fix; see [Parsing](#user-content-parsing).
- Ownership analysis clones each function's `FunctionAnalysis` into lowering,
  since `LowerCtx` owns it; borrowing it would save the copy.
- Lowering on demand, which would skip most of 5a and 5d; see
  [the plan for the IR passes](plan-ir-passes.md).
