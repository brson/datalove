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

- [Engines and the REPL: failing the wrong way](#engines-and-the-repl-failing-the-wrong-way)
  - [A REPL unit that fails keeps what it did to earlier units](#a-repl-unit-that-fails-keeps-what-it-did-to-earlier-units)
  - [A unit that did not run still gives away what it would have taken](#a-unit-that-did-not-run-still-gives-away-what-it-would-have-taken)
  - [Deep recursion aborts the process off Linux](#deep-recursion-aborts-the-process-off-linux)
  - [Compile-time evaluation has no limits](#compile-time-evaluation-has-no-limits)
  - [The JIT has no way back from a full arena](#the-jit-has-no-way-back-from-a-full-arena)
  - [An edit to a module no unit requires reports nothing](#an-edit-to-a-module-no-unit-requires-reports-nothing)

- [The language and its checks](#the-language-and-its-checks)
  - [The loop check asks where a binding ended up, not whether its move repeats](#the-loop-check-asks-where-a-binding-ended-up-not-whether-its-move-repeats)
  - [A non-copy part of a temporary can only be borrowed](#a-non-copy-part-of-a-temporary-can-only-be-borrowed)
  - [A `let` with no type that fails to typecheck leaves its name undefined](#a-let-with-no-type-that-fails-to-typecheck-leaves-its-name-undefined)
  - [No table module, and none can be written](#no-table-module-and-none-can-be-written)
  - [Nothing takes a value back out of data or error](#nothing-takes-a-value-back-out-of-data-or-error)
  - [One function per name, so no prelude of imports](#one-function-per-name-so-no-prelude-of-imports)
  - [Nothing names another module's type](#nothing-names-another-modules-type)
  - [What writing the store demo ran into](#what-writing-the-store-demo-ran-into)
  - [A printed type loses its alias](#a-printed-type-loses-its-alias)
  - [`(T)` is read as a tuple of one](#t-is-read-as-a-tuple-of-one)
  - [T065 refuses a recursive key only where it is written](#t065-refuses-a-recursive-key-only-where-it-is-written)
  - [A list with a recursive field checks in one order and not the other](#a-list-with-a-recursive-field-checks-in-one-order-and-not-the-other)

- [The typecheckers: cost and limits](#the-typecheckers-cost-and-limits)
  - [Deeply nested literals overflow the stack](#deeply-nested-literals-overflow-the-stack)
  - [Datafun records a copy of every expression's type](#datafun-records-a-copy-of-every-expressions-type)

- [Backends and performance](#backends-and-performance)
  - [Cranelift refuses a module function whose end nothing reaches](#cranelift-refuses-a-module-function-whose-end-nothing-reaches)
  - [A generic's descriptor-table cache holds one binding](#a-generics-descriptor-table-cache-holds-one-binding)
  - [No inlining in Cranelift, and the C backend is built at -O0](#no-inlining-in-cranelift-and-the-c-backend-is-built-at--o0)
  - [The C backend emits code the size of the data](#the-c-backend-emits-code-the-size-of-the-data)
  - [The C backend calls a runtime function it never declares](#the-c-backend-calls-a-runtime-function-it-never-declares)
  - [A script's data const is copied into the unit that declares it](#a-scripts-data-const-is-copied-into-the-unit-that-declares-it)
  - [The allocator keeps every block it has ever handed out](#the-allocator-keeps-every-block-it-has-ever-handed-out)

- [Compilation and incrementality](#compilation-and-incrementality)
  - [Pruning stops at the module](#pruning-stops-at-the-module)
  - [Nothing consumes a WorkspaceDelta](#nothing-consumes-a-workspacedelta)
  - [No batch of scripts against one world](#no-batch-of-scripts-against-one-world)
  - [Every REPL unit is a new salsa input](#every-repl-unit-is-a-new-salsa-input)

- [Testing](#testing)
  - [The cli tests run with leak checking on, which no user does](#the-cli-tests-run-with-leak-checking-on-which-no-user-does)

## Engines and the REPL: failing the wrong way

Where running a program crashes, aborts, leaks or answers wrongly instead of reporting an error, in the interpreter's engines, the JIT, compile-time evaluation and the REPL session around them.

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
fifth unit is refused with O013, "`ys` was given away by an earlier unit",
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
recursion is caught, by the IR walker's stack check, and reported as C002 at
the const.

A failed const's diagnostic says what it returned, but not the calls that got
there: the IR has no spans, so an evaluation call stack could only be names.

What it would take: a step budget in the interpreter, checked only when
evaluating a const, failing as a C-code diagnostic on the const; and the
interpreter's frames carried into the diagnostic as a chain of calls.

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

### The loop check asks where a binding ended up, not whether its move repeats

**Reproduced.** A spurious O007 on a loop that re-initializes before it moves.

```datalove
var s = "a"
loop while i .< 3
  set s = "b"        // s is Live again here
  let t = s          // O007 -- but no iteration reads a moved s
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

Loop exits themselves are settled: see `merge_loop_exits`, O014, and
`std_tests/152_owned_past_a_loop_exit`.

### A non-copy part of a temporary can only be borrowed

`f().name` and `f()[0]?` with a non-copy part read as a value are F070 and
F072, as the same reads of a place are, so a program has to borrow the part
or clone it out with a written `@`. Nothing holds a temporary afterwards, so
refusing is stricter than ownership needs, but each way of allowing it does
something the program did not say. Cloning the part hides a clone, which
`@` is meant to mark. Moving it out works for a field, the other fields
dropped beside it, but for an element it means destroying the rest of the
collection to keep one element, which is no meaning a reader would guess.
Until there is a rule worth having, the read is refused.

### A `let` with no type that fails to typecheck leaves its name undefined

**Reproduced** with the debug cli. When a `let` or `var` with no type written
fails to typecheck, the binding is never added, so every later use of the name
reports a second error, F001 "cannot find value", under the real one:

```datalove
let s: (string, i32) = ("a", 1)
let r = s.0       // F070, a non-copy field read out
debuglog r        // F001 cannot find value `r`
```

The cause is `check_variable_decl` in
`crates/datalove-datafun-tycheck/src/statement.rs`: without a type hint it
calls `bind_pattern` only when the value synthesized. It applies to every kind
of failure, not just F070 and F072. A declaration with a hint binds the name at
the hint's type whether or not the value fits it (`error/119`).

Without a hint there is no type to give the name, so the fix needs a type that
stands for "already reported", one that every check accepts silently, so uses of
the name neither fail nor report again. The typechecker has no such type:
`datalit::tycheck::Type::Error` is the language's `error` type, not a poison
type.

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
fields` today, and `t[i]?` is F103, because indexing wants a list, map or
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

### A printed type loses its alias

**Reproduced.** `type Color = enum { atom Red, atom Blue }` and a `let c: Color`
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

### T065 refuses a recursive key only where it is written

**Reproduced.** `let t: #{L} = #{}` with `L` recursive is T065, but
`let s = #{x@}` with `x: L` is accepted and runs, and so does
`%{x@ -> 1}`: the check is made of hints as they are converted, and an
inferred type is never converted. A generic `fun one<T>(x: T): #{T} where
{ T is ord, }` called with an `L` builds a set of `L` the same way, since
`ord` admits every type. The runtime orders a recursive value as any other,
so nothing breaks; what the rule leaves open is whether recursive keys are
meant to be allowed, in which case T065 can go, or refused, in which case
inference and `ord` have to refuse them too. It is also why the check-once
theorem in [formal.md](formal.md) leaves recursive type arguments out: an
instance that spells one into a key is T065 where its generic was not.

### A list with a recursive field checks in one order and not the other

**Reproduced.** `[: { next: rec L [L] } / { next = [] }, { next = [] }]`
is a `[{ next: rec L [L] }]`, but `[{ next = [] }, : { next: rec L [L] } /
{ next = [] }]` is T040. A struct's field holds its parts with no variable
between, so the first element's field comes to `rec L [L]`'s unfolding,
`[rec L [L]]`, and the list's element type is `{ next: [rec L [L]] }`. The
second element's hint is then compared with that by `≈`, which unfolds a
`rec` at the head only, and `{ next: rec L [L] }` and `{ next: [rec L [L]]
}` differ inside a field. With the hint on the field itself, as in the
datalit typing rules' example, the head is the field and `≈` accepts it,
though the list's type still depends on the order. A comparison of the two
orders over twenty thousand generated pairs found nothing else that
depended on order.

So the typing rules' claim that the solution does not depend on order holds
only without `rec`, and is tested only for that, not proved (`formal.md`). The fix that
keeps equality simple is a canonical form: fold every type, when it is
built, to one spelling of each recursive type, so that `==` is equivalence
up to unfolding and the two lists have one type. The cost is one
normalization per type, near linear; the backend's layout of the two
spellings is what to check first. Deep equivalence in `≈` alone would leave
derived `Eq` and `Hash`, and the runtime's descriptors, telling the
spellings apart.

## The typecheckers: cost and limits

Where checking a datalit or datafun input takes time or stack out of
proportion to its size.

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
[Deep recursion aborts the process off Linux](#deep-recursion-aborts-the-process-off-linux)),
but the front end has nothing like it. Inference, which
[One Rule for Literals That Share a Type](plan-literal-typing.md) added to
both checkers, recurses on the expression too, and would need the same
treatment.

### Datafun records a copy of every expression's type

**Reasoned from the code, not measured.** `TypeContext::store_expr_type`
clones the full `Type` of every expression it records. In a nested literal
each level's type contains the type of everything below it, so a literal
nested `d` deep records `O(d²)` of type for `O(d)` of input, and in general
`O(n × d)`. The [stack overflow](#deeply-nested-literals-overflow-the-stack)
stops the program at a depth where this is still too small to measure. Once
that is fixed, it is the next superlinear cost. Inference
([One Rule for Literals That Share a Type](plan-literal-typing.md)) keeps an
inferred type per expression as well, and where that type is known outright,
as a variable's is, it is a cloned `Type` the same way. Interned or shared
types would remove both.

## Backends and performance

Compiled code that is wrong in a corner, or slower or larger than it needs to be.

### Cranelift refuses a module function whose end nothing reaches

**Reproduced.** A module function returning a value whose body ends in a
`loop` it leaves only by `ret` -- `var i: int = 0`, then `loop`, `set i = i +
1`, `if i == 3`, `ret none` -- fails under the engine tests' JIT with
"verifier errors: return: arguments of return must match function
signature". Lowering ends every body that does not end in a `ret` with
`return` and no value (`lower_function_body`), here in the block after the
loop, which nothing jumps to; the interpreters never get there, but
Cranelift compiles every block and verifies that one. A script's function of
the same shape ran under `datalove script --jit`, so something on the way
there removes the block or does not compile it; not looked into.

What it costs: such a function cannot be compiled by Cranelift, the JIT's or
presumably the AOT's, though the second has not been tried. A `break` that
makes the end reachable avoids it, which is what engine fixture 1049 does.
Either lowering leaves out a block nothing reaches, or the Cranelift codegen
compiles only the blocks the entry reaches.

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
[Tuning the JIT](plan-jit-tuning.md#osr).)

- **No inlining in Cranelift.** Nothing implements
  `cranelift_codegen::inline::Inline` or calls `Context::inline`, so neither the
  jit nor the cranelift AOT backend inlines. A call between two compiled
  functions is an indirect call through the callee's code cell
  (`JitCompiler::code_cells`), read at the call site; only a call to a function
  not yet compiled reaches its stub and `__jit_dispatch_call`.
- **The C backend runs `cc -std=c11 -O0 -g`** (`driver/c_aot.rs`, and the
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

### The C backend calls a runtime function it never declares

**Reasoned about, not reproduced as a failure.** C AOT emits a call to
`dtlv_rti_tensor_hyperplane_clone_local` (`c-aot/src/codegen.rs`, the row of a
tensor taken by index) but `c-aot/src/lib.rs` writes no `extern` prototype for
it, as it does for `dtlv_rti_tensor_set_local` and `_init_local`. The C
compiler takes it as an implicit declaration, which C99 removed and which
returns `int` where the function returns `RtStatus`. `for` over a tensor no
longer reaches it, which is why nothing fails; the fix is one more prototype.

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

### The allocator keeps every block it has ever handed out

**Reasoned from `datalove-rt/src/c/impls/alloc.rs`; not measured as a cost.**
The size classes go up to 128 KiB, and a freed block goes back on its class's
free list. Its mapping stays until shutdown. A program's footprint is therefore
its peak in each class, and up to twice that, since the classes are powers of
two. That is fine for a short process, but not for a REPL or anything
long-lived that has one large spike.

Each block of a class above a page is its own `mmap`. Because blocks are
reused, that is one system call per *live* block, not one per allocation.
Above 128 KiB each allocation is mapped on its own, and the last four freed
mappings (at most 32 MiB) are kept for reuse.

## Compilation and incrementality

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
`WorkspaceDelta::apply_to_compiler` applies its module half -- added to
`add_module`, removed to `remove_module`, changed to `update_source`, which keeps
salsa identity -- and refuses a delta changing riders or options, which need a new
module compiler (and, for riders, a rebuilt native component). Both are called only from
their unit tests. The REPL keeps its descriptor in step
(`Engine::set_workspace_module`, which says nothing reads it) but edits the
module compiler directly; no caller diffs two snapshots. So the delta path is built and
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

**Reasoned from the code.** The script engine mints a `bct::input::Source`
for each unit it inserts or appends and keeps it while the unit is in the
session, so an edit is a `set_text` on the unit's own input. But salsa never
collects inputs, so a deleted unit's input, and every unit's after a reset,
stays with its text for the life of the database: a session grows by one input
per unit it has ever held. Small per unit; unbounded in a long session.
`ScriptCompiler::compile_fragment` and `compile_expr`, which callers outside the
engine use, mint one per call the same way.

## Testing

What the suite does not check.

### The cli tests run with leak checking on, which no user does

**Reasoned.** `LeakCheckMode::from_env` defaults to `Ignore`, and the justfile
sets `DATALOVE_LEAK_CHECK=panic` on every test recipe
(`datalove-rt-tests/tests/leak_check_tests.rs` fails if one loses it). The cli
tests spawn the real binary with no `env_remove`, so they inherit `panic` and
test a configuration no user runs. Either clear the variable at the spawn sites
(the `Command::new`s in `datalove-cli/tests`, built inline, which is where a
helper would earn its place) or accept that they are not testing the shipped
default.
