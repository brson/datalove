# Datalove Language Specification

## Contents

- [1. Introduction](#user-content-1-introduction)
- [2. Lexical Conventions](#user-content-2-lexical-conventions)
- [3. Types](#user-content-3-types)
- [4. Type Hints](#user-content-4-type-hints)
- [5. Copy and Linear Types](#user-content-5-copy-and-linear-types)
- [6. Expressions](#user-content-6-expressions)
- [7. Place Expressions and Indexing](#user-content-7-place-expressions-and-indexing)
- [8. Statements](#user-content-8-statements)
  - [8.4 Const Parameters](#user-content-84-const-parameters)
  - [8.5 Generic Functions](#user-content-85-generic-functions)
- [9. Module System](#user-content-9-module-system)
  - [9.4 Native Riders](#user-content-94-native-riders)
- [10. Numeric Widening](#user-content-10-numeric-widening)
- [11. Ownership Analysis](#user-content-11-ownership-analysis)
- [12. Bidirectional Type Inference](#user-content-12-bidirectional-type-inference)
- [Appendix A. Command-Line Interface](#user-content-appendix-a-command-line-interface)
- [Appendix B. Unimplemented Features](#user-content-appendix-b-unimplemented-features)

## 1. Introduction

Datalove is a statically-typed scripting language designed for data manipulation
and incremental computation. The language emphasizes safety through a linear
type system that tracks ownership, preventing use-after-move errors at compile
time.

The language has a three-layer design:

- **Datalit** (.dlt) - A pure data literal sublanguage for representing values.
- **Datafun** (.dfs, .dfm, .dli) - A pure functional layer with functions,
  modules, native rider interfaces, and control flow.
- **Full Datalove** (.dls, .dlm) - Procedures and mutable objects. (Not yet
  implemented.)

This specification describes the Datalit and Datafun layers.

### 1.1 Notation

In syntax descriptions, the following conventions apply:

- `monospace` denotes literal syntax
- *italics* denote syntactic categories
- `[...]` denotes optional elements
- `...` denotes repetition

## 2. Lexical Conventions

### 2.1 Keywords

The following identifiers are reserved:

```datalove
and       atom      break     case      continue  data
default   else      end       enum      error     er
false     for       fun       icall     if        import
in        let       loop      match     mut       none
not       ok        or        out       ref       require
ret       some      table     term
true      type      var       while     xor
```

### 2.2 Literals

**Integers** may be written in decimal or hexadecimal:

```datalove
42
0xFF
```

Bare integer literals synthesize as `int` (arbitrary-precision).
Use a type hint for fixed-width types: `: u32 / 42`.

Underscores may group digits, and say nothing about the value:

```datalove
1_000_000
0xFF_FF
```

A separator goes between digits, so a literal begins and ends with one.
`_1` is a name rather than a number, and `1_` is neither.

**Floating-point** numbers use decimal notation, with an optional exponent:

```datalove
3.14
1.0e300
2.5e-10
1e-7
6.022E23
```

A point or an exponent is what makes a literal a float; `42` is an integer
and `42.0` and `4.2e1` are floats. The exponent's sign may be written or
left out, and its marker may be `e` or `E`. The fraction may be left out
when there is an exponent: `1e-7` needs no `.0`. Separators group the
digits here too, in any of the three runs: `1_0.000_1e1_0`.

This is also how floats are printed, so a value that comes out of the
compiler can be typed back into it. Magnitudes from `1e-5` up to `1e16`
print positionally, the rest with an exponent, and every finite float
prints with a point or an exponent so that none of them reads as an
integer: `42.0`, not `42`.

**Strings** are enclosed in double quotes:

```datalove
"hello, world"
```

**Booleans** are `true` and `false`.

### 2.3 Comments

Line comments begin with `//` and extend to end of line.
Block comments are delimited by `/*` and `*/` and may be nested.

## 3. Types

### 3.1 Primitive Types

| Type | Description |
|------|-------------|
| `bool` | Boolean value |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers |
| `i8`, `i16`, `i32`, `i64` | Signed integers |
| `index` | Addressable size of indexed collections |
| `offset` | Signed addressable size etc. |
| `f32`, `f64` | Floating-point numbers |
| `int` | Arbitrary-precision integer |
| `string` | UTF-8 string |

The `index` and `offset` types are 32-bit by default, or 64-bit when the
`index-64` feature is enabled.
`index` is not the same as the platform pointer size,
which is not exposed to the language.
The size of `index` is less than or equal to the platform pointer size.

### 3.2 Collection Types

**List.** An ordered sequence of elements.

```datalove
[T]              // type
[1, 2, 3]        // literal
```

**Map.** A key-value mapping.

```datalove
%{K = V}         // type
%{ 0 = 5 }       // literal
```

**Set.** An unordered collection of unique elements.

```datalove
#{T}             // type
#{ 1, 2, 3 }     // literal
```

**Table.** A columnar data structure with named columns.

```datalove
{| col1: T1, col2: T2 |}    // type
```

Table literals use a line-oriented syntax:

```datalove
{|
  x, y           // column names
  1, 2           // row 1
  3, 4           // row 2
|}
```

Semicolons permit single-line format: `{| x, y; 1, 2; 3, 4 |}`.

Column projections (e.g., `table.x`) yield a list view that cannot be moved or
mutated, but can be passed to `ref` parameters.

**Tensor.** A multi-dimensional array with fixed shape.

```datalove
[|T, N|]                 // type: element type T, rank N
[| 1 2 3 |]             // 1D literal (shape inferred: [3])
[| 1 2 3, 4 5 6 |]      // 2D literal (shape inferred: [2, 3])
[| 1 2, 3 4,, 5 6, 7 8 |]  // 3D literal (shape inferred: [2, 2, 2])
```

Type hints specify element type and rank:

```datalove
: [|u32, 2|] / [| 1 2, 3 4 |]
```

Shape is inferred from the multi-comma structure: spaces separate elements
along the innermost axis, `,` separates rows (2nd axis), `,,` separates
slabs (3rd axis), `,,,` separates blocks (4th axis), etc. When the outermost
dimension is 1, a trailing comma run preserves rank: `[| 1 2 3, |]` is a
rank-2 tensor with shape [1, 3].

Tensor literals can be created and stored, but element access and tensor
operations (indexing, transpose, slice, reshape) are not yet exposed to the
language. The runtime supports these operations internally.

### 3.3 Aggregate Types

**Tuple.** An ordered sequence of heterogeneous values.

```datalove
(bool, u32)              // type
(true, 42)               // literal
()                       // unit type and value
```

**Struct.** A collection of named fields.

```datalove
{ x: f32, y: f32 }       // type
{ x = 1.0, y = 2.0 }     // literal
```

### 3.4 Option and Result

**Option** represents an optional value:

```datalove
?T                       // type: value or none
some primary             // wrap value
none                     // absent value
```

**Result** represents success or failure:

```datalove
!T                       // type: value or error
ok primary               // wrap success value
er primary               // wrap error value
error "message"          // error literal
```

The `some`, `ok`, `er`, `data`, and `error` keywords take a *primary* expression
as their payload: a literal, variable, function call, or parenthesized expression.
Binary operator expressions require parentheses:

```datalove
some 42                  // ok: literal is primary
some(a +? b)             // ok: parenthesized expression
ok result                // ok: variable is primary
ok(x +! y)              // ok: parenthesized expression
er(error "msg")          // ok: parenthesized expression
```

### 3.5 Data Type

The `data` type is a universal container that can hold any value:

```datalove
data 42
data : int / 100
data(a + b)              // parenthesized for binop payload
```

The `error` type is an existential error value:

```datalove
error "message"
error(some_expr)         // parenthesized for non-primary payload
```

Both `data` and `error` take a primary expression as their payload (see Section 3.4).
Any type coerces to `data`.

### 3.6 Atom, Term, and Enum Types

**Atom.** A named unit type with no payload.

```datalove
atom Red                 // type and value
```

An atom is both a type and a value. Two atoms are the same type if they have
the same name.

**Term.** A named type with a typed payload.

```datalove
term Foo int             // type
term Foo 42              // value
```

Two terms are the same type if they have the same name and payload type.

**Enum.** A closed union of atom and term variants.

```datalove
enum { atom Red, atom Blue, term Custom string }   // type
```

Enum variants are matched by name. Atoms and terms can stand alone as types,
or combine into enums.

**Coercion.** The `@` operator widens an atom or term into a compatible enum
type:

```datalove
let c: enum { atom Red, atom Blue } = (atom Red)@
```

**Match.** Enums are destructured with `match` (see Section 8.6).

### 3.7 Type Aliases

Type aliases provide names for structural types:

```datalove
type Point: { x: f32, y: f32 }
type Age: u32
```

Aliases are purely syntactic; they introduce no new types. Enum types are
commonly given aliases:

```datalove
type Color: enum { atom Red, atom Blue, term Custom string }
```

## 4. Type Hints

Type hints specify expected types for expressions:

```datalove
: type / expression
```

Examples:

```datalove
: u32 / 42
: [i32] / [1, 2, 3]
: f32 / 0xABABABAB     // hex as bit pattern
```

A bare name in type position is a type alias (Section 3.7), or, within a generic
function's signature or body, one of its type parameters (Section 8.5). Data
literals have neither, so a name in a data literal's type is always an error.

## 5. Copy and Linear Types

Types are classified as *copy* or *linear*.

**Copy types** can be freely duplicated: `bool`, fixed-width integers (`u8`
through `u64`, `i8` through `i64`), `index`, `offset`, `f32`, `f64`.
Atoms are always copy. Terms are copy if their payload type is copy.
Enums are copy if all variant payloads are copy.

**Linear types** have move semantics: `int`, `string`, `[T]`, `%{K = V}`,
`#{T}`, `{| ... |}`, `[|T, N|]`, `data`, `error`. Terms and enums with
linear payloads are linear.

A **type parameter** is linear, whatever it is instantiated with, because the
caller may supply a linear type and the function is compiled once for all of
them (Section 8.5).

A linear value can be used exactly once. After a value is moved, subsequent
uses are compile-time errors:

```datalove
let x: int = 42
let y = x              // x is moved
let z = x              // error: use of moved value
```

## 6. Expressions

### 6.1 Operators

Operators listed from highest to lowest precedence:

| Precedence | Operators | Description |
|------------|-----------|-------------|
| 1 | `()` | Grouping |
| 2 | `-` `-?` `-!` `not` `some` `ok` `er` `data` `error` | Unary prefix |
| 3 | `.field` `.0` `[i]` `@` `?` `!` | Postfix (field, index, adapt, try) |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Comparison |
| 7 | `and` | Logical AND |
| 8 | `or` `xor` | Logical OR/XOR |

Postfix binds looser than unary prefix, so a postfix operator applies to the
prefix expression as a whole: `-x@` is `(-x)@`. The payload keywords `some`,
`ok`, `er`, `data` and `error` are the exception, taking postfix onto their
payload instead, so `some x@` is `some (x@)`.

### 6.2 Arithmetic

**Bare arithmetic** (`+`, `-`, `*`, `/`) behaves differently by type:

- **Floats**: Operations return the same float type. Division is permitted.
- **Bigints**: Addition, subtraction, multiplication, and unary negation
  return `int`. Division is not permitted (use checked variants).
- **Fixed integers**: Bare arithmetic is not permitted. Use `@` to widen
  operands to `int` for bigint semantics, or use checked/optional operators
  (`+!`, `-!`, `*!`, `/!` or `+?`, `-?`, `*?`, `/?`) for overflow handling.

**Checked arithmetic** (`+!`, `-!`, `*!`, `/!`) operates on fixed integers and
returns a result type. On overflow or division by zero, the function
early-returns an error:

```datalove
fun add(a: u32, b: u32): !u32
    ret ok (a +! b)
end fun
```

Bigint division `/!` returns `!int`.

**Optional arithmetic** (`+?`, `-?`, `*?`, `/?`) operates on fixed integers and
returns an option type. On overflow or division by zero, the function
early-returns `none`:

```datalove
fun add(a: u32, b: u32): ?u32
    ret some (a +? b)
end fun
```

Bigint division `/?` returns `?int`. Unary `-?` is permitted only for signed
fixed integers.

Integer division truncates toward zero, so `-7 /? 2` is `-3`. There is no
remainder operator; derive one from the quotient, which gives it the sign of
the dividend, or use `int.rem_checked`.

### 6.3 Comparison

```datalove
.<    less than
.>    greater than
<=    less than or equal
>=    greater than or equal
==    equal
!=    not equal
```

All comparison operators return `bool`. Both operands must already have the
same numeric type; there is no implicit widening, and `bool`, `string` and
the collection types are not comparable with these. Compare strings with
`string.eq` and `string.cmp`.

### 6.4 Logical Operators

```datalove
and   logical AND
or    logical OR
xor   logical XOR
not   logical NOT (unary)
```

All require `bool` operands and return `bool`.

### 6.5 Try Operators

The postfix `?` operator unwraps an option, early-returning `none` on failure:

```datalove
fun get_value(opt: ?i32): ?i32
    let x = opt?           // early-return if none
    ret some (x + 1)
end fun
```

The postfix `!` operator unwraps a result, early-returning the error on
failure:

```datalove
fun parse(s: string): !i32
    let n = do_parse(s)!   // early-return if error
    ret ok n
end fun
```

### 6.6 Operator Argument Semantics

All operators treat their operands as immutable references. Operands are not
consumed:

```datalove
let x: int = 42
let a = x + 1       // x is cloned for the operation
let b = x + 2       // x can be used again
ret x               // x is still valid
```

### 6.7 Intrinsic Calls

Intrinsics are low-level operations that compile to machine instructions:

```datalove
icall intrinsic_name(args)
```

Available intrinsics include bitwise operations (`bitnot_u32`, `bitand_u32`,
`bitor_u32`, `bitxor_u32`), shifts (`shl_u32`, `shr_u32`), bit counting
(`popcount_u32`, `clz_u32`, `ctz_u32`), byte manipulation (`swap_bytes_u32`,
`reverse_bits_u32`), wrapping arithmetic (`add_wrapping_u32`,
`sub_wrapping_u32`, `mul_wrapping_u32`), and type reinterpretation
(`u32_to_i32`, `i32_to_u32`).

### 6.8 Adapt Operator

The postfix `@` operator performs explicit clone and/or widening conversions.
It requires a type context (expected type) to determine the target type.

**Clone**: For linear types, `@` creates a deep copy:

```datalove
let msg = "hello"
let a = consume(msg@)   // clone msg, original stays valid
let b = consume(msg)    // msg is still available
```

**Widen**: For fixed integers, `@` widens to a larger type:

```datalove
let n: u8 = 42
let x: int = n@         // widen u8 to int
```

**Cross-sign widen**: Unsigned integers can widen to larger signed types:

```datalove
let n: u8 = 255
let x: i16 = n@         // u8 widens to i16 (value fits)
```

**Atom/term to enum**: An atom or term widens to a compatible enum type:

```datalove
type Color: enum { atom Red, atom Blue }
let c: Color = (atom Red)@   // atom widens to enum
```

Valid widening chains:

- Same-sign: `u8` -> `u16` -> `u32` -> `u64` -> `int`
- Same-sign: `i8` -> `i16` -> `i32` -> `i64` -> `int`
- Cross-sign: `u8` -> `i16`, `i32`, `i64`, `int`
- Cross-sign: `u16` -> `i32`, `i64`, `int`
- Cross-sign: `u32` -> `i64`, `int`
- Index/offset: `index` -> `int`, `offset` -> `int`

The `@` operator cannot synthesize a type; it must appear in a context where
the expected type is known. Those are:

- A `let` or `var` binding with a type annotation.
- Return position in a function with a declared return type.
- An argument to a function call.
- An operand of an operator whose operands have the same type as its result:
  `+`, `-`, `*` and `/` where that type is one they accept, the checked and
  optional operators, and `and`, `or` and `xor`. The expected type of the
  whole expression reaches both operands, so `let c: int = a@ + b@` widens
  both to `int`.
- An operand of a comparison, when the other operand has a type of its own.
  A comparison returns `bool`, which says nothing about its operands, so the
  type comes from across the operator instead: `a@ .< b` takes `b`'s type,
  and `a@ .< 0` takes the literal's. With `@` on both sides there is nothing
  to take, and `a@ .< b@` is an error.

The expected type has to be one the operator accepts, so `@` does not make an
operator available on a type that does not have it. `let r: u32 = a@ + b@`
is still an error, because `@` leaves the operands `u32` and bare `+` is not
defined there; widen to `int`, or use `+!` or `+?`.

## 7. Place Expressions and Indexing

### 7.1 Place vs Value Expressions

Expressions are either **place expressions** (denoting a storage location) or
**value expressions** (producing a fresh owned value).

Place expressions:
- Variables: `x`
- Field access: `x.field`
- Tuple element: `x.0`
- Index: `a[i]?`, `a[i]!`, `m[key]?`, `m[key]!`
- Chains: `a[i]?.field`, `a.b[0]?`

Value expressions:
- Literals, function calls, arithmetic results, constructors, `@` clones

Field access and indexing can also follow value expressions (`f(x).field`,
`f(x)[i]?`), but these produce values, not places -- the result cannot be
used as a `set` target or `mut` argument.

### 7.2 Field Access

Struct fields and tuple elements are accessed with dot notation:

```datalove
let p = { x = 1.0, y = 2.0 }
let a = p.x                    // struct field

let t = (true, 42)
let b = t.0                    // tuple element by index
```

Chained access navigates nested structures:

```datalove
let inner = outer.a.b.c
```

Field access on a linear-type field requires the place to be in a reference
context (`ref` param, `mut` param, binop operand, `set` LHS). In consume
context (`let`, `ret`, `in` param), linear fields require explicit `@` clone:

```datalove
let t = (1, 2)
let x = t.0                   // error: int is linear
let x = t.0@                  // ok: explicit clone
foo(ref t.0)                   // ok: borrow
```

Copy-type fields (bool, fixed integers, floats) are freely extracted.

### 7.3 Fallible Indexing

The `[]` operator on lists, maps, and tensors is fallible: the index may be
out of bounds or the key may be absent. The `?` or `!` postfix resolves the
failure strategy:

```datalove
a[i]?      // early-return none on out-of-bounds
a[i]!      // early-return error on out-of-bounds
m[key]?    // early-return none on missing key
m[key]!    // early-return error on missing key
t[i]?      // early-return none on out-of-bounds (tensor axis-0)
```

Bare `a[i]` without `?` or `!` is a type error in read context. There is no
infallible/panicking index variant.

The enclosing function's return type determines which variant is valid: `?`
requires the function to return `?R`, `!` requires `!R`.

```datalove
fun get_elem(a: [u32], i: index): ?u32
    ret some (a[i]?)
end fun

fun get_elem_r(a: [u32], i: index): !u32
    ret ok (a[i]!)
end fun
```

Lists and tensors are indexed by `index`. Maps are indexed by their key type.
For tensors, each `[i]?` indexes along axis 0, reducing rank by 1.
A rank-1 tensor indexed produces the element type; a rank-N (N>1) tensor
indexed produces a rank-(N-1) sub-tensor **view**. A view is a non-owning
tensor struct that aliases the parent tensor's data buffer. Indexing into
a non-copy element in consume context requires `@` (clone): `t[i]?@`.

**View type restrictions:** Views can be passed to `ref` params but not to
`mut` or `out` params. Whole-value replacement of a view (`set row = ...`)
would overwrite the view struct without affecting the parent's data, causing
leaks. The `set t[i]? = new_row` form is likewise rejected for rank > 1
tensors. Element-level mutation through views (e.g. `set row[j]? = val`)
is semantically correct but currently also rejected because the typechecker
cannot distinguish it from whole-view replacement at the call site. The view
concept is general; tensor sub-views are the only instance today.

### 7.4 Index and Field Chains

Index and field steps can be chained:

```datalove
a[i]?.field           // index into list, then access field
m[key]?.0             // index into map, then access tuple element
a[i]?.b[j]?           // index, field, index again
```

Each `?` or `!` in the chain is an independent early-return point, checked
left to right. If any check fails, the function early-returns immediately
with no mutation.

### 7.5 Destination Contexts

A place expression's behavior depends on its destination context:

| Context | Behavior | Example |
|---------|----------|---------|
| `let` / `ret` / `in` param | Copy or clone value out | `let x = a[i]?` |
| `ref` param | Immutable borrow | `foo(ref a[i]?)` |
| `mut` param | Mutable borrow (no views) | `foo(mut a[i]?)` |
| `set` LHS | Mutation target (no views) | `set a[i]? = 5` |
| binop operand | Immutable borrow | `a[i]? + 1` |

In consume context (`let`, `ret`, `in` param), copy-type elements are copied
out freely. Linear-type elements require explicit `@` clone -- moving an
element out of its container would leave a hole.

### 7.6 Parallel with Checked Arithmetic

Fallible indexing mirrors checked arithmetic:

| Operation | `?` variant | `!` variant |
|-----------|-------------|-------------|
| Arithmetic | `a +? b` returns `?T` | `a +! b` returns `!T` |
| Indexing | `a[i]?` early-returns `none` | `a[i]!` early-returns `error` |

## 8. Statements

### 8.1 Bindings

**Let** binds an immutable value:

```datalove
let x: u32 = 42
let y = compute()      // type inferred
```

**Var** binds a mutable slot:

```datalove
var x: u32 = 0
var y: i32             // uninitialized; must set before use
```

**Const** binds a value the compiler evaluates. It is valid at script top
level, in a script or module function body, and at module top level, where it
is in scope for every function in the module regardless of where it is written:

```datalove
const LIMIT: u32 = : u32 / 10
const DOUBLED: u32 = LIMIT +! LIMIT     // may name a const written above it

fun clamped(x: u32): u32
    const L: u32 = LIMIT                // and so may a function-level const
    ret L
end fun
```

A const expression may only name other consts, since it is evaluated before
anything a parameter or a `let` is bound to exists. Naming a parameter or a
`let` is an error (F058) even where a const of the same name is in scope.
It may call functions, whose parameters are bound by the call. A module-level
const has no enclosing function, so it cannot use the early-return operators.

**Evaluation** runs the expression at compile time, by the same means that runs
it at run time, so anything a function can compute a const can hold: not only
scalars but `string`, `int`, and collections.

```datalove
const MSG: string = "hello"
const BIG: int = 99999999999999999999
const LST: [int] = [1, 2, 3]
```

A const names a value, not a place, so reading one does not consume it. Each
mention produces a value of its own, and a const of a linear type can therefore
be named as often as it is wanted, wherever it is written:

```datalove
const LST: [int] = [1, 2, 3]
let a = LST
let b = LST                    // no `@` needed: reading a const does not move it
```

**Ordering.** A module-level const is evaluated after the functions it calls are
compiled, and before the functions that name it. Functions naming no
module-level const are therefore compiled first, which is what lets a const call
a function while another function reads that const. If a const calls a function
that itself names a module-level const, the two depend on each other and the
compiler reports it rather than choosing an order.

**Set** mutates a var binding, mutable parameter, or indexed/chained target:

```datalove
set x = x + 1
set a[i]? = 5
set a[i]?.field = 10
set m[key]? = v            // update: fail if key absent
set m[key] = v             // upsert: insert or overwrite
```

The `set` target is a place expression (see Section 7). The root must be
`var` or `mut`. For indexed targets, the `?` or `!` on each index step
provides early-return on failure; the write only happens if all checks pass.

**Map upsert.** Bare `set m[key] = v` (without `?` or `!`) is valid only for
maps. It inserts if the key is absent, overwrites if present. Lists reject
bare index on `set` LHS -- list elements must exist to be overwritten.

**Evaluation order** for `set` with indexed targets:

1. Navigate the LHS chain -- evaluate index subexpressions, perform
   bounds/existence checks, early-return on failure.
2. Evaluate the RHS.
3. Drop the old value at the target (if linear type).
4. Store the new value.

The RHS can ref-borrow the same collection (e.g., `set a[0]? = a[1]?@`)
but cannot take mutable or consuming access to it.

### 8.2 Functions

Function definition:

```datalove
fun name(param1: T1, param2: T2): ReturnType
    // body
    ret value
end fun
```

Void functions omit the return type and may omit `ret`:

```datalove
fun log(msg: string)
    // body
end fun
```

### 8.3 Parameter Modes

All parameters are passed by reference. The mode determines permitted
operations:

| Mode | Syntax | Semantics |
|------|--------|-----------|
| `in` | `x: T` | Caller transfers ownership; callee consumes |
| `ref` | `ref x: T` | Caller retains ownership; callee reads only |
| `mut` | `mut x: T` | Caller retains ownership; callee may mutate |
| `out` | `out x: T` | Callee initializes; caller receives value |

**Restrictions:**
- `ref`, `mut`, and `out` parameters cannot be moved.
- `out` parameters must be initialized before the function returns.
- `ref` parameters cannot be passed to `mut` parameters.
- `out` parameters must be written as a whole, not field-by-field.
- View types (e.g. tensor sub-views from rank > 1 indexing) cannot be
  passed to `mut` or `out` parameters. Use `ref` instead.

**Call-site markers.** Every argument repeats its parameter's mode, so that
borrowing and mutation are visible where the call is written rather than only
in the callee's signature. `in` is written by omitting the marker:

```datalove
fun mixed(a: u32, ref b: u32, mut c: u32): u32

mixed(x, ref y, mut z)
```

A marker that disagrees with the declared mode is an error (F057), including
a missing marker for a `ref`, `mut` or `out` parameter and a marker on an
`in` parameter. Intrinsics take every argument by value, so `icall` arguments
never carry a marker.

### 8.4 Const Parameters

The `const` modifier declares a parameter whose value must be known at compile
time:

```datalove
fun repeat(const n: i32, s: string): string
    var result = ""
    var i: i32 = 0
    loop while i .< n
        set result = result + s
        set i = i +! 1
    end loop
    ret result
end fun

const COUNT = 3
let x = repeat(COUNT, "ab")  // COUNT is a const binding
```

**Semantics:**
- The argument must be a const binding name (not a literal or expression)
- The compiler specializes the function for each unique const argument value
- Within the function body, the const parameter is available as a compile-time
  constant, enabling optimizations like loop unrolling and dead code elimination

**Restrictions:**
- Const parameters must have primitive types or simple aggregates
- Cannot combine `const` with `out` or `mut` modes
- Arguments must be const binding names (e.g., `repeat(N, s)` not `repeat(3, s)`)

**Implementation:** The compiler uses union-branch specialization - a single
function with dispatch over a tag of known instantiations. See
`const-param-specialization.md` for design details.

### 8.5 Generic Functions

A function may take type parameters, written after its name:

```datalove
fun pick_first<T>(a: T, b: T): T
    ret a
end fun

fun unwrap_or<T>(x: ?T, default: T): T
    if x |value|
        ret value
    else
        ret default
    end if
end fun
```

A type parameter is a type like any other within the signature and the body. It
is equal only to itself, so nothing in the body can inspect a value of that type
or convert it to anything else.

**Binding.** The call site does not name the types; they are read off the
arguments. Each argument is matched against its parameter's type, and wherever
the parameter has a type parameter, whatever the argument has in that position
is what it stands for:

```datalove
unwrap_or(some (: u32 / 7), : u32 / 0)      // T is u32
unwrap_or(some "hello", "fallback")         // T is string
```

The first argument to reach a type parameter fixes it, and the rest are checked
against the result, so a disagreement is a type mismatch rather than a
reinterpretation:

```datalove
pick_first(: u32 / 1, "not a u32")          // error: expected u32, found string
```

An argument that cannot say what it is on its own binds nothing, and is checked
afterwards against whatever another argument fixed. So `none` is usable where
something else determines the type:

```datalove
unwrap_or(none, "fallback")                 // T is string, from the second
```

Because the first argument fixes it, argument order decides which type a
parameter is when more than one would do. An unsuffixed integer literal is
`int`, so `pick_first(99, : u32 / 1)` makes `T` `int` and widens the `u32` into
it, while the two written the other way round make `T` `u32`.

**Where a type parameter may appear.** Anywhere on its own, and under `?` or `!`
to any depth:

```datalove
fun flatten_or<T>(x: ??T, default: T): T
fun ok_or<T>(x: !T, default: T): T
```

Under a collection -- a list, map, set, tensor, table, tuple, struct, or enum
payload -- it may appear only in a `ref` or `mut` parameter, or in any parameter
of a `native fun` (Section 9.4):

```datalove
fun len<T>(ref self: [T]): index               // ok
fun sorted<T>(x: [T]): [T]                     // error: not erasable
```

The reason is representation, described below.

**Representation.** A generic function is compiled once, not once per type. A
type parameter has no fixed size, so where the callee takes ownership of a value
of that type -- an `in` or `out` parameter, or the return -- the value is
carried as a `data`, which holds a value of any type along with what is needed
to clone and drop it. The call site converts into that shape on the way in and
moves the value back out on the way back, because it is the place that knows the
type. An `out` parameter goes both ways: what was already there is erased into
the value the callee is given, so that the call drops it exactly once as it does
for any out parameter, and what the callee writes is moved back out into
whatever the caller keeps there.

An option or a result holds its payload inline, so converting one means
converting the payload and writing it where the other side keeps it, and a type
parameter below one is reachable that way. A collection packs its elements by
size, so converting `[u32]` to a list of `data` would mean rebuilding the
collection element by element at every call. That is why an owned collection
parameter is refused rather than silently paid for.

A `ref` or `mut` parameter is not converted at all: the value is passed as it
stands, and the descriptor saying what it really is comes from the call site.
This costs nothing, which is why a type parameter under a collection is allowed
there.

**Copy and linearity.** A type parameter is never a copy type, because the
caller may supply a linear one. A value taken out of an option by
`if x |value|` has therefore moved out of `x`, so a function that wants to
return the option it destructured must rebuild it:

```datalove
fun or_option<T>(self: ?T, other: ?T): ?T
    if self |value|
        ret some value        // not `ret self`, which has been moved out of
    else
        ret other
    end if
end fun
```

A collection whose elements are a type parameter cannot be indexed. Indexing
works out where an element sits from the type of the collection, and a generic
function's type for one says `data` where the parameter was written, so the
stride would be wrong. The native list functions read the element type from the
descriptor that travels with the collection, so `sys/std/list` reaches an
element where the index operator cannot.

The rest of what generics do not reach, and why each is where it is, is in
[Where this stands](plan-generics.md#user-content-where-this-stands).

### 8.6 Control Flow

**If statement:**

```datalove
if condition
    // then branch
else
    // else branch
end if
```

**If with binding** unwraps an option or result:

```datalove
if opt |value|
    // value is bound here
end if
```

**Loop:**

```datalove
loop
    if done
        break
    end if
end loop
```

**Conditional loop:**

```datalove
loop while condition
    // body
end loop
```

`break` exits the innermost loop. `continue` jumps to the next iteration.

**Match** destructures an enum value:

```datalove
match c
case atom Red
    debuglog "red"
case atom Blue
    debuglog "blue"
end match
```

Term cases bind the payload to a variable:

```datalove
match shape
case atom Circle
    debuglog "circle"
case term Rect dims
    debuglog dims
end match
```

A `case default` arm matches any unmatched variant. Without a default, the
match must be exhaustive (all enum variants must be covered). Duplicate cases
are an error.

The input expression is consumed (moved) by the match.

### 8.7 Return

`ret` returns a value from a function:

```datalove
ret 42
```

Void functions may use bare `ret` for early exit:

```datalove
ret
```

## 9. Module System

### 9.1 Hierarchy

The module system has three levels: library, package, module.

A **library** is a directory of packages (e.g. `sys/`, `local/`).
A **package** is a directory of modules (e.g. `sys/std/`).
A **module** is a single `.dfm` file (e.g. `sys/std/u32.dfm`).

### 9.2 Require

`require` loads a module or rider:

```datalove
require module sys/std/u32
require rider std
```

### 9.3 Import

`import` brings a name into scope from a required module or rider:

```datalove
import u32.negate
import std.string_len
```

### 9.4 Native Riders

A **rider** is a Rust crate that provides native functions to a package's
modules. Each package has at most one rider. The system library uses the
same mechanism as user packages.

#### Rider Interface

A rider interface file (`rider.dli`) declares native function signatures
using restricted syntax (`native fun` declarations and `type` aliases only):

```datalove
native fun string_len(ref self: string): index
native fun string_contains(ref haystack: string, ref needle: string): bool
native fun list_push<T>(mut self: [T], elem: T)
```

A native function may take type parameters, and they are less restricted than
on an ordinary function: because every parameter arrives as a pointer and a
descriptor, a type parameter may sit under a collection in any parameter, not
only a borrowed one (Section 8.5). The implementation reads the element type
off the descriptor at runtime, so one implementation serves every element type.

#### Rider Crate

The rider crate (`rider/src/lib.rs`) implements native functions as
`extern "C-unwind"` functions following the rider C ABI. Each parameter
is a `(ptr, tydesc)` pair, with an out-param for the return value:

```rust
#[unsafe(no_mangle)]
pub extern "C-unwind" fn dlr_std__string_len(
    rt: LocalRtHandle,
    self_ptr: *const u8,
    self_tydesc: *const TyDesc,
    result_out: *mut u8,
    result_tydesc: *const TyDesc,
) -> RtStatus { ... }
```

Symbol naming convention: `dlr_{rider_name}__{function_name}`.

#### Using Riders

Modules use `require rider` to access native functions:

```datalove
require rider std
import std.string_len

fun my_len(ref s: string): index
    ret string_len(ref s)
end fun
```

Call sites typecheck normally against the declared signatures. The compiler
emits standard `Call` instructions; dispatch to native code is handled by the
backend.

#### Backend Support

All three execution backends support native rider calls:

- **Interpreter**: function pointers are registered in `NativeFunctionTable`
  and dispatched by symbol name. A rider linked into the running binary
  supplies its own addresses; one found on disk is built into a `.so` and
  loaded via `dlopen`.
- **AOT**: Native functions are declared as `Linkage::Import` and resolved by
  the linker against the archive holding the runtime and the riders.
- **JIT**: Native functions are declared as Cranelift imports with their C ABI
  signatures. Symbol addresses are registered via the JIT's symbol lookup
  mechanism from the same table the interpreter uses.

## 10. Numeric Widening

There is no implicit numeric widening in the language. All numeric conversions
require the explicit `@` operator.

Fixed integers cannot use bare arithmetic operators. To perform arithmetic,
either widen to `int` using `@`, or use checked/optional operators:

```datalove
let a: u32 = 10
let b: u32 = 20
let c: int = a@ + b@    // widen to int, then add
let d: u32 = (a +! b)   // checked add, returns same type
```

Valid widening chains for `@`:

```datalove
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
u8 -> i16 -> i32 -> i64 -> int   (cross-sign)
u16 -> i32 -> i64 -> int         (cross-sign)
u32 -> i64 -> int                (cross-sign)
index -> int
offset -> int
```

A chain is shorthand for widening directly to any type after the source, so
`u8` reaches `u64` in one step rather than through `u16`.

Floats do not widen, by `@` or otherwise. `f64.from_f32` converts a value of
the narrower width, exactly, and `f32.from_f64` converts one the other way,
rounding, with none for a finite value too large for an `f32` to hold.

`index` and `offset` widen only to `int`. They are 32-bit or 64-bit
depending on how the compiler is configured, so a conversion to a fixed
width would mean something different in each configuration, while `int`
holds either exactly.

Example requiring explicit widening:

```datalove
fun process(x: int): int
    ret x

fun example(): int
    let n: u32 = 42
    ret process(n@)     // explicit widen u32 to int
```

## 11. Ownership Analysis

Ownership analysis runs after type checking to verify correct use of linear
values.

### 11.1 Errors

| Code | Name | Description |
|------|------|-------------|
| D001 | UseAfterMove | Using a value after it was moved |
| D002 | DoubleMove | Moving a value twice |
| D003 | CannotMoveBorrowed | Moving a `ref`/`mut`/`out` parameter |
| D004 | CannotMutFromRef | Passing `ref` where `mut` required |
| D005 | ReadUninitialized | Reading before initialization |
| D006 | OutParamNotInitialized | Returning without initializing `out` param |
| D007 | MoveInLoop | Moving outer-scoped linear value in loop |
| D008 | InconsistentBranchMove | Moved in one branch but not another |
| D009 | OutParamPartialWrite | Writing fields of `out` param individually |
| D010 | AliasedMutableArgument | Two arguments alias, one is `mut`/`out` |
| D011 | CannotMutateImmutable | Immutable binding passed as `mut`/`out` |
| D012 | CannotMutateTemporary | Temporary passed as `mut`/`out` |

### 11.2 Mutable Arguments

A `mut` or `out` argument is written through, so it must denote something the
caller can assign to: a `var` binding, a projection of one, or a `mut`/`out`
parameter being forwarded.

```datalove
fun bump(mut x: u32)

var v: u32 = 1
bump(mut v)            // ok
bump(mut v.0)          // ok for a var aggregate

let w: u32 = 1
bump(mut w)            // error: `w` is immutable
bump(mut compute())    // error: the write would be discarded
```

An `in` parameter is not assignable either, and a `ref` parameter reports the
more specific D004.

### 11.3 Argument Aliasing

Every parameter is passed by reference, so two arguments naming the same
binding hand the callee two references to one object. That is an error when
either is `mut` or `out`:

```datalove
fun grow(mut self: string, ref other: string)

var s: string = "hi"
grow(mut s, ref s)    // error: aliased mutable argument
```

Two `ref` arguments are permitted, as are two reads of a copy value:

```datalove
let equal = compare(ref x, ref x)    // ok: both ref
let n = mul_checked(b, b)            // ok: both consume a copy type
```

Arguments are compared by the binding their place is rooted at. Distinct
fields (`f(mut p.x, ref p.y)`) and distinct indexes (`f(mut a[i]?, ref a[j]?)`)
share a root and are rejected, even where they do not overlap in fact.

### 11.4 Loop Restrictions

Moving an outer-scoped linear value inside a loop is an error:

```datalove
var b: int = 5
loop
    set a = b      // error: cannot move 'b' in loop
end loop
```

Copy types and operator operands (which are borrowed) are exempt.

### 11.5 Branch Consistency

If a value is moved in one branch, it must be moved in all branches:

```datalove
if cond
    consume(x)     // moves x
else
    // error: x not moved here
end if
```

### 11.6 Tracking Categories

Bindings are categorized for drop scheduling:

- **Copy**: No tracking needed.
- **Precise**: Ownership state known statically.
- **Tracked**: Runtime tracking byte used.

Tracked bindings include `var` bindings, `out` parameters, and uninitialized
variables.

## 12. Bidirectional Type Inference

Expressions can *synthesize* types (bottom-up) or *check* against expected
types (top-down).

**Integer synthesis**: bare integer literals synthesize as `int`
(arbitrary-precision). When an expected type is available, integers check
against it instead:

```datalove
let x = 42              // x: int (synthesized)
let y: u32 = 42         // y: u32 (checked against binding type)
```

Checked arithmetic propagates expected types to operands:

```datalove
fun add(): !u32
    ret ok (1 +! 2)    // 1 and 2 infer u32 from context
end fun
```

With no expected type for a binary operator, one operand can supply the other's.
When exactly one side is a bare numeric literal and the other has a numeric type
of its own, the literal checks against that type:

```datalove
fun f(n: u32): bool
    ret n == 0         // 0 infers u32 from n
end fun
```

With a literal on both sides there is nothing to propagate and both synthesize
`int`, so `1 == 2` compares bigints. A non-numeric operand supplies nothing
either: `1 + "hello"` is a plain mismatch.

Float literals infer their precision from context:

```datalove
fun pi(): f64
    ret 3.14159        // infers f64
end fun
```

`f64` is what a literal falls back to with nothing to infer from, the same
way a bare integer literal falls back to `int`: a width nobody asked for
should be the one that keeps what was written. An `f32` is had by saying so,
with an annotation or a type hint. A negation does not interrupt any of this:
`-3.14159` infers `f64` in the same position, since the sign says nothing
about the width.

The fallback reaches inside collections built from bare literals, so
`[1.0, 2.0]` is a `[f64]`. Where the width is the point rather than the
precision, write it: `let xs: [f32] = [1.0, 2.0]`.

Neither family takes the other's literals. An integer literal where a float
is expected is a mismatch rather than a conversion, and the same in reverse;
`f64.from_int` and `int.from_f64` are the named conversions.

## Appendix A. Command-Line Interface

| Command | Description |
|---------|-------------|
| `script` | Execute a .dfs script |
| `repl` | Interactive REPL |
| `lit-tycheck` | Type check a datalit expression |
| `lit-ast` | Print datalit AST |
| `lit-pretty` | Pretty-print datalit |
| `lit-op` | Perform datalit operations |
| `typecheck-std` | Type check the standard library |

## Appendix B. Unimplemented Features

The following features appear in design documents but are not yet implemented:

- Tensor operations (transpose, slice, reshape) --
  tensor literals and axis-0 indexing work (`t[i]?`/`t[i]!`),
  but higher-level manipulation operations are not exposed.
  Mutation of sub-tensor views via `mut` params is rejected;
  element-level mutation through a view requires direct `set` on the
  original tensor with chained indexing.
- Arena blocks
- Memoization
- Type introspection (`@type`)
- Panic statement
- Full Datalove layer (procedures and objects)
