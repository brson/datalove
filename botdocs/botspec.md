# Datalove Bot Specification

Bot-maintained specification reflecting actual implementation state.
Last verified: 2025-12-17

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
| `f32` | 32-bit float | Implemented |
| `int` | Arbitrary precision signed integer (bigint) | Implemented |
| `string` | UTF-8 string | Implemented |

### 1.2 Collection Types

| Type | Syntax | Example | Interpreter Status |
|------|--------|---------|-------------------|
| List | `[@T]` | `[1, 2, 3]` | Implemented |
| Map | `@map<@K, @V>` | `@map { 0 = 5, 2 = 2 }` | Implemented |
| Set | `@set<@T>` | `@set { 1, 2, 3 }` | Implemented |
| Tensor | `@tensor<@T, N>` | - | [PARTIAL: parsed only] |

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
| `fun` | `fun name(...): ret_type ... end fun` | Implemented |
| `ret` | `ret expr` | Implemented |
| `require module` | `require module sys/std/bool` | Implemented |
| `require data` | `require data name` | [PARTIAL: parsed] |
| `import` | `import module_name.item_name` | Implemented |
| `if` | `if cond ... end if` | Implemented (in function bodies) |
| `if` with binding | `if opt \|value\| ... end if` | Implemented (option/result unwrap) |
| `loop` | `loop ... end loop` | Implemented (in function bodies) |
| `break` | `break` | Implemented (exits innermost loop) |
| `continue` | `continue` | Implemented (jumps to loop start) |

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
| Try option | `expr?` | Implemented (early-return on none) |
| Try result | `expr!` | Implemented (early-return on error) |

### 2.3 Operators

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

### 2.4 Function Definitions

```
fun name(param1: type1, param2: type2): return_type
  let x = param1 + param2
  ret x
end fun
```

### 2.5 Parameter Modes

| Mode | Syntax | Meaning | Interpreter Status |
|------|--------|---------|-------------------|
| `in` | (default) | By value | Implemented |
| `out` | `param: out type` | By mut pointer | [NOT IMPLEMENTED] |
| `ref` | `param: ref type` | By reference | [NOT IMPLEMENTED] |
| `mut` | `param: mut type` | By mut reference | [NOT IMPLEMENTED] |

### 2.6 Loop Statements

Unconditional loop with break/continue control flow:

```
fun count_to_three(): !u32
    let n: u32 = @0
    loop
        let n = n +! @1
        if n >= @3
            break
        end if
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

**Implementation notes:**
- CFG builder creates loop header and exit blocks
- Loop stack tracks nesting for break/continue targets
- Typechecker tracks loop depth to validate break/continue placement

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
```

### 3.3 Coercions

- Values implicitly wrap to `Some`/`Ok` when checking against Option/Result
- Any type coerces to `data` (T → data)
- Data values coerce to `?data` and `!data` (data → Option<data>, data → Result<data>)
- Empty collections check against any element type

### 3.4 Copy vs Linear Types

| Copy Types | Linear Types |
|------------|--------------|
| bool, u8-u64, i8-i64, f32 | int, string, list, map, set, data, error |

Linear types have move semantics; copy types can be freely duplicated.

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
| `docs` | Generate documentation |

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
| `var`/`set` mutation | demo-datafun-script.dfs | Not implemented |
| `@type` introspection | demo-datafun-script.dfs | Not implemented |
| `@data` dynamic type | README.md | Implemented |
| Full Datalove layer | README.md | Not implemented |
