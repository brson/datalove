# Fat references for borrowed generic values

A reference is a pointer and a descriptor. Today only the pointer survives, so
anything computing an address from a reference computes it from the referent's
static type instead -- and inside a generic that type is a lie. This is the
plan to carry the descriptor and take the layout from it.

It fixes two entries in [Known issues](issues.md): the borrowed aggregate using
the wrong field offsets, and the reference into an erased container carrying
the wrong type.

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

Held as a side table on `IrCodeUnit`, absent meaning `Static`:

```rust
pub ref_descs: BTreeMap<ValueId, RefDesc>,
```

### Where it is computed

One pass, `resolve_ref_descriptors`, beside `resolve_call_descriptors` in
`datafun-ir`, run after specialization and inlining have finished rewriting
instructions. The reason is the one already written above
`shape_descriptors_for`: *"One implementation, read by every backend, because a
call site and a callee signature disagreeing about the trailing arguments is
the failure this area keeps producing."* A side table keyed on `ValueId` that
lowering filled in would have to be maintained through inlining's renumbering;
a pass run afterwards cannot drift.

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

The owned path. An owned value is converted at the boundary field by field, so
the layout the callee was compiled for is the layout it gets, and its static
type tells the truth. This was checked by compiling and running ten shapes
across all four backends; see the scope table in [Known issues](issues.md). The
return path is the owned path in the other direction and needs nothing either.

## Staging

1. Add `RefDesc`, the side table, and `resolve_ref_descriptors` producing
   `Static` for everything. No behavior change; the test suite should be
   untouched, which is the proof that the plumbing is inert.
2. Seed roots from `descriptor_params`, propagate through `GetFieldRef`,
   consume in the offset. This closes the borrowed-aggregate entry, `ref` and
   `mut` both.
3. Consume in read-through, `RefStore`, `RefStoreTracked` and `DropViaRef`.
4. Propagate and consume through `ListElementRef`, `MapValueRef` and
   `TensorIndexRef`. This closes the erased-container entry and is what would
   let the index-projection gate be reconsidered -- separately, since that is a
   language decision and not this one.

## Open questions

- **The interpreter needs nothing.** Its values are a pointer and a descriptor
  already, which is why it is right in every case here. Whether it should read
  the same table anyway, so that a disagreement is caught rather than papered
  over, is worth deciding before step 2 rather than after.
- **Enum and term payloads under a borrow** were not probed. `TyInfoEnumVariant`
  carries an `offset` and a `payload` descriptor, so the same rule should
  apply, but whether payload projection goes through `GetFieldRef` or its own
  path has not been checked.
- **Tables** are in `datalit_is_container_of_var` but were not probed either.
- Whether `RefDesc::Field` should hold an `Operand` or a `ValueId` depends on
  whether a fat reference can root at anything other than a parameter. It
  cannot today, but `Operand` costs nothing and does not have to be revisited.
