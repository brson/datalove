# Datalove Operators - `? ! @` etc.




## Operator precedence

| Level | Operators                                           | Description    |
|-------|-----------------------------------------------------|----------------|
| 1     | `()` `f()`                                          | Grouping, call |
| 2     | `-` `-?` `-!` `not` `some` `ok` `er` `data` `error` | Unary prefix   |
| 3     | `.field` `.0` `[i]` `?` `!` `@`                     | Postfix        |
| 4     | `*` `/` `*!` `/!` `*?` `/?`                         | Multiplicative |
| 5     | `+` `-` `+!` `-!` `+?` `-?`                         | Additive       |
| 6     | `.<` `.>` `<=` `>=` `==` `!=`                       | Comparison     |
| 7     | `and`                                               | Logical AND    |
| 8     | `or` `xor`                                          | Logical OR/XOR |

Binary operators are left-associative:
`a - b - c` is `(a - b) - c`,
and `a or b xor c` is `(a or b) xor c`.
Comparisons do not chain: `a == b == c` is an error.

Prefix operators bind tighter than postfix operators,
so `-x?` is `(-x)?` and `not s.a` is `(not s).a`.
The payload keywords `some`, `ok`, `er`, `data` and `error`
are the exception, taking postfix operators onto their payload:
`some x@` is `some (x@)`.

Spacing decides whether a sigil operator is prefix, infix or postfix
before precedence is consulted.
`a - b` and `a-b` subtract, but `a -b` is `a` followed by `-b`.
Postfix operators must be written against their operand: `x?`, not `x ?`.




## Logic operators

Booleans support `and`, `or`, `xor`, and `not`.
All take `bool` operands and produce `bool`.
`not` is prefix; the others are infix,
with `and` binding tighter than `or` and `xor`.

```datalove
let a = true or false and false     // true or (false and false)
let b = not a xor true
```

Both operands are always evaluated:
`and` and `or` do not short-circuit.




## Comparison operators

```datalove
.<    less than
.>    greater than
<=    less than or equal
>=    greater than or equal
==    equal
!=    not equal
```

Less-than and greater-than are spelled `.<` and `.>`;
`<` and `>` are not operators (they are matched braces).

All comparisons produce `bool`,
and both operands must have the same type &mdash;
there is no implicit widening, though `@` on one side
takes its target type from the other side.

The ordering operators take numeric operands only.
Order other values with `sys/std/ord`.

`==` and `!=` take numbers, `bool`, `string`, unit and atoms,
and options, tuples, structs, terms and enums made of those,
compared part by part.
Lists, sets, maps, tables, tensors, results,
`data`, `error` and functions do not have `==` yet.



## Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

This order is available to code through `sys/std/ord`.

Equality, less than, greater than, etc. behave
the standard IEEE 754 way wrt float zeros and NaNs:
`0.0 == -0.0`, and NaN is not equal to, less than or greater than anything,
even inside an option or tuple.




## The adapt operator - `@`

The postfix _adapt_ operator, `@`,
performs a lossless clone and/or coercion.

A single operator for making expression types "fit" their destination.
Balances correctness with scripting ergonomics.

Performs whatever lossless conversion is needed:
- **Widen** fixed integers along their signedness chain
- **Clone** linear types so the original remains valid
- **Both** when widening produces a linear type

It also widens `f32` to `f64`,
and an atom or term to an enum type that lists it.

Target type inferred from context:
a typed `let` or `var`, return position, a call argument,
an operand of an arithmetic or logical operator,
or the other side of a comparison.
With no context, `@` just clones,
and the result has the operand's type.

### Clone (linear types)

```datalove
let x: int = 42
let y = x@          // clone x
let z = x           // x still valid
```

Use in loops where a linear value is consumed repeatedly:

```datalove
fun add(a: int, b: int): int
    ret a + b
end fun

fun sum_n_times(val: int, n: int): int
    var acc: int = 0
    var i: int = 0
    loop while i .< n
        set acc = add(acc, val@)  // clone each iteration
        set i = i + 1
    end loop
    ret acc
end fun
```

Operators borrow their operands rather than consuming them,
so `acc + val` needs no `@`.

Multiple consumption in a single call:

```datalove
fun consume_both(a: int, b: int): int
    ret a + b
end fun

let x: int = 100
let result = consume_both(x@, x)  // clone for first, move for second
```

### Widen (fixed integers)

```datalove
let a: u8 = 10
let b: u32 = a@     // widen u8 to u32
```

Cross-sign conversion is allowed when lossless:

```datalove
let a: u8 = 5
let b: i16 = a@
```

Valid widening chains:

```datalove
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
u8 -> i16, i32, i64, int
u16 -> i32, i64, int
u32 -> i64, int
index -> int
offset -> int
f32 -> f64
```

`index` and `offset` don't participate in fixed-int widening.
Conversions between them and the fixed ints are always considered lossy.


Binary operators propagate expected type:

```datalove
let a: u8 = 100
let b: u8 = 50
let sum: int = a@ + b@    // + propagates int to both sides
let sum2: u32 = a@ +! b@  // +! propagates u32 to both sides

let bytes: u32 = 4096
let limit: u64 = 1000000
if bytes@ .< limit        // .< propagates u64 to left side
    // ...
end if
```

Function parameters propagate their types:

```datalove
fun lerp(a: u64, b: u64, t: u64): !u64
    ret ok (a +! (b -! a) *! t /! 100)
end fun

let lo: u8 = 0
let hi: u8 = 255
let pct: u16 = 50
let mid = lerp(lo@, hi@, pct@)!  // each @ gets u64 from param type
```

The expected type must be one the operator accepts:
`let sum: u32 = a@ + b@` is still an error,
because bare `+` is not defined on `u32`.

### Widen + clone

Widening to `int` produces a linear type, so `@` clones if needed:

```datalove
let a: u8 = 10
let b: int = a@     // widen to int (linear), clone happens implicitly
let c: int = a@     // can do it again
```

### Errors

```datalove
let w: u32 = 1000
let z: u16 = w@     // ERROR: narrowing (u32 to u16 loses precision)
let i: index = 3
let u: u64 = i@     // ERROR: index does not widen to fixed ints
let a: u8 = 10
let b: u32 = a@ + a@  // ERROR: bare + is not defined on u32
```

Without type context `@` is not an error; it clones:

```datalove
let x: u8 = 10
let y = x@          // y: u8
```

### Behavior on copy types without widening

When applied to a copy type where source and target are the same type, `@` is a no-op.

```datalove
let x: u32 = 10
let y: u32 = x@     // no-op, x is copy type, no widening needed
```

### Interaction with try operators

`@` composes with `?` and `!`:

```datalove
let val: u32 = get_byte()?@   // unwrap option, then widen
let n: int = fetch()!@        // unwrap result, then clone
```





