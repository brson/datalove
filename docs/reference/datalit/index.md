# Datalit - Datalove Literals

File extension: `.dlt`

Datalit is the tiny and comprehensible foundation of Datalove, a strongly-typed and declarative pure-data language for expressing typical data structures.

## What is Datalit?

Datalit is:

- A **serialization format** like JSON, but with richer types
- **Strongly statically typed** with optional type hints
- The **foundation** of all Datalove code
- **Pure data** - no computation, just values

If you understand Datalit, you understand 80% of Datalove.

## Core Concepts

- [Types](types.md) - All the types available in Datalit
- [Literals](literals.md) - How to write literal values
- [Type Hints](type-hints.md) - The `: type / expr` syntax
- [Examples](examples.md) - Complete examples

## Quick Example

```datalove
{
  name = "Ada",
  born = 1815,
  interests = ["mathematics", "poetry", "music"],
}
```

With explicit type hints:

```datalove
: {
  name: string,
  born: u32,
  interests: [string],
} / {
  name = "Ada",
  born = 1815,
  interests = ["mathematics", "poetry", "music"],
}
```

## Key Properties

- All types are owned tree-shaped value types
- No interior mutability, native pointers, or cycles
- Supports free non-destructive coercions
- Full runtime type descriptors
- Total ordering on all data types
- Forms the basis of the Datalove runtime ABI

## Learn More

- [Type System Guide](../../guides/type-system.md)
- [CLI Commands for Datalit](../../cli/lit-commands.md)
