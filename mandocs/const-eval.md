# Datalove Constant Evaluation

All datalove functions can be evaluated at compile time.

The `const` statement introduces a compile-time evaluated value,
initialized by a compile-time evaluated expression.
`const` is valid in scripts, script functions, modules, and module functions.

Expressions in `const` statements typecheck exactly
the same as `let` statements;
this is crucial to the mechanism used to implement const evaluation.
Two additional rules:
- `const` expressions may only reference bindings that are also `const`
- a const is borrowed wherever it is named, like a `ref` parameter;
  it may be read any number of times,
  and moving out of a const of a linear type takes a clone with `@`


## Basic usage

Simple literals and expressions:

```datalove
const X: u32 = 42
const Y: int = 15 + 27
let z = X@ + Y
```

Consts can reference other consts:

```datalove
const A: int = 10
const B: int = A@
const C: int = A + B
```

Consts may be of any type, not only scalars:

```datalove
const MSG: string = "hello"
const LST: [int] = [1, 2, 3]
let a = LST@
let b = LST@   // moving out of a const takes a clone
```

Moving out of a const without `@` is an error,
and the compiler says where the `@` goes:

```
[D003] Error: cannot move out of const: `LST`
```

`match`, the destructuring `if` and `let` destructuring
move what they take apart,
so a const of a linear type is taken apart as `match C@`.


Const works identically in script functions and module functions.

```datalove
fun get_value(): int
    const X: int = 100 + 200
    ret X@
end fun
```

A const parameter follows the same rule.
Passing it on as a const argument is not a move,
since a const argument is not passed at all.

`const` expressions can call functions.

```datalove
fun get_value(a: u32): u32
    if a .< 10
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
A module-level const has no enclosing function,
so it cannot use early-return operators at all.

```datalove
fun divide(): ?u32
    const A: u32 = 10
    const B: u32 = 0
    const C: u32 = A /? B    // compile error: const expression returned early
    ret some C
end fun
```
