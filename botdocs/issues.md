# Issues

Known faults and gaps that are not being worked on right now, so that they are
written down somewhere other than a conversation.

This is for things that are *wrong* or *missing*, not for design questions,
which belong in a plan or a design doc. An entry says what breaks, what it
costs, what it would take, and whether it has been reproduced or only reasoned
about -- the last of those matters, and entries should say so plainly.

Entries are grouped by where the fault is, most severe first within a group.
Remove an entry when it is fixed. A fixed issue lives in the commit that fixed
it and in whatever doc describes the working thing; it does not need a second
home here.

## Contents

- [Engines and the REPL: failing the wrong way](#user-content-engines-and-the-repl-failing-the-wrong-way)
  - [A script const that calls a function declared after it panics](#user-content-a-script-const-that-calls-a-function-declared-after-it-panics)
  - [A module const that recurses forever panics](#user-content-a-module-const-that-recurses-forever-panics)
  - [A REPL unit that fails keeps what it did to earlier units](#user-content-a-repl-unit-that-fails-keeps-what-it-did-to-earlier-units)
  - [A unit that did not run still gives away what it would have taken](#user-content-a-unit-that-did-not-run-still-gives-away-what-it-would-have-taken)
  - [Deep recursion aborts the process off Linux](#user-content-deep-recursion-aborts-the-process-off-linux)
  - [Compile-time evaluation has no limits](#user-content-compile-time-evaluation-has-no-limits)
  - [The JIT has no way back from a full arena](#user-content-the-jit-has-no-way-back-from-a-full-arena)
  - [The REPL shows a broken module only as a message](#user-content-the-repl-shows-a-broken-module-only-as-a-message)
  - [Lowering errors say what, never where](#user-content-lowering-errors-say-what-never-where)
  - [An edit to a module no unit requires reports nothing](#user-content-an-edit-to-a-module-no-unit-requires-reports-nothing)

- [The language and its checks](#user-content-the-language-and-its-checks)
  - [A borrow into a collection outlives a later `mut` of it](#user-content-a-borrow-into-a-collection-outlives-a-later-mut-of-it)
  - [The loop check asks where a binding ended up, not whether its move repeats](#user-content-the-loop-check-asks-where-a-binding-ended-up-not-whether-its-move-repeats)
  - [A `let` that fails to typecheck leaves its name undefined](#user-content-a-let-that-fails-to-typecheck-leaves-its-name-undefined)
  - [No table module, and none can be written](#user-content-no-table-module-and-none-can-be-written)
  - [Nothing takes a value back out of data or error](#user-content-nothing-takes-a-value-back-out-of-data-or-error)
  - [One function per name, so no prelude of imports](#user-content-one-function-per-name-so-no-prelude-of-imports)
  - [Nothing names another module's type](#user-content-nothing-names-another-modules-type)
  - [What writing the store demo ran into](#user-content-what-writing-the-store-demo-ran-into)
  - [A printed type loses its alias](#user-content-a-printed-type-loses-its-alias)
  - [`(T)` is read as a tuple of one](#user-content-t-is-read-as-a-tuple-of-one)

- [The typecheckers: cost and limits](#user-content-the-typecheckers-cost-and-limits)
  - [Deeply nested literals overflow the stack](#user-content-deeply-nested-literals-overflow-the-stack)
  - [Datafun records a copy of every expression's type](#user-content-datafun-records-a-copy-of-every-expressions-type)

- [Backends and performance](#user-content-backends-and-performance)
  - [A generic's descriptor-table cache holds one binding](#user-content-a-generics-descriptor-table-cache-holds-one-binding)
  - [No inlining in Cranelift, and the C backend is built at -O0](#user-content-no-inlining-in-cranelift-and-the-c-backend-is-built-at--o0)
  - [The C backend emits code the size of the data](#user-content-the-c-backend-emits-code-the-size-of-the-data)
  - [A script's data const is copied into the unit that declares it](#user-content-a-scripts-data-const-is-copied-into-the-unit-that-declares-it)
  - [The allocator maps a page per large block and keeps every small page](#user-content-the-allocator-maps-a-page-per-large-block-and-keeps-every-small-page)

- [Compilation pipeline and incrementality](#user-content-compilation-pipeline-and-incrementality)
  - [Pruning stops at the module](#user-content-pruning-stops-at-the-module)
  - [Nothing consumes a WorkspaceDelta](#user-content-nothing-consumes-a-workspacedelta)
  - [No batch of scripts against one world](#user-content-no-batch-of-scripts-against-one-world)
  - [Every REPL unit is a new salsa input](#user-content-every-repl-unit-is-a-new-salsa-input)

- [Testing](#user-content-testing)
  - [Nothing proves the suite runs with leak checking on](#user-content-nothing-proves-the-suite-runs-with-leak-checking-on)

## Engines and the REPL: failing the wrong way

Where running a program crashes, aborts, leaks or answers wrongly instead of reporting an error, in the interpreter's engines, the JIT, compile-time evaluation and the REPL session around them.

### A script const that calls a function declared after it panics

**Reproduced.**

```datalove
const C: i64 = f()
fun f(): i64
  ret C
end fun
debuglog C
```

`datalove script` panics at `datalove-datafun-lower/src/context.rs:489`,
"local function 'f' not found - typechecker should catch this", and a REPL
unit like it takes the engine down. The typechecker accepts the cycle. Script
lowering defers a function whose body names a const not yet evaluated, so `f`
is not among the functions handed to const lowering, and the script path passes
`callable: None`, so the `Cycle` check that catches the module form never runs.
The `unreachable!` at the `ConstError::Cycle` arm of the script compiler's
const evaluation ("no script function waits on one") stopped being true when
script deferral was added. With `fun f` first, the typechecker reports `C` as
not found, so only the const-first order panics.

The module form reports `const 'C': depends on a function that is not available
yet ...`, with no span and no module path. A cycle between a const and the
functions it calls is a name-resolution fact, and is the place to report it,
at the const.

### A module const that recurses forever panics

**Reproduced, by a survey of lowering errors; not tried again since.** A
module const whose initializer recurses without end panics in
`datalove-datafun-interp/src/bytecode/call.rs:365`, "RefCell already
borrowed", reached through `resolve_callee`. The same const in a script
reports `failed to lower const 'C': stack overflow`, which is an error, if
mislabelled as lowering; see
[Compile-time evaluation has no limits](#user-content-compile-time-evaluation-has-no-limits).

### A REPL unit that fails keeps what it did to earlier units

**Reproduced.**

```datalove
var p = (1, "a")
---
let xs: [u32] = []
---
set p.1 = "mutated"
let x = xs[(: index / 5)]!
```

The third unit returns early, and so stays in the session offering none of its
bindings. But `p` reads `(1, "mutated")` afterwards: a `set` or an `out`
argument writes into an earlier unit's frame, and nothing puts the write back
when the unit that made it fails. A move out of an earlier unit's binding before
the `!` is the same: the value is gone. So a failed unit influences the
session, which the model in `script-reactivity-architecture.md` says it does
not.

It lasts until something re-derives the unit: its record lists what it wrote
and took, and the rerun of `script-reactivity-architecture.md` section 5 builds
those bindings again before running it. What is missing is doing the same when
the unit fails, and the obvious way does not work: rerunning from the provider
reruns everything that uses the binding after it, which includes the failed unit, and it
fails again having done the same. Copying the places before the unit runs and
restoring them would work, but only for types that can be cloned, and moves
across units were made real so as not to depend on that.

### A unit that did not run still gives away what it would have taken

**Reproduced.**

```datalove
let xs: [u32] = []
---
let ys = [1, 2]
---
let i = xs[(: index / 5)]!
---
let zs = (i, ys)
---
let w = ys
```

The fourth unit reads `i` from the third, which returned early, so it is
blocked and never runs; `ys` is still in the environment as `[1, 2]`. But the
fifth unit is refused with D013, "`ys` was given away by an earlier unit",
because the fourth unit's record says it takes `ys`. A unit that returns early
before its move does the same. The help, to clone `ys` where it was given away,
names no place and the place never ran.

Whether a binding is given away is decided statically, from records, and a unit
that compiled keeps its full record whether or not it ran. The cost beyond the
wrong error is a wasted rerun: re-deriving such a unit reruns from the provider
of what it would take. Fixing it means the records of a unit that did not run
to its end not counting as having taken anything, and then the units after it
being re-derived when it runs -- which the reach does already, since they use
the names it takes. A unit that returned early part way through a move is the
hard half: the runtime knows whether the binding was taken, the record does not.

### Deep recursion aborts the process off Linux

**Reproduced on Linux; the rest reasoned.** On Linux every engine stops
unbounded recursion with an error:

- The bytecode bounds its own frame stack.
- The IR walker checks the native stack pointer at each call against
  `dtlv_rti_native_stack_limit`: 1 MiB above the thread's stack bottom, from
  `pthread_getattr_np`, or a quarter of a smaller stack.
- Compiled code compares its stack pointer with `RtHeader::stack_limit` on
  entering each function. The JIT's abandons back to the interpreter, which
  reports `stack overflow`. AOT code, with nothing under it to return to,
  calls `dtlv_rti_stack_overflow`, which prints `Error: stack overflow` and
  exits with status 1. `dtlv_rti_init` sets the limit.
- `deep_recursion_tests` and `jit_path_tests` check all of these.

What is left:

- **Off Linux there is no limit.** The thread's stack is found only through
  `pthread_getattr_np`, so elsewhere every engine but the bytecode aborts or
  segfaults as before. macOS has `pthread_get_stackaddr_np`. On wasm the call
  stack is the engine's and out of reach, so the IR walker there runs until
  the engine traps; a call-depth count in the walker is what would bound it.
- **AOT on another thread.** The limit is the thread's that called
  `dtlv_rti_init`. An AOT executable runs on that thread, but a runtime used
  from another thread would check against the wrong stack.
- **Panics under compiled code abort.** `__jit_dispatch_call` is `extern "C"`.
  An error comes back through the abandonment protocol, but a *panic* in the
  interpreter under compiled code aborts the process, where without the JIT
  the REPL's `catch_unwind` would have caught it.
- **Unchecked native recursion:** the runtime's destroy, clone, compare and
  print recurse as deep as a value nests. Without recursive types that is
  bounded by the type.
- **No `enable_probestack`** in either Cranelift backend. The entry check has
  a 1 MiB margin, so only a single frame larger than that could step past the
  guard page.

### Compile-time evaluation has no limits

**Reproduced.** CTFE runs the ordinary interpreter (`InterpCtfeEvaluator`) with
no step limit and no allocation limit. A const calling a function that never
terminates hangs `datalove script-ir`, which only compiles. Unbounded
recursion is caught, by the IR walker's stack check, and reported as `const
evaluation error: failed to lower const 'C': stack overflow`.

Errors are reported thinly too. A const whose expression early-returns says
`h::X: early return: const expression returned early via ! or ?` as a lowering
error: the error value is discarded, and there is no span and no evaluation call
stack.

What it would take: a step budget in the interpreter, checked
only when evaluating a const, each failing as a diagnostic on the const; and
carrying the returned error and the call chain into that diagnostic.

### The JIT has no way back from a full arena

**Reasoned from the code.** The JIT puts code and data in one 64 MB arena
reserved at start (`JIT_ARENA_SIZE`), and nothing compiled is ever freed.
Bodies replaced during a session leave their old code behind, and their code
cells stay in `JitCompiler::retired_cells` because old stubs hold their
addresses. When the arena is full, compiling fails as `CompilationFailed`.
That failure is recognised by the text "region exhausted" (`jit_err`), and the
call that asked fails with a `RuntimeError`. The program stops, where it could
have gone on interpreted.

Making it a refusal needs to know the `JITModule` is still usable after a
failed definition. Dropping the engine and starting a fresh one is the
heavier alternative. The CLI's REPL does not use the JIT, so a long session
does not reach this today.

### The REPL shows a broken module only as a message

**Reproduced.** A unit that requires a module that does not compile fails with
``module `x` does not compile: ...``, the module's errors as messages. The
module's diagnostics, framed against its source, are what the engine keeps in
`ModuleErrors`, and only the CLI renders them; the REPL's Detail tab shows the
unit's own diagnostics, which say nothing of the module's.

### Lowering errors say what, never where

**Mostly reproduced; which variants are dead was reasoned about.** No error
raised after ownership analysis -- in lowering, const evaluation or
specialization -- carries a span or a source. The most one has is a name: a
binding, a function, or a `func::const` prefix. The CLI prints a script's as
`Lowering error: {message}` and a module's bare, without even the module's
path, and the REPL's Detail tab shows only the message.

They should mostly not be lowering errors at all. What reaches users:

- **A name bound by a unit that did not compile.** `BindingNotAvailable`,
  which the script compiler words as ``"`t` is bound by unit 0, which did not
  compile"``. The typechecker offers later units the names of a unit that
  failed ownership analysis; it should report the use, with its span, or not
  offer the name.
- **A cycle between a const and a function it calls.** The module form is an
  error with no location; the script form panics, above. Name resolution's.
- **A const that returns early through `!` or `?`**, reported as `const 'C'
  early-returned` or `CTFE error: early return`. A real compile-time failure,
  but it belongs at the const's span.
- **A const that overflows the stack**, reported as `failed to lower const
  'C': stack overflow` -- the label is wrong too.
- **Over 64 instantiations of a function's const parameters**, an
  implementation limit, which names the function and not the call that went
  over.

`LowerError` has only `BindingNotAvailable` left. The lowering crate also has
about a hundred panicking checks, some saying "typechecker should catch this";
the const cycle above is one of them being hit.

The fix is per error, in the phase that should have caught it, with a span;
what is left in lowering then is the compiler's own bugs, for which a message
with no span is fine.

### An edit to a module no unit requires reports nothing

**Reproduced.** The REPL engine compiles only the modules the session's units
require (see [repl-architecture.md](repl-architecture.md)), so
`Engine::edit_module` on any other module compiles nothing. An edit that
breaks one comes back with no unit reports, nothing in `module_errors()` and no
diagnostics. The error surfaces only when a unit later requires the module, and
then as that unit's failure: ``module `local/test/m` does not compile: ...``. Not
wrong as semantics -- a module nothing requires fails nothing -- but a reader
editing a module learns nothing about the edit.

The capability would be an `Engine::check_module(path)` that compiles the
module, with what it requires, beside the session's roots and puts the roots
back, touching neither the executor nor `module_errors()`, and an `edit_module`
that returns that check with its unit reports. The check is the same shape as
`ModuleErrors`, messages and stored diagnostics, so the CLI renders either.
How the interactive REPL shows it is the open part; it has no module editing
yet, so nothing calls `edit_module` but the tests.

## The language and its checks

Missing features, and the checker refusing what it should accept or accepting what it should refuse.

### A borrow into a collection outlives a later `mut` of it

**Reproduced.** An operand that names an element of a collection is read
through a reference into it, taken when the operand is evaluated, and nothing
stops a later operand of the same expression from taking the collection
`mut` and reallocating it:

```datalove
var xs = [100000000000000000000, 5]
let y = xs[(: index / 0)]? + grow(mut xs)   // grow pushes until xs moves
```

The addition reads the element through the dangling reference, and gives a
garbage number rather than failing. The same holds for any pair of operands,
call arguments aside (which `check_argument_aliasing` covers): a reference
taken by an earlier operand, then a `mut` or consuming use of its root by a
later one. `set` no longer has a case of it, since it runs all of its own code
before it reaches its place, but `set xs[f(mut xs)]? += xs[1]?` still does:
the value's reference into the list is taken, then the key's call grows it.
Its IR shows the order; run, it read the freed buffer, which happened still
to hold the right number.

The fix is an ownership rule: within an expression, a binding an earlier
operand borrows by reference can't be taken `mut` or moved by a later one, as
Rust's borrow checker refuses it. The alternative, reading such an operand by
copy, would cost a clone of every linear element an operator reads.

### The loop check asks where a binding ended up, not whether its move repeats

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
ended up. One thing depends on the present rule and would have to move with it:
the loop-exit merge (`merge_loop_exits`) takes the state before the loop as the
state at a condition-failure exit. That is sound only because `MoveInLoop`
guarantees the end of the body agrees with it for every outer non-copy binding.
Relax the rule and that guarantee goes, and the exit merge is unsound rather
than imprecise.

Loop exits themselves are settled: see `merge_loop_exits`, D014, and
`std_tests/152_owned_past_a_loop_exit`.

### A `let` that fails to typecheck leaves its name undefined

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

### No table module, and none can be written

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

### Nothing takes a value back out of data or error

**Checked against the spec and the parser.** `data x` and `error x` wrap any
value, and nothing in the language unwraps one: there is no type test, no cast,
no type pattern in `match`. `is` parses only in a `where` bound. So a `data` can
be moved, cloned, compared and printed, and an `error` can be printed, but code
cannot recover what is inside.

The candidate syntaxes considered were a type-testing `if` (`if d is u32 |x|`),
`as` returning a result with `as!` panicking, and type patterns in `match`
(`case : u32 |x|`). Unchosen, along with whether the test is structural or
nominal.

### One function per name, so no prelude of imports

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

### Nothing names another module's type

**Reproduced.** A module's `type` alias cannot be named outside it. A qualified
name does not parse in any type position -- `let a: qt.Shape`,
`const A: qt.Shape` and a parameter `s: qt.Shape` are all parse errors -- and
`import qt.Shape` looks for a function and reports F002. So a value of another
module's named type can only be annotated by writing the type out in full, as
`std_tests` and `engines/spec_033_comptime_collections` do.

The typechecker already records each module's `exported_type_aliases`; what is
missing is a way to reach them, either a qualified type name or an `import`
that takes types as well as functions.

### What writing the store demo ran into

**Reproduced**, each while writing `demos/store`, about 800 lines over three
data files. None stopped it; each cost a detour.

- **Nothing prints a string as it is.** `debuglog` is the only output, and it
  quotes and escapes a string, so the report prints every line in quotes.
- **A const cannot fail, and nothing can panic.** A module const built by a
  function that uses `!` has to unwrap the result itself, and the `else`
  branch needs a value it will never return. `db.product_slots` returns an
  empty map there and says why.
- **An `index` does not widen with `@`,** so counting into a `u64` takes
  `u64.from_index`, where a `u32` widens with `@`.
- **Updating a collection held in a map by reading it out copies it.** The
  natural `var s = map.get_or(ref m, ref k, #{})`, change `s`, `map.insert(mut
  m, k, s)` clones the whole collection out and destroys the old one, every
  time. `people.by_country` did that for a set of customers per country on
  every sale, and it was 14% of the interpreter's run. In place it is `set
  m[k@] = #{}` when absent, then `set.insert(mut m[k]!, x)`. Nothing points
  the writer from the first to the second.

## The typecheckers: cost and limits

Where checking a datalit or datafun input takes time or stack out of
proportion to its size.

### A printed type loses its alias

**Reproduced.** `type Color: enum { atom Red, atom Blue }` and a `let c: Color`
show `c` as `enum { atom Red, atom Blue }`, in the REPL and in every message.
The typechecker expands an alias into what it names, and nothing after the
parse keeps the name: `TypeHint::Alias` is the last form with one. Showing the
alias would take carrying it beside the expansion, in `datalit::tycheck::Type`
or in a side table, as far as `TypeSyntax`.

### `(T)` is read as a tuple of one

**Reproduced.** A tuple type written `(T)` parses as the tuple of one, the same
as `(T,)`. A value is the other way: `(x)` is `x` in parentheses, a tuple of one
is `(x,)`, and a pattern `(x)` is refused (P070). The type printer writes
`(T,)`, so what is printed reads the same either way; whether `(T)` should be
refused as a pattern is, or read as `T` in parentheses, is the open part.

### Deeply nested literals overflow the stack

**Reproduced, debug build.** A list nested 700 deep, `[[[ ... 1 ... ]]]`,
aborts `datalove lit-tycheck` with "thread 'main' has overflowed its stack".
At 400 deep it checks. `datalove lit-ast` parses the 700-deep file, so the
datalit parser gets past the point where the checker fails, and the checker's
recursion in `synthesize` and `check`
(`crates/datalove-datalit/src/tycheck/`) is what runs out. The same list as
`let x = [[[ ... ]]]` in a datafun script aborts at 700 and runs at 400; which
phase overflows there is not identified. The release-build threshold is not
measured.

Nothing in the parsers or checkers limits nesting, so a file of a few
kilobytes crashes the process instead of producing an error. Data files are
the exposed case: they are written by programs, and a `require data` of one
aborts the compiler.

The fix is either a nesting limit with a diagnostic, which the datalit spec
would then have to state, or an explicit work stack in place of recursion in
each phase that walks the expression. The runtime already bounds recursion
against the native stack (`dtlv_rti_native_stack_limit`, see
[Deep recursion aborts the process off Linux](#user-content-deep-recursion-aborts-the-process-off-linux)),
but the front end has nothing like it. Inference, which
[One Rule for Literals That Share a Type](plan-literal-typing.md) added to
both checkers, recurses on the expression too, and would need the same
treatment.

### Datafun records a copy of every expression's type

**Reasoned from the code, not measured.** `TypeContext::store_expr_type`
clones the full `Type` of every expression it records. In a nested literal
each level's type contains the type of everything below it, so a literal
nested `d` deep records `O(d²)` of type for `O(d)` of input, and in general
`O(n × d)`. The [stack overflow](#user-content-deeply-nested-literals-overflow-the-stack)
stops the program at a depth where this is still too small to measure. Once
that is fixed, it is the next superlinear cost. Inference
([One Rule for Literals That Share a Type](plan-literal-typing.md)) keeps an
inferred type per expression as well, and where that type is known outright,
as a variable's is, it is a cloned `Type` the same way. Interned or shared
types would remove both.

## Backends and performance

Compiled code that is wrong in a corner, or slower or larger than it needs to be.

### A generic's descriptor-table cache holds one binding

**Reasoned about, not measured.** Every engine fills a generic's descriptor
table on entry through a cache of one: the last call's type-parameter
descriptors and the table they built (`IrLayout::table_cache` in the
interpreter, a `static` per function in C, a data object per function in
Cranelift). A call at the same binding as the last costs a compare per type
parameter; any other rebuilds the table, instantiating every template entry.

So a generic called alternately at two bindings rebuilds on every call. The
plainest case is recursion at a growing type, `f<T>` calling `f<[T]>`, where no
two consecutive calls share a binding; a hot loop calling one generic at two
types is the other. It is a cost only, never a wrong answer.

A few entries keyed by binding would cover both. The compiled caches are also
plain statics, written without synchronization, which is fine with one runtime
per process and wrong with two on different threads; a per-runtime cache, as
the instantiation cache already is, would fix that too.

### No inlining in Cranelift, and the C backend is built at -O0

**Reasoned from the code; the speedups are measured.** Gaps in what the
compiled tiers do, each cheap to state and none started. (Another, that the
JIT could not see a loop in a function called once, is fixed by OSR; see
[Tuning the JIT](plan-jit-tuning.md#user-content-osr).)

- **No inlining in Cranelift.** Nothing implements
  `cranelift_codegen::inline::Inline` or calls `Context::inline`, so neither the
  jit nor the cranelift AOT backend inlines. A call between two compiled
  functions is an indirect call through the callee's code cell
  (`JitCompiler::code_cells`), read at the call site; only a call to a function
  not yet compiled reaches its stub and `__jit_dispatch_call`.
- **The C backend runs `cc -std=c11 -O0 -g`** (`pipeline/c_aot.rs`, and the
  same in `engine_tests.rs`). `-O2` measured 8.4x on `loop_arith`, the largest
  number in the jit report. The work is whatever undefined behaviour `-O2` exposes
  in the emitted C, which the four-backend differential suite would find.

### The C backend emits code the size of the data

**Reproduced** with the store demo: at 200,000 orders `datalove aot-compile
--c --link` writes about 200 MB of C, and `cc1` is killed for want of memory
after nearly four minutes. At 20,000 orders it builds and runs. Every element
of a `require data` const is built by statements of its own in
`__dtlv_statics_init`, so the source, and what the C compiler holds while
compiling it, grows with the data. The Cranelift AOT backend does the same in
machine code -- the 200,000-order executable is 36 MB and takes 30 seconds to
build -- but gets through it. Emitting the data as initialized bytes and
building from them in a loop would make both proportional to the types rather
than the values.

### A script's data const is copied into the unit that declares it

**Reasoned, not measured.**

Consts are shared as `Arc<ConstValue>` from where they are resolved to the
`StaticRef` a reference lowers to, so naming one costs a pointer copy. What is
still a copy is the declaration in a script unit: a script const is lowered as
a binding whose initializer is a `Const` instruction holding the value, and
`Instruction::Const` owns its `ConstValue`. For a `require data` that is two
copies of the dataset per declaring unit -- lowering the `DataFile` expression,
and const inlining writing the evaluated value over it -- and the unit builds
the value once at run time where its other references share the static.
Module consts have no declaring instruction and are not affected.

Salsa also hashes a module's consts whole wherever they are a tracked
function's argument (`lower_module_functions`, `ModulePreResolvedConsts`), once
per compile of the module. That is a hash, not a copy.

### The allocator maps a page per large block and keeps every small page

**Reasoned from `datalove-rt/src/c/impls/alloc.rs`; none of it measured as a
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

## Compilation pipeline and incrementality

What the driver and salsa layers recompute, keep or cannot yet do.

### Pruning stops at the module

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

### Nothing consumes a WorkspaceDelta

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

### No batch of scripts against one world

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

### Every REPL unit is a new salsa input

**Reasoned from the code.** `ScriptCompiler::compile_fragment` and
`compile_expr` mint a `bct::input::Source` per call, and salsa never collects
inputs, so a session grows by one input (and its text) per unit, forever. Small
per unit; unbounded in a long session. Editing a unit already goes through a
stable per-unit `Source` and `set_text`; appending does not.

The same minting means a fresh compiler recompiling a session's units from
scratch would reuse nothing, since identical text is a different input. Nothing
does that today -- `Engine::reset` after a panic starts an empty session rather
than recompiling the old one -- so that half only matters if recovery is ever meant to keep the
session. Holding a stable `Source` per unit for appends too would fix both.

## Testing

What the suite does not check.

### Nothing proves the suite runs with leak checking on

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
