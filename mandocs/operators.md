# Datalove Operators


## Operator precedence

| Level | Operators                                           | Description    |
|-------|-----------------------------------------------------|----------------|
| 1     | `()`                                                | Grouping       |
| 2     | `-` `-?` `-!` `not` `some` `ok` `er` `data` `error` | Unary prefix |
| 3     | `.field` `.0` `[i]` `@` `?` `!`                    | Postfix        |
| 4     | `*` `/` `*!` `/!` `*?` `/?`                        | Multiplicative |
| 5     | `+` `-` `+!` `-!` `+?` `-?`                        | Additive       |
| 6     | `<` `>` `≤` `≥` `≡` `≢`                            | Comparison     |
| 7     | `and`                                               | Logical AND    |
| 8     | `or` `xor`                                          | Logical OR/XOR |


## Arithmetic

**Bare arithmetic** (`+`, `-`, `*`, `/`) depends on type:

- **Floats**: Returns the same float type. Division permitted.
- **Bigints** (`int`): Addition, subtraction, multiplication, unary negation.
  Division not permitted (use checked variants).
- **Fixed integers**: Bare arithmetic not permitted.
  Use `@` to widen to `int`, or use checked/optional operators.

**Checked arithmetic** (`+!` `-!` `*!` `/!`) operates on fixed integers
and returns a result type. On overflow or division by zero, the enclosing
function early-returns an error.

```datalove
fun add(a: u32, b: u32): !u32
    ret ok (a +! b)
end fun
```

**Optional arithmetic** (`+?` `-?` `*?` `/?`) is the same but returns
an option type. On overflow, the function early-returns `none`.

```datalove
fun add(a: u32, b: u32): ?u32
    ret some (a +? b)
end fun
```

All operators borrow their operands -- operands are not consumed.


## Comparison operators

```
<     less than
>     greater than
≤     less than or equal
≥     greater than or equal
≡     equal
≢     not equal
```

All comparison operators require same-type operands and return `bool`.
Like arithmetic operators, they borrow their operands.


## Logical operators

`and`, `or`, `xor`, and `not` operate on `bool` values and return `bool`.


## Try operators `?` and `!`

Postfix `?` unwraps an option, early-returning `none` if absent.

```datalove
fun get_value(opt: ?i32): ?i32
    let x = opt?
    ret some (x + 1)
end fun
```

Postfix `!` unwraps a result, early-returning the error on failure.

```datalove
fun parse(s: string): !i32
    let n = do_parse(s)!
    ret ok n
end fun
```

The enclosing function's return type must match:
`?` requires the function to return `?R`, `!` requires `!R`.


## Indexing

List, map, and tensor indexing is fallible -- there is no
infallible/panicking variant.

```datalove
a[i]?       // early-return none on out-of-bounds
a[i]!       // early-return error on out-of-bounds
m[key]?     // early-return none on missing key
m[key]!     // early-return error on missing key
```

Bare `a[i]` without `?` or `!` is a type error in read context.

Index and field steps can be chained:

```datalove
a[i]?.field
m[key]?.0
a[i]?.b[j]?
```


## The adapt operator `@`

The postfix adapt operator performs a lossless clone and/or coercion.
A single operator for making expression types fit their destination.

### Clone (linear types)

```datalove
let x: int = 42
let y = x@          // clone x
let z = x           // x still valid
```

### Widen (fixed integers)

```datalove
let a: u8 = 10
let b: u32 = a@     // widen u8 to u32
```

Cross-sign widening is allowed when lossless:

```datalove
let a: u8 = 5
let b: i16 = a@
```

Valid widening chains:

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
u8 -> i16, i32, i64, int       (cross-sign)
u16 -> i32, i64, int           (cross-sign)
u32 -> i64, int                (cross-sign)
index -> int
offset -> int
```

`@` requires a type context -- it cannot synthesize a target type.

### Atom/term to enum

```datalove
type Color: enum { atom Red, atom Blue }
let c: Color = (atom Red)@
```

### Widen + clone

Widening to `int` produces a linear type, so `@` clones if needed:

```datalove
let a: u8 = 10
let b: int = a@     // widen + clone
let c: int = a@     // can do it again
```

### Composition with try operators

```datalove
let val: u32 = get_byte()?@   // unwrap option, then widen
let data: int = fetch()!@     // unwrap result, then widen+clone
```

### Errors

```datalove
let x: u8 = 10
let y = x@          // error: no type context
let w: u32 = 1000
let z: u16 = w@     // error: narrowing not permitted
```

When applied to a copy type with no widening needed, `@` is a no-op.
