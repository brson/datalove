# There's a fixpoint algorithm to determine tydescs for empty collections

Understand the impact of this.

## 2026/10/02 int.neg_checked is ! instead of ?

looks inconsistent

## 2026/10/02 term syntax footgun

```datalove
type X = term Foo term Bar int
```

easy to accidentally

```datalove
type X = enum {
  term Foo       // missing comma
  term Bar int
}
```

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