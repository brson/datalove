# Type Hints

Datalit supports explicit type annotations using the `: type / expr` syntax.

## Basic Syntax

The general form is:

```
: <type> / <expression>
```

- `:` introduces a type annotation
- `<type>` is the type you want to specify
- `/` separates the type from the expression
- `<expression>` is the actual value

## Examples

### Simple Type Hints

```datalove
: @u32 / 42
: string / "hello"
: bool / @true
```

### Compound Type Hints

Lists:

```datalove
: [string] / ["hello", "world"]
: [@u32] / [1, 2, 3]
```

Structs:

```datalove
: {
  name: string,
  age: u32,
} / {
  name = "Ada",
  age = 36,
}
```

### Nested Type Hints

You can nest type hints within expressions:

```datalove
{
  name = : string / "Ada",
  age = : u32 / 36,
  interests = : [string] / ["math", "poetry"],
}
```

### Full Example

```datalove
: {
  name: string,
  born: u32,
  interests: [string],
  address_book: set<struct AddressEntry {
    kind: enum { Friend, Family },
    name: string,
  }>,
} / {
  name = "Ada",
  born = 1815,
  interests = : [string] / [
    "mathematics", "poetry", "music",
  ],
  address_book = set {
    struct AddressEntry {
      kind = enum Friend,
      name = : string / "Charles",
    },
    struct AddressEntry {
      kind = enum Family,
      name = "George",
    },
  },
}
```

## When to Use Type Hints

Type hints are optional but useful when:

- You want to document the expected type
- The type isn't obvious from context
- You're learning Datalove and want to be explicit
- The compiler needs help with type inference

## See Also

- [Types](types.md) - All available types
- [Literals](literals.md) - Writing literal values
