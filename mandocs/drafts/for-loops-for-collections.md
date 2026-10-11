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
fun greatest<T>(ref self: [T]): ?T with { T is ord }
  var best: ?T = none
  let n = list_len(ref self)
  var i: index = : index / 0
  loop while i .< n
    set best = some max(self[i]?, ref held)
    set i = icall add_wrapping_index(i, : index / 1)
  end loop
  ret best
end fun
```

