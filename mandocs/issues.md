## 2026/10/08 sys/std/int functions take args by val

seems like a lot of them should be ref


## 2026/10/08 runtime rtstatus results are mostly ignored

probably need to trigger a runtime abort and reset


## 2026/10/06 const arguments probably should require `const` annotation

like ref etc


## 2026/10/04 Decide how zsts are represented

There is no single convention yet:
the interpreter gives a zero-sized buffer a non-null address
equal to its alignment, never dereferenced or freed,
and an empty list's data pointer is null.


## 2026/10/03 All vars have tracking bytes

Only vars that aren't initialized in their declaration actually need them.
Others can by precisely analyzed like let bindings.


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
