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

This has a number of deficiencies,
but for the purposes of the work at hand I care about the
`self[i]?`, which does a bounds check every iteration,
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
fun index_of<T>(ref self: [T], ref elem: T): ?index with { T is ord }
  var at: ?index = none
  let n = list_len(ref self)
  var i: index = : index / 0
  for item in self
    if equal(ref item, ref elem)
      set at = some i
      break
    end if
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret at
end fun
```
