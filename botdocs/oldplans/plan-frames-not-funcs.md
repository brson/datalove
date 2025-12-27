# Plan: Generalize Interpreter for Script Execution

## Status: COMPLETE

## Goal

Divorce the interpreter from function-specific assumptions so it can later support script/REPL execution. This is a refactoring task only - no script execution implementation yet.

## Summary

1. Make unit type `()` the implicit return type of void functions - DONE
2. Replace `func: StmtFun` in StackFrame with abstracted context info - DONE
3. Update interpreter to use context info instead of `func.xxx()` accessors - DONE
4. Remove `Type::Void` from type system - DONE
5. Remove `CfgControl::ReturnVoid` - bare `ret` returns unit - DONE
6. Enable calling void functions in expression context - DONE

---

## Completed Changes

### Type System (`tycheck.rs`)

- Removed `Type::Void` variant entirely
- Added `is_void_function: bool` to `TypeContext` for AST-level tracking
- Added `unit_type(db)` tracked function to create unit type
- Added `is_unit_type(db, ty)` helper to check for unit type
- Void functions now have return type `()` after typechecking

### Frame Context (`interp/frame.rs`)

- Added `FrameContext` struct with `context_name` and `return_type`
- Replaced `func: StmtFun` field with `context: FrameContext` in StackFrame
- Removed `CfgControl::ReturnVoid` - only `Continue` and `Return(Value)` remain

### Interpreter (`interp/mod.rs`)

- Updated frame creation to build `FrameContext` from function analysis
- Bare `ret` now calls `write_unit_to_return_dest()` and returns `Return(value)`
- Implicit return at function end writes unit for unit-returning functions
- Removed error check that prevented void functions in expression context
- Added `write_unit_to_return_dest()` helper (writes nothing - unit is ZST)
- Updated doc comments to reflect "all functions return a value"

### Function Analysis (`function_analysis/mod.rs`)

- Added `return_type: TypeAndHeap` to `FunctionAnalysis`
- Computes return type in tracked context (unit for void functions)

### Other Files

- `copyability.rs`: Removed `Type::Void` match arm
- `type_sizing.rs`: Removed `DatafunType::Void` match arm
- `funlit_equiv.rs`: Removed `Type::Void` match arm
- `tycheck_world_tests.rs`: Removed `Type::Void` references

### Test

- Added `223_call_void_function.world` demonstrating void function call in expression context
- Output: `@()` (unit value)

---

## Design Decisions

1. **Unit is ZST**: Kept unit type as size 0, align 1. No special handling needed.

2. **Void vs Unit distinction**: After typechecking, there's no distinction. `is_void_function` bool in `TypeContext` tracks AST-level void functions for error messages only.

3. **All functions return a value**: Simplifies interpreter - no `Option<Value>` return semantics needed internally.

---

## Testing

All 219 module_interp tests pass, plus full workspace test suite.
