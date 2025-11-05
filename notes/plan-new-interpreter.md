# New AST-Walk Interpreter Design

## Progress Status

- [x] Phase 0: Script Execution Infrastructure - **COMPLETED**
- [ ] Phase 1: Core Infrastructure (Frame Management)
- [x] Phase 2: Expression Evaluation - **COMPLETED** (literals and binary ops)
- [ ] Phase 3: Move Semantics
- [ ] Phase 4: Control Flow
- [ ] Phase 5: Drop Execution
- [ ] Phase 6: Function Calls
- [ ] Phase 7: Module Integration
- [ ] Phase 8: Testing & Validation

**Current Status**: Phase 2 completed with literals, binary arithmetic operations, and zero-argument function calls. Implemented automatic u32→Int widening for bare operators (+, -, *, /). Added function call support for script-level zero-argument functions with proper local variable scoping and return value handling. Implemented basic copy type detection (u32, Bool are copy; Int, String are linear). Converted to example-based testing pattern. Fixed memory management issues (ScriptResult Drop, destroy_value). All 20 interp tests passing with zero memory leaks. Ready for function parameters and remaining expression types (comparisons, tuples).

## Overview

Design for a new tree-walking interpreter that uses the function_analysis framework to achieve safe, leak-free execution with proper linear type semantics and package world integration.

## Problems with Old Interpreter

Based on analysis of `crates/datalove-datafun/src/interp_old/`:

1. **Unsafe Code**: 302 unsafe blocks for manual memory management
2. **Memory Leaks**: Unfound leaks, complex double-free prevention logic
3. **Unoptimized**: HashMap-based stack frames instead of packed layouts
4. **No Linear Semantics**: Manual ownership tracking without compiler help
5. **Incomplete**: TODOs for Error/Data handling, Result payload freeing
6. **Package World**: Doesn't properly integrate with module system

## Design Principles

1. **Analysis-Driven**: Use function_analysis results to drive execution
2. **Safe Rust**: Minimize unsafe code, leverage Rust's ownership system
3. **Packed Frames**: Use computed frame layouts from analysis
4. **Linear Types**: Follow move/copy semantics from analysis
5. **Explicit Drops**: Execute drops at analysis-computed drop points
6. **Module-Aware**: Integrate with PackageWorld for module resolution

## Architecture

### High-Level Structure

```
┌─────────────────────────────────────────────────────────────┐
│                 Script + Package World                       │
│  - Script: Vec<ScriptUnit> (REPL history)                   │
│  - PackageWorld: sys/local libraries with modules           │
│  - Combined via ScriptWithPackageWorld                       │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              Compilation Pipeline                            │
│  - Parse: per-ScriptUnit (Salsa memoized)                   │
│  - Resolve: full script + package world modules             │
│  - Typecheck: full script + package world                   │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              Function Analysis (per function)                │
│  - CFG, Slots, Liveness, Moves, Drops, Validation          │
│  - Triggered for each function in script + modules          │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              New Interpreter                                 │
│  - Script execution (batch mode)                             │
│  - REPL execution (incremental mode)                         │
│  - Function execution (analysis-guided)                      │
│  - Packed frames, safe ownership tracking                   │
└─────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                Runtime (datalove-rt)                         │
│  - rust::Runtime (RAII wrapper)                              │
│  - impls::* for internal operations                          │
└─────────────────────────────────────────────────────────────┘
```

### Script Execution Model

**Two execution modes:**

1. **Batch Mode**: Execute complete script file
   - Load PackageWorld (sys library)
   - Parse entire script as single unit
   - Typecheck script + package world
   - Execute all statements in order
   - Return final result

2. **REPL Mode**: Interactive incremental execution
   - Maintain persistent interpreter context
   - On each submission: add new ScriptUnit to history
   - Parse only new unit (previous units cached by Salsa)
   - Typecheck full combined script
   - Execute only new unit's statements
   - Accumulate variables and functions in context

### Core Data Structures

#### 1. Interpreter Context

```rust
pub struct InterpContext<'db> {
    db: &'db dyn crate::Db,
    runtime: datalove_rt::rust::Runtime,
    package_world: PackageWorld,
    script: Option<Script<'db>>,  // Current script being executed
    call_stack: Vec<StackFrame<'db>>,
    script_scope: ScriptScope<'db>,  // Script-level state (REPL mode)
}

/// Script-level scope for REPL incremental execution.
pub struct ScriptScope<'db> {
    variables: HashMap<InternedText<'db>, ScriptVariable>,  // Script-level let bindings
    functions: HashMap<InternedText<'db>, StmtFun<'db>>,  // Script-level functions
}

/// Script-level variable with move tracking.
pub struct ScriptVariable {
    value: Value,
    state: ScriptVarState,
    is_copy: bool,  // Cached from type analysis
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ScriptVarState {
    Available,
    Moved,
}
```

**Key differences from old interpreter:**
- Uses `rust::Runtime` (RAII wrapper, auto-cleanup)
- Stores `PackageWorld` for module resolution
- Stores `Script` to track which units are being executed
- `ScriptScope` separates script-level state from call stack
- Script variables track move state (linear semantics preserved)
- No `TypeTable` needed - uses analysis types directly

#### 2. Stack Frame

```rust
pub struct StackFrame<'db> {
    function: StmtFun<'db>,
    analysis: FunctionAnalysis<'db>,
    frame_data: Vec<u8>,  // Packed allocation for all slots
    slot_states: Vec<SlotState>,  // Track init/moved state
    program_counter: ProgramCounter,
}

#[derive(Copy, Clone)]
pub enum SlotState {
    Uninitialized,
    Initialized,
    Moved,
}

pub struct ProgramCounter {
    block_id: BlockId,
    stmt_index: usize,
}
```

**Key differences from old interpreter:**
- Single `Vec<u8>` for all frame data (packed layout)
- Explicit slot state tracking (no implicit ownership)
- Program counter for CFG-based execution
- No HashMap for variables - use offsets from analysis

#### 3. Value Representation

```rust
pub struct Value {
    ptr: *mut u8,          // Pointer into frame_data or heap
    tydesc: *const TyDesc,  // Runtime type descriptor
    location: ValueLocation,
}

#[derive(Copy, Clone)]
pub enum ValueLocation {
    FrameSlot(SlotId),     // Lives in current frame
    HeapOwned,             // Heap allocation owned by this value
    Reference,             // Reference to caller's data
}
```

