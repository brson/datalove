# Fat references for borrowed generic values

A reference is a pointer and a descriptor. Only the pointer used to survive, so
anything computing an address from a reference computed it from the referent's
static type instead -- and inside a generic that type is a lie. This is the plan
to carry the descriptor and take the layout from it.

**Done**, for every way of reaching into a borrowed value: field projections,
list, map and tensor indexing, and parts whose erased type is a composite
holding `data`. See "What was built" below.

## What is already in place

Most of this exists. The gap is narrower than the two bugs make it look.

**The descriptor already arrives.** `descriptor_params`
(`datafun-lower/src/func.rs:168`) is built for exactly the parameters whose
static type does not describe what arrives -- borrowed, and mentioning a type
parameter:

```rust
if borrowed
    && datalove_datafun_ir::type_hint_mentions_param(&p.type_hint, &type_params)
{
    descriptor_params.push(id);
}
```

Both compiled backends already carry it. C emits a trailing
`const dtlv_tydesc_t* d{i}` per entry (`c-aot/src/lib.rs:477`); cranelift
pushes a trailing pointer param and binds it into `descriptor_values`
(`cranelift/src/codegen/mod.rs:129`, `:508`). The call site already fills it
with the argument's *concrete* descriptor (`c-aot/src/codegen.rs:2123`), and
`operand_tydesc` already prefers the handed-over one over the static one
(`c-aot/src/codegen.rs:2344`).

**The descriptor already holds the offsets.** A tydesc is not a size and an
alignment to be recombined -- field offsets are precomputed in it
(`datalove-rtdt/src/lib.rs:1020`):

```rust
pub struct TyInfoTupleField  { pub offset: u32, pub tydesc: *const TyDesc }
pub struct TyInfoStructField { pub name: *const u8, pub name_len: u32,
                               pub offset: u32, pub tydesc: *const TyDesc }
```

So "dynamic offset and alignment calculation" is a **load**, not arithmetic.
Alignment was settled when the descriptor was built. Each field also carries
its own descriptor, which is what makes a chain of projections work.

**The vocabulary already exists.** `DescriptorRef::{Static, Own}`
(`datafun-ir/src/lib.rs:2413`) is already the distinction between a descriptor
known at compile time and one handed over at run time.

## What is missing

One thing: `GetFieldRef` produces a bare pointer.

```rust
let offsets = types::compute_tuple_field_offsets(&field_types);
let field_offset = offsets[field_index as usize];
writeln!(out, "    *(void**){} = {} + {};", dest_addr, src_addr, field_offset);
```
(`c-aot/src/codegen.rs:1436`; `cranelift/src/codegen/aggregates.rs:313` is the
same with `iadd_imm_s`.)

`field_types` comes from the static type. Nothing associates a descriptor with
the destination, so the next step of a chain, and every other consumer of the
reference, has nothing to consult and falls back to the same lie.

Note that the static type cannot be used to detect this. A `data` written by
hand and a `data` standing in for an erased `T` are the same `IrType`, and the
static layout is right for the first and wrong for the second. Fatness has to
be recorded where it is still known, which is lowering.

## The design

Every `Ref`-typed value has a descriptor, resolved at compile time to one of
two things:

- **`Static`** -- the referent's static type describes it. Nothing is carried,
  the offset folds to the same constant emitted today, and codegen is
  byte-for-byte unchanged. This is every reference outside a generic.
- **dynamic** -- materialized as a second machine value beside the pointer.
  This is the fat reference, and it exists only where the static type lies.

So "all references are fat" is true as a rule and costs nothing in the common
case, because the fat half is usually a compile-time constant that folds away.
That is the same trick `DescriptorRef::Static` already plays for shapes.

### Representation

```rust
/// Where a reference's descriptor comes from.
///
/// A reference is a pointer and a descriptor. The descriptor is usually the
/// one the referent's static type gives, and then it is a constant and nothing
/// is carried. It is otherwise only inside a generic, where the static type
/// says `data` at a type parameter and lies about the layout.
pub enum RefDesc {
    /// The referent's static type describes it.
    Static,
    /// The descriptor this function was handed for a borrowed parameter, by
    /// index into `descriptor_params`.
    Param(u32),
    /// Field `n` of another reference's descriptor.
    Field(Operand, u32),
    /// The element descriptor of another reference's container descriptor.
    Element(Operand),
}
```

