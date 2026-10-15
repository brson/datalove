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

Parsing has no hot spot; see [Parsing](#user-content-parsing). Typecheck and ownership analysis keep their
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
  allocator.
- Parsing: no single fix; see [Parsing](#user-content-parsing).
- The analysis tables: dense per-function indices instead of `BTreeMap`s keyed
  by `ExprKey`, if their hashing and ordering requirements allow.
- Ownership analysis clones each function's `FunctionAnalysis` into lowering,
  since `LowerCtx` owns it; borrowing it would save the copy.
- Lowering on demand, which would skip most of 5a and 5d; see
  [the plan for the IR passes](plan-ir-passes.md).
