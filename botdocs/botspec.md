# Datalove Bot Specification

Bot-maintained specification reflecting actual implementation state.
Last verified: 2026-01-08

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
| `loop carry` | `loop carry (i = 0) ... end loop` | Implemented (loop with iteration state) |
| `loop bring` | `loop ... end loop bring (x)` | Implemented (loop that produces values) |
| `loop while` | `loop while cond ... end loop` | Implemented (conditional loop) |
| `break` | `break` or `break(values...)` | Implemented (exits loop, optionally with bring values) |
| `continue` | `continue` or `continue(values...)` | Implemented (next iteration, optionally with new carries) |

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

#### Loop with Carry (Iteration State)

Carries pass explicit iteration state via block parameters. Mnemonic: "continue and carry".

```
fun factorial(n: u32): u32
    loop carry (acc: u32 = @1, i = n)
        if i <= @1
            break
        end if
        continue(acc * i, i - @1)
    end loop
    ret acc
end fun
```

**Carry syntax:**
- `loop carry (name: type = init, ...)` - declare carries with initial values
- Type annotations optional (inferred from init expression)
- Carry bindings are visible inside the loop body
- `continue(values...)` passes new values to next iteration
- Plain `continue` not allowed - must provide values (prevents accidental reuse)

**Static analysis:** Loops with carries must not fall through. Every path must explicitly `break`, `continue(...)`, or `ret`. This prevents accidentally forgetting to update carry values.

#### Loop with Bring (Exit Values)

Brings capture values when the loop exits via break. Mnemonic: "break and bring".

```
fun find_first_over(threshold: u32): u32
    var x: u32 = @0
    loop
        set x = x + @1
        if x .> threshold
            break(x)
        end if
    end loop bring (found: u32)
    ret found
end fun
```

**Bring syntax:**
- `end loop bring (name: type, ...)` - declare bindings assigned on break
- Type annotations required (cannot infer without seeing break values first)
- Bring bindings are visible after the loop
- `break(values...)` provides values for bring bindings
- `break` without values when loop has brings is a typecheck error

**Static analysis:** Loops with only brings (no carries) can fall through - it just means "keep looping until we break".

#### Combined Carry and Bring

```
fun factorial(n: u32): u32
    loop carry (acc: u32 = @1, i = n)
        if i <= @1
            break(acc)
        end if
        continue(acc * i, i - @1)
    end loop bring (result: u32)
    ret result
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

**Loop while with carries:**

When `loop while` has carries but no brings, carries are implicitly dropped when the condition is initially false:

```
fun sum_to(limit: u32): u32
    var total: u32 = @0
    loop carry (i: u32 = @0) while i .< limit
        set total = total + i
        continue(i + @1)
    end loop
    ret total
end fun
```

**Loop while with brings (requires else break):**

When `loop while` has brings, an `else break` clause is required to provide bring values when the condition is initially false:

```
fun first_over(threshold: u32): u32
    var x: u32 = @0
    loop while x .<= threshold else break (x)
        set x = x + @1
        if x .> @100
            break(x)
        end if
    end loop bring (result: u32)
    ret result
end fun
```

**Full combined form:**

```
fun factorial(n: u32): u32
    loop carry (acc: u32 = @1, i = n) while i .> @1 else break (acc)
        continue(acc * i, i - @1)
    end loop bring (result: u32)
    ret result
end fun
```

**Loop while syntax:**
- `loop while condition` - basic conditional loop
- `loop carry (...) while condition` - carries only, implicit drop when false
- `loop while condition else break (values) ... end loop bring (...)` - brings require else break
- `else break` is NOT allowed without brings (type error)
- `else break (values)` must match bring types and arity

**Behavior:**
- `loop ... end loop` repeats indefinitely until `break` or `ret`
- `break` exits the innermost loop
- `break(values...)` exits and assigns bring bindings
- `continue` jumps to the start of the innermost loop
- `continue(values...)` jumps with new carry values
- `break`/`continue` outside a loop is a typecheck error
- Nested loops supported; break/continue affect only the innermost loop
- No loop labels - only innermost loop can be targeted

**Implementation notes:**
- Uses SSA block parameters (not Phi nodes) for carries/brings
- Loop header block has params for carries; exit block has params for brings
- Goto/Branch terminators pass args to target blocks
- Typechecker validates arity and types of break/continue values

**Carry/bring value semantics:**
- Carry/bring bindings define IR values with fixed frame locations
- `continue(values...)` and `break(values...)` MOVE their arguments INTO the carry/bring locations
- Interpreter: `pass_block_args()` copies data into block param frame locations, marks source dropped
- AOT scalars: pure SSA (Cranelift block param IS the value, no frame location)
- AOT aggregates: block param is pointer to source, memcpy to local frame location on block entry

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
| bool, u8-u64, i8-i64, f32 | int, string, list, map, set, data, error |

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

**Solution - use carry bindings:** For mutable state across loop iterations, use `loop carry`:

```
loop carry (a = :int/@4, b = :int/@5)
    if a == b
        break
    end if
    continue(b, a)   // explicit state passing
end loop
```

**Exceptions:**
- Copy types (fixed-width integers, bool, f32) can be used freely in loops
- Carry bindings are designed for loop iteration state and follow different rules
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