Absence means `Static`, so the enum as built has no such variant and no
`Operand` in it -- see "What was built" for the shape it actually took.

### Where it is computed

One pass, `resolve_ref_descriptors`, beside `resolve_call_descriptors` in
`datafun-ir`, derived per unit by the backend about to compile it rather than
stored anywhere. The reason is the one already written above
`shape_descriptors_for`: *"One implementation, read by every backend, because a
call site and a callee signature disagreeing about the trailing arguments is
the failure this area keeps producing."* A side table keyed on `ValueId` that
lowering filled in would have to be maintained through inlining's renumbering;
deriving it on demand cannot drift at all.

Roots are `descriptor_params`, unchanged -- it is already exactly the right
set. Propagation is one rule per reference-producing instruction:

| instruction | `dest`'s descriptor |
|---|---|
| `GetFieldRef { dest, src, field_index }` | `Static` if `src` is, else `Field(src, field_index)` |
| `ListElementRef { dest, list, .. }` | `Static` if `list` is, else `Element(list)` |
| `MapValueRef`, `TensorIndexRef` | as `ListElementRef` |

### Where it is consumed

Each of these takes its layout from the descriptor when the reference is fat,
and is left exactly as it is when it is `Static`:

| consumer | today | fat |
|---|---|---|
| `GetFieldRef` offset | `compute_tuple_field_offsets` | field offset from the descriptor |
| `Operand::ValueRef` read-through | size from static type | `desc->size` |
| `RefStore`, `RefStoreTracked` | size from static type | `desc->size` |
| `DropViaRef` | `tydesc_name(&inner_ty)` (`c-aot/src/codegen.rs:2738`) | the carried descriptor |
| `ListElementRef` stride | `operand_type(list)` | element descriptor's size |

`DropViaRef` is worth calling out: it builds a static descriptor from the
erased inner type, so dropping through a reference into a generic aggregate is
wrong today for the same reason the offsets are. The design fixes it without a
separate change.

### Runtime helpers rather than four walkers

Reading a field's offset out of a descriptor should be a runtime call, not
open-coded in each backend:

```c
uint32_t              dtlv_rti_field_offset(const dtlv_tydesc_t*, uint32_t index);
const dtlv_tydesc_t*  dtlv_rti_field_tydesc(const dtlv_tydesc_t*, uint32_t index);
```

Two reasons. The first is the one `plan-generics.md` already gives for putting
the erased-element decision in the runtime: deciding it in each backend is four
answers to one question, which is the shape of most of the bugs this area has
had. The second is concrete -- `TyInfoTuple` and `TyInfoStruct` do not agree on
field order:

```rust
pub struct TyInfoTuple  { pub num_fields: u32, pub fields: *const TyInfoTupleField }
pub struct TyInfoStruct { pub fields: *const TyInfoStructField, pub num_fields: u32 }
```

Pointer and count are swapped between them. Four backends open-coding that walk
would get it wrong at least once.

The cost is a call per projection, and only in generic code. If it matters
later it can be inlined; it should not be inlined first.

## Why the calling convention does not change

References are never first class and never returned, so a fat reference never
crosses a function boundary *as* a fat thing. It crosses as a pointer and a
trailing descriptor -- which is precisely what `descriptor_params` already
passes. Nothing is added to any signature.

What changes is only what fills the descriptor slot. `operand_tydesc` today
special-cases `Operand::Param`; it gains an arm for a fat `Operand::ValueRef`,
so that `second(ref p.q)` inside a generic hands over the descriptor carried by
`p.q` rather than a static one built from its erased type.

References also never land in slots -- lowering only ever puts them in values
(`datafun-lower/src/stmt.rs:841`, `:855`, `:868`, `:909`,
`datafun-lower/src/expr.rs:345`, `:1756`, `:1870`). A slot holds a *referent*.
So the fat half lives in SSA values only, which is where a second machine value
is cheapest.

