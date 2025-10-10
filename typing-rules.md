# Datalit Typing Rules

This document specifies the type system for datalit expressions.
It follows a bidirectional typing discipline based on Dunfield & Krishnaswami (2013).

## Core Principles

- **Bidirectional**: Expressions either synthesize (⇒) or check (⇐) against types
- **Explicit heaps**: `@` for local, `#` for global, omitted for inferred (defaults to local)
- **Structural typing**: Anonymous types (tuples, structs, enums) are compatible by structure
- **Nominal typing**: Named types (`struct Foo`, `enum Bar`, `tuple Pair`) are distinct even with same structure
- **Explicit at boundaries**: The `: type / expr` syntax provides type information at expression boundaries

## Type Equivalence

Two types are equivalent (T ≡ T') when:

### Primitives
- `@bool ≡ @bool`
- `@u32 ≡ @u32`
- `@f32 ≡ @f32`
- `@int ≡ @int` (arbitrary precision integers)
- `@string ≡ @string`

### Tuples (structural)
- `(T1, T2, ..., Tn) ≡ (T1', T2', ..., Tn')` iff Ti ≡ Ti' for all i
- Order matters
- Length must match

Examples:
```
(@u32, @bool) ≡ (@u32, @bool)  ✓
(@u32, @bool) ≡ (@bool, @u32)  ✗ (different order)
(@u32, @bool) ≡ (@u32)         ✗ (different length)
```

### Anonymous Structs (structural)
- `{f1: T1, f2: T2, ...} ≡ {f1: T1', f2: T2', ...}` iff:
  - Same field names
  - Same field order (field order must be correct)
  - Ti ≡ Ti' for all i

Examples:
```
{x: @u32, y: @bool} ≡ {x: @u32, y: @bool}  ✓
{x: @u32, y: @bool} ≡ {y: @bool, x: @u32}  ✗ (different order)
{x: @u32} ≡ {x: @u32, y: @bool}            ✗ (different fields)
```

### Named Types (nominal)
- `@struct Point{x: T1, y: T2} ≢ @struct Vec2{x: T1, y: T2}` even if fields match
- `@tuple Pair(T1, T2) ≢ @tuple Point(T1, T2)` even if elements match
- `@enum Result{...} ≢ @enum Option{...}` even if variants match

### Anonymous Enums (structural)
- Enum types are equivalent if they have the same set of variants (order-independent)
- Variant names must match
- Payload types must match

Examples:
```
@enum{Foo, Bar(@u32)} ≡ @enum{Bar(@u32), Foo}  ✓ (order doesn't matter for enums)
@enum{Foo} ≡ @enum{Bar}                        ✗ (different variant names)
```

### Collections
- `[@T] ≡ [@T']` iff T ≡ T'
- `@map<K, V> ≡ @map<K', V'>` iff K ≡ K' and V ≡ V'
- `@set<T> ≡ @set<T'>` iff T ≡ T'

### Option and Result
- `@?T ≡ @?T'` iff T ≡ T'
- `@!T ≡ @!T'` iff T ≡ T'

### Error type
- `@error ≡ @error`

## Subtyping

**Conservative approach**: Minimal implicit conversions.

### Option/Result implicit wrapping

Values can be implicitly wrapped in Option or Result types:

Examples:
```
: @?@u32 / @42    ✓ (implicit Some wrapping)
: @?@u32 / @none  ✓ (explicit none)
: @!@u32 / @42    ✓ (implicit Ok wrapping)
```

### Anonymous to named coercion

Anonymous types can be coerced to named types with matching structure:

Examples:
```
: @struct Point{x: @u32, y: @u32} / {x = @1, y = @2}  ✓ (anon struct → named struct)
: @tuple Pair(@u32, @u32) / (@1, @2)                  ✓ (anon tuple → named tuple)
: @enum Result{Ok: @u32, Err} / @enum Ok(@42)         ✓ (anon enum → named enum)
```

## Synthesis Rules (e ⇒ T)

Synthesis rules determine what type an expression produces.

### Rule: Syn-TypedExpr
```
e : ExprFull with type_hint = Some(T)
e.expr ⇐ T
────────────────────────────────────
e ⇒ T
```

Example:
```
: @u32 / @42 ⇒ @u32
```

### Rule: Syn-Bool
```
─────────────────
@true ⇒ @bool

─────────────────
@false ⇒ @bool
```

### Rule: Syn-Int
```
n : integer literal (as string)
n fits in u32 range
────────────────────────────────
@n ⇒ @u32

n : integer literal (as string)
n does not fit in u32 range
────────────────────────────────
@n ⇒ IntOutOfRange error
```

**Note**: Integer literals without type context default to `@u32`. If the value doesn't fit in u32 range, it's a type error. Use explicit type hints for arbitrary precision integers.

Examples:
```
@42 ⇒ @u32                           ✓
@99999999999999999999 ⇒ error        ✗ (IntOutOfRange)
: @int / @99999999999999999999 ⇒ @int  ✓ (explicit type hint)
```

### Rule: Syn-Float
```
f : float literal (contains decimal point)
────────────────────────────────────
@f ⇒ @f32
```

Example:
```
@1.0 ⇒ @f32
@3.14159 ⇒ @f32
```

### Rule: Syn-String
```
s : string literal
────────────────────
"s" ⇒ @string
```

Example:
```
"hello" ⇒ @string
```

### Rule: Syn-NamedTuple (resolved)
```
@tuple Point(T1, T2, ...) is defined
∀i. ei ⇐ Ti
───────────────────────────────────────
@tuple Point(e1, e2, ...) ⇒ @tuple Point(T1, T2, ...)
```

Example:
```
Type hint: @tuple Pair(@u32, @bool)
Expression: @tuple Pair(@1, @true) ⇒ @tuple Pair(@u32, @bool)
```

### Rule: Syn-NamedStruct (resolved)
```
@struct Point{f1: T1, f2: T2, ...} is defined
∀i. fields contain fi with value ei where ei ⇐ Ti
All fields present
──────────────────────────────────────────────────
@struct Point{f1 = e1, f2 = e2, ...} ⇒ @struct Point{f1: T1, f2: T2, ...}
```

Example:
```
Type hint: @struct Vec2{x: @f32, y: @f32}
Expression: @struct Vec2{x = @1.0, y = @2.0} ⇒ @struct Vec2{x: @f32, y: @f32}
```

### Rule: Syn-NamedEnum (resolved)
```
@enum Result{Ok: T, Err: E, ...} is defined
variant is Ok with payload type T
e ⇐ T
────────────────────────────────────────
@enum Result.Ok(e) ⇒ @enum Result{Ok: T, Err: E, ...}
```

Example:
```
Type hint: @enum Result{Ok: @u32, Err: @string}
Expression: @enum Result.Ok(@42) ⇒ @enum Result{Ok: @u32, Err: @string}
```

### Rule: Syn-None
```
─────────────────
Cannot synthesize type for @none (needs context)
```

**Note**: `@none` can only be checked, not synthesized.

### Rule: Syn-Anonymous-Types
```
─────────────────
Cannot synthesize types for:
- Anonymous tuples: (@1, @2)
- Anonymous structs: {x = @1}
- Anonymous enums: @enum Foo(@1)
- Lists: [@1, @2]
- Maps: @map{@1 = "one"}
- Sets: @set{@1, @2}
```

**Note**: These expressions need type context to determine their element/field types.

## Checking Rules (e ⇐ T)

Checking rules verify an expression against an expected type.

### Rule: Check-Subsume
```
e ⇒ T'
T' ≡ T
───────────
e ⇐ T
```

This is the key rule that allows synthesizing expressions to be checked.

### Rule: Check-Int
```
n : integer literal (as string)
n fits in u32 range
────────────────────────────────
@n ⇐ @u32

n : integer literal (as string)
────────────────────────────────
@n ⇐ @int
```

Examples:
```
@42 ⇐ @u32     ✓
@42 ⇐ @int     ✓
@-1 ⇐ @u32     ✗ (negative, out of range for u32)
@-1 ⇐ @int     ✓
@99999999999999999999 ⇐ @int ✓
@99999999999999999999 ⇐ @u32 ✗ (out of range)
```

### Rule: Check-Float
```
f : float literal
────────────────────────────────
@f ⇐ @f32
```

Example:
```
@3.14 ⇐ @f32  ✓
```

### Rule: Check-AnonTuple
```
(T1, T2, ..., Tn) is expected type
length matches
∀i. ei ⇐ Ti
──────────────────────────────────
(e1, e2, ..., en) ⇐ (T1, T2, ..., Tn)
```

Example:
```
: (@u32, @bool) / (@42, @true)
                   ↑
              (@42, @true) ⇐ (@u32, @bool)  ✓
```

### Rule: Check-NamedTuple (coercion)
```
@tuple Point(T1, T2, ..., Tn) is expected type
length matches
∀i. ei ⇐ Ti
──────────────────────────────────
(e1, e2, ..., en) ⇐ @tuple Point(T1, T2, ..., Tn)
```

**Note**: Anonymous tuples can be coerced to named tuples.

Example:
```
: @tuple Pair(@u32, @bool) / (@42, @true)
                              ↑
                         (@42, @true) ⇐ @tuple Pair(@u32, @bool)  ✓
```

### Rule: Check-NamedTuple (exact match)
```
@tuple Point(T1, T2, ..., Tn) is expected type
names match
length matches
∀i. ei ⇐ Ti
──────────────────────────────────
@tuple Point(e1, e2, ..., en) ⇐ @tuple Point(T1, T2, ..., Tn)
```

### Rule: Check-AnonStruct
```
{f1: T1, f2: T2, ...} is expected type
fields match (same names, same order)
∀i. ei ⇐ Ti
─────────────────────────────────────────
{f1 = e1, f2 = e2, ...} ⇐ {f1: T1, f2: T2, ...}
```

Example:
```
: {x: @u32, y: @bool} / {x = @42, y = @true}
                         ↑
                    {x = @42, y = @true} ⇐ {x: @u32, y: @bool}  ✓
```

### Rule: Check-NamedStruct (coercion)
```
@struct Point{f1: T1, f2: T2, ...} is expected type
fields match (same names, same order)
∀i. ei ⇐ Ti
─────────────────────────────────────────
{f1 = e1, f2 = e2, ...} ⇐ @struct Point{f1: T1, f2: T2, ...}
```

**Note**: Anonymous structs can be coerced to named structs.

Example:
```
: @struct Point{x: @u32, y: @bool} / {x = @42, y = @true}
                                      ↑
                                 {x = @42, y = @true} ⇐ @struct Point{x: @u32, y: @bool}  ✓
```

### Rule: Check-NamedStruct (exact match)
```
@struct Point{f1: T1, f2: T2, ...} is expected type
names match
fields match (same names, same order)
∀i. ei ⇐ Ti
─────────────────────────────────────────
@struct Point{f1 = e1, f2 = e2, ...} ⇐ @struct Point{f1: T1, f2: T2, ...}
```

### Rule: Check-AnonEnum
```
@enum{V1, V2: T2, ...} is expected type
variant Vi exists
If Vi has payload type Ti, then e ⇐ Ti
If Vi has no payload, then expression has no payload
──────────────────────────────────────────────────
@enum Vi(...) ⇐ @enum{V1, V2: T2, ...}
```

Example:
```
: @enum{Foo, Bar: @u32} / @enum Bar(@42)
                          ↑
                     @enum Bar(@42) ⇐ @enum{Foo, Bar: @u32}  ✓
```

### Rule: Check-NamedEnum (coercion)
```
@enum Result{V1, V2: T2, ...} is expected type
variant Vi exists in the named enum
If Vi has payload type Ti, then e ⇐ Ti
If Vi has no payload, then expression has no payload
──────────────────────────────────────────────────
@enum Vi(...) ⇐ @enum Result{V1, V2: T2, ...}
```

**Note**: Anonymous enum constructors (without enum name prefix) can be coerced to named enums.

Example:
```
: @enum Result{Ok: @u32, Err} / @enum Ok(@42)
                                 ↑
                            @enum Ok(@42) ⇐ @enum Result{Ok: @u32, Err}  ✓
```

### Rule: Check-NamedEnum (exact match)
```
@enum Result{V1, V2: T2, ...} is expected type
enum names match
variant Vi exists
If Vi has payload type Ti, then e ⇐ Ti
If Vi has no payload, then expression has no payload
──────────────────────────────────────────────────
@enum Result.Vi(...) ⇐ @enum Result{V1, V2: T2, ...}
```

### Rule: Check-List
```
[@T] is expected type
∀ei ∈ elements. ei ⇐ T
──────────────────────────
[e1, e2, ...] ⇐ [@T]
```

Example:
```
: [@u32] / [@1, @2, @3]
           ↑
      [@1, @2, @3] ⇐ [@u32]  ✓
```

### Rule: Check-EmptyList
```
───────────
[] ⇐ [@T]
```

Empty lists check against any list type.

### Rule: Check-Map
```
@map<K, V> is expected type
∀(ki, vi) ∈ entries. ki ⇐ K ∧ vi ⇐ V
──────────────────────────────────────
@map{k1 = v1, k2 = v2, ...} ⇐ @map<K, V>
```

Example:
```
: @map<@u32, @string> / @map{@1 = "one", @2 = "two"}
                        ↑
                   @map{@1 = "one", @2 = "two"} ⇐ @map<@u32, @string>  ✓
```

### Rule: Check-Set
```
@set<T> is expected type
∀ei ∈ elements. ei ⇐ T
──────────────────────────
@set{e1, e2, ...} ⇐ @set<T>
```

Example:
```
: @set<@u32> / @set{@1, @2, @3}
               ↑
          @set{@1, @2, @3} ⇐ @set<@u32>  ✓
```

### Rule: Check-None
```
──────────────
@none ⇐ @?T
```

`@none` checks against any option type.

### Rule: Check-Option (implicit wrapping)
```
e ⇐ T
──────────────
e ⇐ @?T
```

**Note**: This allows implicit wrapping in Some. Values that successfully check against type T can also check against @?T.

Example:
```
: @?@u32 / @42
           ↑
      @42 ⇐ @u32  ✓
      @42 ⇐ @?@u32  ✓ (implicit Some wrapping)
```

### Rule: Check-Result (implicit wrapping)
```
e ⇐ T
──────────────
e ⇐ @!T
```

**Note**: This allows implicit wrapping in Ok. Values that successfully check against type T can also check against @!T.

Example:
```
: @!@u32 / @42
           ↑
      @42 ⇐ @u32  ✓
      @42 ⇐ @!@u32  ✓ (implicit Ok wrapping)
```

### Rule: Check-Error
```
──────────────────────
@error "msg" ⇐ @error
```

## Heap Checking

**Design decision**: Heaps are tracked separately from types.

Each type and expression has an associated heap:
- `@` means local heap
- `#` means global heap
- Omitted means inferred (defaults to local)

### Heap Compatibility Rules

```
Local heap values can flow to local heap types:
e has local heap
T has local heap
─────────────────
e : T is valid

Global heap values can flow to global heap types:
e has global heap
T has global heap
─────────────────
e : T is valid

Omitted heap is compatible with local heap:
e has omitted heap
T has local heap
─────────────────
e : T is valid

e has local heap
T has omitted heap
─────────────────
e : T is valid
```

Different heaps (local vs global) never unify. They are not compatible.

Examples:
```
: @u32 / @42   ✓ (both local heap)
: #u32 / #42   ✓ (both global heap)
: @u32 / 42    ✓ (omitted → local)
: @u32 / #42   ✗ (heap mismatch)
: #u32 / @42   ✗ (heap mismatch)
```

## Error Type

The `@error` type represents error values:

```
────────────────────
@error "msg" ⇐ @error
```

This is distinct from the `@!T` (result) type. The `@error` type is for errors without associated success types.

## Design Decisions Summary

### 1. Empty collections

Empty collections can check against any element type:

```
[] ⇐ [@T]           ✓ for any T
@set{} ⇐ @set<T>    ✓ for any T
@map{} ⇐ @map<K, V> ✓ for any K, V
```

In nested contexts, inner empty collections can infer type from neighbors:

```
[
  [@true],
  []        ; inferred as [@bool]
]
```

### 2. Field order in anonymous structs

Field order must be correct:

```
{x: @u32, y: @bool} ≢ {y: @bool, x: @u32}
```

This differs from many languages but simplifies implementation.

### 3. Anonymous to named coercion

Anonymous types (tuples, structs, enums) can be coerced to named types with matching structure. This is a key feature for ergonomic data construction.

### 4. Default numeric types

Bare numeric literals default to concrete types:
- Integer literals → `@u32` (with range check)
- Float literals → `@f32`

Use explicit type hints for other numeric types:
```
: @int / @99999999999999999999  ; arbitrary precision
: @i64 / @-42                     ; signed 64-bit (future)
```

### 5. Option and Result implicit wrapping

Values can be implicitly wrapped in Option/Result types. This is the only way to construct Some and Ok values (no explicit constructors).

## Implementation Notes

### Bidirectional algorithm structure

```rust
fn synthesize(expr: ExprFull) -> Result<Type, TypeError>
fn check(expr: ExprFull, expected: Type) -> Result<(), TypeError>
```

### Interaction with name resolution

Before typechecking:
1. Parse to AST
2. Run name resolution (resolve.rs)
3. Run typechecker with resolution context

Typechecker needs:
- Resolution results to look up types for named structs, tuples, enums
- Type definitions from the type hint in `: type / expr` syntax

### Error messages

Typechecker produces clear errors:
- "Expected type `@u32`, but expression has type `@bool`" (TypeMismatch)
- "Cannot synthesize type for expression (add type annotation)" (CannotSynthesize)
- "Integer literal out of range for type `@u32`" (IntOutOfRange)
- "Heap mismatch: expected `@`, got `#`" (HeapMismatch)
- "Field order mismatch" (FieldOrderMismatch)
- "Variant not found" (VariantNotFound)
- "Arity mismatch: expected N fields, got M" (ArityMismatch)

## Future Extensions

Features to consider adding later:

1. **More numeric types**: `@i8`, `@i16`, `@i32`, `@i64`, `@u8`, `@u16`, `@u64`, `@f64`
2. **Numeric widening**: Automatic coercion from smaller to larger types
3. **Type variables and generics**: User-defined generic types
4. **Type aliases**: `type Point = {x: @f32, y: @f32}`
5. **Refinement types**: `@u32{x | x > 0}` (positive integers)
6. **Gradual typing**: Mix of static and dynamic checking
