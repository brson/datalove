# Datalit Types

Complete reference for all types available in Datalit.

## Scalar Types

### Booleans

```datalove
@true
@false
```

Type: `@bool` or `bool`

### Integers

Fixed-width integers:

- `u8`, `u16`, `u32`, `u64`, `u128` - unsigned integers
- `i8`, `i16`, `i32`, `i64`, `i128` - signed integers

Arbitrary precision:

- `int` - bigint (arbitrary precision integer)

Examples:

```datalove
: @u32 / 42
: @i64 / -1000
: int / 999999999999999999999999
```

### Floats

- `f32` - 32-bit IEEE 754 float
- `f64` - 64-bit IEEE 754 float

```datalove
: @f32 / 3.14
: @f64 / 2.718281828
```

## Compound Types

### Tuples

Anonymous tuples:

```datalove
(1, 2, 3)
("hello", 42, @true)
```

Named tuples:

```datalove
(x = 1, y = 2, z = 3)
```

### Structs

```datalove
struct Person {
  name = "Ada",
  age = 36,
}
```

Struct type:

```datalove
struct Person {
  name: string,
  age: u32,
}
```

### Enums

```datalove
enum Color::Red
enum Color::Green
enum Color::Blue(intensity = 255)
```

### Lists

```datalove
[1, 2, 3, 4, 5]
: [string] / ["hello", "world"]
```

### Strings

```datalove
"Hello, Datalove!"
```

### Maps

```datalove
map {
  "key1" => "value1",
  "key2" => "value2",
}
```

### Sets

```datalove
set { 1, 2, 3, 4, 5 }
```

## Special Types

### Option

```datalove
some(42)
none
```

Type: `?T` for any type `T`

### Result

```datalove
ok(42)
err("something went wrong")
```

Type: `!T` for any type `T`

### Data (Runtime Type)

```datalove
data(42)
```

The `data` type can hold any Datalit value with runtime type information.

### Error

```datalove
error("error message")
```

Used for error handling, can contain any Datalit value.

## Heap Sigils

Types can be prefixed with `@` to indicate heap allocation strategy:

```datalove
@u32    % heap-allocated u32
@bool   % heap-allocated bool
```

In most cases, heap sigils can be omitted and will be inferred.

## Total Ordering

All Datalit types support total ordering, used for maps and sets.

For floats, the ordering is:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

## See Also

- [Literals](literals.md) - How to write these types
- [Type Hints](type-hints.md) - Explicit type annotations
- [Type System Guide](../../guides/type-system.md) - Deep dive into the type system
