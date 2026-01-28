# Datalove Constant Evaluation

All datalove functions can be evaluated at compile time.

The `const` statement introduces a compile-time evaluated value,
initialized by a compile-time evaluated expression.
`const` is valid in scripts, script functions, modules, and module functions.

Expressions in `const` statements typecheck exactly
the same as `let` statements;
this is crucial to the mechanism used to implement const evaluation.
Two additional restrictions:
- `const` expressions may only reference bindings that are also `const`
- `const` expressions may not move




## Basic usage

Simple literals and expressions:

```datalove
const X: u32 = 42
const Y: i64 = 15 + 27
let z = X + Y
```

Consts can reference other consts:

```datalove
const A: i64 = 10
const B: i64 = A
const C: i64 = A + B
```


Const works identically in script functions and module functions.

```datalove
fun get_value(): int
    const X: int = 100 + 200
    ret X
end fun
```

`const` expressions can call functions.

```datalove
fun get_value(a: u32): u32
    if a < 10
      ret 2
    else
      ret 3
    end if
end fun

fun compute(): u32
    const X: u32 = get_value(2)
    ret X
end fun
```

Early-return operators can be constant-evaluated,
but if they early-return they will cause a compile failure.

```datalove
fun divide(): ?u32
    const A: u32 = 10
    const B: u32 = 0
    const C: u32 = A /? B    // compile error: division by zero
    ret some C
end fun
```




