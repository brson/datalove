# Generics: how it works

A generic function is compiled once, whatever it is called with. A type
parameter has no fixed size, so the positions it stands in are shaped to fit any
type, and a *type descriptor* -- the runtime's `TyDesc` -- says what is really
there whenever that matters.

This describes the implementation as it stands. The language-level rules are
[Section 8.5 of the spec](botspec.md#user-content-85-generic-functions); the
reasoning that led here, and what was considered instead, is in
[Generics and Specialization](plan-generics.md).

## Contents

- [The two halves](#user-content-the-two-halves)
- [Erasure: what a position becomes](#user-content-erasure-what-a-position-becomes)
- [Where descriptors come from](#user-content-where-descriptors-come-from)
- [Getting further in](#user-content-getting-further-in)
- [Opening a wrapped container](#user-content-opening-a-wrapped-container)
- [Converting between shapes](#user-content-converting-between-shapes)
- [What the runtime provides](#user-content-what-the-runtime-provides)
- [What each backend carries](#user-content-what-each-backend-carries)
- [What is refused](#user-content-what-is-refused)
- [Why not monomorphization](#user-content-why-not-monomorphization)
- [Where the tests are](#user-content-where-the-tests-are)

## The two halves

**Erasure** decides the shape a value has inside the generic. **Descriptors**
say what is actually in that shape. Neither is enough alone: a shape that fits
any type cannot say which type, and a descriptor with nothing to describe is a
pointer to nowhere.

The invariant the whole thing rests on:

> A value's static type describes its layout -- except where a descriptor says
> otherwise, and then the descriptor is reachable from wherever the layout is
> needed.

Every fault this area has had was that second clause not holding: something
computed an address, a width or a stride from a static type that had been
erased, with no descriptor to hand.

## Erasure: what a position becomes

`erased_param_type` in `datafun-ir` is the single place that decides. The caller
and the callee both read it, because a call site converting into a shape the
callee was not compiled for is not a mismatch anything reports -- it is two
sizes disagreeing about the same bytes.

It forks on whether the position is **borrowed**.

### Owned: `in`, `out`, and the return

Converted at the boundary, by `erased_owned_type`:

| Written | Becomes | Cost |
|---|---|---|
| `T` | `data` | wraps; a narrow scalar rides in the two words, anything else is boxed |
| `?T`, `!T`, `(T, u32)`, `{a: T}`, `term W T`, an enum payload | the same shape with the parts converted | a walk over as many parts as the type has |
| `[T]`, `#{T}`, `%{K = V}`, a tensor, a table | `data`, wrapped whole | one small allocation, elements untouched |

The third row is the one worth explaining. A list is a pointer, a length and a
capacity whatever its elements are, so it is already the right size and there is
nothing to convert it *into*. What it lacks is the element type, and wrapping is
what gives it somewhere to keep one. Converting `[u32]` into a list of `data`
would mean rebuilding it element by element -- an O(n) the design avoids by not
doing it.

The rule reaches inside a composite, so `([T], u32)` becomes `(data, u32)`: a
tuple converts field by field, so a container field is converted too, and
wrapping is the only conversion a container has.

A return is an owned parameter going the other way -- nobody keeps a copy -- so
`erased_return_type` is `erased_param_type` with `borrowed: false`. That is why
nothing is smuggled on the return path: what comes back is a `data`, which
describes itself.

### Borrowed: `ref` and `mut`

**Nothing is converted.** The value is passed as it stands, so the callee's
static type for it is the erased one -- `{a: data, b: u32}` where the caller
holds `{a: u8, b: u32}` -- and that type is a *lie about the layout*. The
descriptor for what really arrived comes from the call site.

This is the case everything in the rest of this document exists to serve.

## Where descriptors come from

Three channels, and which one applies is decided by where the value came from.

### 1. The value itself

A `data` carries its own descriptor. That covers every owned position: an owned
container, an owned bare `T`, and every return. Nothing is passed alongside.

### 2. `descriptor_params` -- for a borrowed value

`FunctionContext::descriptor_params` lists the parameters whose own type does
not describe what arrives. It is built in `datafun-lower/src/func.rs` under
exactly the condition that makes the type a lie:

```rust
if borrowed && type_hint_mentions_param(&p.type_hint, &type_params) {
    descriptor_params.push(id);
}
```

The call site fills each one with the argument's concrete descriptor. It is the
only place that knows.

### 3. `descriptor_shapes` -- for a value being built

A value that *arrives* can be described by something that arrived with it.
A collection written in the body has no such value: `var out: [T] = []` has
nothing to read a descriptor off. So the body declares the shape it needs and
the call site hands one over as a trailing argument.

`FunctionContext::descriptor_shapes` holds those, in the order they arrive.
Nothing is constructed at run time: `[T]` with `T` bound to a concrete type is a
concrete type, and that has a static descriptor already.

Which shapes a function needs is what its own body builds, **closed over its
calls**: a generic passing its type parameter to one that builds a collection of
it carries a descriptor too, and forwards it. `close_shapes` settles that, and
`resolve_call_descriptors` then records what each call site hands over, as a
`DescriptorRef`:

- `Static(IrType)` -- a type this call site knows outright, so a static symbol.
- `Own(i)` -- one this function was itself handed, forwarded whole.

Both are worked out once and stored on the `Call`, rather than re-derived per
backend.

## Getting further in

A borrowed value's descriptor arrives at the *parameter*. Reading a part of it
needs the descriptor of that part, and a projection that dropped the descriptor
would leave the next step with nothing but the lie.

`RefDesc` says what a reference points at, where its static type does not.
`resolve_ref_descriptors` derives the whole map for a unit:

| variant | means |
|---|---|
| `ParamField { param, index }` | field `index` of a borrowed parameter's descriptor |
| `RefField { base, index }` | field `index` of the descriptor worked out for `base` |
| `ParamElement { param }` | what a borrowed parameter's container holds |
| `RefElement { base }` | what the container described for `base` holds |
| `Unwrapped` | the descriptor a `DataBorrow` read out of a wrapper |

A value **absent** from the map is the ordinary case: its static type describes
it, the offset folds at compile time, and the emitted code is what it always
was. So "every reference carries a descriptor" is true as a rule and costs
nothing outside a generic.

The map is **derived, not stored**. Each backend calls
`resolve_ref_descriptors` once for the unit it is about to compile. A field on
`IrCodeUnit` would have to survive specialization renumbering values and
inlining offsetting them; deriving on demand cannot go stale.

Roots are `descriptor_params` and `DataBorrow`. Nothing else roots one: a local
holding an erased value holds a `data` for real, so its static type is honest.

Consumers take their layout from the descriptor when the reference is fat and
are untouched when it is not: field offsets, read-through widths, element
strides, the descriptor handed to a callee, and `DropViaRef`.

## Opening a wrapped container

An owned container arrives wrapped, so a generic holding `xs: [T]` by value
holds a `data` and cannot index it directly.

`Instruction::DataBorrow` opens it: the pointer and the descriptor come out
together (`dtlv_rti_data_borrow`), and the reference it produces carries the
descriptor as `RefDesc::Unwrapped`. Everything downstream is the borrowed case,
which already works -- so nothing needed a new way of *indexing*, only a way of
getting at the container.

Knowing there *is* a container to open is the awkward half.
`IrType::from_datalit` collapses a container of a type parameter to `data`
everywhere -- parameter, local and call result alike -- so the IR type says
nothing about being a list. An index expression can ask the typechecker for the
unerased type (`container_shape`), but a *place* like `xs[0]` has no expression
node for its root. So the shape is recorded where the binding is made, in
`LowerBody::wrapped_shapes`, at the two places that bind a wrapper: a parameter,
from its type hint, and a `let` or `var`, from its initializer.

That table is the one piece of this that is bookkeeping rather than descriptors.
It would go away if an owned container erased structurally to `[data]` and took
a descriptor the way a borrowed one does -- the header is the same three words
either way, so nothing would be rebuilt.

## Converting between shapes

`convert` in `datalove-rt/src/impls/boxing.rs` walks two descriptors and
converts at every position one of them calls `data`. It handles options,
results, terms, enums, tuples and structs, recursing structurally, and it is
driven entirely by the descriptor pointers -- so one found at run time does as
well as a static one.

Two directions and one variant:

- `erase_local` -- into the erased shape. Moves.
- `reify_local` -- back out of it. Moves.
- `clone_erased_local` -- into the erased shape, cloning. A value read out of
  something borrowed has to be left where it is, so this clones in the real
  shape first and moves the clone (`clone_into_erased_shape`).

**Whether anything was erased is a structural question, not a size one.** A
`data` is two words and so is a `string`, so `{a: string, b: u32}` and
`{a: data, b: u32}` are the same tag *and* the same width while being different
layouts. `needs_erasure` walks the two descriptors looking for a position where
one says `data` and the other does not. Comparing widths instead writes a string
where a wrapper belongs.

The same three-way decision appears wherever a value crosses between shapes, and
is always the runtime's rather than each backend's: a destination that is a
`data` holding something that is not one wants packing, a destination that
really is a `data` wants a plain clone, and from a call site the two look alike.
`dtlv_rti_list_get_erased_local` makes it for an element, `field_read` for a
field, `clone_erased_local` for a clone.

`needs_erasure` is **directional**, and both directions get asked. An element
pushed into a collection built inside a generic is in the shape the generic
holds it in -- a `(A, B)` is a tuple of two `data` -- while the slot is the real
`(string, u32)` the collection's descriptor names, so that one reifies. A value
coming back out of a `data` is usually the other way round. `data_into_local`
asks both and picks, which is what lets a collection hold an element that is a
composite of type parameters rather than only a bare one.

## What the runtime provides

| Function | Answers |
|---|---|
| `dtlv_rti_field_offset` / `dtlv_rti_field_tydesc` | where a field is, and what it is |
| `dtlv_rti_element_tydesc` | what a container holds (a map's *value*) |
| `dtlv_rti_field_read_local` | read a field out, packing it if the destination wants that |
| `dtlv_rti_data_borrow` | the pointer and descriptor inside a wrapper |
| `dtlv_rti_erase_local` / `dtlv_rti_reify_local` / `dtlv_rti_clone_erased_local` | convert between two shapes |
| `dtlv_rti_list_get_erased_local` | read an element, packing it if the destination wants that |
| `dtlv_rti_btreemap_contains_key_erased_local` / `..._get_value_ref_erased_local` | look up a key that may have arrived packed |

These are in the runtime rather than open-coded per backend for one reason: four
derivations of one question is where the bugs come from. Two concrete traps make
it more than a principle. `TyInfoTuple` is `{num_fields, fields}` and
`TyInfoStruct` is `{fields, num_fields}` -- pointer and count swapped -- so a
hand-written descriptor walk gets one of them wrong. And whether a key arrived
packed is a comparison between two descriptors (`unwraps_the_element`,
`borrow_lookup_key`) that a call site cannot make from its static type alone.

## What each backend carries

The **interpreter** needs the least: its values are already a pointer and a
descriptor, and a parameter's is per-frame (`Frame::param_tydescs`). What it
lacked was anywhere to put one for an SSA value, which is `Frame::value_tydescs`
-- written by the projections, read by `value_deref`.

The **compiled backends** take one extra pointer parameter per entry in
`descriptor_params`, then one per entry in `descriptor_shapes`, after the
ordinary parameters and in that order. Cranelift keeps materialized descriptors
in `ref_desc_values`; the C backend declares a `__rd{n}` per fat reference in the
function prologue, so a `goto` between blocks never jumps over a declaration.

The **jit** compiles through the same cranelift codegen, so jit code passes both
groups the same way. What it adds is two boundaries of its own, and both have to
know that there are two groups and not one:

- `bridge::call_jit` builds the argument list when the interpreter enters jit
  code. It takes the `descriptor_params` and the shape descriptors separately
  and appends them in that order.
- `__jit_dispatch_call` is the trampoline out of jit code. It reads the
  parameter descriptors off the front of the array it was handed and the shape
  descriptors off the back, then either forwards both to `call_jit` or hands
  them to `call_in_context_with_shapes`.

This paragraph used to say that a dropped descriptor was closed by construction,
because `LocalCallee` pairs a declared function with the `descriptor_params` its
signature asks for and the stub forwards what it was handed. That covers the
first group and not the second: the trampoline consumed only the parameter
prefix and called `call_in_context`, so a jit-compiled function calling
`sys/std/list.reversed` built its result from whatever was in the register, and
every other backend was right. `backend/23_shape_descriptors_across_jit.dfs` is
the case the other fixtures missed -- a plain function calling a generic that
builds, which is ungated and therefore compiled, where every generic fixture
before it called from the script unit body or from another generic that
forwards.

A `CallDispatcher` gets them too: the shape descriptors travel on
`DispatchCallContext::shape_descriptors`, so the interpreter offers a
shape-taking call to the dispatcher like any other. What the jit still refuses
is width rather than shape: `bridge::enterable` turns away a function whose
parameters and descriptors together exceed `MAX_DIRECT_ARGS` words, since
`call_jit` has no function pointer type to enter it through.

The **inliner** has a related rule, for a different reason. `inline_call_site`
refuses a callee whose `descriptor_shapes` is non-empty, because its body names
those shapes by index -- `DescriptorRef::Own` on a call, `descriptor` on a
`ListNew` -- and an index means a different shape, or nothing at all, in the
caller's list. What it does carry across is `DescriptorRef::Static`, which names
a whole type and so is valid in any body. A function that is concrete still
hands static descriptors to the generics it calls, and blanking them on the way
in entered the callee with nothing to build from
(`interp/989_inline_static_descriptor`).

The jit also needs runtime symbols registered twice: once in
`register_runtime_symbols` and once in `trampoline_all_runtime_imports`. Nothing
catches a missed one at compile time -- it aborts at run time.

One rule at a call site is easy to get backwards. An argument whose static type
reads `data` may be a *wrapper* to unwrap, or a *pointer at the value* that
merely reads as `data` because it was erased. Anything carrying its own
descriptor is the second: `operand_ref_desc(arg).is_some()` is the test. Read
the wrong way, a string's first two words are taken for a wrapper's pointers.

And one about ownership. A call consumes its `in` arguments, and the call is
what ownership analysis sees taking them -- but an erased parameter puts a
conversion in between, so the call takes the *erased value* and the binding it
came from is never seen going. A non-copy binding has to be moved out before
the conversion, or it is destroyed at the end of its scope as well as by the
callee. A copy binding is left where it stands, which is what lets one be
erased into two calls.

## What is refused

**No bounds, so no operations on a `T`.** A value of a bare type parameter can
be moved, dropped, cloned, printed, stored, returned and handed on. `x + y` and
`x == y` are errors whatever the call site supplied. Printing is the exception
because the descriptor travelling with the value is enough to format it.

`float`, `fixedint` and `ord` bounds do exist and dispatch through the runtime
on descriptors (`dyn_ops`), which is how `sys/std` writes arithmetic over a type
parameter and how a set or map over one gets its ordering.

**A generic that builds a collection and recurses at a strictly larger type.**
Building `[T]` and calling a generic at `[[T]]` needs a descriptor one level
deeper at every call, so the set never settles. `close_shapes` decides this from
the call graph rather than from how big a shape gets: each call draws an edge
from the callee's type parameter to the caller's, *strict* when the binding puts
a type constructor around it. A cycle of plain edges is a renaming and cannot
enlarge anything; a cycle holding one strict edge adds a level every lap. That
is the occurs check read across the call graph, which is what keeps a merely
deep type from being mistaken for a growing one.

**A generic `out` parameter has to be a whole binding or a field.** Both work,
and so does passing one straight on; anything else is refused rather than
written wrong.

## Why not monomorphization

Erasure costs at run time: a local of type `T` is two words rather than a
register, type-directed operations are indirect calls into the runtime, and call
sites convert at the boundary.

What it buys is what a language with a REPL needs. Compile time is O(1) in the
number of instantiations -- a new one appearing in the REPL compiles nothing.
There is nothing to invalidate, because a generic function is one salsa-tracked
entity rather than a family whose membership depends on the whole program. And
the whole-program assumption goes away: monomorphization needs to see every call
site before it can emit the callee, which is exactly what a REPL cannot give,
since call sites arrive one at a time.

Monomorphizing what is *hot* remains open, and would reuse the counters
`optimizing.rs` already keeps for inlining and jit compilation. It is an
optimization over a complete implementation rather than a different design.

## Where the tests are

`crates/datalove-cli/tests/fixtures/backend/` runs the interpreter, the jit and
both AOT backends and compares them, so a recorded output is what all four
agree on rather than a blessed difference:

| Fixture | Covers |
|---|---|
| `10_generic_indexing` | reading an element whose type a generic cannot name |
| `15_borrowed_generic_aggregate` | field offsets under `ref` and `mut` |
| `16_generic_element_borrow` | a borrow taken out of a borrowed list |
| `17_generic_map_tensor_index` | map lookups with a packed key, and tensor strides |
| `18_generic_composite_part` | a part whose erased type holds a `data` |
| `19_owned_generic_container_index` | indexing a container the generic owns |

`crates/datalove-datafun/tests/fixtures/std_tests/` has the language-level
coverage: `102_generic_forwarding`, `105_borrowed_param_forwarding`,
`113_return_position_inference`, `114_build_generic_collections`,
`115_shape_closure_recursion`, `116_build_generic_sets_and_maps`,
`119_deep_but_not_growing`, and the bound tests `124`-`127`.

Four backends agreeing is load-bearing here. Several of these faults were
*silent* -- a wrong answer, not a crash -- and two were identical in all four,
which is the case comparing them does not catch. Those needed a fixture that
says what the answer should be.
