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

- [No table module, and none can be written](#user-content-no-table-module-and-none-can-be-written)
- [A unit that fails part way through leaves its index to the next one](#user-content-a-unit-that-fails-part-way-through-leaves-its-index-to-the-next-one)
- [Inlining never triggers from a caller in an earlier unit](#user-content-inlining-never-triggers-from-a-caller-in-an-earlier-unit)
- [A field of a const cannot be projected, but a const can be destructured](#user-content-a-field-of-a-const-cannot-be-projected-but-a-const-can-be-destructured)
- [A specialized function keeps an original nobody calls](#user-content-a-specialized-function-keeps-an-original-nobody-calls)

## No table module, and none can be written

**Investigated, not a fault.** A table can be built, passed, cloned, dropped
and held inside anything (`std_tests/144_table_shapes`); what it has no
operations at all is `sys/std`. Two things stop a module being written.

**There is no way to say "any table".** A table's type carries its whole column
list -- `{| x: u32, y: string |}` -- and there is no row or column type
parameter and no `table` bound. So `fun len(ref self: ???)` has nothing to put
in the hole. A type parameter bound to a table does pass through a generic
unharmed, but a function that only forwards a value is not an operation:

```datalove
fun rows<T>(ref t: T): index    // compiles, and can do nothing with `t`
```

Writing the natives over a bare `T` instead would accept a list or a string
just as readily and read their bytes as a table's.

**A column's type varies by column.** `dtlv_rti_table_get_local` takes a row
and a column and hands back a raw pointer, because there is no one type to
hand back. A `get(ref self, row, col)` has no return type to write: the answer
depends on `col`, which is a value. So even a module written for one concrete
table type could not have a general `get`; it would need one accessor per
column, which is what column projection syntax (`t.x`) would give.

**What exists underneath.** The runtime has `table_create`, `table_destroy`,
`table_push_row`, `table_build_from_rows`, `table_get`, `table_set`,
`table_clear` and `table_len`, none of them reachable from the language. The
spec mentions column projections yielding a list view; `t.x` is
`ProjectionOnNonAggregate` today, and `t[i]?` is refused because indexing wants
a list, map or tensor.

**What it would take**, in the order it would have to happen: column projection
(`t.x`, giving `[T]`), which makes per-column access expressible; then either a
row type parameter or a `table` bound, which is what a *generic* `len` and
`push_row` need. The second is a type-system feature rather than a library one.

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

## Inlining never triggers from a caller in an earlier unit

**Known gap, marked in the source.**

`DynamicInliner::dispatch_call` finds the caller to inline into by matching on
its reference, and the `CodeRef::External` arm returns `None` with a TODO. So a
call site in a later unit into an earlier unit's function never triggers
inlining, however hot it gets.

The lookup is available -- `registry.get_external_function_as_unit(unit, id)`
-- so this is about three lines. The half that used to be missing is already
done: `FuncIdentity` folds `Local` and `External` together, so a body optimized
while its unit was current is found when a later unit calls the same function
externally.

## A field of a const cannot be projected, but a const can be destructured

**Reproduced.**

```datalove
const PAIR: (int, int) = (: int / 10, : int / 20)
let a = PAIR.0       // NonCopyFieldProjection
let b = PAIR.0@      // NonCopyFieldProjection as well

const MAYBE: ?int = some :int/42
if MAYBE |val|       // fine: unwraps a fresh copy
    debuglog val
end if
```

So a linear payload may be taken out of a const option and a linear field may
not be taken out of a const tuple, though both are reading one part of a
constant. A const of an aggregate type can therefore be passed along whole and
not read into, which is what `specialize_differential/017_comptime_tuple` means
by "can't destructure tuples yet".

The projection rule exists to stop a non-copy field moving out of a place
someone still holds. Reading a constant is not that: each read materializes its
own copy, which is why reading a const binding does not consume it. Settling
this is a rule about every const rather than about const parameters, which is
why it was left alone while those were being worked on.

Note that `p.a@` on a `let` binding works, since lowering borrows the field and
clones through the borrow. The const path refuses earlier, at typecheck, so it
never reaches that.

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
