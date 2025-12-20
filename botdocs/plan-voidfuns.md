# Void Function Semantics Fix

## Design Intent (from design-notes.md)

**Void functions** (no return type):
- Don't require a return statement - function can end without `ret`
- Allow bare `ret` without value
- Must NOT have `ret` with a value

**Non-void functions** (with return type):
- Require `ret` with value matching the declared type

```
fun foo()           // void - OK, no ret needed
end fun

fun choose(a: u32)  // void - bare ret OK
  if a < 10
    ret
  end if
end fun

fun get(a: u32): bool  // non-void - ret with value required
  ret a < 10
end fun
```

## Current Implementation Issues

### 1. Parser/AST (BLOCKS bare ret)

**File:** `crates/datalove-datafun-compiler/src/ast.rs:71-73`
```rust
pub struct StmtRet<'db> {
    pub value: ExprFun<'db>,  // Required - not Optional!
}
```

**File:** `crates/datalove-datafun-compiler/src/parser.rs:399-408`
```rust
fn parse_ret(...) {
    self.eat_word(tokens, "ret");
    let value = self.parse_expr_full(tokens);  // Always parses expression!
    ast::Statement::Ret(ast::StmtRet::new(self.db, value))
}
```

**Problem:** Parser always requires an expression after `ret`. No syntax for bare `ret`.

### 2. Type Checker (NO void handling)

**File:** `crates/datalove-datafun-compiler/src/tycheck.rs:772-784`
```rust
Statement::Ret(stmt) => {
    let value = stmt.value(db);
    if let Some(expected_ret_ty) = ctx.expected_return_type {
        check_expr(ctx, value, expected_ret_ty)?;  // Always checks value!
    } else {
        ctx.add_error(...);  // Errors if no return type
    }
}
```

**File:** `tycheck.rs:2165-2188` - `types_equivalent()` doesn't handle `Type::Void`

**Problems:**
- Always expects ret to have a value
- No validation that void functions shouldn't return values
- `Type::Void` not handled in type equivalence

### 3. Interpreter (expects all paths to have ret)

**File:** `crates/datalove-datafun-compiler/src/interp/mod.rs:680-686`
```rust
Terminator::Return => {
    return Err(InterpError::RuntimeError(
        format!("Function '{}' reached Return terminator without ret statement", ...)
    ));
}
```

**Problem:** Errors if function ends without explicit `ret` - but void functions shouldn't need one.

**File:** `mod.rs:762-766`
```rust
ast::Statement::Ret(ret_stmt) => {
    let value = eval_return_expression_frame(ctx, ret_stmt.value(ctx.db))?;  // Always expects value
    Ok(CfgControl::Return(value))
}
```

**Problem:** Always evaluates return value, but void functions with bare `ret` won't have one.

## Fix Plan

### Phase 1: AST Change

**File:** `ast.rs:71-73`

Change:
```rust
pub struct StmtRet<'db> {
    pub value: Option<ExprFun<'db>>,  // Optional for void functions
}
```

### Phase 2: Parser Change

**File:** `parser.rs:399-408`

Update `parse_ret()` to:
1. After eating `ret`, peek at next token
2. If next is `end`, newline, or statement keyword → bare ret (value = None)
3. Otherwise → parse expression as before (value = Some(expr))

### Phase 3: Type Checker Fixes

**File:** `tycheck.rs:772-784`

Rewrite ret handling:
```rust
Statement::Ret(stmt) => {
    let ret_value = stmt.value(db);
    let expected_ty = ctx.expected_return_type;

    match (ret_value, expected_ty) {
        (Some(value), Some(ty)) if ty != Type::Void => {
            // Non-void function with value - check type
            check_expr(ctx, value, ty)?;
        }
        (Some(_), Some(ty)) if ty == Type::Void => {
            // Void function with value - ERROR
            ctx.add_error("void function cannot return a value");
        }
        (None, Some(ty)) if ty != Type::Void => {
            // Non-void function with bare ret - ERROR
            ctx.add_error("function requires return value");
        }
        (None, Some(ty)) if ty == Type::Void => {
            // Void function with bare ret - OK
        }
        (_, None) => {
            ctx.add_error("cannot infer return type");
        }
    }
}
```

