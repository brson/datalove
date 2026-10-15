---
title: "Iterating collections without first-class references"
category: design
summary: "Language-integrated iteration over collections faces compromises when you don't have reference types"
---




In my [previous round of benchmarking](../posts/20261009a-compound-assignment.md)
I noted that Datalove's system library iterates over collections by maintaining
a loop induction variable of type [`index`](../datafun.md#user-content-collections-and-indexing),
incrementing it with wrapping math,
and indexing into the collection on each iteration to access the element.
This is a problem of missing basic language features.
A simple solution is relatively straightforward
but is slightly complexified by the limitations of Datalove's simple type system.

- [The situation with loops in Datalove](#user-content-the-situation-with-loops-in-datalove)
- [A simple design for `for` loops](#user-content-a-simple-design-for-for-loops)
- [Getting at that loop induction variable](#user-content-getting-at-that-loop-induction-variable)
- [A protocol for iterating over collections](#user-content-a-protocol-for-iterating-over-collections)
- [What worked and what remains to be solved](#user-content-what-worked-and-what-remains-to-be-solved)




## The situation with loops in Datalove

Here's how `ord.contains` did it before this work:

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

fun equal_to_at<T>(
  ref self: [T], i: index, ref elem: T
): ?bool where { T is ord }
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
fun is_subset_of<T>(ref self: #{T}, ref other: #{T}): bool where { T is ord }
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
Those algorithmically abhorrent accessor APIs are waiting on
a way to iterate over collections before they can be retired.

The indexing is a performance hazard,
but also the wrapping math is a
cognitive/maintainability hazard:
every time I see wrapping math I must stop and think hard about the invariants.
This math could also be reasonably done as checked math with a debug assert,
but Datalove doesn't yet have any kind of assert,
and the wrapping is impossible to hit and will be eliminated shortly.

Both of these problems are solvable with language-level container iteration,
like Python's `for x in xs`.
A simple design for this is obvious,
so I'll go ahead and do it now!

We have some constraints:
Datalove has a linear type system where values are neither garbage-collected nor reference-counted.
They are always inline value types.
So copying can be expensive, but we want to iterate cheaply.
Datalove also [does not have first-class references](../datafun.md#user-content-borrows-are-second-class),
so in the surface language we can't take pointers directly into collections and treat them as values
that can be moved between bindings, returned from functions, etc.
That's going to heavily limit our expressivity.
But working within limitations is fun, so let's have fun.




<!-- the thesis -->

## A simple design for `for` loops

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
it probably makes sense to write the word `ref` somewhere in the loop header &mdash;
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
Sets bind a reference to the element,
maps to both the key and the value.

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
once every time through the loop.
Here the `ord.index_of` function
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
(which would mean they probably do the same wrapping math I want to avoid),
the iteration needs to provide the index value as well.

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
So for now this is a compromise I'll make so that `for` loops are useful in
the most common case.

Sets and maps won't support this form since they don't index by number.




## A protocol for iterating over collections

Datalove's collections' data layouts and APIs are defined in the runtime,
callable by both the compiler's emitted code and native code linked to Datalove packages.
So I need to have a clear ABI and API for iterating over collections.
In practice most backends won't actually call the runtime collections API for every operation &mdash;
iterating over a list by calling a native function at each step would be slow
when the compiler can just bump a pointer itself.
But I think having the API spelled out in the runtime makes it clear what the contract is,
and the reference IR interpreter does actually make those calls
for the fast backends to compare against.

Between array-like collections
(lists and rank-1 tensors)
and tree-like collections
(sets and maps)
we have two
fundamentally different methods of iteration:
array-like collections want to just bump a pointer against their buffer according to some fixed stride;
tree-like collections instead want to follow pointers between heap-allocated nodes.
Rank-2+ tensors walk row by row, with a third cursor type not detailed here.

We can represent these styles of iteration with two cursor types:

```rust
#[repr(C)]
pub struct StrideCursor {
    pub elem: *const u8,     // The current element.
    pub stride: usize,       // Bytes from one element to the next.
    pub index: Index,        // The current element's position; len is done.
    pub len: Index,          // How many elements the walk visits.
}

#[repr(C)]
pub struct TreeCursor {
    pub leaf: *const u8,     // The current leaf; null is done.
    pub elem: *const u8,     // The current key; a set's element.
    pub value: *const u8,    // A map's current value; null for a set.
    pub slot: u32,           // The current key's slot within the leaf.
}
```

Since these have to be part of a public API and ABI that the compiler knows about directly,
it's convenient to consolidate them instead of having a distinct type for each collection.
But we do need to give every collection type its own suite of functions,
all part of the runtime API:

```rust
// Stride cursors
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
fn dtlv_rti_stride_cursor_advance_local(
    rt: LocalRtHandle,
    cursor_mut: *mut StrideCursor,
) -> RtStatus;

// Tree cursors
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
```

Lots of the detail here is related to Datalove runtime API conventions:
every call gets a `LocalRtHandle` and returns an `RtStatus`,
return values are in out-pointers,
and the term "local" refers to the class of heap.
Some of the functions are collection-specific;
some are shared between cursor types.
The general shape and use of the cursors is:

- Call a collection-specific `begin` constructor.
- Ask whether iteration is done by checking whether the `index`
  field of the `StrideCursor` equals its `len`, or whether the `leaf` field
  of the `TreeCursor` is null.
  The cursors own nothing so don't need to be destructed.
- Read the element and/or value pointer off the cursor.
- If needed, read the iteration count off the `StrideCursor`'s `index`,
  so an indexed `for i, elem` needs no counter of its own.
- Call an `advance` function to move the cursor forward.

Datalove can't express an iterator in the language itself,
so the cursor protocol _is_ the abstraction,
and it lives in the runtime ABI instead of in something like a trait.
I love this for the compiler.
Because the compiler knows exactly what a cursor looks like,
every backend can open-code the hot parts &mdash;
the 'done' check, the element, and a strided advance are just loads and adds against the struct &mdash;
while the IR interpreter calls the real runtime functions
and gives the fast backends something to be checked against.
For hypothetical users, though, it's tough luck:
they can't write an iterator.




## What worked and what remains to be solved

After implementing the cursor protocol I was able
to convert 25 `loop while` loops to `for` loops &mdash;
15 in the system library,
7 in the "store demo" I use to prove features and performance,
and 3 in the benchmarks &mdash;
and eliminate all 25 of their explicit loop induction variables,
along with the wrapping increments that went with them.
The demo loops were a special kind of ugly:
having no way to walk a map,
they built a list of its keys with `map.keys`
and then did a full lookup for each key to get the value.
Now they just bind both:

```datalove
for category, r in ref revenue
    let c = map.get_or(ref cost, ref category, 0)
    let u = map.get_or(ref units, ref category, 0)
    call list.push(mut rows, (r, category@, c, u))
end for
```

Performance is about what you'd hope.
The benchmark sums a list of 100,000 `u32`s 200 times,
and a set of 5,000 elements 20 times
(release build, `hyperfine` means):

| | interp | JIT | AOT |
|---|---:|---:|---:|
| list, index loop | 230 ms | 38 ms | 18.1 ms |
| list, `for`, every cursor operation a runtime call | 487 ms | 155 ms | 52 ms |
| list, `for`, open-coded | 193 ms | 34 ms | 16.4 ms |
| set, `set.get` by position | 58 ms | 61 ms | 53 ms |
| set, `for` | 7.2 ms | 7.5 ms | 1.4 ms |

The middle row is why the backends open-code the cursors:
calling into the runtime for every step cost two to four times the old index loop.
Open-coded, a list's `for` beats its index loop on every engine,
though not by much,
since the index loop was already just a bounds check and an add.
The set is the real win:
linear instead of quadratic,
8x faster on the interpreter and 38x under AOT at only 5,000 elements.

Not every loop converted.
Eight functions in the system library still count by hand.

`repeated` builds a list `n` long and has nothing to walk.

```datalove
// A list of n clones of one element.
fun repeated<T>(ref elem: T, n: index): [T]
  var built: [T] = []
  call list_reserve(mut built, n)
  var i: index = : index / 0
  loop while i .< n
    call push(mut built, elem@)
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun
```

`take` and `skip` walk part of a list.
An indexed `for` with a `break` or `continue` could do it,
but what they really want is some kind of slice.

```datalove
// The first n elements, or all of them if there are fewer.
fun take<T>(ref self: [T], n: index): [T]
  var built: [T] = []
  var i: index = : index / 0
  loop while i .< n
    if get(ref self, i) |elem|
      call push(mut built, elem)
    else
      break
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret built
end fun
```


Some jump around
(the merge in `sorted`, `binary_search`),
which no cursor will help with.
`zip` walks two lists in lockstep,
which would need two cursors in one loop header.

And some are directly about holding first-class references.
`is_sorted` and `deduped` compare each element to the one before it,
and the obvious way to write that is to hang on to a reference to the previous element
across iterations.

```datalove
fun is_sorted<T>(ref self: [T]): bool where { T is ord }
  var ordered = true
  let n = list_len(ref self)
  var i: index = : index / 1
  loop while i .< n
    let i_prev = icall sub_wrapping_index(i, : index / 1)
    if greater_at(ref self, i_prev, i) |out_of_order|
      if out_of_order
        set ordered = false
        break
      end if
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret ordered
end fun
```

Datalove can't, so they keep an index
and read both elements back out of the list by position.
`least` and `greatest` did convert,
but they have the same problem in a different shape.
They can't hold on to the best element seen so far,
so they hold its index and re-index the list on every comparison:

```datalove
fun least<T>(ref self: [T]): ?T where { T is ord }
  var best: index = : index / 0
  for i, item in ref self
    if less(ref item, ref self[best]?)
      set best = i
    end if
  end for
  ret some self[best]?@
end fun
```

So the special-cased `for` that counts the index
provides a workaround here for not having references.
A pleasant consolation.
