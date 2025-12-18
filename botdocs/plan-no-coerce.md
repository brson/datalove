# Plan: Remove Option/Result Coercion and Add Explicit Constructors

## Summary

Remove automatic T → Option<T> and T → Result<T> coercion, and implement explicit `some`, `ok`, `er` constructor keywords.

## Current State

**Coercion locations:**
- Type checking: `check_type_coercion()` in `datafun-compiler/src/tycheck.rs:2535-2594`
- Runtime: `coerce_value_to_dest()` in `datafun-compiler/src/interp/coerce.rs:44-114`
- Datalit tycheck: `Check-Option` (line 653) and `Check-Result` (line 666) rules in `datalit/src/tycheck.rs`

**Keyword status:**
- `none`: Already implemented (parser, tycheck, interpreter)
- `some`, `ok`, `er`: Not implemented - AST variants missing

**Tests affected:** 16 coercion test fixtures need updating

---

## Phase 1: Add AST Support for `some`, `ok`, `er`

### 1.1 Datalit AST (`crates/datalove-datalit/src/ast.rs`)

Add new expression variants:
```rust
// In Expr enum (around line 161):
Some(ExprSome<'db>),  // some <expr>
Ok(ExprOk<'db>),      // ok <expr>
Er(ExprEr<'db>),      // er <error-expr>

// Add structs:
#[salsa::tracked]
pub struct ExprSome<'db> {
    pub payload: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprOk<'db> {
    pub payload: ExprFull<'db>,
}

#[salsa::tracked]
pub struct ExprEr<'db> {
    pub payload: ExprFull<'db>,  // Must be error type
}
```

### 1.2 Datafun AST (`crates/datalove-datafun-compiler/src/ast.rs`)

Add corresponding variants in `ExprFunKind` (around line 166):
```rust
Some(ExprSome<'db>),
Ok(ExprOk<'db>),
Er(ExprEr<'db>),

// Add structs (similar to ExprErr pattern at line 378)
```

---

## Phase 2: Add Parser Support

### 2.1 Datalit Parser (`crates/datalove-datalit/src/parser.rs`)

Add keyword parsing (pattern from `none` at line 815):
```rust
// In parse_primary_expr or similar:
Some("some") => {
    // Consume 'some', parse payload expression
    self.next();
    let payload = self.parse_expr_full()?;
    Expr::Some(ExprSome::new(db, payload))
}
Some("ok") => {
    self.next();
    let payload = self.parse_expr_full()?;
    Expr::Ok(ExprOk::new(db, payload))
}
Some("er") => {
    self.next();
    let payload = self.parse_expr_full()?;  // Should be error expr
    Expr::Er(ExprEr::new(db, payload))
}
```

### 2.2 Datafun Parser (`crates/datalove-datafun-compiler/src/parser.rs`)

Similar additions for datafun expressions.

---

## Phase 3: Add Type Checking

### 3.1 Datalit Tycheck (`crates/datalove-datalit/src/tycheck.rs`)

**Synthesize rules:**
- `some <expr>`: Synthesize inner type T, return `?T`
- `ok <expr>`: Synthesize inner type T, return `!T`
- `er <expr>`: Check payload is error type, return `!T` (T from context)

**Check rules:**
- `some <expr>` against `?T`: Check payload against T
- `ok <expr>` against `!T`: Check payload against T
- `er <expr>` against `!T`: Check payload is valid error

### 3.2 Datafun Tycheck (`crates/datalove-datafun-compiler/src/tycheck.rs`)

Add synthesis/checking for new expression kinds.

---

## Phase 4: Remove Coercion

### 4.1 Remove Type-Level Coercion

**File: `crates/datalove-datafun-compiler/src/tycheck.rs`**

- Lines 1807-1819: Remove `allow coercion from T to Option<T>` and `allow coercion from T to Result<T>`
- Lines 2553-2571: Remove Option/Result coercion from `check_type_coercion()`
- Keep: Data coercion (T → data), numeric widening

**File: `crates/datalove-datalit/src/tycheck.rs`**