**Key differences from old interpreter:**
- No separate enum variants for each type
- Explicit location tracking (frame vs heap)
- All values use uniform representation
- Location enum enables safe ownership tracking

### Execution Model

#### Top-Level Entry Points

**1. Batch Script Execution**
```rust
pub fn execute_script<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    package_world: PackageWorld,
) -> Result<Value, InterpError> {
    // Create interpreter context.
    let mut ctx = InterpContext::new(db, package_world, Some(script));

    // Parse and typecheck script with package world.
    let script_with_world = load_script_with_package_world(db, script, package_world)?;

    // Verify no typecheck errors.
    verify_no_errors(db, script_with_world)?;

    // Parse the script AST.
    let parsed_script = parse_script(db, script)?;

    // Execute all statements in script.
    for stmt in parsed_script.statements(db) {
        execute_script_statement(&mut ctx, stmt)?;
    }

    // Return final result (e.g., value of 'output' variable).
    ctx.script_scope.variables.get(&S("output"))
        .C()
        .ok_or(InterpError::NoOutputVariable)
}
```

**2. REPL Incremental Execution**
```rust
pub fn execute_script_unit<'db>(
    ctx: &mut InterpContext<'db>,
    script: Script<'db>,
    unit_index: usize,
) -> Result<ScriptUnitResult, InterpError> {
    // Update context with new script.
    ctx.script = Some(script);

    // Parse only the new unit (Salsa caches previous units).
    let unit_ast = parse_script_unit(ctx.db, script, unit_index)?;

    // Typecheck FULL script (including all active units).
    let script_with_world = load_script_with_package_world(ctx.db, script, ctx.package_world)?;
    verify_no_errors(ctx.db, script_with_world)?;

    // Execute only statements from the new unit.
    for stmt in unit_ast.statements(ctx.db) {
        execute_script_statement(ctx, stmt)?;
    }

    Ok(ScriptUnitResult {
        last_value: get_last_expression_value(ctx),
        functions_defined: count_functions_in_unit(unit_ast),
    })
}
```

**3. Script Statement Execution**
```rust
fn execute_script_statement<'db>(
    ctx: &mut InterpContext<'db>,
    stmt: Stmt<'db>,
) -> Result<(), InterpError> {
    match stmt.kind(ctx.db) {
        StmtKind::Let(let_stmt) => {
            // Evaluate expression (may move variables).
            let value = eval_expression_in_script_scope(ctx, let_stmt.expr(ctx.db))?;

            // Determine if type is copy.
            let ty = get_expression_type(ctx.db, let_stmt.expr(ctx.db));
            let is_copy = is_copy_type(ctx.db, ty);

            // Bind to script-level variable.
            let name = let_stmt.name(ctx.db);
            ctx.script_scope.variables.insert(name, ScriptVariable {
                value,
                state: ScriptVarState::Available,
                is_copy,
            });

            Ok(())
        }

        StmtKind::Function(fun_stmt) => {
            // Analyze function.
            let analysis = analyze_function(ctx.db, fun_stmt)?;

            // Verify no linear usage errors.
            if !analysis.errors(ctx.db).is_empty() {
                return Err(InterpError::AnalysisErrors(analysis.errors(ctx.db).C()));
            }

            // Add to script-level function table.
            let name = fun_stmt.name(ctx.db);
            ctx.script_scope.functions.insert(name, fun_stmt);

            Ok(())
        }

        StmtKind::Return(_) => {
            Err(InterpError::ReturnOutsideFunction)
        }

        StmtKind::If(_) => {
            Err(InterpError::IfOutsideFunction)
        }

        StmtKind::Require(_) | StmtKind::Import(_) => {
            // Already handled by package world loading.
            Ok(())
        }
    }
}
```

#### Function Execution Phases

**Phase 1: Function Entry**
1. Query `analyze_function(db, func)` to get analysis results
2. Verify no analysis errors (use-after-move, double-move, etc.)
3. Allocate packed frame using `frame_layout.total_size()`
4. Initialize Reference slots (parameters) from caller
5. Set all Local/Temporary slots to Uninitialized state
6. Set program counter to entry block

**Phase 2: CFG-Based Execution**
```rust
loop {
    let block = get_current_block();
    for stmt in block.statements() {
        execute_statement(stmt);
        if early_return_triggered() {
            break;
        }
    }
    match block.terminator() {
        Terminator::Return => return extract_return_value(),
        Terminator::TryReturn => return handle_early_return(),
        Terminator::Branch { then_block, else_block } => {
            pc.block_id = if condition { then_block } else { else_block };
        }
        Terminator::Goto(next) => pc.block_id = next,
    }
}
```

**Phase 3: Drop Execution**
Before each return (normal or early):
1. Query `drop_points` for current exit block
2. For each drop point:
   - Check slot state (skip if Uninitialized or Moved)
   - Get tydesc and pointer from slot
   - Call runtime drop: `dtlv_rti_any_destroy(ptr, tydesc)`
   - Mark slot as Moved (prevent double-drop)

**Phase 4: Return**
- Extract return value from designated slot
- Transfer ownership to caller
- Frame deallocated (automatic via Vec drop)
- Runtime auto-cleanup via Drop trait

#### Statement Execution

```rust
fn execute_statement<'db>(
    ctx: &mut InterpContext<'db>,
    frame: &mut StackFrame<'db>,
    stmt: &Stmt<'db>,
) -> Result<ControlFlow, InterpError> {
    match stmt.kind(ctx.db) {
        StmtKind::Let(let_stmt) => {
            // Evaluate RHS expression.
            let value = eval_expression(ctx, frame, let_stmt.expr(db))?;

            // Find destination slot from analysis.
            let slot_id = find_slot_for_let(frame.analysis, let_stmt);
            let slot_info = get_slot_info(frame.analysis, slot_id);

            // Write value to slot at computed offset.
            write_value_to_slot(frame, slot_info, value)?;

            // Mark slot as initialized.
            frame.slot_states[slot_id] = SlotState::Initialized;

            // If RHS was a move, mark source as moved.
            if let Some(source_slot) = get_move_source(frame.analysis, stmt) {
                frame.slot_states[source_slot] = SlotState::Moved;
            }

            Ok(ControlFlow::Continue)
        }

        StmtKind::Return(ret_stmt) => {
            // Evaluate return expression.
            let value = eval_expression(ctx, frame, ret_stmt.expr(db))?;

            // Write to return slot.
            write_return_value(frame, value)?;

            // Execute drops for this exit point.
            execute_drops_for_return(ctx, frame)?;

            Ok(ControlFlow::Return)
        }

        StmtKind::If(if_stmt) => {
            // Evaluate condition.
            let condition = eval_expression(ctx, frame, if_stmt.condition(db))?;

            // Determine branch from analysis CFG.
            let (then_block, else_block) = get_branch_targets(frame.analysis, stmt);

            // Update program counter (execution loop handles rest).
            if extract_bool(condition) {
                Ok(ControlFlow::Branch(then_block))
            } else {
                Ok(ControlFlow::Branch(else_block))
            }
        }

        StmtKind::Require(_) | StmtKind::Import(_) => {
            // Already handled by package world / analysis.
            Ok(ControlFlow::Continue)
        }

        StmtKind::Function(_) => {
            // Already analyzed; functions are resolved via package world.
            Ok(ControlFlow::Continue)
        }
    }
}
```

