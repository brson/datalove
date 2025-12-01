# Plan: Phase 4 - CFG-Based Control Flow

**STATUS: COMPLETE**

## Goal

Implement CFG-based execution for if-statements in the new interpreter, enabling proper control flow and preparing for Phase 5 (drop execution).

## What Was Done

- Extended `ControlFlowGraph` with `stmt_map: Vec<Statement<'db>>` field
- Added `condition_stmt: StmtId` to `Terminator::Branch`
- Updated `CfgBuilder` to store statements when allocating `StmtId`
- Added `CfgControl` enum and `execute_cfg_statement()` in interpreter
- Rewrote `execute_function_body_with_frame()` for CFG-based execution
- Added `extract_bool()` helper for condition evaluation
- Added `EarlyReturn` variant to `InterpError`

## Tests Added

- `51_if_simple.world` - Basic if/else returning from branches
- `52_if_no_else.world` - If without else (early return pattern)
- `53_if_nested.world` - Nested if statements
- `54_if_in_module.world` - If in module function (max/min)
- `55_if_else_branch.world` - Tests else branch execution

## Notes

- If-statement bodies are represented as separate CFG blocks, not executed directly
- This approach naturally supports Phase 5 (drop execution) since we can track which block we exit from
- `TryReturn` handling for `?` operator is stubbed - full implementation deferred
- Script-level if statements are intentionally not supported (returns `IfOutsideFunction` error)
