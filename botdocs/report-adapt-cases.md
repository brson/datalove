# Adapt Operator (`@`) Recoverable Error Cases

This document catalogs all type checker and ownership errors that can be recovered
by inserting the `@` (adapt) operator. This analysis supports the future "auto-adapt"
mode that will discover these errors and automatically suggest or insert `@`.

## Overview

The `@` operator performs two operations:

1. **Clone**: Creates a deep copy of linear values (string, int, list, map, set,
   tensor, data, error, tables)
2. **Widen**: Performs lossless integer type conversion along signedness chains,
   and f32 to f64

## Type System Background

### Copy Types (no tracking needed)
- `bool`, `u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`
- `index`, `offset`, `f32`, `f64`

### Linear Types (must be used exactly once)
- `int` (arbitrary precision)
- `string`
- Collections: `list`, `map`, `set`, `tensor`
- `data`, `error`
- Tables `{| col: Type |}`

### Widening Chains

Standard widening (implicit in many contexts):
```
u8 → u16 → u32 → u64 → int
i8 → i16 → i32 → i64 → int
index → int
offset → int
```

Cross-sign widening (requires `@`):
```
u8 → i16, i32, i64, int
u16 → i32, i64, int
u32 → i64, int
```

Float widening (requires `@`, never implicit):
```
f32 → f64
```

---

## Type Checker Errors (F-codes)

### F016: Type Mismatch

The most common adapt-recoverable error. Occurs when expression type doesn't match
expected type but `@` could fix it.

#### Case 1: Integer Widening Needed

**Scenario**: Have a narrower integer type, need a wider one.

```datalove
fn process(x: int) -> int
    ret x

fn example() -> int
    let n: u32 = 42
    ret process(n)  // ERROR: expected int, found u32
```

**Fix with @**:
```datalove
fn example() -> int
    let n: u32 = 42
    ret process(n@)  // OK: u32 widens to int
```

**Detection heuristic**: `can_clone_coerce_to(actual, expected)` returns true.

**Source locations**:
- `check.rs:501-526` - General fallback type checking
- `check.rs:487-491` - Binary operator result checking
- `synthesize.rs:460-467` - Binary operand type mismatch

#### Case 2: Cross-Sign Widening Needed

**Scenario**: Have an unsigned type, need a larger signed type.

```datalove
fn needs_signed(x: i32) -> i32
    ret x

fn example() -> i32
    let n: u8 = 255
    ret needs_signed(n)  // ERROR: expected i32, found u8
```

**Fix with @**:
```datalove
fn example() -> i32
    let n: u8 = 255
    ret needs_signed(n@)  // OK: u8 can widen to i32 via @
```

**Detection heuristic**: Check cross-sign widening rules:
- `u8` → `i16`, `i32`, `i64`, `int`
- `u16` → `i32`, `i64`, `int`
- `u32` → `i64`, `int`

**Source location**: `datalove-datafun-common/src/lib.rs:388-394`

#### Case 3: Clone Needed for Function Argument

**Scenario**: Passing a linear value to a function that consumes it, but want to
keep the original.

```datalove
fn consume(s: string) -> string
    ret s

fn example() -> string
    let msg = "hello"
    let copy1 = consume(msg)    // msg is moved here
    let copy2 = consume(msg)    // ERROR: use after move (becomes type mismatch in some contexts)
    ret copy2
```

**Fix with @**:
```datalove
fn example() -> string
    let msg = "hello"
    let copy1 = consume(msg@)   // Clone msg, original stays valid
    let copy2 = consume(msg)    // OK: msg is still valid
    ret copy2
```

#### Case 4: Collection Element Type Mismatch

**Scenario**: Collection element has narrower type than expected.

```datalove
fn example() -> list<int>
    let x: u8 = 10
    ret [x]  // ERROR: expected list<int>, element is u8
```

**Fix with @**:
```datalove
fn example() -> list<int>
    let x: u8 = 10
    ret [x@]  // OK: x widens to int
```

**Source locations**:
- `check.rs:551-574` - `check_list_elements`
- `check.rs:577-600` - `check_set_elements`
- `check.rs:603-630` - `check_map_entries`

