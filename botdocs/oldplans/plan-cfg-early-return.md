# Refactor: Full CFG Integration for Early-Return Operators

## Problem

`InterpError::OptionNone` and `InterpError::ResultErr` conflate errors with normal early-return control flow. The CFG has `TryReturn` infrastructure but it's not fully utilized.

## All Early-Return Operators

| Category | Operators | Early-return type |
|----------|-----------|-------------------|
| Try operators | `expr?`, `expr!` | Option/Result |
| Binary optional | `+?`, `-?`, `*?`, `/?` | Option (overflow/div0) |
| Binary checked | `+!`, `-!`, `*!`, `/!` | Result (overflow/div0) |
| Unary optional | `-?x` | Option (overflow) |
| Unary result | `-!x` | Result (overflow) |

## Current Architecture

1. **CFG builder** (`cfg.rs`):
   - `expr_may_return_early` only checks `TryOption`/`TryResult` nodes
   - Creates Branch terminator with TryReturn block for try operators
   - Does NOT detect optional/result arithmetic

2. **Interpreter** (`mod.rs`, `control.rs`, `arith.rs`):
   - Uses `Err(InterpError::OptionNone)` / `Err(InterpError::ResultErr)` for all early returns
   - Error propagates up call stack
   - `execute_function_body` catches and converts to None/Err value
   - `Terminator::TryReturn` just returns `Err(InterpError::EarlyReturn)` - unused

3. **Problem**: Expression evaluation is deep in call stack; can't "jump" to CFG block.

## Proposed Design

### New Control Flow Mechanism

Replace error-based propagation with explicit control flow return type:

```rust
/// Result of expression evaluation with possible early return.
enum EvalResult {
    /// Normal completion - value written to dest.
    Ok,
    /// Early return triggered - value already written to return_dest.
    EarlyReturn,
}
```

### Changes by File

#### `error.rs`
Remove control-flow-as-error variants:
- `OptionNone` - becomes EvalResult::EarlyReturn + write None
- `ResultErr` - becomes EvalResult::EarlyReturn + write Err
- `Overflow` - becomes EvalResult::EarlyReturn + write Err (for `+!` etc.)
- `DivisionByZero` - becomes EvalResult::EarlyReturn + write Err (for `/!`)
- `EarlyReturn` - replaced by EvalResult::EarlyReturn
- `FunctionReturn(Value)` - investigate if used; may be legacy (ret uses CfgControl::Return now)

Keep actual runtime errors:
- VariableNotFound, UseAfterMove, FunctionNotFound, etc.
- InvalidExpression, RuntimeError
- TypeErrors, AnalysisErrors

#### `cfg.rs`
Extend `expr_may_return_early` to detect all early-return operators:

```rust
fn expr_may_return_early(&self, db: &'db dyn Db, expr: ExprFun<'db>) -> bool {
    match expr.expr(db) {
        ExprFunKind::TryOption(_) | ExprFunKind::TryResult(_) => true,
        ExprFunKind::BinOp(binop) => {
            // Check operator type
            matches!(binop.op(db),
                BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional |
                BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked
            ) || self.expr_may_return_early(db, binop.lhs(db))
              || self.expr_may_return_early(db, binop.rhs(db))
        }
        ExprFunKind::UnaryOp(unary) => {
            matches!(unary.op(db), UnaryOp::NegOptional | UnaryOp::NegResult)
                || self.expr_may_return_early(db, unary.operand(db))
        }
        // ... rest unchanged
    }
}
```

#### `arith.rs` / `arith_widening.rs`
Change optional/result operators to:
1. On success: write result to dest, return `EvalResult::Ok`
2. On failure: write None/Err to `return_dest`, return `EvalResult::EarlyReturn`

Need access to `return_dest` from the current stack frame.

#### `control.rs`
Change `eval_try_option` / `eval_try_result`:
- On Some/Ok: write inner to dest, return `EvalResult::Ok`
- On None/Err: write None/Err to `return_dest`, return `EvalResult::EarlyReturn`

#### `mod.rs`
1. Change `eval_expression_frame` return type to `Result<EvalResult, InterpError>`
2. Propagate `EvalResult::EarlyReturn` up to statement level
3. At statement execution, check result:
   - `EvalResult::Ok` -> continue normally
   - `EvalResult::EarlyReturn` -> CFG takes else_block (TryReturn path)
4. At `Terminator::TryReturn`: just return (value already in return_dest)
5. Remove the catch-and-convert logic in `execute_function_body`

### Execution Flow Example

For `let x = a +? b` where `a +? b` overflows:

1. `eval_add_optional` detects overflow
2. Writes `Option::None` to `ctx.call_stack[frame].return_dest`
3. Returns `EvalResult::EarlyReturn`
4. `eval_expression_frame` returns `Ok(EvalResult::EarlyReturn)`
5. `execute_let_statement_frame` sees `EarlyReturn`, returns it
6. `execute_cfg_statement` sees `EarlyReturn`, returns `CfgControl::EarlyReturn`
7. CFG executor at Branch terminator takes else_block (TryReturn)
8. At `Terminator::TryReturn`, function exits with value already in return_dest

## Implementation Order

1. Add `EvalResult` enum to `mod.rs` or new module
2. Update `expr_may_return_early` in `cfg.rs`
3. Update `eval_try_option`/`eval_try_result` in `control.rs`
4. Update optional/result operators in `arith.rs` and `arith_widening.rs`
5. Thread `EvalResult` through `eval_expression_frame` and callers
6. Update CFG executor to check EvalResult and branch accordingly
7. Remove error-based early return handling from `execute_function_body`
8. Remove `OptionNone`/`ResultErr` from `InterpError`
9. Run tests

## Files to Modify

- `crates/datalove-datafun-compiler/src/interp/mod.rs` - main changes
- `crates/datalove-datafun-compiler/src/interp/error.rs` - remove variants
- `crates/datalove-datafun-compiler/src/interp/control.rs` - try operators
- `crates/datalove-datafun-compiler/src/interp/arith.rs` - optional arithmetic
- `crates/datalove-datafun-compiler/src/interp/arith_widening.rs` - all arithmetic dispatch
- `crates/datalove-datafun-compiler/src/function_analysis/cfg.rs` - extend detection

## Test Coverage

Existing tests that must continue to pass:
- `242_binop_add_optional_success.world` - success case
- `243_binop_add_optional_overflow.world` - overflow -> early return @none
- Similar tests for sub, mul, div optional (244-249)
- `121_try_option_none.world` - try option early return
- `123_try_result_err.world` - try result early return
- `236_try_option_chained_early_return.world`
- `238_try_result_chained_early_return.world`
