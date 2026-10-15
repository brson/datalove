---
title: "Iterating collections without first-class references"
category: design
summary: "Language-integrated iteration over collections faces compromises when you don't have reference types"
---




In my [previous round of benchmarking](../posts/20261009a-compound-assignment.md)
I noted that Datalove's system library walks collections in an awkward way.
It keeps a loop induction variable of type [`index`](../datafun.md#user-content-collections-and-indexing),
increments it with wrapping math,
and indexes into the collection on each pass to get the element.
Datalove is missing a basic language feature.
The fix is simple,
but the limits of Datalove's simple type system complexify it a bit.

- [The situation with loops in Datalove](#user-content-the-situation-with-loops-in-datalove)
- [A simple design for `for` loops](#user-content-a-simple-design-for-for-loops)
- [Getting at that loop induction variable](#user-content-getting-at-that-loop-induction-variable)
- [A protocol for iterating over collections](#user-content-a-protocol-for-iterating-over-collections)
- [What worked and what remains to be solved](#user-content-what-worked-and-what-remains-to-be-solved)




## The situation with loops in Datalove

Here's how `ord.contains` worked before:

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

This code has many problems,
but here I care about two.
The first is `self[i]?`, which does a bounds check every iteration.
(`equal_to_at` exists only to make room for the
[early return that `?` does](../datafun.md#user-content-option-and-result-handling).)
The second is the intrinsic call to `add_wrapping_index`.

Indexing gets _much worse_ for sets and maps.
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

`get(ref self, i)` returns the element at position `i` in sort order.
A set is a B+tree,
so it can't jump straight to position `i`.
Each call starts at the leftmost leaf,
walks the chain of leaves past `i` elements,
and clones the element it lands on.
That's far more than a bounds check and some pointer math.
Reaching element `i` costs O(i),
so a loop over the whole set costs O(n²).
Maps have the same problem with `map.key_at` and `map.value_at`.
These algorithmically abhorrent APIs can't be retired
until there's another way to iterate.

The indexing hurts performance.
The wrapping math hurts maintenance.
Every time I see wrapping math I have to stop and think hard about the invariants.
Checked math with a debug assert would also work,
but Datalove doesn't have asserts yet.
Anyway, the wrap can't happen, and it's going away soon.

Language-level iteration solves both problems,
like Python's `for x in xs`.
The simple design is obvious,
so I'll just do it now!

There are some constraints.
Datalove has a linear type system.
Values are neither garbage-collected nor reference-counted.
They are always inline value types.
So copying can be expensive, but iteration should be cheap.
Datalove also [has no first-class references](../datafun.md#user-content-borrows-are-second-class).
The surface language can't take a pointer into a collection and treat it as a value,
to move between bindings or return from a function.
That will limit what we can express.
But working within limits is fun, so let's have fun.




<!-- the thesis -->

## A simple design for `for` loops

I want to write `contains` like this:

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

`item` is a [reference (`ref`) binding](../datafun.md#user-content-functions),
a pointer straight into the container.
So `item` can't be moved,
and `self` can't be mutably referenced or mutated by index.
This differs from [`match` and destructuring `if`](../datafun.md#user-content-data-types-and-destructuring),
which fully destructure their operands.

I don't plan to add by-value iteration soon.
Still, the loop header should probably say `ref` somewhere.
Datalove generally requires the word `ref` everywhere there's a reference binding.
Here I put it next to the _collection_, not the binding.
As we'll see, a loop header can have two bindings,
and I don't want to think yet about one being by-value and the other by-ref.
I may have to revisit this.

Lists aren't the only collections that need iteration.
There are also maps, sets, tensors, and tables.

Maps and sets are trees kept in total order,
so `for` walks them in that order.
A set binds a reference to each element.
A map binds references to the key and the value.

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
Datalove has no first-class references to put in a tuple,
so building one would mean cloning both elements.
As with lists, the key binding is immutable.
The value could someday be `mut` to allow in-place mutation.

[Tensors and tables](../datalit.md#user-content-collections) each add their own complications,
but they mostly extend the same design.




<!-- but ... -->

## Getting at that loop induction variable

Many loops in the system library use the induction variable
for more than indexing the collection.
`ord.index_of` needs to keep the index and return it:

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

So iterating over the collection elements isn't enough.
Without the index, users would keep their own counter in sync,
and they'd likely repeat the wrapping math I want to remove.
The loop needs to provide the index too.

Go, Zig, and probably others add a second name to the loop header for the index.
I'll do that too!

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

**This [smells](https://en.wikipedia.org/wiki/Code_smell)
a bit to me. Smells like a big special case.**
I'm used to Rust and its many iterator adapters,
which do this and much more.
(What about reverse iteration? Seems important...)
I don't know if Datalove will grow an iterator protocol like Rust's.
I don't know if it will ever get first-class references to store in an iterator struct.
I don't intend Datalove's language integration to be very expressive.
So for now I'll accept this compromise
to make `for` useful in the most common case.

Sets and maps don't support this form, since they aren't indexed by number.




## A protocol for iterating over collections

Datalove's runtime defines the data layouts and APIs of its collections.
Both compiled code and native code link to Datalove packages call them.
So iteration needs a clear ABI and API.
In practice most backends won't call the runtime for every step.
Calling a native function per element would be slow
when the compiler can just bump a pointer.
But spelling the API out in the runtime makes the contract clear.
And the reference IR interpreter does make those calls,
which gives the fast backends something to compare against.

Array-like collections
(lists and rank-1 tensors)
and tree-like collections
(sets and maps)
iterate in two fundamentally different ways.
Array-like collections bump a pointer through their buffer by a fixed stride.
Tree-like collections follow pointers between heap-allocated nodes.
Rank-2+ tensors walk row by row, with a third cursor type I won't cover here.

Two cursor types represent these two styles:

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

These are part of a public API and ABI that the compiler knows directly,
so it's convenient to share them rather than give each collection its own type.
But each collection still needs its own functions in the runtime API:

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

Much of this detail comes from Datalove's runtime API conventions.
Every call takes a `LocalRtHandle` and returns an `RtStatus`.
Results go in out-pointers.
"Local" refers to the class of heap.
Some functions are specific to one collection,
and some are shared by every collection with that cursor type.
Using a cursor goes like this:

- Call the collection's `begin` function.
- Check whether iteration is done.
  A `StrideCursor` is done when `index` equals `len`.
  A `TreeCursor` is done when `leaf` is null.
- Read the element pointer off the cursor, and for a map the value pointer.
- For an indexed `for i, elem`, read `index` off the `StrideCursor`.
  The loop needs no counter of its own.
- Call an `advance` function to move forward.

The cursors own nothing, so they need no destructor.

Datalove can't express an iterator in the language,
so the cursor protocol _is_ the abstraction.
It lives in the runtime ABI, not in something like a trait.
I love this for the compiler.
Every backend knows exactly what a cursor looks like,
so each can open-code the hot parts.
The done check, the element read, and a strided advance
are just loads and adds against the struct.
Meanwhile the IR interpreter calls the real runtime functions,
which gives the fast backends something to check against.
For hypothetical users, though, it's tough luck.
They can't write an iterator.




## What worked and what remains to be solved

With the cursor protocol in place,
I converted 25 `loop while` loops to `for` loops.
Fifteen were in the system library,
seven in the "store demo" I use to prove features and performance,
and three in the benchmarks.
All 25 explicit induction variables are gone,
along with their wrapping increments.

The demo loops were a special kind of ugly.
With no way to walk a map,
they built a list of its keys with `map.keys`,
then did a full lookup per key to get the value.
Now they just bind both:

```datalove
for category, r in ref revenue
    let c = map.get_or(ref cost, ref category, 0)
    let u = map.get_or(ref units, ref category, 0)
    call list.push(mut rows, (r, category@, c, u))
end for
```

Performance is as expected.
This benchmark sums a list of 100,000 `u32`s 200 times
and a set of 5,000 elements 20 times.
These are `hyperfine` means for a release build.

| | interp | JIT | AOT |
|---|---:|---:|---:|
| list, index loop | 230 ms | 38 ms | 18.1 ms |
| list, `for`, every cursor operation a runtime call | 487 ms | 155 ms | 52 ms |
| list, `for`, open-coded | 193 ms | 34 ms | 16.4 ms |
| set, `set.get` by position | 58 ms | 61 ms | 53 ms |
| set, `for` | 7.2 ms | 7.5 ms | 1.4 ms |

The middle row is why the backends open-code the cursors.
Calling into the runtime for every step cost two to four times as much as the old index loop.
Open-coded, a list's `for` beats its index loop on every engine,
though not by much.
The index loop was already just a bounds check and an add.
The set is the real win:
linear instead of quadratic,
8x faster on the interpreter and 38x under AOT, at only 5,000 elements.

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
but what they really want is a slice.

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

Some functions jump around,
like the merge in `sorted` and `binary_search`.
No cursor helps there.
`zip` walks two lists in lockstep,
which would need two cursors in one loop header.

And some come down to first-class references.
`is_sorted` and `deduped` compare each element to the one before it.
The obvious way to write that holds a reference to the previous element
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

Datalove can't do that,
so these functions keep an index
and read both elements back out by position.
`least` and `greatest` did convert,
but they have the same problem in another shape.
They can't hold on to the best element so far,
so they keep its index and re-index the list on every comparison:

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
doubles as a workaround for missing references.
A pleasant consolation.
