---
title: "`for` loops without first-class references"
category: dev
summary: "Language-integrated iteration over collections faces compromises when you don't have reference types"
---

<!--

Audience: programming language hackers.
Problem: Language-integrated iteration over collections faces compromises
when you don't have reference types.

-->



<!-- the problem -->

In my [previous round of benchmarking](../posts/20261009A-compound-assignment.md)
I noted that Datalove's system library iterates over collections by maintaining
a loop induction variable of type [`index`](../datafun.md#user-content-collections-and-indexing),
incrementing it with wrapping math,
and indexing into the collection on each iteration to access the element,
and that this is a problem to be solved soon.

Here's how it's done today:

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
(and for which `equal_to_at` exists just to accommodate the
[early-return error handling indicated by `?`](../datafun.md#user-content-option-and-result-handling)),
and the intrinsic call to `add_wrapping_index`.

The indexing hazard gets _much worse_ when we look at how sets and maps are iterated today:

```datalove
todo
```

No longer just a bounds check and some pointer math,
each of those is a full B-tree lookup,
which makes a loop over the full collection O(n log n).

The indexing is a performance hazard,
but also the wrapping math is a
cognitive/maintainability hazard:
every time I see wrapping math I must stop and think hard about the invariants.

Both of these are solvable with language-level container iteration,
like Python's `for x in xs` or Rust's `for x in &xs`.
A simple design for this is obvious
so I'll go ahead and do it now!

Two properties of the language that are going
to impact our design are that Datalove has a linear type system,
and that [Datalove does not have first-class references](../datafun.md#user-content-borrows-are-second-class).
The latter is going to heavily limit our expressivity.




<!-- the thesis -->

## A simple design of `for` loops

I want to write the previous `contains` function like the following.

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
since as we'll see below there may be many bindings in a loop header.
We'll see how that feels over time.

Besides lists we also have other collections that need
iteration consideration: maps, sets, tensors, and tables.

Maps and sets are trees kept in the total order,
so `for` walks them in that order.
Sets bind a reference to the key,
maps to both the key and value.

```datalove
todo map example
```

The bindings are not a tuple:
a `(K, V)` would be a new value built from the two,
not a place in the map.
And Datalove doesn't have first-class references that could be shoved into a tuple
so to build one means cloning both elements.
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

So it's not enough to just make collections iterable:
unless I want to force users to keep their own index counter in sync with the collection index
(which would mean they probably do the same wrapping math I want to avoid)
then the iteration needs to provide the index value as well.

Go and Zig and probably others do this with another form of `for` that adds another term to
the loop header to represent the index. So I guess I'll just do that too!

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

Personally **this smells a bit to me, smells like a big special case**,
and I'm used to Rust with its many iterator adapters that
can do this and much more (what about reverse iteration? seems important...).
I don't know if Datalove will grow an iterator protocol like Rust has.
I don't intend Datalove to have a high level of expressivity in its language integration.
So for now this is a compromise I have to make to make `for` loops useful in
the most common case.

Sets and maps won't support this form since they don't index by number.




<!-- is this the payoff? -->

## An efficient collection cursor

Implementing this is a significant piece of work,
but most of the compiler bits are relatively straightforward extensions to what already exists.
The big missing piece to make this all work is
collection cursors in the runtime.
All the existing accessors are per-element.

Our design needs to solve the performance problems we identified earlier:
todo
Also want to be able to short-circuit the runtime calls completely when
iterating over lists.

I ended up on one cursor shape for all collections.
It's a small plain-data struct:
a pointer to the current element,
a position, and a count
(plus additional fields for sets and maps).

```rust
todo
```

todo rewrite this to be the payoff

The position doubles as the loop index, so the indexed form of `for` is free.
Each collection gets a "begin" and an "advance" function with the same signature,
and "done" is just comparing the position to the count,
which every backend can do inline.

```
dtlv_rti_list_cursor_begin_local(rt, coll_ref, coll_tydesc, cursor_out, scratch) -> RtStatus
dtlv_rti_list_cursor_advance_local(rt, cursor, coll_tydesc) -> RtStatus
```

For lists and tensors the fast backends don't call the runtime at all:
advancing is a pointer bump.
Sets and maps are B+trees whose leaves are already linked in order,
so advancing steps along a leaf and hops to the next one,
instead of descending from the root for every element like `key_at` does.

The compiler knows the layout of `Cursor`,
which provides a simple protocol for the compiler to codegen against.




<!-- the results -->

<!--

don't expect much problems here.
maybe number of loop conversions that were able to remove the explicit variable.
some perf numbers for the set walk
cases where there's still wrapping addition related to loops or the for loop wasn't sufficient.
maybe show whether cranelift can lift the bounds check without the for loop.

-->