#### Expression Evaluation

```rust
fn eval_expression<'db>(
    ctx: &mut InterpContext<'db>,
    frame: &mut StackFrame<'db>,
    expr: ExprFun<'db>,
) -> Result<Value, InterpError> {
    match expr.kind(ctx.db) {
        ExprFunKind::Name(name) => {
            // Find slot for this variable.
            let slot_id = find_slot_for_name(frame.analysis, name);
            let slot_info = get_slot_info(frame.analysis, slot_id);

            // Check if this is a move or copy.
            let move_info = frame.analysis.move_info(ctx.db);
            let is_copy = is_copy_move(move_info, expr);

            if is_copy {
                // Copy the value (scalar types).
                copy_value_from_slot(ctx, frame, slot_info)
            } else {
                // Check if this is a last use (move optimization).
                let is_last_use = is_last_use(move_info, slot_id, expr);

                if is_last_use {
                    // Move the value out.
                    let value = read_value_from_slot(frame, slot_info)?;
                    frame.slot_states[slot_id] = SlotState::Moved;
                    Ok(value)
                } else {
                    // Must clone (not last use, not copy type).
                    clone_value_from_slot(ctx, frame, slot_info)
                }
            }
        }

        ExprFunKind::BinOp(binop) => {
            // Evaluate operands (analysis tells us if they move).
            let lhs = eval_expression(ctx, frame, binop.lhs(ctx.db))?;
            let rhs = eval_expression(ctx, frame, binop.rhs(ctx.db))?;

            // Find temporary slot for result.
            let temp_slot = find_temp_for_expr(frame.analysis, expr);

            // Execute operation using runtime.
            let result = execute_binop(ctx, binop.op(ctx.db), lhs, rhs)?;

            // Write to temporary slot.
            write_value_to_slot(frame, temp_slot, result)?;
            frame.slot_states[temp_slot.slot_id] = SlotState::Initialized;

            // Return value (ownership transferred).
            Ok(result)
        }

        ExprFunKind::FunctionCall(call) => {
            // Resolve function via package world.
            let callee = resolve_function(ctx, frame, call.function_name(ctx.db))?;

            // Analyze callee.
            let callee_analysis = function_analysis(ctx.db, callee);

            // Evaluate arguments.
            let args = call.arguments(ctx.db).iter()
                .map(|arg| eval_expression(ctx, frame, *arg))
                .collect::<Result<Vec<_>, _>>()?;

            // Call function (recursive interpreter call).
            call_function(ctx, callee, callee_analysis, args)
        }

        ExprFunKind::Literal(lit) => {
            // Allocate literal value on heap or stack.
            allocate_literal(ctx, frame, lit)
        }

        ExprFunKind::Tuple(tuple) => {
            // Evaluate all elements.
            let elements = tuple.elements(ctx.db).iter()
                .map(|elem| eval_expression(ctx, frame, *elem))
                .collect::<Result<Vec<_>, _>>()?;

            // Find temp slot for tuple.
            let temp_slot = find_temp_for_expr(frame.analysis, expr);

            // Construct tuple at slot location.
            construct_tuple(ctx, frame, temp_slot, elements)?;

            // Return tuple value.
            read_value_from_slot(frame, temp_slot)
        }

        ExprFunKind::Try(try_expr) => {
            // Evaluate inner expression.
            let value = eval_expression(ctx, frame, try_expr.expr(ctx.db))?;

            // Check if Option/Result is None/Err.
            if is_none_or_err(value) {
                // Trigger early return.
                execute_drops_for_early_return(ctx, frame)?;
                return Err(InterpError::EarlyReturn);
            }

            // Extract payload.
            extract_payload(value)
        }

        // ... other expression kinds
    }
}
```

### Memory Management Strategy

#### 1. Frame Allocation

```rust
fn allocate_frame<'db>(
    db: &'db dyn crate::Db,
    analysis: FunctionAnalysis<'db>,
) -> Vec<u8> {
    let layout = analysis.frame_layout(db);
    let total_size = layout.total_size(db) as usize;

    // Single allocation for entire frame.
    vec![0u8; total_size]
}
```

**Safety:**
- Rust owns the allocation (Vec)
- Automatic deallocation on drop
- No manual free needed
- Alignment handled by writing values properly

#### 2. Slot Access

```rust
fn get_slot_ptr<'db>(
    frame: &StackFrame<'db>,
    slot_info: SlotInfo<'db>,
    db: &'db dyn crate::Db,
) -> *mut u8 {
    let offset = slot_info.offset(db) as usize;
    unsafe {
        frame.frame_data.as_ptr().add(offset) as *mut u8
    }
}
```

**Safety:**
- Offsets computed by analysis (guaranteed valid)
- Lifetime tied to frame
- No dangling pointers (frame owns data)

#### 3. Value Operations

**Write to slot:**
```rust
fn write_value_to_slot<'db>(
    frame: &mut StackFrame<'db>,
    slot_info: SlotInfo<'db>,
    value: Value,
    db: &'db dyn crate::Db,
) -> Result<(), InterpError> {
    let ptr = get_slot_ptr(frame, slot_info, db);
    let size = get_type_size(value.tydesc);

    unsafe {
        // Copy bytes into slot.
        std::ptr::copy_nonoverlapping(value.ptr, ptr, size);
    }

    // If value was heap-owned, we now own it via slot.
    // Original value pointer is invalidated.
    Ok(())
}
```

