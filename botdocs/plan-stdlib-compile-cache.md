# Caching the Compiled Standard Library, In Memory

Compiling `sys/std` is the largest cost in a datalove invocation and the largest cost in the
test suite, and it produces the same answer every time. This is how to stop doing it twice.

In memory only. Where a serialized artifact would live is deliberately left open.

## Contents

- [What it costs](#user-content-what-it-costs)
- [What has to be cached, and why that is awkward](#user-content-what-has-to-be-cached-and-why-that-is-awkward)
- [The shape](#user-content-the-shape----done)
- [What stays per fixture](#user-content-what-stays-per-fixture)
- [The wrinkle, which is most of the win](#user-content-the-wrinkle-which-is-most-of-the-win----done)
- [Two sources, so two keys](#user-content-two-sources-so-two-keys)
- [What this does not solve](#user-content-what-this-does-not-solve)
- [Order](#user-content-order)
- [What it did](#user-content-what-it-did)

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

## The shape -- **done**

```rust
// datalove_datafun::pipeline
pub fn with_world<R>(
    key: &str,
    build: impl FnOnce() -> WorkspaceDescriptor,
    f: impl FnOnce(World) -> R,
) -> R;

pub struct World {
    pub db: &'static Database,
    pub descriptor: &'static WorkspaceDescriptor,
    pub compiled: &'static CompiledModules<'static>,
}
```

`pipeline/world_cache.rs`, a `thread_local!` map from key to leaked bundle.

It knows nothing about `sys`, which is what let it live in `datalove-datafun` rather than in a
new crate. `datalove-stdlib` depends on `datalove-datafun`, so the reverse is a cycle, and a
cache that takes a descriptor-building closure and a caller-chosen name needs neither. A caller
that wants the standard library writes the three lines that describe it, which
`std_all_tests` and `benches/jit.rs` now both do.

The key is named by the caller rather than derived from the descriptor: a descriptor is
expensive to compare, and two callers that mean the same world know it better than anything
here could work out. Naming two different worlds the same thing hands out the first, which is a
caller's bug and not a detectable one.

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

## The wrinkle, which is most of the win -- **done**

Four suites spawned a fresh thread per fixture to run anything with a jit in it, three of them
saying "to work around Cranelift JIT + PIE issues" and `interp_jit_tests` explaining it as
"placing the JIT memory in the mmap region rather than near the PIE base". A thread-local
cache gives a thread that lives for one fixture nothing at all, so those compiles would have
survived it.

**The workaround was obsolete, and the argument is structural rather than "it passed".** It
was added 2026-01-10. `7603fbc3`, 2026-03-22, "Fix x86_64 JIT relocation overflow for code,
data, and imports", removed both sources of the 32-bit PC-relative overflow the thread was
dodging: code and data now come from one contiguous `ArenaMemoryProvider` region, and every
import -- the `dtlv_rti_*` runtime and the dispatch trampoline, which live in the main binary
-- is reached through a local trampoline embedding the absolute address. After that, no
relocation refers to the distance between jit memory and the executable's base, so where mmap
landed stopped mattering.

The spawns are gone from all four. In three of them the thread was only the workaround and
`.join().expect(...)` re-panicked anyway, so removing it changes nothing. In `std_all_tests`
the join was also a panic boundary -- a jit that panics on one fixture was reported as that
fixture disagreeing rather than taking the suite down -- so that became
`std::panic::catch_unwind`, which keeps the boundary without the thread.

## Two sources, so two keys

There are two different standard libraries and a process can want both:

- `WorkspaceDescriptor::load_sys_dir(repo_root/sys)` reads the sources in the tree, which is
  what `std_all_tests` uses because it is testing them.
- `WorkspaceDescriptor::from_system_library(&datalove_stdlib::system_library())` uses the copy
  embedded in the binary, which is what the cli and the repl use.

They are not interchangeable -- `embedded_matches_tree` exists to check they agree -- so the
cache is keyed rather than singular. `std_all_tests` asks for `"std_all_tests/sys-dir"` and
`benches/jit.rs` for `"bench/sys-embedded"`. A descriptor also carries a work dir and compiler
options, so a caller that varies those varies its key too; the cache cannot check that for it.

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

1. ~~**Confirm the PIE workaround is obsolete** and drop the per-fixture spawn.~~ Done, in all
   four suites that had one.
2. ~~**Add `with_world`.**~~ Done, `pipeline/world_cache.rs`.
3. ~~**Convert `std_all_tests` and measure.**~~ Done. See below.
4. ~~**Convert `benches/jit.rs`**, which had its own private copy.~~ Done.
5. **Still to do**: the repl's and cli's in-process tests, if they turn out to pay it.

## What it did

`std_all_tests`, measured the same way the problem was found -- `eprintln` per phase, summed:

| Phase | Before | After |
|---|---|---|
| Compiling the library | **572 calls, 534s** | **4 calls, 3.4s** |
| `run_aot` | -- | 143 calls, 271s |
| `run_c_aot` | -- | 143 calls, 271s |
| Interpreter backend | -- | 143 calls, 5.5s |
| `build_and_load_riders` | 286 calls, 0.9s | unchanged |
| **Suite wall time** | **258s** | **143s** |

Four compilations, one per worker thread, exactly as designed. The library is no longer
measurable in the suite.

What is left is the two AOT backends, and they are now the whole of it: 542 CPU-seconds
between them for 286 runs, about 1.9s each, which is cranelift or C codegen plus a `cc`
invocation plus spawning the resulting executable. That is a linking-and-process problem
rather than a compilation one and wants its own look. The four backends also still each build
their own `ScriptCompiler` and `ScriptExecutor`, which is right -- one accumulates units and
the other owns a runtime -- and cheap: the interpreter backend is 5.5s for all 143.
