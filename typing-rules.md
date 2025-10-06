# Datalit Typing Rules

This document specifies the type system for datalit expressions.
It follows a bidirectional typing discipline based on Dunfield & Krishnaswami (2013).

## Core Principles

- **Bidirectional**: Expressions either synthesize (⇒) or check (⇐) against types
- **Explicit heaps**: `@` for local, `#` for global, omitted for inferred (TBD)
- **Structural typing**: Anonymous types (tuples, structs, enums) are compatible by structure
- **Nominal typing**: Named types (`struct Foo`, `enum Bar`, `token Nil`) are distinct even with same structure
- **Explicit at boundaries**: The `: type / expr` syntax provides type information at expression boundaries

## Type Equivalence

Two types are equivalent (T ≡ T') when:

### Primitives
- `@bool ≡ @bool`
- `@u32 ≡ @u32`
- `@f32 ≡ @f32`
- `@int ≡ @int`
- `@string ≡ @string`
- `@nil ≡ @nil`

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
  - Same field order (for now - may relax later)
  - Ti ≡ Ti' for all i

Examples:
```
{x: @u32, y: @bool} ≡ {x: @u32, y: @bool}  ✓
{x: @u32, y: @bool} ≡ {y: @bool, x: @u32}  ✗ (different order - for now)
{x: @u32} ≡ {x: @u32, y: @bool}            ✗ (different fields)
```

### Named Types (nominal)
- `@struct Point{x: T1, y: T2} ≢ @struct Vec2{x: T1, y: T2}` even if fields match
- `@tuple Pair(T1, T2) ≢ @tuple Point(T1, T2)` even if elements match
- `@enum Result{...} ≢ @enum Option{...}` even if variants match
- `@token Nil ≡ @token Nil` (only by name)
- `@token Nil ≢ @token None` (different names)

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

### Dynamic types
- `@data ≡ @data`
- `@error ≡ @error`

## Subtyping

**Conservative approach**: Minimal implicit conversions to start.

### Integer subtyping
- `@int <: @int` (reflexive)
- No implicit conversions between integer types initially

Examples:
```
: @u32 / @1234  ✓ (explicit annotation)
: @int / @1234  ✓ (explicit annotation)
: @u32 / : @int / @1234  ✗ (no implicit int-to-u32 conversion)
```

**Open question**: Should we allow `@int <: @u32` with runtime range check?

### Nil subtyping
- `@nil <: @nil`
- No implicit nil-to-option conversion (must use `@none` explicitly)

### Option/Result subtyping
- No implicit wrapping initially

Examples:
```
: @?@u32 / @42    ✗ (no implicit Some wrapping - use explicit annotation if needed)
: @?@u32 / @none  ✓ (explicit none)
: @!@u32 / @42    ✗ (no implicit Ok wrapping)
```

### Anonymous to named
- No subtyping between anonymous and named types

Examples:
```
: @struct Point{x: @u32, y: @u32} / {x = @1, y = @2}  ✗ (anon struct ≮: named struct)
: (@u32, @u32) / @tuple Pair(@1, @2)                  ✗ (named tuple ≮: anon tuple)
```

**Note**: These conversions might be allowed later with explicit syntax.

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

### Rule: Syn-U32
```
n : u32 value
─────────────────
@n ⇒ @u32
```

Example:
```
@42 ⇒ @u32
@1234 ⇒ @u32
```

### Rule: Syn-F32
```
f : f32 value (contains decimal point)
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

### Rule: Syn-Nil
```
─────────────────
@nil ⇒ @nil
```

### Rule: Syn-Token (resolved)
```
@Foo resolves to type hint @token Foo
──────────────────────────────────────
@Foo ⇒ @token Foo
```

Example (after name resolution):
```
: @token Nil / @Nil
                ↑
             @Nil ⇒ @token Nil
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

## Checking Rules (e ⇐ T)

Checking rules verify an expression against an expected type.

### Rule: Check-Subsume
```
e ⇒ T'
T' <: T
───────────
e ⇐ T
```

This is the key rule that allows synthesizing expressions to be checked.

### Rule: Check-Int
```
n : arbitrary precision integer
n fits in u32 range
────────────────────────────────
@n ⇐ @u32

n : arbitrary precision integer
────────────────────────────────
@n ⇐ @int
```

Examples:
```
@42 ⇐ @u32     ✓
@42 ⇐ @int     ✓
@-1 ⇐ @u32     ✗ (out of range)
@-1 ⇐ @int     ✓
```

**Note**: Bare numerals without `@` are treated as `@int` for now.

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

### Rule: Check-Error
```
──────────────────────
@error "msg" ⇐ @error
```

### Rule: Check-Dynamic-Data
```
────────────
e ⇐ @data
```

Any expression checks against `@data` (runtime validation only).

## Heap Checking

**Design decision**: Heaps are tracked separately from types for now.

Each type and expression has an associated heap:
- `@` means local heap
- `#` means global heap
- Omitted means inferred (defaults to local for now)

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

Open question: Can local flow to global? Global to local?
```

**For now**: Require exact heap match. May relax later.

Examples:
```
: @u32 / @42   ✓ (both local heap)
: #u32 / #42   ✓ (both global heap)
: @u32 / #42   ✗ (heap mismatch - for now)
: #u32 / @42   ✗ (heap mismatch - for now)
```

## Dynamic Types

### @data type

The `@data` type represents dynamically typed data. Any expression can be checked against `@data`:

```
────────────
e ⇐ @data
```

Type checking is deferred to runtime. The typechecker simply accepts any expression.

Example:
```
: @data / @42              ✓
: @data / "hello"          ✓
: @data / {x: @1, y: @2}   ✓
```

**Open question**: Can `@data` synthesize? Or only check?
- If synthesis: `e : @data ⇒ @data` (loses precision)
- If check only: Must have explicit type annotation

### @error type

Similar to `@data`, but for error values:

```
────────────────────
@error "msg" ⇐ @error
```

## Edge Cases and Open Questions

### 1. Empty collections

**Current rule**: Empty collections can check against any element type.

```
[] ⇐ [@T]           ✓ for any T
@set{} ⇐ @set<T>    ✓ for any T
@map{} ⇐ @map<K, V> ✓ for any K, V
```

**Question**: Should empty collections require a type annotation?

### 2. Nested type hints

```
: @u32 / : @int / @5
```

**Current rule**: Inner type hint wins. The outer `@u32` is ignored, and the expression has type `@int`.

**Alternative**: Require inner type to be subtype of outer type?

### 3. Field order in anonymous structs

**Current rule**: Field order matters.

```
{x: @u32, y: @bool} ≢ {y: @bool, x: @u32}
```

**Alternative**: Allow unordered fields (more flexible, but complicates implementation).

### 4. Anonymous enum to named enum coercion

From demo-data.dle:
```
; this is actually a coercion from anonymous enum (no dot)
: @enum Quux {
  Bar(@u32),
} / @enum Bar(@true, 1)
```

**Question**: Is this allowed? If so, what are the rules?
- Named enum must have a variant matching the anonymous constructor?
- Payload types must match?

### 5. Bare numerals without @ or # sigil

```
: @u32 / 42   (no @ on the 42)
```

**Current behavior**: Parser treats bare numerals as having omitted heap.

**Question**: What type do they synthesize?
- Option A: Synthesize `@int` (most general)
- Option B: Cannot synthesize (must be checked)
- Option C: Synthesize based on value (small ints are u32, etc.)

### 6. Token types and structural equivalence

Are token types purely nominal, or do they have structure?

```
: @token Nil / @Nil
```

**Current**: Tokens are nominal zero-sized types. `@token Nil ≡ @token Nil` only.

**Question**: Should tokens be compatible with `@nil`? Or completely distinct?

### 7. Option sugar and implicit wrapping

Should these be allowed?

```
: @?@u32 / @42     (implicit Some wrapping?)
: @!@u32 / @42     (implicit Ok wrapping?)
```

**Current**: No implicit wrapping. Must be explicit.

**Alternative**: Allow implicit wrapping when expected type is option/result.

### 8. Nil vs None

```
@nil  : type constructor
@none : value of option type
```

Are these related? Should `@nil ⇐ @?T` be allowed?

**Current**: No. `@nil` has type `@nil`. `@none` has type `@?T` for any T.

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
- Resolution results to look up types for tokens, named structs, etc.
- Scope information for type variables (if added later)

### Error messages

Typechecker should produce clear errors:
- "Expected type `@u32`, but expression has type `@bool`"
- "Field `y` is missing in struct construction"
- "Cannot synthesize type for expression (add type annotation)"
- "Integer literal `99999999999` is out of range for type `@u32`"

## Future Extensions

Features to consider adding later:

1. **Type variables and generics**: `list<T>`, `@map<K, V>`
2. **Subtyping**: More permissive conversions (with safety checks)
3. **Type aliases**: `type Point = {x: @f32, y: @f32}`
4. **Dependent types**: Types that depend on values
5. **Refinement types**: `@u32{x | x > 0}` (positive integers)
6. **Gradual typing**: Mix of static and dynamic checking
