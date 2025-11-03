# Function Analysis Plan

## Overview

Implement per-function analysis for a linear type system with explicit move semantics.
This replaces the current HashMap-based interpreter frame with a packed stack frame layout.

## Goals

1. Pack all locals and temporaries into a single allocation with computed offsets
2. Track liveness ranges for each slot
3. Track move vs. borrow operations
4. Identify drop points for linear resource management
5. Provide all information needed for an efficient interpreter

## Architecture

Analysis is performed in the Salsa compile-time world using `tycheck::TypeAndHeap<'db>`.
The interpreter consumes this analysis and uses `TypeTable` to map types to runtime `*const rtdt::TyDesc`.

## Data Structures

### Core Analysis Result

```rust
#[salsa::tracked]
pub struct FunctionAnalysis<'db> {
    pub function: StmtFun<'db>,
    pub frame_layout: FrameLayout<'db>,
    pub live_ranges: LiveRanges<'db>,
    pub move_info: MoveInfo<'db>,
    pub drop_points: DropPoints<'db>,
    pub control_flow: ControlFlowGraph<'db>,
}
```

### Frame Layout

```rust
#[salsa::tracked]
pub struct FrameLayout<'db> {
    pub total_size: u32,
    pub total_align: u32,
    #[returns(ref)]
    pub slots: Vec<SlotInfo<'db>>,
}

#[salsa::tracked]
pub struct SlotInfo<'db> {
    pub slot_id: SlotId,
    pub name: Option<InternedText<'db>>,  // None for temporaries
    pub kind: SlotKind,
    pub offset: u32,
    pub ty: crate::tycheck::TypeAndHeap<'db>,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum SlotKind {
    Reference,   // All parameters (In/Out/Ref/Mut) - pointer to caller's data
    Local,       // Let bindings - actual storage in callee's frame
    Temporary,   // Expression temporaries - actual storage in callee's frame
}

// Note: Reference slots are always pointer-sized (8 bytes on 64-bit platforms),
// but the `ty` field in SlotInfo still holds the *referenced* type, not pointer-to-type.
// This allows move analysis to understand what type is being accessed through the reference.

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct SlotId(u32);

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct StmtId(u32);

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct ExprId(u32);
```

### Liveness Tracking

```rust
#[salsa::tracked]
pub struct LiveRanges<'db> {
    #[returns(ref)]
    pub ranges: Vec<LiveRange<'db>>,
}

#[salsa::tracked]
pub struct LiveRange<'db> {
    pub slot_id: SlotId,
    pub birth: ProgramPoint,    // where value is written
    pub death: ProgramPoint,    // last use
    pub is_initialized: InitState,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum InitState {
    Always,           // definitely initialized
    Sometimes,        // conditionally initialized (if-branches)
    Never,            // never initialized on this path
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct ProgramPoint {
    pub stmt_id: StmtId,
    pub position: Position,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum Position {
    Before,
    After,
}
```

### Move Tracking

```rust
#[salsa::tracked]
pub struct MoveInfo<'db> {
    #[returns(ref)]
    pub moves: Vec<MoveOp<'db>>,
    #[returns(ref)]
    pub last_uses: Vec<(SlotId, ExprId)>,
}

#[salsa::tracked]
pub struct MoveOp<'db> {
    pub expr_id: ExprId,
    pub slot_id: SlotId,
    pub move_kind: MoveKind,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum MoveKind {
    FunctionCall,      // argument moved to callee
    FunctionReturn,    // return value moved to caller
    Assignment,        // let binding consumes value
    LastUse,          // last use optimization
    Copy,             // automatic copy for scalar types (bool, u32, i32, f32)
    // Clone will be added later for explicit .clone() operations
}
```

### Drop Points

```rust
#[salsa::tracked]
pub struct DropPoints<'db> {
    #[returns(ref)]
    pub drops: Vec<DropPoint<'db>>,
}

#[salsa::tracked]
pub struct DropPoint<'db> {
    pub slot_id: SlotId,
    pub location: ProgramPoint,
    pub reason: DropReason,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub enum DropReason {
    EndOfScope,
    EarlyReturn,
    Moved,           // slot was moved, no drop needed
    Uninitialized,   // slot never initialized, no drop needed
}
```

