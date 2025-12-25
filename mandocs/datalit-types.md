## The Datatypes of Datalit

Datalove's datatypes, their representation,
and semantics are focused on correctness and completeness,
particularly with regard to tricky corner cases related to subjects like overflow, NaNs, etc;
and we take some difficult positions.

Datalit types are subtypes of Datafun types,
and have a dedicated declarative syntax suitable
for serialization and configuration.

Datalit types are:

- [Booleans](#user-content-booleans),
  `bool` - `true` and `false`
- [Fixed integers](#user-content-fixed-integers),
  `u8`, `u16`, `u16`, `u32`, `u64`,
  `i8`, `i16`, `i16`, `i32`, `i64`
- [Big integers](#user-content-big-integers),
  `int`
- [Floating point numbers](#user-content-floating-point-numbers),
  `f32`
- [Anonymous structs](#user-content-anonymous-structs)
- [Anonymous enums (ADTs)](#user-content-anonymous-enums)
- [Lists](#user-content-lists),
  `list<T>` - contiguous, growable, heap-allocated arrays
- [Maps and Sets](#user-content-maps-and-sets)
  `map<K, V>` and `set<T>`
- [Tensors](#user-content-tensors),
  `tensor<T>` - multidimensional arrays
- [Optional types](#user-content-optional-types),
  `?T`
- [Result types](#user-content-result-types),
  `!T` - which may either hold `T` or `error`
- [Existential data types](#user-content-existential-data-types),
  `data` - dynamically-typed and pointer-packed version of any of the above
- [Existential error types](#user-content-existential-error-types),
  `error` - dynamically-typed and pointer-packed version of any of the above

Datalit types are value types and stored inline.
Datalit structs and enums are structurally-typed.




## Booleans

The `bool` type has two values, `true` and `false`.

```datalove
let a: bool = true
let b: bool = false
```

Booleans are used in `if` and `else if` conditions.

```datalove
require sys/std/debug

let a = true
let b = false
if a
  debug.print("a")
else if b
  debug.print("b")
else
  debug.print
end if
```

Booleans support logic operators `and`, `or`, `xor`, `implies` and `not`.

```datalove
let a = true
let b = false
let c = a or b
let d = a and  b
let e = a xor b
let f = a implies b
let g = not a
```

Truth tables:

| a     | b     | a and b |
|-------|-------|---------|
| false | false | false   |
| false | true  | false   |
| true  | false | false   |
| true  | true  | true    |

| a     | b     | a or b |
|-------|-------|--------|
| false | false | false  |
| false | true  | true   |
| true  | false | true   |
| true  | true  | true   |

| a     | b     | a xor b |
|-------|-------|---------|
| false | false | false   |
| false | true  | true    |
| true  | false | true    |
| true  | true  | false   |

| a     | b     | a implies b |
|-------|-------|-------------|
| false | false | true        |
| false | true  | true        |
| true  | false | false       |
| true  | true  | true        |

| a     | not a |
|-------|-------|
| false | true  |
| true  | false |




## Fixed integers

## Big integers

## Floating point numbers

## Anonymous structs

## Anonymous enums

## Lists

## Maps and sets

## Tensors

## Optional types

## Result types

## Existential data types

## Existential error types
