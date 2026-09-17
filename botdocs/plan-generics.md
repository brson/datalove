# Generics and Specialization

How datalove should implement type parameters, and what to do about const parameter
specialization now that it has been built and measured.

The short version: generics by erasure and type descriptors, which the runtime already
does everywhere; monomorphization later and only for code the JIT has already decided is
hot. Const parameters are a separate feature that should drop union-branch for plain
monomorphization.

## Contents

- [Why not the const parameter machinery](#user-content-why-not-the-const-parameter-machinery)
- [What already exists](#user-content-what-already-exists)
- [The design](#user-content-the-design)
- [Cost](#user-content-cost)
- [Surface syntax](#user-content-surface-syntax)
- [Const parameters after this](#user-content-const-parameters-after-this)
- [What a first attempt found](#user-content-what-a-first-attempt-found)
- [What the erased shape is for](#user-content-what-the-erased-shape-is-for)
- [Carrying the descriptor with the value](#user-content-carrying-the-descriptor-with-the-value)
  -- including [a borrow taken out of a borrowed container](#user-content-what-this-does-not-leave-a-borrow-taken-out-of-a-borrowed-container),
  parked
- [Where this stands](#user-content-where-this-stands) -- what works, and the
  current limitations
- [Order of work](#user-content-order-of-work)
- [Prior art](#user-content-prior-art)

## Why not the const parameter machinery

Const parameter specialization looks like a foundation for generics and is not one. The
reason is worth stating precisely, because it is the impasse earlier work reached without
naming.

Union-branch keeps a single function signature by *removing* the const parameter and
adding a tag:

```
fun repeat(const n: i32, s: string): string
->  fun repeat(tag: i32, s: string): string     // one signature, N branches
```

Every branch agrees on the signature because the parameter that varied is gone. A type
parameter cannot be removed that way, because it is not only a parameter, it is the type
of other parameters and of the return:

```
fun swap(const T: type, x: T, y: T): (T, T)
->  fun swap(tag: i32, x: ???, y: ???): ???     // nothing to write here
```

`const-param-specialization.md` reaches this point and offers three answers: a union
sized to the largest instantiation, a pointer plus a size descriptor, or boxing. All
three are the same answer, which is *adopt a uniform representation*, and that is a
different mechanism with nothing to do with tags. Monomorphizing a value keeps the
signature; monomorphizing a type changes it. The machinery stops there.

The second reason not to build on it: union-branch does not pay for itself even for const
values. `build_dispatch_blocks` clones every body block once per instantiation, so the
output is monomorphization plus a `Switch` on a discriminant that every call site passes
as a literal, and fusing the instantiations into one symbol defeats the per-function call
counting in `optimizing.rs`. It is dominated by the simpler thing.

What *is* reusable is CTFE: the const binding graph, `ConstValue`, and evaluating const
expressions on the real interpreter. That is a genuine prerequisite for const generics
and for any type-level evaluation.

## What already exists

The pieces generics need are mostly built, in the runtime rather than the compiler.

**The runtime is type-descriptor driven throughout.** `dtlv_rti_clone_local`,
`eq_local`, `cmp_local`, `move_value_local`, `any_destroy_local` and
`pretty_print_local` all take `(ptr, tydesc)`. Clone and drop, the two operations linear
types make type-directed, are already dispatched on descriptors at runtime rather than
compiled per type.

**Collections are already generic.** `dtlv_rti_btreemap_insert_local` takes key and value
type descriptors. Lists, maps, sets, tensors and tables are implemented once for all
element types. Nothing about `[T]` or `%{K = V}` needs monomorphization to run.

**There is a working erased representation.** `data` is two words using the `anypack`
encoding, with tags `SmallImmediate`, `InlineWithTyDesc` and `TwoPointers`, so small
scalars are held immediately and only larger values indirect. The spec already says any
type coerces to `data`. This is a uniform representation with a test suite.

**The calling convention exists.** The native rider ABI is `(ptr, tydesc)` pairs with an
out-parameter for the return. That is what a function called with values whose types it
does not statically know requires, and `rider.dli` already declares signatures over
`data`.

The missing piece is in the compiler: `IrType` is a closed enum with no type variable, so
no IR function can be parametric.

## The design

Two layers. The first is the language feature; the second is an optimization that can
come much later or never.

### Layer 1: erasure with type descriptors

A generic function is compiled once. Values of a type parameter are held in the uniform
representation and accompanied by a type descriptor, which the function passes to the
runtime for anything type-directed.

```
fun unwrap_or(self: ?T, default: T): T
```

compiles to a single IR function taking a descriptor for `T` alongside the erased values,
and lowers `clone`, `drop` and comparison on `T` to the existing `dtlv_rti_*` calls with
that descriptor. Call sites erase at the boundary and reify on the way out.

Properties that matter here:

- Compile time is O(1) in the number of instantiations. A new instantiation appearing in
  the REPL compiles nothing.
- Nothing to invalidate. A generic function is one salsa-tracked entity, not a family
  whose membership depends on the whole program.
- The whole-program assumption goes away. Union-branch and monomorphization both need to
  see every call site before they can emit the callee; erasure does not, which is what
  makes it work in a REPL where call sites arrive one at a time.
- `option`, `result`, `list`, `map` and `set` over any `T` become writable immediately,
  which is the actual reason generics are wanted.

### Layer 2: monomorphize what is hot

`optimizing.rs` already counts calls, inlines at 50 and JIT-compiles at 100. Specializing
a generic function for a concrete descriptor is the same kind of decision and should
reuse the same counters: when an instantiation is hot, compile a version with the
descriptor fixed, unboxed and its runtime calls devirtualized.

This is Julia's arrangement and .NET's, and it puts the cost where a scripting language
can afford it. Nothing is specialized to start, so startup stays fast; code that runs
enough to matter gets specialized, and only that code.

Layer 2 is optional. Layer 1 is a complete, correct implementation on its own.

## Cost

Erasure is free at compile time, not at run time, and the honest accounting is:

- A local of type `T` inside a generic function is two words rather than a register. For
  `unwrap_or` on a `u32` that is an unbox where monomorphization would have a move.
  `SmallImmediate` means no allocation for scalars, but it is not nothing.
- Type-directed operations become indirect calls into the runtime instead of inline
  instructions. Again, already true of everything the runtime does today.
- Call sites erase and reify at the boundary.

Against that: no code growth, no compile-time growth, no whole-program requirement, and a
REPL that does not recompile when a new type shows up. For a language whose default
execution tier is an interpreter, paying an unbox to avoid a compile is the right side of
the trade. Layer 2 buys the difference back where it is measurable.

## Surface syntax

Use `<T>`:

```datalove
fun unwrap_or<T>(self: ?T, default: T): T
```

not Zig-style `const T: type`. The earlier recommendation of the Zig form was reasoning
from union-branch, which needed types to be const values so the same tag machinery would
apply. With erasure that machinery is not involved and the argument disappears, leaving
the costs: template-style generics cannot be checked before instantiation, so errors land
inside the callee and constraints are implicit; and the known weak points are exactly
autocomplete, hover documentation and incremental compilation. Those are the properties
this compiler is built around, with a REPL and an LSP on the roadmap.

`<T>` also keeps type parameters and const parameters visibly different, which they are:
one is erased and passed as a descriptor, the other is known at compile time and folded.

Bounds can wait. With erasure, a generic body may only do what the runtime can do for any
type — clone, drop, compare, print, store in a collection — so there is nothing to
constrain yet. Bounds become necessary when generic code needs operations that are not
universal, and that is a later design.

## Const parameters after this

Independent of generics, and worth doing regardless:

- Replace union-branch with monomorphization. Smaller, faster to compile, simpler code,
  and it restores per-instantiation tiering. Keep the differential test suite, which is
  the valuable part of the existing work.
- Reconsider the const-binding-only restriction. `pow(2, 10)` failing because `10` is a
  literal is a bad first impression, and once specialization no longer depends on
  pre-resolved `ResolvedConsts` lookups the restriction is easier to lift.
- `ConstValue::Type` may still be wanted, for const generics such as tensor dimensions.
  It is not a step toward type parameters.

## What a first attempt found

Written after building the front end far enough to hit the real boundary, then
reverting it. Measurements, not estimates.

**`<T>` parses for free.** `Sigil::AngleOpen` is already a bracer with a close
pair, so `fun identity<T>(x: T): T` arrives at the parser as one branch, the same
shape as the parameter list. Adding `type_params: Vec<InternedText>` to `StmtFun`
and reading that branch is about thirty lines, and `T` then reaches the
typechecker as `UnresolvedTypeAlias("T")`, which is the correct next error.

**A type variable in the datafun `Type` is cheap.** Only five files match on it,
and adding `Var(InternedText)` needs `types_equivalent` (equal to itself),
`type_to_string`, and about twenty-five mechanical arms in the type hint
converter. Resolution then falls out for free: seed the function's type
parameters into the alias map that `convert_type_hint_with_aliases` already
consults, and `T` resolves wherever a named type would.

**But that only reaches bare `T`, which is not the goal.** Those twenty-five arms
are all the same shape, converting a nested type hint into a *datalit* type:
`[T]`, `?T`, `(T, T)` all require the variable to live in
`datalit::tycheck::Type`, not in the datafun wrapper. The stdlib motivation is
`option.unwrap_or(self: ?T, default: T): T`, which is nested. So the front end
splits in two: bare `T` is a small change to datafun's type, and useful `T` is a
change to datalit's, which is the type of every data literal in the language.

**The back end is the wall.** `IrType::from_tycheck` is infallible, so a type
variable has to become some `IrType`. Fifty-four files reference `IrType` and
about eighty arms match it exhaustively, across the interpreter, Cranelift, the C
backend, layout, tydesc emission and ownership. Each is a place to answer what a
generic value's size and representation are, which is one decision but eighty
edits, and none of it is the calling convention work.

**Erasing `T` to `data` avoids all of that, and is the better plan.** This
document first dismissed it, on the grounds that `report-match-downcast.md` lists
downcasting out of `data` as an open question. That confuses a missing surface
syntax with a missing mechanism. A generic function never asks a user to write a
downcast: the compiler erases at the call site, where it knows the concrete type,
and reifies on the way back. What it needs is an IR instruction, not syntax.

The mechanism is already there. `anypack` exposes `value_ptr`, `value_ptr_as<T>`,
`tydesc` and `tytag`, plus a typed accessor for every type in the language:
`as_u32`, `as_string`, `as_list`, `as_option`, `as_result` and the rest. Wrapping
exists as `Instruction::DataFrom`. Only the extract is missing.

What this buys, against the `IrType::Generic` route:

- No new `IrType`. A generic function compiles once with each `T` replaced by
  `data`, and `data` is an ordinary type every backend already handles. The
  eighty match arms do not happen.
- Nested positions come free. `?T` becomes `?data` and `[T]` becomes `[data]`,
  both ordinary types, so datalit's `Type` does not have to change either.
- The generic body compiles like any other function. Frames, drops and clones all
  work, because dropping and cloning a `data` is already tydesc-driven.

Both erased signatures compile and run today, checked before writing this down:

```datalove
fun identity(x: data): data
fun unwrap_or(self: ?data, default: data): data
```

So the remaining work is an extract instruction, wrap and unwrap insertion at
generic call sites, and the front end. One instruction across three backends
rather than a representation decision across eighty arms.

Two costs to know about.

**Every wrap allocates.** `data_from_local` heap-allocates and copies
unconditionally, so erasing a `u32` costs an allocation. `anypack` has
`can_inline` and `from_immediate` for exactly this case and boxing does not use
them, so the fix is local and worth doing before measuring anything.

**Containers do not want this.** Converting one element by element at the
boundary would be O(n) and would allocate per element: `list.len(ref self: [T])`
would copy the whole list to ask its length. Containers want the tydesc route
instead, which the runtime already does natively and which the rider ABI already
speaks. So the split is scalars, options and results by erasure, which is the
stdlib blocker, and containers by descriptor.

Written before it was clear that a container never needs converting at all. See
[What the erased shape is for](#user-content-what-the-erased-shape-is-for).

The order below is unchanged by this, but step 2 gets much smaller: `T := data`
rather than a new IR type.

## What the erased shape is for

Erasure exists to make a value of unknown size fit a slot of known size. A
function compiled once has one frame layout, and a `T` standing alone could be
a four-byte `u32` or a sixteen-byte string or a struct of any width, so the
slot holding it is a `data`: two words, with the value inside or on the heap.

That reasoning is about size, and it does not reach every type a parameter can
sit inside. Three cases, and they are not alike.

**A container is already the right shape.** `IrType::List(_)` computes its
layout as `struct_layout::<rtdt::List>()`, and the `_` is the point: a list is
a pointer, a length and a capacity whatever its elements are. `[u32]` and
`[data]` are the same size and the same alignment. Sets, maps, tensors and
tables are the same. The element type lives in the descriptor and nowhere else,
which is exactly why `sys/std/list.get(ref self: [T], i)` reads `u32`s out of a
real `[u32]` today. Converting `[u32]` into `[data]` would be O(n), but nothing
asks for it: there is no shape to convert into.

**A tuple or a struct is not.** `(u32, u32)` is eight bytes and `(data, data)`
is thirty-two, so those do need converting. The cost is a walk over the fields,
and the type fixes how many there are: `compute_tuple_layout` gives the offsets
and `iter_tuple_fields` gives the descriptors, so it is the walk `?T` already
does with more than one payload. Bounded by the type rather than by the data,
which is the distinction the O(n) argument was reaching for and missed.

**A bare `T` genuinely needs the box.** Nothing else makes an arbitrary type
fit a fixed slot short of sizing frames at run time.

So the line is not scalar against composite. It is: does the erased shape
differ from the concrete one, and if so, is the walk between them bounded by
the type or by the data.

### What owned containers actually need

Not a conversion. A borrowed container already crosses unconverted, with the
call site supplying the descriptor its own type cannot give. An owned one would
cross the same way. What owning it adds is that the callee drops it, and drop
takes a descriptor:

```rust
let ty = self.get_operand_type(operand)?;   // compile_drop
```

That reads the static type, which inside a generic says `[data]`. Destroying a
real `[u32]` through it would walk each four-byte element as a sixteen-byte
anypack. That is the corruption the current refusal prevents, by accident
rather than by design -- a borrowed container never reaches it because the
callee never drops one.

So owned containers split by what the body does with the parameter:

- Only forwards it or lets it drop. The descriptor is the one the call site
  supplied, keyed to the parameter, which is what `descriptor_params` already
  carries. Drop has to prefer it over the static type.
- Moves it into a local, into another container, or returns it. Then the
  descriptor has to travel with the value rather than the parameter, which is
  the same missing piece behind indexing and behind the modes and return types
  a type parameter cannot reach.

Which makes the value-level descriptor the keystone rather than one item among
several: owned containers are not a separate cost problem sitting behind it,
they are one of the things it unlocks.

### What a prototype of that found

Built and thrown away, to find where the line actually falls rather than argue
about it. The change was small: a predicate saying a container of a type
parameter crosses unconverted, read by the caller so it emits no `Erase`, by
the callee so the parameter joins `descriptor_params`, by the interpreter so it
keys the descriptor on that list rather than on the mode, and by `compile_drop`
in both compiled backends so destroying uses the supplied descriptor rather
than the static type. About a hundred lines across eight files.

Four of five cases worked, on all four backends, with a `[string]` so that a
lost drop would leak and a doubled one would crash:

- taking an owned `[T]` and reading it
- returning it
- forwarding it to another generic
- an owned `%{K = V}`

The fifth is where it stops. Moving the parameter into a local:

```datalove
fun grow<T>(xs: [T], v: T): index
  var ys = xs
  push(mut ys, v)
  ret len(ref ys)
end fun
```

lowers to

```
v0 = move p0
store.move.tracked s0, v0
...
drop.tracked s0
```

The descriptor was supplied for `p0`. `s0` is typed `[data]`, and both the
`push` and the drop read that, so the elements are walked as anypacks. It
segfaults, and it segfaults in every backend, because nothing carries the
descriptor across the store.

So the boundary is not owned against borrowed, and not container against
scalar. It is whether the value stays in the parameter it arrived in. That is
the same boundary indexing runs into, and the same one behind the modes and
return types a type parameter cannot reach, which is the argument for doing the
value-level descriptor rather than four point fixes that each stop here.

The prototype is not committed. Shipping the four working cases would mean
shipping the fifth as a crash, and the check that would refuse it -- knowing a
value carries a supplied descriptor -- is the feature itself.

## Carrying the descriptor with the value

A bare `T` has never had any of these problems, and the reason is worth saying
plainly: erased to `data`, it becomes an anypack, and an anypack is a value and
its descriptor together. Move it, drop it, clone it, compare it, print it --
all of that reads the descriptor out of the value. It is already a value-level
descriptor and has been all along.

`[T]` erased to `[data]` is not. The memory holds a `[u32]`, and `[data]`
describes neither the element type nor the stride, so the truth has to travel
beside the value. Today it travels as an argument keyed to a parameter, which
holds for exactly as long as the value stays in the parameter it arrived in.

So the work is not to invent a mechanism. It is to give the second case the
property the first already has.

### The route

**Erase a container whole rather than structurally.** `[T]` becomes `data`,
not `[data]`. The value is then self-describing, and everything that failed
above works without being taught anything: storing in a local, returning,
dropping, and passing on are all things `data` already does.

What this costs is one allocation and one struct copy at each owned crossing.
That is O(1) and not O(n): `data_from_local` allocates `inner_tydesc.size`
bytes and copies the value there, and for a list that value is a pointer and
two indices -- sixteen bytes. The elements are never touched. The O(n) figure
that ruled this out was the cost of the structural erasure, which nobody has to
perform.

What this needs beyond the wrapping is a way to read back through the wrapper,
so a wrapped `[T]` can still be handed to `sys/std/list`.
`dtlv_rti_data_parts` is it: the value pointer and the descriptor together,
borrowed rather than moved, which is the pair the native ABI already speaks.
A call site reads through it where an argument arrives wrapped and the callee
wants it borrowed, passing what comes out as the pointer and as that
parameter's descriptor. Both leave the same call, so neither can be taken from
somewhere the other was not.

### What this leaves

An owned container can be taken, moved into a local, returned, forwarded to
another generic, dropped, read and mutated, on all four backends.

A tuple or a struct is the other half, and it is the case that does need
converting: `(u32, u32)` is eight bytes where `(data, data)` is thirty-two.
`convert` walks the fields, taking the offsets from `compute_tuple_layout` and
the descriptors from `iter_tuple_fields`, which is the Option arm with more
than one payload. Nesting composes, and so does a tuple under an option.

A field that fails after an earlier one succeeded leaves the moved prefix in
the destination and the rest in the source. Only allocation failure does that,
and neither side is destroyed by the walk, so nothing is freed twice; what is
lost is the prefix.

A container inside a tuple is converted the way a container anywhere is: by
wrapping it. The tuple's walk reaches the field, the field's conversion is the
`data` one, and the two compose without either knowing about the other. That
is why erasing an owned value reaches inside a composite rather than stopping
at the top: `([T], u32)` becomes `(data, u32)`, and `(u32, ([T], #{u32}))`
becomes `(u32, (data, #{u32}))`.

A borrowed parameter is still structural, because nothing is converted at one
and the descriptor the call site supplies covers the whole of it.

### What this does not leave: a borrow taken out of a borrowed container

*Parked, not decided. Written down mid-thought so it can be picked up.*

The descriptor a call site supplies for a borrowed parameter "holds for exactly
as long as the value stays in the parameter it arrived in", as above. Indexing
takes it out. `xs[i]?` inside `fun via<T>(ref xs: [T], ...)` produces a
reference, and a reference carries nothing: the interpreter reads its type off
the static `Ref(data)`, and the compiled backends take the stride from the
static element type and land between elements. It segfaults. Lowering keeps an
erased element on the `ListGet` path to avoid forming one at all, which is a
gate rather than an answer. See [Known issues](issues.md).

**The idea is a fat reference, and a non-first-class one.** A reference into an
erased container carries the element's descriptor beside the pointer. The
descriptor is already in reach where the reference is made -- `ListElementRef`
reads it off the list to get the stride -- so nothing has to be computed or
looked up, only kept.

Non-first-class is what keeps it small. A reference here is not a value anyone
stores: `IrType::Ref` is only ever a `fresh_value` produced by a projection and
consumed by the read, clone or argument that follows it. If that stays true by
rule rather than by accident, the fat form has to exist only between those two
points. It never goes in a slot, an aggregate or a return, so the layout, the
serialized IR and the runtime's value model do not have to learn a new
first-class representation -- which is most of what makes the fat-values
migration above a migration.

**Why this is not the option turned down.** "Descriptors as dataflow" keeps the
representation and passes descriptors alongside, which is why it has "two
things that have to stay in agreement". A fat reference describes itself.
It may also retire `descriptor_params`, which exists only because a borrowed
value is not self-describing the way an owned one is; whether that comes with
it or trails it is open.

**To resume on.** Whether a reference really is never stored, or only never
stored today. Whether the fat form is every reference or only one into an
erased container -- the second is cheaper and makes the representation depend
on the pointee, which is the kind of conditional this area keeps being bitten
by. What `mut` and `out` want, since they write through the reference rather
than read through it. And whether the same reasoning covers `GetFieldRef` into
an erased aggregate, which has not been tested and probably fails the same way.

A term is its payload under a name, laid out exactly as the payload is, and an
enum is a payload chosen by a discriminant. Both convert the way an option
does. The discriminant is an index into the variant list and that list is
sorted by name, so both sides have to sort it the same way; building the erased
enum in source order is what this first did, and an `atom None` came out as a
variant that wanted a payload.

With those, erasure reaches every position a type parameter can occupy, and
`TypeParamNotErasable` has nothing left to refuse. The check stays: it is the
decision procedure for whether erasure reaches a shape, and a type form nobody
has taught it about falls through to being refused rather than to the arm in
`convert` that copies one size over another. What it is not any more is
something a program written today can provoke, so there is no fixture for it.

### What was weighed against it

**Fat values.** An erased container as an inline `(List, *TyDesc)` -- twenty
four bytes rather than sixteen, no allocation, the boundary writing the
descriptor beside the value. Faster, and a new representation kind that
`IrType`, the layout, all four backends and the runtime each have to learn.
The right destination if the allocation turns up in a measurement, and the
language surface does not change between the two, so it is a migration rather
than a redesign.

**Descriptors as dataflow in the IR.** Descriptor values propagated alongside,
with `Drop`, `Call`, indexing and every store taking one. No representation
change, and the largest surface of the three: two things that have to stay in
agreement, spread over every instruction and four backends. Every generics bug
found so far has been that shape, and the failure mode is silent corruption
rather than a refusal.

**Specialization instead.** `specialize.rs` already turns a `const` parameter
into one function with a branch per instantiation, dispatching on a
discriminant. Pointed at a type parameter it would remove the problem rather
than solve it, since inside a branch `T` is `u32`. It keeps one function per
source function, so compile time and incrementality survive, and it costs code
in proportion to how many instantiations there are. Worth a real comparison
before the second half of this is built, rather than assuming erasure is the
design.

### Indexing, which was never blocked by this

The refusal was a typecheck test on the element type, raised whether or not a
descriptor was in reach. It is in reach: `ListGet` in the interpreter already
read the element type off the list's descriptor, and the compiled backends
took the stride from the static type instead, which inside a generic is a
`data`'s and lands between elements.

So indexing now reads the stride from the descriptor everywhere. Whether the
element wants packing on the way out is the runtime's to decide, in
`dtlv_rti_list_get_erased_local`: a generic asking for an element it cannot
name wants one packed, a list whose elements really are `data` wants one
copied, and the two look alike from a call site. Deciding it in each backend
would be four answers to one question, which is the shape of most of the bugs
this area has had.

> **A borrowed value's erased type is a lie about its layout.** The table above
> says every position works, and it was checked by compiling each. What none of
> those checks covered is reading a *part* of a borrowed value whose erased
> layout differs from its real one: `ref p: {a: T, b: u32}` was compiled against
> `{a: data, b: u32}` while the caller holds `{a: u8, b: u32}`, so `p.b` was at
> the wrong offset -- and under `mut` the same offset was *written*.
>
> **Fixed for fields.** A reference now carries the descriptor of what it points
> at, and offsets come from that; see
> [Fat references for borrowed generic values](plan-fat-refs.md) and
> `backend/15_borrowed_generic_aggregate.dfs`. Element references still do not,
> which is the entry left in [Known issues](issues.md).
>
> Owned was sound throughout, and was checked the same way: ten shapes -- a bare
> `T`, a `[T]` field, `?T`, `!T`, nested aggregates, tuples, two parameters,
> three levels, return position, and forwarding through a second generic --
> agree across all four backends. The boundary converts an owned value field by
> field, so what broke was the borrow rather than the aggregate.

> **Both halves now.** The note here used to say that only the reading half was
> done, and that `ListElementRef`, `MapValueRef` and `TensorIndexRef` still took
> their stride from `operand_type(list)`, so a borrowed index inside a generic
> landed between elements and segfaulted. They take it from the container's
> descriptor now, and the element's descriptor travels with the reference; see
> [Fat references for borrowed generic values](plan-fat-refs.md).
>
> The reading half was also less done than it said: a map lookup inside a
> generic described the map by its static type, a `%{data = data}`, and returned
> `none` for a key that was there -- quietly, and the same way in all four
> backends.

## Where this stands

Type parameters nest to any depth inside options, results, tuples, structs,
terms and enums, and a collection of one can be passed and returned in any
mode. What follows is the state of it, then the limitations, then how each was
arrived at.

### What works

Each of these was checked by compiling it, not by reading the rules.

| Position | Bare `T` | `?T` `!T` `??T` | Tuple, struct, term, enum | Collection: `[T]`, `%{K = V}`, `#{T}` |
|---|---|---|---|---|
| `in`, owned | yes | yes | yes | yes |
| `ref`, `mut` | yes | yes | yes | yes |
| `out` | yes | yes | yes | yes |
| return | yes | yes | yes | yes |
| any parameter of a `native fun` | yes | yes | yes | yes |

Every position a type parameter can occupy is one erasure reaches. The three
cases it splits into are in the section below: a bare `T` becomes a `data`, a
composite is converted field by field, and a collection is wrapped whole --
that last one when it is owned. A borrowed collection is not converted at all,
because nothing is converted at a borrow; it keeps the shape it was written
with and the call site hands over a descriptor for what is really behind it.
`erased_param_type` is the fork: `borrowed` takes
`from_type_hint_erasing`, which leaves `[T]` as `[data]`, and owned takes
`erased_owned_type`, which returns `Data` for a container of a type parameter.
`descriptor_params` is populated under the matching condition, `borrowed &&
type_hint_mentions_param`.

A call site names no types: each argument is matched against its parameter and
the first to reach a parameter fixes it. `sys/std`'s `option`, `result`,
`list`, `map` and `set` are written this way.

### Limitations

**A generic that builds a collection and recurses at a larger type is
refused.** That is the only thing left here, and it is narrow.

A collection over a type parameter can be built: `var out: [T] = []` works, and
so do `#{T}` and `%{K = V}`. One that arrives carries a descriptor saying what
its elements are; one written in the body has no value to read one off, so the
call site hands one over as a trailing argument. Nothing is worked out at run
time, because `[T]` with `T` bound to a concrete type is a concrete type and
that has a static descriptor already. `sys/std/list` gained `reversed` and
`concat` on the strength of it.

Which descriptors a function takes is what its own body builds, closed over its
calls: a generic passing its type parameter to one that builds a collection of
it carries a descriptor too, and forwards it. That closure is a query on a
callee's signature, not an analysis over the program -- the same thing a caller
already does for parameter types, except that this part of a signature is
derived from a body, so it is answered before anything reads one.

What is refused is a cycle whose substitution *grows* a shape: a generic
building a `[T]` and calling a generic at a strictly larger type needs `[T]`,
then `[[T]]`, without end. Whether that happens is decided from the call graph
rather than from how big a shape gets. Each call draws an edge from the callee's
type parameter to the caller's, strict when the binding puts a type constructor
around it; a cycle of plain edges is a renaming and cannot enlarge anything,
while a cycle holding one strict edge adds a level every lap. That is the occurs
check read across the call graph, and it is what keeps a merely deep type from
being mistaken for a growing one. Erasure means such a function compiles to one body
and would merely recurse for ever at run time, the way any missing base case
does; what cannot be written down is the descriptors, not the code. It was
unreachable before collections could be built, because a `[[T]]` could not be
obtained without building one.

**A generic out parameter has to be a whole binding or a field.** Both work,
and so does passing one straight on to another function; anything else is
refused rather than written wrong.

Whatever the destination held is destroyed by the caller, before the call.
The callee cannot do it: its tracking byte for an out parameter starts
uninitialized, so its first store destroys nothing. A destination that is
itself an out parameter being forwarded has already been cleared once, and
its tracking byte is what says not to do it again.

**The C backend calls riders and takes descriptors like the others.** It was
written before generics existed and had neither, which is why nothing in its
386 fixtures reaches the standard library. It now compiles all of `sys/std`,
so a program that calls a rider, does bigint arithmetic or passes a borrowed
collection of a type parameter builds and runs the same under it as under the
interpreter and the two cranelift backends.

**No bounds, so no operations on a `T`.** A type parameter is equal only to
itself and nothing can be done with a value of that type but move it, drop it,
clone it, print it, and hand it back. Printing is the exception because the
descriptor travelling with the value is enough to format it; `==` is not, so
`contains`, `index_of` and `max` stay unwritable. That rules out `map`, `and_then`, `filter` and
anything else needing a function argument, though those want closures first.

**A type parameter is always linear**, because the caller may supply a linear
type. A value taken out of an option by `if x |v|` has moved out of `x`, so a
function returning the option it destructured has to rebuild it with `some v`.

**Which type a parameter is fixed at depends on argument order.** An unsuffixed
integer literal is `int`, so `pick_first(99, : u32 / 1)` makes `T` `int` and
widens the `u32` into it, while the two the other way round make `T` `u32`.
Both check; they just do not name the same `T`.

**Nothing is monomorphized.** One compiled function serves every
instantiation, and every owned crossing costs a wrap and an unwrap. That is the
intended trade for compilation speed; specializing hot instantiations is step 5
below and may never be needed.

### How it works

**The type variable went into datalit, and it was cheap.**

The section
above treats that as the expensive branch, on an estimate of eighty match arms.
Measured instead of estimated, adding `Var(InternedText)` to
`datalit::tycheck::Type` costs seven sites in the whole workspace: printing it
in two places, `types_equivalent` (a variable is equal only to itself),
`is_copy_type` (an unconstrained parameter is move, since the caller may supply
a linear type), two `unreachable!`s where a parameter cannot occur, and one arm
in `IrType::from_datalit` that maps it to `data`. The eighty-arm figure was for
`IrType`, which every backend matches exhaustively; `datalit::Type` is mostly
consumed through predicates and constructors that already have fallbacks.

That last arm is the whole of erasure. `from_datalit` recurses structurally, so
one line erases a parameter at any depth: `?T` lowers as `?data`, `??T` as
`??data`. The twenty-four "cannot nest a type parameter" arms in the type hint
converter were deleted rather than written.

**This does not add generics to the surface language.** A bare `T` already
parsed as `TypeHint::Alias("T")`, and datalit's own resolver rejects every alias
(`Type aliases are resolved by datafun typechecker, not datalit`). The single
place a `Var` is built is datafun-resolve seeding the alias map with the
enclosing signature's type parameters, in a crate datalit does not depend on. A
data literal mentioning `T` fails exactly as it did before.

**Call sites unify resolved types, not the callee's AST.** `bind_type_params`
walks the parameter's type and the argument's in step, binding at each `Var`;
the first argument reaching a parameter fixes it and later ones must agree.
An argument that cannot synthesize on its own, such as a bare `none`, binds
nothing and is checked afterwards against whatever another argument fixed, so
`unwrap_or(none, "fallback")` infers. Speculative synthesis runs under
`try_check` so the abandoned attempt does not report.

**Why collections looked impossible, and what they cost instead.** The
runtime's `convert` recursed properly through options and results and memcpyd
everything else, which is a reinterpretation rather than a conversion. A
collection's elements are packed by size, so `[u32]` and `[data]` disagree
about where every element after the first begins: passing one through appeared
to work because nothing touched the elements, but the callee held a tydesc that
lied about them, and the drop at the end of a generic that merely ignored its
argument corrupted the allocator. That was checked, not reasoned about, and
`first_unerasable_type_param` refused those signatures with
`TypeParamNotErasable`.

Rebuilding element by element is what that reasoning assumed, and it is what
made the cost look prohibitive. Wrapping the collection whole avoids it: the
`data` holds a pointer and the collection's own descriptor, so a crossing
copies one wrapper and leaves every element where it is. It is O(1) and one
allocation, not O(n), and the elements are never touched, so their packing
never has to agree with anything. `first_unerasable_type_param` now returns
`None` for every type form the language has; it is kept as a backstop for
future ones rather than because anything reaches it.

**A borrowed collection escapes that, by not being converted.** Nothing happens
to a `ref` or `mut` parameter at the boundary: the value goes across as it
stands and the callee never drops it. What it lacks is a descriptor, since its
own type says `data` where the parameter was written, and the call site has
that. `FunctionContext::descriptor_params` records which parameters need one,
and how it is carried is left to each backend: the interpreter needs nothing,
because a value there is a pointer and a descriptor already, while the compiled
backends take one extra pointer after the ordinary parameters.

That distinction is about ownership rather than shape. A parameter the callee
owns needs a slot, and `data` is the one shape that fits any value. A parameter
it borrows needs a pointer, which fits anything already.

**The all-backends suite is what makes this safe to have.** Descriptor passing
worked in the interpreter for a while before it worked anywhere else, because
interpreter values carry their descriptors and the compiled backends pick one
from the static type when they emit the call. Left enabled, that would have
been a feature that worked in one backend and corrupted memory in the other
two. It was caught by `std_all_tests`, with a segfault rather than a wrong
answer, and refused until the backends agreed. Generic `len` passed all three
even then, because `list_len` happens not to look at the element type, which is
exactly the kind of accident that makes a wrong descriptor look like a working
feature.

## Order of work

Steps 1 to 4 are done. The type variable went into datalit rather than `IrType`,
which is why step 2 reads as it does.

1. ~~**Fix CTFE.**~~ Done, along with module-level consts and const bindings no
   longer moving when read.
2. ~~**Add a type variable**~~ and teach lowering to emit descriptor parameters
   alongside erased values. Done, in `datalit::tycheck::Type` and
   `FunctionContext::descriptor_params`.
3. ~~**Erase and reify at call boundaries**, reusing the rider convention.~~ Done.
4. ~~**Write the stdlib that motivated this**~~: `option`, `result`, `list`,
   `map` and `set` are all generic.
5. **Only then**, if measurement says so, specialize hot instantiations in the
   JIT tier. Nothing is monomorphized today.

What is left, in the order it is worth doing:

- **Owned collections**, by erasing a container whole rather than structurally
  so that the value carries its own descriptor. The route and what was weighed
  against it are in
  [Carrying the descriptor with the value](#user-content-carrying-the-descriptor-with-the-value).
  This is the one being built.
- **Descriptor-driven indexing**, which is the other place the language refuses
  something a reader expects to work, and which turns out not to depend on the
  above: when the container is a parameter the descriptor is already in reach,
  and `xs[i]` wants lowering to the runtime call `list.get` already makes.
- **Owned tuples and structs**, which unlike collections do need converting,
  but by a walk whose length the type fixes rather than the data.
- **Bounds of some kind**, without which nothing can be done to a `T` but move
  it. Closures are the bigger prerequisite for the functions people ask for.

## Prior art

The survey in [Const Parameter Specialization](const-param-specialization.md) covers
Harper and Morrisett's intensional type analysis, dictionary passing, Go's GCShape
stenciling, Swift's witness tables, defunctionalization and partial evaluation. Two
systems it omits are the closest fits to this problem.

**The .NET CLR** splits by representation rather than by type: reference types share one
JIT-compiled instantiation through the `__Canon` placeholder, value types are
monomorphized. The cost surfaces as "runtime handle lookup" when shared code needs the
concrete type back. The split maps closely onto datalove's copy and linear types.

**Julia** specializes lazily, at the first call with a new type signature, and caches the
result. Its documented failure mode is the relevant warning: over-parameterized types
cause a compile-time blowup that can exceed the runtime benefit, and 1.6 deliberately made
the compiler less eager to specialize. Julia also warns specifically against enthusiasm
for values as type parameters, which is what const parameter specialization is. Its
world-splitting, branching over up to four candidate types instead of dispatching
dynamically, is the technique union-branch resembles, and it applies where the type is
*not* statically known.

References:

- Kennedy, A. & Syme, D. (2001). [Design and Implementation of Generics for the .NET Common Language Runtime](https://web.eecs.umich.edu/~bchandra/courses/papers/Kennedy_Generics.pdf). PLDI.
- Nikitin, A. [.NET Generics under the hood](https://alexandrnikitin.github.io/blog/dotnet-generics-under-the-hood/).
- Zhang, Y. [Sharing .NET generic code under the hood](https://yizhang82.dev/dotnet-generics-sharing).
- Bezanson, J. et al. (2018). [Fast Flexible Function Dispatch in Julia](https://arxiv.org/pdf/1808.03370).
- Holy, T. (2020). [Analyzing sources of compiler latency in Julia: method invalidations](https://julialang.org/blog/2020/08/invalidations/).
- [Julia's latency: Past, present and future](https://viralinstruction.com/posts/latency/).
- Rao, V. [Zig-style generics are not well-suited for most languages](https://typesanitizer.com/blog/zig-generics.html).