### Control Flow Graph

```rust
#[salsa::tracked]
pub struct ControlFlowGraph<'db> {
    #[returns(ref)]
    pub blocks: Vec<BasicBlock>,
    #[returns(ref)]
    pub edges: Vec<ControlFlowEdge>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct BasicBlock {
    pub block_id: BlockId,
    pub statements: Vec<StmtId>,
    pub terminator: Terminator,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct BlockId(u32);

#[derive(Clone, Hash, PartialEq, Eq)]
pub enum Terminator {
    Return,
    Branch { then_block: BlockId, else_block: BlockId },
    Goto(BlockId),
    TryReturn,  // early return from ? or !
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ControlFlowEdge {
    pub from: BlockId,
    pub to: BlockId,
}
```

## Analysis Passes

### Phase 1: Initial Setup

1. **Slot Allocation**
   - Walk function body to identify all locals (let bindings, parameters, if-bindings)
   - Identify temporary slots needed for expressions
   - Assign slot IDs

2. **Type Size Computation**
   - For each slot's `TypeAndHeap<'db>`, compute size and alignment
   - Reference slots are always pointer-sized (8 bytes on 64-bit)
   - Local and Temporary slots use actual type size
   - Handle recursive types, tuples, structs, etc.

3. **Frame Layout Computation**
   - Pack slots with proper alignment
   - Reference slots aligned to pointer alignment
   - Local and Temporary slots aligned to their type's alignment
   - Compute total frame size

### Phase 2: Control Flow Analysis

4. **Build CFG**
   - Identify basic blocks
   - Build edges between blocks
   - Handle if-statements, early returns (ret, try operators)

5. **Dominator Analysis** (if needed for optimizations)
   - Compute dominator tree
   - Identify loop headers (for future use)

### Phase 3: Linear Type Analysis

6. **Initialization Analysis**
   - Track definite/possible initialization across all paths
   - Handle conditional initialization (if-branches)
   - Detect use-before-initialization

7. **Liveness Analysis**
   - Compute live ranges for each slot
   - Identify birth points (writes)
   - Identify death points (last reads)
   - Reference slots live for entire function duration

8. **Move Analysis**
   - Identify all move operations:
     - Function calls with `In` mode: move through reference
     - Function returns (value moved)
     - Let bindings (RHS moved to slot)
   - Distinguish moves through references vs direct moves
   - Mark last-use points
   - No clones should exist at this stage

9. **Borrow Analysis** (future)
   - Track temporary borrows
   - Ensure borrowed values not moved while borrowed
   - Handle parameter modes: In, Out, Ref, Mut

### Phase 4: Resource Management

10. **Drop Point Insertion**
    - For each slot, determine where it must be dropped:
      - Reference slots: never dropped (caller owns the data)
      - Local/Temporary slots at end of scope (if not moved)
      - Local/Temporary slots before early returns
      - Never (if moved or never initialized)
    - Generate drop instructions

11. **Drop Flag Elimination** (optimization)
    - If initialization state is statically known, elide runtime flags
    - Mark slots that always/never need dropping

### Phase 5: Validation

12. **Linear Usage Check**
    - Verify each value used exactly once (no multiple moves)
    - Verify no use after move
    - Generate errors for violations

13. **Unreachable Code Detection**
    - Mark code after returns/diverging try operators
    - Warn about unreachable code

## Implementation Order

### Milestone 1: Basic Infrastructure
- [x] Create analysis module structure (`crates/datalove-datafun/src/function_analysis/`)
- [x] Define core data structures (slot info, frame layout)
- [x] Implement slot allocation for parameters and locals
- [x] Implement type size/align computation
- [x] Implement frame packing algorithm with Reference slot handling

### Milestone 2: Control Flow
- [x] Build basic CFG from function body
- [x] Handle if-statements (branches)
- [x] Handle return statements
- [x] Handle try operators (early returns)
- [x] Fix CFG builder bug (duplicate blocks with self-loops in if-statements)

### Milestone 3: Linear Analysis
- [x] Implement initialization analysis
  - Fixed-point iteration across CFG blocks
  - Track Always/Sometimes/Never initialization states
  - Handle Reference slots (always initialized), Local/Temporary slots
  - Merge states at join points (Always+Always→Always, else→Sometimes)
  - 6 comprehensive tests covering linear, conditional, nested cases
- [x] Implement liveness analysis (birth/death points)
  - Computes birth points (where slots are written)
  - Computes death points (last reads for Local/Temporary, function exit for Reference)
  - Reference slots live for entire function duration
  - Tracks reads through expressions (Name, BinOp, UnaryOp, FunctionCall, Tuple, Try)
  - 5 comprehensive tests covering parameters, locals, binary ops, tuples, multiple uses
- [x] Implement move tracking
  - Function registry for resolving function calls to definitions
  - Detects moves in: let bindings (Assignment), return statements (FunctionReturn), function calls with In parameters (FunctionCall)
  - Distinguishes In (move) vs Ref/Mut/Out (borrow) parameter modes
  - Recursively tracks moves through complex expressions (tuples, binary ops, function calls)
  - 6 comprehensive tests: let binding, return, In parameter, Ref parameter (no move), nested calls, tuple construction
- [x] Implement last-use detection
  - Tracks all reads during AST walk with stmt_id and expr_id
  - Groups reads by slot and finds the maximum stmt_id (last read location)
  - Marks all reads at the last statement as last uses (handles multiple reads in same statement)
  - Works correctly for Reference slots (parameters) and Local/Temporary slots
  - 5 comprehensive tests: simple linear, multiple reads, conditionals, parameter chains, tuple construction

### Milestone 4: Drop Points
- [x] Identify drop points for all slots
  - Iterates through exit blocks (Return/TryReturn) in CFG
  - Checks initialization state at each exit using InitializationAnalysis
  - Skips Reference slots (never dropped)
  - Skips slots that were moved (no drop needed)
  - Skips never-initialized slots (no drop needed)
- [x] Generate drop instructions
  - Inserts DropPoint at last statement of exit blocks
  - Distinguishes EndOfScope (normal return) vs EarlyReturn (try operators)
  - Conservative approach: tracks moves globally (not path-sensitive)
- [x] Handle moved values (no drop needed)
  - Collects all moved slots from MoveInfo
  - Skips drop insertion for moved slots
  - 5 comprehensive tests: simple local, parameters, not-moved local, conditionals, multiple locals

### Milestone 5: Integration
- [x] Create Salsa tracked query for function analysis
  - Created `analyze_function` Salsa tracked query that orchestrates all analysis phases
  - Converted `SlotAllocation` and `AllocatedSlot` to Salsa tracked structs
  - Implemented type extraction from TypecheckResult for frame layout
  - All existing tests pass (26 tests across CFG, liveness, moves, drops)
- [ ] Integrate with existing typechecker
- [ ] Update interpreter to use frame layout
- [ ] Add tests

### Milestone 6: Validation & Optimization
- [ ] Add linear usage validation
- [ ] Add unreachable code detection
- [ ] Implement drop flag elimination
- [ ] Add comprehensive test suite

## Integration Points

### With Typechecker
- Analysis runs after successful typecheck
- Consumes `TypecheckResult<'db>` to get expression types
- Uses `TypeAndHeap<'db>` for all type information

