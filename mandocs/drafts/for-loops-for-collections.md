---
title: "`for` loops without first-class references"
category: design
summary: "Language-integrated iteration over collections faces compromises when you don't have reference types"
---

<!--

Audience: programming language hackers.
Problem: Language-integrated iteration over collections faces compromises
when you don't have reference types.

-->



<!-- the problem -->

In my [previous round of benchmarking](../posts/20261009a-compound-assignment.md)
I noted that Datalove's system library iterates over collections by maintaining
a loop induction variable of type [`index`](../datafun.md#user-content-collections-and-indexing),
incrementing it with wrapping math,
and indexing into the collection on each iteration to access the element,
and that this is a problem to be solved soon.
A simple solution is relatively straightforword
but is slightly complexified by the limitations of Datalove's simple type system.

Here's how it has been done prior to this work in `list.contains`:

```datalove
fun contains<T>(ref self: [T], ref elem: T): bool where { T is ord }
  var found = false
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if equal_to_at(ref self, i, ref elem) |same|
      if same
        set found = true
        break
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret found
end fun

fun equal_to_at<T>(ref self: [T], i: index, ref elem: T): ?bool where { T is ord }
  ret some equal(ref self[i]?, ref elem)
end fun
```

This has a number of deficiencies,
but for the purposes of the work at hand I care about the
`self[i]?`, which does a bounds check every iteration
(and for which `equal_to_at` exists just to accommodate the
[early-return error handling indicated by `?`](../datafun.md#user-content-option-and-result-handling)),
and the intrinsic call to `add_wrapping_index`.

The indexing hazard gets _much worse_ when we look at how sets and maps are iterated.
Here's `set.is_subset_of`:

```datalove
fun is_subset_of<T>(ref self: #{T}, ref other: #{T}): bool where { T is ord, }
  var all_present: bool = true
  let n = len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      if contains(ref other, ref elem)
      else
        set all_present = false
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret all_present
end fun
```

`get(ref self, i)` is the element at position `i` in sort order.
A set is a B+tree,
so there's no jumping straight to position `i`:
each call starts at the leftmost leaf,
walks the chain of leaves until it has passed `i` elements,
and clones the element it lands on.
So it's no longer just a bounds check and some pointer math per element.
Reaching element `i` costs O(i),
which makes a loop over the whole set O(n²).
Maps are the same, through `map.key_at` and `map.value_at`.
Those algorithmically-abhorrent accessor APIs are waiting on
some way to iterate over collections before they can be retired.

The indexing is a performance hazard,
but also the wrapping math is a
cognitive/maintainability hazard:
every time I see wrapping math I must stop and think hard about the invariants.

Both of these are solvable with language-level container iteration,
like Python's `for x in xs` or Rust's `for x in &xs`.
A simple design for this is obvious
so I'll go ahead and do it now!

We have some constraints:
Datalove has a linear type system where values are neither garbage-collected nor reference-counted,
but are inline value types.
So copying can be expensive, but we want to iterate cheaply.
Datalove also [does not have first-class references](../datafun.md#user-content-borrows-are-second-class),
so in the surface language we can't take pointers directly into collections and treat them as values
that can be moved between bindings, returned from functions, etc.
That's going to heavily limit our expressivity.
But working within limitations is fun, so let's have fun.




<!-- the thesis -->

## A simple design of `for` loops

I want to write the previous `contains` function like the following.

```datalove
fun contains<T>(ref self: [T], ref elem: T): bool where { T is ord }
  var found = false
  for item in ref self
    if equal(ref item, ref elem)
      set found = true
      break
    end if
  end for
  ret found
end fun
```

The `item` variable is a [reference (`ref`) binding](../datafun.md#user-content-functions),
a pointer directly into the container.
So `item` can't be moved, nor can the iteratee, `self`,
be mutably referenced or mutated via indexing.
This is in contrast to [`match` and destructuring `if`](../datafun.md#user-content-data-types-and-destructuring),
which fully destructure their operands.

Even though I don't plan immediately to have by-value iteration,
it probably makes sense to write the word `ref` somewhere in the loop header -
I've got a mostly consistent explicit philosophy of writing `ref` everywhere
there is a reference binding.
Here I've put it next to the _collection_, not the binding,
since as we'll see below there may be two bindings in a loop header
and just now I don't want to think about distinguishing the case where one is by-val and one is by-ref.
We'll see how that feels over time.
Seems like something I'll have to revisit.

Besides lists we also have other collections that need
iteration consideration: maps, sets, tensors, and tables.

Maps and sets are trees kept in the total order,
so `for` walks them in that order.
Sets bind a reference to the key,
maps to both the key and value.

```datalove
fun top_checksum(ref counts: %{string = u32}): !u32
  var ranked: [(u32, string)] = []
  for word, seen in ref counts
    call list.push(mut ranked, (u32.max_value() -! seen, word@))
  end for
  // ...
```

_The bindings are not a tuple_:
a `(K, V)` would be a new value built from the two,
not a place in the map.
And Datalove doesn't have first-class references that could be shoved into a tuple,
so if we built a tuple the elements would need to be cloned.
As with lists, the key binding is immutable;
the value could someday be `mut`.

[Tensors and tables](../datalit.md#user-content-collections) both add their own complications here,
but are generally extensions in the same solution space.




<!-- but ... -->

## Getting at that loop induction variable

Many of the collection iterations in the system library
also make use of the loop induction variable for more than just indexing the collection
once every time through the loop. Here the `list.index_of` function
needs to hold onto that index and return it:

```datalove
fun index_of<T>(ref self: [T], ref elem: T): ?index where { T is ord }
  var at: ?index = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    if equal(ref self[i]?, ref elem)
      set at = some i
      break
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret at
end fun
```

So it's not enough to just make collections iterable:
unless I want to force users to keep their own index counter in sync with the collection index
(which would mean they probably do the same wrapping math I want to avoid)
then the iteration needs to provide the index value as well.

Go and Zig and probably others do this with another form of `for` that adds another term to
the loop header to represent the index. So I guess I'll do that too!

```datalove
fun index_of<T>(ref self: [T], ref elem: T): ?index where { T is ord }
  var at: ?index = none
  for i, item in ref self
    if equal(ref item, ref elem)
      set at = some i
      break
    end if
  end for
  ret at
end fun
```

Personally **this smells a bit to me, smells like a big special case**,
and I'm used to Rust with its many iterator adapters that
can do this and much more (what about reverse iteration? seems important...).
I don't know if Datalove will grow an iterator protocol like Rust has.
I don't know if Datalove will ever get first-class reference types that could be stored in an iterator struct.
I don't intend Datalove to have a high level of expressivity in its language integration.
So for now this is a compromise I have to make to make `for` loops useful in
the most common case.

Sets and maps won't support this form since they don't index by number.




<!-- the payoff? -->

## How to implement iteration over collections

Datalove's collection's data layouts and APIs are defined in the runtime,
callable by both the compiler's emitted code and native code linked to Datalove packages.
So I need to have a clear ABI and API for iterating over collections.
In practice most backends won't actually call the runtime collections API for every operation &mdash;
iterating over a list by calling a native function at each step would be slow.
I think having the API spelled out in the runtime makes it clear what the contract is,
and I'll likely have the reference IR interpreter actually make those calls
for the fast backends to compare against.

Between array-like collections
(lists and and rank-1 tensors)
and tree-like collections
(sets and maps)
we have two
fundamentally methods of iteration:
array-like collections want to just bump a pointer against their buffer according to some fixed stride;
tree-like collections instead want to follow pointers between heap-allocated nodes.
Rank-2+ tensors have some additional considerations not detailed here.

We can represent these styles of iteration with two cursor types:

```rust
#[repr(C)]
pub struct StrideCursor {
    pub elem: *const u8,     // The current element.
    pub stride: usize,       // Bytes from one element to the next.
    pub remaining: Index,    // Elements from this one to the end; 0 is done.
}

#[repr(C)]
pub struct TreeCursor {
    pub leaf: *const u8,     // The current leaf; null is done.
    pub elem: *const u8,     // The current key; a set's element.
    pub value: *const u8,    // A map's current value; null for a set.
    pub slot: u32,           // The current key's slot within the leaf.
}
```

Since these have to part of a public API and ABI that the compiler knows about directly
its convenient to consolidate them instead of having a distinct type for each collection.
But we do need to give every collection type its own suite of functions,
all part of the runtime API:

```rust
fn dtlv_rti_list_cursor_begin_local(
    rt: LocalRtHandle,
    list_ref: *const u8,
    list_tydesc: *const TyDesc,
    cursor_out: *mut StrideCursor,
) -> RtStatus;
fn dtlv_rti_tensor_cursor_begin_local(
    rt: LocalRtHandle,
    tensor_ref: *const u8,
    tensor_tydesc: *const TyDesc,
    cursor_out: *mut StrideCursor,
) -> RtStatus;
fn dtlv_rti_stride_cursor_done_local(
    rt: LocalRtHandle,
    cursor_ref: *const StrideCursor,
    done_out: *mut u8,
) -> RtStatus;
fn dtlv_rti_stride_cursor_elem_local(
    rt: LocalRtHandle,
    cursor_ref: *const StrideCursor,
    elem_out: *mut *const u8,
) -> RtStatus;
fn dtlv_rti_stride_cursor_advance_local(
    rt: LocalRtHandle,
    cursor_mut: *mut StrideCursor,
) -> RtStatus;

fn dtlv_rti_btreeset_cursor_begin_local(
    rt: LocalRtHandle,
    set_ref: *const u8,
    set_tydesc: *const TyDesc,
    cursor_out: *mut TreeCursor,
) -> RtStatus;
fn dtlv_rti_btreemap_cursor_begin_local(
    rt: LocalRtHandle,
    map_ref: *const u8,
    map_tydesc: *const TyDesc,
    cursor_out: *mut TreeCursor,
) -> RtStatus;
fn dtlv_rti_btreeset_cursor_advance_local(
    rt: LocalRtHandle,
    cursor_mut: *mut TreeCursor,
    set_tydesc: *const TyDesc,
) -> RtStatus;
fn dtlv_rti_btreemap_cursor_advance_local(
    rt: LocalRtHandle,
    cursor_mut: *mut TreeCursor,
    map_tydesc: *const TyDesc,
) -> RtStatus;
fn dtlv_rti_tree_cursor_done_local(
    rt: LocalRtHandle,
    cursor_ref: *const TreeCursor,
    done_out: *mut u8,
) -> RtStatus;
fn dtlv_rti_tree_cursor_elem_local(
    rt: LocalRtHandle,
    cursor_ref: *const TreeCursor,
    elem_out: *mut *const u8,
) -> RtStatus;
fn dtlv_rti_tree_cursor_value_local(
    rt: LocalRtHandle,
    cursor_ref: *const TreeCursor,
    value_out: *mut *const u8,
) -> RtStatus;
```

There are pros and cons of Datalove not having any capability to express abstractions such as iterators itself.
Pro: we don't have to come up with a single abstraction facility that can express iteration over all types of collections;
Con: we do have to define explicit runtime APIs for the collections we do support and commit to them.






<!-- the results -->

<!--

don't expect much problems here.
maybe number of loop conversions that were able to remove the explicit variable.
some perf numbers for the set walk
cases where there's still wrapping addition related to loops or the for loop wasn't sufficient.
maybe show whether cranelift can lift the bounds check without the for loop.
open qs: reverse, zip, etc, by-value iters

-->