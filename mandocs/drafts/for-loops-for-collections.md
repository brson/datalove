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
(and for which `equal_to_at` exists just to accomodate the error handling),
and the intrinsic call to `add_wrapping_index`.
The indexing is a performance hazard,
but maybe more importantly the wrapping math is a big
cognitive/maintainability hazard:
every time I see wrapping math I must stop and think hard about the invariants.

Both of these are solvable with language-level container iteration.
The design for this is fairly obvious and implementation easy,
so I'll go ahead and do it now.
Trying to clean up the code quality in the system library,
modest as it is.

I want to write that function like the following.

```datalove
fun contains<T>(ref self: [T], ref elem: T): bool with { T is ord }
  var found = false
  for item in self
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
So `item` can't be moved, nor can the iteratee, `self`
be mutably referenced or mutated via indexing.
This is in contrast to `match` and destructuring `if`
which fully destructure their operands.

Besides lists we also have other collections that need
iteration consideration: maps, sets, tensors, and tables.
