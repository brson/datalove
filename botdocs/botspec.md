# Datalove Bot Specification

Bot-maintained specification reflecting actual implementation state.
Last verified: 2026-01-25

## Contents

- [Overview](#user-content-overview)
- [1. Datalit Layer](#user-content-1-datalit-layer)
  - [1.1 Primitive Types](#user-content-11-primitive-types)
  - [1.2 Collection Types](#user-content-12-collection-types)
  - [1.3 Aggregate Types](#user-content-13-aggregate-types)
  - [1.4 Special Types](#user-content-14-special-types)
  - [1.5 Literal Syntax](#user-content-15-literal-syntax)
- [2. Datafun Layer](#user-content-2-datafun-layer)
  - [2.1 Statements](#user-content-21-statements)
  - [2.2 Expressions](#user-content-22-expressions)
  - [2.3 Operators](#user-content-23-operators)
  - [2.4 Function Definitions](#user-content-24-function-definitions)
  - [2.5 Parameter Modes](#user-content-25-parameter-modes)
  - [2.6 Loop Statements](#user-content-26-loop-statements)
  - [2.7 Operator Argument Semantics](#user-content-27-operator-argument-semantics)
  - [2.8 Module System](#user-content-28-module-system)
  - [2.9 Void Functions](#user-content-29-void-functions)
  - [2.10 Intrinsic Calls](#user-content-210-intrinsic-calls)
  - [2.11 Type Aliases](#user-content-211-type-aliases)
- [3. Type System](#user-content-3-type-system)
  - [3.1 Bidirectional Typing](#user-content-31-bidirectional-typing)
  - [3.2 Numeric Widening](#user-content-32-numeric-widening)
  - [3.3 Coercions](#user-content-33-coercions)
  - [3.4 Copy vs Linear Types](#user-content-34-copy-vs-linear-types)
  - [3.5 Move Semantics](#user-content-35-move-semantics)
  - [3.6 Ownership Analysis](#user-content-36-ownership-analysis)
- [4. Runtime/REPL](#user-content-4-runtimerepl)
  - [4.1 CLI Commands](#user-content-41-cli-commands)
  - [4.2 REPL Capabilities](#user-content-42-repl-capabilities)
- [Appendix A: Documented But Unimplemented Features](#user-content-appendix-a-documented-but-unimplemented-features)

## Overview

Datalove is a typed scripting language with a three-layer design:
1. **Datalit** (.dlt) - Pure data literal language
2. **Datafun** (.dfs script, .dfm module) - Pure functional layer
3. **Full Datalove** (.dls, .dlm) - Procedures and objects [NOT IMPLEMENTED]

Uses Salsa for incremental compilation. REPL-first design.

---

## 1. Datalit Layer

### 1.1 Primitive Types

| Type | Description | Interpreter Status |
|------|-------------|-------------------|
| `bool` | Boolean | Implemented |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers | Implemented |
| `i8`, `i16`, `i32`, `i64` | Signed integers | Implemented |
| `index` | Unsigned index type (32 or 64-bit) | Implemented |
| `offset` | Signed index type (32 or 64-bit) | Implemented |
| `f32` | 32-bit float | Implemented |
| `f64` | 64-bit float | Implemented |
| `int` | Arbitrary precision signed integer (bigint) | Implemented |
| `string` | UTF-8 string | Implemented |

**Index Types (index/offset):**

`index` and `offset` are platform-configurable index types used for collection sizes, capacities, and array indices. Their bit width is controlled by the `index-64` compile-time feature:

| Feature | `index` | `offset` |
|---------|---------|----------|
| Default | u32 | i32 |
| `index-64` | u64 | i64 |

These types widen to `int` like other fixed integers. See `botdocs/index-64.md` for details on the feature.

### 1.2 Collection Types

| Type | Syntax | Example | Interpreter Status |
|------|--------|---------|-------------------|
| List | `[T]` | `[1, 2, 3]` | Implemented |
| Map | `map<K, V>` | `map { 0 = 5, 2 = 2 }` | Implemented |
| Set | `set<T>` | `set { 1, 2, 3 }` | Implemented |
| Tensor | `tensor<T, N>` | - | [PARTIAL: parsed only] |
| Table | `{| col: T, ... |}` | `{| x, y; 1, 2 |}` | Implemented |

**Table Type:**

Tables provide struct-of-array (columnar) memory layout, similar to dataframes in Pandas/Polars/Arrow.

Type syntax: `{| col1: T1, col2: T2, ... |}`

Expression syntax:
```
{|
  col1, col2    // header row (column names required)
  val1, val2    // data row 1
  val3, val4    // data row 2
|}
```

The `{|` opening bracket enters a line-oriented parsing context where rows are separated by newlines. Semicolons can be used for single-line format: `{| x, y; 1, 2; 3, 4 |}`.

Example with type hint:
```
: {| x: u32, y: u32 |} / {|
  x, y
  1, 2
  3, 4
|}
```

Column projections (e.g., `table.x`) have type "list of column type" but cannot be mutated or moved. They can be passed to `ref`-mode function arguments.

### 1.3 Aggregate Types

**Anonymous Tuple:** (Implemented)
```
: (bool, u32) / (true, 1)
: () / ()
```

**Anonymous Struct:** (Implemented)
```
: { field1: bool, field2: u32 } / { field1 = true, field2 = 1 }
```

**Anonymous Enum:** (Implemented)
```
: enum { Foo, Bar(u32) } / enum Foo
: enum { Bar(u32) } / enum Bar(2)
```

Note: Named tuples, structs, and enums were removed from the language.

### 1.4 Special Types

| Type | Syntax | Values | Interpreter Status |
|------|--------|--------|-------------------|
| Option | `?T` | value or `none` | Implemented |
| Result | `!T` | value or `error "msg"` | Implemented |
| Data | `data` | `data 1`, `data : int / 1` | Implemented |
| Error | `error` | `error "oops"`, `error : int / 1` | Implemented (as result payload) |

### 1.5 Literal Syntax

**Type hint syntax:** `: type / expression`
```
: u32 / 42
```

**Hex literals:** `0x` prefix for hexadecimal values
```
: u32 / 0xFF        // integer value 255
: u8 / 0x7F         // integer value 127
: f32 / 0xABABABAB  // f32 bit pattern coercion
```
Hex literals can be used with any integer type or f32/f64. With floats, the hex value is interpreted as a raw bit pattern.

---

## 2. Datafun Layer

### 2.1 Statements

| Statement | Syntax | Status |
|-----------|--------|--------|
| `let` | `let name: type = expr` | Implemented |
| `var` | `var name: type = expr` or `var name: type` | Implemented (mutable slot) |
| `set` | `set name = expr` | Implemented (mutate var or mut/out param) |
| `type` | `type Name: structural_type` | Implemented (type alias) |
| `fun` | `fun name(...): ret_type ... end fun` | Implemented |
| `ret` | `ret expr` | Implemented |
| `require module` | `require module sys/std/bool` | Implemented |
| `require data` | `require data name` | [PARTIAL: parsed] |
| `import` | `import module_name.item_name` | Implemented |
| `if` | `if cond ... end if` | Implemented (in function bodies) |
| `if` with binding | `if opt \|value\| ... end if` | Implemented (option/result unwrap) |
| `loop` | `loop ... end loop` | Implemented (in function bodies) |
| `loop while` | `loop while cond ... end loop` | Implemented (conditional loop) |
| `break` | `break` | Implemented (exits innermost loop) |
| `continue` | `continue` | Implemented (next iteration of innermost loop) |

### 2.2 Expressions

| Expression | Example | Status |
|------------|---------|--------|
| Datalit | Any datalit value | Implemented |
| Name/variable | `foo` | Implemented |
| Binary op | `a + b` | Implemented |
| Function call | `foo(a, b)` | Implemented |
| Tuple | `(a, b)` | Implemented |
| Unary negation | `-x` | Implemented (int only) |
| Unary optional | `-?x` | Implemented (signed fixed ints) |
| Unary result | `-!x` | Implemented (signed fixed ints) |
| Logical not | `not x` | Implemented (bool only) |
| Logical and/or/xor | `a and b` | Implemented (bool only) |
| Try option | `expr?` | Implemented (early-return on none) |
| Try result | `expr!` | Implemented (early-return on error) |
| Intrinsic call | `icall name(args)` | Implemented |

### 2.3 Operators

#### Precedence (Highest to Lowest)

| Level | Operators | Description |
|-------|-----------|-------------|
| 1 | `()` | Parenthesized grouping |
| 2 | `-` `-?` `-!` `not` | Unary prefix |
| 3 | `?` `!` | Postfix try |
| 4 | `*` `/` `*!` `/!` `*?` `/?` | Multiplicative |
| 5 | `+` `-` `+!` `-!` `+?` `-?` | Additive |
| 6 | `.<` `.>` `<=` `>=` `==` `!=` | Comparison |
| 7 | `and` | Logical AND |
| 8 | `or` `xor` | Logical OR/XOR |

See `botdocs/op-precedence.md` for detailed reference.

#### Bare Arithmetic (`+ - * /`)

| Type | `+` `-` `*` | `/` | Unary `-` |
|------|-------------|-----|-----------|
| **f32/f64** | Returns same type | Returns same type | Returns same type |
| **int** (bigint) | Returns int | Not allowed (use `/!` or `/?`) | Returns int |
| **Fixed ints** | Widens to int | Not allowed | Not allowed |

- Floats: All bare ops work, return same float type.
- Bigints: Add/sub/mul and unary neg work. Division requires checked variant (div0 possible).
- Fixed ints: Bare `+ - *` widen both operands to `int`, return `int`. No bare `/` or unary `-`.

**Tycheck:** Correct per spec.
**Interpreter:** Correct for widening behavior.

#### Checked Arithmetic (`+! -! *! /!`) - Early-return Result

| Type | `+!` `-!` `*!` | `/!` | Unary `-!` |
|------|----------------|------|------------|
| **f32/f64** | Not allowed | Not allowed | Not allowed |
| **int** | Not allowed | Returns `!int` | Not allowed |
| **Fixed ints** | Returns `!T` (same type) | Returns `!T` | Returns `!T` |

These operators early-return on overflow/div0, requiring the enclosing function to return `!T`.

**Tycheck:** Correct per spec.
**Interpreter:** Correct per spec (early-returns error on overflow).

#### Optional Arithmetic (`+? -? *? /?`) - Early-return Option

| Type | `+?` `-?` `*?` | `/?` | Unary `-?` |
|------|----------------|------|------------|
| **f32/f64** | Not allowed | Not allowed | Not allowed |
| **int** | Not allowed | Returns `?int` | Not allowed |
| **Fixed ints** | Returns `?T` (same type) | Returns `?T` | Signed only, returns `?T` |

These operators early-return `none` on overflow/div0. `-?` disallowed for unsigned ints (footgun).

**Tycheck:** Correct per spec (operator yields T, function must return ?T).
**Interpreter:** Correct per spec (early-returns OptionNone on overflow).

#### Comparison (`.<` `.>` `<=` `>=` `==` `!=`)

| Op | Meaning |
|----|---------|
| `.<` | Less than |
| `.>` | Greater than |
| `<=` | Less or equal |
| `>=` | Greater or equal |
| `==` | Equal |
| `!=` | Not equal |

**Tycheck:** Returns `bool` for any numeric operands.
**Interpreter:** Implemented for all numeric types.

#### Logical Operators (`and` `or` `xor` `not`)

| Op | Type | Description |
|----|------|-------------|
| `and` | Binary | Logical AND |
| `or` | Binary | Logical OR |
| `xor` | Binary | Logical XOR |
| `not` | Unary prefix | Logical NOT |

All require `bool` operands and return `bool`.

**Tycheck:** Implemented.
**Interpreter:** Implemented.
**AOT:** Implemented.

### 2.4 Function Definitions

```
fun name(param1: type1, param2: type2): return_type
  let x = param1 + param2
  ret x
end fun
```

### 2.5 Parameter Modes

All parameters are passed by reference (pointer to caller's data). The mode determines allowed operations:

| Mode | Syntax | Semantics | Status |
|------|--------|-----------|--------|
| `in` | `x: T` (default) | Read and consume; ownership transfers to callee | Implemented |
| `ref` | `ref x: T` | Read only; caller retains ownership | Implemented |
| `mut` | `mut x: T` | Read and write via `set`; caller retains ownership | Implemented |
| `out` | `out x: T` | Write only via `set`; callee must initialize before return | Implemented |

**Compile-time checks:**
- `ref`/`mut`/`out` params cannot be moved (caller owns them)
- `out` params must be initialized before reading or returning
- `ref` params cannot be passed to `mut` parameters
- `out` params cannot be partially written (must write whole value, not fields)

#### Out Parameter Semantics

Out parameters enable functions to write results to caller-provided locations. The caller destroys any existing value before the call; the callee writes to an uninitialized slot.

**Call site behavior:**
```
var result: (u32, u32) = (0, 0)
init_pair(result)      // caller destroys (0, 0), callee writes new value
```

**Callee behavior:**
```
fun init_pair(out p: (u32, u32))
    set p = (42, 100)  // OK: writes whole value
end fun
```

**Partial writes disallowed:**
```
fun bad(out p: (u32, u32))
    set p.0 = 42       // ERROR D009: cannot partially write to out parameter
    set p.1 = 100      // ERROR D009
end fun
```

This restriction exists because runtime tracking is per-parameter, not per-field. The first field write would mark the parameter as initialized, causing the second write to incorrectly try to destroy an uninitialized field.

#### Uninitialized Var Bindings

Var bindings can be declared without an initializer:

```
var x: i32              // declared but not initialized
set x = 42              // must initialize before use
debuglog x              // now valid
```

**Requirements:**
- Type hint is required when no initializer is provided
- Must be initialized via `set` before reading
- Conditional initialization must occur in all branches

**Example with conditional initialization:**
```
var result: i32
if condition
    set result = 1
else
    set result = 2
end if
debuglog result         // valid: initialized on all paths
```

**Error cases:**
```
var x: i32
debuglog x              // ERROR D005: read of uninitialized binding

var y: i32
if condition
    set y = 1
end if
debuglog y              // ERROR: may be uninitialized (no else branch)
```

**Implementation:** Uninitialized vars reuse the same tracking mechanism as `out` parameters. Both use runtime tracking bytes to determine if a value has been written, enabling conditional drops at scope exit.

#### Field Projections as Arguments

Field projections can be passed to `ref`, `mut`, or `out` parameters:

```
var t: (u32, u32) = (100, 200)
write_42(t.0)          // pass t.0 as out param
debuglog t.0           // prints 42
```

For `out` params, the caller destroys the field value before the call. The callee sees an uninitialized slot and must write to it.

### 2.6 Loop Statements

#### Basic Loop

Unconditional loop with break/continue control flow:

```
fun count_to_three(): !u32
    var n: u32 = 0
    loop
        set n = n +! 1
        if n >= 3
            break
        end if
    end loop
    ret n
end fun
```

#### Loop While (Conditional Loop)

Conditional loop that checks condition at start of each iteration:

```
fun count_while(): u32
    var n: u32 = 0
    loop while n .< 10
        set n = n + 1
    end loop
    ret n
end fun
```

**Behavior:**
- `loop ... end loop` repeats indefinitely until `break` or `ret`
- `break` exits the innermost loop
- `continue` jumps to the start of the innermost loop
- `break`/`continue` outside a loop is a typecheck error
- Nested loops supported; break/continue affect only the innermost loop
- No loop labels - only innermost loop can be targeted

### 2.7 Operator Argument Semantics

All binary operators and unary operators treat their operands as **immutable references** (`ref`), not by-value (`in`).

**Implications:**
- Operands are read, not consumed
- For copy types: values are implicitly copied to temporaries
- For linear types: values are cloned; originals remain valid after the operation
- A value can be used in multiple operators without explicit cloning

**Example:**
```
let x: int = 42
let a = x + 1    // x is cloned for the operation
let b = x + 2    // x can be used again
ret x            // x is still valid
```

**Implementation Note:** The interpreter evaluates operands into temporary slots. For copy types, this creates a copy. For linear types, the interpreter clones the value so the original remains available.

### 2.8 Module System

**Three-level hierarchy:** library -> package -> module

**Require syntax:**
```
require module sys/std/u32
```

**Import syntax:**
```
import u32.negate
```

Modules are loaded from `.dfm` files. Each package needs a main module (e.g., `std/std.dfm`).

### 2.9 Void Functions

Functions without a return type are void functions:
- Don't require a `ret` statement - function can end without `ret`
- Allow bare `ret` without value for early exit
- Must NOT have `ret` with a value

```
fun log_value(x: u32)        // void - no ret needed
end fun

fun early_exit(n: u32)       // void - bare ret OK
  if n .< 10
    ret
  end if
end fun
```

### 2.10 Intrinsic Calls

Low-level operations that compile directly to machine instructions without function call overhead.

**Syntax:** `icall intrinsic_name(args)`

**Available Intrinsics:**

| Name | Params | Return | Description |
|------|--------|--------|-------------|
| `bitnot_u32` | `u32` | `u32` | Bitwise NOT |
| `bitand_u32` | `u32, u32` | `u32` | Bitwise AND |
| `bitor_u32` | `u32, u32` | `u32` | Bitwise OR |
| `bitxor_u32` | `u32, u32` | `u32` | Bitwise XOR |
| `shl_u32` | `u32, u32` | `u32` | Shift left |
| `shr_u32` | `u32, u32` | `u32` | Shift right (unsigned) |
| `popcount_u32` | `u32` | `u32` | Count set bits |
| `clz_u32` | `u32` | `u32` | Count leading zeros |
| `ctz_u32` | `u32` | `u32` | Count trailing zeros |
| `swap_bytes_u32` | `u32` | `u32` | Byte-swap (endian convert) |
| `reverse_bits_u32` | `u32` | `u32` | Reverse bit order |
| `add_wrapping_u32` | `u32, u32` | `u32` | Add with wrapping |
| `sub_wrapping_u32` | `u32, u32` | `u32` | Subtract with wrapping |
| `mul_wrapping_u32` | `u32, u32` | `u32` | Multiply with wrapping |
| `u32_to_i32` | `u32` | `i32` | Reinterpret as signed |
| `i32_to_u32` | `i32` | `u32` | Reinterpret as unsigned |
| `is_big_endian` | (none) | `bool` | Query platform endianness |

**Example:**
```
require module sys/std/u32
import u32.bitnot

fun bitnot(self: u32): u32
  ret icall bitnot_u32(self)
end fun
```

**Implementation:**
- Intrinsics are defined in `datalove-datafun-intrinsics` crate
- Typechecked against a central definition table
- Interpreter executes via Rust operations
- AOT compiles to inline Cranelift IR instructions (no call overhead)

### 2.11 Type Aliases

Type aliases provide names for structural types, improving readability without creating new types.

**Syntax:**
```
type AliasName: structural_type
```

**Examples:**
```
type Age: u32
type Point: { x: f32, y: f32 }
type Callback: { on_success: bool, data: int }

fun create_point(x: f32, y: f32): Point
  ret { x = x, y = y }
end fun

fun process(p: Point): Age
  ret 25
end fun
```

**Processing Order:**

Type aliases are collected in Pass 0 of typechecking, before function signatures (Pass 1) and statement typechecking (Pass 2). This means:
- Aliases must be defined before use (no forward references)
- Function parameters and return types can reference any alias defined earlier in the file
- Aliases defined in imported modules are available after the import

**Restrictions:**

| Restriction | Error |
|-------------|-------|
| Forward reference | `UnresolvedTypeAlias` |
| Duplicate alias name | `DuplicateTypeAlias` |
| Shadowing primitive type | `CannotShadowPrimitive` |

**Primitives that cannot be shadowed:** `bool`, `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`, `index`, `offset`, `f32`, `f64`, `int`, `string`

**Semantics:**
- Type aliases are purely syntactic - the alias name resolves to the structural type during typechecking
- No runtime representation difference between aliased and structural types
- Aliases can reference other aliases (if defined earlier)
- Aliases can be used in type hints, function parameters, and return types

---

## 3. Type System

### 3.1 Bidirectional Typing

- Expressions **synthesize** types (bottom-up)
- Expressions **check** against expected types (top-down)
- Type hints provide expected types: `: type / expr`

#### Binop Type Propagation

Arithmetic binary operators support bidirectional type propagation, allowing operand types to be inferred from context rather than requiring explicit type hints.

**Checked/Optional Arithmetic (`+!` `-!` `*!` `/!` `+?` `-?` `*?` `/?`):**

When checking against a fixed-int type inside an `ok`/`some` wrapper, the expected type propagates to operands:

```
fun add(): !u32
    ret ok (1 +! 2)    // 1 and 2 infer type u32 from !u32 context
end fun

fun sub(a: i64, b: i64): ?i64
    ret some (a -? b)  // operands checked against i64
end fun
```

Requirements for bidirectional propagation:
- Checked ops (`+!` etc.) require the function to return `!T` (Result type)
- Optional ops (`+?` etc.) require the function to return `?T` (Option type)
- If return type doesn't match, falls through to synthesis (producing proper error)

**Bigint Division (`/!` `/?`):**

Bigint division also supports bidirectional propagation when checking against `int`:

```
fun div(a: int, b: int): !int
    ret ok (a /! b)    // checked against int
end fun
```

**Bare Float Arithmetic (`+` `-` `*` `/`):**

Float literals infer their type from context:

```
fun add(): f32
    ret 1.0 + 2.0      // literals infer f32 from return type
end fun

fun mul(a: f64, b: f64): f64
    ret a * b          // operands checked against f64
end fun
```

**Bare Integer Arithmetic:**

Bare arithmetic on fixed integers (`+` `-` `*`) widens operands to `int` via synthesis. No bidirectional propagation occurs - widening is the correct semantic behavior.

```
fun add(): int
    ret 1 + 2          // synthesizes: both widen to int, result is int
end fun
```

### 3.2 Numeric Widening

Fixed integers widen along chains:
```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
index -> int
offset -> int
```

### 3.3 Coercions

- Any type coerces to `data` (T → data)
- Empty collections check against any element type

**Explicit constructors for Option/Result:**
- `some expr` - wrap in Some variant
- `ok expr` - wrap in Ok variant
- `er expr` - wrap error in Err variant
- `none` - None variant (requires type context)
- `error expr` - error literal (requires type context in Result)

### 3.4 Copy vs Linear Types

| Copy Types | Linear Types |
|------------|--------------|
| bool, u8-u64, i8-i64, index, offset, f32, f64 | int, string, list, map, set, tensor, table, data, error |

Linear types have move semantics; copy types can be freely duplicated.

### 3.5 Move Semantics

**Use after move:** A linear value can only be used once. After being consumed (passed to an `in` parameter, assigned to a variable, etc.), subsequent uses are compile-time errors.

```
let x: int = 42
let y = x        // x is moved into y
let z = x        // ERROR: use of moved value: x
```

**Move in loop:** Moving an outer-scoped linear value inside a loop body is a compile-time error. The loop could iterate multiple times, but the value can only be moved once.

```
var a: int = 4
var b: int = 5
loop
    set a = b    // ERROR: cannot move 'b' in loop
end loop
```

**Exceptions:**
- Copy types (fixed-width integers, bool, f32, f64) can be used freely in loops
- Binary operators borrow their operands (don't consume), so `a + b` doesn't move `a` or `b`

### 3.6 Ownership Analysis

Ownership analysis runs after typechecking and before IR lowering. It performs static analysis of value ownership, detecting errors and computing drop schedules.

#### Analysis Errors

| Code | Error | Description |
|------|-------|-------------|
| D001 | UseAfterMove | Using a value after ownership was transferred |
| D002 | DoubleMove | Transferring ownership twice in sequence |
| D003 | CannotMoveBorrowed | Attempting to move a `ref`/`mut`/`out` parameter |
| D004 | CannotMutFromRef | Passing immutable `ref` where `mut` is required |
| D005 | ReadUninitialized | Reading uninitialized binding (`out` param or uninitialized `var`) |
| D006 | OutParamNotInitialized | Returning without initializing `out` param |
| D007 | MoveInLoop | Moving outer-scoped value inside loop body |
| D008 | InconsistentBranchMove | Value moved in one branch but not another |
| D009 | OutParamPartialWrite | Partial field write to `out` param |

#### Initialization Tracking

Some bindings may be uninitialized at declaration and must be tracked:

| Binding Type | Starts Initialized | Tracking |
|--------------|-------------------|----------|
| `let x = expr` | Yes | Not tracked |
| `var x = expr` | Yes | Tracked (for reassignment) |
| `var x: Type` | No | Tracked (for init + reassignment) |
| `out` param | No | Tracked (for init) |
| `in`/`ref`/`mut` param | Yes | Not tracked for init |

**Initialization state transitions:**
- `Uninitialized` → `Initialized`: on first `set` to the binding
- Reading while `Uninitialized`: compile-time error (D005)
- Scope exit while `Uninitialized`: no drop (nothing to destroy)

**Branch convergence:**
- If a binding is initialized in one branch, it must be initialized in all branches
- The analysis tracks init state through if/else and merges at convergence points
- A binding that's uninitialized on some paths cannot be read after the branch

#### Tracking Categories

Each binding is assigned a tracking category that determines how moves and drops are handled:

| Category | Description | Instructions Used |
|----------|-------------|-------------------|
| Copy | Copy type, no tracking needed | No drops emitted |
| Precise | State statically known at every program point | Precise move/drop (no runtime checks) |
| Tracked | State may vary at runtime | Tracked move/drop (with runtime checks) |

**Tracked bindings include:**
- `var` bindings (mutable slots that may be reassigned)
- `out` parameters (may be uninitialized)
- Uninitialized `var` bindings (declared without initializer)
- Exported script bindings

#### Drop Scheduling

The analysis computes when Drop instructions should be emitted:

| Drop Point | Description |
|------------|-------------|
| Scope exit | When bindings go out of scope |
| Branch exit | For convergence when branches have different ownership states |
| Before return | Cleanup all live bindings |
| Before break/continue | Cleanup bindings before control flow transfer |
| Loop body end | Drop iteration-scoped bindings |
| Before try-return | Cleanup before early return from `?` or `!` operators |

---

## 4. Runtime/REPL

### 4.1 CLI Commands

| Command | Description |
|---------|-------------|
| `script` | Execute a .dfs datafun script |
| `lit-tycheck` | Type check a .dlt expression |
| `lit-ast` | Print AST of datalit expression |
| `lit-pretty` | Pretty-print datalit |
| `lit-op` | Perform operations on datalit values |
| `repl` | Interactive REPL |
| `typecheck-std` | Typecheck the sys/std library |

### 4.2 REPL Capabilities

- Incremental unit execution
- Script-level variable and function tracking
- Module/package loading
- State persistence across commands

---

## Appendix A: Documented But Unimplemented Features

Features from documentation that have no or minimal implementation:

| Feature | Source | Status |
|---------|--------|--------|
| Tensor operations | notes/arrays.md | Parsed only, no runtime |
| Zipper heaps | notes/zipper-heaps.md | Design only |
| `panic` statement | notes/panicking.md | Not implemented |
| Pattern matching / match | demo-datafun-script.dfs | Not implemented |
| `arena` blocks | demo-datafun-script.dfs | Not implemented |
| `memoize` | demo-datafun-script.dfs | Not implemented |
| `@type` introspection | demo-datafun-script.dfs | Not implemented |
| `@data` dynamic type | README.md | Implemented |
| Full Datalove layer | README.md | Not implemented |