- Lines 653-658: Remove `Check-Option` rule (any expr checks against Option<T>)
- Lines 666-671: Remove `Check-Result` rule (any expr checks against Result<T>)
- Keep: `Check-None` rule (none checks against Option), `Check-ResultErr` rule (error checks against Result)

### 4.2 Remove Runtime Coercion

**File: `crates/datalove-datafun-compiler/src/interp/coerce.rs`**

- Lines 44-78: Remove T → Option<T> wrapping
- Lines 80-114: Remove T → Result<T> wrapping
- Keep: Data coercion, type matching

---

## Phase 5: Add Interpreter Support

### 5.1 Literal Writing (`crates/datalove-datafun-compiler/src/interp/literals.rs`)

Add functions (pattern from `write_option_none_to_dest` at line 101):
```rust
fn write_option_some_to_dest(...) -> Result<Value, InterpError>
fn write_result_ok_to_dest(...) -> Result<Value, InterpError>
fn write_result_er_to_dest(...) -> Result<Value, InterpError>
```

### 5.2 Expression Evaluation (`crates/datalove-datafun-compiler/src/interp/mod.rs`)

Add match arms for `ExprFunKind::Some`, `ExprFunKind::Ok`, `ExprFunKind::Er`.

---

## Phase 6: Update Tests

### 6.1 Delete or Update Coercion Tests

**Delete (no longer valid):**
- `100_let_coercion_option.world` → Delete
- `101_let_coercion_result.world` → Delete
- `102_fun_let_coercion_option.world` → Delete
- `104_return_coercion_option.world` → Delete
- `105_return_coercion_result.world` → Delete
- `217_name_coercion_option.world` → Delete
- `218_name_coercion_result.world` → Delete
- All matching tycheck fixtures (51, 52, 106-112)

### 6.2 Add New Tests

Add tests for explicit constructors:
- `xxx_explicit_option_some.world` - Test `some <expr>` construction
- `xxx_explicit_option_none.world` - Test `none` (already works)
- `xxx_explicit_result_ok.world` - Test `ok <expr>` construction
- `xxx_explicit_result_er.world` - Test `er error "msg"` construction

---

## Phase 7: Update Documentation

### 7.1 Botspec (`botdocs/botspec.md`)

Section 3.3 Coercions - Update:
```markdown
### 3.3 Coercions

- Any type coerces to `data` (T → data)
- Data values coerce to `?data` and `!data`
- Empty collections check against any element type
- Numeric widening (u8 → u16 → u32 → u64 → int, etc.)

**Removed:** Automatic T → Option<T> and T → Result<T> coercion.
Use explicit `some`, `ok`, `er` constructors.
```

Add to Section 2.2 Expressions:
```markdown
| some expr | `some 4` | Implemented |
| ok expr | `ok 4` | Implemented |
| er expr | `er error "msg"` | Implemented |
```

---

## Execution Order

1. AST changes (both datalit and datafun)
2. Parser changes (both layers)
3. Type checker additions (some/ok/er support)
4. Interpreter additions
5. Remove coercion from type checker
6. Remove coercion from interpreter
7. Update tests
8. Update botspec
9. Run full test suite

---

## Critical Files

**AST:**
- `crates/datalove-datalit/src/ast.rs`
- `crates/datalove-datafun-compiler/src/ast.rs`

**Parser:**
- `crates/datalove-datalit/src/parser.rs`
- `crates/datalove-datafun-compiler/src/parser.rs`

**Type Checker:**
- `crates/datalove-datalit/src/tycheck.rs`
- `crates/datalove-datafun-compiler/src/tycheck.rs`

**Interpreter:**
- `crates/datalove-datafun-compiler/src/interp/mod.rs`
- `crates/datalove-datafun-compiler/src/interp/coerce.rs`
- `crates/datalove-datafun-compiler/src/interp/literals.rs`

**Tests:**
- `crates/datalove-datafun/tests/fixtures/interp/*coercion*`
- `crates/datalove-datafun-compiler/tests/fixtures/tycheck/*coercion*`

**Docs:**
- `botdocs/botspec.md`
