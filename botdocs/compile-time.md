# Where Compile Time Goes

What a cold compile of the system library costs, phase by phase, and how to
measure it. Written October 2026, after a pass that took a script requiring
every `sys/std` module from 273M to 202M user instructions.

## Contents

- [Measuring](#user-content-measuring)
- [The breakdown](#user-content-the-breakdown)
- [Page faults](#user-content-page-faults)
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

Shares of a run, inclusive, after the fixes below:

| Phase | Share |
|---|---:|
| Parse, including lexing (5.6%) | 27% |
| Phase 5a, lowering every function | 17% |
| Phase 5d, of which inlining and `simplify` 9% | 13% |
| Typecheck | 11% |
| Ownership analysis | 10% |
| Process start, the sys packages, the script | the rest |

An empty script takes 3.7ms and 767 page faults; the library is everything
above that.

Parsing has no hot spot. Its cost is spread across the lexer, the bracer, the
token stream and the creation of a salsa tracked struct per expression
(`ExprFun::new`), at about 22MB/s. Typecheck and ownership analysis keep their
per-expression tables in `BTreeMap`s keyed by `ExprKey`, and inserting into
those is the largest item in each.

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

## What was fixed

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
- The CLI's `script` command dropped the salsa database table by table on its
  way out, 5% of the run. It forgets it instead, after dropping the
  interpreters, which check for leaks as they go.

## What is left

- Huge pages, above: the largest single item, and a decision about the
  allocator.
- Parsing: no single fix; the per-expression tracked struct is the structural
  cost.
- The analysis tables: dense per-function indices instead of `BTreeMap`s keyed
  by `ExprKey`, if their hashing and ordering requirements allow.
- Ownership analysis clones each function's `FunctionAnalysis` into lowering,
  since `LowerCtx` owns it; borrowing it would save the copy.
- Lowering on demand, which would skip most of 5a and 5d; see
  [the plan for the IR passes](plan-ir-passes.md).
