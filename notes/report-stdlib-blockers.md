# Compiler Improvements to Unblock Stdlib Development

This report identifies compiler/language features that would enable more stdlib functionality, based on hands-on experience implementing bool.dfm, u32.dfm additions, and result.dfm.

## Executive Summary

The stdlib is currently limited by three categories of blockers:

1. **Linear type analysis bugs** - False positives blocking valid code patterns
2. **Missing language features** - No generics, no tuple access, no closures
3. **Missing runtime intrinsics** - Bit operations, string/list operations

The highest-impact fixes are the linear type analysis bugs, which block result combinators and all int.dfm functions.

---

## Category 1: Linear Type Analysis Bugs (High Priority)

### 1.1 UseAfterMove False Positive in Branching

**Problem:** The move analysis incorrectly flags valid patterns where a linear parameter is used in different branches of an `if` statement.

**Example (result.dfm):**
```datafun
fun or_result(self: !u32, other: !u32): !u32
  if self |value|
    ret self      // <-- Analysis says UseAfterMove here
  else |error|
    ret other
  end if
end fun
```

This is semantically valid: `self` is consumed exactly once regardless of which branch executes. The analysis appears to be treating the `if self |value|` destructuring as a move, then flagging the `ret self` in the then-branch as a second use.

**Impact:** Blocks `or_result`, `and_result` for result.dfm. Similar patterns would be needed for option combinators.

**Fix approach:** The move analysis needs to understand that:
- In `if x |binding|`, the value flows into `binding`, but `x` itself isn't "moved" in a way that prevents returning it
- Or alternatively, the pattern should allow returning the original value from the same branch that destructured it

### 1.2 Linear Types Block All int.dfm Functions

**Problem:** The `int` type (bigint) is linear, meaning comparisons consume the value.

**Example:**
```datafun
fun abs(self: int): int
  if self .< 0      // <-- self is moved here
    ret -self       // <-- Error: self already moved
  else
    ret self        // <-- Error: self already moved
  end if
end fun
```

**Impact:** Blocks ALL of:
- `abs`, `signum`, `is_positive`, `is_negative`, `is_zero`
- `max`, `min`, `clamp`
- Any function that needs to both compare and return an int

**Fix approaches (choose one):**
1. **Borrow semantics for comparisons** - Comparisons take `&int` implicitly
2. **Clone intrinsic** - Add `clone(self: int): int` runtime function
3. **Copy semantics for int** - Make int a copy type (may have performance implications)
4. **Runtime intrinsics** - Implement these functions in the runtime instead of stdlib

---

## Category 2: Missing Language Features (Medium Priority)

### 2.1 No Generics

**Problem:** All stdlib functions are monomorphic. We have separate implementations for `?u32` and `!u32`.

**Impact:**
- Code duplication across types (option.dfm works for `?u32` only)
- No way to write generic containers or utilities
- Users can't easily use stdlib for their own types

**Current workaround:** Monomorphic modules (option works for `?u32` only).

**Fix:** Implement generic function support. This is a large feature but would dramatically improve stdlib expressiveness.

### 2.2 No Tuple Field Access

**Problem:** Can't access tuple fields by index or destructure tuples.

**Impact:** Can't implement tuple utilities like `first`, `second`, `swap`.

**Example (not currently possible):**
```datafun
fun first(self: (u32, u32)): u32
  ret self.0    // No syntax for this
end fun
```

**Fix:** Add tuple destructuring syntax (either `let (a, b) = tuple` or field access `tuple.0`).

### 2.3 No First-Class Functions / Closures

**Problem:** Can't pass functions as arguments.

**Impact:** Blocks all higher-order functions:
- `option.map`, `option.filter`, `option.and_then`
- `result.map`, `result.map_err`
- `list.map`, `list.filter`, `list.fold`

**Fix:** Implement function types and closure support. This is a significant feature.

### 2.4 Error Literal Coercion in Scripts

