# Issues

Known faults and gaps that are not being worked on right now, so that they are
written down somewhere other than a conversation.

This is for things that are *wrong* or *missing*, not for design questions,
which belong in a plan or a design doc. An entry says what breaks, what it
costs, what it would take, and whether it has been reproduced or only reasoned
about -- the last of those matters, and entries should say so plainly.

Remove an entry when it is fixed. A fixed issue lives in the commit that fixed
it and in whatever doc describes the working thing; it does not need a second
home here.

## Contents

- [Nothing but a test inlines](#user-content-nothing-but-a-test-inlines)
- [No table module, and none can be written](#user-content-no-table-module-and-none-can-be-written)
- [A unit that fails part way through leaves its index to the next one](#user-content-a-unit-that-fails-part-way-through-leaves-its-index-to-the-next-one)
- [The loop check asks where a binding ended up, not whether its move repeats](#user-content-the-loop-check-asks-where-a-binding-ended-up-not-whether-its-move-repeats)
- [The jit cannot see a loop, and the C backend is built at -O0](#user-content-the-jit-cannot-see-a-loop-and-the-c-backend-is-built-at--o0)
- [A `let` that fails to typecheck leaves its name undefined](#user-content-a-let-that-fails-to-typecheck-leaves-its-name-undefined)
- [A failed `set m[k]!` on a map says "index out of bounds"](#user-content-a-failed-set-mk-on-a-map-says-index-out-of-bounds)
- [Compile-time evaluation has no limits](#user-content-compile-time-evaluation-has-no-limits)
- [Nothing takes a value back out of data or error](#user-content-nothing-takes-a-value-back-out-of-data-or-error)
- [One function per name, so no prelude](#user-content-one-function-per-name-so-no-prelude)
- [Borrowed enum payloads and tables inside a generic were never probed](#user-content-borrowed-enum-payloads-and-tables-inside-a-generic-were-never-probed)
- [Nothing proves the suite runs with leak checking on](#user-content-nothing-proves-the-suite-runs-with-leak-checking-on)
- [The allocator maps a page per large block and keeps every small page](#user-content-the-allocator-maps-a-page-per-large-block-and-keeps-every-small-page)
- [Module ids are positional](#user-content-module-ids-are-positional)
- [Pruning stops at the module](#user-content-pruning-stops-at-the-module)
- [Nothing consumes a WorkspaceDelta](#user-content-nothing-consumes-a-workspacedelta)
- [No batch of scripts against one world](#user-content-no-batch-of-scripts-against-one-world)
- [Every REPL line is a new salsa input](#user-content-every-repl-line-is-a-new-salsa-input)
- [Deep recursion aborts the process, in every engine but the bytecode](#user-content-deep-recursion-aborts-the-process-in-every-engine-but-the-bytecode)
- [The C backend's tensor views are static, so not reentrant](#user-content-the-c-backends-tensor-views-are-static-so-not-reentrant)
- [Nothing names another module's type](#user-content-nothing-names-another-modules-type)
- [The C backend cannot build a const table inside a module function](#user-content-the-c-backend-cannot-build-a-const-table-inside-a-module-function)

## Nothing but a test inlines

**Reproduced**, in the sense that the constructors are countable.

`DynamicInliner` is constructed in exactly one place,
`OptimizingDispatcher::with_config`. `OptimizingDispatcher` in turn is
constructed only in its own unit tests, in the two dispatch suites, and in
`datalove-bench/benches/jit.rs` -- and a bench is compiled by `just test` but
not run by it. `datalove script --jit` builds a bare `JitEngine`; the REPL and
`worldfile_analysis` pass no dispatcher at all. So the inliner has never run in
anything a user can invoke.

The other inliner, the directive-driven `datalove-datafun-inline` API --
`parse_inline_directives`, `inline_module`, `inline_cross_module`, and the
`inline-directives` worldfile section -- is reachable only from
`ir_inline_tests`, which compares printed IR before and against after and never
*runs* the result. 21 fixtures, none generic.

What does run is the two dispatch suites, over the 409 worldfiles in
`fixtures/interp`: 7 inlinings in tuned mode, 151 in chaos mode, checked by
comparing against the plain interpreter.

**Worth knowing before spending anything here.** An inlined body is picked up in
`execute_call`, when the function is *entered*, so an inlining performed during
an invocation does not affect that invocation -- only a later call to the same
caller. A hot loop inside a single call never benefits from inlining its
callees. That is what makes the feature much weaker than the thresholds
suggest, and it is worth deciding whether the inliner earns its place at all
before building anything on top of it.

**Two more things, reasoned from the code.** The inlined body never reaches the
jit: `OptimizingDispatcher::dispatch_call` always hands `try_jit_execution` the
original callee ("We always pass the original function to JIT"), and the inlined
caller is only picked up in `execute_call`, after dispatch has declined. So
`InlinedJit` is a metrics label, not a different compile. And `DynamicInliner`
has no cost model: it inlines when a site's count crosses `threshold` (100), with
no size limit, no growth budget, no depth limit and no recursion check, so a hot
self-recursive function inlines into itself and the moved sites, with fresh ids,
cross the threshold in turn. The directive-driven path does check recursion.

## No table module, and none can be written

**Investigated, not a fault.** A table is a first-class opaque value: built,
moved, cloned, compared, sorted, keyed on, held inside anything
(`std_tests/144_table_shapes`). What nothing does is look inside one -- no cell
accessor, no row count, no push or pop -- and no module could add them. Two
things stop one being written.

Note that `ord` already reaches tables: `ord.compare` and `ord.equal` take one,
because the comparison walks a value from its descriptor, so `ord.sorted`,
`ord.contains` and the rest work over a `[{| ... |}]`. Whole-table operations
are not the gap.

**There is no way to say "any table".** Tables *are* generic per column --
`fun ident<T>(t: {| x: T, y: u32 |})` compiles and runs, and
`bind_type_params` unifies two table types column by column. What cannot be
written is a signature whose *column list* is not fixed: a type parameter
stands for a column's type, and nothing stands for the set of columns.

```datalove
fun len(ref self: ???): index    // nothing goes in the hole
```

The same hole stops the **native** being declared, which is why this is not
merely a missing library. Writing it over a bare `T` instead would accept a
list or a string just as readily and read their bytes as a table's.

**A column's type varies by column.** `dtlv_rti_table_get_local` takes a row
and a column and hands back a raw pointer, because there is no one type to
hand back. A `get(ref self, row, col)` has no return type to write: the answer
depends on `col`, which is a value. So even a module written for one concrete
table type could not have a general `get`; it would need one accessor per
column, which is what column projection syntax (`t.x`) would give.

**What exists underneath.** The runtime has `table_create`, `table_destroy`,
`table_push_row`, `table_build_from_rows`, `table_get`, `table_set`,
`table_clear` and `table_len`, none of them reachable from the language. The
spec mentions column projections yielding a list view; `t.x` is F068 `has no
fields` today, and `t[i]?` is F011, because indexing wants a list, map or
tensor.

Because nothing reaches `table_push_row`, a table's row count is whatever was
written in the literal that made it, so a table cannot be built from data at
all.

**What it would take** is worked through in
[Tables: what the type system is missing](design-table-rows.md). The short of
it: column projection first, which needs no type-system change and is already
specified -- and which is cheap because the storage is columnar, so a column
view is a well-formed list header pointing into the table. Then a row type, so
that `table R` makes the row an ordinary parameter and the natives writable.

**The tensor module went the other way** and is written, because a tensor's
element *is* a type parameter -- `[|T, 1|]` -- even though its rank is not. See
`sys/std/tensor.dfm`.

## A unit that fails part way through leaves its index to the next one

**Reasoned, not reproduced.**

A script unit is registered once it finishes
(`IrInterpreter::execute_script_unit_in_env` calls `env.add_unit` after
execution, and returns early on error). So a unit that runs far enough to leave
something behind -- an inlined body in the dispatcher, a JIT entry -- and then
errors is never registered, and the next unit takes the index it was using.
Anything remembered under that index is then the wrong unit's.

`FuncIdentity` keys on the unit a local reference belongs
to, and take it from the registry's count, which is the index the running unit
will have. That is the same number the compiler uses for a later unit's
`CodeRef::External`, so the two agree -- as long as a failed unit does not
disturb the counting.

Closing it means the compiler's unit numbering and the runtime's agreeing about
failed units, which `CodeRef::External` already assumes today. Worth examining
as its own question rather than patching the key.

## The loop check asks where a binding ended up, not whether its move repeats

**Reproduced.** A spurious D007 on a loop that re-initializes before it moves.

```datalove
var s = "a"
loop while i .< 3
  set s = "b"        // s is Live again here
  let t = s          // D007 -- but no iteration reads a moved s
  ...
end loop
```

Every path reaches the move with `s` live, because the `set` precedes it. The
pass knows: `analyze_set` marks the binding `Live` again. What it does with that
is nothing, because `MoveInLoop` is positional -- it asks whether the binding is
`Moved` at the end of the body, not whether the move could ever read a value
already given away.

**Why one pass is otherwise enough.** `analyze_loop` analyzes the body once, from
the state before the loop; iteration 2 is never modelled. That works because the
pass does not carry a "maybe moved" to widen into -- a binding is `Live` or
`Moved` and drop points are static -- so instead of converging it *demands the
loop-head state be invariant* and refuses anything else. Accepted programs are
exactly those already at a fixpoint after one pass. The approximation is paired
with a rule that excludes its blind spot, which is why it is sound; this is the
precision that rule costs.

The condition is read once too, before the body, so a condition reading a binding
the body moves is never analyzed against the moved state. Nothing in the
condition handling catches that -- `MoveInLoop` does, by refusing the move at
all. So the blunt rule is load-bearing for soundness, not only for precision.

**What it would take**, and the coupling to watch. A real fixpoint: analyze the
body, feed the end state back to the head, repeat until it settles, and ask of
each *use* whether it can read a moved value rather than asking where the binding
ended up. Two things depend on the present rule and would have to move with it:

- The loop-exit merge (`merge_loop_exits`) takes the state before the loop as the
  state at a condition-failure exit. That is sound only because `MoveInLoop`
  guarantees the end of the body agrees with it for every outer non-copy
  binding. Relax the rule and that guarantee goes, and the exit merge is
  unsound rather than imprecise.
- Auto-adapt mode recovers from the same site by inserting a clone, so the
  imprecision shows up there as a copy per iteration that nothing needed, not as
  an error. A fixpoint would remove those too.

Loop exits themselves are settled: see `merge_loop_exits`, D014, and
`std_tests/152_owned_past_a_loop_exit`.

## The jit cannot see a loop, and the C backend is built at -O0

**Reasoned from the code; the speedups are measured.** Four gaps in what the
compiled tiers do, each cheap to state and none started.

- **No back-edge counting and no OSR.** `JitEngine::record_call` counts calls
  and only calls, and compiled code is entered only at a call. A function called
  once that loops ten million times is never compiled. `loop_arith`'s 86x exists
  because its loop sits in a function called twenty times; move it up a level and
  the speedup is 1.0x.
- **No inlining in Cranelift.** Nothing implements
  `cranelift_codegen::inline::Inline` or calls `Context::inline`, so neither the
  jit nor the cranelift AOT backend inlines. A call between two compiled
  functions is an indirect call through the callee's stub, which reads the
  callee's code cell (`JitCompiler::code_cells`); only a call to a function not
  yet compiled reaches `__jit_dispatch_call`.
- **The C backend runs `cc -std=c11 -O0 -g`** (`pipeline/c_aot.rs`, and the
  same in `c_dual_tests.rs`). `-O2` measured 8.4x on `loop_arith`, the largest
  number in the jit report. The work is whatever undefined behaviour `-O2` exposes
  in the emitted C, which the four-backend differential suite would find.
- **`register_natives` in the cli only finds a bare `JitEngine`.** It downcasts
  the dispatcher to `JitEngine`, so an `OptimizingDispatcher` would get no native
  symbols and the first native call from jitted code would abort the process.
  Unreachable today, since the cli never builds one; it is in the way of doing so.

## A `let` that fails to typecheck leaves its name undefined

**Reproduced** with the debug cli. When a `let` or `var` initializer fails to
typecheck, the binding is never added, so every later use of the name reports a
second error, F001 "cannot find value", under the real one:

```datalove
let s: (string, i32) = ("a", 1)
let r = s.0       // F070, a non-copy field read out
debuglog r        // F001 cannot find value `r`
```

The cause is `check_variable_decl` in `tycheck/src/statement.rs`: it calls
`bind_pattern` only when the value checked or synthesized, and on failure adds
the error and stops. It applies to every kind of failure, not just F070 and
F072.

Fixing it in part is easy: when the declaration has a type hint, the hint is the
binding's type whether or not the value fits it, so the name can be bound
anyway. Without a hint there is no type to give it, so the full fix needs a type
that stands for "already reported", one that every check accepts silently, so
uses of the name neither fail nor report again. The typechecker has no such
type: `datalit::tycheck::Type::Error` is the language's `error` type, not a
poison type.

## A failed `set m[k]!` on a map says "index out of bounds"

**Reasoned about** from the code. Reading `m[k]!` fails with `"key not found"`
(`lower/src/expr.rs` picks the message by collection kind), but the `set` path,
`emit_fallible_index_check` in `lower/src/stmt.rs`, emits `MapContainsKey` for a
map and then hard-codes `"index out of bounds"` for the error. The fix is to
pick the message the way `expr.rs` does, then drop the exception botspec §7.3
notes.

## Compile-time evaluation has no limits

**Reproduced.** CTFE runs the ordinary interpreter (`InterpCtfeEvaluator`) with
no step limit, no recursion-depth limit and no allocation limit. A const calling
a function that never terminates hangs `datalove script-ir`, which only compiles;
a const calling unbounded recursion aborts the compiler with a Rust stack
overflow. There is a `ConstEvalError::GasExpired` and an arm mapping an error
message containing "gas" to it, but nothing produces such a message, so the arm
is dead.

Errors are reported thinly too. A const whose expression early-returns says
`h::X: early return: const expression returned early via ! or ?` as a lowering
error: the error value is discarded, and there is no span and no evaluation call
stack.

What it would take: a step budget and a depth limit in the interpreter, checked
only when evaluating a const, each failing as a diagnostic on the const; and
carrying the returned error and the call chain into that diagnostic.

## Nothing takes a value back out of data or error

**Checked against the spec and the parser.** `data x` and `error x` wrap any
value, and nothing in the language unwraps one: there is no type test, no cast,
no type pattern in `match`. `is` parses only in a `with` bound. So a `data` can
be moved, cloned, compared and printed, and an `error` can be printed, but code
cannot recover what is inside.

The candidate syntaxes considered were a type-testing `if` (`if d is u32 |x|`),
`as` returning a result with `as!` panicking, and type patterns in `match`
(`case : u32 |x|`). Unchosen, along with whether the test is structural or
nominal.

## One function per name, so no prelude of imports

**Reproduced; worked around by qualified calls.** `TypeContext.functions` maps
a name to one `TypeFunction`, and importing a second function of the same name
is F059 ("a name binds one function"). Qualified calls (botspec Section 9.4)
now let one script use both `u8.from_int` and `i8.from_int`, each through its
module's alias, so the collision no longer blocks a program.

It still blocks a prelude made of imports: 152 of the 275 distinct function
names in `sys/std` are defined in more than one module (`min`, `max` and
`clamp` in fifteen, `from_int` in thirteen), so any prelude importing from more
than one numeric type collides with itself. A prelude that requires modules
and leaves their functions to be called qualified does not.

What it would take to import both is exact-match overloading or
`import ... as`. Overloading is cheaper than its reputation here, because
datalove has no implicit conversions to rank: a call resolves to the one
candidate whose parameters match, in the typechecker, and nothing below it
changes.

## Borrowed enum payloads and tables inside a generic were never probed

**Not probed.** Borrowed generic values carry their descriptor, and field
projections, list, map and tensor indexing under a borrow are covered by
`backend/15` to `backend/23`. Two shapes are not:

- An enum or term payload reached through a borrow inside a generic --
  matching on a `ref` parameter like `?{a: T, b: u32}` and reading the payload.
  `TyInfoEnumVariant` has an offset and a payload descriptor, so the same rule
  should apply, but whether payload projection goes through `GetFieldRef` or its
  own path was never checked. `backend/18`'s `get_opt` returns such an option
  whole; nothing reads into it.
- A table as a borrowed generic value, `ref t: {| a: T, ... |}`. Nothing can look
  inside a table yet (see above), so this is about moving and cloning one.

What it would take is a fixture in `backend/` for each, run across the four
backends.

## Nothing proves the suite runs with leak checking on

**Reasoned; the first half was reproduced when the default flipped.**
`LeakCheckMode::from_env` defaults to `Ignore`, and the justfile sets
`DATALOVE_LEAK_CHECK=panic` on every test recipe. Nothing checks that it does:
`DATALOVE_LEAK_CHECK=ignore just test` passes, so if a recipe loses the variable
the hundred-odd fixtures that exist to be checked by the leak detector -- and the
double-free and `free`-mismatch checks, which catch codegen bugs -- silently stop
checking anything. The `alloc.rs` unit tests pass `LeakCheckMode::Panic`
explicitly, so they test the mechanism and not the wiring. The fix is one canary
in `datalove-rt-tests` that leaks through `dtlv_rti_init` with the mode taken from
the environment and expects the panic.

Separately, the cli tests spawn the real binary with no `env_remove`, so they
inherit `panic` and test a configuration no user runs. Either clear the variable
at the spawn sites (seven `Command::new`s in `datalove-cli/tests`, built inline,
which is where a helper would earn its place) or accept that they are not testing
the shipped default.

## The allocator maps a page per large block and keeps every small page

**Reasoned from `datalove-rt/src/impls/alloc.rs`; none of it measured as a
cost.** Three structural costs in the runtime allocator:

- **Every block from 2049 to 4096 bytes is its own `mmap`.** The largest size
  class is 4096 and `PAGE_SIZE` is 4096, so `allocate_page_for_size_class` makes
  one block per page. Freed blocks go back on the free list and are reused, so
  this costs a syscall per *live* block, not per allocation. Above 4096 every
  allocation is an `mmap` and every free a `munmap`, with no cache in front.
- **`free_large` scans `large_pages` linearly** to find the page, so freeing one
  of *n* live large blocks is O(n) and a workload holding many is quadratic.
- **Small pages are never returned.** `free_small` pushes onto the free list and
  the page stays mapped until shutdown. Fine for a short process, not for a REPL
  or anything long-lived.

A program working in buffers of a few kilobytes would find the first two.

## Module ids are positional

**Measured.** `ir_module_ids` numbers rider modules first, by path, then regular
modules in graph order, and the ids are encoded in the IR. So a module appearing
anywhere but the end renumbers everything after it: a newly required module that
sorts first costs one parse, typecheck and ownership analysis, but re-lowers every
module. A newly required rider, numbered ahead of all of them, should renumber
every regular module (reasoned, not measured).

It costs nothing shipped today, because the cli fixes its roots once and the REPL
compiles `Roots::All`, so no module set changes mid-session. It blocks narrowing
the REPL. The two fixes are priced very differently: hash the path to an id
(small, but about 215 expected-output files render ids as `m{}` and need
blessing, and collisions need deterministic resolution), or name modules
symbolically in the IR
and assign dense numbers at registry-build time (the right end state; touches the
IR, its serialization, both AOT backends, the jit and display).

## Pruning stops at the module

**Reasoned from the code.** Narrowing roots drops unreachable *modules*, and
nothing finer:

- **Unused functions in a reachable module are emitted.** The cranelift AOT
  backend walks the whole registry, and DCE (`datalove-datafun-const/src/dce.rs`)
  removes unreachable blocks within a function, never a function.
- **The original of a fully specialized function is kept.** Const-parameter
  specialization is additive (`specialize.rs`, "Why the original is kept"),
  because a later script line may call it. An AOT build has no later line, so an
  original whose every call site was specialized is dead weight there.
- **`rider_interfaces` parses every rider source** whether or not anything
  requires it. Memoized, so paid once per process, but paid on every short
  invocation.

## Nothing consumes a WorkspaceDelta

**Checked.** `WorkspaceDescriptor::diff` produces a `WorkspaceDelta`, and
`WorkspaceDelta::apply_to_pipeline` applies its module half -- added to
`add_module`, removed to `remove_module`, changed to `update_source`, which keeps
salsa identity -- and refuses a delta changing riders or options, which need a new
pipeline (and, for riders, a rebuilt native component). Both are called only from
their unit tests. The REPL keeps its descriptor in step
(`Engine::set_workspace_module`, which says nothing reads it) but edits the
pipeline directly; no driver diffs two snapshots. So the delta path is built and
unexercised, and the first real consumer (a file watcher, an LSP) is what would
test it.

## No batch of scripts against one world

**Not built.** `datalove script` takes one file. Compiling `a.dfs b.dfs c.dfs`
against one `CompiledWorld` would pay the library once and roughly a ninth of it
per script after, with no serialization: the same reuse that took
`std_all_tests` from 534s to 17s of library compilation.

It matters as the baseline for salsa `persistence`, the cross-process
alternative. That was sized, not attempted, at about 132 `persist` annotations
across ten crates, with everything they hold needing `Serialize` and
`Deserialize`, and it can fail late: one unserializable tracked struct blocks it
after most of the annotating. It should be justified against the in-process batch,
not against today's one-script-per-process.

## Every REPL line is a new salsa input

**Reasoned from the code.** `ScriptCompiler::compile_fragment` and
`compile_expr` mint a `bct::input::Source` per call, and salsa never collects
inputs, so a session grows by one input (and its text) per line, forever. Small
per line; unbounded in a long session. Editing a unit already goes through a
stable per-unit `Source` and `set_text`; appending does not.

The same minting means a fresh compiler replaying a session's lines would reuse
nothing, since identical text is a different input. Nothing replays today --
`Engine::reset` after a panic starts an empty session rather than replaying the
old one -- so that half only matters if recovery is ever meant to keep the
session. Holding a stable `Source` per unit for appends too would fix both.

## Deep recursion aborts the process, in every engine but the bytecode

**Reproduced.** Every engine but the bytecode runs datalove calls on the
native stack, and nothing limits the depth, checks for overflow or recovers
from it. A trivial recursive function, `down(n)` returning `down(n - 1) + 1`,
on a release build:

| engine | stack | deepest that ran |
|---|---|---|
| IR walker | main thread, 8 MiB | ~7,450 |
| JIT (`script --jit`) | spawned thread, 2 MiB | ~14,400 |

The bytecode, the default engine, makes calls from one bytecode body to
another without recursing on the Rust stack (`plan-frame-stack.md`), so it ran
a million deep, and ten million fails as `InterpError::StackOverflow` when the
frame stack reaches its limit; `deep_recursion_tests` checks both. What it
leaves on the Rust stack is calls it hands to the general path -- any call made
while a dispatcher is installed, and calls the fast path does not suit -- and
what is left of the issue applies to chains of those. Its error does not
destroy what the unwound frames owned, as no error does yet.

Past that, `fatal runtime error: stack overflow, aborting`: the whole process
goes, the REPL included, whose `catch_unwind` cannot catch it. A debug build of
the interpreter spends 14-17 KB of stack a call (`execute_instruction` and
`run_bytecode` have frames of about 10 KB each), which on a 2 MiB thread is
about 120 levels.

Where the stack goes:

- **IR walker**: `execute_blocks`, `execute_instruction`, `execute_call_site`,
  `run_frame` -- four Rust frames, about 1 KB, per datalove call. Frame data
  is on the interpreter's own `FrameStack`; only control nests. Running out of
  that stack is an `InterpError::StackOverflow`, but at the default limit the
  Rust stack runs out long before it.
- **Bytecode**: nothing for a call between bytecode bodies; a call through the
  general path nests `run_bytecode` again, as the IR walker nests.
- **JIT**: a stack slot of the shared frame layout per function, and a call
  through a per-callee stub, so two machine frames a call. A call from JIT code
  to a function not compiled yet goes through `__jit_dispatch_call` back into
  the interpreter, so mixed chains interleave both on the one stack.
- **AOT**: the same, Cranelift stack slots or a `uint8_t __frame[N]` on the C
  stack.
- **Natives** are leaves: nothing calls back into datalove code from a native or
  the runtime. The runtime's destroy, clone and compare do recurse natively in
  proportion to how deeply a value nests.

Related faults:

- `script --jit` and the REPL worker run on `std::thread::spawn`'s default 2 MiB
  stack, a quarter of the main thread's.
- Neither Cranelift backend sets `enable_probestack`; a JIT frame bigger than a
  page could step past the guard page rather than onto it. Not checked against
  this Cranelift's default.
- `__jit_dispatch_call` is `extern "C"`, not `"C-unwind"`, so a panic, or an
  `InterpError` turned into one, from interpreted code under JIT code aborts.
- Compile-time evaluation has no depth limit either; see
  [Compile-time evaluation has no limits](#user-content-compile-time-evaluation-has-no-limits).
- No test recurses deeper than about 30. `worldgen_dual_tests` works around the
  interpreter's depth with a 32 MiB thread.

**What is wanted**: the interpreter and the JIT detect overflow and tear down
safely -- an error that unwinds every frame, destroying what each owns, and
leaves the REPL and the host running. The AOT backends abort, as compiled code
does, but with a message rather than a segfault where that is cheap. Detection
can be a depth count or a check of the stack pointer against a limit at calls
(both engines cross through a few known places: `execute_call_site`,
`fast_call`, the JIT's stubs and its dispatch trampoline). Moving bytecode calls
off the Rust stack (see "Calls" in `plan-bytecode.md`) would make the
interpreter's own limit a bounds check on its frame stack, but mixed JIT and
interpreter chains still nest natively and still need the check.

## The C backend's tensor views are static, so not reentrant

**Reasoned about, not reproduced.** Indexing a tensor of rank above 1 by
reference (`TensorIndexRef`) makes a view tensor and hands out a pointer to it.
The C backend puts the view in a function-scope `static` buffer
(`datalove-datafun-c-aot/src/codegen.rs`, the rank-above-1 arm of
`TensorIndexRef`, `static _Alignas(8) uint8_t __view[...]`), presumably because
a block-local array would not outlive the block that makes it. A static is
shared by every activation of the function: a recursive call that indexes
through the same instruction overwrites the view its caller still holds, and so
would two threads. The comment beside it says "on C stack as a local variable",
which it is not.

The Cranelift backends get this right: the view is a stack slot of the
function (`datalove-datafun-cranelift/src/codegen/tensors.rs`), one per
activation. The fix is the same for C -- declare the buffer at function scope,
beside `__frame`, or give it room in the frame layout.

One instruction executed twice in the same activation, in a loop, reuses its
view in every backend; whether a reference from an earlier iteration can still
be live then is a separate question, not looked at.

## Nothing names another module's type

**Reproduced.** A module's `type` alias cannot be named outside it. A qualified
name does not parse in any type position -- `let a: qt.Shape`,
`const A: qt.Shape` and a parameter `s: qt.Shape` are all parse errors -- and
`import qt.Shape` looks for a function and reports F002. So a value of another
module's named type can only be annotated by writing the type out in full, as
`std_tests` and `specialize_differential/033_comptime_collections` do.

The typechecker already records each module's `exported_type_aliases`; what is
missing is a way to reach them, either a qualified type name or an `import`
that takes types as well as functions.

## The C backend cannot build a const table inside a module function

Reproduced by `interp/832_ctfe_table_module_func`, which opts out of the C
engine for it. A `const` table in a module function's body becomes a static
whose initializer, `__dtlv_statics_init` in `script.c`, names the row type's
descriptor `__tydesc_1` -- a descriptor declared only in the module's own C
file, so `cc` rejects the program. The interpreters and Cranelift agree on the
output. Probably the statics initializer needs the descriptors it names
emitted into the script's file, the way the script's own types are. Found the
first time the C backend ran over the `interp/` corpus, by `engine_tests`.