### With Interpreter
- Interpreter queries `function_analysis(db, func)` for each function call
- Uses `FrameLayout` to allocate single packed frame
- Uses `TypeTable` to map `TypeAndHeap<'db>` to `*const rtdt::TyDesc`
- Follows move semantics (no clones unless explicitly requested)
- Inserts drop calls at specified drop points

### With Future Compiler
- Frame analysis will be consumed by bytecode compiler
- Drop points become explicit bytecode instructions
- Move operations tracked for optimization

## Testing Strategy

1. **Unit tests for each pass**
   - Slot allocation
   - Frame packing
   - CFG construction
   - Liveness computation

2. **Integration tests**
   - Simple functions (linear flow)
   - Functions with if-statements
   - Functions with early returns
   - Functions with try operators
   - Nested function calls

3. **Property tests**
   - Every slot has exactly one birth and one death (or is unused)
   - Drop points cover all initialized slots
   - No use after move
   - Frame size matches sum of slot sizes + padding

## Design Decisions

### 1. Temporary Expression Slots

**Decision**: Allocate explicit slots for all subexpressions initially.

- Each compound expression gets its own temporary slot
- Track all temporaries in frame layout
- Optimization for slot reuse deferred to later milestone
- Rationale: Simplicity first, optimize once basic analysis works

