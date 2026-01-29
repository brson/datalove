# Datalove Operators - `? ! @` etc.




## Operator precedence

| Level | Operators                      | Description    |
|-------|--------------------------------|----------------|
| 1     | `()`                           | Grouping       |
| 2     | `-` `-?` `-!` `not`            | Unary prefix   |
| 3     | `?` `!` `$` `~` `@`            | Postfix        |
| 4     | `*` `/` `*!` `/!` `*?` `/?`    | Multiplicative |
| 5     | `+` `-` `+!` `-!` `+?` `-?`    | Additive       |
| 6     | `.<` `.>` `<=` `>=` `==` `!=`  | Comparison     |
| 7     | `and`                          | Logical AND    |
| 8     | `or` `xor`                     | Logical OR/XOR |




## Logic operators

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




## Postfix operator `@` - lossless clone and coerce

A single operator for making expression types "fit" their destination.
Balances correctness with scripting ergonomics.

Performs whatever lossless conversion is needed:
- **Widen** fixed integers along their signedness chain
- **Clone** linear types so the original remains valid
- **Both** when widening produces a linear type

Target type inferred from context (assignment, parameter, binary op).

### Clone (linear types)

```
let x: int = 42
let y = x@          // clone x
let z = x           // x still valid
```

Use in loops where a linear value is consumed repeatedly:

```
fun sum_n_times(val: int, n: u32): int
    var acc: int = 0
    var i: u32 = 0
    loop while i .< n
        set acc = acc + val@  // clone each iteration
        set i = i + 1
    end loop
    ret acc
end fun
```

Multiple consumption in a single call:

```
fun consume_both(a: int, b: int): int
    ret a + b
end fun

let x: int = 100
let result = consume_both(x@, x)  // clone for first, move for second
```

### Widen (fixed integers)

```
let a: u8 = 10
let b: u32 = a@     // widen u8 to u32
```

Cross-sign conversion is allowed when lossless:

```
let a: u8 = 5
let b: i16 = a@
```

Valid widening chains:

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
index -> int
offset -> int
u8 -> i16 ...
u16 -> i32 ...
u32 ->
```

`index` and `offset` don't participate in fixed-int widening.
Conversion to/from these types are always considered lossy.


Binary operators propagate expected type:

```
let a: u8 = 100
let b: u8 = 50
let sum: u32 = a@ + b@    // + propagates u32 to both sides

let bytes: u32 = 4096
let limit: u64 = 1000000
if bytes@ .< limit        // .< propagates u64 to left side
    // ...
end if
```

Function parameters propagate their types:

```
fun lerp(a: u64, b: u64, t: u64): u64
    ret a + (b - a) * t / 100
end fun

let lo: u8 = 0
let hi: u8 = 255
let pct: u16 = 50
let mid = lerp(lo@, hi@, pct@)  // each @ gets u64 from param type
```

### Widen + clone

Widening to `int` produces a linear type, so `@` clones if needed:

```
let a: u8 = 10
let b: int = a@     // widen to int (linear), clone happens implicitly
let c: int = a@     // can do it again
```

### Errors

```
let x: u8 = 10
let y = x@          // ERROR: no type context for coercion
let w: i32 = x@     // ERROR: crosses sign boundary (unsigned to signed)
```

### Behavior on copy types without widening

When applied to a copy type where source and target are the same type, `@` is a no-op.

```
let x: u32 = 10
let y: u32 = x@     // no-op, x is copy type, no widening needed
```

### Interaction with try operators

`@` composes with `?` and `!`:

```
let val: u32 = get_byte()?@   // unwrap option, then widen
let data: int = fetch()!@     // unwrap result, then clone
```





