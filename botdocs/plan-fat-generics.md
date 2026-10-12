# Generics by Reference

A plan to replace erasure with one representation: inside a generic, every
value whose type mentions a type parameter is a pointer at the value in its
real layout together with that value's descriptor, and the descriptors a body
needs are built at run time from one descriptor per type parameter. Written
October 2026; not started.

This supersedes the erasure half of [Generics and
Specialization](plan-generics.md) and, once done, most of [Generics: how it
works](generics.md). [Monomorphizing Generics](plan-monomorphization.md)
stays the plan for speed, and comes after this.

## Contents

- [Why](#user-content-why)
- [The shape](#user-content-the-shape)
- [Types in the IR](#user-content-types-in-the-ir)
- [Descriptors](#user-content-descriptors)
- [Storage](#user-content-storage)
- [Calls](#user-content-calls)
- [Operations on a dynamic value](#user-content-operations-on-a-dynamic-value)
- [Natives](#user-content-natives)
- [The runtime](#user-content-the-runtime)
- [What goes away](#user-content-what-goes-away)
- [What stays](#user-content-what-stays)
- [What it does for other work](#user-content-what-it-does-for-other-work)
- [Cost](#user-content-cost)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## Why

Consistency and simplicity first, speed after. The erased implementation works
on every backend, but it gets there with three ways of getting a descriptor
(the value, `descriptor_params`, `descriptor_shapes`), a conversion layer
between a value's real shape and its erased one (`erase`, `reify`, `convert`,
`needs_erasure` asked in both directions), and a derivation of what each
reference points at (`resolve_ref_descriptors`). Its recurring fault, named in
[plan-generics](plan-generics.md#user-content-what-a-prototype-of-that-found),
is a descriptor that holds "for exactly as long as the value stays in the
parameter it arrived in": owned containers moved into a local, borrows taken
out of a borrowed container, and now a reference held across a `for` loop all
meet it. Most of its fixes have been silent wrong answers rather than crashes.

The rider ABI and the interpreter already agree on the alternative: a value is
a pointer and a descriptor. Compiled generics are the one place that says
something else.

## The shape

- A **dynamic type** is an IR type that mentions a type parameter: `T`,
  `(T, u32)`, `?T`, `[T]`, `%{K = V}`.
- A **dynamic value** is a value of a dynamic type. In a frame it is a **fat
  pointer**, `{ptr, tydesc}`, two words whatever the type: `ptr` is the value
  in its real layout, and `tydesc` describes that layout. Slots, block
  parameters and by-value passing keep a fixed size, which is what `data`
  gave and what this keeps.
- The value a fat pointer points at lives on a **shadow stack** owned by the
  runtime, or in the caller's frame for an argument.
- A generic is called with its arguments as `(ptr, tydesc)` -- the rider
  ABI -- and **one descriptor per type parameter**. Anything compound the body
  needs is built from those at run time.

Nothing is converted at a boundary. A `u32` passed for a `T` is passed as a
pointer at the `u32` and the `u32` descriptor; a `(u8, u32)` passed for a
`(T, u32)` is a pointer at the `(u8, u32)`.

Code outside generics does not change. A type that mentions no type parameter
is laid out and passed exactly as today.

## Types in the IR

`IrType` gains `Param(u32)`, a type parameter by position, and keeps it inside
the types around it: `(T, u32)` is `Tuple([Param(0), U32])`, not
`Tuple([Data, U32])`. `IrType::from_datalit` and `from_type_hint` stop
collapsing type parameters to `Data`, and `erased_param_type`,
`erased_owned_type` and `erased_return_type` go: a parameter's IR type is its
written type.

Layout has one new rule: a dynamic type is laid out as a fat pointer. A
non-dynamic type keeps its layout, so `ir::layout` changes in one place.

`Data` stays, for the language's own `data` type, which is a value written as
one (`data 42`) and has nothing to do with generics after this. `T` bound to
`data` is a fat pointer at a `data`, like any other binding.

## Descriptors

**Hidden parameters.** A generic takes one descriptor per type parameter of
its declaration, after its ordinary parameters. That number comes from the
signature, not the body, so a callee's calling convention never depends on
what its body builds.

**The descriptor table.** `FunctionContext` gains the list of dynamic types
the body uses -- each a type expression over the function's own parameters --
and the function fills a table of their descriptors at entry, by asking the
runtime to build each from the type-parameter descriptors. Every instruction
that needs the descriptor of a dynamic type reads its entry. A body that only
touches `T` itself has nothing to build; `T`'s entry is the hidden parameter.

**Building.** The runtime builds a descriptor from a type form and the
descriptors of its parts -- a list of this, a tuple of these, an option of
that -- and caches it, so a table fill is a lookup per entry after the first
call at a given binding. See [The runtime](#user-content-the-runtime).

**Call sites.** For each type parameter of a callee, the caller supplies a
descriptor: a static one, when the binding is concrete; its own table entry,
when the binding is a type over its own parameters. The second includes
forwarding one of its own type parameters, which is the entry that is the
hidden parameter. `resolve_call_descriptors` survives as that lookup, with
the two-group `DescriptorRef` replaced by "static, or a table index".

**What this removes.** `descriptor_params` folds into the fat pointers.
`descriptor_shapes` and `close_shapes` have nothing left to do: nothing about
a callee's descriptors is derived from its body, so there is no closure over
the call graph, and a call at a growing type -- `f<T>` calling `f<[T]>` --
builds one more descriptor per level of recursion rather than needing an
infinite set ahead of time. The refusal in `growing_parameters` goes with it.

The rule that every type parameter be determined at a call site stays as it
is, and is now plainly the rule that the caller has to have a descriptor to
pass.

## Storage

**Each dynamic value and slot owns its storage.** A move copies `desc.size`
bytes into the destination's storage, rather than handing over the pointer.
Handing it over would be cheaper and is wrong: in

```datalove
loop
  let x = f()
  set acc = x
end loop
```

`acc` taking `x`'s pointer means the next iteration writes the new `x` over
it.

**The dynamic frame.** At entry, after filling the descriptor table, a
function with dynamic values bumps the shadow stack once for all of them:
each value and slot of a dynamic type gets space sized and aligned from its
descriptor, and its fat pointer is set to point there. A value defined in a
loop reuses its space every iteration, so the shadow stack grows with the
number of IR values, never with the iteration count. The offsets depend only
on the descriptors, so they can be cached beside the table.

**Release.** The function records the shadow stack's top at entry and restores
it on every exit. Early returns from `?` and `!` are ordinary returns.
Abandoning compiled code returns through its callers the same way. The
runtime's `reset`, which is how a datalove error frees what it unwound
through, resets the shadow stack too.

**Values that leave** are copied out: a return into the caller's storage, a
push into the collection's buffer, a store into an aggregate. Nothing keeps a
pointer into a frame's shadow stack space after the frame returns, which is
what makes the stack discipline sound.

## Calls

Per parameter, by mode, for a dynamic parameter:

| Mode | Passed | Who owns what |
|---|---|---|
| `ref` | `(ptr, tydesc)` | the caller, throughout |
| `mut` | `(ptr, tydesc)` | the caller; the callee writes in place |
| `in` | `(ptr, tydesc)` at the caller's storage | the value passes to the callee, which destroys it or moves it on; the caller frees the storage, not the value |
| `out` | `(ptr, tydesc)` at storage for the result | the callee writes; as today, the caller destroys what was there first |
| return | `(ptr, tydesc)` at storage for the result | the callee writes it, as an `out` |

A concrete caller passes a concrete argument with no conversion: a pointer at
its own frame slot and the static descriptor. An `in` argument made as a
temporary is materialized in the caller's frame first, which it already is.
A concrete caller receiving a dynamic return provides its own concrete
destination and its static descriptor.

Then the type-parameter descriptors, in declaration order. Natives keep their
own trailing descriptors for now; see [Natives](#user-content-natives).

## Operations on a dynamic value

The instruction set stays as it is; what changes is how a backend implements
an instruction whose operand is dynamic, the way `RefDesc` changes it today.

| Operation | On a dynamic value |
|---|---|
| move, store, load, block argument | copy `desc.size` bytes into the destination's storage |
| clone | `any_clone` into the destination's storage |
| drop | `any_destroy` on the value; the storage goes with the frame |
| field, element, payload reference | a fat pointer: `ptr` plus the offset from the descriptor, and the part's descriptor |
| field, element, payload read of a concrete part | a load at the offset from the descriptor |
| tuple, struct, option, term, enum, result construction | each part stored at its offset from the destination's descriptor |
| option and result tests | the tag, which is at offset 0 whatever the payload |
| bounded operations (`ord`, `fixedint`, `float`) | `dyn_ops` on `(ptr, tydesc)` pairs |
| `debuglog` | the runtime's printer on the pair |

The offsets are what `dtlv_rti_field_offset` and `rtdt::layout` already give.
A projection off a fat pointer is a fat pointer, so what
`resolve_ref_descriptors` derives across a unit becomes a local computation at
each projection.

`Erase`, `EraseTracked`, `Reify` and `DataBorrow` are no longer emitted.
`DataFrom` stays for the `data` type.

## Natives

The rider ABI does not change: `(ptr, tydesc)` per argument, the result
through an out pair, trailing descriptors for a type parameter that appears
only in the return. A generic calling a native passes its fat pointers
through, and the trailing descriptors come from its table instead of from a
closure.

What changes for riders is what they are handed. A result slot is always the
real layout now, never a `data` a native has to wrap into, and an argument is
never a `data` standing for something else. The `data`-slot branches in
`sys/std/rider` and the runtime's `*_erased_local` entry points become dead,
and go in a cleanup once nothing reaches them. Moving natives to one
descriptor per type parameter would make them uniform with generics; that is a
rider ABI change and a separate step.

## The runtime

**Descriptor construction.** One entry per type form -- list, set, map,
tensor, option, result, tuple, struct, term, enum -- taking the part
descriptors and returning a descriptor. Layouts come from `rtdt::layout`,
which already computes tuple, struct, option, result, enum and map and set
node layouts from descriptors, and which `ir::layout` agrees with; flags from
`TyDesc::computed_flags`. Built descriptors are interned in a cache keyed by
the form and the part descriptors' addresses, and live until shutdown, which
frees them so leak checking stays quiet. A miss for a type that already has a
static descriptor elsewhere builds a duplicate, which is harmless: descriptors
are compared structurally (`same_tydesc`), never by address.

**The shadow stack.** Its top and limit go in `RtHeader`, beside
`stack_limit`, so compiled code bumps and restores it inline and calls the
runtime only to grow it. It grows by chunks, never by moving, so a pointer
into it stays good. It is per `RtLocal`, which is per thread, so it is
reentrant and thread-safe by construction -- the property the C backend's
static tensor view lacks.

**Tests** in `datalove-rt-tests`: every type form built at run time compared
field by field with the descriptor the compiler emits for the same type;
nested forms; cache hits; the shadow stack across chunk boundaries, reset, and
restore.

## What goes away

- `erased_param_type`, `erased_owned_type`, `erased_return_type`, and type
  parameters collapsing to `Data` in `from_datalit` and `from_type_hint_erasing`
- `Erase`, `EraseTracked`, `Reify`, `DataBorrow`, and their lowering
- `erase_local`, `reify_local`, `clone_erased_local`, `convert`,
  `needs_erasure`, `data_into_local`, `unwraps_the_element`,
  `borrow_lookup_key`, and the `*_erased_local` collection entries
- `descriptor_params`, `descriptor_shapes`, `close_shapes`,
  `close_shapes_over_calls`, `close_shapes_in_units`, `growing_parameters`,
  and the refusal of a call at a growing type
- `RefDesc` and `resolve_ref_descriptors`, replaced by fat pointers
- `wrapped_shapes` and `container_shape` in lowering
- the two descriptor groups in the JIT bridge and trampoline
- the call-site question of whether a `data` is a wrapper or a pointer
- the ownership rule that a binding is moved out before its conversion
- OSR's refusal to enter a loop that needs a reference whose descriptor is
  worked out where it is made

## What stays

- Static descriptors for every concrete type, emitted at compile time.
- Bounds as a closed set dispatched through `dyn_ops`.
- The determinability rule at call sites.
- Type parameters always linear.
- The language. Nothing a program can write changes, except that a call at a
  growing type compiles.

## What it does for other work

**`for` loops.** A fat reference carries its own descriptor, so it can sit in
a slot or block parameter; that was the first of the reasons a reference held
across a loop was a problem. The [cursor plan](plan-cursors.md) does not need
it and does not wait for it, but its element descriptor becomes the
descriptor table's entry rather than a hoisted `element_tydesc`.

**Monomorphization.** An instance bypasses all of this, so it is no harder;
the erased path it sits beside is one shape rather than three, and the
`Specializer`'s "descriptors back to types" reads the type-parameter
descriptors a call already has.

**Incremental compilation.** A generic's body no longer contributes to its
callers' calling convention, so editing it invalidates less.

**Const parameter specialization** (`specialize.rs`) is untouched.

## Cost

What gets cheaper: packing a bare `T` into a `data` and back, the boxing
allocation for anything wider than two words, and wrapping an owned container
-- together the largest share of the jit Sieve profile in [Are We Fast Yet for
benchvs](research/research-awfy-benchmarks.md). Shadow-stack bumps replace
allocator calls.

What gets dearer: a move of a dynamic value is a copy of `desc.size` bytes
rather than two words, a scalar `T` is read through a pointer, a call fills a
descriptor table, and a dynamic parameter is two words, so the JIT's
`MAX_DIRECT_ARGS` is reached sooner.

Measured before starting and after finishing, recorded here: jit and
interpreted Sieve and the other benchvs benches, the `ord` benchmark from the
`for` work, the store demo, and the compile time of `sys/std`.

## Phases

The representation is decided in lowering and every backend reads it, so the
switch cannot be made one backend at a time inside one build. Instead the new
lowering sits behind a setting, `DATALOVE_GENERICS=fat`, which the suite runs
under as its own configuration -- as it runs under `index-64` -- until it is
the only one. Each phase leaves the default suite passing.

0. **Baseline.** The measurements in [Cost](#user-content-cost).
1. **Runtime.** Descriptor construction and its cache; the shadow stack in
   `RtHeader`; their tests.
2. **IR.** `IrType::Param`; the fat-pointer layout rule; the descriptor table
   on `FunctionContext`; display, serialization, verification. No lowering
   produces them yet.
3. **Lowering**, under the setting: written types kept, the table built,
   type-parameter descriptors at call sites, no `Erase` or `Reify`. Ownership
   analysis is unchanged, the language being unchanged.
4. **Interpreter and CTFE**, the reference: the dynamic frame, the operations
   table above, calls. Done when every fixture passes under the setting with
   leak checking on.
5. **Cranelift**, AOT and JIT: the same, plus the bridge, the trampoline and
   OSR carrying fat pointers and type-parameter descriptors.
6. **C AOT.**
7. **Bytecode.**
8. **Flip and delete.** The setting becomes the default and then goes; the
   list in [What goes away](#user-content-what-goes-away) is deleted; a call
   at a growing type gets a fixture.
9. **Docs and measurement.** `generics.md` rewritten, `plan-generics.md`
   marked superseded where it is, `native-abi.md` and `ir.md` updated, and the
   measurements recorded.

## Open questions

- **Entry-time or lazy storage.** Bumping once at entry for every dynamic
  value costs per call in proportion to how many the body has, used or not.
  Allocating each on its first definition costs a test per definition
  instead. Entry-time is simpler; measure on `sys/std` before choosing.
- **Where the cache lives.** Per `RtLocal` needs no lock and builds each
  descriptor once per thread; shared builds each once and needs a lock or a
  concurrent map.
- **Inline containers.** A `[T]` header is the same three words whatever `T`
  is, so it could live in the frame with its descriptor beside it rather than
  behind a pointer. An optimization for after, once everything is one shape.
- **Fat pointers in registers.** Cranelift can keep the two words as two
  variables rather than a struct in a stack slot, and C as two locals. Worth
  doing in phase 5 if it is not much harder.
- **The rider ABI.** Whether natives move to one descriptor per type
  parameter, and when.
- **Tracking bytes.** A dynamic slot that is conditionally initialized needs
  its tracking byte as now; whether the dynamic frame changes anything about
  where those live has not been looked at.
