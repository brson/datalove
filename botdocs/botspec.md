# Datalove Bot Specification

Bot-maintained specification reflecting actual implementation state.
Last verified: 2026-01-17

## Overview

Datalove is a typed scripting language with a three-layer tower:
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
| `usize` | Unsigned index type (32 or 64-bit) | Implemented |
| `isize` | Signed index type (32 or 64-bit) | Implemented |
| `f32` | 32-bit float | Implemented |
| `int` | Arbitrary precision signed integer (bigint) | Implemented |
| `string` | UTF-8 string | Implemented |

**Index Types (usize/isize):**

`usize` and `isize` are platform-configurable index types used for collection sizes, capacities, and array indices. Their bit width is controlled by the `index-64` compile-time feature:

| Feature | `usize` | `isize` |
|---------|---------|---------|
| Default | u32 | i32 |
| `index-64` | u64 | i64 |

These types widen to `int` like other fixed integers. See `botdocs/index-64.md` for details on the feature.

### 1.2 Collection Types

| Type | Syntax | Example | Interpreter Status |
|------|--------|---------|-------------------|
| List | `[@T]` | `[1, 2, 3]` | Implemented |
| Map | `@map<@K, @V>` | `@map { 0 = 5, 2 = 2 }` | Implemented |
| Set | `@set<@T>` | `@set { 1, 2, 3 }` | Implemented |
| Tensor | `@tensor<@T, N>` | - | [PARTIAL: parsed only] |
| Table | `{| col: @T, ... |}` | `{| x, y; 1, 2 |}` | Implemented |

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
  @1, @2
  @3, @4
|}
```

Column projections (e.g., `table.x`) have type "list of column type" but cannot be mutated or moved. They can be passed to `ref`-mode function arguments.

### 1.3 Aggregate Types

**Anonymous Tuple:** (Implemented)
```
: (@bool, @u32) / (@true, 1)
: () / ()
```

**Anonymous Struct:** (Implemented)
```
: { field1: @bool, field2: @u32 } / { field1 = @true, field2 = 1 }
```

**Anonymous Enum:** (Implemented)
```
: @enum { Foo, Bar(@u32) } / @enum Foo
: @enum { Bar(@u32) } / @enum Bar(2)
```

Note: Named tuples, structs, and enums were removed from the language.

### 1.4 Special Types

| Type | Syntax | Values | Interpreter Status |
|------|--------|--------|-------------------|
| Option | `?@T` | value or `@none` | Implemented |
| Result | `!@T` | value or `@error "msg"` | Implemented |
| Data | `@data` | `@data 1`, `@data : int / 1` | Implemented |
| Error | `@error` | `@error "oops"`, `@error : int / 1` | Implemented (as result payload) |

### 1.5 Heap Annotations

Every type has an associated heap:
- `@` - Local heap (default)
- `#` - Global heap
- (omitted) - Inferred

```
: @u32 / @1    // local
: #u32 / #1    // global
: u32 / 1      // inferred
```

### 1.6 Literal Syntax

**Type hint syntax:** `: type / expression`
```
: @u32 / 42
```

**Hex literals:** `0x` prefix for hexadecimal values
```
: @u32 / 0xFF        // integer value 255
: @u8 / 0x7F         // integer value 127
: @f32 / 0xABABABAB  // f32 bit pattern coercion
```
Hex literals can be used with any integer type or f32. With f32, the hex value is interpreted as a raw bit pattern.

---

## 2. Datafun Layer

### 2.1 Statements

| Statement | Syntax | Status |
|-----------|--------|--------|
| `let` | `let name: type = expr` | Implemented |
| `var` | `var name: type = expr` | Implemented (mutable slot) |
| `set` | `set name = expr` | Implemented (mutate var or mut/out param) |
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
| **f32** | Returns f32 | Returns f32 | Returns f32 |
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
| **f32** | Not allowed | Not allowed | Not allowed |
| **int** | Not allowed | Returns `!int` | Not allowed |
| **Fixed ints** | Returns `!T` (same type) | Returns `!T` | Returns `!T` |

These operators early-return on overflow/div0, requiring the enclosing function to return `!T`.

**Tycheck:** Correct per spec.
**Interpreter:** Correct per spec (early-returns error on overflow).

#### Optional Arithmetic (`+? -? *? /?`) - Early-return Option

| Type | `+?` `-?` `*?` | `/?` | Unary `-?` |
|------|----------------|------|------------|
| **f32** | Not allowed | Not allowed | Not allowed |
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

### 2.6 Loop Statements

#### Basic Loop

Unconditional loop with break/continue control flow:

```
fun count_to_three(): !u32
    var n: u32 = @0
    loop
        set n = n +! @1
        if n >= @3
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
    var n: u32 = @0
    loop while n .< @10
        set n = n + @1
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

---

## 3. Type System

### 3.1 Bidirectional Typing

- Expressions **synthesize** types (bottom-up)
- Expressions **check** against expected types (top-down)
- Type hints provide expected types: `: type / expr`

### 3.2 Numeric Widening

Fixed integers widen along chains:
```
u8 -> u16 -> u32 -> u64 -> int
i8 -> i16 -> i32 -> i64 -> int
usize -> int
isize -> int
```

### 3.3 Coercions

- Any type coerces to `data` (T → data)
- Empty collections check against any element type

**Explicit constructors for Option/Result:**
- `some expr` - wrap in Some variant
- `ok expr` - wrap in Ok variant
- `er expr` - wrap error in Err variant
- `@none` - None variant (requires type context)
- `@error expr` - error literal (requires type context in Result)

### 3.4 Copy vs Linear Types

| Copy Types | Linear Types |
|------------|--------------|
| bool, u8-u64, i8-i64, usize, isize, f32 | int, string, list, map, set, tensor, table, data, error |

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
- Copy types (fixed-width integers, bool, f32) can be used freely in loops
- Binary operators borrow their operands (don't consume), so `a + b` doesn't move `a` or `b`

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