**Read from slot (move):**
```rust
fn read_value_from_slot<'db>(
    frame: &StackFrame<'db>,
    slot_info: SlotInfo<'db>,
    db: &'db dyn crate::Db,
) -> Result<Value, InterpError> {
    let ptr = get_slot_ptr(frame, slot_info, db);
    let tydesc = get_slot_tydesc(slot_info, db);

    Ok(Value {
        ptr,
        tydesc,
        location: ValueLocation::FrameSlot(slot_info.slot_id(db)),
    })
}
```

**Clone from slot:**
```rust
fn clone_value_from_slot<'db>(
    ctx: &mut InterpContext<'db>,
    frame: &StackFrame<'db>,
    slot_info: SlotInfo<'db>,
) -> Result<Value, InterpError> {
    let src_ptr = get_slot_ptr(frame, slot_info, ctx.db);
    let tydesc = get_slot_tydesc(slot_info, ctx.db);
    let size = unsafe { (*tydesc).size };

    // Allocate new storage.
    let dst_ptr = allocate_on_runtime(ctx, size, tydesc);

    // Clone using runtime.
    unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            ctx.runtime.handle(),
            src_ptr,
            tydesc,
            dst_ptr,
        );
    }

    Ok(Value {
        ptr: dst_ptr,
        tydesc,
        location: ValueLocation::HeapOwned,
    })
}
```

#### 4. Drop Execution

```rust
fn execute_drops_for_return<'db>(
    ctx: &mut InterpContext<'db>,
    frame: &mut StackFrame<'db>,
) -> Result<(), InterpError> {
    let drop_points = frame.analysis.drop_points(ctx.db);
    let current_block = frame.program_counter.block_id;

    // Find drops for this exit block.
    let drops = drop_points.drops(ctx.db).iter()
        .filter(|dp| dp.location(ctx.db).block_id == current_block);

    for drop_point in drops {
        let slot_id = drop_point.slot_id(ctx.db);

        // Skip if not initialized or already moved.
        match frame.slot_states[slot_id] {
            SlotState::Uninitialized => continue,
            SlotState::Moved => continue,
            SlotState::Initialized => {}
        }

        // Get slot info.
        let slot_info = get_slot_info(frame.analysis, slot_id);
        let ptr = get_slot_ptr(frame, slot_info, ctx.db);
        let tydesc = get_slot_tydesc(slot_info, ctx.db);

        // Execute drop via runtime.
        unsafe {
            datalove_rt::c::dtlv_rti_any_destroy_local(
                ctx.runtime.handle(),
                ptr,
                tydesc,
            );
        }

        // Mark as moved to prevent double-drop.
        frame.slot_states[slot_id] = SlotState::Moved;
    }

    Ok(())
}
```

**Safety guarantees:**
- Analysis computes exactly which slots need drops
- Slot states prevent double-drops
- Reference slots never dropped (caller owns data)
- Copy types never get drop points (analysis filters them)

### Package World Integration

#### Module Resolution

```rust
fn resolve_function<'db>(
    ctx: &InterpContext<'db>,
    frame: &StackFrame<'db>,
    name: InternedText<'db>,
) -> Result<StmtFun<'db>, InterpError> {
    // First check script-level functions (REPL accumulation).
    if let Some(func) = ctx.script_scope.functions.get(&name) {
        return Ok(*func);
    }

    // Then check local function definitions (nested functions).
    if let Some(func) = find_local_function(frame.function, name) {
        return Ok(func);
    }

    // Finally check imported modules via package world.
    let script = ctx.script.expect("script should be set during execution");
    let import_demands = extract_import_demands_from_script(ctx.db, script);
    let package_world_map = package_world_map(ctx.db, ctx.package_world);

    for import in import_demands {
        let module = resolve_module(package_world_map, import)?;
        if let Some(func) = find_function_in_module(ctx.db, module, name) {
            return Ok(func);
        }
    }

    Err(InterpError::FunctionNotFound(name))
}
```

**Resolution order:**
1. Script-level functions (from accumulated units in REPL)
2. Local nested functions (in current function body)
3. Package world modules (imported via require/import)

#### Typechecking Integration

```rust
fn call_function<'db>(
    ctx: &mut InterpContext<'db>,
    callee: StmtFun<'db>,
    analysis: FunctionAnalysis<'db>,
    args: Vec<Value>,
) -> Result<Value, InterpError> {
    // Verify function was typechecked successfully.
    let tycheck_result = typecheck_function(ctx.db, callee);
    if tycheck_result.has_errors(ctx.db) {
        return Err(InterpError::TypeErrors);
    }

    // Verify analysis found no linear usage errors.
    let errors = analysis.errors(ctx.db);
    if !errors.is_empty() {
        return Err(InterpError::AnalysisErrors(errors.C()));
    }

    // Allocate new frame.
    let mut new_frame = StackFrame::new(callee, analysis);

    // Write arguments to parameter slots.
    initialize_parameters(&mut new_frame, args)?;

    // Push frame and execute.
    ctx.call_stack.push(new_frame);
    let result = execute_function_body(ctx);
    ctx.call_stack.pop();

    result
}
```

### Error Handling

```rust
pub enum InterpError {
    // Analysis-time errors.
    TypeErrors,
    AnalysisErrors(Vec<AnalysisError>),

    // Runtime errors.
    SlotNotInitialized(SlotId),
    SlotAlreadyMoved(SlotId),
    FunctionNotFound(InternedText),
    ModuleNotFound(String),
    InvalidCast { expected: String, actual: String },
    RuntimeError(String),

    // Control flow (not actual errors).
    EarlyReturn,
    Return,

    // Runtime failures.
    AllocationFailed,
    StackOverflow,
}
```

## Advantages Over Old Interpreter

| Aspect | Old Interpreter | New Interpreter |
|--------|-----------------|-----------------|
| **Unsafe Code** | 302 blocks | Minimal (only for FFI) |
| **Memory Management** | Manual with complex tracking | Analysis-guided, Rust-owned |
| **Stack Frames** | HashMap (slow) | Packed Vec (fast) |
| **Ownership** | Manual tracking | Follow analysis moves |
| **Double-Free Prevention** | Complex runtime logic | Slot state tracking |
| **Leaks** | Unfound leaks exist | Prevented by RAII + analysis |
| **Linear Types** | Not supported | Full support via analysis |
| **Drop Insertion** | Manual at end of scope | Analysis-computed points |
| **Module Resolution** | Limited | Full PackageWorld integration |
| **Copy Semantics** | Manual check each operation | Analysis pre-computed |
| **Validation** | Runtime only | Compile-time + runtime |