#### Case 5: Tuple/Struct Field Type Mismatch

**Scenario**: Tuple or struct field has wrong type but can be adapted.

```datalove
fn example() -> (int, int)
    let a: u16 = 100
    let b: u32 = 200
    ret (a, b)  // ERROR: expected (int, int), found (u16, u32)
```

**Fix with @**:
```datalove
fn example() -> (int, int)
    let a: u16 = 100
    let b: u32 = 200
    ret (a@, b@)  // OK: both widen to int
```

**Source locations**:
- `check.rs:678-713` - `check_tuple_elements`
- `check.rs:716-756` - `check_struct_fields`

#### Case 6: Binary Operator Result Type

**Scenario**: Arithmetic on fixed integers widens to `int`, but context expects
the original type.

```datalove
fn example() -> u32
    let a: u32 = 10
    let b: u32 = 20
    ret a + b  // ERROR: expected u32, found int (arithmetic widens)
```

**Note**: This case is NOT recoverable with `@` because the arithmetic result IS
`int`. The user needs to use checked arithmetic (`+!`, `+?`) instead. This is
intentional - we want to force explicit overflow handling for fixed integers.

---

## Ownership Errors (D-codes)

### D001: UseAfterMove

**Scenario**: Using a linear value after ownership was transferred.

```datalove
fn consume(s: string) -> string
    ret s

fn example() -> string
    let msg = "hello"
    let a = consume(msg)   // msg is moved
    let b = consume(msg)   // ERROR D001: use after move
    ret b
```

**Fix with @**:
```datalove
fn example() -> string
    let msg = "hello"
    let a = consume(msg@)  // Clone msg, original stays valid
    let b = consume(msg)   // OK: msg is still live
    ret b
```

**Detection heuristic**: When D001 is raised, check if cloning the value before
its first consuming use would allow subsequent uses.

**Source location**: `ownership/src/lib.rs:539-542`

### D002: DoubleMove

**Scenario**: Attempting to transfer ownership twice.

```datalove
fn consume(s: string) -> string
    ret s

fn example() -> (string, string)
    let msg = "hello"
    ret (consume(msg), consume(msg))  // ERROR D002: double move
```

**Fix with @**:
```datalove
fn example() -> (string, string)
    let msg = "hello"
    ret (consume(msg@), consume(msg))  // Clone for first use
```

Or:
```datalove
fn example() -> (string, string)
    let msg = "hello"
    ret (consume(msg@), consume(msg@))  // Clone both if needed
```

**Detection heuristic**: When D002 is raised at a specific expression, check if
cloning would resolve the double-move. One clone is sufficient if there are
exactly two uses.

**Source location**: `ownership/src/lib.rs:349-352`

### D007: MoveInLoop

**Scenario**: Moving an outer-scoped value inside a loop body.

```datalove
fn consume(s: string) -> string
    ret s

fn example() -> string
    let msg = "hello"
    var result = ""
    loop
        set result = consume(msg)  // ERROR D007: move in loop
        if result.len() > 5
            break
    ret result
```

**Fix with @**:
```datalove
fn example() -> string
    let msg = "hello"
    var result = ""
    loop
        set result = consume(msg@)  // Clone on each iteration
        if result.len() > 5
            break
    ret result
```

**Detection heuristic**: When D007 is raised, the fix is always to clone with `@`
inside the loop.

**Source location**: `ownership/src/lib.rs:1335-1341`

---

## Errors NOT Recoverable with @

### D003: CannotMoveBorrowed

Borrowed parameters (`ref`, `mut`, `out`) cannot be moved because the caller
retains ownership. Cloning doesn't help because the issue is about borrowing
semantics, not ownership.

```datalove
fn example(ref s: string) -> string
    ret s  // ERROR D003: cannot move borrowed parameter
```

**Not fixable with @**: The function signature needs to change to `in s: string`.

### D004: CannotMutFromRef

Passing an immutable ref where mutable is required. Cloning creates an owned
value, not a mutable reference.

```datalove
fn mutate(mut s: string)
    // ...

fn example(ref s: string)
    mutate(s)  // ERROR D004: cannot get mut from ref
```

