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
- [Where phases 3 and 4 stand](#user-content-where-phases-3-and-4-stand)
- [Where phase 5 stands](#user-content-where-phase-5-stands)
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
- A **reference to a dynamic type**, `Ref(T)`, is not a dynamic value with
  storage of its own. It *is* the fat pointer, pointing at storage someone
  else owns. It gets no shadow-stack space, and moving it copies its two
  words, not what it points at. Every rule below about a dynamic value's
  storage is about owned dynamic values only.
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
non-dynamic type keeps its layout. "Dynamic" means *mentions* a parameter, so
`layout_of` checks for it before its match rather than gaining a `Param` arm.

That is the one rule, but not the one place. The static offset helpers --
`aggregate_field_offsets`, `option_payload_offset`, `enum_variant_offsets` --
are meaningless on a dynamic type, and are called directly from the
bytecode's lowering (about two dozen uses), the C backend and cranelift; each
call site needs a dynamic path or an assertion that it never sees one.
Cranelift and C also map types themselves (`ir_type_to_cranelift`,
`ir_type_size`), and the interpreter's `IrLayout` holds one static `TyDesc` per
value and asserts its size against the frame slot (`interp/layout.rs`), which
a dynamic value's fat pointer now fills instead.

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

**Building.** Each entry has a **template**: the static descriptor of its type
expression, with a `TyTag::Param` descriptor wherever a type parameter stands.
At entry the function asks the runtime to instantiate each template against
its type-parameter descriptors (`dtlv_rti_tydesc_instantiate`), and the
runtime caches what it builds, so a table fill is a lookup per entry after the
first call at a given binding. See [The runtime](#user-content-the-runtime).

A lookup per entry per call is still a cost on every call, and a small `sys/std`
generic called in a loop pays it every time. The expected fix is a one-entry
cache per function, keyed on the incoming type-parameter descriptor pointers,
that skips the fill when a call repeats the previous binding. Whether it is
needed is for the baseline to say.

**Specialization.** Const-parameter specialization (`specialize.rs`) removes
const parameters from the function it builds, so the hidden descriptors, which
come after the ordinary parameters, move with them. It copies the descriptor
table as it copies `descriptor_shapes` now. The other places that build a
`FunctionContext` literal gain the field too.

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

**Block arguments are a parallel move.** A jump can pass a block's own
parameters back to it in another order, and copying bytes one argument at a
time then reads storage it has already overwritten. The interpreter handles
this already (`walker.rs`, reading every argument before writing any);
cranelift copies aggregate block parameters one at a time and does not.
Lowering never emits such a jump today. Either that becomes a rule the
verifier checks, or the backends copy through scratch space sized from the
descriptors. The rule is simpler and is the plan unless lowering turns out to
need the jump.

**Release.** The function records the shadow stack's position at entry and
restores it on every ordinary exit. Early returns from `?` and `!` are
ordinary returns. Since the stack grows by chunks, the position is the top
*and* the chunk it is in, so restoring past a chunk boundary also restores the
limit; restoring only the top would leave the limit pointing into the newer
chunk.

Ordinary exits are not every exit. Abandoning compiled code is a bare return
after each call that skips any epilogue (`codegen/calls.rs`), and runtime
entry points are `C-unwind`, so a panic the REPL catches unwinds past compiled
frames without running anything. Neither restores. What makes this sound is
the runtime's `reset`, which is how a datalove error frees what it unwound
through: it resets the shadow stack to its base along with everything else.

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
| move or block argument of a `Ref(T)` | copy the two words; nothing is pointed at anew |
| read through a `Ref(T)` (`ValueRef`) | copy or clone `desc.size` bytes from `ptr` into the destination's storage |
| clone | `dtlv_rti_clone_local` into the destination's storage |
| drop | `dtlv_rti_any_destroy_local` on the value; the storage goes with the frame |
| field, element, payload reference | a fat pointer: `ptr` plus the offset from the descriptor, and the part's descriptor |
| field, element, payload read of a concrete part | a load at the offset from the descriptor |
| tuple, struct, option, term, enum, result construction | each part stored at its offset from the destination's descriptor |
| option and result tests | the tag, which is at offset 0 whatever the payload |
| bounded operations (`ord`, `fixedint`, `float`) | `dyn_ops` on `(ptr, tydesc)` pairs |
| `debuglog` | the runtime's printer on the pair |

The offsets are what `rtdt::layout` already computes, but only part of it is
reachable from compiled code: `dtlv_rti_field_offset` answers for tuples and
structs and panics on anything else. Option, result and enum payload offsets
need a runtime entry beside it, or the layout rules inlined per backend. They
are not one rule: an option's payload is at `align_up(1, align(T))`, a
result's at `align_up(1, max(align(T), 8))` because of the error, and each
enum variant has its own offset in `TyInfoEnumVariant.offset`. A runtime entry
is the plan, for the reason `generics.md` gives for the rest of the runtime
helpers: one derivation, not four.

A projection off a fat pointer is a fat pointer, so what
`resolve_ref_descriptors` derives across a unit becomes a local computation at
each projection.

`Erase`, `EraseTracked`, `Reify` and `DataBorrow` are no longer emitted.
`DataFrom` stays for the `data` type.

## Natives

The rider ABI does not change: `(ptr, tydesc)` per argument, the result
through an out pair, trailing descriptors for a type parameter that appears
only in the return -- one per type parameter no argument determines, as
`DescriptorShape::Param` entries on `NativeContext`, which stay. A generic
calling a native passes its fat pointers
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

**Descriptor construction** is one entry, `dtlv_rti_tydesc_instantiate(rt,
template, params, num_params)`, in `impls/instantiate.rs`. A template is a
static descriptor like any other, emitted by the compiler for a type
expression over a generic's type parameters, except that a parameter is a
descriptor tagged `TyTag::Param` holding its position (`TyInfoParam`). The
runtime walks the template, putting the bound descriptor in for each
parameter and building each form around one anew: tuples, structs, enums,
options, results, terms, lists, sets, maps, tensors and tables. A part with no
parameter in it is the template's own part, unchanged, and a template with
none at all is its own instance.

One entry rather than one per type form, because everything a built
descriptor holds besides its parts comes from its template: struct field
names, enum variant names, term names, table column names, a tensor's rank.
Typing is structural, but the names are part of the structure, and
`eq_tydesc` compares them. So the template is what has to be handed over in
any case, and handed over whole it settles the nesting too: `[(?T, u32)]` is
one call, not three that compiled code would have to chain.

Layouts come from `rtdt::layout`, which `ir::layout` agrees with. Its
functions take the composite's own descriptor rather than a list of parts, so
building a form is: fill in a `TyDesc` with the built parts, compute offsets
and size from it, then flags from `TyDesc::computed_flags`. A container's
header size is its `rtdt` struct's, whatever it holds; map and set node
layouts come from `TyInfoMap::new` and `TyInfoSet::new`.

Built descriptors are cached by the template's address and the parameters'
addresses; only what is asked for is cached, not the parts built on the way.
A miss for a type that already has a static descriptor elsewhere builds a
duplicate, which is harmless: descriptors are compared structurally
(`same_tydesc`), with address equality only as a fast path. The one place that
compared by address alone was a debug assertion in `dtlv_rti_clone_local`,
which is now `same_tydesc`.

A template is never the descriptor of a value. Its `size` and `align` are
meaningless wherever a parameter is inside it, and the runtime's value
operations panic on a `Param`. Descriptor comparison treats a parameter as
its position, so two templates compare as their type expressions do.

**Payload queries.** `dtlv_rti_payload_offset(tydesc, index)` and
`dtlv_rti_payload_tydesc(tydesc, index)` answer for an option, a result, a
term, or variant `index` of an enum, beside `dtlv_rti_field_offset` and
`dtlv_rti_field_tydesc` for tuples and structs.

**Where the cache lives** is per runtime handle (`RtLocal`), and outside its
allocator. Inside `AllocLocal` it would not survive: `reset` calls
`discard_all`, so the first datalove error would free every built descriptor
out from under the cache. Boxed and freed in `dtlv_rti_shutdown`, built
descriptors stay out of leak checking entirely. A process-wide cache is ruled
out by lifetimes rather than locking: its keys are the addresses of static
descriptors that live in a JIT module or an interpreter's table, which go away
when the REPL rebuilds its executor after an error, or when a test drops its
interpreter. Per handle, the cache dies first -- `IrInterpreter` declares its
runtime before its descriptor table -- so no key outlives what it names.

**The shadow stack.** Its top and limit are `RtHeader::shadow_top` and
`shadow_limit`, beside `stack_limit`, so compiled code bumps and restores it
inline and calls the runtime (`dtlv_rti_shadow_alloc`) only when a request
does not fit. It is in `impls/shadow.rs`. It grows by chunks, never by moving, so a pointer
into it stays good; see [Release](#user-content-storage) for what restoring
across a chunk boundary has to restore. It is per `RtLocal`, which is per
runtime handle, and a handle is used by one thread at a time, so it is
reentrant and thread-safe by construction -- the property the C backend's
static tensor view lacks.

**Tests** in `datalove-rt-tests`: every type form built at run time compared
field by field with the descriptor the compiler emits for the same type,
names included; nested forms; cache hits, and misses on a different template
with the same parts; built descriptors surviving `reset`; the shadow stack
across chunk boundaries, reset, and restore across a boundary.

## What goes away

- `erased_param_type`, `erased_owned_type`, `erased_return_type`, and type
  parameters collapsing to `Data` in `from_datalit` and `from_type_hint_erasing`
- `Erase`, `EraseTracked`, `Reify`, `DataBorrow`, and their lowering
- `erase_local`, `reify_local`, `clone_erased_local`, `convert`,
  `needs_erasure`, `data_into_local`, `unwraps_the_element`,
  `borrow_lookup_key`, and the `*_erased_local` collection entries
- `descriptor_params` and `descriptor_shapes` on `FunctionContext`,
  `close_shapes`, `close_shapes_over_calls`, `close_shapes_in_units`,
  `growing_parameters`, and the refusal of a call at a growing type
  (`LowerError::UnboundedDescriptorShape`, whose fixture
  `engines/module_060_unbounded_shape` flips to passing). `NativeContext`'s
  own `descriptor_shapes` stays; see [Natives](#user-content-natives).
- `DescriptorShape` for functions, `shape_descriptors_for`,
  `write_call_descriptors`, and the `shape_descriptors` on `Call`, replaced by
  the per-type-parameter descriptors
- `RefDesc` and `resolve_ref_descriptors`, replaced by fat pointers
- `wrapped_shapes` and `container_shape` in lowering
- the signature check that refuses a type parameter it cannot erase:
  `first_unerasable_type_param` and `unerasable_type_param_error`
- the three predicates for "a container of a type parameter" that have to
  agree: `is_container_of_type_param`, `type_hint_is_container_of_param` and
  `datalit_is_container_of_var`
- the bytecode's `EraseRt` and `ReifyRt`, and cranelift's `codegen/boxing.rs`
- the two descriptor groups in the JIT bridge and trampoline
- the call-site question of whether a `data` is a wrapper or a pointer
- the lowering rule that a binding is moved out before its conversion
  (`datafun-lower/src/expr.rs`, beside `EraseTracked`)
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

**Const parameter specialization** (`specialize.rs`) does what it did; it
carries the descriptor table and the hidden descriptors along, as
[Descriptors](#user-content-descriptors) says.

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
`for` work, the store demo, the compile time of `sys/std`, and a small
generic over a compound type -- one that needs a built descriptor -- called in
a loop, which is where the table fill shows. That last is
`benchvs/generics.dfs`, which is not one of the cross-language benches.

**Baseline** (October 2026, before any of this; release build, hyperfine, at
least three runs, mean seconds, on a 4-CPU sandbox):

| Bench | interp | jit-t1 | jit | aot |
|---|---:|---:|---:|---:|
| fib | 3.455 | 0.463 | 0.477 | 0.357 |
| sum | 3.302 | 0.338 | 0.338 | 0.328 |
| primes | 2.937 | 0.194 | 0.198 | 0.161 |
| wordfreq | 0.960 | 0.598 | 0.605 | 0.524 |
| sieve | 4.126 | 1.228 | 1.210 | 0.949 |
| mandelbrot | 2.870 | 0.555 | 0.547 | 0.544 |
| nbody | 0.794 | 0.423 | 0.415 | 0.299 |
| generics | 2.849 | 0.996 | 0.973 | 0.774 |

The store demo at 20,000 orders: 0.769 interpreted, 0.512 jit. A script
requiring every `sys/std` module and doing nothing else: 15.5 ms, which says
more about how little of `sys/std` a script compiles than about compile time.
The `ord` benchmark from the `for` work does not exist in the repository, so
there is no baseline for it.

## Phases

The representation is decided in lowering and every backend reads it, so the
switch cannot be made one backend at a time inside one build. Instead the new
lowering sits behind a cargo feature, `fat-generics`, which the suite runs
under as its own configuration -- a `test-fat` recipe and CI runner, exactly
as `index-64` has `test-64` -- until it is the only one. Each phase leaves the
default suite passing.

A feature rather than an environment variable because nothing reads an
environment variable in lowering today, and lowering is salsa-cached: a
variable would have to be read once per process and never vary, or become a
query argument the way `skip_const_inlining` is on `lower_module`. A feature
is decided at build time and cannot mix within a process.

**Backends not yet ported** refuse under the feature, and the backend fixtures
-- which run the interpreter, `--jit` and both AOT backends and fail on any
disagreement -- compare only the backends that are ported so far. The AOT
backends have nothing to fall back to, so they report the refusal and the
fixture harness skips them under the feature. The JIT can fall back, since a
function it refuses with `Unsupported` stays interpreted, but the refusal has
to cover **every function that is generic or calls a generic**, not just
those that touch a dynamic type: the interpreter and compiled code share
three hand-coded entry layouts (`bridge::call_jit`, the bytecode's
`call_planned_jit`, and the `__jit_dispatch_call` trampoline), so a compiled
caller of an interpreted generic has to pass what the new convention expects.

0. **Baseline** (done). The measurements in [Cost](#user-content-cost).
1. **Runtime** (done). Descriptor construction from templates, and its cache
   outside the allocator; the payload-offset entry for options, results and enums;
   `clone_local`'s address assertion made structural; the shadow stack in
   `RtHeader`, with a restore that covers chunk boundaries and a `reset` that
   empties it; their tests.
2. **IR** (done). `IrType::Param`; the fat-pointer layout rule, with `Ref` of
   a dynamic type as the fat pointer itself; the static offset helpers
   asserting they are never handed a dynamic type; the descriptor table on
   `FunctionContext`, carried through `specialize.rs`; display, serialization.
   There is no IR verifier to put the block-argument rule in, so it is
   `permuted_block_args`, checked as each function is lowered; script units
   are not checked, since no value in one is dynamic. The `fat-generics`
   feature exists on `datalove-datafun-lower`, and so far only fills the
   descriptor table. No lowering produces a `Param` yet.
3. **Lowering** (done), under the feature: written types kept, the table
   built, type-parameter descriptors at call sites, no `Erase` or `Reify`.
   Ownership analysis is unchanged, the language being unchanged. See
   [Where phases 3 and 4 stand](#user-content-where-phases-3-and-4-stand).
4. **Interpreter, bytecode and CTFE** (done but for the bytecode), the
   reference: the dynamic frame, the operations table above, calls. The
   interpreter's frame is the one the JIT shares for OSR, so the dynamic frame
   is laid out here once for both. CTFE is this interpreter, so it comes along.
   The bytecode is the default engine; until it lowers a dynamic body, such a
   body runs on the IR walker. The JIT refuses as above; the AOT backends
   refuse. Done when every fixture passes on the interpreter under the feature
   with leak checking on, which it does.
5. **Cranelift**, AOT and JIT (done but for OSR into a generic): the same,
   plus the bridge, the trampoline and OSR carrying fat pointers and
   type-parameter descriptors. See [Where phase 5
   stands](#user-content-where-phase-5-stands). OSR shares the interpreter's
   frame (`osr.rs`) and so would adopt its dynamic frame rather than build
   one: `OsrSpec` gains a descriptor-table offset beside `shape_offset`, and
   `enter_osr` loads it the same way. That waits on the bytecode, since only
   the bytecode enters loops and it does not run a generic's body yet.
   Cranelift's one-at-a-time block-parameter copy is safe by the lowering
   check in phase 2.
6. **C AOT.**
7. **Flip and delete.** The feature becomes the default and then goes; the
   list in [What goes away](#user-content-what-goes-away) is deleted; a call
   at a growing type gets a fixture.
8. **Docs and measurement.** `generics.md` rewritten, `plan-generics.md`
   marked superseded where it is, `native-abi.md` and `ir.md` updated, and the
   measurements recorded.

## Where phases 3 and 4 stand

`just test-fat` passes, leak checking on: every engine fixture, every
`sys/std` fixture and every backend fixture, on the walker, the bytecode and
the jit, with the AOT backends skipped where they refuse. How it came out, and
what differs from the sections above:

- **Calls carry `bindings`**, a `Vec<IrType>` on `Call` and `ComptimeCall`:
  what the site bound each of the callee's type parameters to, written over
  the caller's. `type_args` and `shape_descriptors` are left empty under the
  feature, and the shape closure is skipped (`close_shapes_over_calls`,
  `close_shapes_in_units`), as is `build_shape`. A native's trailing
  descriptors are its `descriptor_shapes` substituted with the bindings.
- **The lowering switch** is `crate::FAT_GENERICS` in `datafun-lower`, read
  at the handful of places a type is made (`LowerCtx::ir_type`, `hint_type`,
  `expr_type`, the parameter and return types in `func.rs`, and the call
  site). The rest of the erasure machinery -- `container_shape`,
  `wrapped_shapes`, `open_container` -- fires only on an IR type of `data`,
  which a generic's values no longer have, so it is inert rather than
  switched off. `type_param_count` is set only under the feature, which is
  how every backend tells a generic compiled by reference from an erased one.
- **The interpreter resolves a fat pointer in the frame accessors.** A value
  or slot of a dynamic type has a null static descriptor in `IrLayout`, and
  `Frame::value`, `value_dest`, `slot` and `slot_dest` read the fat pointer
  when they see one. Everything the walker does goes through those, and its
  operations were already driven by the descriptor in hand, so almost no
  instruction needed changing. A reference to a dynamic type is a fat
  pointer whose second word is its descriptor word (`ref_desc_offsets`).
  `enter_dynamic` fills the table and takes shadow stack space after
  `Frame::enter`; `leave_dynamic` gives it back.
- **Which side gives an argument up.** A type parameter is never a copy type
  in the callee, but a copy value handed to one is copied, not moved, so for
  a dynamic parameter the caller asks the argument's own descriptor
  (`TyDesc::PLAIN`), not the callee's type.
- **The bytecode** runs a body with a descriptor table on the walker
  (`run_frame`), and lowers a call with bindings to the general lane, since
  its fast paths do not hand over type parameter descriptors.
- **The JIT refuses** any function that is generic or calls a generic
  (`uses_generics_by_reference`), and the AOT backends refuse a program with
  one; the engine and backend harnesses skip a refusal under the feature.
- **Fixtures whose output differs** under the feature have an
  `.out.expected.fat` beside the default, as `index-64` has `.expected.64`;
  blessing under the feature writes one only where the output differs. Eight
  engine fixtures differ only in the IR they print. `module_060` now compiles,
  as it should. Backend fixture `22_comptime_and_generic` is a fix: under
  erasure a generic's `debuglog x` printed `data "said"`, the wrapper leaking
  into the output, and it now prints `"said"`.
- **A call at a growing type** -- a generic calling itself at `[T]` -- runs,
  building one more descriptor per level. It gets a fixture in phase 7, since
  the default configuration still refuses it.
- **A bounded operation on a `T`** is dispatched by the interpreter on the
  descriptor's tag, which is now the real type rather than `data`; so a
  `fixedint` `T` bound to `u32` reaches the native `u32` arithmetic, and the
  one operation with no unsigned case, checked negation, goes to `dyn_ops`.

Not done in phase 4, and wanted before phase 7: the bytecode lowering
dynamic bodies itself, rather than leaving them to the walker; and the
measurements, since nothing compiled runs a generic yet.

## Where phase 5 stands

Under the feature the JIT and Cranelift AOT compile generics; the full suite
passes with them doing so, and C AOT still refuses.

- **Calling convention.** A dynamic parameter is passed by pointer, like any
  parameter, and one descriptor per type parameter comes last in the
  signature. The callee does not take a descriptor per dynamic argument: its
  table entry for the parameter's type describes it, so the plan's `(ptr,
  tydesc)` per argument was left to natives. Where something has a dynamic
  argument's pointer and not its descriptor -- the JIT's trampoline into the
  interpreter -- it describes it from the type parameters
  (`IrInterpreter::describe`).
- **A dynamic value** is an aggregate whose Cranelift value is the address of
  its storage on the shadow stack, taken on entry, inline, calling the runtime
  only when a chunk is full (`codegen/dynamic.rs`). Moves alias, as they do
  for any aggregate. Its descriptor is its table entry: `static_tydesc` and
  `tydesc_value` answer a dynamic type there, and `tydesc` refuses one, so a
  site that wanted a static symbol fails to compile rather than compiles
  wrong. Sizes come from the descriptor (`value_size`, `copy_value_bytes`),
  and offsets from the runtime (`field_addr`, `payload_addr`).
- **The descriptor table** is filled through a one-entry cache per function,
  a writable data object holding the last call's type parameter descriptors
  and what they built; a repeat binding costs a compare per type parameter.
  This was the fix the plan expected, and it was needed: before it, the table
  fill was a fifth of compiled `sieve`.
- **A gate** (`check_dynamic_support`) refuses a function using an
  instruction on a dynamic value that the codegen does not handle, naming it.
  It now admits everything `sys/std` and the fixtures use; the JIT's refusal
  of generics is gone, and `--jit-stats` lists any refusal with its reason.
  `ENGINE_TESTS_JIT_REFUSALS=1` makes the engine suite print them.
- **The bridge, the trampoline and the dispatcher** carry the type parameter
  descriptors as the trailing descriptors, where erasure's shapes went; a
  function has one or the other.
- **Found on the way:** `dyn_ops` read an `index` or `offset` as eight bytes
  whatever `index-64` said, unreached under erasure because those arrived
  boxed; and `needs_erasure` had no shortcut for one descriptor on both
  sides, which is every call now.

Measured, release build, against the baseline (seconds):

| Bench | jit | aot |
|---|---:|---:|
| generics | 0.51 (0.97) | 0.33 (0.77) |
| sieve | 1.19 (1.21) | 0.87 (0.95) |
| nbody | 0.21 (0.42) | 0.16 (0.30) |
| wordfreq | 0.64 (0.61) | 0.55 (0.52) |

nbody and wordfreq were measured before the table cache. Interpreted runs are
slower, a generic's body running on the walker: sieve 10.1 (4.1), nbody 3.2
(0.8). That is what the bytecode lowering a dynamic body is for.

## Open questions

- **Entry-time or lazy storage.** Bumping once at entry for every dynamic
  value costs per call in proportion to how many the body has, used or not.
  Allocating each on its first definition costs a test per definition
  instead. Entry-time is simpler; measure on `sys/std` before choosing.
- **The table-fill cache.** Whether the one-entry cache per function in
  [Descriptors](#user-content-descriptors) is needed, and whether it lives in
  the callee or at the call site.
- **Inline containers.** A `[T]` header is the same three words whatever `T`
  is, so it could live in the frame with its descriptor beside it rather than
  behind a pointer. An optimization for after, once everything is one shape.
- **Fat pointers in registers.** Cranelift can keep the two words as two
  variables rather than a struct in a stack slot, and C as two locals. Worth
  doing in phase 5 if it is not much harder. OSR reads the frame, so at a
  loop header the JIT can enter, a fat pointer has to be in its frame slot.
- **The rider ABI.** Whether natives move to one descriptor per type
  parameter, and when.

Settled since the first draft:

- **Where the cache lives.** Per runtime handle, outside its allocator; see
  [The runtime](#user-content-the-runtime). A shared cache would outlive the
  descriptors its keys point at.
- **Tracking bytes.** They live in a region after the values and slots in the
  fixed frame, only for tracked slots and `out` parameters
  (`frame_layout.rs`). A fat pointer is fixed-size, so the fixed frame keeps
  its shape and the tracking bytes stay where they are; the dynamic frame
  holds only what fat pointers point at.