## Implementation Phases

### Phase 0: Script Execution Infrastructure - ✅ COMPLETED

**Goal**: Top-level script execution entry points with linear semantics.

**Status**: Completed with basic literal evaluation working

**Tasks**:
1. ✅ Create `crates/datalove-datafun/src/interp/` module
2. ✅ Define `InterpContext` with script and package world support
3. ✅ Define `ScriptScope` with `ScriptVariable` and move tracking
4. ✅ Implement `execute_script()` for batch mode
5. ✅ Implement `execute_script_unit()` for REPL mode
6. ✅ Implement script statement execution (Let, Function definitions)
7. ✅ Implement `read_script_variable()` with copy/move logic
8. ✅ Wire up with ScriptWithPackageWorld
9. ✅ Implement basic literal evaluation (u32, bool, string)
10. ✅ Fix runtime lifetime management

**What was implemented**:
- Created `crates/datalove-datafun/src/interp/mod.rs` with core infrastructure
- Defined complete data structures:
  - `InterpContext<'db>`: interpreter state with db, runtime, package_world, script, script_scope, and tydesc_table
  - `ScriptScope<'db>`: variables (HashMap with move tracking) and functions
  - `ScriptVariable`: value, state (Available/Moved), and is_copy flag
  - `ScriptVarState`: enum for Available/Moved
  - `Value`: Copy struct with ptr and tydesc
  - `ScriptResult`: bundles Value with Runtime to keep memory alive
  - `InterpError`: comprehensive error types
- Implemented execution functions:
  - `execute_script()`: batch mode entry point, returns ScriptResult
  - `execute_script_unit()`: REPL mode entry point
  - `execute_unit()`: per-unit execution
  - `execute_statement()`: statement dispatcher
  - `execute_let_statement()`: variable binding with move tracking
  - `execute_fun_statement()`: function definition registration
- Implemented expression evaluation:
  - `eval_expression_in_script_scope()`: expression dispatcher
  - `read_script_variable()`: enforces linear semantics (use-after-move detection)
  - `clone_value()`: clones values using runtime
  - `eval_datalit_expression()`: evaluates literals
  - `allocate_bool()`: allocates boolean values
  - `allocate_int()`: allocates u32 integers
  - `allocate_string()`: allocates string values
- Fixed critical bugs:
  - **TyDescTable dangling pointer bug**: Fixed in `crates/datalove-datalit/src/tydesc_table.rs`
    - `get_or_create()`, `get_or_create_tuple()`, `create_option_from_inner_tydesc()`, `create_result_from_inner_tydesc()`
    - Was taking pointer before pushing to vector, creating dangling pointers
    - Fixed by pushing first, then getting pointer from stored element
  - **Runtime lifetime bug**: Fixed in `crates/datalove-datafun/src/interp/mod.rs`
    - Runtime was being dropped when `execute_script()` returned, freeing all allocated memory
    - Created `ScriptResult` struct that bundles `Value` with `Runtime`
    - Keeps runtime alive as long as value is used
- Added integration tests in `crates/datalove-datafun/tests/interp_tests.rs`:
  - `test_interp_empty_script`: empty script returns NoOutputVariable error
  - `test_interp_function_definition`: can define functions at script level
  - `test_interp_u32_literal`: can evaluate u32 literals
  - `test_interp_bool_literals`: can evaluate true/false literals
  - `test_interp_string_literal`: can evaluate string literals
- All tests passing: 5 new interp_tests, 136 existing datafun tests

**Success criteria**:
- ✅ Can execute simple scripts with literal evaluation
- ✅ Can define functions at script level
- ✅ Linear types move on use (use-after-move detected)
- ✅ Can accumulate state in REPL mode - infrastructure ready
- ✅ Basic literals working (u32, bool, string)
- ✅ No segfaults or crashes
- ⏸️ Copy types behavior - needs type analysis integration
- ⏸️ Package world modules - deferred until more expressions work
- ⏸️ Full expression evaluation - only literals done

**Deviations from plan**:
- PackageWorld integration temporarily disabled (typechecking deferred)
- Most expression types not yet implemented (BinOp, FunctionCall, Tuple, etc.)
- Copy type detection not yet implemented (always assumes non-copy for now)
- Value representation simplified (no ValueLocation enum yet)

**Next**: Phase 2 (Expression Evaluation) - Implement remaining expression types before moving to frames.

### Phase 1: Core Infrastructure
**Goal**: Basic interpreter shell with frame management for function execution.

**Tasks**:
1. Define `StackFrame`, `Value`, `SlotState` types
2. Implement frame allocation from analysis
3. Implement slot read/write operations
4. Implement slot state tracking (Uninitialized/Initialized/Moved)
5. Write unit tests for frame management

**Success criteria**:
- Can allocate frames with correct size
- Can read/write values to slots at correct offsets
- Slot states track initialization correctly
- No memory leaks in frame allocation tests

### Phase 2: Expression Evaluation - ✅ COMPLETED (literals, binary ops, and zero-arg function calls)

**Goal**: Evaluate expressions including zero-argument function calls.

**Status**: Completed literals, binary operations, and zero-argument function calls with automatic type widening

**Tasks**:
1. ✅ Implement literal evaluation
2. ✅ Implement variable reference (Name expressions) - in script and function scope
3. ✅ Implement BinOp evaluation (arithmetic with automatic u32→Int widening)
4. ✅ Implement zero-argument function calls
5. ✅ Implement basic copy type detection (u32, Bool)
6. ⏸️ Implement Tuple construction - deferred
7. ✅ Add tests for expression evaluation

**What was implemented**:
- Binary operation infrastructure:
  - `execute_binop()`: dispatcher for binary operations
  - `eval_add()`, `eval_sub()`, `eval_mul()`, `eval_div()`: arithmetic with automatic widening
  - Handles u32+u32, Int+Int, u32+Int, Int+u32 (all cases)
  - Automatic widening: u32 operands converted to Int before operation
  - Results are always Int (bigint) for bare operators
- Type checking helpers:
  - `is_u32_value()`: check if value is u32 type
  - `is_int_value()`: check if value is Int (bigint) type