## What is not touched

**Owned aggregates.** An owned aggregate is converted at the boundary field by
field, so the layout the callee was compiled for is the layout it gets, and its
static type tells the truth. This was checked by compiling and running ten
shapes across all four backends; see the scope table in
[Known issues](issues.md). The return path is the owned path in the other
direction and needs nothing either.

**Owned containers, which are a separate fault.** An owned container is *not*
converted field by field -- it is wrapped whole into a `data`. So a generic
holding `xs: [T]` by value holds a `data`, and lowering then emits `ListGet`
against it without unwrapping:

```
fn first(p0):            // p0: Data
    v0 = const 0index
    v1, v2 = listget p0[v0]
```

Both AOT backends refuse this outright (`ListGet on non-list type: Data`), the
jit turns the same error into a panic, and the interpreter reads the anypack's
bytes as a list header and segfaults. No reference is involved and no
descriptor is missing -- the value is self-describing, it is simply never
opened. Fat references do not reach it. It has its own entry in
[Known issues](issues.md).

## What was built

Steps 1 to 3 are done: field projections of borrowed generic aggregates are
right in all four backends, and `backend/15_borrowed_generic_aggregate.dfs`
records what the four agree on across ten shapes.

What landed, and where it differs from the sketch above:

- **`RefDesc` and `resolve_ref_descriptors`** (`datafun-ir`). Derived per unit
  rather than stored on `IrCodeUnit`, which is the one change of substance from
  the plan: a stored table would have to survive specialization renumbering
  values and inlining offsetting them, and each backend calling the pure
  function once for the unit it is compiling cannot go stale. `ValueId` and
  `ParamId` gained `Ord` for the `BTreeMap`.
- **`dtlv_rti_field_offset` / `dtlv_rti_field_tydesc`**, one walk of the
  descriptor rather than four, as planned.
- **`dtlv_rti_field_read_local`**, which was *not* in the plan and turned out to
  be necessary. A field of the erased type is not only at a different offset,
  it is a different shape: `p.a` where `a: T` is really a `u16` and the
  destination is the `data` the signature says. Packing it belongs in the
  runtime for the same reason the erased list get does -- a destination that
  really is a `data` wants a clone and one standing for a `T` wants a pack, and
  the two look alike from a call site.
- **Intermediate projections stopped copying.** `lower_place_value` used to walk
  a chain like `p.q.b` by copying `p.q` out and reading `b` from the copy.
  Inside a generic that copy garbles the value before the next step runs, since
  the destination's layout is the erased one. Intermediates are now borrows.
  This is cheaper outside a generic too -- one fewer aggregate copy per step --
  and is what changed four `dual` fixtures' IR dumps, with their outputs
  unchanged.
- **`Frame::value_tydescs`** in the interpreter: the per-frame override
  `issues.md` said was missing. The interpreter needed it after all. It was
  right on the single-step cases only because a parameter's descriptor is
  already per-frame; a chain, and a read of the erased field itself, were wrong
  there too.
- **Cranelift:** `ref_desc_values`, a scalar destination taking a dynamic offset
  and a load rather than a runtime call, everything else through `field_read`.
  **C:** `__rd{n}` declared in the prologue so a `goto` never jumps a
  declaration. **JIT:** the three new symbols registered, without which every
  generic program aborted.

Three cases were refused or wrong at first and are now handled: reading a part
whose erased type is a composite holding `data`, writing one, and handing such a
borrow to a callee and taking back what it returns. See "The composite
conversion" below.

Element references landed after the fields, and needed one thing the fields did
not: `arg_needs_unwrapping` in Cranelift and its two counterparts in the C
backend decided whether an argument typed `data` is a wrapper to unwrap or a
pointer at the value, and they asked whether the operand was a borrowed
parameter. Anything carrying its own descriptor is a pointer at the value, so
that question became `operand_ref_desc(arg).is_some()`. Without it the callee
took the first two words of a string for a wrapper's pointers.

