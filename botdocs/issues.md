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
