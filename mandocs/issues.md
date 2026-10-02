# There's a fixpoint algorithm to determine tydescs for empty collections

Understand the impact of this.

## 2026/01/02 Disallow else-if chains for destructuring

Seems confusing to me:

```datalove
let a = some 1
let b = ok 2

if a |x|
else if b |y|
else |e|
end if
```