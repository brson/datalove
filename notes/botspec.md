# Datalove Bot Specification

Bot-maintained specification reflecting actual implementation state.
Last verified: 2025-11-25

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
| `u8`, `u16`, `u64` | Unsigned integers | [NOT IMPLEMENTED] |
| `u32` | 32-bit unsigned integer | Implemented |
| `i8`, `i16`, `i32`, `i64` | Signed integers | [NOT IMPLEMENTED] |
| `f32` | 32-bit float | [NOT IMPLEMENTED] |
| `int` | Arbitrary precision signed integer (bigint) | Implemented |
| `string` | UTF-8 string | Implemented |

### 1.2 Collection Types

| Type | Syntax | Example | Interpreter Status |
|------|--------|---------|-------------------|
| List | `[@T]` | `[1, 2, 3]` | [NOT IMPLEMENTED] |
| Map | `@map<@K, @V>` | `@map { 0 = 5, 2 = 2 }` | [NOT IMPLEMENTED] |
| Set | `@set<@T>` | `@set { 1, 2, 3 }` | [NOT IMPLEMENTED] |
| Tensor | `@tensor<@T, N>` | - | [PARTIAL: parsed only] |

### 1.3 Aggregate Types

**Interpreter Status:** All aggregate types are [NOT IMPLEMENTED] in the new interpreter.

**Anonymous Tuple:**
```
: (@bool, @u32) / (@true, 1)
: () / ()
```

**Named Tuple:**
```
: @tuple Bar (@bool, @u32) / @tuple Bar (@true, 1)
```

**Anonymous Struct:**
```
: { field1: @bool, field2: @u32 } / { field1 = @true, field2 = 1 }
```

**Named Struct:**
```
: @struct Foo { field1: @bool } / @struct Foo { field1 = @true }
```

**Anonymous Enum:**
```
: @enum { Foo, Bar(@u32) } / @enum Foo
: @enum { Bar(@u32) } / @enum Bar(2)
```

**Named Enum:**
```
: @enum Quux { Bar(@u32) } / @enum Quux.Bar(1)
```

### 1.4 Special Types

| Type | Syntax | Values | Interpreter Status |
|------|--------|--------|-------------------|
| Option | `@?@T` | value or `@none` | [NOT IMPLEMENTED] |
| Result | `@!@T` | value or `@error "msg"` | [NOT IMPLEMENTED] |
| Data | `@data` | `@data 1`, `@data : int / 1` | [NOT IMPLEMENTED] |
| Error | `@error` | `@error "oops"`, `@error : int / 1` | [NOT IMPLEMENTED] |

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

**Float bitpattern coercion:** Hex literals coerce to f32 bitpattern
```
: @f32 / 0xABABABAB
```

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
| `if` | `if cond ... end if` | [PARTIAL: top-level only, not in function bodies] |

### 2.2 Expressions

| Expression | Example | Status |
|------------|---------|--------|
| Datalit | Any datalit value | Implemented |
| Name/variable | `foo` | Implemented |
| Binary op | `a + b` | Implemented |
| Function call | `foo(a, b)` | Implemented |
| Tuple | `(a, b)` | [NOT IMPLEMENTED in interpreter] |
| Unary op | `-x` | [PARTIAL: parsed] |
| Try option | `expr?` | [NOT IMPLEMENTED in interpreter] |
| Try result | `expr!` | [NOT IMPLEMENTED in interpreter] |

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
**Interpreter:** [DEVIATION] Treats `+!` same as `+` (widens to int instead of checked fixed-int math).

#### Optional Arithmetic (`+? -? *? /?`) - Early-return Option

| Type | `+?` `-?` `*?` | `/?` | Unary `-?` |
|------|----------------|------|------------|
| **f32** | Not allowed | Not allowed | Not allowed |
| **int** | Not allowed | Returns `?int` | Not allowed |
| **Fixed ints** | Returns `?T` (same type) | Returns `?T` | Signed only, returns `?T` |

These operators early-return `none` on overflow/div0. `-?` disallowed for unsigned ints (footgun).

**Tycheck:** Correct per spec.
**Interpreter:** [NOT IMPLEMENTED]

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
**Interpreter:** [NOT IMPLEMENTED]

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

### 2.6 Module System

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
- Anonymous aggregates coerce to named aggregates with matching structure
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
| Comparison operators | demo-datafun-script.dfs | Parsed, not interpreted |
| Try operators `?` `!` | demo-datafun-script.dfs | Parsed, not interpreted |
| `if` in function body | demo-datafun-script.dfs | Not interpreted |
| Pattern matching / match | demo-datafun-script.dfs | Not implemented |
| `loop`/`break`/`continue` | demo-datafun-script.dfs | Not implemented |
| `arena` blocks | demo-datafun-script.dfs | Not implemented |
| `memoize` | demo-datafun-script.dfs | Not implemented |
| `var`/`set` mutation | demo-datafun-script.dfs | Not implemented |
| `@type` introspection | demo-datafun-script.dfs | Not implemented |
| Full Datalove layer | README.md | Not implemented |
