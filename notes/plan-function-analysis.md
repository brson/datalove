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
- [ ] Implement liveness analysis (birth/death points)
- [ ] Implement move tracking
- [ ] Implement last-use detection

### Milestone 4: Drop Points
- [ ] Identify drop points for all slots
- [ ] Generate drop instructions
- [ ] Handle moved values (no drop needed)

### Milestone 5: Integration
- [ ] Create Salsa tracked query for function analysis
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

## Future Extensions

- **Escape analysis**: detect values that don't escape function
- **Stack vs heap allocation**: use escape analysis to avoid heap
- **Move optimization**: elide copies for same-layout moves
- **Lifetime analysis**: for references and borrows
- **Alias analysis**: for mutable references