**Example**:
```
let x = f(g(a), h(b))
```
Gets temporaries: `t1` for `g(a)`, `t2` for `h(b)`, then both moved to `f`.

### 2. Parameter Modes and Calling Convention

**Decision**: All parameters are passed by reference using Reference slots.

For simplicity and uniformity, all parameter modes use the same physical representation:
a pointer-sized Reference slot in the callee's frame that points to data in the caller's frame.

**Frame layout**: All parameters get `SlotKind::Reference` slots (pointer-sized).

**Calling convention**: Caller evaluates arguments and passes pointers to the callee.

**Parameter mode semantics**:

**`In` (by-value)**: Move semantics via reference
- Caller evaluates argument into a slot/temporary
- Callee receives pointer to that location
- Callee "moves" the value by reading through the reference
- After move, caller must not use the value (move analysis enforces)
- Drop responsibility transfers to callee

**`Out` (by-mut-ptr)**: Caller allocates, callee initializes
- Caller allocates uninitialized slot and passes pointer
- Callee must initialize before returning
- Caller owns the value (callee does not drop)
- Analysis: treat like additional return value
- After call, caller has initialized value in its frame

**`Ref` (by-ref)**: Temporary immutable borrow
- Caller passes pointer to existing initialized value
- Callee can read but not move
- Caller retains ownership (callee does not drop)
- Value must remain live during call
- Borrow checker ensures no moves while borrowed

**`Mut` (by-mut-ref)**: Temporary mutable borrow
- Caller passes pointer to existing initialized value
- Callee can read and modify but not move
- Caller retains ownership (callee does not drop)
- Value must remain live during call
- Borrow checker ensures exclusive access

### 3. Error Handling

**Decision**: Separate error type for linear analysis errors.

Define new `AnalysisError` type distinct from `TypeError`:

```rust
pub enum AnalysisError {
    UseAfterMove {
        slot: SlotId,
        use_location: ProgramPoint,
        move_location: ProgramPoint,
    },
    DoubleMove {
        slot: SlotId,
        first_move: ProgramPoint,
        second_move: ProgramPoint,
    },
    UseBeforeInit {
        slot: SlotId,
        use_location: ProgramPoint,
    },
    UninitializedReturn {
        slot: SlotId,
        paths: Vec<ProgramPoint>,
    },
    ValueNotUsed {
        slot: SlotId,
    },
    BorrowedValueMoved {
        slot: SlotId,
        borrow_location: ProgramPoint,
        move_location: ProgramPoint,
    },
}
```

Rationale: Type errors and usage errors are conceptually distinct phases.

### 4. Clone Semantics

**Decision**: Explicit clones for allocated types, automatic for scalars.

**For allocated types** (String, List, Struct, etc.):
- Explicit AST node when clone is added:
  ```rust
  pub enum ExprFunKind<'db> {
      // ... existing variants
      Clone(ExprClone<'db>),  // explicit .clone() syntax
  }
  ```
- Clear in AST where allocation happens
- Analysis can distinguish moves from clones

**For scalar types** (bool, u32, i32, f32):
- Automatic copy semantics (no explicit clone needed)
- These are Copy types - bitwise copy is always valid
- Analysis can optimize away explicit moves

**Move tracking updated**:
```rust
pub enum MoveKind {
    FunctionCall,
    FunctionReturn,
    Assignment,
    LastUse,
    Copy,   // automatic copy for scalar types
}
```

When explicit clone syntax is added, a new `Clone` variant will be added.

### 5. Tracking All Temporaries

**Decision**: Track temporaries for all expressions initially.