**File:** `tycheck.rs:2165-2188`

Add to `types_equivalent()`:
```rust
(Type::Void, Type::Void) => true,
```

### Phase 4: Add ReturnVoid to CfgControl

**File:** `frame.rs:25-31`

Change CfgControl to distinguish void returns:
```rust
pub(super) enum CfgControl {
    /// Continue to the next statement in the block.
    Continue,
    /// Return from the function with a value.
    Return(Value),
    /// Return from void function (no value).
    ReturnVoid,
}
```

No runtime unit type needed - void is purely control flow.

### Phase 5: Interpreter Fixes

**File:** `mod.rs:680-686`

For void functions, `Terminator::Return` without ret is OK:
```rust
Terminator::Return => {
    let func = ctx.call_stack[frame_index].func;
    if func.return_type(ctx.db).is_none() {
        // Void function - implicit return OK
        return Ok(CfgControl::ReturnVoid);
    }
    return Err(InterpError::RuntimeError(...));
}
```

**File:** `mod.rs:762-766`

Handle optional value:
```rust
ast::Statement::Ret(ret_stmt) => {
    match ret_stmt.value(ctx.db) {
        Some(expr) => {
            let value = eval_return_expression_frame(ctx, expr)?;
            Ok(CfgControl::Return(value))
        }
        None => {
            // Bare ret - void function
            Ok(CfgControl::ReturnVoid)
        }
    }
}
```

**File:** `mod.rs` - Change function signatures

`execute_function_body` and `execute_function_body_with_frame` return `Result<Value, InterpError>`.
For void functions, change to `Result<Option<Value>, InterpError>`:
- `Some(value)` for non-void functions
- `None` for void functions

This propagates to callers:
- `eval_function_call_frame`
- Script-scope function calls in `script.rs`

**File:** `mod.rs:673-676`

Handle CfgControl::ReturnVoid in the CFG loop:
```rust
match execute_cfg_statement(ctx, stmt)? {
    CfgControl::Continue => continue,
    CfgControl::Return(value) => return Ok(Some(value)),
    CfgControl::ReturnVoid => return Ok(None),
}
```

**File:** `script.rs` caller handling

When calling void functions, don't try to use the return value:
```rust
match result {
    Ok(Some(value)) => { /* use value */ }
    Ok(None) => { /* void function - no value */ }
    Err(e) => { /* error */ }
}
```

### Phase 6: Tests

**Parser tests:**
- `ret` followed by `end fun` → bare ret
- `ret` followed by expression → ret with value
- `ret` followed by newline then `end fun` → bare ret

**Typechecker tests:**
- Void function with no ret → OK
- Void function with bare ret → OK
- Void function with `ret <value>` → ERROR
- Non-void function with no ret → ERROR (if path exits without ret)
- Non-void function with bare ret → ERROR
- Non-void function with `ret <value>` → OK

**Interpreter tests:**
- Void function with no ret → executes, returns None
- Void function with bare ret → executes, returns None
- Void function with early ret in branch → works correctly

## Files to Modify

1. `crates/datalove-datafun-compiler/src/ast.rs` - StmtRet optional value
2. `crates/datalove-datafun-compiler/src/parser.rs` - parse_ret optional value
3. `crates/datalove-datafun-compiler/src/tycheck.rs` - ret validation + types_equivalent
4. `crates/datalove-datafun-compiler/src/interp/frame.rs` - add CfgControl::ReturnVoid
5. `crates/datalove-datafun-compiler/src/interp/mod.rs` - change signatures to Option<Value>, handle ReturnVoid
6. `crates/datalove-datafun-compiler/src/interp/script.rs` - handle void returns
7. `crates/datalove-datafun-compiler/src/function_analysis.rs` - may need updates for void functions
8. Test fixtures for parser, typechecker, interpreter

## Execution Order

1. AST change (breaks compile)
2. Parser change (fixes parse)
3. Fix all `stmt.value(db)` call sites (grep for them)
4. Type checker validation
5. Add CfgControl::ReturnVoid to frame.rs
6. Change execute_function_body signature to Result<Option<Value>, _>
7. Update all callers (mod.rs, script.rs)
8. Add tests
9. Run full test suite
