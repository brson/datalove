# Datalit Literals

How to write literal values in Datalit.

## Boolean Literals

```datalove
@true
@false
```

## Integer Literals

Decimal:

```datalove
42
-100
999999999999999999999999
```

Hexadecimal:

```datalove
0xFF
0x1A2B
```

Binary:

```datalove
0b1010
0b11111111
```

## Float Literals

```datalove
3.14
-2.718
1.0e10
```

## String Literals

```datalove
"Hello, world!"
"Multi\nline\nstring"
"Unicode: \u{1F496}"
```

## Tuple Literals

Anonymous:

```datalove
(1, 2, 3)
("hello", 42)
```

Named:

```datalove
(x = 10, y = 20)
(name = "Ada", age = 36)
```

## List Literals

```datalove
[]
[1, 2, 3, 4, 5]
["a", "b", "c"]
```

## Map Literals

```datalove
map {}
map {
  "name" => "Ada",
  "age" => 36,
}
```

## Set Literals

```datalove
set {}
set { 1, 2, 3 }
set { "a", "b", "c" }
```

## Struct Literals

```datalove
struct Person {
  name = "Ada",
  age = 36,
}
```

## Enum Literals

```datalove
enum Color::Red
enum Color::Blue(intensity = 255)
```

## Option Literals

```datalove
some(42)
none
```

## Result Literals

```datalove
ok(42)
err("error message")
```

## See Also

- [Types](types.md) - All available types
- [Type Hints](type-hints.md) - Adding type annotations