- Value allocation:
  - `allocate_bigint()`: allocates Int values
  - `widen_u32_to_int()`: converts u32 to Int with proper limb allocation
- Memory management fixes:
  - `destroy_value()`: properly destroys both contents AND allocation
  - `ScriptResult` includes `tydesc_table` to keep type descriptors alive
  - `ScriptResult::Drop`: cleans up output value before runtime shutdown
  - Destroy original u32 values after widening in all arithmetic operations
  - Fixed all 8 mixed-type branches (u32-Int and Int-u32) in add/sub/mul/div
- Function call infrastructure:
  - `eval_function_call()`: looks up function in script scope, validates zero arguments
  - `execute_function_body()`: executes function statements with local variable scope
  - `execute_function_statement()`: handles let, ret statements in functions
  - `eval_expression_in_function_scope()`: evaluates expressions with access to local and script variables
  - `cleanup_local_variables()`: properly destroys local variables on return
  - FunctionReturn error type: used to propagate return values up the call stack
- Copy type detection:
  - `is_copy_type()`: checks if value is u32 or Bool (copy types)
  - Int and String are linear types (move on use)
  - Copy types are cloned when read multiple times
  - Linear types are moved on first read, error on subsequent reads
- Test infrastructure:
  - Converted `interp_tests.rs` to example-based testing (like `old_interp_tests.rs`)
  - Created `tests/fixtures/interp2/` directory with 20 test cases
  - `pretty_print_value()`: formats values using runtime pretty printer
  - Expected error handling: NoOutputVariable returns Ok() with error message
- Integration with `crates/datalove-datafun/Cargo.toml`:
  - Added `[[test]]` configuration for `interp_tests`
  - Uses `harness = false` for ExampleTestRunner

**Test results**:
- 20 tests passing with zero memory leaks:
  - 01_empty_script (expected error: NoOutputVariable)
  - 02_function_def (expected error: NoOutputVariable)
  - 03_u32_literal (@42)
  - 04_bool_true (@true)
  - 05_bool_false (@false)
  - 06_string_literal (@"hello")
  - 07_u32_add (@10 + @20 = @30)
  - 08_u32_sub (@50 - @20 = @30)
  - 09_u32_mul (@6 * @7 = @42)
  - 10_u32_div (@84 / @2 = @42)
  - 11_expression_chain (let a = @5 + @10; a * @2 = @30)
  - 12_int_literal (@42 as Int)
  - 13_int_add (@10 + @20 = @30 as Int)
  - 14_int_sub (@50 - @20 = @30 as Int)
  - 15_int_mul (@6 * @7 = @42 as Int)
  - 16_int_div (@84 / @2 = @42 as Int)
  - 17_fun_call_simple (zero-arg function returns u32)
  - 18_fun_call_string (zero-arg function returns String)
  - 19_fun_call_arithmetic (zero-arg function with arithmetic)
  - 20_fun_call_chained (function calling another function with local variables)
- Full test suite: `just test` passes (282+ tests total)

**Success criteria**:
- ✅ Can evaluate literals (u32, bool, string)
- ✅ Can read variables from script and function scope
- ✅ Can perform arithmetic operations with automatic widening
- ✅ Expression chaining works (let bindings + arithmetic)
- ✅ Can call zero-argument functions from script scope
- ✅ Functions can have local variables and return values
- ✅ Recursive function calls work (function calling another function)
- ✅ Basic copy type detection (u32, Bool copy; Int, String linear)
- ✅ Linear semantics enforced (use-after-move detection)
- ✅ Zero memory leaks (verified with DATALOVE_LEAK_CHECK)
- ⏸️ Can construct tuples - not yet implemented
- ⏸️ Functions with parameters - not yet implemented
- ⏸️ Can read variables from slots - deferred until frame implementation

**Next**: Implement function parameters, or implement remaining expression types (comparisons, tuples, try operators), or begin proper frame-based execution (Phase 1).

### Phase 3: Move Semantics
**Goal**: Implement analysis-guided move tracking.

**Tasks**:
1. Implement copy detection (use MoveKind::Copy)
2. Implement move operations (mark slot as Moved)
3. Implement last-use optimization
4. Add clone operations for non-last-use
5. Add tests for move semantics

**Success criteria**:
- Copy types can be used multiple times
- Linear types are moved on use
- Last-use correctly transfers ownership
- Clones are inserted when needed

### Phase 4: Control Flow
**Goal**: Execute CFG-based control flow.

**Tasks**:
1. Implement CFG-based execution loop
2. Implement if-statement branching
3. Implement return statements
4. Implement try operators (early return)
5. Add tests for control flow

**Success criteria**:
- Can execute linear statement sequences
- If-statements branch correctly
- Returns exit function properly
- Try operators trigger early returns

### Phase 5: Drop Execution
**Goal**: Execute drops at analysis-computed points.

**Tasks**:
1. Implement drop execution at exit blocks
2. Implement slot state tracking (prevent double-drop)
3. Handle drops for early returns
4. Skip drops for Reference/Copy/Moved slots
5. Add tests for drop execution

**Success criteria**:
- Drops executed at correct points
- No double-drops
- Reference slots never dropped
- Copy types never dropped
- Memory properly freed

### Phase 6: Function Calls
**Goal**: Support recursive function calls.

**Tasks**:
1. Implement parameter passing (write to Reference slots)
2. Implement frame push/pop
3. Implement return value transfer
4. Add stack overflow protection
5. Add tests for function calls

**Success criteria**:
- Can call functions recursively
- Arguments passed correctly
- Return values transferred properly
- Stack overflow detected

### Phase 7: Module Integration
**Goal**: Full integration with PackageWorld and scripts.

**Tasks**:
1. Implement module resolution via package world
2. Implement function lookup in modules
3. Integrate import demands from script
4. Handle multi-unit scripts (REPL history)
5. Add tests with module system

**Success criteria**:
- Can resolve functions from sys/std modules
- Imports work correctly from script
- Module-scoped execution works
- REPL can accumulate functions across units
- Scripts can execute against package world

### Phase 8: Testing & Validation
**Goal**: Comprehensive test coverage.

**Tasks**:
1. Port old_interp_tests to new interpreter
2. Add new tests for linear semantics
3. Add tests for error cases
4. Benchmark against old interpreter
5. Fix any remaining bugs

