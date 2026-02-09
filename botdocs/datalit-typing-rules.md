# Datalit Typing Rules

This document specifies the type system for datalit expressions.
It follows a bidirectional typing discipline based on Dunfield & Krishnaswami (2013).

## Core Principles

- **Bidirectional**: Expressions either synthesize (=>) or check (<=) against types
- **Explicit heaps**: `@` for local, `#` for global, omitted for inferred (defaults to local)
- **Structural typing**: Anonymous types (tuples, structs) are compatible by structure
- **Nominal typing**: Named types (`struct Foo`, `tuple Pair`) are distinct even with same structure
- **Explicit at boundaries**: The `: type / expr` syntax provides type information at expression boundaries

## Type Equivalence

Two types are equivalent (T = T') when:

### Primitives
- `@bool = @bool`
- `@u8 = @u8`, `@i8 = @i8`
- `@u16 = @u16`, `@i16 = @i16`
- `@u32 = @u32`, `@i32 = @i32`
- `@u64 = @u64`, `@i64 = @i64`
- `@f32 = @f32`
- `@int = @int` (arbitrary precision integers)
- `@string = @string`
- `@data = @data`
- `@error = @error`

### Tuples (structural)
- `(T1, T2, ..., Tn) = (T1', T2', ..., Tn')` iff Ti = Ti' for all i
- Order matters
- Length must match

Examples:
```
(@u32, @bool) = (@u32, @bool)  ok
(@u32, @bool) = (@bool, @u32)  FAIL (different order)
(@u32, @bool) = (@u32)         FAIL (different length)
```

### Anonymous Structs (structural)
- `{f1: T1, f2: T2, ...} = {f1: T1', f2: T2', ...}` iff:
  - Same field names
  - Same field order (field order must be correct)
  - Ti = Ti' for all i

Examples:
```
{x: @u32, y: @bool} = {x: @u32, y: @bool}  ok
{x: @u32, y: @bool} = {y: @bool, x: @u32}  FAIL (different order)
{x: @u32} = {x: @u32, y: @bool}            FAIL (different fields)
```

### Named Types (nominal)
- `@struct Point{x: T1, y: T2} != @struct Vec2{x: T1, y: T2}` even if fields match
- `@tuple Pair(T1, T2) != @tuple Point(T1, T2)` even if elements match
### Collections
- `[@T] = [@T']` iff T = T'
- `@map<K, V> = @map<K', V'>` iff K = K' and V = V'
- `@set<T> = @set<T'>` iff T = T'

### Option and Result
- `@?T = @?T'` iff T = T'
- `@!T = @!T'` iff T = T'

### Tensor
- `@tensor<T, R> = @tensor<T', R'>` iff T = T' and R = R' (same element type and rank)

### Atom, Term, and Enum
- `atom A = atom A'` iff A and A' have the same name
- `term T(P) = term T'(P')` iff T = T' (same name) and P = P' (payload types equivalent)
- `enum { v1, v2, ... } = enum { v1', v2', ... }` iff variants match (sorted by name)

Each enum variant is either an atom (no payload) or a term (with payload type).

### Data and Error types
- `@data = @data`
- `@error = @error`

## Numeric Widening

There is no implicit numeric widening in Datalove. All numeric conversions
require the explicit `@` (adapt) operator.

### Widening with `@`

The `@` operator can widen fixed integers along these chains:

Unsigned integers:
```
u8 -> u16 -> u32 -> u64 -> int
```

Signed integers:
```
i8 -> i16 -> i32 -> i64 -> int
```

Cross-sign widening is allowed when lossless (unsigned to larger signed):
```
u8 -> i16, i32, i64, int
u16 -> i32, i64, int
u32 -> i64, int
```

Examples:
```datalove
let a: @u8 = @10
let b: @u16 = a@        // OK: @ widens u8 to u16
let c: @u32 = b@        // OK: @ widens u16 to u32
let d: @int = c@        // OK: @ widens u32 to int

let e: @i32 = c         // ERROR: no implicit widening (requires @)
```

## Synthesis Rules (e => T)

Synthesis rules determine what type an expression produces.

### Rule: Syn-TypedExpr
```
e : ExprFull with type_hint = Some(T)
e.expr <= T
------------------------------------
e => T
```

Example:
```
: @u32 / @42 => @u32
```

### Rule: Syn-Bool
```
-----------------
@true => @bool

-----------------
@false => @bool
```

### Rule: Syn-Int
```
n : integer literal (as string)
--------------------------------
@n => @int
```

**Note**: Integer literals without type context synthesize as `@int`
(arbitrary-precision). Use explicit type hints for fixed-width integer types.

Examples:
```
@42 => @int                             ok
@99999999999999999999 => @int           ok (arbitrary-precision)
: @u32 / @42 => @u32                     ok (explicit type hint)
: @i32 / @-42 => @i32                    ok (explicit type hint)
```

### Rule: Syn-Float
```
f : float literal (contains decimal point)
------------------------------------
@f => @f32
```

Example:
```
@1.0 => @f32
@3.14159 => @f32
```

### Rule: Syn-Hex
```
h : hex literal (0x...)
--------------------------------
@h => @int
```

**Note**: Hex literals synthesize as `@int`. Use type hints for fixed-width types.

Example:
```
@0xFF => @int
@0xFFFFFFFFFF => @int                   ok (arbitrary-precision)
: @u64 / @0xFFFFFFFFFF => @u64          ok (explicit type hint)
: @f32 / @0xABABABAB => @f32            ok (hex as bit pattern)
```

### Rule: Syn-String
```
s : string literal
--------------------
"s" => @string
```

Example:
```
"hello" => @string
```

### Rule: Syn-AnonTuple
```
for all i: ei => Ti
-----------------------------------
@(e1, e2, ..., en) => @(T1, T2, ..., Tn)
```

**Note**: Anonymous tuples can be synthesized by synthesizing each element independently.

Example:
```
@(@true, @42, @3.14) => @(bool, u32, f32)
```

### Rule: Syn-AnonStruct
```
for all i: ei => Ti (for field fi = ei)
-------------------------------------------
@{f1 = e1, f2 = e2, ...} => @{f1: T1, f2: T2, ...}
```

**Note**: Anonymous structs can be synthesized by synthesizing each field value independently.

Example:
```
@{x = @42, y = @3.14} => @{x: u32, y: f32}
```

### Rule: Syn-List
```
n >= 1
e1 => T
for all i in [2..n]: ei => T' where T = T'
-------------------------------------------
@[e1, e2, ..., en] => @[T]
```

**Note**: Lists can be synthesized by synthesizing all elements and ensuring they have the same type. The first element determines the expected type. Empty lists cannot be synthesized.

Example:
```
@[@1, @2, @3] => @[u32]
@[] => error (CannotSynthesize - no way to infer element type)
```

### Rule: Syn-Set
```
n >= 1
e1 => T
for all i in [2..n]: ei => T' where T = T'
-------------------------------------------
@set{e1, e2, ..., en} => @set<T>
```

**Note**: Sets can be synthesized by synthesizing all elements and ensuring they have the same type. Empty sets cannot be synthesized.

Example:
```
@set{@true, @false} => @set<bool>
@set{} => error (CannotSynthesize)
```

### Rule: Syn-Map
```
n >= 1
k1 => K, v1 => V
for all i in [2..n]: ki => K' where K = K'
for all i in [2..n]: vi => V' where V = V'
--------------------------------------------
@map{k1 = v1, k2 = v2, ...} => @map<K, V>
```

**Note**: Maps can be synthesized by synthesizing all keys and values. Empty maps cannot be synthesized.

Example:
```
@map{@1 = @10, @2 = @20} => @map<u32, u32>
@map{} => error (CannotSynthesize)
```

### Rule: Syn-Tensor
```
shape = [d1, d2, ..., dn]
elements.len() == d1 * d2 * ... * dn
all elements synthesize to same type T
-----------------------------------------
@tensor[d1, d2, ...]{e1, e2, ...} => @tensor<T, n>
```

**Note**: Tensors can be synthesized if non-empty and all elements have the same type. Rank is the length of the shape vector.

### Rule: Syn-Some
```
e => T
---------------------------
@some(e) => @?T
```

**Note**: Explicit `some` constructor synthesizes an Option type.

Example:
```
@some(@42) => @?@u32
```

### Rule: Syn-Ok
```
e => T
---------------------------
@ok(e) => @!T
```

**Note**: Explicit `ok` constructor synthesizes a Result type.

Example:
```
@ok(@42) => @!@u32
```

### Rule: Syn-Data
```
---------------------------
@data "..." => @data
```

**Note**: Data literals synthesize as `@data` type.

### Rule: Syn-Error
```
msg : string literal
------------------------
@error msg => @error
```

**Note**: Error values always synthesize as `@error` type.

Example:
```
@error "oops" => @error
```

### Rule: Syn-None
```
-----------------
Cannot synthesize type for @none (needs context)
```

**Note**: `@none` can only be checked, not synthesized.

### Rule: Syn-Er
```
-----------------
Cannot synthesize type for @er (needs context)
```

**Note**: Explicit `er` constructor cannot be synthesized - requires Result type context.

## Checking Rules (e <= T)

Checking rules verify an expression against an expected type.

### Rule: Check-Subsume
```
e => T'
T' = T
-----------
e <= T
```

This is the key rule that allows synthesizing expressions to be checked.

### Rule: Check-Int (all integer types)
```
n : integer literal (as string)
n fits in u8 range
--------------------------------
@n <= @u8

n fits in i8 range
--------------------------------
@n <= @i8

... (similarly for u16, i16, u32, i32, u64, i64)

any integer literal
--------------------------------
@n <= @int
```

Examples:
```
@42 <= @u32     ok
@42 <= @int     ok
@-1 <= @u32     FAIL (negative, out of range for u32)
@-1 <= @i32     ok
@-1 <= @int     ok
@99999999999999999999 <= @int ok
@99999999999999999999 <= @u32 FAIL (out of range)
```

### Rule: Check-TypedInt
```
@n has type hint T_hint
@n <= T_hint  (validates literal fits in hinted type)
T_hint = T_expected OR can_widen(T_hint, T_expected)
------------------------------------------------
: T_hint / @n <= T_expected
```

**Note**: When an integer literal has a type hint, the hint is respected. The hinted type must either match or widen to the expected type.

Example:
```
: @u8 / @10 <= @u32    ok (u8 widens to u32)
: @u32 / @10 <= @u8    FAIL (u32 cannot narrow to u8)
```

### Rule: Check-Float
```
f : float literal
--------------------------------
@f <= @f32
```

### Rule: Check-Hex (all types)
```
h : hex literal (0x...)
h fits in u8 range
--------------------------------
@h <= @u8

... (similarly for u16, u32, u64, int)

h fits in 32 bits
--------------------------------
@h <= @f32  (as bit pattern)
```

**Note**: Hex literals can check against any integer type. For f32, they are interpreted as IEEE 754 bit patterns.

### Rule: Check-AnonTuple
```
(T1, T2, ..., Tn) is expected type
length matches
for all i: ei <= Ti
----------------------------------
(e1, e2, ..., en) <= (T1, T2, ..., Tn)
```

### Rule: Check-AnonStruct
```
{f1: T1, f2: T2, ...} is expected type
fields match (same names, same order)
for all i: ei <= Ti
-----------------------------------------
{f1 = e1, f2 = e2, ...} <= {f1: T1, f2: T2, ...}
```

### Rule: Check-List
```
[@T] is expected type
for all ei in elements: ei <= T
------------------------------
[e1, e2, ...] <= [@T]
```

### Rule: Check-EmptyList
```
-----------
[] <= [@T]
```

Empty lists check against any list type.

### Rule: Check-Map
```
@map<K, V> is expected type
for all (ki, vi) in entries: ki <= K and vi <= V
------------------------------------------------
@map{k1 = v1, k2 = v2, ...} <= @map<K, V>
```

### Rule: Check-Set
```
@set<T> is expected type
for all ei in elements: ei <= T
------------------------------
@set{e1, e2, ...} <= @set<T>
```

### Rule: Check-Tensor
```
@tensor<T, R> is expected type
rank matches R
element count matches product of shape dimensions
for all ei: ei <= T
-----------------------------------------------
@tensor[d1, ...]{e1, ...} <= @tensor<T, R>
```

### Rule: Check-None
```
--------------
@none <= @?T
```

`@none` checks against any option type.

### Rule: Check-Some
```
e <= T
--------------
@some(e) <= @?T
```

**Note**: Explicit `some` constructor checks payload against inner type.

### Rule: Check-Ok
```
e <= T
--------------
@ok(e) <= @!T
```

**Note**: Explicit `ok` constructor checks payload against inner type.

### Rule: Check-Er
```
e is error expression or data expression
--------------
@er(e) <= @!T
```

**Note**: Explicit `er` constructor for Results. Payload must be an error or data expression.

### Rule: Check-ResultErr (implicit Err wrapping)
```
----------------------
@error "msg" <= @!T
```

**Note**: Error expressions can check against any Result type as implicit Err wrapping.

Example:
```
: @!@u32 / @error "failed"
           |
      @error "failed" <= @error  ok
      @error "failed" <= @!@u32  ok (implicit Err wrapping)
```

### Rule: Check-Data
```
----------------------
@data "..." <= @data
```

### Rule: Check-Error
```
----------------------
@error "msg" <= @error
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
-----------------
e : T is valid

Global heap values can flow to global heap types:
e has global heap
T has global heap
-----------------
e : T is valid

Omitted heap is compatible with any heap:
e has omitted heap
-----------------
e : T is valid (for any heap on T)
```

Different heaps (local vs global) never unify. They are not compatible.

Examples:
```
: @u32 / @42   ok (both local heap)
: #u32 / #42   ok (both global heap)
: @u32 / 42    ok (omitted -> local)
: @u32 / #42   FAIL (heap mismatch)
: #u32 / @42   FAIL (heap mismatch)
```

## Type Error Codes

The type checker produces the following error codes:

- **T001**: Integer/hex literal out of range (synthesis)
- **T005-T012**: Integer out of range for specific types (u8, i8, u16, i16, u32, i32, u64, i64)
- **T013**: Cannot infer type for empty list / hex out of range for f32
- **T014**: Cannot infer type for empty set
- **T015**: Cannot infer type for empty map
- **T016**: Cannot synthesize type for None or Er
- **T017**: Cannot type-check expression with parse errors
- **T018**: List element type mismatch
- **T019**: Set element type mismatch
- **T020**: Map key type mismatch
- **T021**: Map value type mismatch
- **T022**: Type mismatch for primitive literal
- **T032**: General type mismatch (subsumption fallback)
- **T033**: List element heap mismatch
- **T034**: Set element heap mismatch
- **T035**: Map key heap mismatch
- **T036**: Map value heap mismatch
- **T037**: General heap mismatch
- **T038**: Tuple arity mismatch
- **T039**: Struct arity mismatch
- **T040**: Type hint mismatch / er payload must be error
- **T042**: Struct field order mismatch
- **T048**: Tensor rank mismatch
- **T049**: Tensor element count mismatch
- **T050**: Cannot infer type for empty tensor
- **T051**: Tensor element count mismatch (synthesis)
- **T052**: Tensor element type mismatch
- **T053**: Tensor element heap mismatch

## Design Decisions Summary

### 1. Empty collections

Empty collections can check against any element type:

```
[] <= [@T]           ok for any T
@set{} <= @set<T>    ok for any T
@map{} <= @map<K, V> ok for any K, V
```

### 2. Field order in anonymous structs

Field order must be correct:

```
{x: @u32, y: @bool} != {y: @bool, x: @u32}
```

### 3. Default numeric types

Bare numeric literals default to concrete types:
- Integer literals -> `@int` (arbitrary-precision)
- Float literals -> `@f32`
- Hex literals -> `@int` (arbitrary-precision)

Use explicit type hints for fixed-width numeric types:
```
: @u32 / @42                    ; unsigned 32-bit
: @i64 / @-42                   ; signed 64-bit
: @u8 / @255                    ; unsigned 8-bit
```

### 4. Explicit Option/Result constructors

Option and Result values use explicit constructors:
- `@none` for Option's None
- `@some(x)` for Option's Some
- `@ok(x)` for Result's Ok
- `@er(@error "msg")` for Result's Err
- `@error "msg"` can implicitly check against `@!T`

### 5. Anonymous composite type synthesis

Anonymous composite types can be synthesized when all their components can be synthesized:
- **Tuples**: `@(@true, @42)` synthesizes as `@(bool, u32)`
- **Structs**: `@{x = @42}` synthesizes as `@{x: u32}`
- **Lists**: `@[@1, @2, @3]` synthesizes as `@[u32]`
- **Sets**: `@set{@true, @false}` synthesizes as `@set<bool>`
- **Maps**: `@map{@1 = @10}` synthesizes as `@map<u32, u32>`

For collections (lists, sets, maps), all elements/keys/values must have the same type. The first element determines the expected type for the rest. Empty collections cannot be synthesized.

### 6. Data and Error types

- `@data` type represents opaque data values
- `@error` type represents error values with messages
- Error expressions can implicitly wrap into Result types

## Implementation Notes

### Bidirectional algorithm structure

```rust
fn synthesize(ctx, expr: ExprFull) -> Result<TypeAndHeap, TypeError>
fn check(ctx, expr: ExprFull, expected: TypeAndHeap) -> Result<(), TypeError>
```

### Type representation

The `Type` enum in `crates/datalove-datalit/src/tycheck/types.rs` includes:
- Primitives: Bool, U8, I8, U16, I16, U32, I32, U64, I64, F32, Int, String
- Composites: AnonTuple, AnonStruct
- Collections: List, Map, Set
- Wrappers: Option, Result
- Tagged: Atom, Term, Enum
- Special: Tensor, Data, Error

### Interaction with name resolution

Before typechecking:
1. Parse to AST
2. Run name resolution (resolve.rs)
3. Run typechecker with resolution context

Typechecker needs:
- Resolution results to look up types for named structs, tuples
- Type definitions from the type hint in `: type / expr` syntax
