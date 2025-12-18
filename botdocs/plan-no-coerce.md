# Plan: Remove Option/Result Coercion and Add Explicit Constructors

## Summary

Remove automatic T → Option<T> and T → Result<T> coercion, and implement explicit `some`, `ok`, `er` constructor keywords.

## Progress

- [x] Phase 1: Add explicit constructors (`some`, `ok`, `er`)
- [ ] Phase 2: Remove coercion

---

## Phase 1: Explicit Constructors (COMPLETED)

### Completed Work

**AST Changes:**
- `crates/datalove-datalit/src/ast.rs`: Added `ExprSome`, `ExprOk`, `ExprEr` structs and enum variants
- `crates/datalove-datafun-compiler/src/ast.rs`: Same additions

**Parser Changes:**
- `crates/datalove-datalit/src/parser.rs`: Added `some`, `ok`, `er` keyword parsing
- `crates/datalove-datafun-compiler/src/parser.rs`: Added keywords with disambiguation
  - `ok(x)` parses as function call (allows stdlib functions named `ok`)
  - `ok x` parses as keyword constructor

**Type Checking:**
- `crates/datalove-datalit/src/tycheck.rs`:
  - Syn-Some: `some <expr>` synthesizes `?T`
  - Syn-Ok: `ok <expr>` synthesizes `!T`
  - Check-Some: `some <expr>` against `?T`
  - Check-Ok: `ok <expr>` against `!T`
  - Check-Er: `er <expr>` against `!T`
- `crates/datalove-datafun-compiler/src/tycheck.rs`: Same rules

**Interpreter:**
- `crates/datalove-datafun-compiler/src/interp/mod.rs`: Added evaluation for `Some`, `Ok`, `Er`
- `crates/datalove-datafun-compiler/src/interp/literals.rs`: Added helper functions:
  - `write_option_some_from_value`
  - `write_result_ok_from_value`
  - `write_result_er_from_value`

**Supporting Files Updated:**
- `ast_serde.rs` (both crates)
- `canon.rs`, `pretty.rs` (datalit)
- `function_analysis/*.rs` (liveness, moves, slot_allocation, validation)
- `funlit_equiv.rs`

### Test Results

- 174 interp tests pass
- 20 stdlib tests pass
- 151 datalit tests pass
- 1 error_equiv test fails (edge case for truncated input - expected)

---

## Phase 2: Remove Coercion (PENDING)

### 2.1 Remove Type-Level Coercion

**File: `crates/datalove-datafun-compiler/src/tycheck.rs`**
- Remove Option/Result coercion from `check_type_coercion()`
- Keep: Data coercion (T → data), numeric widening

**File: `crates/datalove-datalit/src/tycheck.rs`**
- Remove `Check-Option` rule (any expr checks against Option<T>)
- Remove `Check-Result` rule (any expr checks against Result<T>)
- Keep: `Check-None`, `Check-Some`, `Check-Ok`, `Check-Er`, `Check-ResultErr`

### 2.2 Remove Runtime Coercion

**File: `crates/datalove-datafun-compiler/src/interp/coerce.rs`**
- Lines 44-78: Remove T → Option<T> wrapping
- Lines 80-114: Remove T → Result<T> wrapping
- Keep: Data coercion, type matching

### 2.3 Update Tests

**Delete coercion tests:**
- `100_let_coercion_option.world`
- `101_let_coercion_result.world`
- `102_fun_let_coercion_option.world`
- `104_return_coercion_option.world`
- `105_return_coercion_result.world`
- `217_name_coercion_option.world`
- `218_name_coercion_result.world`

**Add explicit constructor tests:**
- Test `some <expr>` construction
- Test `ok <expr>` construction
- Test `er error "msg"` construction

### 2.4 Update Documentation

Update `botdocs/botspec.md` Section 3.3 Coercions.

---

## Syntax Reference

```datalove
// Option construction
let a: ?u32 = some 3
let b: ?u32 = none

// Result construction
let c: !u32 = ok 3
let d: !u32 = er error "oops"

// Function calls still work (disambiguation)
let e = result.ok(some_result)  // calls stdlib function
```

---

## Critical Files

**AST:** `ast.rs` in datalit and datafun-compiler
**Parser:** `parser.rs` in datalit and datafun-compiler
**Type Checker:** `tycheck.rs` in datalit and datafun-compiler
**Interpreter:** `interp/mod.rs`, `interp/coerce.rs`, `interp/literals.rs`
**Tests:** `fixtures/interp/*coercion*`, `fixtures/tycheck/*coercion*`
**Docs:** `botdocs/botspec.md`
