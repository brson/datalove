# Script Interpreter Rewrite: Frame-Based Execution

## Problem Statement

The current script interpreter is "hacked together" with unclear semantics:
- Variables stored in `HashMap<InternedText, ScriptVariable>`
- Expressions evaluated with `Option<Destination>` - allocates on demand
- No pre-analysis phase, no CFG, no frame layout
- DPS conversion blocked because no pre-allocated destinations

The function interpreter is clean:
- Pre-analysis via `FunctionAnalysis` (slots, CFG, drop points, layout)
- `StackFrame` is a packed byte buffer with pre-allocated slots
- Full DPS: every expression has a `Destination`
- CFG-based control flow

## Design Decisions

**Forward-only semantics (initial implementation):**
- Declarations and statements only affect subsequent code
- `require`/`import`/`fun` only visible after defined
- `let` bindings only visible to following statements
- Simpler, matches traditional REPL expectations

**Future: Bidirectional/reactive mode** (not in scope for now):
- Declarations could affect earlier code (like modules)
- Would fit Salsa's reactive model
- Complex, potentially confusing for users

## Current Status (December 2025)

**Script interpreter is gutted.** The REPL engine currently has:
```rust
fn eval_script_statement(&mut self, _source: String) -> Eval {
    todo!("script interpreter gutted - pending frame-based rewrite")
}
```

**DPS cleanup is complete.** All expressions now use mandatory `Destination`,
not `Option<Destination>`. The code explicitly marks the old script scope as removed.

**Function interpreter is the reference implementation.** The frame-based
execution with FunctionAnalysis, StackFrame, CFG, and DropPoints is complete
and working. This serves as the template for the script rewrite.

**Remaining work:** Phases 1-4 below (ScriptUnitAnalysis, ScriptUnitFrame,
name lookup, CFG execution).

## Proposed Architecture

**Per-unit analysis and frames, semantically contiguous.**

Each script unit is analyzed and executed independently, but name lookup
searches backward through all frames (forward-only visibility).

```
InterpContext
├── script_frames: Vec<ScriptUnitFrame>  // one frame per unit, persistent
├── script_analyses: Vec<ScriptUnitAnalysis>  // cached, one per unit
└── call_stack: Vec<FunctionFrame>       // transient, push/pop on calls
```

**Key insight:** CFG never jumps between units. Loops/ifs must be syntactically
complete within a single unit. Units execute sequentially; within each unit,
CFG handles control flow.

### Script Structure

Scripts are like modules interleaved with executable statements:

**Declarations (module-like):**
- `fun` - function definitions
- `require` - module imports
- `import` - item imports

**Executable statements (script-like):**
- `let` - variable bindings
- `if`/`loop`/`break`/`continue` - control flow (to be supported)
- Bare expressions - evaluate and keep

### Scoping Rules

**Functions are isolated:**
- See only their parameters and locals
- Can call other functions (script-defined or imported)
- CANNOT see script `let` bindings

**Script statements:**
- See prior `let` bindings (forward-only)
- See prior `fun`/`import` declarations (forward-only)
- CFG handles control flow (`if`/`loop`)

### Drop Timing

- End of script (batch mode)
- End of REPL session (interactive mode)
- Scope boundaries within script (loops, if blocks)
- NOT at unit boundaries (units are organizational only)
- Cross-unit moves: receiver owns and drops; source slot marked Moved

## Implementation Approach

### Phase 1: ScriptUnitAnalysis

Create per-unit analysis (parallel to `FunctionAnalysis`):

```rust
#[salsa::tracked]
pub struct ScriptUnitAnalysis<'db> {
    pub script: Script,
    pub unit_index: usize,
    pub frame_layout: FrameLayout<'db>,
    pub slot_allocation: SlotAllocation<'db>,
    pub control_flow: ControlFlowGraph<'db>,  // per-unit CFG
    pub drop_points: DropPoints<'db>,
    #[returns(ref)]
    pub tracked_slots: Vec<SlotId>,
}

/// Context passed from prior units for forward-only name resolution.
#[salsa::tracked]
pub struct ScriptUnitContext<'db> {
    /// Available variable names and their types from prior units.
    #[returns(ref)]
    pub available_vars: Vec<(InternedText<'db>, TypeAndHeap<'db>, usize)>,  // (name, type, frame_index)
    /// Available function names from prior units.
    #[returns(ref)]
    pub available_funs: Vec<InternedText<'db>>,
}
```

