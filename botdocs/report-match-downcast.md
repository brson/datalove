# Research: Destructuring and Pattern Matching for Datalove

**Status:** Enum match (Option A) is implemented. The `match`/`case`/`end match`
syntax works with atom and term variants, exhaustiveness checking, and default
arms. The open questions below about `@data` downcast, `let` destructuring, and
nested patterns remain unimplemented.

## Current State

Datalove has:
- Anonymous enums: `@enum { Foo, Bar(@u32) }` with values `@enum Foo`, `@enum Bar(2)`
- `if` binding for option: `if opt |value| ... else ... end if`
- `if` binding for result: `if res |value| ... else |error| ... end if`
- Try operators: `expr?` (option early-return), `expr!` (result early-return)
- `@data` and `@error` types (dynamically typed wrappers)

## Language Precedents

### Rust

```rust
// Full match
match value {
    Variant::A => ...,
    Variant::B(x) => ...,
    Variant::C { field } => ...,
}

// if let - single pattern
if let Some(x) = opt { ... }

// let else - refutable pattern with diverging else
let Some(x) = opt else { return; };

// while let
while let Some(x) = iter.next() { ... }
```

### Swift

```swift
// switch (exhaustive)
switch value {
case .a: ...
case .b(let x): ...
}

// if case let
if case .some(let x) = opt { ... }

// guard case let (early exit)
guard case .success(let x) = result else { return }
```

### OCaml/F#

```ocaml
match value with
| A -> ...
| B x -> ...
| C { field } -> ...
```

### Kotlin

```kotlin
when (value) {
    is Foo -> ...
    is Bar -> value.payload  // smart cast
}
```

### Elixir

```elixir
case value do
  {:ok, x} -> ...
  {:error, e} -> ...
end
```

### Zig

```zig
switch (value) {
    .foo => ...,
    .bar => |payload| ...,
}
```

## Design Options for Datalove

Given datalove's existing conventions (keyword blocks, `|binding|` syntax, `@` sigils), here are concrete options:

### Option A: Keyword-delimited `match` Block

```datalove
match value
case Foo
    ...
case Bar |x|
    ...
end match
```

Pros:
- Consistent with `if...end if`, `loop...end loop`, `fun...end fun`
- Uses existing `|binding|` syntax
- Clean vertical layout

Cons:
- Requires `case` keyword for each arm
- More verbose than some alternatives

### Option B: Brace-style `match` (More Compact)

```datalove
match value {
    Foo => ...,
    Bar |x| => ...,
}
```

Pros:
- More compact
- Familiar to Rust/Swift users

Cons:
- Inconsistent with datalove's keyword-block style
- Braces already used for structs

### Option C: Pattern `if` Extension (No New Keyword)

Extend the existing `if` binding to work with enums:

```datalove
if value |Foo|
    ...
else |Bar(x)|
    ...
end if
```

or with explicit pattern syntax:

```datalove
if value = @enum Foo
    ...
else if value = @enum Bar |x|
    ...
end if
```

Pros:
- No new top-level construct
- Builds on existing `if |binding|` familiarity

Cons:
- Gets awkward with many variants
- Exhaustiveness checking harder to express

### Option D: Hybrid - `match` as Special `if`

```datalove
if match value
    Foo =>
        ...
    Bar |x| =>
        ...
end if
```

## Destructuring `let`

For future `let` destructuring:

```datalove
// Tuple
let (a, b) = tuple

// Struct
let { x, y } = struct_val

// Enum (refutable - needs else or must be in match)
let Bar |x| = value else
    ret @none
end let
```

## Downcast Mechanism for `@data` / `@error`

`@data` wraps any type. Downcasting needs:

### Option 1: `as` with Result

```datalove
let typed: !u32 = data_val as u32  // returns result
let typed = data_val as u32!       // early return on type mismatch
```

### Option 2: `match` with type patterns

```datalove
match data_val
case : u32 |x|
    ...
case : string |s|
    ...
case _
    ...
end match
```

### Option 3: Type-testing `if`

```datalove
if data_val is u32 |x|
    // x is u32
else
    ...
end if
```

## Recommendations

Given datalove's style:

### For `match`: Option A (keyword-delimited)

Fits best with the language's existing idioms:

```datalove
match value
case Foo
    ret @1
case Bar |x|
    ret x
end match
```

### For `@data` downcast: Combination approach

`is` for type testing, `as` for conversion:

```datalove
// Type test with binding
if data_val is u32 |x|
    ...
end if

// Assertion-style (panics on mismatch)
let x = data_val as! u32

// Result-returning
let x: !u32 = data_val as u32
```

### For `let` destructuring (future)

Keep it simple and irrefutable by default:

```datalove
let (a, b) = tuple
let { x, y } = struct_val
```

Refutable patterns only in `match` or with explicit `else`.

## Open Questions

1. Should `match` require exhaustiveness, or allow a default `case _`?
2. Should enum variant names need the `@enum` prefix in patterns?
3. For `@data`, should the type test be structural or nominal?
4. Should bindings in patterns support nested destructuring? (`case Bar |(a, b)|`)
