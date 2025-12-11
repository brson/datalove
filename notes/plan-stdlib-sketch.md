# Project: see how much datalove std we can write

We have a standard library in sys/std,
and a test suite for it in std_tests.

We have recently improved the interpreter
such that most of the features in the botspec are supported.

lets do this:

reevaluate the state of std and std_tests:
does the existing implementation make sense and does it have thorough test coverage?

then lets consider filling out the existing std modules
with features that can be built off the existing language
features -
that is also without adding any runtime calls from std,
for which we don't yet have a mechanism.

make a list of potential functions to add to existing std modules.
mostly consider rust's core library in comparison.

make a list of potential basic core modules and their functions to add to std
to support the current datalove featureset

---

# Analysis

## Current State of sys/std

### Existing Modules

1. **bool.dfm** - Empty (0 bytes)
2. **int.dfm** - Empty (0 bytes)
3. **list.dfm** - Empty (0 bytes)
4. **u32.dfm** - Has 40 function stubs, but most are unimplemented (`// todo`)
5. **option.dfm** - 7 implemented functions for `?u32` (monomorphic)

### Test Coverage (std_tests)

7 passing tests, all for u32 and option modules:
- `01_u32_bits` - tests `bits()` constant function
- `02_option_is_some_is_none` - tests `is_some`, `is_none`
- `03_option_unwrap_or` - tests `unwrap_or`
- `04_option_unwrap_or_zero` - tests `unwrap_or_zero`
- `05_option_or_option` - tests `or_option`
- `06_option_and_option` - tests `and_option`
- `07_option_xor_option` - tests `xor_option`

### Issues with Current Implementation

1. **u32.dfm**: Most functions are stubs returning 0 or `@none`. Only `bits()`, `min_value()`, `max_value()` and a few shift/rotate functions are meaningful. Functions like `count_ones`, `bitnot`, `bitand`, etc. would require runtime intrinsics.

2. **option.dfm**: Hardcoded to `?u32` - not generic. This is a limitation without generics.

3. **Empty modules**: bool.dfm, int.dfm, list.dfm have no content.

---

## Available Language Features (from botspec)

**Can use for stdlib functions:**
- Function definitions with parameters
- `let` bindings
- `if/else` conditionals
- `if |value|` option/result destructuring
- Comparison operators: `.<`, `.>`, `<=`, `>=`, `==`, `!=`
- Arithmetic: `+ - *` (bare, widening for fixed ints)
- Checked arithmetic: `+! -! *! /!` (early-return `!T`)
- Optional arithmetic: `+? -? *? /?` (early-return `?T`)
- Unary negation: `-` (int), `-?` (signed fixed), `-!` (signed fixed)
- Try operators: `?` and `!` (early return)
- Recursion

**Cannot do without runtime intrinsics:**
- Bit manipulation (count_ones, leading_zeros, bitnot, bitand, etc.)
- Byte manipulation (swap_bytes, etc.)
- String operations (length, concat, substring, etc.)
- List operations (length, push, pop, get, etc.)
- Map/Set operations
- Integer-to-string conversion
- Type introspection

---

## Potential Functions for Existing Modules

### bool.dfm

**Implementable without runtime:**
- `not(self: bool): bool` - logical negation using `if`
- `and(self: bool, other: bool): bool` - using `if`
- `or(self: bool, other: bool): bool` - using `if`
- `xor(self: bool, other: bool): bool` - using `if`
- `implies(self: bool, other: bool): bool` - logical implication
- `then_some(self: bool, value: u32): ?u32` - conditional Some (monomorphic)

### int.dfm (bigint)

**Implementable:**
- `abs(self: int): int` - using `if` and unary neg
- `signum(self: int): int` - returns -1, 0, or 1
- `is_positive(self: int): bool`
- `is_negative(self: int): bool`
- `is_zero(self: int): bool`
- `max(self: int, other: int): int`
- `min(self: int, other: int): int`
- `clamp(self: int, min: int, max: int): int`
- `div_euclid(self: int, other: int): !int` - Euclidean division
- `rem_euclid(self: int, other: int): !int` - Euclidean remainder

**Requires intrinsics:**
- String conversion, bit operations, pow, etc.

### u32.dfm (and other fixed integers)

**Currently stubbed but implementable with intrinsics (NOT YET):**
- Most bit manipulation functions need runtime support

**Implementable without runtime:**
- `is_zero(self: u32): bool`
- `is_power_of_two(self: u32): bool` - requires bitand, so not yet
- `max(self: u32, other: u32): u32`
- `min(self: u32, other: u32): u32`
- `clamp(self: u32, min: u32, max: u32): u32`
- `abs_diff(self: u32, other: u32): u32` - absolute difference

