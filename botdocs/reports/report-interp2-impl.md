# Interpreter Ownership and Lifetime Management

This document describes how the interpreter coordinates ownership tracking,
moves, references, and destruction across script scope, function frames,
and the function analysis system.

## Core Concepts

### Value Representation

Every runtime value is represented by `Value`:

```rust
pub struct Value {
    pub ptr: *mut u8,           // Pointer to the data
    pub tydesc: *const TyDesc,  // Type descriptor for runtime type info
    pub location: ValueLocation, // Ownership tracking
}

pub enum ValueLocation {
    Borrowed,   // Points to memory owned elsewhere; don't free structure
    TempOwned,  // Freshly allocated; free structure after use
}
```

The `location` field is critical for preventing leaks and double-frees:
- `TempOwned`: This value owns its memory. When destroyed, both contents
  AND the outer structure are freed.
- `Borrowed`: This value references memory owned by someone else. When
  destroyed, only contents are cleaned up; the structure is not freed.

### Destruction Functions

Three key functions handle cleanup:

1. **`destroy_value(ctx, value)`**: Destroys contents AND frees structure if TempOwned.
2. **`destroy_value_contents_only(ctx, value)`**: Only destroys internal allocations.
3. **`free_value_structure(ctx, value)`**: Only frees outer structure if TempOwned.

## Script Scope Execution

### Variable Storage

Script-level variables are stored in `ScriptScope`:

```rust
pub struct ScriptVariable {
    pub value: Value,              // The stored value (typically TempOwned)
    pub state: ScriptVarState,     // Available or Moved
    pub is_copy: bool,             // Whether type is copyable
}

pub enum ScriptVarState {
    Available,  // Can be read
    Moved,      // Has been consumed; cannot be read again
}
```

### Reading Script Variables

When a script variable is read via `read_script_variable`:

**Copy types:**
- Clone the value, return the clone as TempOwned
- Original stays Available

**Linear (non-copy) types:**
- Mark variable as `Moved`
- Return value as `TempOwned` (ownership transfers to caller)
- Caller is now responsible for cleanup

### Script Cleanup

When `cleanup_script_scope` runs at script end:

- **Available variables**: Call `destroy_value` (contents + structure)
- **Moved variables**: Skip entirely (ownership was transferred to consumer)

## Function Execution

### Frame Layout

Each function call creates a stack frame with slots computed by `function_analysis`:

```rust
pub enum SlotKind {
    Reference,   // Pointer to caller-owned memory (parameters)
    Local,       // Stack-allocated local variable
    Temporary,   // Expression evaluation temporary
}
```

### Parameter Passing

When a function is called:

1. Arguments are evaluated, producing `Vec<Value>` (typically TempOwned)
2. For each parameter:
   - Create a Reference slot in frame
   - Store `arg_value.ptr` (just the pointer, not ownership info)
3. Function body executes using the frame

### Reading from Reference Slots

When the function body reads a parameter (Reference slot):

**Copy types:**
- Clone to destination, return clone
- Slot stays Available

**Linear types:**
- Mark slot as `Moved`
- Return `Value { ptr, tydesc, location: Borrowed }`
- The value is Borrowed because the structure is owned by the caller

