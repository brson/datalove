---
title: "`for` loops for collections"
category: dev
summary: "Yeah of course we need `for` loops"
---

In my [previous round of benchmarking](../posts/20261009A-compound-assignment.md)
I noted that the system library iterates over collections by maintaining
a loop induction variable of type `index`,
incrementing it with wrapping math,
and indexing into the collection on each iteration to access the element.

```datalove
fun contains<T>(ref self: [T], ref elem: T): bool with { T is ord }
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

fun equal_to_at<T>(ref self: [T], i: index, ref elem: T): ?bool with { T is ord }
  ret some equal(ref self[i]?, ref elem)
end fun
```

This has a number of deficiencies,
but for the purposes of the work at hand I care about the
`self[i]?`, which does a bounds check every iteration
(and for which `equal_to_at` exists just to accommodate the error handling),
and the intrinsic call to `add_wrapping_index`.
The indexing is a performance hazard,
but maybe more importantly the wrapping math is a big
cognitive/maintainability hazard:
every time I see wrapping math I must stop and think hard about the invariants.

Both of these are solvable with language-level container iteration.
The design for this is fairly obvious and implementation is mostly straightforward,
so I'll go ahead and do it now.
Trying to clean up the code quality in the system library,
modest as it is.

I want to write that function like the following.

```datalove
fun contains<T>(ref self: [T], ref elem: T): bool with { T is ord }
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

The `item` variable is a reference binding,
a pointer directly into the container.
So `item` can't be moved, nor can the iteratee, `self`,
be mutably referenced or mutated via indexing.
This is in contrast to `match` and destructuring `if`
which fully destructure their operands.

Even though I don't plan immediately to have by-value iteration,
it probably makes sense to write the word `ref` somewhere in the loop header -
I've got a pretty consistent explicit philosophy of writing `ref` everywhere
there's a reference binding.
Here I've put it next to the _collection_, not the binding,
since as we'll see below there may be many bindings in a loop header.
We'll see how it goes.

Besides lists we also have other collections that need
iteration consideration: maps, sets, tensors, and tables.

Sets are the easy one.
A set is a tree kept in the total order,
so `for` walks it in that order
and binds a reference to each element,
just as for a list.

```datalove
fun is_subset_of<T>(ref self: #{T}, ref other: #{T}): bool with { T is ord }
  var all_present = true
  for elem in ref self
    if not contains(ref other, ref elem)
      set all_present = false
      break
    end if
  end for
  ret all_present
end fun
```

The binding can never be mutable, even if lists eventually grow a mutable form of iteration:
changing an element in place could move where it belongs in the order.

Maps bind two references, the key and the value.

```datalove
for key, value in ref totals
  debuglog (key@, value@)
end for
```

Not a tuple: a `(K, V)` would be a new value built from the two,
not a place in the map,
and building it means cloning both.
Today `sys/std/map` exposes `key_at` and `value_at`
precisely so maps can be walked with an index like a list,
and each of those walks the tree to the position from the top.
A `for` walks it once.
As with sets, the key binding is immutable forever;
the value could someday be `mut`.

Tensors iterate along axis 0, the same axis `t[i]?` indexes.
At rank 1 the binding is a reference to an element.
At higher ranks it is a view of rank one less,
the same non-owning view `t[i]?` produces today,
with the same restrictions.
So walking every element of a matrix is two loops:

```datalove
for row in ref grid
  for cell in ref row
    set total = total + cell
  end for
end for
```

Not sure how to do tables yet.
The language needs additional features to do anything with them.
Iterating over the columns would be easy but doesn't seem like what one would want.
`for` could be a mechanism to iterate over
partial row projections while binding references to individual fields,
avoiding materializing a fresh struct.
This could be the basic tool needed for table transformations.

```datalove
let table = {|
  name, number, status
  "brian", 1, true
  "sid", 2, false
|}

for { name, number, .. } in ref table
  ...
end for
```


## Getting the index

Many of the collections iterations in the system library
also make use of the loop induction variable for more than just indexing the collection
once every time through the loop.

```datalove
fun index_of<T>(ref self: [T], ref elem: T): ?index with { T is ord }
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

So it's not enough to just make collections iterable.
Unless I want to force users to keep their own index counter in sync with the collection index
(which would mean they probably do the same wrapping math I want to avoid)
then the iteration needs to provide the index value as well.

Go and Zig and probably others do this with another form of `for` that adds another term to
the loop header to represent the index. So I guess I'll just do that too.

```datalove
fun index_of<T>(ref self: [T], ref elem: T): ?index with { T is ord }
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

I don't know if Datalove will ever grow an iterator protocol like Rust has.
Datalove isn't intended for that level of expressivity in its language integration.

For sets and maps I guess it doesn't make sense to support this
since they are not index by number.


## Implementation considerations

The big missing piece to make this all work is some kind of
collection cursors in the runtime.
Most of the compiler work is relatively straightforward extensions to what already exists.

I've sketched one cursor shape for all four collections.
It's a small plain-data struct:
a pointer to the current element
(plus one to the current value, for maps),
a position, and a count.
Each collection gets a "begin" and an "advance" function with the same signature,
and "done" is just comparing the position to the count,
which every backend can do inline.
The position doubles as the loop index, so the indexed form is free.

For lists and tensors the fast backends don't call the runtime at all:
advancing is a pointer bump.
Sets and maps are B+trees whose leaves are already linked in order,
so advancing steps along a leaf and hops to the next one,
instead of descending from the root for every element like `key_at` does.

The cursor is also what keeps the compiler side simple.
The thing that lives across the loop body is the cursor, which is just data.
Each use of `item` makes a fresh reference from it,
the same kind of short-lived reference that indexing makes today.