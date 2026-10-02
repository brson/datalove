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
- [A specialized function keeps an original nobody calls](#user-content-a-specialized-function-keeps-an-original-nobody-calls)
- [The loop check asks where a binding ended up, not whether its move repeats](#user-content-the-loop-check-asks-where-a-binding-ended-up-not-whether-its-move-repeats)

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

`FuncIdentity` and `FunctionKey::of` key on the unit a local reference belongs
to, and take it from the registry's count, which is the index the running unit
will have. That is the same number the compiler uses for a later unit's
`CodeRef::External`, so the two agree -- as long as a failed unit does not
disturb the counting.

Closing it means the compiler's unit numbering and the runtime's agreeing about
failed units, which `CodeRef::External` already assumes today. Worth examining
as its own question rather than patching the key.

## A specialized function keeps an original nobody calls

**By design, and wrong for one of the two cases.**

Const parameter specialization is additive: the original function stays and a
copy per instantiation is added beside it. That is what makes a call site the
pass cannot see -- a script unit's, compiled later -- still have something to
call.

An AOT build has no later script unit. There, a function whose every call site
was specialized keeps an original that nothing reaches, and it is emitted.
Dead-code elimination over the module graph would remove it; nothing does that
today.

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