**Checked/saturating/wrapping** - many are stubbed; could implement if overflow detection works:
- These rely on knowing when overflow occurs, which needs runtime support

### option.dfm

**Note:** Currently monomorphic (`?u32`). With generics these would be generic.

**Implementable for ?u32:**
- `map(self: ?u32, f: fun(u32): u32): ?u32` - requires first-class functions (NOT AVAILABLE)
- `filter(self: ?u32, f: fun(u32): bool): ?u32` - requires closures (NOT AVAILABLE)
- `flatten(self: ??u32): ?u32` - nested option (would need `??u32` type)
- `zip(self: ?u32, other: ?u32): ?(u32, u32)` - pair of options

**Already implemented:**
- `is_some`, `is_none`, `unwrap_or`, `or_option`, `xor_option`, `and_option`, `unwrap_or_zero`

**Could add:**
- `expect(self: ?u32, msg: string): !u32` - convert option to result
- `ok_or(self: ?u32, err: string): !u32` - convert option to result

### list.dfm

**Cannot implement without intrinsics:**
- `len`, `is_empty`, `first`, `last`, `get`, `push`, `pop`, `concat`, `reverse`, `map`, `filter`, `fold`

All list operations require runtime support.

---

## Potential New Core Modules

### 1. **result.dfm** - Result type utilities

For `!u32` (monomorphic like option):
- `is_ok(self: !u32): bool`
- `is_err(self: !u32): bool`
- `unwrap_or(self: !u32, default: u32): u32`
- `unwrap_or_default(self: !u32): u32` - returns 0
- `ok(self: !u32): ?u32` - convert to option
- `err(self: !u32): ?string` - get error message as option (needs string handling)
- `or_result(self: !u32, other: !u32): !u32`
- `and_result(self: !u32, other: !u32): !u32`

### 2. **i32.dfm** (and i8, i16, i64)

Similar to u32.dfm but for signed integers:
- `min_value()`, `max_value()`, `bits()`
- `abs(self: i32): ?i32` - can overflow at MIN
- `abs_checked(self: i32): ?i32`
- `signum(self: i32): i32`
- `is_positive`, `is_negative`, `is_zero`
- `saturating_abs(self: i32): i32`
- `max`, `min`, `clamp`

### 3. **cmp.dfm** - Comparison utilities

Generic-like comparison helpers (monomorphic versions):
- `max_u32(a: u32, b: u32): u32`
- `min_u32(a: u32, b: u32): u32`
- `max_int(a: int, b: int): int`
- `min_int(a: int, b: int): int`
- `clamp_u32`, `clamp_int`

### 4. **tuple.dfm** - Tuple utilities

For specific tuple types (no generics):
- `first(self: (u32, u32)): u32` - requires tuple field access (NOT AVAILABLE?)
- `second(self: (u32, u32)): u32`
- `swap(self: (u32, u32)): (u32, u32)`

Need to check if tuple field access syntax exists.

### 5. **f32.dfm** - Float utilities

- `is_nan(self: f32): bool` - requires intrinsic
- `is_infinite(self: f32): bool` - requires intrinsic
- `is_finite(self: f32): bool`
- `abs(self: f32): f32` - requires intrinsic or bit manipulation
- `floor`, `ceil`, `round` - require intrinsics
- `max`, `min` - implementable with comparisons (but NaN handling needs intrinsics)

Most float operations need runtime support.

---

## Recommendations

### Phase 1: Pure Functions (No Runtime Needed)

1. **bool.dfm**: Implement `not`, `and`, `or`, `xor`, `implies`
2. **int.dfm**: Implement `abs`, `signum`, `is_positive`, `is_negative`, `is_zero`, `max`, `min`, `clamp`
3. **u32.dfm**: Keep existing stubs, add `is_zero`, `max`, `min`, `clamp`, `abs_diff`
4. **option.dfm**: Add `zip` (returns tuple)
5. **New result.dfm**: Implement `is_ok`, `is_err`, `unwrap_or`, `ok`, `or_result`, `and_result`
6. **New i32.dfm**: Mirror u32 structure for signed integer
7. **New cmp.dfm**: Simple max/min/clamp helpers

### Phase 2: After Adding Runtime Intrinsics

1. Bit operations for all integer types
2. String operations
3. List operations
4. Float math functions

---

## Test Coverage Needed

For each new function, add corresponding std_tests fixtures:
- Test normal cases
- Test edge cases (zero, max values, overflow conditions)
- Test with option/result return types

Current test structure is good (fixture-based), just needs expansion.
