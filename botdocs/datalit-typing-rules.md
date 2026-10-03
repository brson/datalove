# Datalit Typing Rules

This document specifies the type system for datalit expressions.
It follows a bidirectional typing discipline based on Dunfield & Krishnaswami (2013).

## Core Principles

- **Bidirectional**: Expressions either synthesize (=>) or check (<=) against types
- **Structural typing**: Anonymous types (tuples, structs) are compatible by structure
- **No nominal types**: There are no named struct or tuple types. A type alias is
  only a name for a structural type, and two aliases for the same structure are
  the same type (botspec Section 3.7)
- **Explicit at boundaries**: The `: type / expr` syntax provides type information at expression boundaries

## Type Equivalence

Two types are equivalent (T = T') when:

### Primitives
- `bool = bool`
- `u8 = u8`, `i8 = i8`
- `u16 = u16`, `i16 = i16`
- `u32 = u32`, `i32 = i32`
- `u64 = u64`, `i64 = i64`
- `index = index`, `offset = offset`
- `f32 = f32`, `f64 = f64`
- `int = int` (arbitrary precision integers)
- `string = string`
- `data = data`
- `error = error`

### Tuples (structural)
- `(T1, T2, ..., Tn) = (T1', T2', ..., Tn')` iff Ti = Ti' for all i
- Order matters
- Length must match

Examples:
```datalove
(u32, bool) = (u32, bool)  ok
(u32, bool) = (bool, u32)  FAIL (different order)
(u32, bool) = (u32)        FAIL (different length)
```

### Anonymous Structs (structural)
- `{f1: T1, f2: T2, ...} = {f1: T1', f2: T2', ...}` iff:
  - Same field names
  - Same field order
  - Ti = Ti' for all i

Examples:
```datalove
{x: u32, y: bool} = {x: u32, y: bool}  ok
{x: u32, y: bool} = {y: bool, x: u32}  FAIL (different order)
{x: u32} = {x: u32, y: bool}           FAIL (different fields)
```

### Collections
- `[T] = [T']` iff T = T'
- `%{K = V} = %{K' = V'}` iff K = K' and V = V'
- `#{T} = #{T'}` iff T = T'

### Option and Result
- `?T = ?T'` iff T = T'
- `!T = !T'` iff T = T'

### Tensor
- `[|T, R|] = [|T', R'|]` iff T = T' and R = R' (same element type and rank)

### Table
- `{| c1: T1, c2: T2, ... |} = {| c1': T1', c2': T2', ... |}` iff column names and types match in order

### Atom, Term, and Enum
- `atom A = atom A'` iff A and A' have the same name
- `term T(P) = term T'(P')` iff T = T' (same name) and P = P' (payload types equivalent)
- `enum { v1, v2, ... } = enum { v1', v2', ... }` iff variants match (sorted by name)

Each enum variant is either an atom (no payload) or a term (with payload type).

## No Implicit Widening

There is no implicit numeric widening, in datalit or datafun. A value of one
numeric type is never accepted where another is expected, however lossless the
conversion would be; datafun converts with the explicit `@` operator, and
datalit, having no operators, does not convert at all.

A type hint is what makes a datalit value one numeric type rather than
another, so the rule shows up as a rule about hints: a hint must be the type
that is expected of it (Check-Hinted).

```datalove
: [u16] / [: u8 / 42]                  FAIL (a u8 where a u16 is expected)
: [u16] / [42]                         ok
: {a: f64} / {a = : f32 / 1.0}         FAIL (floats do not widen either)
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
```datalove
: u32 / 42 => u32
```

### Rule: Syn-Bool
```
-----------------
true => bool

-----------------
false => bool
```

### Rule: Syn-Int
```
n : integer literal
--------------------------------
n => int
```

Integer literals without type context synthesize as `int` (arbitrary-precision).
Use explicit type hints for fixed-width integer types.

Examples:
```datalove
42 => int                              ok
99999999999999999999 => int            ok (arbitrary-precision)
: u32 / 42 => u32                      ok (explicit type hint)
: i32 / -42 => i32                     ok (explicit type hint)
```

### Rule: Syn-Float
```
f : float literal (contains decimal point)
------------------------------------
f => f64
```

Float literals without type context synthesize as `f64`.
Use a type hint for `f32`: `: f32 / 3.14`.

Example:
```datalove
1.0 => f64
3.14159 => f64
: f32 / 3.14 => f32
```

### Rule: Syn-Hex
```
h : hex literal (0x...)
--------------------------------
h => int
```

Hex literals synthesize as `int`. Use type hints for fixed-width types.

Example:
```datalove
0xFF => int
0xFFFFFFFFFF => int                    ok (arbitrary-precision)
: u64 / 0xFFFFFFFFFF => u64           ok (explicit type hint)
: f32 / 0xABABABAB => f32             ok (hex as bit pattern)
: f64 / 0x400921FB54442D18 => f64     ok (hex as f64 bit pattern)
```

### Rule: Syn-String
```
s : string literal
--------------------
"s" => string
```

Example:
```
"hello" => string
```

### Rule: Syn-AnonTuple
```
for all i: ei => Ti
-----------------------------------
(e1, e2, ..., en) => (T1, T2, ..., Tn)
```

Anonymous tuples are synthesized by synthesizing each element independently.

Example:
```datalove
(true, 42, 3.14) => (bool, int, f64)
```

### Rule: Syn-AnonStruct
```
for all i: ei => Ti (for field fi = ei)
-------------------------------------------
{f1 = e1, f2 = e2, ...} => {f1: T1, f2: T2, ...}
```

Anonymous structs are synthesized by synthesizing each field value independently.

Example:
```datalove
{x = 42, y = 3.14} => {x: int, y: f64}
```

### Rule: Syn-List
```
n >= 1
e1 => T
for all i in [2..n]: ei => T' where T = T'
-------------------------------------------
[e1, e2, ..., en] => [T]
```

Lists are synthesized by synthesizing all elements and ensuring they have
the same type. The first element determines the expected type. Empty lists
synthesize as `[()]` (list of unit).

Example:
```datalove
[1, 2, 3] => [int]
[] => [()]                              (empty list, unit element type)
```

### Rule: Syn-Set
```
n >= 1
e1 => T
for all i in [2..n]: ei => T' where T = T'
-------------------------------------------
#{e1, e2, ..., en} => #{T}
```

Sets are synthesized by synthesizing all elements and ensuring they have
the same type. Empty sets synthesize as `#{()}`.

Example:
```
#{true, false} => #{bool}
#{} => #{()}                       (empty set, unit element type)
```

### Rule: Syn-Map
```
n >= 1
k1 => K, v1 => V
for all i in [2..n]: ki => K' where K = K'
for all i in [2..n]: vi => V' where V = V'
--------------------------------------------
%{k1 = v1, k2 = v2, ...} => %{K = V}
```

Maps are synthesized by synthesizing all keys and values. Empty maps
synthesize as `%{() = ()}`.

Example:
```datalove
%{1 = 10, 2 = 20} => %{int = int}
%{} => %{() = ()}                   (empty map, unit key/value types)
```

### Rule: Syn-Tensor
```
shape = [d1, d2, ..., dn]
elements.len() == d1 * d2 * ... * dn
all elements synthesize to same type T
-----------------------------------------
[| e1 e2 ... |] => [|T, n|]
```

Tensors are synthesized if non-empty and all elements have the same type.
Rank is the length of the shape vector. Empty tensors synthesize as `tensor<(), N>`.

### Rule: Syn-Table
```
-----------------
Cannot synthesize type for table expressions (needs type hint)
```

Tables always require a type hint.

### Rule: Syn-Some
```
e => T
---------------------------
some e => ?T
```

The `some` constructor synthesizes an Option type.

Example:
```datalove
some 42 => ?int
some "hello" => ?string
```

### Rule: Syn-Ok
```
e => T
---------------------------
ok e => !T
```

The `ok` constructor synthesizes a Result type.

Example:
```
ok 42 => !int
```

### Rule: Syn-Data
```
e => T
---------------------------
data e => data
```

Data expressions synthesize as `data` type.

### Rule: Syn-Error
```
msg => T
------------------------
error msg => error
```

Error values always synthesize as `error` type.

Example:
```datalove
error "oops" => error
```

### Rule: Syn-None
```
-----------------
Cannot synthesize type for none (needs context)
```

`none` can only be checked, not synthesized.

### Rule: Syn-Er
```
-----------------
Cannot synthesize type for er (needs context)
```

`er` cannot be synthesized - requires Result type context.

### Rule: Syn-Atom
```
-----------------
atom A => atom A
```

### Rule: Syn-Term
```
e => T
-----------------
term A e => term A T
```

### Rule: Syn-Enum
```
-----------------
Cannot synthesize type for enum { v } (needs context)
```

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
n : integer literal
n fits in u8 range
--------------------------------
n <= u8

n fits in i8 range
--------------------------------
n <= i8

... (similarly for u16, i16, u32, i32, u64, i64, index, offset)

any integer literal
--------------------------------
n <= int
```

Examples:
```datalove
: u32 / 42                              ok
: int / 42                              ok
: u32 / -1                              FAIL (negative, out of range for u32)
: i32 / -1                              ok
: int / -1                              ok
: int / 99999999999999999999            ok
: u32 / 99999999999999999999            FAIL (out of range)
```

### Rule: Check-Hinted
```
e : ExprFull with type_hint = Some(T_hint)
T_hint = T
e.expr <= T
------------------------------------------------
: T_hint / e.expr <= T
```

Applies before every other checking rule. The hint has to be the expected
type exactly; it is not a conversion. Parentheses keep a hint of their own
where they are themselves under one, so `: u32 / (: u8 / 1)` is a `u8` where a
`u32` is expected, and fails.

Example:
```datalove
: [u16] / [: u8 / 10]                  FAIL (u8 is not u16)
: [u8] / [: u8 / 10]                   ok
```

### Rule: Check-Float
```
f : float literal
--------------------------------
f <= f32

f : float literal
--------------------------------
f <= f64
```

Float literals check against both `f32` and `f64`.

### Rule: Check-Hex
```
h : hex literal (0x..., non-negative)
h fits in u8 range
--------------------------------
h <= u8

... (similarly for u16, u32, u64, index)

any hex literal
--------------------------------
h <= int

h fits in 32 bits (non-negative)
--------------------------------
h <= f32  (as IEEE 754 bit pattern)

h fits in 64 bits (non-negative)
--------------------------------
h <= f64  (as IEEE 754 bit pattern)
```

Hex literals can check against unsigned integer types, `int`, and float
types (as bit patterns). They cannot check against signed integer types.

Examples:
```datalove
: u8 / 0xFF                             ok
: u32 / 0xFFFFFFFF                      ok
: int / 0xFFFFFFFFFF                    ok
: f32 / 0x3F800000                      ok (IEEE 754 bit pattern for 1.0)
: f64 / 0x3FF0000000000000              ok (IEEE 754 bit pattern for 1.0)
: i32 / 0xFF                            FAIL (hex cannot check against signed)
: i32 / -0x1                            FAIL (a hex literal takes no sign)
```

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
[T] is expected type
for all ei in elements: ei <= T
------------------------------
[e1, e2, ...] <= [T]
```

Empty lists check against any list type.

### Rule: Check-Map
```
%{K = V} is expected type
for all (ki, vi) in entries: ki <= K and vi <= V
------------------------------------------------
%{k1 = v1, k2 = v2, ...} <= %{K = V}
```

### Rule: Check-Set
```
#{T} is expected type
for all ei in elements: ei <= T
------------------------------
#{e1, e2, ...} <= #{T}
```

### Rule: Check-Tensor
```
[|T, R|] is expected type
rank matches R
element count matches product of shape dimensions
for all ei: ei <= T
-----------------------------------------------
[| e1 ... |] <= [|T, R|]
```

### Rule: Check-Table
```
{| c1: T1, c2: T2, ... |} is expected type
column count matches
column names match in order
for all rows: row element count matches column count
for all row elements: ei <= Ti (column type)
-----------------------------------------------
{| headers; rows... |} <= {| c1: T1, c2: T2, ... |}
```

### Rule: Check-None
```
--------------
none <= ?T
```

`none` checks against any option type.

### Rule: Check-Some
```
e <= T
--------------
some e <= ?T
```

The `some` constructor checks payload against inner type.

### Rule: Check-Ok
```
e <= T
--------------
ok e <= !T
```

The `ok` constructor checks payload against inner type.

### Rule: Check-Er
```
e <= error
--------------
er e <= !T
```

The `er` constructor for Results. The payload is the error the result holds,
and is checked against `error`, so it is written `er error "failed"`. An
`error` does not check against `!T` by itself: there is no implicit wrapping,
in datalit or datafun.

### Rule: Check-Data
```
e => T
----------------------
data e <= data
```

### Rule: Check-Error
```
e => T
----------------------
error e <= error
```

The payload has to synthesize a type of its own, which is the type the value
carries, so `data none` is an error and `data : ?u32 / none` is not.

### Rule: Check-Atom
```
----------------------
atom A <= atom A

atom A is a variant of E
----------------------
atom A <= E
```

### Rule: Check-Term
```
e <= T
----------------------
term A e <= term A T

term A T is a variant of E    e <= T
----------------------
term A e <= E
```

### Rule: Check-Enum
```
v <= E    E is an enum type
----------------------
enum { v } <= E
```

An atom or term checks against an enum listing it without the
`enum { }` wrapper, as it does in datafun.

## Type Error Codes

The type checker produces the following diagnostic codes:

| Code | Context | Description |
|------|---------|-------------|
| T005 | int/hex | u8 out of range |
| T006 | int/hex | i8 out of range |
| T007 | int/hex | u16 out of range |
| T008 | int/hex | i16 out of range |
| T009 | int/hex | u32 out of range |
| T010 | int/hex | i32 out of range |
| T011 | int/hex | u64 out of range |
| T012 | int/hex | i64 out of range |
| T013 | hex | f32 bit pattern out of range |
| T014 | hex | f64 bit pattern out of range |
| T015 | int/hex | index out of range |
| T016 | synth | Cannot synthesize type for none, er or an enum literal |
| T017 | synth | Cannot type-check expression with parse errors |
| T018 | synth | List element type mismatch; also: table requires type hint |
| T019 | synth | Set element type mismatch |
| T020 | synth | Map key type mismatch |
| T021 | synth | Map value type mismatch |
| T022 | check | Primitive literal type mismatch (bool, string) |
| T032 | check | General type mismatch (subsumption fallback) |
| T038 | check | Tuple arity mismatch |
| T039 | check | Struct arity mismatch |
| T040 | check | Type hint mismatch / er payload must be error |
| T042 | check | Struct field order mismatch |
| T048 | check | Tensor rank mismatch |
| T049 | check | Tensor element count mismatch |
| T051 | synth | Tensor element count mismatch |
| T052 | synth | Tensor element type mismatch |
| T054 | check | Table column count mismatch |
| T055 | check | Table column name mismatch |
| T056 | check | Table row column count mismatch |
| T057 | check | Atom or term is not the expected atom, term or enum variant |
| T058 | check | Enum literal checked against a type that is not an enum |

## Design Decisions Summary

### 1. Empty collections

Empty collections synthesize with unit element type, and check against any element type:

```datalove
[] => [()]                   (synthesis: unit element type)
: [u32] / []                 ok (checking: any element type)
#{} => #{()}            (synthesis)
: #{u32} / #{}          ok (checking)
%{} => %{() = ()}        (synthesis)
: %{u32 = string} / %{}  ok (checking)
```

### 2. Field order in anonymous structs

Field order must match:

```datalove
{x: u32, y: bool} != {y: bool, x: u32}
```

### 3. Default numeric types

Bare numeric literals synthesize to concrete types:
- Integer literals -> `int` (arbitrary-precision)
- Float literals -> `f64`
- Hex literals -> `int` (arbitrary-precision)

Use explicit type hints for fixed-width numeric types:
```datalove
: u32 / 42                              unsigned 32-bit
: i64 / -42                             signed 64-bit
: u8 / 255                              unsigned 8-bit
: f32 / 3.14                            32-bit float
```

### 4. Explicit Option/Result constructors

Option and Result values use explicit constructors:
- `none` for Option's None
- `some x` for Option's Some
- `ok x` for Result's Ok
- `er error "msg"` for Result's Err

### 5. Anonymous composite type synthesis

Anonymous composite types can be synthesized when all their components can be synthesized:
- **Tuples**: `(true, 42)` synthesizes as `(bool, int)`
- **Structs**: `{x = 42}` synthesizes as `{x: int}`
- **Lists**: `[1, 2, 3]` synthesizes as `[int]`
- **Sets**: `#{true, false}` synthesizes as `#{bool}`
- **Maps**: `%{1 = 10}` synthesizes as `%{int = int}`

For collections (lists, sets, maps), all elements/keys/values must have the
same type. The first element determines the expected type for the rest.

### 6. Data and Error types

- `data` type represents opaque data values
- `error` type represents error values with messages
- An error becomes a Result's failure only under `er`

### 7. Hex literal type restrictions

Hex literals check against unsigned integer types (`u8`, `u16`, `u32`, `u64`,
`index`), `int`, and float types (`f32`, `f64` as IEEE 754 bit patterns).
They do NOT check against signed integer types.

## Implementation Notes

### Bidirectional algorithm structure

```rust
fn synthesize(ctx, expr: ExprFull) -> Result<Type, TypeError>
fn check(ctx, expr: ExprFull, expected: Type) -> Result<(), TypeError>
```

### Type representation

The `Type` enum in `crates/datalove-datalit/src/tycheck/types.rs` includes:
- Primitives: Bool, U8, I8, U16, I16, U32, I32, U64, I64, Index, Offset, F32, F64, Int, String
- Composites: AnonTuple, AnonStruct
- Collections: List, Map, Set
- Wrappers: Option, Result
- Tagged: Atom, Term, Enum
- Special: Tensor, Table, Data, Error
- Type parameters: Var (a type parameter of the enclosing generic function)
