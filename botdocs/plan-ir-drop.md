# Plan: Precise Drop Points in IR Lowering

## Problem

The IR interpreter uses `destroy_all()` at script end, which hides drop bugs. We need precise drops emitted during IR lowering.

## Scope Rules

- **Functions**: emit drops at returns, early returns, scope exits
- **Script units internal scopes** (loops, if blocks): emit drops at scope exit
- **Script unit top-level bindings**: exported, NOT dropped (cleaned at script finalize)
- **Expr unit results**: kept for REPL, cleaned at script finalize

## Design: ScopeTracker in LowerCtx

Add scope tracking during IR lowering to emit Drop instructions at scope exits.

### Data Structures

```rust
struct ScopeTracker {
    scopes: Vec<Scope>,
}

struct Scope {
    kind: ScopeKind,
    /// (name, ValueId, needs_drop)
    values: Vec<(String, ValueId, bool)>,
    /// (name, SlotId, needs_drop)
    slots: Vec<(String, SlotId, bool)>,
}

enum ScopeKind {
    Function,
    ScriptUnit,
    Loop { header: BlockId, exit: BlockId },
    IfThen { merge: BlockId },
    IfElse { merge: BlockId },
}
```

### Algorithm

1. **Enter scope**: push new Scope to stack
2. **Record bindings**: when creating let/var, add to current scope with `needs_drop = !is_copy_type(ty)`
3. **Mark moves**: when value is consumed (return, call arg, assignment), mark as moved
4. **Emit drops at scope exit**: for each non-moved, non-copy value/slot in scope, emit `Drop`
5. **Exit scope**: pop from stack

### Drop Points

| Context | When to Drop |
|---------|--------------|
| Loop body | Before Goto(loop_header), before break/continue |
| If branch | Before Goto(merge_block) |
| Function return | Before Return/TryReturn terminator |
| Set statement | Drop old slot value before SlotStore |
| Script unit | Only internal scopes, not top-level exports |

### Branch Convergence

If a value is moved in one branch but not the other:
- Each branch emits drops for its own scope before Goto(merge)
- Values defined BEFORE the if and moved in one branch need drop in the other
- Track pre-branch state, compare moves after each branch

### If-Bindings (Runtime Tracking Case)

If-bindings create slots whose initialization depends on runtime condition:
```datafun
if option |value|     // value only exists if option is Some
    use(value)
end if
// Should we drop value? Depends on whether option was Some!
```

**Current state**: IR lowering doesn't implement if-bindings yet (only bool conditions).

**When implemented**: If-bindings need runtime tracking:
1. Create slot for binding
2. Branch terminator sets initialization flag if condition is true
3. At merge point, emit conditional drop: `if initialized { drop slot }`

**For now**: Focus on static drops; defer if-binding support to later

## Implementation Steps

### Step 1: Add ScopeTracker to LowerCtx

File: `crates/datalove-datafun-compiler/src/ir/lower.rs`

- Add `scope_tracker: ScopeTracker` field
- Add `is_copy_type(ty: &IrType) -> bool` helper (adapt from function_analysis/copyability.rs)

### Step 2: Scope Entry/Exit for Functions

- `lower_function`: enter Function scope, exit before final block
- Emit drops before Return/TryReturn terminators

### Step 3: Scope Entry/Exit for Loops

- `lower_loop`: enter Loop scope after creating blocks
- Emit drops before Goto(loop_header) at loop end
- Handle break: emit drops for all scopes up to loop, then Goto(exit)
- Handle continue: emit drops for loop scope only, then Goto(header)

### Step 4: Scope Entry/Exit for If Blocks

- `lower_if`: enter IfThen/IfElse scopes for each branch
- Emit drops before Goto(merge) in each branch

### Step 5: Record Bindings

- `lower_let`: record value in current scope
- `lower_var`: record slot in current scope

### Step 6: Mark Moves

- Return expression: mark as moved
- Function call arguments: mark non-copy args as moved
- Let/var initialization from existing value: mark source as moved

### Step 7: Set Statement Drops

- Before SlotStore, emit Drop for old slot value (if slot needs_drop)

### Step 8: Script Unit Handling

- ScriptUnit scope for top-level
- Don't emit drops for ScriptUnit scope at unit end (exports)
- Do emit drops for nested Loop/If scopes

### Step 9: Remove destroy_all

- Remove `Frame::destroy_all` usage in interpreter
- Keep `ScriptEnvironment.finalize()` for script-end cleanup only

## Files to Modify

1. `crates/datalove-datafun-compiler/src/ir/lower.rs`
   - Add ScopeTracker, Scope, ScopeKind structs
   - Add is_copy_type helper
   - Modify lower_function, lower_loop, lower_if, lower_statement

2. `crates/datalove-datafun-compiler/src/ir/interp.rs`
   - Ensure Drop handles Operand::Slot (load and destroy)
   - Remove Frame::destroy_all calls
   - Keep ScriptEnvironment cleanup for finalize only

3. `crates/datalove-datafun/src/worldfile_analysis_ir3.rs`
   - Rename destroy_all to finalize() or similar

## Test Cases

1. Function with let binding - verify drop before return
2. Function returning value - verify NO drop for return value
3. Loop with internal let - verify drop before loop-back
4. Break from loop - verify drops emitted before break
5. If-else with branch-local bindings - verify drops in each branch
6. Set statement - verify old value dropped before store
7. Script unit exports - verify NOT dropped at unit end
8. Nested scopes - verify correct drop order (LIFO)
