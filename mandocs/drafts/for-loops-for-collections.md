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

