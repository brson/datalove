# Design: Postfix Operators `$` (Clone) and `~` (Widen)

Status: Design complete, not implemented
Date: 2026-01-17

## Overview

Two new postfix operators for explicit value manipulation:
- `$` - clone a linear value
- `~` - widen a numeric type (lossless coercion)

Both occupy precedence level 3 alongside `?` and `!`.

## `$` Clone Operator

Creates a deep copy of a linear value. Original remains valid after cloning.

### Syntax

```
expr$
```

### Type Rules

| Operand type | Result |
|--------------|--------|
| Linear type  | Same type, cloned value |
| Copy type    | **Error**: cannot clone copy type |

Linear types: `int`, `string`, `[@T]`, `@map<K,V>`, `@set<T>`, `@[|T, N|]`, `{| ... |}`, `@data`

Copy types: `bool`, `u8`-`u64`, `i8`-`i64`, `f32`

### Synthesis

`$` synthesizes. Output type equals input type.

### Rationale for Copy Type Error

Copy types already duplicate implicitly. Explicit `$` on a copy type likely indicates programmer confusion about the type's semantics. Deny now; may relax for generic code later.

### Examples

```
let x: int = 42
let y = x$              // OK: clone linear type
let z = x               // OK: x still valid

let n: u32 = @1
let m = n$              // ERROR: cannot clone copy type u32
```

Loop usage (linear values cannot be moved in loops):

```
fun sum_n_times(val: int, n: u32): int
    var acc: int = 0
    var i: u32 = @0
    loop while i .< n
        set acc = acc + val$    // clone each iteration
        set i = i + @1
    end loop
    ret acc
end fun
```

## `~` Widen Operator

Lossless numeric coercion to a wider type in the same signedness family.

### Syntax

```
expr~
```

### Valid Widening Chains

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
```

### Type Rules

| Condition | Result |
|-----------|--------|
| Target wider than source, same sign | Widened value |
| No type context | **Error**: cannot infer coercion target |
| Target equals source | **Error**: coercion to same type |
| Cross sign boundary | **Error**: e.g., u8 -> i32 |

### Checking (Bidirectional)

`~` checks against expected type. Does not synthesize.

Target type propagates from:
- Variable declarations: `let x: u32 = y~`
- Function parameters: `foo(y~)` where param is wider type
- Binary operators: `a~ + b` where `+` has expected result type

Binary operators propagate expected type to both operands:

```
let sum: u32 = a~ + b~      // + propagates u32 to both ~ operators
```

### Examples

```
let a: u8 = @10
let b: u32 = a~             // OK: u8 -> u32

let c = a~                  // ERROR: no type context
let d: u8 = a~              // ERROR: same type
let e: i32 = a~             // ERROR: crosses sign boundary
```

Function parameter propagation:

```
fun process(n: u64): u64
    ret n
end fun

let small: u8 = @42
let result = process(small~)    // u8 -> u64 via param type
```

## Chaining

Postfix operators associate left-to-right.

Evaluation: left-to-right
Typechecking: right-to-left

### Valid Chains

```
let a: u8 = @10
let b: int = a~$        // widen u8 -> int, then clone int
```

### Invalid Chains

```
let a: u8 = @10
let b: int = a$~        // ERROR at $: cannot clone copy type u8
```

The type at each step determines validity:
1. `a` is `u8` (copy type)
2. `a$` - error, cannot clone copy type

## Precedence

| Level | Operators              |
|-------|------------------------|
| 1     | `()`                   |
| 2     | `-` `-?` `-!` `not`    |
| 3     | `?` `!` `$` `~`        |
| 4     | `*` `/` `*!` `/!` etc. |
| 5     | `+` `-` `+!` `-!` etc. |
| 6     | `.<` `.>` `<=` etc.    |
| 7     | `and`                  |
| 8     | `or` `xor`             |

## Implementation Notes

### Parser

Add `$` and `~` to postfix operator parsing at same precedence as `?` and `!`.

### Typechecker

`$`:
- Check operand is linear type
- Synthesize same type

`~`:
- Require expected type from context
- Verify source and target in same sign family
- Verify target strictly wider than source
- Check against target type

### Interpreter

`$`:
- Deep clone the value in the slot
- Both original and clone are valid

`~`:
- Convert value representation to wider type
- For fixed-to-fixed: zero-extend (unsigned) or sign-extend (signed)
- For fixed-to-int: construct bigint from fixed value

### AOT

`$`:
- Emit clone intrinsic or inline clone sequence

`~`:
- Emit appropriate extension instruction (zext/sext)
- Or bigint construction for -> int