**Success criteria**:
- All old interpreter tests pass
- No memory leaks (run with valgrind)
- Performance equivalent or better
- Full linear type semantics enforced

## Testing Strategy

### Unit Tests
- Frame allocation and deallocation
- Slot read/write operations
- Value cloning
- Drop execution
- Slot state transitions

### Integration Tests
```rust
#[test]
fn test_simple_arithmetic() {
    let source = r#"
fun test(): u32
    let x = @42
    let y = @100
    ret x +! y
end fun
    "#;
    assert_eq!(run_script(source), 142);
}

#[test]
fn test_script_level_linear_semantics() {
    let source = r#"
let x = "hello"
let y = x  // Move x
// let z = x would error: use-after-move
    "#;
    assert_eq!(run_script(source), "hello");
}

#[test]
fn test_script_level_copy_semantics() {
    let source = r#"
let x = @42
let y = x  // Copy (u32 is copy type)
let z = x  // Copy again (ok)
let output = y +! z
    "#;
    assert_eq!(run_script(source), 84);
}

#[test]
fn test_linear_move() {
    let source = r#"
fun test(): String
    let x = "hello"
    ret x  // x is moved
end fun
    "#;
    assert_eq!(run_script(source), "hello");
}

#[test]
fn test_copy_multiple_use() {
    let source = r#"
fun test(): u32
    let x = @42
    let y = x  // copy (u32 is copy type)
    let z = x  // copy again (ok)
    ret y +! z
end fun
    "#;
    assert_eq!(run_script(source), 84);
}

#[test]
fn test_drops_executed() {
    // Use leak detection to verify drops.
    let source = r#"
fun test(): u32
    let x = "temp"  // should be dropped
    ret @42
end fun
    "#;
    run_script_with_leak_check(source);
}
```

### Memory Safety Tests
```rust
#[test]
fn test_no_use_after_move() {
    let source = r#"
fun test(): String
    let x = "hello"
    let y = x  // move
    ret y
end fun
    "#;
    // Should succeed (analysis validates this).
    run_script(source);
}

#[test]
fn test_reject_use_after_move() {
    let source = r#"
fun test(): String
    let x = "hello"
    let y = x  // move
    ret x      // ERROR: use after move
end fun
    "#;
    // Should fail analysis.
    assert!(analyze_has_errors(source));
}
```

### Leak Tests
```rust
#[test]
fn test_no_leaks_simple() {
    let source = "fun test(): u32 ret @42 end fun";
    run_with_leak_detection(source);
}

#[test]
fn test_no_leaks_complex() {
    let source = r#"
fun test(): String
    let a = "one"
    let b = "two"
    let c = "three"
    if @true
        ret a
    else
        ret b
    end if
end fun
    "#;
    run_with_leak_detection(source);
}
```

## Script vs Function Execution Boundary

### Key Design Decisions

**1. What gets analyzed?**
- **Functions**: Always analyzed via `analyze_function(db, func)`
- **Script statements**: NOT analyzed (direct interpretation)
- **Expressions in script**: Evaluated in script scope (no frame)

**2. When does function analysis run?**
- **Batch mode**: When function is first called
- **REPL mode**: When function is defined (immediate validation)
- **Lazy**: Analysis is Salsa-tracked, so only recomputed when needed

**3. ScriptUnit awareness:**
- **Parsing**: Per-unit (Salsa memoized per `parse_script_unit`)
- **Typechecking**: Full combined script (not per-unit)
- **Execution**: Per-unit in REPL, full script in batch
- **Analysis**: Per-function (triggered for each function in any unit)

**4. Script-level expressions:**
Script-level expressions (in `let` statements) don't have frames:
- Evaluated directly in script scope
- Can reference script-level variables (following linear semantics)
- Can call functions (which DO use frames)
- Copy types can be used multiple times
- Linear types move on use (or must be cloned)

**5. REPL state persistence with linear semantics:**
```
Unit 1: fun foo(): String ret "hello" end fun
  → Added to ctx.script_scope.functions

Unit 2: let x = foo()
  → Calls foo (creates frame, analyzes foo)
  → Result stored in ctx.script_scope.variables
  → x.state = Available, x.is_copy = false

Unit 3: let y = x
  → Reads x from script_scope.variables
  → String is linear type, so x is MOVED
  → x.state = Moved
  → y.state = Available

Unit 4: let z = x
  → ERROR: x.state == Moved (use-after-move)

--- For copy types: ---

Unit 5: let a = @42
  → a.state = Available, a.is_copy = true

Unit 6: let b = a
  → u32 is copy type, so a is COPIED (not moved)
  → a.state = Available (still usable)
  → b.state = Available

Unit 7: let c = a
  → OK: a.is_copy == true, can use again
```

## Open Questions

### Q1: How to track moves in script-level expressions?

**Decision**: Script variables track move state, enforcing linear semantics.

**Implementation**:
```rust
fn read_script_variable<'db>(
    ctx: &mut InterpContext<'db>,
    name: InternedText<'db>,
) -> Result<Value, InterpError> {
    let var = ctx.script_scope.variables.get_mut(&name)
        .ok_or(InterpError::VariableNotFound(name))?;

    // Check if already moved.
    if var.state == ScriptVarState::Moved {
        return Err(InterpError::UseAfterMove(name));
    }

    if var.is_copy {
        // Copy types: clone the value, keep state Available.
        Ok(clone_value(ctx, var.value))
    } else {
        // Linear types: move the value, mark as Moved.
        var.state = ScriptVarState::Moved;
        Ok(var.value)
    }
}
```

**REPL behavior**:
- Copy types: Can be used multiple times across units
- Linear types: Can only be used once (move on read)
- Explicit clone (future): `let y = x.clone()` to duplicate

**This preserves linear semantics at all levels of the language.**

### Q2: How to handle Reference parameter moves?

**Scenario**: `In` parameters are Reference slots but have move semantics.