**Not fixable with @**: The parameter mode needs to change.

### D005: ReadUninitialized

Reading a binding before it's initialized. Cloning doesn't help - there's no
value to clone.

```datalove
fn example() -> string
    var s: string
    ret s  // ERROR D005: read uninitialized
```

### D006: OutParamNotInitialized

Returning without initializing an `out` parameter. Not related to cloning.

```datalove
fn example(out result: string)
    ret  // ERROR D006: out param not initialized
```

### F011: CannotSynthesize

The `@` operator itself requires type context - it cannot synthesize a type.
This is by design.

```datalove
let x = value@  // ERROR F011: @ requires type context
```

**Fix**: Provide type annotation:
```datalove
let x: int = value@  // OK: context provides expected type
```

---

## Auto-Adapt Implementation Strategy

### Phase 1: Detection

For each error, determine if it's adapt-recoverable:

1. **F016 TypeMismatch**: Call `can_clone_coerce_to(actual, expected)`. If true,
   error is recoverable.

2. **D001 UseAfterMove**: Track which expression caused the move. If that
   expression could be wrapped with `@`, error is recoverable.

3. **D002 DoubleMove**: Same as D001 - identify the first move, suggest `@`.

4. **D007 MoveInLoop**: Always recoverable with `@` on the moved expression.

### Phase 2: Suggestion

Generate a diagnostic note suggesting `@`:

```datalove
error[D001]: use after move
  --> example.dfs:5:20
   |
 4 |     let a = consume(msg)
   |                     --- value moved here
 5 |     let b = consume(msg)
   |                     ^^^ error: use after move
   |
   = help: consider using `msg@` to clone the value before the first use
```

### Phase 3: Auto-Fix (Optional)

In auto-fix mode, automatically insert `@`:

1. Parse the error location
2. Identify the expression to adapt
3. Rewrite `expr` to `expr@`
4. Re-run type checking to verify fix

### Key Functions for Implementation

- `can_clone_coerce_to()` in `datalove-datafun-common/src/lib.rs:367-395`
- `analyze_expr_moves()` in `ownership/src/lib.rs:526-719`
- `mark_moved()` in `ownership/src/lib.rs:341-359`

---

## Summary Table

| Error Code | Error Name | Recoverable? | Fix |
|------------|------------|--------------|-----|
| F016 | TypeMismatch (widening) | Yes | `value@` |
| F016 | TypeMismatch (cross-sign) | Yes | `value@` |
| F016 | TypeMismatch (clone needed) | Yes | `value@` |
| D001 | UseAfterMove | Yes | Clone before first use: `value@` |
| D002 | DoubleMove | Yes | Clone for one use: `value@` |
| D007 | MoveInLoop | Yes | Clone in loop: `value@` |
| D003 | CannotMoveBorrowed | No | Change function signature |
| D004 | CannotMutFromRef | No | Change parameter mode |
| D005 | ReadUninitialized | No | Initialize variable |
| D006 | OutParamNotInitialized | No | Initialize out param |
| F011 | CannotSynthesize (`@` itself) | N/A | Add type annotation |

---

## Test Cases for Auto-Adapt

The following test scenarios should be implemented:

### Widening Cases
1. `u8` → `u16`, `u32`, `u64`, `int`
2. `i8` → `i16`, `i32`, `i64`, `int`
3. `u8` → `i16` (cross-sign)
4. `u16` → `i32` (cross-sign)
5. `u32` → `i64` (cross-sign)

### Clone Cases
1. String passed to consuming function
2. List passed to consuming function
3. String used twice in same expression
4. Value moved then used again
5. Value moved in loop body

### Collection Cases
1. List with narrower element types
2. Map with narrower key/value types
3. Set with narrower element types
4. Tuple with narrower field types
5. Struct with narrower field types

### Edge Cases
1. Nested adapt: `((x@)@)` should simplify to `(x@)`
2. Already adapted: Don't suggest `@` if already present
3. Non-linear type: Don't suggest `@` for copy types (unnecessary)
4. Type incompatible: Don't suggest `@` when conversion impossible
