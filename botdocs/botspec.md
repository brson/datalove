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
and       break     continue  data      else      end
enum      error     er        false     for       fun
icall     if        import    in        let       loop
map       mut       none      not       ok        or
out       ref       require   ret       set       some
table     tensor    true      type      var       while
xor
```

### 2.2 Literals

**Integers** may be written in decimal or hexadecimal:

```
42
0xFF
```

**Floating-point** numbers use standard notation:

```
3.14
1.0e-10
```

**Strings** are enclosed in double quotes:

```
"hello, world"
```

**Booleans** are `true` and `false`.

### 2.3 Comments

Line comments begin with `//` and extend to end of line.

## 3. Types

### 3.1 Primitive Types

| Type | Description |
|------|-------------|
| `bool` | Boolean value |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers |
| `i8`, `i16`, `i32`, `i64` | Signed integers |
| `index` | Platform-sized unsigned index |
| `offset` | Platform-sized signed offset |
| `f32`, `f64` | Floating-point numbers |
| `int` | Arbitrary-precision integer |
| `string` | UTF-8 string |

The `index` and `offset` types are 32-bit by default, or 64-bit when the
`index-64` feature is enabled.

### 3.2 Collection Types

**List.** An ordered sequence of elements.

```
[T]              // type
[1, 2, 3]        // literal
```

**Map.** A key-value mapping.

```
map<K, V>        // type
map { 0 = 5 }    // literal
```

**Set.** An unordered collection of unique elements.

```
set<T>           // type
set { 1, 2, 3 }  // literal
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

**Enum.** A tagged union of variants.

```
enum { None, Some(T) }   // type
enum None                // variant without payload
enum Some(42)            // variant with payload
```

### 3.4 Option and Result

**Option** represents an optional value:

```
?T                       // type: value or none
some expr                // wrap value
none                     // absent value
```

**Result** represents success or failure:

```
!T                       // type: value or error
ok expr                  // wrap success value
er expr                  // wrap error value
error "message"          // error literal
```

### 3.5 Data Type

The `data` type is a universal container that can hold any value:

```
data 42
data : int / 100
```

Any type coerces to `data`.

### 3.6 Type Aliases

Type aliases provide names for structural types:

```
type Point: { x: f32, y: f32 }
type Age: u32
```

Aliases are purely syntactic; they introduce no new types.

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

**Linear types** have move semantics: `int`, `string`, `list`, `map`, `set`,
`table`, `data`, `error`.

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
| 3 | `?` `!` | Postfix try |
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
- **Fixed integers**: Operands widen to `int`, result is `int`. Division and
  unary negation are not permitted.

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

### 7.4 Control Flow

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

### 7.5 Return

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

Fixed integers widen to larger types:

```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
index -> int
offset -> int
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

- Tensor operations
- Pattern matching (`match` expressions)
- Arena blocks
- Memoization
- Type introspection (`@type`)
- Panic statement
- Full Datalove layer (procedures and objects)
