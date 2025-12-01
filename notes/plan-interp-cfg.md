# Plan: Phase 4 - CFG-Based Control Flow

## Goal

Implement CFG-based execution for if-statements in the new interpreter, enabling proper control flow and preparing for Phase 5 (drop execution).

## Current State

- Tree-walking execution: `execute_function_body_with_frame` loops through `func.body(db)` directly
- CFG is built by `function_analysis/cfg.rs` but ignored by interpreter
- If-statements return error: "If statements in functions not yet implemented"

## Key Design Decision

**CFG stores StmtId but has no mapping to Statement**. The CFG builder allocates `StmtId` sequentially but doesn't store the actual `Statement` objects. We need to add this mapping.

## Implementation Plan

### Step 1: Extend CFG to Store Statement Map

**File:** `crates/datalove-datafun/src/function_analysis/cfg.rs`

Add statement storage to `ControlFlowGraph`:

```rust
#[salsa::tracked]
pub struct ControlFlowGraph<'db> {
    #[returns(ref)]
    pub blocks: Vec<BasicBlock>,
    #[returns(ref)]
    pub edges: Vec<ControlFlowEdge>,
    #[returns(ref)]
    pub stmt_map: Vec<Statement<'db>>,  // NEW: StmtId -> Statement
}
```

Modify `CfgBuilder` to collect statements:
- Add `stmts: Vec<Statement<'db>>` field
- In `alloc_stmt_id()`, also store the statement
- Pass statement to `alloc_stmt_id(stmt)` instead of allocating blindly

Add accessor method:
```rust
impl ControlFlowGraph<'db> {
    pub fn get_stmt(&self, db: &'db dyn Db, id: StmtId) -> Option<&Statement<'db>> {
        self.stmt_map(db).get(id.0 as usize)
    }
}
```

### Step 2: Add Condition Extraction for Branch Terminators

When a block has `Terminator::Branch`, the last statement in that block is the if-statement. Its condition determines which branch to take.

Add condition_stmt to Branch:
```rust
pub enum Terminator {
    Return,
    Branch {
        condition_stmt: StmtId,  // The if-statement containing the condition
        then_block: BlockId,
        else_block: BlockId
    },
    Goto(BlockId),
    TryReturn,
}
```

### Step 3: Rewrite Function Execution to Use CFG

**File:** `crates/datalove-datafun/src/interp/mod.rs`

- Start at block 0 (entry block)
- Execute all statements in current block
- Handle terminator to determine next block
- For Branch: evaluate if-statement's condition, pick then_block or else_block
- For Goto: jump to next block
- For Return: return value (should have been returned via CfgControl::Return)

### Step 4: Add CFG Statement Execution

```rust
enum CfgControl {
    Continue,
    Return(Value),
}
```

In CFG mode, if-statements don't execute their bodies - the CFG's then_block/else_block represent those as separate blocks.

### Step 5: Add Boolean Extraction Helper

Extract bool value from Value, destroy the value container after.

## Files to Modify

1. `crates/datalove-datafun/src/function_analysis/cfg.rs`:
   - Add `stmt_map: Vec<Statement<'db>>` to ControlFlowGraph
   - Add `stmts: Vec<Statement<'db>>` to CfgBuilder
   - Modify `alloc_stmt_id()` to take statement parameter
   - Add `condition_stmt: StmtId` to Terminator::Branch
   - Add `get_stmt()` method

2. `crates/datalove-datafun/src/interp/mod.rs`:
   - Add CfgControl enum
   - Rewrite `execute_function_body_with_frame()` for CFG-based execution
   - Add `execute_cfg_statement()` function
   - Add `extract_bool()` helper

## Testing

Add test cases in `tests/fixtures/interp2/`:
- `38_if_simple.world` - Basic if with else
- `39_if_no_else.world` - If without else
- `40_if_return.world` - Return inside if branches
- `41_if_nested.world` - Nested if statements

## Notes

- The if-statement's `then_body` and `else_body` are NOT executed directly in CFG mode - they're represented as separate blocks
- This approach naturally supports Phase 5 (drop execution) since we can track which block we exit from
- TryReturn handling for `?` operator is stubbed - full implementation deferred