Maps and tensors followed, and the map needed one more thing again. A lookup has
a **key** as well as a container, and inside a generic the key parameter is
erased too, so the key in hand is packed into a `data` while the map holds the
real thing. Both halves were wrong and in opposite directions: the compiled
backends described the map by its static type, a `%{data = data}`, while the
interpreter described the *key* by the map's real key type while holding a
packed one. Either way the comparison read the wrong bytes and the lookup
returned `none` for a key that was there -- silently, and identically in all
four backends, so running them against each other did not catch it.
`dtlv_rti_btreemap_{contains_key,get_value_ref}_erased_local` decide whether the
key is packed, by the reading `unwraps_the_element` already makes on the insert
side. A tensor was the list's problem again: a stride from the static element
type. `compute_tensor_element_addr` now takes a width rather than a constant.

## The composite conversion

Reading a part of a borrowed generic value has three cases. A destination whose
static type is the truth takes a copy at an offset from the descriptor; one that
is exactly `data` takes a packed clone. A destination like `{a: data, b: u32}`
is neither: what is really there is `{a: u8, b: u32}`, so it has to be converted
position by position.

**Nothing new was needed.** `erase_local` and `reify_local` already walk two
descriptors and convert wherever one of them says `data` -- that is how an owned
parameter crosses a call -- and they take both descriptors as *pointers*, so one
found at run time does as well as a static one. What was missing was calling
them. Three places assumed instead:

- The **read** cloned in the source's shape into a destination laid out
  differently. A value read out of something borrowed has to be left where it
  is, and `erase_local` moves, so `clone_into_erased_shape` clones in the real
  shape first and moves the clone. That is the one allocation this adds, on the
  one path that needs it.
- The **write** copied one width over the other. It reifies now, which for two
  descriptors that agree everywhere is the move it always was.
- **`data_into_local`** moved a payload out at the *destination's* width rather
  than the payload's, reading past the end of the allocation. That is the
  silent-garbage route: a generic returning `?{a: T, b: u32}` gets a `data` back
  from one whose own parameter was a bare `T`, so the box holds a concrete
  `{a: u8, b: u32}` while the destination is laid out as `{a: data, b: u32}`.

**Comparing widths does not decide whether anything was erased**, which cost a
round of debugging. A `data` is two words and so is a `string`, so
`{a: string, b: u32}` and `{a: data, b: u32}` are the same tag *and* the same
size while being different layouts. Taking that for "nothing was erased" writes
a string where a wrapper belongs, and what reads it back finds a descriptor
pointer that is the string's first eight bytes -- `Result/32609`, in the run
that caught it. `needs_erasure` walks the two descriptors the way `convert`
does, looking only for a position where one says `data` and the other does not.

## What is left

Nothing in this family. The remaining [Known issue](issues.md) nearby is the
owned container that is never unwrapped, which is a lowering gap rather than a
descriptor one: no reference is involved and the value describes itself.

## Open questions

- **The interpreter needs the table too, for step 4.** It is right on the
  aggregate cases, because a parameter's descriptor is per-frame
  (`Frame::param_tydescs`). It is *not* right on a borrowed index: there is
  nowhere to put a descriptor for an SSA value, so `Frame::value_deref` reads
  `self.layout.value_tydescs[idx]`, which is computed from static types
  (`datafun-interp/src/frame.rs:174`). The borrowed-index repro segfaults in
  the interpreter as well as the compiled backends. So step 4 needs a per-frame
  value descriptor override beside `param_tydescs`, and the claim that the
  interpreter is right in every case here is wrong.
- **Enum and term payloads under a borrow** were not probed. `TyInfoEnumVariant`
  carries an `offset` and a `payload` descriptor, so the same rule should
  apply, but whether payload projection goes through `GetFieldRef` or its own
  path has not been checked.
- **Tables** are in `datalit_is_container_of_var` but were not probed either.
- Whether `RefDesc::Field` should hold an `Operand` or a `ValueId` depends on
  whether a fat reference can root at anything other than a parameter. It
  cannot today, but `Operand` costs nothing and does not have to be revisited.
