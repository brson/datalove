# Datalove Language Specification

## Contents

- [1. Introduction](#user-content-1-introduction)
- [2. Lexical Conventions](#user-content-2-lexical-conventions)
- [3. Types](#user-content-3-types)
- [4. Type Hints](#user-content-4-type-hints)
- [5. Copy and Linear Types](#user-content-5-copy-and-linear-types)
- [6. Expressions](#user-content-6-expressions)
- [7. Statements](#user-content-7-statements)
- [8. Module System](#user-content-8-module-system)
- [9. Numeric Widening](#user-content-9-numeric-widening)
- [10. Ownership Analysis](#user-content-10-ownership-analysis)
- [11. Bidirectional Type Inference](#user-content-11-bidirectional-type-inference)
- [Appendix A. Command-Line Interface](#user-content-appendix-a-command-line-interface)
- [Appendix B. Unimplemented Features](#user-content-appendix-b-unimplemented-features)

## 1. Introduction

Datalove is a statically-typed scripting language designed for data manipulation
and incremental computation. The language emphasizes safety through a linear
type system that tracks ownership, preventing use-after-move errors at compile
time.

The language has a three-layer design:

- **Datalit** (.dlt) - A pure data literal sublanguage for representing values.
- **Datafun** (.dfs, .dfm) - A pure functional layer with functions, modules,
  and control flow.
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

```
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

```
42
0xFF
```

Bare integer literals synthesize as `int` (arbitrary-precision).
Use a type hint for fixed-width types: `: u32 / 42`.

**Floating-point** numbers use decimal notation:

```
3.14
```

**Strings** are enclosed in double quotes:

```
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

```
[T]              // type
[1, 2, 3]        // literal
```

**Map.** A key-value mapping.

```
%{K = V}         // type
%{ 0 = 5 }       // literal
```

**Set.** An unordered collection of unique elements.

```
#{T}             // type
#{ 1, 2, 3 }     // literal
```

**Table.** A columnar data structure with named columns.

```
{| col1: T1, col2: T2 |}    // type
```

Table literals use a line-oriented syntax:

```
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

```
[|T, N|]                 // type: element type T, rank N
[| 1 2 3 |]             // 1D literal (shape inferred: [3])
[| 1 2 3, 4 5 6 |]      // 2D literal (shape inferred: [2, 3])
[| 1 2, 3 4,, 5 6, 7 8 |]  // 3D literal (shape inferred: [2, 2, 2])
```

Type hints specify element type and rank:

```
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

```
(bool, u32)              // type
(true, 42)               // literal
()                       // unit type and value
```

**Struct.** A collection of named fields.

```
{ x: f32, y: f32 }       // type
{ x = 1.0, y = 2.0 }     // literal
```

### 3.4 Option and Result

**Option** represents an optional value:

```
?T                       // type: value or none
some primary             // wrap value
none                     // absent value
```

**Result** represents success or failure:

```
!T                       // type: value or error
ok primary               // wrap success value
er primary               // wrap error value
error "message"          // error literal
```

The `some`, `ok`, `er`, `data`, and `error` keywords take a *primary* expression
as their payload: a literal, variable, function call, or parenthesized expression.
Binary operator expressions require parentheses:

```
some 42                  // ok: literal is primary
some(a +? b)             // ok: parenthesized expression
ok result                // ok: variable is primary
ok(x +! y)              // ok: parenthesized expression
er(error "msg")          // ok: parenthesized expression
```

### 3.5 Data Type

The `data` type is a universal container that can hold any value:

```
data 42
data : int / 100
data(a + b)              // parenthesized for binop payload
```

The `error` type is an existential error value:

```
error "message"
error(some_expr)         // parenthesized for non-primary payload
```

Both `data` and `error` take a primary expression as their payload (see Section 3.4).
Any type coerces to `data`.

### 3.6 Atom, Term, and Enum Types

**Atom.** A named unit type with no payload.

```
atom Red                 // type and value
```

An atom is both a type and a value. Two atoms are the same type if they have
the same name.

**Term.** A named type with a typed payload.

```
term Foo int             // type
term Foo 42              // value
```

Two terms are the same type if they have the same name and payload type.

**Enum.** A closed union of atom and term variants.

```
enum { atom Red, atom Blue, term Custom string }   // type
```

Enum variants are matched by name. Atoms and terms can stand alone as types,
or combine into enums.

**Coercion.** The `@` operator widens an atom or term into a compatible enum
type:

```
let c: enum { atom Red, atom Blue } = (atom Red)@
```

**Match.** Enums are destructured with `match` (see Section 7.5).

### 3.7 Type Aliases

Type aliases provide names for structural types:

```
type Point: { x: f32, y: f32 }
type Age: u32
```

Aliases are purely syntactic; they introduce no new types. Enum types are
commonly given aliases:

```
type Color: enum { atom Red, atom Blue, term Custom string }
```

## 4. Type Hints

Type hints specify expected types for expressions:

```
: type / expression
```

Examples:

```
: u32 / 42
: [i32] / [1, 2, 3]
: f32 / 0xABABABAB     // hex as bit pattern
```

## 5. Copy and Linear Types

Types are classified as *copy* or *linear*.

**Copy types** can be freely duplicated: `bool`, fixed-width integers (`u8`
through `u64`, `i8` through `i64`), `index`, `offset`, `f32`, `f64`.
Atoms are always copy. Terms are copy if their payload type is copy.
Enums are copy if all variant payloads are copy.

**Linear types** have move semantics: `int`, `string`, `[T]`, `%{K = V}`,
`#{T}`, `{| ... |}`, `[|T, N|]`, `data`, `error`. Terms and enums with
linear payloads are linear.

A linear value can be used exactly once. After a value is moved, subsequent
uses are compile-time errors:

```
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
| 2 | `-` `-?` `-!` `not` | Unary prefix |
| 3 | `@` `?` `!` | Postfix (adapt, try) |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Comparison |
| 7 | `and` | Logical AND |
| 8 | `or` `xor` | Logical OR/XOR |

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

```
fun add(a: u32, b: u32): !u32
    ret ok (a +! b)
end fun
```

Bigint division `/!` returns `!int`.

**Optional arithmetic** (`+?`, `-?`, `*?`, `/?`) operates on fixed integers and
returns an option type. On overflow or division by zero, the function
early-returns `none`:

```
fun add(a: u32, b: u32): ?u32
    ret some (a +? b)
end fun
```

Bigint division `/?` returns `?int`. Unary `-?` is permitted only for signed
fixed integers.

### 6.3 Comparison

```
.<    less than
.>    greater than
<=    less than or equal
>=    greater than or equal
==    equal
!=    not equal
```

All comparison operators return `bool`.

### 6.4 Logical Operators

```
and   logical AND
or    logical OR
xor   logical XOR
not   logical NOT (unary)
```

All require `bool` operands and return `bool`.

### 6.5 Try Operators

The postfix `?` operator unwraps an option, early-returning `none` on failure:

```
fun get_value(opt: ?i32): ?i32
    let x = opt?           // early-return if none
    ret some (x + 1)
end fun
```

The postfix `!` operator unwraps a result, early-returning the error on
failure:

```
fun parse(s: string): !i32
    let n = do_parse(s)!   // early-return if error
    ret ok n
end fun
```

### 6.6 Operator Argument Semantics

All operators treat their operands as immutable references. Operands are not
consumed:

```
let x: int = 42
let a = x + 1       // x is cloned for the operation
let b = x + 2       // x can be used again
ret x               // x is still valid
```

### 6.7 Intrinsic Calls

Intrinsics are low-level operations that compile to machine instructions:

```
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

```
let msg = "hello"
let a = consume(msg@)   // clone msg, original stays valid
let b = consume(msg)    // msg is still available
```

**Widen**: For fixed integers, `@` widens to a larger type:

```
let n: u8 = 42
let x: int = n@         // widen u8 to int
```

**Cross-sign widen**: Unsigned integers can widen to larger signed types:

```
let n: u8 = 255
let x: i16 = n@         // u8 widens to i16 (value fits)
```

**Atom/term to enum**: An atom or term widens to a compatible enum type:

```
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
the expected type is known (function argument, let binding with annotation,
return position, etc.).

## 7. Statements

### 7.1 Bindings

**Let** binds an immutable value:

```
let x: u32 = 42
let y = compute()      // type inferred
```

**Var** binds a mutable slot:

```
var x: u32 = 0
var y: i32             // uninitialized; must set before use
```

**Set** mutates a var binding or mutable parameter:

```
set x = x + 1
```

### 7.2 Functions

Function definition:

```
fun name(param1: T1, param2: T2): ReturnType
    // body
    ret value
end fun
```

Void functions omit the return type and may omit `ret`:

```
fun log(msg: string)
    // body
end fun
```

### 7.3 Parameter Modes

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

### 7.4 Const Parameters

The `const` modifier declares a parameter whose value must be known at compile
time:

```
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

### 7.5 Control Flow

**If statement:**

```
if condition
    // then branch
else
    // else branch
end if
```

**If with binding** unwraps an option or result:

```
if opt |value|
    // value is bound here
end if
```

**Loop:**

```
loop
    if done
        break
    end if
end loop
```

**Conditional loop:**

```
loop while condition
    // body
end loop
```

`break` exits the innermost loop. `continue` jumps to the next iteration.

**Match** destructures an enum value:

```
match c
case atom Red
    debuglog "red"
case atom Blue
    debuglog "blue"
end match
```

Term cases bind the payload to a variable:

```
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

### 7.6 Return

`ret` returns a value from a function:

```
ret 42
```

Void functions may use bare `ret` for early exit:

```
ret
```

## 8. Module System

### 8.1 Hierarchy

The module system has three levels: library, package, module.

### 8.2 Require

`require` loads a module:

```
require module sys/std/u32
```

### 8.3 Import

`import` brings a name into scope:

```
import u32.negate
```

## 9. Numeric Widening

There is no implicit numeric widening in the language. All numeric conversions
require the explicit `@` operator.

Fixed integers cannot use bare arithmetic operators. To perform arithmetic,
either widen to `int` using `@`, or use checked/optional operators:

```
let a: u32 = 10
let b: u32 = 20
let c: int = a@ + b@    // widen to int, then add
let d: u32 = (a +! b)   // checked add, returns same type
```

Valid widening chains for `@`:

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
u8 -> i16 -> i32 -> i64 -> int   (cross-sign)
u16 -> i32 -> i64 -> int         (cross-sign)
u32 -> i64 -> int                (cross-sign)
index -> int
offset -> int
```

Example requiring explicit widening:

```
fun process(x: int): int
    ret x

fun example(): int
    let n: u32 = 42
    ret process(n@)     // explicit widen u32 to int
```

## 10. Ownership Analysis

Ownership analysis runs after type checking to verify correct use of linear
values.

### 10.1 Errors

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

### 10.2 Loop Restrictions

Moving an outer-scoped linear value inside a loop is an error:

```
var b: int = 5
loop
    set a = b      // error: cannot move 'b' in loop
end loop
```

Copy types and operator operands (which are borrowed) are exempt.

### 10.3 Branch Consistency

If a value is moved in one branch, it must be moved in all branches:

```
if cond
    consume(x)     // moves x
else
    // error: x not moved here
end if
```

### 10.4 Tracking Categories

Bindings are categorized for drop scheduling:

- **Copy**: No tracking needed.
- **Precise**: Ownership state known statically.
- **Tracked**: Runtime tracking byte used.

Tracked bindings include `var` bindings, `out` parameters, and uninitialized
variables.

## 11. Bidirectional Type Inference

Expressions can *synthesize* types (bottom-up) or *check* against expected
types (top-down).

**Integer synthesis**: bare integer literals synthesize as `int`
(arbitrary-precision). When an expected type is available, integers check
against it instead:

```
let x = 42              // x: int (synthesized)
let y: u32 = 42         // y: u32 (checked against binding type)
```

Checked arithmetic propagates expected types to operands:

```
fun add(): !u32
    ret ok (1 +! 2)    // 1 and 2 infer u32 from context
end fun
```

Float literals infer their precision from context:

```
fun pi(): f64
    ret 3.14159        // infers f64
end fun
```

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

- Tensor element access and operations (indexing, transpose, slice, reshape) -
  tensor literals work, but manipulation is not exposed
- Arena blocks
- Memoization
- Type introspection (`@type`)
- Panic statement
- Full Datalove layer (procedures and objects)
