# Reusing Compilation

Compiling `sys/std` is the largest cost in a datalove invocation and the largest cost in the
test suite, and it produces the same answer every time. This is why, and what to do about it
for the two cases that want different answers: a batch compiler that should not recompile
yesterday's library, and a long-lived process that should compile it once and keep it.

Supersedes an earlier version of this plan that proposed a leaked, thread-local cache of the
compiled world. That was a workaround for not having understood why the work repeats, and
the answer turns out to be somewhere else entirely.

## Contents

- [What it costs](#user-content-what-it-costs)
- [Why it is recompiled](#user-content-why-it-is-recompiled)
- [What salsa says about threads](#user-content-what-salsa-says-about-threads)
- [Compilation and execution are not under the same constraint](#user-content-compilation-and-execution-are-not-under-the-same-constraint)
- [Strategy A, a batch compiler: persist](#user-content-strategy-a-a-batch-compiler-persist)
- [Strategy B, a repl or a test run: keep the world](#user-content-strategy-b-a-repl-or-a-test-run-keep-the-world)
- [What to do](#user-content-what-to-do)

## What it costs

**Per invocation**, release: the process starts in 2.7ms, a trivial script with `--no-sys`
finishes in 3.6ms, the same script with the library takes 84.8ms. So the library is **81ms**
and everything else about a small script is under a millisecond. `botdocs/learn.dfs` -- 773
lines, most of the language -- adds 7.6ms on top of that floor.

**In the test suite**, debug: `std_all_tests` compiles the library **572 times for 534
CPU-seconds**, in a suite that runs in 258. By comparison `build_and_load_riders` is 286 calls
and 0.9 seconds. The interp and dispatch suites build their world from a worldfile's own
module sections and never load `sys`, which is why 409 fixtures cost a second and 143 cost
four minutes.

## Why it is recompiled

Not for the reason it looks like. Measured four ways, debug, compiling `sys/std`:

| | |
|---|---|
| fresh database, fresh pipeline | 756ms |
| **same** database, fresh pipeline | 766ms |
| **cloned** database, fresh pipeline | 757ms |
| same database, **same pipeline**, second compile | **84ms** |

So the database is not the unit of reuse, and cloning it buys nothing at all. The unit is the
`ModuleCompilationPipeline`, which owns the `Source` salsa inputs that every tracked query is
keyed on. `descriptor.to_pipeline(db)` makes new inputs, so everything downstream misses.
Given the same pipeline, salsa does exactly what it is for: **9x**, and what is left is the
untracked glue in `compile_impl` that walks the graph and builds registries.

**The 84ms has since been worked on, and the numbers above are the state before that.**
Phase 5a is memoized per module, the registries share their IR rather than
copying it, and the aggregators hold handles rather than materialized copies.
On a 32-module synthetic world, release, an unchanged recompile went from
15.3ms to 1.4ms and a one-module edit from 15.5ms to 3.6ms; `benches/incremental.rs`
is the measurement. What is left of the floor is still the untracked glue, and
most of it is now in `lower_module_graph_with_evaluator` -- see the phase 5
section of compiler-guide.md for what stands in the way of tracking the rest.

One correction to something written down elsewhere: `ModuleId` is `#[salsa::interned]`, not
`#[salsa::input]`, so the same path gives the same id and interning is not the problem. It is
the `Source` inputs that a new pipeline creates.

This means no cache is needed for the in-process case. Keeping the pipeline is enough, and a
pipeline is an ordinary owned struct.

## What salsa says about threads

`Storage::clone` keeps the `Arc<Zalsa>` -- every memo, interned value and tracked struct --
and makes a fresh `ZalsaLocal`, which is the per-thread query stack:

```rust
impl<Db> Clone for Storage<Db> {
    fn clone(&self) -> Self {
        Self { handle: self.handle.clone(), zalsa_local: ZalsaLocal::new() }
    }
}
```

So the answer to whether a database must be thread-local is: **the handle must be, what it
points at need not be**. `Database` is `Send` and not `Sync` by design, and `db.clone()` is
the supported way to give another thread a handle onto the same storage. That is a model, not
an obstacle to work around with a `thread_local`.

Note what the table above already says, though: a clone alone buys nothing here, because the
cost is in the inputs rather than in the storage. Cloning matters for handing *an already
warm* pipeline's database to another thread, not for getting warm in the first place.

## Compilation and execution are not under the same constraint

`IrCodeUnit` is `Send + Sync`, and so is `Arc<ModuleFunctionRegistry>`. Execution touches
salsa not at all: a `ScriptExecutor` owns a `Runtime` and reads IR that is plain owned data.

So the shape that fits both halves is a split:

- **Compilation** is single-threaded per handle, because the query stack is. One owner holds
  the database and the pipeline and hands out owned `IrCodeUnit`s.
- **Execution** is free. N executors, each with its own `Runtime`, running that IR in parallel.

For the **test suite** that means compiling a fixture once and running the four backends on it
in parallel, rather than compiling the library four times. For a **batch compiler** it means a
compile phase and a run phase, which it already wants for other reasons. For the **repl** it
means the session owns the compiler and each prompt is a compile against a warm pipeline plus
a run.

## Strategy A, a batch compiler: persist

Salsa 0.28 has this built in, behind its `persistence` feature:

```rust
impl dyn Database {
    pub fn as_serialize(&mut self) -> impl serde::Serialize + '_;
    pub fn deserialize<'db, D: serde::Deserializer<'db>>(&mut self, d: D) -> Result<(), D::Error>;
}
```

It serializes the runtime and every ingredient that opts in, ordering structs before tracked
functions so that a memo's input exists by the time the memo is read. The artifact is the memo
table: a later process deserializes it and every query that was answered is answered again,
including the ones our own IR is downstream of.

What it needs:

- The `persistence` feature turned on. It is not today; the defaults are `accumulator`,
  `inventory`, `macros`, `rayon`, `salsa_unstable`.
- A decision about which ingredients opt in.
- **A fingerprint.** A serialized memo table is valid only for the exact queries that produced
  it, so the artifact has to carry the compiler's own version and a hash of the library
  sources, and a mismatch has to mean "recompile" rather than "trust it".
- To be optional, as asked: with no artifact present, behaviour is exactly what it is now.

**The narrower alternative** is to serialize our own output rather than salsa's memos.
`ModuleFunctionRegistry` is a map of `IrCodeUnit`, all of it `Serialize`, and
`ir_serial_tests` already round-trips IR through RON and checks that the deserialized form
executes identically on the interpreter *and* the AOT backend. What that does not carry is
what the **typechecker** needs: a script importing `sys/std` is checked against the library's
exported signatures, which are salsa tracked values and not in the IR. So this route needs a
module-interface artifact designed, which the salsa route gets for nothing. Its advantage is
that the format is ours and does not move when salsa does.

**Measure before choosing.** Deserializing is not free, and if it costs 40ms of the 81ms then
the win is half of what it looks like. That number is cheap to get once the feature is on, and
it decides between the two routes as much as anything else does.

## Strategy B, a repl or a test run: keep the world

Nothing needs caching here, which is the point of the table above. A value owns the database
and the pipeline, and scripts are compiled against it:

```rust
pub struct CompiledWorld {
    db: Database,
    pipeline: ModuleCompilationPipeline,
}

impl CompiledWorld {
    pub fn new(descriptor: &WorkspaceDescriptor) -> Self;
    /// Compile a script against the modules already compiled here.
    pub fn compile_script(&mut self, source: &str) -> ScriptCompilationResult;
}
```

Owned, dropped when its owner drops. No leak and no thread-local, because the lifetime problem
that drove both -- `CompiledModules<'db>` borrowing the database -- only exists if the borrow
has to escape. It does not: the world hands out **IR**, which is owned, rather than lending a
view of itself.

What blocks this today is plumbing rather than lifetimes.
`ExampleTestRunner::new(dir, analyze_file)` takes a bare `fn(&Path) -> Result<String, String>`,
so a fixture cannot be handed anything and every fixture necessarily builds its own world.
A runner that takes a closure, or a context parameter, lets `main` own the world and lend it.
That is the change worth making, and it is small.

Per thread, two options, and the numbers favour the second:

- Each worker owns a `CompiledWorld` and pays one cold compile -- 756ms debug each, four
  workers, about 3 seconds. Simple, and no cross-thread anything.
- One thread compiles and the workers only execute, which is the split the previous section
  describes and costs one cold compile total.

## What to do

1. ~~**Give `ExampleTestRunner` a way to pass context.**~~ Done. It takes an initializer as
   well as an analyzer and lends a context per worker; `new` wraps the old signature in a
   context of `()`, so no existing suite changed.
2. ~~**`CompiledWorld`, owned.**~~ Done, and `std_all_tests` converted.
3. ~~**Keep the pipeline in the repl.**~~ Done. Not originally on this list, and the clearest
   win of the three.
4. **Split compile from execute** -- premise largely spent for the test suite; see below.
5. **Measure salsa `persistence`** -- sized, not attempted; see below.

## What it did

**`std_all_tests`**, measured by timing each phase and summing:

| Phase | Before | After |
|---|---|---|
| Compiling the library | **572 calls, 534s** | **143 calls, 17s**, over 4 worlds |
| **Suite wall time** | **258s** | **133s** |

Four worlds, one per worker thread, and each fixture pays the warm cost rather than the cold
one. All four backends share the database instead of building one each.

Getting to one context per worker took three attempts, which is the part worth remembering:

- Rayon's `map_init` calls its initializer **once per work split, not once per thread**. It
  built 80 worlds for 143 fixtures and the compile phase was 63.9s instead of 17s.
- One context per `par_chunks` chunk builds the fewest -- exactly one per thread -- but gives
  up rayon's per-fixture work stealing, so a chunk of slow fixtures straggles. 140s wall.
- A **pool** keeps both: borrow a context, run the fixture, hand it back. The pool never holds
  more than the number of threads running at once, and rayon still schedules a fixture at a
  time. 4 worlds, 17s, 133s wall.

**The repl.** `Session::compile` built a pipeline, compiled, and dropped the pipeline, so every
`reset` -- which happens on each panic recovery -- recompiled the library from nothing. Keeping
the pipeline on the `Engine`: startup 664ms, reset **84ms**, second reset 74ms, debug. The
library it recompiles is the copy embedded in the binary and cannot have changed, so reuse is
not merely faster but obviously correct.

**Splitting compile from execute** was next on the list and is now worth much less than it
looked. The compile phase is 17s of a 133s suite; the rest is the two AOT backends, at about
1.9s a fixture for codegen plus a `cc` invocation plus spawning the executable. Splitting
compilation out of that does not touch the `cc`. The idea still stands for a batch compiler,
where compiling many scripts against one world is the whole point, but it is no longer a way to
make this suite faster.

**Salsa `persistence`, sized rather than attempted.** Every item that should survive has to say
so -- `#[salsa::tracked(persist)]`, `#[salsa::input(persist)]`, `#[salsa::interned(persist)]` --
and everything they hold has to be `Serialize` and `Deserialize`. On the compile path that is:

| | |
|---|---|
| `bcts` | 40 |
| `datalove-datafun-tycheck` | 23 |
| `datalove-datafun-compiler` | 21 |
| `datalove-datalit` | 14 |
| `datalove-datafun-parser` | 8 |
| `datalove-datafun` | 8 |
| `datalove-datafun-resolve`, `-common` | 6 each |
| `datalove-datafun-ast` | 5 |
| `datalove-datafun-sema` | 1 |
| **total** | **132 across ten crates** |

So it is not a spike, it is a project, and it can fail late: one tracked struct holding
something unserializable blocks the route after most of the annotating is done. Before starting
it, the cheaper thing to know is whether the **in-process batch** covers the actual need --
`datalove script a.dfs b.dfs c.dfs` compiling all three against one `CompiledWorld` pays the
library once and about a ninth of it per script after, with no serialization at all. Persisting
across *processes* is the only thing that needs an artifact, and it should be justified against
that baseline rather than against today's.

## Two sources, so two worlds

There are two standard libraries and a process can want both:
`WorkspaceDescriptor::load_sys_dir(repo_root/sys)` reads the sources in the tree, which is
what `std_all_tests` uses because it is testing them, and
`WorkspaceDescriptor::from_system_library(&datalove_sys_packages::system_library())` uses the copy
embedded in the binary, which is what the cli and the repl use. `embedded_matches_tree` exists
because they are not interchangeable, so whatever holds a world holds one per descriptor, and
a persisted artifact is fingerprinted against the sources it came from.