This is critical: Reference slots return **Borrowed** values because:
1. The caller owns the structure memory (it's in `arg_values`)
2. The function body shouldn't free the structure
3. Caller's cleanup handles structure freeing

### Slot State Tracking

Each slot tracks whether it's been moved:

```rust
pub enum SlotState {
    Available,  // Slot contains valid data
    Moved,      // Data was consumed/moved out
}
```

When a linear value is read from a slot, the slot is marked `Moved`.

### Frame Cleanup

When `cleanup_frame` runs after function body:

- Iterates all slots
- Skips `Moved` slots (already consumed)
- Skips `Reference` slots (caller owns that memory)
- Destroys `Available` Local/Temporary slots

### Argument Cleanup After Function

`cleanup_args_after_frame` handles argument memory after function returns:

**Copy types:**
- `free_value_structure(arg_value)` - free the original since function cloned it

**Linear types:**
- If slot is `Available`: `destroy_value(arg_value)` - never used, destroy fully
- If slot is `Moved` AND `TempOwned`: `free_value_structure(arg_value)` - contents
  were consumed by function, but structure needs freeing
- If slot is `Moved` AND `Borrowed`: nothing - someone else owns the structure

## Pattern Matching (Option/Result)

`evaluate_branch_condition` handles if-let pattern matching:

### Option Handling
```rust
if value.location == ValueLocation::TempOwned {
    // Free the Option structure
    dtlv_rti_mem_free_local(...);
}
```

### Result Handling
Same pattern - only free if TempOwned.

This consistency is important: when a Reference slot value (Borrowed) is
pattern-matched, the structure isn't freed here. The caller's
`cleanup_args_after_frame` handles it.

## Ownership Flow Example

Consider:
```datalove
let ok1: !u32 = @42
let result = is_ok(ok1)
```

1. `@42` creates u32 (TempOwned), coerced to Result, stored in `ok1`
2. `is_ok(ok1)` call:
   - `read_script_variable(ok1)` returns TempOwned, marks ok1 as Moved
   - TempOwned value added to arg_values
   - Function creates Reference slot pointing to arg_value.ptr
3. Inside is_ok, `if self |value|`:
   - Read from Reference slot returns Borrowed
   - `evaluate_branch_condition` receives Borrowed value
   - Pattern match extracts payload
   - Since Borrowed, structure NOT freed here
4. Function returns bool
5. `cleanup_args_after_frame`:
   - Slot was Moved (pattern match consumed it)
   - arg_value is TempOwned
   - Call `free_value_structure` - frees the Result structure
6. Script cleanup:
   - ok1 is Moved, skip it

No leaks, no double-frees.

## function_analysis Integration

The `function_analysis` module computes:

### Slot Layout
- Which variables need slots
- Slot kinds (Reference, Local, Temporary)
- Offsets in frame buffer

### Liveness Analysis
- When variables are live (initialized and not yet dropped)
- Used for determining when slots need cleanup

### Move Analysis
- Tracks which expressions are "last uses" of values
- Helps determine when ownership transfers

### Drop Points
- Computed drop points for each variable
- Used by interpreter to know when to cleanup

Currently, the interpreter uses slot states (`Available`/`Moved`) rather than
the computed drop points directly. The analysis results are used mainly for:
- Frame layout computation
- Validation (use-after-move, double-move errors)
- Copyability determination

## Design Flaws and Observations

### 1. Reference Slot Ownership Ambiguity

Reference slots store only the pointer, not the ownership info. When reading
from a Reference slot, we must assume Borrowed because we don't know if the
caller passed TempOwned or Borrowed.

**Impact**: Works correctly but loses information. A more precise design
would track ownership in the slot itself.

### 2. Implicit Ownership Transfer Rules

The ownership transfer rules are implicit and scattered:
- `read_script_variable` returns TempOwned for linear types
- Reference slot reads return Borrowed
- `cleanup_args_after_frame` must know to free TempOwned moved args

**Impact**: Easy to introduce bugs. Adding new code paths requires careful
understanding of all the implicit contracts.

### 3. evaluate_branch_condition Side Effects

Pattern matching in `evaluate_branch_condition` has side effects:
- Extracts payload and writes to frame slot
- Frees structure if TempOwned

This mixes evaluation and cleanup in one function.

### 4. Drop Points Not Fully Used

`function_analysis` computes drop points, but the interpreter uses its own
slot state tracking. This duplication could lead to inconsistencies.

### 5. No Explicit Ownership Types

Rust distinguishes `T`, `&T`, `&mut T`. The interpreter has only:
- TempOwned (roughly like `T`)
- Borrowed (roughly like reference, but not clearly distinguished)

There's no distinction between shared borrows and exclusive borrows.

### 6. Copy Type Detection is Heuristic

`is_copy_type` checks type tags heuristically. If a new type is added and
not properly marked as copy/linear, ownership bugs can occur.

### 7. Cleanup Order Dependencies

Cleanup must happen in correct order:
1. Function body cleanup (cleanup_frame)
2. Argument cleanup (cleanup_args_after_frame)
3. Script cleanup (cleanup_script_scope)

The runtime's shutdown must happen after all cleanups. This ordering is
implicit and could break if code is reorganized.

## Summary

The interpreter uses a two-level ownership model:
- **TempOwned**: Caller is responsible for freeing structure
- **Borrowed**: Someone else owns the structure

Key invariants:
1. Linear script variables transfer ownership when read (become Moved)
2. Reference slots return Borrowed values (caller owns structure)
3. cleanup_args_after_frame frees TempOwned structures for Moved args
4. evaluate_branch_condition only frees TempOwned structures
5. cleanup_script_scope skips Moved variables

The system works but is fragile. Changes to any component must consider
the full ownership flow to avoid leaks or double-frees.