Reuse existing analysis infrastructure:
- `slot_allocation.rs` - `let` → Local, temps → Temporary
- `cfg.rs` - build CFG for unit body
- `drops.rs` - compute drop points
- `layout.rs` - compute frame layout

### Phase 2: ScriptUnitFrame

Per-unit frame:

```rust
pub struct ScriptUnitFrame<'db> {
    pub frame_data: Vec<u8>,           // packed byte buffer
    pub slot_states: Vec<SlotState>,   // per-slot move tracking
    pub layout: FrameLayout<'db>,
    pub cfg: ControlFlowGraph<'db>,
    pub drop_points: DropPoints<'db>,
    pub unit_index: usize,
}
```

### Phase 3: Script Name Lookup

Search frames backward (newest to oldest):

```rust
fn find_script_variable<'db>(
    ctx: &InterpContext<'db>,
    name: InternedText<'db>,
) -> Option<(usize, SlotInfo<'db>)> {  // (frame_index, slot)
    for (i, frame) in ctx.script_frames.iter().enumerate().rev() {
        if let Some(slot) = find_slot_by_name(ctx.db, frame.layout, name) {
            return Some((i, slot));
        }
    }
    None
}
```

Function name lookup remains isolated (function frame only).

Functions look up callees via:
- Script-defined functions: `ctx.script_functions`
- Imported functions: `ctx.module_functions_graph`

### Phase 4: Per-Unit CFG Execution

Execute one unit at a time:

```rust
fn execute_script_unit<'db>(
    ctx: &mut InterpContext<'db>,
    unit_index: usize,
) -> Result<(), InterpError> {
    let analysis = &ctx.script_analyses[unit_index];
    let frame = &mut ctx.script_frames[unit_index];

    let mut current_block = BlockId(0);
    loop {
        let block = analysis.control_flow.get_block(ctx.db, current_block)?;

        for stmt in &block.statements {
            execute_script_statement(ctx, unit_index, stmt)?;
        }

        match &block.terminator {
            Terminator::ScriptEnd => break,  // end of unit
            Terminator::Branch { .. } => { /* evaluate, pick branch */ }
            Terminator::Goto(next) => { current_block = *next; }
            // etc. (no Return - that's function-only)
        }
    }
    Ok(())
}
```

### Phase 5: Full DPS for Script Expressions ✅ COMPLETE

DPS is now mandatory everywhere. All expression evaluation uses:

```rust
fn eval_expression_frame<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ast::ExprFun<'db>,
    dest: Destination,  // required, not optional
) -> Result<(), InterpError>
```

Scripts will reuse the same `eval_expression_frame` function.

### Phase 6: Migration ✅ MOSTLY COMPLETE

1. ~~Build new script interpreter in parallel with old~~
2. ~~Add tests for parity~~
3. ~~Switch over~~
4. ~~Remove old `ScriptScope`-based code~~ - Script scope removed
5. ~~Clean up `Option<Destination>` → require `Destination` everywhere~~ - Done

The script interpreter is currently gutted; implementing Phases 1-4 will
complete the migration.

## Files Involved

**To create:**
- `script_analysis/mod.rs` - orchestrate script analysis
- (or extend existing `function_analysis/` to handle scripts)

**To modify:**
- `interp/script.rs` - rewrite with frame-based execution (currently gutted)
- `interp/context.rs` - add `script_frames` and `script_analyses` vectors

**Already done:**
- `interp/mod.rs` - DPS with mandatory `Destination` is complete

**Reference (existing frame implementation):**
- `interp/mod.rs` - `eval_expression_frame`, `execute_function_body_with_frame`
- `interp/frame.rs` - `StackFrame` structure
- `function_analysis/*.rs` - analysis passes

## Resolved Questions

1. **Per-unit vs whole-script frames**: Per-unit. Avoids re-typechecking entire script.

2. **Cross-unit CFG jumps**: None. Loops/ifs must be complete within a unit.

3. **Forward vs bidirectional**: Forward-only for initial implementation.

4. **Bare expressions**: Evaluate to temp and keep. REPL communication TBD.

5. **Cross-unit drops**: Unit that receives value owns it and drops if appropriate.
   Source unit's slot marked Moved, skipped at cleanup.

## Open Questions

1. **Script-level `if`/`loop`**: Currently errors at script level. Need to enable
   in parser/tycheck before interpreter can handle them.