- Even scalar types get temporary slots
- Simplifies initial implementation
- Enables uniform treatment of all types
- Future optimization can eliminate scalar temporaries that don't need tracking

## Tracking Temporary Expressions for Type Resolution

### Problem Statement

Temporary slots are allocated during slot allocation but we can't look up their types because we don't track which expression created each temporary. This causes all temporaries to get placeholder `bool` types instead of their actual types from the typechecker.

### Root Cause

1. **Slot allocation** creates temporaries when analyzing expressions (BinOp, FunctionCall, etc.) but only stores `SlotId` without recording the corresponding `ExprFun`
2. **Typechecker** indexes expression types by `ExprFun`'s Salsa ID in `TypecheckResult.expr_types`
3. **Frame layout** has a `SlotId` but no way to map it back to the `ExprFun` to look up its type

### Solution: Add Expression Field to AllocatedSlot

Add `expr: Option<ExprFun<'db>>` field to `AllocatedSlot`:
- Reference/Local slots: `expr = None` (they don't come from expressions)
- Temporary slots: `expr = Some(expr)` pointing to the creating expression

### Implementation Steps

1. **Update `AllocatedSlot` structure** - Add `pub expr: Option<ExprFun<'db>>` field
2. **Update `SlotAllocationBuilder`** - Add 4th element to internal tuple: `Option<ExprFun<'db>>`
3. **Update `alloc_slot` signature** - Add `expr: Option<ExprFun<'db>>` parameter
4. **Update parameter/local allocations** - Pass `None` for expression (they don't come from expressions)
5. **Update temporary allocations** - Pass `Some(expr)` when creating temporaries for BinOp, FunctionCall, Tuple, UnaryOp, TryOption, TryResult
6. **Update `allocate_slots` conversion** - Include expression in `AllocatedSlot::new` call
7. **Fix type lookup in `build_frame_layout`** - Use `slot.expr(db)` to get expression, then look up type in `expr_types`

### Expected Outcome

- All temporary slots will have accurate types from the typechecker
- Frame layout will compute correct sizes for temporaries
- No more placeholder `bool` types for temporaries
- Full type information available for all three slot kinds: Reference, Local, and Temporary

### Status: COMPLETED

All changes implemented and tested:
- Added `expr: Option<ExprFun<'db>>` field to `AllocatedSlot`
- Updated `SlotAllocationBuilder` to track expressions
- Modified `alloc_slot` signature to accept expression parameter
- Updated all allocation sites (parameters/locals pass `None`, temporaries pass `Some(expr)`)
- Fixed type lookup in `build_frame_layout` to use expression for temporaries
- All 26 function analysis tests pass

## Phase 6: Validation Implementation Plan

### Architecture
- Add `errors` field to `FunctionAnalysis<'db>` to store validation errors
- Create `AnalysisError` enum with all error variants
- Implement validation passes as separate functions, each returning `Vec<AnalysisError>`
- Add validation phase to `analyze_function` after drop points computation

### Error Type Definitions

Create new module `crates/datalove-datafun/src/function_analysis/validation.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnalysisError {
    UseBeforeInit {
        slot: SlotId,
        use_location: ProgramPoint,
    },
    DoubleMove {
        slot: SlotId,
        first_move: ProgramPoint,
        second_move: ProgramPoint,
    },
    UseAfterMove {
        slot: SlotId,
        use_location: ProgramPoint,
        move_location: ProgramPoint,
    },
    UninitializedReturn {
        slot: SlotId,
        paths: Vec<ProgramPoint>,
    },
    ValueNotUsed {
        slot: SlotId,
    },
    BorrowedValueMoved {
        slot: SlotId,
        borrow_location: ProgramPoint,
        move_location: ProgramPoint,
    },
}
```

### FunctionAnalysis Update

Add error storage to `FunctionAnalysis<'db>`:

```rust
#[salsa::tracked]
pub struct FunctionAnalysis<'db> {
    pub function: StmtFun<'db>,
    pub frame_layout: FrameLayout<'db>,
    pub live_ranges: LiveRanges<'db>,
    pub move_info: MoveInfo<'db>,
    pub drop_points: DropPoints<'db>,
    pub control_flow: ControlFlowGraph<'db>,
    #[returns(ref)]
    pub errors: Vec<AnalysisError>,  // NEW
}
```

Update `analyze_function` to include validation:

```rust
// Phase 7: Validation
let errors = validate_function(db, func, slots, control_flow, init_analysis, move_info, live_ranges);

FunctionAnalysis::new(
    db,
    func,
    frame_layout,
    live_ranges,
    move_info,
    drop_points,
    control_flow,
    errors,  // NEW
)
```

### Validation Passes (Implement One at a Time)

#### Pass 1: Use-Before-Initialization Check

**Purpose**: Detect reads of uninitialized slots.

**Algorithm**:
1. Walk all statements collecting reads with their locations
2. For each read at program point P:
   - Look up slot's initialization state at P using `InitializationAnalysis`
   - Error if state is `Never` or `Sometimes`
   - Skip Reference slots (parameters - always initialized)
3. Return list of `UseBeforeInit` errors

**Implementation**:
```rust
fn check_use_before_init<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    init: InitializationAnalysis<'db>,
    cfg: ControlFlowGraph<'db>,
) -> Vec<AnalysisError>
```

**Test cases**:
- Use of uninitialized local
- Conditional initialization with use after if
- Proper use after initialization (no error)

#### Pass 2: Double-Move Check

**Purpose**: Detect multiple moves of the same slot.

**Algorithm**:
1. Group moves by slot_id from `MoveInfo.moves`
2. For each slot with 2+ moves:
   - Since analysis is not path-sensitive, any duplicate is an error
   - Create `DoubleMove` error with first and second move locations
3. Return list of `DoubleMove` errors

**Implementation**:
```rust
fn check_double_move<'db>(
    db: &'db dyn crate::Db,
    move_info: MoveInfo<'db>,
    slots: &[AllocatedSlot<'db>],
) -> Vec<AnalysisError>
```

**Test cases**:
- Move same local twice in linear flow
- Move parameter then local derived from it (should be ok - different slots)
- Single move (no error)

#### Pass 3: Use-After-Move Check

**Purpose**: Detect reads that occur after a slot has been moved.

**Algorithm**:
1. Build map of slot -> move locations from `MoveInfo`
2. Walk all reads with their locations
3. For each read at program point P:
   - Check if slot has any moves before P
   - Compare `ProgramPoint` ordering (stmt_id, position)
   - Error if read follows move
4. Return list of `UseAfterMove` errors

**Implementation**:
```rust
fn check_use_after_move<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    move_info: MoveInfo<'db>,
) -> Vec<AnalysisError>
```

**Ordering logic**:
- If stmt_id_1 < stmt_id_2: move comes before use
- If stmt_id_1 == stmt_id_2: check position (Before < After)

**Test cases**:
- Read after move in linear flow
- Read before move (no error)
- Read in different branch from move (conservative: still error)

#### Pass 4: Uninitialized-Return Check

**Purpose**: Detect return from function with uninitialized Out parameters.

**Algorithm**:
1. Find all exit blocks (Return/TryReturn terminators) from CFG
2. For each exit block:
   - Get exit_states from `InitializationAnalysis`
   - For each slot with kind != Reference:
     - If state is Never or Sometimes, may indicate improper usage
   - For Out parameters specifically:
     - Must be Always initialized at return
3. Return list of `UninitializedReturn` errors

**Implementation**:
```rust
fn check_uninitialized_return<'db>(
    db: &'db dyn crate::Db,
    slots: &[AllocatedSlot<'db>],
    init: InitializationAnalysis<'db>,
    cfg: ControlFlowGraph<'db>,
) -> Vec<AnalysisError>
```

**Test cases**:
- Out parameter not initialized before return
- Conditional initialization of Out parameter
- Proper initialization (no error)

#### Pass 5: ValueNotUsed Check

**Purpose**: Detect slots that are written but never read.

**Algorithm**:
1. For each Local/Temporary slot:
   - Check if it has any reads (look in LiveRanges for death point)
   - If no death point found, slot was never read
   - Skip Reference slots (parameters - used by caller)
2. Return list of `ValueNotUsed` errors

**Implementation**:
```rust
fn check_value_not_used<'db>(
    db: &'db dyn crate::Db,
    slots: &[AllocatedSlot<'db>],
    live_ranges: LiveRanges<'db>,
) -> Vec<AnalysisError>
```

**Test cases**:
- Let binding never used
- All values used (no error)
- Parameter not used (should be ok or warning, not error)

#### Pass 6: Unreachable Code Detection

**Purpose**: Warn about code that can never execute.

**Algorithm**:
1. Find blocks with no predecessors (except entry block)
2. Find statements after Return/TryReturn terminators
3. Generate warnings (not errors)

**Implementation**:
```rust
fn check_unreachable_code<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    cfg: ControlFlowGraph<'db>,
) -> Vec<AnalysisWarning>  // New type for warnings
```

**Test cases**:
- Code after return
- Unreachable else branch
- All code reachable (no warning)

### Orchestration Function

Create main validation entry point:

```rust
pub fn validate_function<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &[AllocatedSlot<'db>],
    cfg: ControlFlowGraph<'db>,
    init: InitializationAnalysis<'db>,
    move_info: MoveInfo<'db>,
    live_ranges: LiveRanges<'db>,
) -> Vec<AnalysisError> {
    let mut errors = Vec::new();

    // Pass 1: Use-before-init
    errors.extend(check_use_before_init(db, func, slots, init, cfg));

    // Pass 2: Double-move
    errors.extend(check_double_move(db, move_info, slots));

    // Pass 3: Use-after-move
    errors.extend(check_use_after_move(db, func, slots, move_info));

    // Pass 4: Uninitialized return
    errors.extend(check_uninitialized_return(db, slots, init, cfg));

    // Pass 5: Value not used
    errors.extend(check_value_not_used(db, slots, live_ranges));

    errors
}
```

### Testing Strategy

For each validation pass:
1. Create dedicated test module in `crates/datalove-datafun/src/function_analysis/validation.rs`
2. Test positive cases (error detected)
3. Test negative cases (no error when code is correct)
4. Test edge cases (empty functions, parameters, conditionals)

Example test structure:
```rust
#[test]
fn test_use_before_init_detects_error() {
    // Function with uninitialized use
    // Assert error is detected
}

#[test]
fn test_use_before_init_no_error_when_initialized() {
    // Function with proper initialization
    // Assert no errors
}
```

### Implementation Status

- [x] Create validation.rs module with AnalysisError enum
- [x] Update FunctionAnalysis with errors field
- [x] Implement Pass 1: Use-Before-Initialization
  - Implemented with linear walk tracking initialization states
  - Handles conditional branches (if-statements)
  - 4 tests: detects errors, no false positives, conditionals, parameters
- [x] Implement Pass 2: Double-Move Check
  - Groups moves by slot and detects duplicates
  - Filters out Copy moves (scalar types)
  - 3 tests: detects errors, different slots ok, single move ok
- [x] Implement Pass 3: Use-After-Move Check
  - Builds map of move locations and checks reads against them
  - Uses ProgramPoint ordering (stmt_id, position)
  - 3 tests: detects errors, single use ok, conditional move ok
- [x] Implement Pass 4: Uninitialized-Return Check
  - Modified initialization analysis to treat Out parameters as Never initially
  - Checks exit states of all return blocks for Out parameters
  - Fixed CFG bug where join_block wasn't created after if-statements
  - 3 tests: detects uninitialized Out param, conditional init, properly initialized ok
- [x] Implement Pass 5: ValueNotUsed Check
  - Checks LiveRanges to find slots with no reads (death == birth)
  - Skips Reference slots (parameters used by caller)
  - 3 tests: detects unused local, all values used ok, unused parameter ok
- [ ] Implement Pass 6: Unreachable Code Detection
- [ ] Integration testing with full analysis pipeline

## Future Extensions

- **Escape analysis**: detect values that don't escape function
- **Stack vs heap allocation**: use escape analysis to avoid heap
- **Move optimization**: elide copies for same-layout moves
- **Lifetime analysis**: for references and borrows
- **Alias analysis**: for mutable references
