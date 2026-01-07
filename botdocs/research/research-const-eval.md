# Const Evaluation Design Brainstorm

Research notes on implementing const/comptime function evaluation for Datafun.

## Enabling Properties

Datafun's type system provides properties that make const evaluation straightforward:

1. **Pure functions** - No side effects (except I/O which const eval doesn't allow)
2. **Linear types** - Predictable, deterministic memory management
3. **No panics** - Functions always return or early-return with error
4. **No pointer types** - Values are self-contained, no aliasing concerns
5. **Termination potential** - Loop induction variables enable termination analysis
6. **Interpreter/AOT parity** - Existing interpreter can execute const functions

## Architecture Overview

```
Source → Parser → AST → Tycheck → IR → Const Eval (Interpreter) → ConstValue
                                      ↓
                              AOT/Interp (runtime)
```

Const evaluation happens after IR lowering, before final code generation.
The interpreter already executes IR; const eval reuses it with constraints.

## Proposed Design

### Const Contexts

Two places where const evaluation applies:

1. **Const declarations** - New statement type:
   ```
   const PI: f32 = @3.14159
   const MAX_SIZE: u32 = compute_max()
   ```

2. **Comptime arguments** - Function arguments marked `comptime`:
   ```
   fun make_buffer(comptime size: u32): [@u8]
       ...
   end fun
   ```

### Constness Analysis

**All Datafun functions are const by construction.** There is no I/O in the language - all functions are pure and side-effect free. The only constraint for const evaluation is **termination**.

No special constness analysis pass is needed. Any function can be const-evaluated if:
- It terminates (either provable via loop analysis, or enforced via step limit)

All IR operations are const-evaluable:
- Const literals, binary/unary ops, pack/unpack
- Option/Result construction, collection construction
- Function calls (recursive const eval)
- SlotStore/SlotLoad (local mutation within pure function)
- DebugLog can be ignored or collected during const eval

### Implementation Strategy

**Phase 1: Basic Const Eval**
- Create `ConstEvaluator` struct wrapping `IrInterpreter`
- Execute const functions in isolated environment
- Convert results back to `ConstValue` for IR emission

**Phase 2: Const Declarations**
- Add `const` statement to parser and AST
- Evaluate const declarations during compilation
- Substitute const values at use sites

**Phase 3: Comptime Parameters**
- Add `comptime` parameter mode
- Specialize functions at call sites with known const args
- Monomorphization of comptime-parameterized functions

### Const Evaluator

```rust
pub struct ConstEvaluator {
    interp: IrInterpreter,
    max_steps: usize,      // Limit execution to prevent infinite loops
    max_memory: usize,     // Limit memory for const eval
}

impl ConstEvaluator {
    pub fn evaluate_function(
        &mut self,
        func: &IrFunction,
        args: Vec<ConstValue>,
    ) -> Result<ConstValue, ConstEvalError>;
}

pub enum ConstEvalError {
    StepLimitExceeded,
    MemoryLimitExceeded,
    NonConstOperation(String),
    EarlyReturn(Error),
}
```

### Result Caching

Store const-eval results in the Salsa database:

```rust
#[salsa::tracked]
fn const_eval_function(
    db: &dyn Database,
    func: FuncId,
    args: Vec<ConstValue>,
) -> Result<ConstValue, ConstEvalError>;
```

Salsa's memoization prevents re-evaluation of the same const calls.

### Termination Guarantees

Loop analysis already tracks carry/bring values. Extend to detect:
- Monotonic progress (induction variable changes each iteration)
- Bounded iteration (known upper bound)
- Guaranteed break (all paths lead to break or return)

For const eval, require provable termination or impose step limit.

## Considerations

### Memory Management

The interpreter uses the runtime for memory management. Const evaluation should:
- Use a separate "const heap" that's freed after evaluation
- Convert heap values to serializable `ConstValue` before returning
- Limit total allocation size

### Error Handling

Const functions can early-return with errors (`!` operator):
- Early returns become compile-time errors
- Clear error messages showing const-eval call stack

### Recursion

Allow recursive const functions with:
- Stack depth limit
- Total step count limit
- Memoization of recursive calls

### Cross-Module Const

Const functions from imported modules:
- IR already serializable (`Serialize`/`Deserialize` derived)
- Load module IR, evaluate const functions
- Cache results per-module

## Example Usage

```datafun
// Const function computing factorial
fun factorial_const(n: u32): u32
    loop carry (acc: u32 = @1, i = n)
        if i <= @1
            break(acc)
        end if
        continue(acc * i, i - @1)
    end loop bring (result: u32)
    ret result
end fun

// Const declaration using const function
const FACTORIAL_10: u32 = factorial_const(@10)

// Comptime-parameterized function
fun make_zeroes(comptime n: u32): [@u32]
    // n is known at compile time, can be used for optimizations
    ...
end fun

// Call with const argument - specialized at compile time
let zeros = make_zeroes(@100)
```

## Next Steps

1. Add step-limited evaluation wrapper around interpreter
2. Design const declaration syntax and parsing
3. Implement basic const evaluation pipeline
4. Add comptime parameter mode
