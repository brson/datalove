## 2026/10/03 Revisit prefix op precedence

> Prefix operators bind tighter than postfix operators,
> so `-x?` is `(-x)?` and `not s.a` is `(not s).a`.
> The payload keywords `some`, `ok`, `er`, `data` and `error`
> are the exception, taking postfix operators onto their payload:
> `some x@` is `some (x@)`.

Seems confusing, especially `(not s).a`.


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

# 2026/10/01 There's a fixpoint algorithm to determine tydescs for empty collections

Understand the impact of this.

