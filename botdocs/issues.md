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

- [A reference into an erased container carries the wrong type](#user-content-a-reference-into-an-erased-container-carries-the-wrong-type)
- [A unit that fails part way through leaves its index to the next one](#user-content-a-unit-that-fails-part-way-through-leaves-its-index-to-the-next-one)
- [Inlining never triggers from a caller in an earlier unit](#user-content-inlining-never-triggers-from-a-caller-in-an-earlier-unit)
- [A field of a const cannot be projected, but a const can be destructured](#user-content-a-field-of-a-const-cannot-be-projected-but-a-const-can-be-destructured)
- [A specialized function keeps an original nobody calls](#user-content-a-specialized-function-keeps-an-original-nobody-calls)

## A reference into an erased container carries the wrong type

**Reproduced.** Segfaults.

```datalove
fun take<T>(ref x: T): ?T
  ret some (x@)
end fun

fun via<T>(ref xs: [T], i: index): ?T
  ret take(ref xs[i]?)
end fun

let words: [string] = ["alpha", "beta"]
debuglog via(ref words, 0)      // SIGSEGV
```

Both neighbours work: `take(ref s)` on a concrete `string` is fine, because a
borrowed parameter mentioning a type parameter takes its descriptor from the
call site (`FunctionContext::descriptor_params`). And a caller whose list type
is concrete is fine, because the static type is truthful. Only the erased
combination faults.

**What is wrong.** `ListElementRef` stores a bare pointer. Its stride is right
-- `list_element_info` reads the element descriptor off the *list's* runtime
descriptor -- but nothing carries that descriptor onward. `Frame::value_deref`
takes the inner type from the *reference's static tydesc*, which inside a
generic says `data`, so the bytes are read as the wrong thing.

`MapValueRef` and `TensorIndexRef` have the same shape and presumably the same
fault; only the list case has been reproduced.

**Why the rest of indexing is fine.** `ListGet`, `MapGet` and `TensorGet` never
form such a reference. They clone the element, and when the destination is
erased they go through `dtlv_rti_list_get_erased_local` so that the runtime
decides whether the element wants wrapping.
`backend/10_generic_indexing.dfs` says why that decision is the runtime's:
"Deciding it in each backend would be four answers to one question."

**The descriptor is already in reach, which is the frustrating part.**
`via<T>(ref xs: [T], ...)` has a borrowed parameter mentioning a type
parameter, so it is a `descriptor_param` and the call site hands it a `[T]`
descriptor. The element's descriptor is a field of that one --
`type_info.list.element_tydesc`, which `list_element_info` already reads. So
the information exists at the point of the fault; nothing carries it to where
it is read.

**The trailing-descriptor channel does not reach this.** `shape_descriptors`
passes descriptors to a callee, and `DescriptorRef` has two forms: `Static`, a
descriptor the call site knows outright, and `Own(i)`, one this function was
handed, forwarded whole. `shape_descriptors_for` matches a wanted shape against
`own_shapes` exactly, so a function holding `[T0]` and asked for `T0` fails --
it can forward a descriptor but not a part of one.

Adding that part is small and does not break what the design rests on. Reading
`element_tydesc` out of a list descriptor is a load, not construction, so
"nothing is put together at run time" still holds; a `DescriptorRef` variant
meaning "the element of the one I was handed at index i" would do it.

**But it would not fix this fault**, because the fault is not at a call
boundary. In

```
block2:
    v2 = listelementref p0[p1]
    v3 = call @0 u0(*v2)
```

the `*v2` is read in `via`'s own frame, and `Frame::value_deref` takes the
inner type from `v2`'s static tydesc. The value handed to the callee is already
wrong before any descriptor is passed anywhere. A trailing descriptor gets the
right type *to* a callee; it does not give a correctly typed local borrow.

**What it would take.** The descriptor has to travel with the reference. That
is the "descriptors as dataflow in the IR" option that
[Generics and Specialization](plan-generics.md) weighed and turned down:

> the largest surface of the three: two things that have to stay in agreement,
> spread over every instruction and four backends. Every generics bug found so
> far has been that shape, and the failure mode is silent corruption rather
> than a refusal.

This fault is that failure mode, arriving early.

**Cheaper than fixing it.** Refuse `ref xs[i]?` where the element type is
erased, at typecheck, turning a segfault into a diagnostic. That does not close
the general question, only this way of reaching it.

**Reach.** Not reachable from `sys/std`, whose `list.get<T>` delegates to a
native rider rather than using indexing syntax. It needs user code to write a
borrowed index inside a generic.

**Also held back by this.** Lowering borrows a projection in an operand
position, which reads a linear element once rather than cloning it twice, but
it has to leave an erased element on the `ListGet` path for the reason above.
So there are two lowerings for indexing, which is a smaller version of the same
gap. See the comment in `lower_operand`.

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
