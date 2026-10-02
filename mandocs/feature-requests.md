## 2026/10/02 Bare binops that produce optionals

Would be nice:

```datalove
fun do_some_math(a: u32, b: u32, c: u32): ?u32
  ret (a + b) / c
end fun
```

## 2026/10/02 Compare and eq for more types

```datalove
if some 1 == some 2
end if
```

Semantics for aggregates get complex.


## 2026/10/01 Call by module-qualified name

```datalove
require module sys/std/int

var counter = 10
if int.rem_checked(counter, 2) == some 0
end if
```