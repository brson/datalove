## 2026/10/06 const arguments probably should require `const` annotation

like ref etc

## 2026/10/05 table type synthesis

datalit doesn't synthesize table types but dafun does?


## 2026/10/04 Decide how zsts are represented

There is no single convention yet:
the interpreter gives a zero-sized buffer a non-null address
equal to its alignment, never dereferenced or freed,
and an empty list's data pointer is null.


## 2026/10/03 All vars have tracking bytes

Only vars that aren't initialized in their declaration actually need them.
Others can by precisely analyzed like let bindings.


## 2026/10/03 const evaluation caching

Runs in the interpreter so not memoized.
Can we cache results manually?


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

# 2026/10/01 There's a fixpoint algorithm to determine tydescs for empty collections

Understand the impact of this.

