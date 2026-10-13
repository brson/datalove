# `for` Loops over the Collections

A plan for `for` over lists, tensors, sets and maps, from the parser to the
backends. It replaces "Immutable Cursors for the Collections", which proposed
one runtime cursor for every collection and was written before generics moved
to fat pointers. Refreshed October 2026 against the compiler as it stands;
not implemented. The language side is in
`mandocs/drafts/for-loops-for-collections.md`.

## Contents

- [What it is for](#user-content-what-it-is-for)
- [The language](#user-content-the-language)
- [One loop shape](#user-content-one-loop-shape)
- [Lists and tensors: no cursor](#user-content-lists-and-tensors-no-cursor)
- [Sets and maps: a tree cursor](#user-content-sets-and-maps-a-tree-cursor)
- [The indexed form](#user-content-the-indexed-form)
- [Front end](#user-content-front-end)
- [Ownership](#user-content-ownership)
- [Backends](#user-content-backends)
- [What changed from the cursor plan](#user-content-what-changed-from-the-cursor-plan)
- [Bugs found on the way](#user-content-bugs-found-on-the-way)
- [What exists and what is missing](#user-content-what-exists-and-what-is-missing)
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
  it or through it, no `mut`/`out` argument. That is what keeps the
  references sound, and the runtime does not check it.
- A tensor of rank above 1 binds a view of rank one less, as `t[i]?` does
  under a borrow.
- Tables and strings are not iterable yet.
- `break` and `continue` work as in `loop`; `continue` goes to the next
  element.

## One loop shape

Every collection lowers to the same skeleton: a hidden counter `i` against a
length `n` read once, which is sound because the iteratee is frozen. Only the
per-element access differs by kind.

```
    n = len v_coll
    slotstore.copy s_i, 0
    brif (lt 0, n), begin, exit
begin:
    cursorbegin s_c, v_coll           // sets and maps only
    goto body
body:
    i = slotload.copy s_i
    r = <element ref>                 // listelementref v_coll[i], cursorkeyref s_c, ...
    ...                               // item is *r
    goto latch
latch:                                // continue jumps here
    i1 = add i, 1
    slotstore.copy s_i, i1
    brif (lt i1, n), step, exit
step:
    cursoradvance s_c, v_coll         // sets and maps only; empty otherwise
    goto body
exit:
```

The counter is never more than `n`, so its increment cannot overflow, and no
source code has to write the wrapping add.

The loop is rotated -- the test is at the latch -- so the cursor's
operations have simple contracts: `begin` runs only on a non-empty
collection, and `advance` only when there is another element. Neither needs a
"done" sentinel.

**The element reference is made once per iteration**, at the head of the
body, and `item` is bound to `Operand::ValueRef(r)`. The cursor plan made one
per *use*, so that no reference outlived an instruction; that rule came from
erasure, where a reference held across a loop needed its descriptor tracked.
Under fat generics a `Ref` is a plain pointer whose descriptor is the table
entry for its pointee (`plan-fat-generics.md`, "What it does for other
work"), and `r` lives only within one iteration, dominating every use. The
precedent is promoted function consts, the one named `Ref` value today
(`const/src/promote.rs`). An index binding is the loaded `i`, as
`Operand::Value`, so it cannot be assigned either.

## Lists and tensors: no cursor

A list or tensor needs nothing new beyond a length. The access is the
existing `ListElementRef` or `TensorIndexRef` on the counter:

- **Every backend already inlines them.** Cranelift computes
  `data + i * size` (`codegen/lists.rs`, `compute_element_addr`), reading the
  size off the table entry for a `[T]`. C AOT emits one line. The interpreter
  computes it in the walker. The bytecode has `Op::ListElementRef`, and
  `ListElementRefRt` for the generic case.
- **A tensor of rank above 1** builds a view per iteration, as `t[i]?` does
  today, in storage each backend already reserves per instruction. Nested
  `for` over rows and cells works out of the box, since an inner loop's
  iteratee is the outer one's view.
- **No bounds check.** The latch's `i < n` on a frozen collection is the
  check. IR invariant 8 says an element reference is dominated by its check
  instruction; it gets a second clause for this.

The pointer bump the cursor plan wanted -- `elem += step` instead of
`data + i * size` -- is strength reduction, and can come later inside a
backend, without the IR or the runtime knowing.

`Len { dest, coll }` is the one new instruction: a list's `size`, a set's or
map's `len`, a tensor's `shape[0]`. All four are a load from the header, so
every backend inlines it. It reads the collection without consuming it.

## Sets and maps: a tree cursor

Sets and maps are B+trees with their entries in leaves chained by
`next_leaf`, so an in-order walk is the leaves left to right
(`btreemap_collect_impl`, `btreeset_to_list_impl`). A cursor remembers where
it is in that chain:

```rust
#[repr(C)]
pub struct TreeCursor {
    pub leaf: *const u8,   // The current leaf.
    pub key: *const u8,    // The current key; a set's element.
    pub value: *const u8,  // A map's current value; null for a set.
    pub slot: u32,         // The key's slot within the leaf.
}
```

It is plain data and owns nothing. It is copy, so drops and tracking never see
it, and it lives in a slot of a new `IrType::TreeCursor` (layout
`struct_layout::<rtdt::TreeCursor>()`). There is no position or count in it:
the loop's counter does that job.

Four runtime entry points:

```
dtlv_rti_btreeset_cursor_begin_local(rt, set_ref, set_tydesc, cursor_out) -> RtStatus
dtlv_rti_btreeset_cursor_advance_local(rt, cursor_mut, set_tydesc) -> RtStatus
dtlv_rti_btreemap_cursor_begin_local(rt, map_ref, map_tydesc, cursor_out) -> RtStatus
dtlv_rti_btreemap_cursor_advance_local(rt, cursor_mut, map_tydesc) -> RtStatus
```

- **begin** descends the left spine (`leftmost_leaf`) to the first element.
  The collection is non-empty.
- **advance** moves to the next slot, or along `next_leaf` to the first slot
  of the next leaf. There is a next element.
- Both take the collection's descriptor for the leaf layout
  (`set_leaf_layout`, `map_leaf_layout`) and the key and value sizes. In a
  generic that is the table entry for `#{T}` or `%{K = V}`; nothing else is
  needed for erased elements.

**No leaf is empty.** Both trees rebalance on removal (`fix_underflow`,
`MIN_KEYS`), and an empty set or map has a null root, so every leaf in a
chain holds at least one element and advance never has to skip one. A debug
assertion in advance says so. Until October 2026 a set's removal only shifted
keys within the leaf, leaving empty leaves in the chain; that was fixed
ahead of this plan, and `btreeset_tests` now checks the tree's shape after
every removal.

Three instructions besides `Len`:

| Instruction | Does |
|---|---|
| `CursorBegin { cursor, coll }` | Calls begin into the slot |
| `CursorAdvance { cursor, coll }` | Calls advance on the slot |
| `CursorKeyRef { dest, cursor }` / `CursorValueRef { dest, cursor }` | Load `key` or `value`; produce a `Ref` |

The key and value references are loads, so they are inline everywhere. Begin
and advance are calls everywhere at first. Advance within a leaf can later be
inlined -- bump `slot`, `key` and `value` while `slot + 1 < leaf.len`, and call
only to cross a leaf -- which is one call per up to eleven elements.

## The indexed form

**With this shape the index costs nothing for any collection:** it is the
loop's counter, which every kind has. So whether sets and maps get it is a
language question only, and nothing in the implementation depends on the
answer.

The answer here is not yet, for either:

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

What a counter is wanted for on a set -- "is this the first", "how many so
far" -- is rare, and the user's own counter does it. If that becomes common,
a form that names the position the same way on every collection fits
better than reusing the two-binding form.

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
| `lower/src/stmt.rs`, `script.rs`, `context.rs` | the loop shape; `LoopLowerContext` gains a `continue_target`, which is the header for `loop` and the latch for `for` |
| `worldgen` | the shape in `ALL_SHAPES` and the generator |
| `datalove-mode.el`, the four `datalove-prism.js` copies | keyword and indentation |

The script lowering duplicates `break`/`continue`; both copies need the
continue target. The counter and cursor slots are not bindings, so the
lowering stores and loads them with the `SlotStoreCopy` and `SlotLoadCopy`
instructions directly. `emit_slot_store_copy` asks `is_slot_tracked`, which
panics for a slot with no binding.

## Ownership

- **The bindings** are allocated as `alloc_const_param` allocates a const
  parameter: `param_mode: Some(ParamMode::Ref)`, so a move is D003, a
  `mut`/`out` argument D004, and nothing drops them.
- **Freezing.** A stack of frozen root `BindingId`s on `AnalysisCtx`, pushed
  for the body, checked in `mark_moved` and the zero-step place consume path,
  in `analyze_set` against the place's root (whole, indexed, field and
  compound), and in `check_mutable_argument`. Each refusal is a new error
  code. A nested loop over the outer binding's row freezes the same root
  again, harmlessly.
- **Exits.** `merge_loop_exits` merges `state_before` only for
  `loop while`. A `for` can always run out of elements, so it merges it too.
  It is typed on `StmtLoop` and needs to take either.
- **Moves in the body** of an outer binding are D007 as for any loop.

## Backends

| | `Len` | Element refs | Cursor ops |
|---|---|---|---|
| Interpreter | header load | existing | calls `datalove_rt::c` directly |
| Bytecode | a small `Op` (or `Op::Ir` at first) | existing `ListElementRef` ops | `Op::Ir` |
| Cranelift | inline load | existing | `RuntimeImports` calls; key/value loads inline |
| C AOT | inline load | existing | calls, with prototypes in `emit_header` |

- **A new entry point** goes in `rt/src/c/mod.rs`, then `just gen-rti`, then
  `RuntimeImports` (the field, `declare`, `all_mut`), the JIT's
  `runtime_addresses`, and a C prototype.
- **`ABI_VERSION`** picks up the new table entries by itself. `TreeCursor`
  does not appear in the table, and nothing in the ABI passes one to a rider,
  so it needs no `mix` line unless that changes.
- **The cursor slot** needs a descriptor in the interpreter, which checks each
  slot's descriptor against `layout_of` (`interp/src/layout.rs`). A
  plain-data stand-in like `dynamic_reference`'s does. Cranelift and C keep
  it in the frame like any aggregate.
- **The generics gates**: `supports_dynamic` in Cranelift and C AOT must
  admit `Len` and the cursor instructions on a dynamic collection, or every
  generic using `for` falls back to the interpreter under the JIT and fails to
  build under AOT.
- **DCE**: `CursorBegin` and `CursorAdvance` write a slot and have no dest, so
  they go in `has_side_effects`. `Len`, the cursor refs, `ListElementRef` and
  `TensorIndexRef` go in `consumed_owned`'s non-consuming list. Otherwise, a
  body that never reads `item` gets its unused element reference removed
  *and a `Drop` of the collection inserted*, when the collection is an owned
  `Value`. Today every element reference has a use, so this has never fired.
- **OSR** works without anything new: the counter and cursor are slots in the
  interpreter's frame, which compiled code continues from.

## What changed from the cursor plan

| The cursor plan | Now |
|---|---|
| One `Cursor` for every kind, six instructions, eight entry points | Lists and tensors use the existing element refs; a cursor only for trees, three instructions and `Len`, four entry points |
| `pos` and `end` in the cursor | The loop's own counter, shared by every kind |
| A fresh `CursorElem` per use of `item` | One reference per iteration; `Ref` is a plain pointer under fat generics |
| Erased elements via `dtlv_rti_element_tydesc`, and a map-key accessor to add | Removed with erasure; every descriptor is a table entry |
| `get_at` and `key_at` descend from the root per element | They walk the leaf chain from the left per call: O(index), quadratic per walk |
| "`next_leaf` is null exactly when `pos` reaches `end`" | A set's chain could hold empty leaves; set removal now rebalances, so it cannot |
| A negative stride, wrapping "as the runtime's own offset arithmetic does" | The runtime saturates, and nothing makes a negative stride today |
| Cursor fields in Cranelift `Variable`s | `Variable`s are used only for OSR; slots, as everything else |
| `ABI_VERSION` changes on its own | True for table entries; a struct only via a hand-written `mix` |

The first table row is the main change. A uniform cursor would have put a new
IR type, a runtime call per element in the interpreter, and scratch for a
view in every backend onto lists, which already have inline, tested,
bytecode-accelerated access. Under the bytecode, cursor instructions would
fall back to the walker, making a list `for` slower than the index loop it
replaces.

## Bugs found on the way

- **C AOT tensor views are static.** `TensorIndexRef` at rank above 1 writes
  its view into `static _Alignas(8) uint8_t __view[...]`
  (`c-aot/src/codegen.rs`), one per instruction. A recursive call that
  reaches the same instruction overwrites the caller's live view. This
  predates `for`, but nested `for` over a tensor will lean on it. The view
  belongs in `__frame`, as the interpreter's frame extension and Cranelift's
  stack slot have it.
- **The DCE hazard** above.
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
- **A missing prototype.** C AOT emits
  `dtlv_rti_tensor_hyperplane_clone_local` with no `extern` prototype, so it
  compiles as an implicit declaration.

## What exists and what is missing

| Piece | Status |
|---|---|
| `ListElementRef`, `TensorIndexRef` in every backend, generic included | exists |
| Leaf chaining and `leftmost_leaf`, for sets and maps | exists |
| Descriptors for elements, keys and values in a generic | exist, as table entries |
| `ref` binding allocation (`alloc_const_param`) | exists |
| `for` in the parser, AST, tycheck, ownership and lowering | missing |
| Freezing the iteratee | missing |
| A continue target separate from the header | missing |
| `Len` | missing |
| `rtdt::TreeCursor`, its four entry points, `IrType::TreeCursor` and three instructions | missing |

## Phases

1. **Fix the DCE hazard and the C AOT static view** first. Both are small,
   and both would otherwise show up as `for` bugs.
2. **`for` over lists**, end to end: the front end, freezing, `Len`, the loop
   shape, `ListElementRef`. Every engine runs it with no new runtime. Engine
   fixtures for: empty, one element, `break` and `continue`, a body that never
   reads `item`, linear and copy elements, a generic `[T]`, nested loops,
   iterating a `ref` parameter, a field and an index projection, each freeze
   error.
3. **The indexed form for lists and tensors**, and **tensors**, rank 1 and
   above, including a transposed and a sliced one.
4. **Sets and maps**: the runtime cursor with `datalove-rt-tests` for one
   leaf, several leaves and levels, trees after removals have merged leaves,
   and bulk-built trees; then the IR and the four engines.
5. **Port `sys/std` and the demos.** `ord.contains`, `index_of`,
   `count_of`, `least`, `greatest`, `fixedint.sum`/`product`, `list.flattened`
   and `concat`, the six walks in `set.dfm` (where `is_subset_of` gains its
   early exit), the `keys()`-then-`get_or` loops in the demos, and
   `wordfreq.top_checksum`. Measure `benchvs/ordered.dfs` and `wordfreq` before
   and after.
6. **Inline tree advance within a leaf**, and a bytecode `Len` op, if
   measurement says so.

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
  values. The tree cursor's `value` is already a place; a set element and a
  map key never are, since changing one moves where it belongs.
- **Reverse walks.** Lists and tensors are a counter running down. Tree
  leaves link forward only, so a reverse tree walk needs back links or a
  stack. Nothing in the survey asks for it.
- **Tables.** A row walk binding fields of one row, per column, waits on the
  table design (`design-table-rows.md`).
