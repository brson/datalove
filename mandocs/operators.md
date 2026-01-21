# Datalove Operators - Arithmetic, Logical, Comparison




## Operator precedence

| Level | Operators                      | Description    |
|-------|--------------------------------|----------------|
| 1     | `()`                           | Grouping       |
| 2     | `-` `-?` `-!` `not`            | Unary prefix   |
| 3     | `?` `!` `$` `~`                | Postfix        |
| 4     | `*` `/` `*!` `/!` `*?` `/?`    | Multiplicative |
| 5     | `+` `-` `+!` `-!` `+?` `-?`    | Additive       |
| 6     | `.<` `.>` `<=` `>=` `==` `!=`  | Comparison     |
| 7     | `and`                          | Logical AND    |
| 8     | `or` `xor`                     | Logical OR/XOR |




## 2026-01-07 - Logic operators

Booleans support `and`, `or`, `xor`, and `not`.
todo say more




## Comparison operators



## Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.





## 2026/01/21 - Postifix operator `@` - lossless clone or coerce

A single operator for making expression types "fit" their destination.
In the spirit of balancing correctness with scripting ergonomics.




## 2026/01/17 - Postifix operator `$` - clone

Creates a deep copy of a linear value. The original remains valid after cloning.

```
let x: int = 42
let y = x$          // clone x
let z = x           // x still valid
```

Only valid on linear types.
Errors on copy types.
Synthesizes the same type as its operand.

```
// Loop usage
fun sum_n_times(val: int, n: u32): int
    var acc: int = 0
    var i: u32 = @0
    loop while i .< n
        set acc = acc + val$  // clone each iteration
        set i = i + @1
    end loop
    ret acc
end fun

// Multiple consumption
fun consume_both(a: int, b: int): int
    ret a + b
end fun

let x: int = 100
let result = consume_both(x$, x)  // clone for first, move for second
```




### 2026/01/17 - Postfix operator `~` - widen / lossless coercion

Widens a numeric type to a larger type in the same signedness family.

```
let a: u8 = @10
let b: u32 = a~     // widen u8 to u32
```

Valid widening chains:

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
```

Target type inferred from context.
Error if no type context available.
Error if target is same type.
Error if crossing sign boundary (u8 -> i32 not allowed).

Binary operators propagate expected type to `~` operands:

```
let a: u8 = @100
let b: u8 = @50
let sum: u32 = a~ + b~    // + propagates u32 to both sides

let bytes: u32 = @4096
let limit: u64 = @1000000
if bytes~ .< limit        // .< propagates u64 to left side
    // ...
end if
```

Function parameters propagate their types:

```
fun lerp(a: u64, b: u64, t: u64): u64
    ret a + (b - a) * t / @100
end fun

let lo: u8 = @0
let hi: u8 = @255
let pct: u16 = @50
let mid = lerp(lo~, hi~, pct~)  // each ~ gets u64 from param type
```

Errors:

```
let x: u8 = @10
let y = x~          // ERROR: no type context
let z: u8 = x~      // ERROR: same type, not widening
let w: i32 = x~     // ERROR: crosses sign boundary
```

### Chaining Postfix Operators

Postfix operators evaluate left-to-right, typecheck right-to-left.

```
let a: u8 = @10
let b: int = a~$    // widen u8 to int, then clone (int is linear)
```

Order matters based on types at each step:
- `a~$` on u8: widen to int (linear), then clone - valid
- `a$~` on u8: clone u8 (copy type) - error at `$`