**Problem:** `@error "message"` doesn't coerce to `!T` in script contexts.

**Example:**
```datafun
// In script (not inside a function):
let err: !u32 = @error "test"   // Type error: can't coerce @error to !u32
```

**Impact:** Can't test error paths in std_tests. All result.dfm tests only cover the Ok path.

**Fix:** Enable error-to-result coercion in script contexts, same as function contexts.

---

## Category 3: Missing Runtime Intrinsics (Lower Priority)

These require runtime support and are expected to need implementation work.

### 3.1 Bit Operations

**Needed for u32.dfm:**
- `bitnot`, `bitand`, `bitor`, `bitxor`
- `shift_left`, `shift_right`
- `count_ones`, `count_zeros`
- `leading_zeros`, `trailing_zeros`

**Current state:** All stubbed with `// todo`, return 0 or @none.

### 3.2 String Operations

**Needed for string.dfm:**
- `len`, `is_empty`
- `concat`, `substring`
- `starts_with`, `ends_with`, `contains`
- `to_uppercase`, `to_lowercase`

### 3.3 List Operations

**Needed for list.dfm:**
- `len`, `is_empty`
- `get`, `first`, `last`
- `push`, `pop`, `insert`, `remove`
- `concat`, `reverse`

### 3.4 Integer Conversion

**Needed for numeric modules:**
- `to_string` for all numeric types
- `parse` from string to numeric types
- `cast` between integer types

### 3.5 Checked/Saturating/Wrapping Arithmetic

**Needed for u32.dfm stubs:**
- `add_checked`, `sub_checked`, `mul_checked` - detect overflow
- `add_saturating`, `sub_saturating` - clamp at bounds
- `add_wrapping`, `sub_wrapping` - wrap around

The language has `+!` and `+?` operators that return Result/Option, but the stdlib `_checked` functions are stubbed.

---

## Recommended Priority Order

### Phase 1: Quick Wins (Unblock existing patterns)

1. **Fix UseAfterMove false positive in branching** - Unblocks result/option combinators
2. **Fix error literal coercion in scripts** - Enables testing error paths

### Phase 2: Linear Type Improvements (Unblock int.dfm)

3. **Add borrow semantics for comparisons** OR **int clone intrinsic** - Unblocks all int.dfm functions

### Phase 3: Core Language Features

4. **Tuple destructuring** - Small feature, enables tuple utilities
5. **Generics** - Large feature, but transforms stdlib expressiveness

### Phase 4: Runtime Intrinsics

6. **Bit operations** - Complete u32.dfm
7. **String operations** - Enable string.dfm
8. **List operations** - Enable list.dfm

---

## Current Stdlib Status

| Module | Functions | Status |
|--------|-----------|--------|
| bool.dfm | 6 | Complete (not, and, or, xor, implies, then_some) |
| u32.dfm | 43 | 8 working, 35 stubbed (need intrinsics) |
| option.dfm | 7 | Complete for ?u32 |
| result.dfm | 5 | Partial (or_result/and_result blocked by analysis bug) |
| int.dfm | 0 | Blocked entirely by linear type semantics |
| list.dfm | 0 | Blocked by missing intrinsics |

---

## Appendix: Specific Code Patterns That Fail

### A. or_result pattern (UseAfterMove)
```datafun
fun or_result(self: !u32, other: !u32): !u32
  if self |value|
    ret self      // Error: UseAfterMove
  else |error|
    ret other
  end if
end fun
```

### B. int comparison pattern (linear move)
```datafun
fun is_negative(self: int): bool
  ret self .< 0   // self moved by comparison, can't return bool
end fun

fun max(self: int, other: int): int
  if self .>= other   // both moved
    ret self          // Error: already moved
  else
    ret other         // Error: already moved
  end if
end fun
```

### C. Error literal in script
```datafun
// test script
let err: !u32 = @error "test"   // Type error
let result = is_err(err)
```
