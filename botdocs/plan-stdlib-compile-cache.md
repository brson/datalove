# Caching the Compiled Standard Library, In Memory

Compiling `sys/std` is the largest cost in a datalove invocation and the largest cost in the
test suite, and it produces the same answer every time. This is how to stop doing it twice.

In memory only. Where a serialized artifact would live is deliberately left open.

## Contents

- [What it costs](#user-content-what-it-costs)
- [What has to be cached, and why that is awkward](#user-content-what-has-to-be-cached-and-why-that-is-awkward)
- [The shape](#user-content-the-shape)
- [What stays per fixture](#user-content-what-stays-per-fixture)
- [The wrinkle, which is most of the win](#user-content-the-wrinkle-which-is-most-of-the-win)
- [Two sources, so two slots](#user-content-two-sources-so-two-slots)
- [What this does not solve](#user-content-what-this-does-not-solve)
- [Order](#user-content-order)

## What it costs

Measured; see
[report-jit-and-inliner.md](reports/report-jit-and-inliner.md#user-content-what-a-real-program-actually-spends-its-time-on)
for the invocation numbers.

**Per invocation**, release: the process starts in 2.7ms, a trivial script with `--no-sys`
finishes in 3.6ms, and the same script with the library takes 84.8ms. So the library is
**81ms** and everything else about a small script is under a millisecond.
`botdocs/learn.dfs` -- 773 lines, most of the language -- adds 7.6ms on top of that floor.

**In the test suite**, debug, instrumented: `std_all_tests` calls `setup_and_compile` **572
times for 534 CPU-seconds**, and `build_and_load_riders` 286 times for **0.9 seconds**. The
riders are not the problem; cargo is a cheap no-op after the first build. 534 seconds across
the runner's worker threads is most of that suite's 258 second wall time, and that one suite
is most of `just test`:

| Suite | Wall time | Compiles `sys/std`? |
|---|---|---|
| `std_all_tests` -- 143 fixtures, 4 backends | **258s** | yes, once per backend per fixture |
| `backend_tests` -- 21 fixtures, 4 backends | 48s | via the cli |
| `interp_dispatch_tuned_tests` -- 409 fixtures | 17s | no |
| `interp_tests` -- 409 fixtures | 1s | no |

The interp and dispatch suites build their world from a worldfile's own module sections and
never load `sys`, which is why 409 fixtures cost a second and 143 cost four minutes.

## What has to be cached, and why that is awkward

Three facts, in the order they constrain the design.

**The result borrows the database.** `compile_fresh(db) -> CompiledModules<'db>`, and the
pipeline it came from has to outlive it too. So the cacheable unit is not `CompiledModules`
but the whole bundle -- database, pipeline, descriptor, compiled modules -- which is
self-referential. Leaking it is the practical answer, and a process-lifetime cache is what is
wanted anyway.

**`Database` is `Send` but not `Sync`.** `salsa::Storage` holds a `ZalsaLocal`, which holds a
`RefCell<QueryStack>` and an `UnsafeCell<HashMap<IngredientIndex, PageIndex>>`. So
`is_sync::<Database>()` does not compile, and neither does
`static DB: OnceLock<&'static Database>` -- that needs `&Database: Send`, which needs
`Database: Sync`.

> A trap worth writing down: a `#[test]` function in a `harness = false` target is **stripped
> before type checking**, so a probe written that way compiles clean and proves nothing.
> `std_all_tests` is such a target. Use a plain `#[allow(dead_code)] fn`.

**Therefore the cache is thread-local, not process-wide.** A `thread_local` hands out a
reference only within the thread that owns it, so no `Sync` is required. A process-wide cache
would have to move the bundle between threads under a lock -- `Send` allows that -- but then
it can only ever be lent to one thread at a time, which serialises every fixture. Not worth
it, given the thread-local version costs one compile per worker.

## The shape

```rust
/// The compiled system library, built once per thread and kept.
pub fn with_sys<R>(source: SysSource, f: impl FnOnce(SysWorld<'_>) -> R) -> R;

pub struct SysWorld<'a> {
    pub db: &'a Database,
    pub descriptor: &'a WorkspaceDescriptor,
    pub compiled: &'a CompiledModules<'a>,
}
```

A `thread_local!` holding a `OnceCell` per source, filled on first use by building the bundle
and leaking it. This is not a new pattern in the tree: `with_world` in
`crates/datalove-bench/benches/jit.rs` is exactly it, written because compiling `sys/std` per
benchmark iteration was both the slowest part of the setup and enough leaked salsa state to
exhaust memory. Generalising it is the work.

**Where it lives.** It needs `datalove-datafun` and `datalove-stdlib`, and it is test and
bench support rather than product code. Either a new `datalove-testsupport` crate depending on
both, or a module on `datalove-datafun` behind a feature the test targets turn on. A new crate
is the cleaner answer for the dependency DAG: `datalove-stdlib` already depends on nothing
that would make it circular, and product code has no use for this.

## What stays per fixture

The bundle is the library. Everything a fixture does with it is fresh and cheap:

- **`ScriptCompiler`** accumulates script units, so each fixture wants its own.
  `compiled.script_compiler_default(db)` makes one.
- **`ScriptExecutor`** owns a `Runtime` and an interpreter. Fresh per fixture.
- **Native symbol registration** is per executor, so it is repeated -- but
  `build_and_load_riders` is 0.9s over 286 calls, so it does not matter.

Sharing salsa across fixtures is the point rather than a hazard: the queries being reused are
the library's, and each fixture's script is a new input. The suites that deliberately test
incrementality -- `durability_tests`, `span_revision_tests`, `span_backdating_tests` -- must
keep building their own database and should not use this.

## The wrinkle, which is most of the win

`std_all_tests::analyze_file` spawns **a fresh thread per fixture** for the JIT backend, with
the comment "in spawned thread for Cranelift PIE workaround". A thread-local cache gives a
thread that lives for one fixture nothing at all, so 143 of the 572 compiles would survive.

**The workaround looks obsolete.** Nothing else in the tree does it:

- `interp_jit_tests` builds a `JitEngine` on the test thread and runs 409 fixtures.
- `datalove script --jit` builds one on the main thread, and that binary is a PIE.
- `benches/jit.rs` builds one per iteration on the bench thread.

So: confirm it, drop the spawn, and one world per worker thread covers all four backends.
If it turns out to be real, the fallback is a single long-lived JIT worker fed by a channel,
which keeps a thread-local world of its own.

## Two sources, so two slots

There are two different standard libraries and a process can want both:

- `WorkspaceDescriptor::load_sys_dir(repo_root/sys)` reads the sources in the tree, which is
  what `std_all_tests` uses because it is testing them.
- `WorkspaceDescriptor::from_system_library(&datalove_stdlib::system_library())` uses the copy
  embedded in the binary, which is what the cli and the repl use.

They are not interchangeable -- `embedded_matches_tree` exists to check they agree -- so the
cache is keyed by which, rather than holding one and hoping. Hence `SysSource` in the
signature above. A descriptor also carries a work dir and compiler options; if a caller varies
those, that is part of the key too, and the honest move is to make `SysSource` carry whatever
the descriptor was built from rather than to guess.

## What this does not solve

**The 81ms a cli invocation pays.** A thread-local cache lives and dies with the process, so
`datalove script` still compiles the library on every run. That wants a serialized artifact,
and the machinery is closer than it looks: `IrCodeUnit`, `ModuleFunctionRegistry` and
`FunctionRegistry` all derive `Serialize`, and `ir_serial_tests` already round-trips IR
through RON and checks the deserialized form executes identically on the interpreter *and* the
AOT backend. What is unsettled is how much beyond the IR has to travel, because a script
importing `sys/std` is typechecked against the library's exported signatures and those live in
salsa, not in the IR. Deliberately not decided here, and no cache location is being chosen.

**Shipped programs.** An AOT-compiled binary pays the library at build time, not at run time,
so none of this touches what a user's compiled program costs.

**The repl** already pays once per session, which is fine.

## Order

1. **Confirm the PIE workaround is obsolete** and drop the per-fixture spawn in
   `std_all_tests`. Without this the cache misses a quarter of the compiles.
2. **Add `with_sys`**, generalised from `with_world` in `benches/jit.rs`, in a crate the test
   targets can reach.
3. **Convert `std_all_tests`**, and measure. The instrumentation to check is two `eprintln`s
   around `setup_and_compile` and `build_and_load_riders`, summed by phase -- which is how the
   534 seconds above was found.
4. **Convert what else pays it**: the repl's and cli's in-process tests, and
   `benches/jit.rs`, which should then drop its private copy.

**Expected effect.** `std_all_tests` goes from 572 library compilations to one per worker
thread, so from 534 CPU-seconds to single digits. What remains of its 258 seconds is the four
backends' own work -- the AOT `cc` invocations, and running each fixture four times.
