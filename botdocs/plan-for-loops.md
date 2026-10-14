# `for` Loops over the Collections

A plan for `for` over lists, tensors, sets and maps, from the parser to the
backends. Every collection is walked through a cursor whose whole protocol is
runtime functions; a backend that open-codes them is optimizing, not defining
them. It replaces "Immutable Cursors for the Collections", which predated fat
generics. Written and implemented October 2026, all five phases; what was
built differs from the plan where [What was built](#user-content-what-was-built)
says. The language side is in `mandocs/drafts/for-loops-for-collections.md`.

## Contents

- [What it is for](#user-content-what-it-is-for)
- [The language](#user-content-the-language)
- [The protocol](#user-content-the-protocol)
- [Strided cursors: lists and rank-1 tensors](#user-content-strided-cursors-lists-and-rank-1-tensors)
- [Row cursors: tensors of rank above 1](#user-content-row-cursors-tensors-of-rank-above-1)
- [Tree cursors: sets and maps](#user-content-tree-cursors-sets-and-maps)
- [In the IR](#user-content-in-the-ir)
- [The indexed form](#user-content-the-indexed-form)
- [Tradeoffs](#user-content-tradeoffs)
- [Front end](#user-content-front-end)
- [Ownership](#user-content-ownership)
- [Backends](#user-content-backends)
- [Bugs found on the way](#user-content-bugs-found-on-the-way)
- [What was built](#user-content-what-was-built)
- [Measurements](#user-content-measurements)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## What it is for

`sys/std` and the demos walk collections with an `index` counter, a wrapping
add, and an indexed read per element. For a list that read is a bounds check
and, for a linear element, a clone or a helper function to get around `?`
(`ord.contains` and `equal_to_at`). For a set or a map it is worse: `set.get`,
`map.key_at` and `map.value_at` walk the leaf chain from the leftmost leaf to
the position on every call (`btreeset_get_at_impl`, `btreemap_at_inner`), so
a whole walk is quadratic, and each read clones.

A survey of `sys/`, `demos/` and `benchvs/` found about 88 loops over a
collection. About 33 are plain forward walks that `for item in ref xs` covers
directly, 7 more walk a map through `map.keys()` and would be `for k, v`, and
about 10 use the index for something besides reading the same list
(`index_of`, `least`, `greatest`, `build_*_slots`, separators). The rest --
binary searches, heaps, merges, sieves, writes through the index, lockstep
walks, and 19 loops over another module's counts -- are out of reach of any
`for`, and stay `loop while`.

## The language

```datalove
for item in ref xs          // list, set, tensor
for i, item in ref xs       // list, tensor: i is the index along axis 0
for key, value in ref m     // map
```

- The iteratee is written with `ref`, and is a place: a name, a parameter, a
  field or index projection (`ref p.items`, `ref xss[i]?`). A temporary
  (`ref f()`) is left for later; see [Open questions](#user-content-open-questions).
- The bindings are `ref` bindings: they cannot be moved, assigned or passed
  `mut`/`out`. `@` clones. A copy element reads like any value.
- The iteratee's root binding is **frozen** for the loop: no move, no `set` of
  it or through it, no `mut`/`out` argument. That is what keeps the cursor's
  pointers sound, and the runtime does not check it.
- A tensor of rank above 1 binds a view of rank one less, as `t[i]?` does
  under a borrow.
- Tables and strings are not iterable yet.
- `break` and `continue` work as in `loop`; `continue` goes to the next
  element.

## The protocol

Every collection kind has a cursor type and the same five operations. The
kind is always known statically -- an erased `[T]` is still a list -- so
nothing dispatches on a descriptor's tag.

| Operation | Does | Precondition |
|---|---|---|
| **begin** | Fills a cursor at the first element, or done for an empty collection | The collection is frozen until the walk ends |
| **done** | Whether the walk is over | -- |
| **elem** | A pointer at the current element; a map's current key | Not done |
| **value** | A pointer at a map's current value | Not done; maps only |
| **advance** | Moves to the next element, or to done after the last | Not done |

The loop tests at the top, so begin and done carry the empty case and no
operation needs a non-empty collection.

Three cursor types cover the four collections:

| Collection | Cursor | Done when |
|---|---|---|
| list | `StrideCursor` | `remaining == 0` |
| tensor, rank 1 | `StrideCursor` | `remaining == 0` |
| tensor, rank above 1 | `RowCursor` | `remaining == 0` |
| set, map | `TreeCursor` | `leaf` is null |

Each is plain data in `datalove-rtdt`. It owns nothing, so it is copy: drops,
tracking bytes and leak checking never see it. A cursor holds pointers into
the collection's heap storage, which is sound only because the iteratee is
frozen.

Every function, by name:

```
// Lists and rank-1 tensors.
dtlv_rti_list_cursor_begin_local(rt, list_ref, list_tydesc, cursor_out) -> RtStatus
dtlv_rti_tensor_cursor_begin_local(rt, tensor_ref, tensor_tydesc, cursor_out) -> RtStatus
dtlv_rti_stride_cursor_done(cursor_ref) -> bool
dtlv_rti_stride_cursor_elem(cursor_ref) -> *const u8
dtlv_rti_stride_cursor_advance(cursor_mut)

// Tensors of rank above 1.
dtlv_rti_tensor_row_cursor_begin_local(rt, tensor_ref, tensor_tydesc, cursor_out) -> RtStatus
dtlv_rti_row_cursor_done(cursor_ref) -> bool
dtlv_rti_row_cursor_elem(cursor_ref) -> *const u8
dtlv_rti_row_cursor_advance(cursor_mut)

// Sets and maps.
dtlv_rti_btreeset_cursor_begin_local(rt, set_ref, set_tydesc, cursor_out) -> RtStatus
dtlv_rti_btreemap_cursor_begin_local(rt, map_ref, map_tydesc, cursor_out) -> RtStatus
dtlv_rti_btreeset_cursor_advance_local(rt, cursor_mut, set_tydesc) -> RtStatus
dtlv_rti_btreemap_cursor_advance_local(rt, cursor_mut, map_tydesc) -> RtStatus
dtlv_rti_tree_cursor_done(cursor_ref) -> bool
dtlv_rti_tree_cursor_elem(cursor_ref) -> *const u8
dtlv_rti_tree_cursor_value(cursor_ref) -> *const u8
```

Sixteen functions. Begin, and the tree's advance, read the collection's heap,
take its descriptor, and follow the `_local` and `RtStatus` conventions. The
rest only read or bump the cursor's own fields, need neither the handle nor a
descriptor, cannot fail, and return their answer directly, as
`dtlv_rti_field_offset` does.

## Strided cursors: lists and rank-1 tensors

```rust
#[repr(C)]
pub struct StrideCursor {
    pub elem: *const u8,     // The current element.
    pub stride: usize,       // Bytes from one element to the next.
    pub remaining: Index,    // Elements from this one to the end; 0 is done.
}
```

- **List begin:** `elem = data`, `stride` the element size from the list's
  descriptor (in a generic, the table entry for `[T]`), `remaining = size`.
- **Rank-1 tensor begin:** `elem = ptr_base + offset_elems * size`,
  `stride = strides[0] * size`, `remaining = shape[0]`.
- **advance:** `elem += stride` (wrapping), `remaining -= 1`.
- **done:** `remaining == 0`. **elem:** `elem`.

The count, rather than an end pointer, is what makes zero-sized elements
work. `Unit` and `Atom` are zero-sized, so a `[()]` of length 3 has its end
pointer equal to its start, and an end-pointer loop would run no times. A
count also survives a stride standing for a negative one, which the tensor's
field comment allows and nothing creates today.

## Row cursors: tensors of rank above 1

```rust
#[repr(C)]
pub struct RowCursor {
    pub view: Tensor,        // The current row: a view of rank one less.
    pub stride: Index,       // Elements from one row to the next (strides[0]).
    pub remaining: Index,    // Rows from this one to the end; 0 is done.
}
```

The view is the one `TensorIndexRef` builds today: capacity zero, aliasing
the parent's buffer, `shape` and `strides` one past the parent's. It lives in
the cursor, so there is no scratch for a backend to reserve.

- **begin:** the view at row 0, `stride = strides[0]`, `remaining = shape[0]`.
- **advance:** `view.offset_elems += stride` (wrapping), `remaining -= 1`.
- **elem:** a pointer at `view`, inside the cursor. It is valid for the
  iteration, since the cursor's slot lives for the whole loop and only
  advance changes it. An inner `for` over the row begins its own cursor from
  that view, as from any tensor.

## Tree cursors: sets and maps

```rust
#[repr(C)]
pub struct TreeCursor {
    pub leaf: *const u8,     // The current leaf; null is done.
    pub elem: *const u8,     // The current key; a set's element.
    pub value: *const u8,    // A map's current value; null for a set.
    pub slot: u32,           // The current key's slot within the leaf.
}
```

There is no count. The end is the leaf chain's end: the last leaf's
`next_leaf` is null, and an empty collection's root is null.

- **begin:** a null root gives a null `leaf`. Otherwise it descends the left
  spine (`leftmost_leaf`) and takes slot 0.
- **advance:** `slot += 1`. If `slot == leaf.len`, it moves to `next_leaf`
  and slot 0, and a null `next_leaf` leaves the cursor done. `elem` and
  `value` are recomputed from the leaf's layout (`set_leaf_layout`,
  `map_leaf_layout`) and the key and value sizes, which is why advance takes
  the descriptor.
- **done:** `leaf` is null. **elem**, **value:** the fields.

**No leaf is empty.** Both trees rebalance on removal (`fix_underflow`,
`MIN_KEYS`), so the leaf advance moves to always holds an element and nothing
has to skip. A debug assertion in advance says so. Until October 2026 a set's
removal only shifted keys within the leaf, leaving empty leaves in the chain;
that was fixed ahead of this plan, and `btreeset_tests` checks the tree's
shape after every removal.

## In the IR

One type and five instructions:

```rust
IrType::Cursor(CursorKind)          // CursorKind::{Stride, Row, Tree}

CursorBegin   { cursor: SlotId, coll: Operand }
CursorDone    { dest: ValueId, cursor: SlotId }            // dest: Bool
CursorElem    { dest: ValueId, cursor: SlotId }            // dest: Ref(element or key)
CursorValue   { dest: ValueId, cursor: SlotId }            // dest: Ref(value)
CursorAdvance { cursor: SlotId, coll: Operand }
```

`CursorKind` picks the layout (`struct_layout::<rtdt::StrideCursor>()` and so
on) and the functions. `CursorAdvance` names the collection for the tree's
descriptor; the other kinds ignore it.

A loop lowers to:

```
    cursorbegin s_c, v_coll
    slotstore.copy s_i, 0             // indexed form only
    goto head
head:
    d = cursordone s_c
    brif d, exit, body
body:
    r = cursorelem s_c
    v = cursorvalue s_c               // maps only
    i = slotload.copy s_i             // indexed form only
    ...                               // item is *r
    goto next
next:                                 // continue jumps here
    cursoradvance s_c, v_coll
    slotstore.copy s_i, (add i, 1)    // indexed form only
    goto head
exit:
```

**The element reference is made once per iteration**, at the head of the
body, and `item` is bound to `Operand::ValueRef(r)`. Under fat generics a
`Ref` is a plain pointer described by the table entry for its pointee
(`plan-fat-generics.md`, "What it does for other work"), and `r` lives only
within one iteration, dominating every use. The precedent is promoted
function consts, the one named `Ref` value today (`const/src/promote.rs`).
An index binding is the loaded `i`, as `Operand::Value`, so it cannot be
assigned either.

## The indexed form

The index is a counter the lowering keeps, and only when the loop asks for
one. It is never more than the collection's length, so its increment cannot
overflow, and no source code has to write the wrapping add. Lists and tensors
have it.

Sets and maps do not, for now:

- **Nothing asks for it.** No loop in `sys/`, the demos or the benchmarks uses
  a set or map position for anything but walking in order. `map.key_at` and
  `map.value_at` have no callers outside a test fixture, and `set.get` has
  none outside `set.dfm`'s own walks.
- **A position in a tree is not a handle.** The only thing to do with it is
  `set.get(i)`, which is O(i). For a list, `i` is the cheap way back to the
  element; for a set, it is a trap.
- **Two bindings already mean key and value on a map.** `for i, x` on a set
  and `for k, v` on a map would read alike and mean different things. For a
  `#{index}` turned into a `%{index = V}` the change would typecheck.

Nothing in the protocol would change if they did: the counter is the
lowering's, not the cursor's.

## Tradeoffs

**A protocol of runtime functions, open-coded later.** The full protocol is
one place to read, and the interpreter runs exactly it. Open-coding is an
optimization in Cranelift and C AOT, and it has to keep the cursor's layout
and meaning, field for field: OSR enters compiled code with a cursor the
interpreter built. The cost is speed until then. A list `for` is three calls
per element in every engine; the bytecode's `loop while` it replaces is a
fused bounds check and an inline element reference. Measure before and after
open-coding.

**Three cursor types, not one.** One struct for every kind needed fields
some kinds ignored and a `step` meaning bytes, elements or a slot. Per-kind
structs are each minimal, and the IR type names its kind, so a backend still
knows the layout statically.

**A count for strided cursors, a sentinel for trees.** An end pointer fails
for zero-sized elements, as above. A tree has no such problem, since it moves
by slot, and its chain already ends in null, so a count would be bookkeeping
the walk does not need. Both done tests are one load and one compare.

**Testing at the top.** The loop calls done once more than a rotated loop
would, on entry. In exchange, begin and advance are total over the states a
loop can reach, and no caller has to check emptiness first.

**The row view inside the cursor.** The earlier plan built the view in
caller-lent scratch. Inside the cursor it needs no scratch in any backend,
and the C AOT `static` view (see [Bugs](#user-content-bugs-found-on-the-way))
never comes into it. The cost is a cursor of 48 bytes (64 under
`index-64`), and an element pointer pointing into the cursor rather than the
collection.

**The index in lowering, not in the cursor.** A loop without `i` pays
nothing for it. A loop with it keeps a counter beside a cursor that, for a
list, could have yielded it by subtraction and division. That is the case to
revisit if open-coding makes the counter visible in profiles.

## Front end

Every statement begins with its own keyword, so `for` takes no name away
(`botspec.md` 2.1). `in` sits where no name can, as `while` does after `loop`,
and joins that list. `ref` after `in` is the mode, not a value named `ref`,
for the same reason -- but only while the mode is mandatory. A by-value
`for x in xs` would make the word after `in` either a mode or a name, and
2.1 settles that by reserving `ref` for values. See
[Open questions](#user-content-open-questions).

Where the statement threads through:

| Site | Change |
|---|---|
| `ast/src/ast.rs`, `ast_serde.rs` | `Statement::For(StmtFor)`: bindings, iteratee place, body |
| `ast/src/reachable.rs` | `statement_completes` is true; `contains_break` does not descend into it |
| `parser/src/statement.rs` | dispatch on `for`, `parse_for` after `parse_loop`, the P001 keyword list |
| `sema/src/lib.rs` | a new `StmtKey` kind (18) |
| `tycheck/src/statement.rs` | the iteratee's type gives the bindings' types; arity by kind; `loop_depth` |
| `ownership/src/lib.rs` | see [Ownership](#user-content-ownership) |
| `lower/src/stmt.rs`, `script.rs`, `context.rs` | the loop shape; `LoopLowerContext` gains `next`, where `continue` goes: the header for `loop`, the advance for `for` |
| `worldgen` | the shape in `ALL_SHAPES` and the generator |
| `datalove-mode.el`, the four `datalove-prism.js` copies | keyword and indentation |

The script lowering duplicates `break`/`continue`; both copies need the
continue target. The cursor and counter slots are not bindings, so the
lowering stores and loads them with the `SlotStoreCopy` and `SlotLoadCopy`
instructions directly. `emit_slot_store_copy` asks `is_slot_tracked`, which
panics for a slot with no binding.

## Ownership

- **The bindings** are allocated with `param_mode: Some(ParamMode::Ref)`, as
  a `ref` parameter is, so a move is D003, a `mut`/`out` argument D004, and
  nothing drops them: not at the body's end, and not at a `break` or
  `continue` either.
- **Freezing.** A stack of walked roots on `AnalysisCtx`, pushed for the
  body: a binding, or the name of one an earlier script unit made, which has
  no binding here. The root is found before the loop's own names are bound,
  so `for xs in ref xs` freezes the outer `xs`. It is checked in `mark_moved`,
  in `analyze_set` against the place's root (whole, indexed, field and
  compound), and in `check_mutable_argument`. Every refusal is D015, at the
  `set`'s target or at the expression. A nested loop over the outer binding's
  row freezes the same root again, harmlessly.
- **Exits.** `merge_loop_exits` merges `state_before` for a loop that can end
  without running its body, `loop while` and `for`, and the state at each
  `break`. Out parameters are settled the same way: one is written after the
  loop only if every way out wrote it.
- **Moves in the body** of an outer binding are D007 as for any loop.

## Backends

| | Calls the protocol | Open-coded |
|---|---|---|
| IR walker | every operation | nothing |
| Bytecode | every operation: done, elem, value and a strided or row advance as their own ops (`Op::Cursor*`), begin and a tree's advance on the walker | nothing |
| Cranelift | begin over a tensor or tree; a tree's advance across leaves | a list's begin; every done, elem and value; a strided or row advance; a tree's advance within a leaf |
| C AOT | as Cranelift | as Cranelift |

The bytecode's ops call the runtime's functions, which thin LTO inlines into
the bytecode loop, so the interpreter runs the protocol as written and still
beats the index loop it replaces. The compiled backends read every field
offset with `offset_of!` on the `rtdt` structs, so there is no second
description of a cursor's layout to drift from the runtime's.

- **A new entry point** goes in `rt/src/c/mod.rs`, then `just gen-rti`, then
  `RuntimeImports` (the field, `declare`, `all_mut`), the JIT's
  `runtime_addresses`, and a C prototype.
- **`ABI_VERSION`** picks up the new table entries by itself. No cursor is
  passed to a rider, so the structs need no `mix` lines unless that changes.
  A backend that open-codes a cursor reads its layout from the `rtdt`
  structs themselves.
- **The cursor slot** needs a descriptor in the interpreter, which checks each
  slot's descriptor against `layout_of` (`interp/src/layout.rs`). A
  plain-data stand-in like `dynamic_reference`'s does. Cranelift and C keep
  it in the frame like any aggregate.
- **The generics gates**: `supports_dynamic` in Cranelift and C AOT must
  admit the cursor instructions on a dynamic collection, or every generic
  using `for` falls back to the interpreter under the JIT and fails to build
  under AOT.
- **DCE**: `CursorBegin` and `CursorAdvance` write a slot and have no dest, so
  they are in `has_side_effects`. The others read a copy slot and are
  removable as they are.
- **OSR** works without anything new: the cursor and counter are slots in the
  interpreter's frame, which compiled code continues from.

## Bugs found on the way

- **C AOT tensor views are static.** `TensorIndexRef` at rank above 1 writes
  its view into `static _Alignas(8) uint8_t __view[...]`
  (`c-aot/src/codegen.rs`), one per instruction. A recursive call that
  reaches the same instruction overwrites the caller's live view. `for` no
  longer goes through it; `t[i]?` still does. The view belongs in `__frame`.
- **Fixed: DCE could drop a borrowed collection.** `consumed_owned` treated
  `ListElementRef`, `MapValueRef` and `TensorIndexRef` as consuming, so
  removing an unused one whose collection was an owned `Value` inserted a
  `Drop` of the collection. Every element reference had a use, so it never
  fired. They are in the non-consuming list beside `GetFieldRef` now.
- **Fixed: a reference binding read for its value handed over the pointer.**
  `lower_expression` returned a name bound to `Operand::ValueRef` as the
  reference itself, which only promoted consts had been, and those are never
  copy. A `for`'s copy element put in a tuple, a list, a `let` or an argument
  was the address. It now copies through the reference. Fixture
  `engines/for_006_copy_elements_by_value`; the store demo found it, by
  crashing.
- **Fixed: set removal never rebalanced**, leaving empty leaves in the
  chain. It now borrows and merges as the map's does.
- **Fixed: bulk-built trees had wrong separators.**
  `btreeset_build_from_sorted_slice` and `btreemap_build_from_sorted_slices`,
  which datalit instantiation uses, took an internal child's first key as
  the separator before it, rather than the least key of its subtree. A set
  or map of 133 or more entries built that way did not find some of them.
- **Fixed: the map lost a child borrowing into an empty internal node.**
  `borrow_from_left_internal` shifted the node's children only when it had
  keys, so a node down to none lost its one child. Only a bulk-built map,
  whose internal nodes can start below the minimum, reached it.
- **Fixed, found in review:**
  - `break` and `continue` dropped a `for`'s element bindings, freeing them
    inside the collection: `live_bindings_in_scopes`, unlike every other drop
    point, did not skip a borrowed binding, and none had lived in a loop
    before. Fixture `engines/for_009`.
  - A walk over an element whose key is built for the lookup, `m["k"]!` or
    `m[key()]!`, tripped an assertion that a place leaves no temporaries. The
    key goes once the reference is had, before the walk.
  - A field of a temporary, `make().items`, was taken for a place, and the
    struct leaked (F087, `error/104`).
  - A collection an earlier script unit made was not frozen, the walked root
    being looked up only among this unit's bindings (`engines/for_007`).
  - `for x, x` was accepted (D038, `error/105`).
  - An out parameter written only in a loop's body counted as written after
    a loop that can end before its body runs, `loop while` as well as `for`
    (D006, `error/106`).
  - Assigning an earlier unit's binding inside any loop's body panicked in
    lowering, which found no slot for it (`engines/for_008`).
  - C AOT advanced a strided cursor past its buffer as a pointer, which C
    does not define; it adds to the integer now.
- **A missing prototype.** C AOT emits
  `dtlv_rti_tensor_hyperplane_clone_local` with no `extern` prototype, so it
  compiles as an implicit declaration.

## What was built

What the plan says, with these differences:

- **The bytecode has its own cursor ops** rather than running every cursor
  instruction on the walker. They call the protocol, so the interpreter still
  open-codes nothing.
- **Begin over a tensor or a tree stays a call** in the compiled backends; it
  runs once a walk.
- **Errors.** P078 (a `for` header that does not parse), P079 (no `ref`),
  F087 (a temporary, a value that is not a collection, or the wrong number of
  names for it), D015 (changing the collection being walked).
- **Worldgen writes `for`** over a collection in scope, at its own
  probability (`for_probability`), since the roll for a statement rarely
  found one: one world in eighty had a `for` before, fifteen after.
- **Not done:** the C AOT `static` view and the missing prototype, below in
  [Bugs](#user-content-bugs-found-on-the-way), which `for` no longer reaches.

Fixtures: `engines/for_001` to `for_006`, `std_tests/179_for_over_a_shrunk_set`,
`error/96` to `103`, and `datalove-rt-tests/tests/cursor_tests.rs` for the
protocol alone, transposed and sliced tensors included, which no program can
build yet.

## Measurements

Release build, one machine, `hyperfine` means. A list of 100,000 `u32`s
summed 200 times, and a set of 5,000 summed 20 times:

| | interp | JIT | AOT |
|---|---:|---:|---:|
| list, index loop | 230 ms | 38 ms | 18.1 ms |
| list, `for`, every operation a call | 487 ms | 155 ms | 52 ms |
| list, `for`, as built | 193 ms | 34 ms | 16.4 ms |
| set, `set.get` by position | 58 ms | 61 ms | 53 ms |
| set, `for` | 7.2 ms | 7.5 ms | 1.4 ms |

Calls for every operation cost the list two to four times the index loop,
which is why open-coding came before porting `sys/std`. As built, a list's
`for` is faster than its index loop on every engine, and a set's is linear
rather than quadratic.

`benchvs`, before phase 4 and after phase 5:

| | interp | JIT | AOT |
|---|---:|---:|---:|
| `wordfreq` before | 851 ms | 521 ms | 470 ms |
| `wordfreq` after | 831 ms | 509 ms | 445 ms |
| `ordered` before | 2.87 s | 674 ms | 505 ms |
| `ordered` after | 2.83 s | 670 ms | 504 ms |

`wordfreq` stops building a map's keys and values as lists to walk them.
`ordered` spends its time in binary searches, and its one converted walk is
in the noise.

## Phases

All five are done.

1. **The runtime protocol**, all three cursors, with `datalove-rt-tests`
   walking each kind against the positional reads: empty, one element, a
   `[()]`, lists of linear elements, a transposed and a sliced tensor, rank 2
   and 3, trees of one leaf and of several levels, trees after removals have
   merged leaves, and bulk-built trees.
2. **`for` over lists on the interpreter**: the front end, freezing, the IR,
   the loop shape, the interpreter's calls. Engine fixtures for: empty, one
   element, `break` and `continue`, a body that never reads `item`, linear
   and copy elements, a generic `[T]`, nested loops, a `ref` parameter, a
   field and an index projection, each freeze error.
3. **Every collection on every engine**, through the calls: tensors, sets and
   maps, the indexed form, Cranelift and C AOT, the generics gates.
4. **Port `sys/std` and the demos.** `ord.contains`, `index_of`,
   `count_of`, `least`, `greatest`, `fixedint.sum`/`product`, `list.flattened`
   and `concat`, the six walks in `set.dfm` (where `is_subset_of` gains its
   early exit), the `keys()`-then-`get_or` loops in the demos, and
   `wordfreq.top_checksum`. Measure `benchvs/ordered.dfs` and `wordfreq`
   against the `loop while` versions.
5. **Open-code** the strided and row cursors, and the tree's done, elem,
   value and in-leaf advance, in Cranelift and C AOT. Measure again. Bytecode
   ops if the interpreter's numbers call for them.

## Open questions

- **By-value iteration.** `for x in xs` consuming the list is natural for a
  linear type, but would make the word after `in` either a mode or a name,
  which 2.1 settles only by reserving `ref` for values. Decide before it is
  wanted, since it takes a name away.
- **Temporaries as the iteratee.** `for x in ref f()` needs the temporary held
  to the loop's end and dropped on every exit, `ret` and `?` included. Places
  only, until something wants it.
- **Whether `set.get`, `map.key_at` and `map.value_at` stay.** They exist so
  that a set or map could be walked by index. Once `for` does that, they are
  quadratic traps with no callers.
- **A mutable `for`.** `for mut x in mut xs` over a list, tensor, or a map's
  values. The cursors' `elem` and `value` would become writable places,
  never a set element or a map key, since changing one moves where it
  belongs.
- **Reverse walks.** A strided cursor runs backward with a negated stride
  and a begin at the last element. Tree leaves link forward only, so a
  reverse tree walk needs back links or a stack. Nothing in the survey asks
  for it.
- **Tables.** A row walk binding fields of one row, per column, waits on the
  table design (`design-table-rows.md`).
