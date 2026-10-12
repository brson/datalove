# Immutable Cursors for the Collections

> **Written under erasure.** Generics have since moved to fat pointers and
> descriptor tables ([Generics: how it works](generics.md)). What this says
> about erased values, `data` wrappers, `erase`/`reify` or descriptor shapes
> describes the old implementation.

A design for the runtime half of `for` loops: one cursor shape that walks a
list, a set, a map or a tensor in order, borrowing what it visits. Written
October 2026 alongside the `for` draft in `mandocs/drafts/`; not implemented.

## Contents

- [What it is for](#user-content-what-it-is-for)
- [The cursor](#user-content-the-cursor)
- [Operations](#user-content-operations)
- [Per collection](#user-content-per-collection)
- [Erased elements](#user-content-erased-elements)
- [In the IR](#user-content-in-the-ir)
- [In the backends](#user-content-in-the-backends)
- [What keeps it sound](#user-content-what-keeps-it-sound)
- [What exists and what is missing](#user-content-what-exists-and-what-is-missing)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## What it is for

`sys/std` walks collections with an `index` counter, wrapping adds, and a
bounds-checked read per element. For a list that costs a check; for a set or a
map it costs a descent from the root per element, since `btreeset_get_at` and
`btreemap_key_at` are positional and the tree has no other way in. Both clone
what they return.

A `for` loop wants something that visits each element once, in order, and
hands out a borrow of it. That is a cursor. It is immutable: it reads the
collection and never changes it, and the collection does not change while it
is in use (see [What keeps it sound](#user-content-what-keeps-it-sound)).

## The cursor

One struct for every collection, in `datalove-rtdt`:

```rust
#[repr(C)]
pub struct Cursor {
    pub elem: *const u8,   // The current element; a map's current key.
    pub value: *const u8,  // A map's current value; null for the rest.
    pub node: *const u8,   // A tree's current leaf; null for the rest.
    pub step: usize,       // How to get to the next one; see below.
    pub pos: Index,        // How many elements have been passed.
    pub end: Index,        // How many there are.
}
```

It is plain data. It owns nothing, so it is a copy type: nothing drops it,
the drop schedule and the tracking bytes never see it, and leak checking has
nothing to say about it. It is the same size for every collection, so a
backend can give it a slot without knowing which collection fills it.

Two fields mean the same thing everywhere, and that is what the shape buys:

- **`pos == end` is done.** Every backend tests it inline, for every
  collection, with no call. `end` is the length when the walk began.
- **`elem` is the current element.** For a list, a set and a tensor of any
  rank, the loop binding is `elem` and nothing else is consulted. A map
  adds `value`.

`pos` is also the index, so a `for i, item in xs` form costs nothing: `i` is
`pos` for a list or tensor, and the position in sort order for a set or map.

## Operations

Two runtime entry points per collection, one signature for all four:

```
dtlv_rti_<kind>_cursor_begin_local(rt, coll_ref, coll_tydesc, cursor_out, scratch) -> RtStatus
dtlv_rti_<kind>_cursor_advance_local(rt, cursor, coll_tydesc) -> RtStatus
```

`<kind>` is `list`, `btreeset`, `btreemap` or `tensor`, following the names
already there. The collection kind is always known statically -- an erased
`[T]` is still a list -- so there is no dispatch on the descriptor's tag, and
the tree advances, which run once per element, stay a direct call.

- **begin** fills the cursor at the first element, or with `pos == end == 0`
  for an empty collection, whose `elem` is then never read.
- **advance** moves to the next element. Calling it when `pos == end` is a
  bug in the caller; the compiler always emits the done test first.
- **done** and **current** are field reads, not calls.

`scratch` is storage the caller lends for the life of the walk, as
`data_borrow`'s is: `size_of::<Tensor>()` bytes at its alignment. Only a
tensor of rank above 1 uses it, and everyone else may pass null. Both entry
points return a status because every `dtlv_rti_*` does; neither can fail on a
well-formed collection.

## Per collection

| | `elem` | `value` | `node` | `step` | `end` |
|---|---|---|---|---|---|
| list | element pointer | null | null | element size, bytes | `size` |
| tensor, rank 1 | element pointer | null | null | `strides[0]` x element size, bytes | `shape[0]` |
| tensor, rank > 1 | `scratch`, holding a view | null | null | `strides[0]`, elements | `shape[0]` |
| set | key pointer in the leaf | null | leaf | slot within the leaf | `len` |
| map | key pointer in the leaf | value pointer in the leaf | leaf | slot within the leaf | `len` |

**List.** Begin is `elem = data`, `step = size`. Advance is
`elem += step; pos += 1`.

**Tensor, rank 1.** Begin is
`elem = ptr_base + offset_elems * size`, `step = strides[0] * size`. Advance
is the list's. A stride is stored unsigned and may stand for a negative one,
so the add wraps, as the runtime's own offset arithmetic does.

**Tensor, rank above 1.** The binding is a view of one rank less, the same
view `TensorIndexRef` builds today: capacity zero, aliasing the parent's
buffer, with `shape` and `strides` pointing one past the parent's. Begin
builds it in `scratch` and points `elem` at it; advance adds `step` to the
view's `offset_elems`. A view owns nothing, so the scratch needs no
destroying, and an inner `for` over it begins its own cursor from it as from
any tensor.

**Set and map.** Both are B+trees whose leaves are chained by `next_leaf`, so
an in-order walk is the leaves left to right, which `btreemap_collect_impl`
already does. Begin descends the left spine once (`leftmost_leaf`). Advance is

```
slot += 1
if slot == leaf.len: leaf = leaf.next_leaf; slot = 0
elem = leaf.keys + slot * key_size     (and value for a map)
pos += 1
```

with the layout offsets read from the descriptor's leaf layout, which is why
advance takes the collection's descriptor. The last leaf's `next_leaf` is
null exactly when `pos` reaches `end`; the advance that lands there sets
`elem` and `value` to null instead of reading through it.

## Erased elements

Inside a generic, the loop binding's static type is `data` and the element is
whatever the caller's collection holds. A reference to it has to carry the
element's descriptor, which `ListElementRef` already arranges: Cranelift
keeps it in `ref_desc_values` and the interpreter in the value's tydesc. A
cursor's element descriptor is constant for the whole walk, so it is read
once, before the loop, off the collection's descriptor with
`dtlv_rti_element_tydesc`. That covers a list, a set, a tensor and a map's
value. A map's key has no such accessor yet and needs one.

The cursor itself never sees a descriptor, so it does not need a fat form.

## In the IR

The cursor goes in a slot of a new `IrType::Cursor`, copy and pointer-aligned,
and six instructions use it:

| Instruction | Does |
|---|---|
| `CursorBegin { cursor, coll }` | Borrows `coll` and fills the cursor slot |
| `CursorDone { dest, cursor }` | `dest = pos == end` |
| `CursorElem { dest, cursor }` | Produces a `Ref` to the current element (a map's key) |
| `CursorValue { dest, cursor }` | Produces a `Ref` to a map's current value |
| `CursorPos { dest, cursor }` | `dest = pos`, an `index` |
| `CursorAdvance { cursor }` | Moves to the next element |

The rank-above-1 scratch is not in the IR: each backend reserves it per
`CursorBegin`, as each already reserves the view `TensorIndexRef` builds.

**References stay non-first-class.** A `for` binding is not lowered to a
stored `Ref`. Every use of `item` in the body lowers to a fresh `CursorElem`
consumed by the read, clone or argument that follows it, which is the rule
[Generics](plan-generics.md#user-content-what-this-does-not-leave-a-borrow-taken-out-of-a-borrowed-container)
wants kept. What persists across the body, its `break`s and its nested loops
is the cursor, which is data and not a reference.

A loop lowers to:

```
    cursorbegin s0, v_coll
head:
    v1 = cursordone s0
    brif v1, exit, body
body:
    ... v2 = cursorelem s0 ... (each use)
    cursoradvance s0
    jump head
exit:
```

with `continue` jumping to the advance and `break` to `exit`.

## In the backends

**Cranelift and C AOT, lists and tensors.** No runtime call at all. The
cursor's fields live in variables rather than memory -- Cranelift
`Variable`s, C locals -- so `elem`, `pos` and `end` stay in registers, begin
is the loads above, advance is a pointer add and an increment, and done is a
compare. That is the pointer-math shortcut; the struct is the contract for
everyone else, and these backends are free not to materialize it. The
erased case reads the stride off the element descriptor before the loop,
which is the one difference from the static case, and the same split
`compile_list_element_ref` already makes.

**Cranelift and C AOT, sets and maps.** Call begin and advance, keep the
cursor in a stack slot, and read `elem`, `value`, `pos` and `end` from it
inline. Later, advance can be inlined within a leaf -- bump `slot`, `elem`
and `value` when `slot + 1 < leaf.len`, and call only to cross to the next
leaf, which is one call per eleven elements.

**Interpreter.** Calls the runtime for every kind and keeps the cursor in its
slot. Inlining the list case is an option, not a requirement.

**Bytecode.** The same, until a fused per-kind `cursor next` op is worth
having; `plan-bytecode.md` is where that would be weighed.

These are `dtlv_rti_*` entry points the compiler emits by name, for a
language construct, the way literals and operators reach the runtime today.
The rule in the compiler guide that domain operations move into riders is
about operations a module calls; there is no module between `for` and the
runtime.

## What keeps it sound

The cursor holds pointers into the collection's heap storage: a list's buffer,
a tree's leaves, a tensor's buffer and its shape and strides. Any change to
the collection can free or move those. The runtime does not check for that,
because the language rules it out: the iteratee of a `for` is frozen for the
loop -- no `set xs[...]`, no `mut xs`, no move of `xs` -- which ownership
analysis enforces as it does for any borrow.

The container's header moving is harmless, since nothing points at it. Debug
builds can afford a cheap assertion in the tree advance (that `node` is a
leaf and `slot` is in range), which catches a frozen-iteratee bug that got
past analysis.

## What exists and what is missing

| Piece | Status |
|---|---|
| Leaf chaining and `leftmost_leaf` for maps and sets | exists |
| View construction for tensor rank above 1 | exists, in `TensorIndexRef` |
| Fat references to erased elements | exists, in `ListElementRef` |
| `dtlv_rti_element_tydesc` for list, set, tensor, map value | exists |
| A map key descriptor accessor | missing |
| `rtdt::Cursor` and the eight entry points | missing |
| `IrType::Cursor` and the instructions | missing |
| Backend support | missing |

`ABI_VERSION` changes on its own once `Cursor` and the table entries exist,
since it hashes both.

## Phases

1. **Runtime.** `rtdt::Cursor`, the eight entry points, the map key
   descriptor accessor. Tests in `datalove-rt-tests` walk each kind and
   compare with the positional reads: empty, one element, trees of several
   leaves and of several levels, trees after removals have merged leaves,
   tensor views of rank 2 and 3, and a negative stride through the runtime's
   own transpose.
2. **IR and interpreter.** The type, the instructions with their display,
   serialization, verification and DCE, and the walker calling the runtime.
3. **Cranelift and C AOT.** Lists and tensors inline, trees through the
   runtime.
4. **Bytecode.**
5. **Tree advance inlined within a leaf**, if measurement says so.

Lowering `for` itself onto this belongs to the language work; with phase 2
done it can land and run on the interpreter while phases 3 and 4 follow.

## Open questions

- **Reverse walks.** Leaves chain forward only. A reverse cursor over a tree
  would need back links or a stack of the path down; lists and tensors are a
  negative `step`. Nothing asks for it yet.
- **A mutable cursor.** Out of scope, but the shape should not foreclose it:
  `elem` and `value` would become writable places, a list's or tensor's
  element and a map's value, never a set element or a map key.
- **Tables.** A row loop over named columns would be one list-like walk per
  column, advanced in lockstep and sharing `pos`. Whether that is several
  cursors or a table cursor with a pointer per column waits on the table
  design.
- **Whether to inline the list case in the interpreter.** Cheap to do, but it
  is a second implementation of the same walk.
