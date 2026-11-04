# Script Sessions in Parallel

A plan for running several script sessions in one engine, sharing the
compiler, each with its own interpreter and runtime, and eventually each on a
thread of its own. Written November 2026. Phase 1, several sessions on one
thread, is implemented; phases 2 and 3 are not.

## Contents

- [Why](#user-content-why)
- [What already holds](#user-content-what-already-holds)
- [Phase 1: sessions in one engine](#user-content-phase-1-sessions-in-one-engine)
- [What stands in the way of threads](#user-content-what-stands-in-the-way-of-threads)
- [Phase 2: a thread per session](#user-content-phase-2-a-thread-per-session)
- [Phase 3: sessions that move between threads](#user-content-phase-3-sessions-that-move-between-threads)
- [Open questions](#user-content-open-questions)

## Why

An `Engine` used to be one session: one database, one pipeline, one
`ScriptSession`, one `ScriptExecutor`. Several scripts against the same
workspace meant several engines, and so several compiles of the standard
library and several copies of everything salsa memoizes. The compile is the
expensive part and it is the same for every script; only the units and their
values differ. So the engine should hold the compile once and the sessions
beside it, and since sessions do not share values, nothing in principle stops
them from running at once.

## What already holds

Most of what sharing needs was already true, because of how the reactive
script work keyed things:

- **A unit is a salsa input**, `ScriptUnit`, linked to the one before it
  (`crates/datalove-datafun-ast/src/script.rs`). Every per-unit query is keyed
  on the handle, so two sessions are two chains in one database and their
  memos cannot collide. No query is keyed on a unit's id, so both sessions
  numbering their units from 1 is fine.
- **Each interpreter owns its runtime**, `datalove_rt::rust::Runtime`, with its
  own allocator. A runtime error freeing everything the runtime allocated
  frees one session's values.
- **Globals are per thread or atomic.** The thread-locals are the query and
  unit-analysis logs, the JIT's dispatch context and the stack limit; the rest
  are atomic counters and mutex-guarded caches (`OPEN_LIBRARIES`,
  `COMPONENT_CACHE`).
- **The module IR is shared already.** An executor holds the modules as an
  `Arc<ModuleFunctionRegistry>`.

## Phase 1: sessions in one engine

Done. `crates/datalove-script-engine/src/engine.rs` splits the engine into
`Shared` -- the database, pipeline, workspace, natives, `also_roots` and
`module_errors` -- and a `SessionState` per session: the `ScriptSession`, the
executor, its registered natives and its last step. `Engine::open_session`,
`close_session` and `session(id) -> SessionMut` are the API; the methods on
`Engine` itself act on the session it started with, which is all the REPL and
`script-world` use. The tests are `tests/engine_session_tests.rs`.

Three things had to change, and none of them was the compiler:

- **The roots are the union of every session's requires.** A session settling
  the roots settles them on the other sessions' units too, or it would take
  their modules out of the compile. A module only another session requires
  changes nothing a unit means, since a unit sees only what it requires.
- **An executor takes new modules by count, not by step.** A step used to give
  the executor the new module registry when that step moved the roots. With
  sessions sharing roots, session B can require a module session A already
  brought in, and B's step moves nothing. So `Shared::modules` counts every
  change to the compiled modules, each session remembers the count it took,
  and a session behind takes them before it runs.
- **A reset is a session's.** A runtime error or a crash rebuilds the one
  session, settling the roots on what the others still require. A module edit
  is the one step that is the engine's: it re-derives what it reaches in every
  session and returns a report per session.

One session's units are not recompiled when another brings a module into the
compile; the test `a_module_another_session_brings_in_recompiles_nothing_here`
holds that.

## What stands in the way of threads

**But** the sessions still take turns. Two facts decide how far they can stop:

- **`ScriptExecutor` is not `Send`.** It holds raw pointers to type
  descriptors and IR, `Rc<BcFunction>`, `Rc<IrLayout>`, `Rc<[BindingRef]>`, a
  `Box<dyn CallDispatcher>` without `Send`, and `dyn Any`.
  `RegisteredNatives` holds `*const u8`. The database is `Send` and not
  `Sync`, as salsa means it to be: one clone per thread.
  `ModuleCompilationPipeline` is `Send + Sync` and `ScriptSession` is `Send`.
- **A salsa write waits for every other clone of the database to be
  dropped** (`Storage::cancel_others` in salsa 0.28), cancelling the queries
  they are running. Editing a unit is `set_text` and splicing one is
  `set_prev`, both writes; appending a unit only makes inputs, which takes
  `&db`. Running units touches no database at all.

So execution can be parallel freely, compiling can be parallel between
writes, and every edit is a point where all compiling stops.

## Phase 2: a thread per session

**Therefore** each session lives on a thread that builds its executor and
never lets it go, as the REPL's worker does today with the whole engine. A
coordinator owns the database and the pipeline and makes every write; a
session thread compiles on a clone of the database it takes for the step and
drops after.

- `ModuleCompilationPipeline::compile` takes `&mut self` for its const cache
  and module ids. The `CompiledModules<'db>` it returns borrows only the
  database, so a mutex held for the call is enough to begin with.
- A session compiling when another session edits sees `salsa::Cancelled` and
  has to retry the step from its start. Which steps can be retried whole --
  an append can; a rerun that has already run units cannot -- is the first
  thing to settle.
- `Shared` splits along the same line as phase 1 drew: what the coordinator
  owns, and what a session reads. The roots and `module_errors` are the
  coordinator's, decided over every session's units.

## Phase 3: sessions that move between threads

Making `ScriptExecutor` `Send` -- `Rc` to `Arc` in the bytecode caches,
`CallDispatcher: Send`, the pointers wrapped in types that say why sending
them is sound -- would let sessions run on a pool rather than a thread each.
That is an unsafe audit of the interpreter and the JIT, and nothing in phase 2
needs it. The JIT compiles per executor today, so sharing compiled code
between sessions is a separate step again.

## Open questions

- Should a module edit wait for every session to finish what it is running,
  or let the sessions that the edit does not reach keep going on the old
  modules?
- What does a session see while another session's edit is between its write
  and its re-derivation: the old module, or nothing until it settles?
- Is one thread per session acceptable for the number of sessions we expect,
  or does that number make phase 3 a requirement rather than an option?
