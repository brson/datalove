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

- [Indexing a container a generic function owns is never unwrapped](#user-content-indexing-a-container-a-generic-function-owns-is-never-unwrapped)
- [A part of a borrowed generic aggregate that is itself a composite is refused](#user-content-a-part-of-a-borrowed-generic-aggregate-that-is-itself-a-composite-is-refused)
- [A unit that fails part way through leaves its index to the next one](#user-content-a-unit-that-fails-part-way-through-leaves-its-index-to-the-next-one)
- [Inlining never triggers from a caller in an earlier unit](#user-content-inlining-never-triggers-from-a-caller-in-an-earlier-unit)
- [A field of a const cannot be projected, but a const can be destructured](#user-content-a-field-of-a-const-cannot-be-projected-but-a-const-can-be-destructured)
- [A specialized function keeps an original nobody calls](#user-content-a-specialized-function-keeps-an-original-nobody-calls)

## Indexing a container a generic function owns is never unwrapped

**Reproduced.** Refused by both AOT backends, a panic in the jit, a segfault in
the interpreter.

```datalove
fun first<T>(xs: [T]): ?T
  ret some (xs[0]?@)
end fun

let ns: [u32] = [10, 20]
debuglog first(ns)
// aot and c-aot: "ListGet on non-list type: Data"
// jit: panic; interpreter: SIGSEGV
```

An owned container is the one thing erasure does not walk: it is wrapped whole
into a `data` (`erased_owned_type`, and the comment above `erased_param_type`
says why -- rebuilding a list into a different stride is not a conversion worth
having). So `xs` inside `first` is a `Data`. Lowering then emits `ListGet`
against it without opening the wrapper:

```
fn first(p0):            // p0: Data
    v0 = const 0index
    v1, v2 = listget p0[v0]
```

`emit_list_get` matches `IrType::List` and errors on anything else
(`c-aot/src/codegen.rs:2790`), which is why the compiled backends refuse. The
interpreter does not check, and reads the anypack's bytes as a list header.

**Not the same fault as the two above.** No reference is involved and no
descriptor is missing -- a `data` carries its own. The value is simply never
opened. So [fat references](plan-fat-refs.md) do not reach this, and fixing it
is a lowering change: unwrap the owned container before indexing it, or keep
`[T]` unwrapped when owned and convert at the boundary instead.

**Scope, checked by compiling and running each.**

| shape | result |
|---|---|
| `ref xs: [T]`, `xs[i]?@` -- borrowed, read | correct: `descriptor_params` supplies the list descriptor and `dtlv_rti_list_get_erased_local` uses it |
| `xs: [T]`, `xs[0]?@` -- **owned**, read | **this entry** |
| `ref xs: [T]`, `ref xs[i]?` -- borrowed, borrow | the entry above; segfaults |

So of the three ways to index a generic list, one works and two do not, and
they fail for unrelated reasons.

**The plan believes the reading half is done.** `plan-generics.md` says
indexing reads the stride from the descriptor everywhere. That is true of the
borrowed read and false of the owned one, which never gets as far as a stride.

## A part of a borrowed generic aggregate that is itself a composite is refused

**Reproduced.** Refused, not miscompiled. A gap rather than a fault.

```datalove
fun get_q<T>(ref p: {q: {a: T, b: u32}, c: u32}): {a: T, b: u32}
  ret p.q@
end fun

fun set_q<T>(mut p: {q: {a: T, b: u32}, c: u32}, v: {a: T, b: u32})
  set p.q = v
end fun
```

Reading or writing a *whole sub-aggregate* that holds the type parameter, as
opposed to reading a field of it, is refused by `erasure_is_composite`.

Reading a part of a borrowed generic value has three cases and only two are
handled. A destination whose static type is the truth takes a copy at an offset
from the descriptor. A destination that is exactly `data` takes a packed one,
which `dtlv_rti_field_read_local` does. A destination like `{a: data, b: u32}`
is neither: what is really there is `{a: u8, b: u32}`, so it would have to be
converted field by field, the way the boundary converts an owned parameter.
Nothing does that from a descriptor rather than from a pair of static types.

The write side refuses a bare `data` target as well as a composite one, because
that direction would have to *unpack* rather than pack.

**What it would take.** A runtime conversion driven by two descriptors -- what
is there and what is wanted -- rather than by two static types. `Erase` and
`Reify` already do this at a call boundary, but from types both of which the
backend knows. Extending that to take the source shape from a descriptor is the
same piece of work the erased-element read needed, and would subsume it.

**A refusal here costs a case that would have worked.** A `data` written by
hand and a `data` left by erasure are one `IrType`, so a genuinely written
`{x: data}` field is refused too. That is the price of the two being
indistinguishable, and it is paid only inside a generic, on a borrowed
aggregate, for a field that is a composite holding `data`.

**Not uniform.** The two AOT backends refuse at compile time. The jit falls
back to the interpreter rather than refusing, and the interpreter refuses at run
time -- `Interpreter::field_write_size` asserts the two sides agree on a width,
and the read path panics in the runtime. So the program is rejected everywhere,
but by three different mechanisms and three different messages.

**And one route to it is not refused at all.** Handing the borrow to a callee
rather than reading it here reaches the same conversion through the return
value, where nothing checks:

```datalove
fun take<T>(ref x: T): ?T
  ret some (x@)
end fun

fun f<T>(ref p: {q: {a: T, b: u32}, c: u32}): ?{a: T, b: u32}
  ret take(ref p.q)
end fun
```

`take` is handed the descriptor of what is really there, clones it and packs a
`data` holding a *concrete* `{a: u8, b: u32}`. The caller then reifies that into
`?{a: data, b: u32}`, its own erased shape, and the two disagree about what is
inside the `data`. The compiled backends print `some {a = 0, b = 1}` and
`some {a = 0, b = 2}` -- different garbage each -- and the interpreter panics.
Reaching it through a borrowed list element rather than a field does the same.

This predates fat references: the same program gave the same garbage at
`6a1f611b`, checked by building it. What is new is that the borrow now carries
the truthful descriptor, so the disagreement is between a truthful callee and an
erased caller rather than between two consistently wrong ones.

**All owned is fine.** The same shape with no borrow anywhere -- `take(p)` on an
owned parameter -- gives `some {a = 1, b = 7}` in every backend, because the
boundary converts field by field in both directions. So the missing piece is
precisely a conversion driven by descriptors rather than by two static types.

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