**Solution**: When reading from `In` parameter slot:
1. Check move_info to see if this is a move
2. If move, mark slot as Moved (caller can't use it anymore)
3. Value stays in caller's frame (we just reference it)
4. Drop responsibility transfers to callee

### Q2: How to represent values spanning multiple slots?

**Scenario**: Tuple expressions create temporaries that reference other slots.

**Solution**: Values can be composite:
```rust
pub enum ValueLocation {
    FrameSlot(SlotId),      // Single slot
    HeapOwned,              // Heap allocation
    Reference,              // Reference to caller
    Composite(Vec<Value>),  // Tuple of values
}
```

### Q3: How to handle Out parameters?

**Scenario**: Caller allocates slot, callee writes to it.

**Solution**:
1. Caller allocates uninitialized slot
2. Passes pointer as Reference slot to callee
3. Callee writes to pointer (Reference slot)
4. Analysis verifies Out param initialized before return
5. Caller reads initialized value after call

### Q4: Should we eliminate frame_data Vec for Copy types?

**Decision**: No, keep uniform treatment.
- Analysis already identifies Copy types
- Uniform representation simplifies implementation
- Optimization can come later (inline scalars in SlotState)

### Q5: How do ScriptUnits interact with incremental compilation?

**Answer**:
- Parsing is incremental (Salsa memoizes `parse_script_unit` per unit)
- Typechecking is NOT incremental (full script re-typechecked)
- Execution can be incremental (REPL only executes new unit)
- Analysis is incremental (Salsa memoizes per function)

**Optimization opportunity**: Could cache typecheck results per active unit set.

### Q6: Can script statements use analysis framework?

**Problem**: Script-level statements aren't in functions, so no frame analysis.

**Answer**: Partial - script execution doesn't use full analysis:
- No packed frames (use HashMap for script scope)
- Move tracking via ScriptVarState (manual, not analysis-driven)
- No drops (script scope lives for entire session)
- Functions within script DO get full analysis
- Copy detection reused from function analysis framework

### Q7: How to integrate with existing tests?

**Strategy**: Parallel implementation
- Keep old interpreter as `interp_old`
- New interpreter in `interp`
- Duplicate test suites initially
- Gradually migrate tests
- Remove old interpreter when complete

## Migration Path

### Step 1: Implement in Parallel - ✅ COMPLETED
- ✅ Create `crates/datalove-datafun/src/interp/` (new)
- Keep `crates/datalove-datafun/src/interp_old/` (existing)
- New CLI flag: `--interp-version=old|new`

### Step 2: Feature Parity
- Implement all phases 1-7
- Pass equivalent test suite
- Verify memory safety (valgrind)

### Step 3: Deprecation
- Make new interpreter default
- Mark old interpreter as deprecated
- Keep for comparison/fallback

### Step 4: Removal
- Remove old interpreter
- Remove compatibility layer
- Clean up unused code

## Success Criteria

Implementation complete when:
- ✓ All expression types supported
- ✓ All statement types supported
- ✓ Move semantics enforced via analysis
- ✓ Copy semantics work correctly
- ✓ Drops execute at correct points
- ✓ No memory leaks (valgrind clean)
- ✓ No double-frees
- ✓ Analysis errors prevent execution
- ✓ Module resolution works
- ✓ Function calls work recursively
- ✓ Old test suite passes
- ✓ Performance >= old interpreter
- ✓ Zero unsafe blocks (except FFI boundary)

## Implementation Notes

### Files Created (Phases 0-2)

**New interpreter module:**
- `crates/datalove-datafun/src/interp/mod.rs` - Core interpreter implementation (~1200 lines)
  - Data structures: InterpContext, ScriptScope, ScriptVariable, ScriptVarState, Value, ScriptResult, InterpError
  - Entry points: execute_script(), execute_script_unit()
  - Execution: execute_unit(), execute_statement(), execute_let_statement(), execute_fun_statement()
  - Expression evaluation: eval_expression_in_script_scope(), read_script_variable(), clone_value(), eval_datalit_expression()
  - Binary operations: execute_binop(), eval_add(), eval_sub(), eval_mul(), eval_div()
  - Type helpers: is_u32_value(), is_int_value()
  - Value operations: allocate_bool(), allocate_int(), allocate_bigint(), allocate_string(), widen_u32_to_int(), destroy_value()
  - Output formatting: pretty_print_value()

**Test files:**
- `crates/datalove-datafun/tests/interp_tests.rs` - Example-based integration tests (~50 lines)
- `crates/datalove-datafun/tests/fixtures/interp2/` - Test fixtures directory
  - 11 `.dfs` script files
  - 11 `.out.expected` expected output files

**Modified files:**
- `crates/datalove-datafun/src/lib.rs` - Added `pub mod interp;` declaration
- `crates/datalove-datafun/Cargo.toml` - Added `[[test]]` configuration for interp_tests

### Current State

**Working:**
- Module structure and data types
- Script parsing and unit iteration
- Function definition registration
- Variable binding infrastructure
- Linear semantics enforcement (use-after-move detection)
- REPL state accumulation infrastructure
- Literal evaluation (u32, bool, string)
- Binary arithmetic operations (+, -, *, /) with automatic u32→Int widening
- Expression chaining (let bindings + arithmetic)
- Runtime value allocation (allocate_bool, allocate_int, allocate_bigint, allocate_string)
- Type widening (widen_u32_to_int)
- Value cloning using runtime
- Value destruction (destroy_value) - both contents and allocation
- TyDescTable type descriptor management
- Runtime lifetime management via ScriptResult (includes tydesc_table)
- Pretty-printing values for test output
- Example-based testing infrastructure

**Not yet implemented:**
- Remaining expression types (Comparison, FunctionCall, Tuple, UnaryOp, Try operators)
- PackageWorld integration (temporarily disabled)
- Type analysis integration for copy detection
- Function execution with stack frames
- CFG-based control flow
- Drop insertion

**Test results:**
- All existing tests pass (136 datafun tests + 93 datalit tests + others)
- 11 new interp_tests pass with zero memory leaks:
  - 01_empty_script
  - 02_function_def
  - 03_u32_literal
  - 04_bool_true
  - 05_bool_false
  - 06_string_literal
  - 07_u32_add
  - 08_u32_sub
  - 09_u32_mul
  - 10_u32_div
  - 11_expression_chain
- Full test suite: `just test` passes (282+ tests total)

## References

- `notes/oldplans/plan-function-analysis.md` - Analysis framework design
- `notes/oldplans/plan-copy.md` - Copy type detection
- `notes/oldplans/plan-rt-refactor.md` - Runtime API refactoring
- `notes/module-system.md` - Package world documentation
- `crates/datalove-datafun/src/function_analysis/` - Analysis implementation
- `crates/datalove-datafun/src/interp_old/` - Old interpreter (reference)
- `crates/datalove-datafun/src/interp/` - New interpreter (in progress)
