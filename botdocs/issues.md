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

- [The jit drops the descriptors a generic callee builds with](#user-content-the-jit-drops-the-descriptors-a-generic-callee-builds-with)
- [Inlining drops a call's shape descriptors, and nothing but a test inlines](#user-content-inlining-drops-a-calls-shape-descriptors-and-nothing-but-a-test-inlines)
- [A table literal silently drops what follows a column name](#user-content-a-table-literal-silently-drops-what-follows-a-column-name)
- [No table module, and none can be written](#user-content-no-table-module-and-none-can-be-written)
- [A unit that fails part way through leaves its index to the next one](#user-content-a-unit-that-fails-part-way-through-leaves-its-index-to-the-next-one)
- [Inlining never triggers from a caller in an earlier unit](#user-content-inlining-never-triggers-from-a-caller-in-an-earlier-unit)
- [A field of a const cannot be projected, but a const can be destructured](#user-content-a-field-of-a-const-cannot-be-projected-but-a-const-can-be-destructured)
- [A specialized function keeps an original nobody calls](#user-content-a-specialized-function-keeps-an-original-nobody-calls)

## The jit drops the descriptors a generic callee builds with

**Reproduced**, on a shipped flag, with memory corruption rather than a
diagnostic. This is the worst thing in this file.

```datalove
require module sys/std/list
import list.reversed

fun go(ref xs: [u32]): [u32]
    ret reversed(ref xs)
end fun

let l: [u32] = [1, 2, 3]
debuglog go(ref l)
```

`datalove script` prints `[3, 2, 1]`. So do both AOT backends. `datalove script
--jit` aborts:

```
list_create:value_out: pointer 0x7f4b366ef3c0 not aligned to 738201760 bytes
```

That is `reversed` building its result list from a descriptor it was never
handed, reading whatever was in the register.

**The cause is that the jit's two ABI boundaries know about `descriptor_params`
and not about `descriptor_shapes`.** Compiled code passes trailing descriptors
in one array, the parameter ones first and then one per shape the callee
declared, which is what `codegen/calls.rs` emits and what an AOT build reads.
Neither jit boundary reads the second half:

- `bridge::call_jit` builds `[rt_handle, sret?, args.., param_descriptors..]`
  and stops. Its signature takes `descriptor_params: &[ParamId]` and there is no
  shape parameter to pass.
- `trampoline::__jit_dispatch_call` receives the whole array but consumes only
  `descriptor_params.len()` of it, then falls back to `interp.call_in_context`
  rather than `call_in_context_with_shapes`.

So `descriptor_shapes` appears nowhere in `datalove-datafun-cranelift-jit`
outside its own test fixtures.

**Why it is not caught.** The interpreter refuses to hand a call to the
dispatcher when that call supplies shape descriptors -- `execute_call_with_shapes`
comes first in the `Call` arm -- so a *direct* call to a collection-building
generic never reaches the jit. What that does not cover is a caller of one. A
function whose own `descriptor_shapes` is empty is ungated, gets compiled, and
its call to the generic then goes out through the trampoline. `go` above is
exactly that, and so is every non-generic function that calls `sys/std/list`,
which is most code anyone would write.

**Why the fixtures miss it.** `backend/*.dfs` runs all four backends and
compares, and eight of its fixtures are generic -- but in every one the generic
is called from the script unit body, which is not a compilable function, or
from another generic that forwards the shape and is therefore gated too. The
missing shape is a plain function calling a generic that builds. One fixture
closes the hole. `interp_jit_tests` runs 407 worldfiles through the jit and not
one of them mentions a type parameter.

**What it would take.** Give both boundaries the shapes: `call_jit` appends one
descriptor per `func_ctx.descriptor_shapes` after the parameter ones, and the
trampoline reads that suffix and calls `call_in_context_with_shapes`. Where the
descriptors come from on the way in is the question to settle first -- a
`DescriptorRef::Own` has to be read from the compiled frame, which is the part
the interpreter does through `resolve_shape_ref`.

Until then `--jit` is unsound for any program where a function calls a generic
that builds a collection, which includes most of `sys/std`.

## Inlining drops a call's shape descriptors, and nothing but a test inlines

**Reproduced**, but only through a dispatcher nothing outside the test suite
constructs. Recorded together because the second half decides what to do about
the first.

`RemapContext::remap_instruction` blanks `shape_descriptors` when it copies a
`Call` into the body it is inlining into. That is half right: a
`DescriptorRef::Own(i)` indexes the *enclosing* function's declared shapes and
means nothing once the body has moved, so keeping it would be wrong. A
`DescriptorRef::Static(ty)` is a whole type and is valid anywhere, and blanking
throws it away. The callee is then entered with no descriptor:

```datalove
fun wrap<T>(x: T): [T]
    var out: [T] = [x]
    ret out
end fun

fun wrap_s(x: string): [string]     // concrete, so it declares no shapes,
    ret wrap(x)                     // but this call carries a static one
end fun

fun hot(): [string]
    ret wrap_s("s")
end fun
```

Calling `hot` past the inline threshold puts `wrap_s`'s body inside it without
the descriptor, and the interpreter panics on `a shape built with is one this
function declared`, leaks 86 bytes and aborts in the destructor.

`ListNew`, `SetNew` and `MapNew` carry a `descriptor: Some(i)` index with the
same problem, copied verbatim into a function with a different shape list. That
one is not reachable: a body containing one belongs to a function that declares
shapes, and calls to such a function are gated away from the dispatcher, so it
is never inlined. Only the `Static` blanking is live.

**Nothing inlines outside tests.** `DynamicInliner` is constructed in exactly
one place, `OptimizingDispatcher::with_config`, and `OptimizingDispatcher` is
constructed in exactly two, both test binaries. `datalove script --jit` builds a
bare `JitEngine`; the REPL, the benches and `worldfile_analysis` pass no
dispatcher at all. So the inliner has never run in anything a user can invoke.

The other inliner, the directive-driven `datalove-datafun-inline` API --
`parse_inline_directives`, `inline_module`, `inline_cross_module`, and the
`inline-directives` worldfile section -- is reachable only from
`ir_inline_tests`, which compares printed IR before and against after and never
*runs* the result. 21 fixtures, none generic.

What does run is the two dispatch suites, over the 407 worldfiles in
`fixtures/interp`: 5 inlinings in tuned mode, 125 in chaos mode, checked by
comparing against the plain interpreter. None of those 407 mentions a type
parameter, which is why this was never seen.

**Worth knowing before fixing either.** An inlined body is picked up in
`execute_call`, when the function is *entered*, so an inlining performed during
an invocation does not affect that invocation -- only a later call to the same
caller. A hot loop inside a single call never benefits from inlining its
callees. That is what makes the feature much weaker than the thresholds
suggest, and it is worth deciding whether the inliner earns its place at all
before spending anything on the descriptor handling.

## A table literal silently drops what follows a column name

**Reproduced.** Accepted and wrong, with no diagnostic.

```datalove
let t: {| x: u32 |} = {| x zzz |}
debuglog t                          // {| x |}

let xs: [u32] = [1, 2]
let u: {| x: u32 |} = {| x = xs |}
debuglog u                          // {| x |}
```

Anything written after a column name in a table header is discarded, so a typo
reads as a table with no rows rather than an error. `{| x = xs |}` is the same
thing: not a columnar literal, just `{| x |}` with `= xs` thrown away.

**Cause not found.** `parse_table_header` takes the first token of a header cell
and lets the rest go, which looks like it, but making the cell require exactly
one token did not change the behaviour, and neither did setting `had_error` on
the D031 path -- so the extra tokens are gone before the header parser sees
them, somewhere in `split_lines` or the tree tokenizer. The typechecker is not
at fault: T054 and T055 do check a header's arity and names, and would have
caught a two-column header.

**What it costs.** Small but the bad kind: a misspelling produces a working
program with an empty table rather than a refusal.

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
spec mentions column projections yielding a list view; `t.x` is
`ProjectionOnNonAggregate` today, and `t[i]?` is refused because indexing wants
a list, map or tensor.

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
