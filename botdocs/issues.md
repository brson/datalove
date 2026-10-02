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
- [The runtime is duplicated across the rider dlopen boundary, and shares a heap across it](#user-content-the-runtime-is-duplicated-across-the-rider-dlopen-boundary-and-shares-a-heap-across-it)
- [A move followed by a `break` is refused, though it cannot repeat](#user-content-a-move-followed-by-a-break-is-refused-though-it-cannot-repeat)

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

## The runtime is duplicated across the rider dlopen boundary, and shares a heap across it

**Latent. Works today only because both copies use the same system malloc.**

`rider_build.rs` synthesizes a crate with `extern crate datalove_rt;` and builds
it as a `cdylib`, which `rider_load.rs` then `dlopen`s. That shared library
carries its own copy of `datalove-rt` and its own Rust global allocator, so the
process contains **two runtimes**. They do not merely coexist: they touch the
same `AllocLocal`, whose `active_allocations` is a `HashMap` on the Rust heap.

So a `HashMap` grown inside the dlopened library is allocated by that copy's
allocator and freed by the main binary's, or the reverse. Both resolve to glibc
malloc today, so it happens to work.

Putting `#[global_allocator] = mimalloc` on the `std_all_tests` harness segfaults
immediately, and the backtrace says exactly this:

```
mi_free (p=0x7f1b240020f0)                      <- not a mimalloc pointer
<mimalloc::MiMalloc as GlobalAlloc>::dealloc
hashbrown::raw::free_buckets
hashbrown::raw::reserve_rehash<(*mut u8, datalove_rt::impls::alloc::unix_impl::AllocationInfo), ...>
<datalove_rt::impls::alloc::unix_impl::AllocLocal>::alloc
datalove_rt::impls::string::string_push_bytes_local
```

The cli was unaffected by the same experiment: it reaches riders through
`register_linked_natives`, which are linked into the binary, so there is one
runtime and one allocator. It is the dlopen path that duplicates. Nothing in the
tree sets a global allocator today -- the experiment was measured and backed out
-- so this is latent rather than live.

This is worth fixing on its own terms, separately from any allocator question. A
rider library built by a different toolchain, a different rustc, or with a
different allocator would break the same way, and the failure is a segfault in
free rather than anything that names the cause. The fix is for the boundary to
be a C ABI over pointers the two sides do not both own -- the shared library
should not be handing Rust heap ownership back and forth with its host.

## A move followed by a `break` is refused, though it cannot repeat

**Reproduced.** A spurious D007 and D008.

```datalove
var s = "x"
loop
  if true
    let t = s          // D007 `cannot move s in loop`, D008
    break
  end if
end loop
```

The move cannot happen twice: the only path that reaches it leaves the loop
immediately after. `analyze_loop` sees the binding `Moved` at the end of the body
and raises `MoveInLoop` without asking whether anything gets back to the loop
head from there.

**This is what is left of a larger fault.** The same merge mishandled `ret`, and
that part is fixed: a branch that returns is now left out of the merge after an
`if` or a `match`, so a guard clause with owned values live is accepted and a
recursive function over `int` compiles. See
`std_tests/151_owned_past_a_returning_branch` and
`reachable::body_returns`, whose docs say why.

**`break` is not the same problem, and the easy version of the fix is wrong.**
Returning arrives at no merge, so ignoring the branch costs nothing. A `break`
arrives after the loop, and whatever it moved is given away on that path. Leave
it out of the merge and the binding reads `Live` afterwards, so scope exit drops
a value the break path already handed over -- a double free, not a spurious
error. The state after a loop genuinely depends on which exit was taken, which is
the ambiguity the pass refuses in order to keep drop points static.

So closing this wants either a merge at the loop exit over the break paths and
the fall-through together -- and a refusal when they disagree, with a message
that says so rather than `cannot move in loop` -- or the loop exit knowing that a
`break` is the only way out, which makes the binding definitely moved. The
present message is the misleading part either way: the move is fine, and what is
wrong is that nothing can say what is true after the loop.

`continue` has the same shape, going to the loop head instead.

`ir_inline/008_multi_return` was written as two else-less early returns over an
`int` and is spelled as an `else if` chain to avoid the `ret` half of this; it
can go back now.
