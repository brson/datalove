# Comptime Arguments: Research & Design

## Overview

This document analyzes the feasibility of adding Zig-style comptime arguments to datalove,
using `const` as the argument modifier. Comptime arguments are function parameters whose
values must be known at compile time, enabling function specialization based on those values.

```
fun repeat(const n: i32, s: string) -> string
    let result = ""
    let i = 0
    loop while i .< n
        set result = result + s
        set i = i + 1
    end loop
    result
end fun

// At call site, n must be a compile-time constant:
let x = repeat(3, "ab")  // specializes to repeat_n3
```

## Current Architecture Analysis

### Relevant Components

| Component | Location | Role |
|-----------|----------|------|
| AST ParamMode | `datafun-ast/src/ast.rs:158-165` | In, Out, Ref, Mut enum |
| Type function | `datafun-common/src/lib.rs:215-223` | TypeFunction with param_types, param_modes |
| IR ParamMode | `datafun-ir/src/lib.rs:686-697` | Mirrors AST enum |
| IrFunction | `datafun-ir/src/lib.rs:1221-1260` | params, param_modes, param_types parallel arrays |
| CTFE evaluator | `datafun-interp/src/ctfe.rs` | Executes IR at compile time |
| Const inlining | `datafun-const/src/inline.rs` | Replaces const bindings with literal values |
| Call synthesis | `datafun-tycheck/src/synthesize.rs:689-729` | Type checks function calls |
| Call lowering | `datafun-lower/src/expr.rs:67-120` | Lowers call arguments by mode |

### Current CTFE Capabilities

The existing CTFE system is robust:

1. **Evaluation**: `CtfeEvaluator` trait with `InterpCtfeEvaluator` implementation
2. **Supported types**: All primitives, aggregates, collections, Option/Result
3. **Function calls**: Const expressions can call functions (cross-module supported)
4. **Gas limits**: Prevents infinite loops during compile-time evaluation
5. **3-phase pipeline**: Collection → Evaluation → Inlining (allows memoization)

### Current Argument Handling

Parameters flow through the pipeline as:

```
AST (FunParam with mode: ParamMode)
    ↓
Typecheck (TypeFunction with param_modes: Vec<ParamMode>)
    ↓
IR (IrFunction with param_modes: Vec<ParamMode>)
    ↓
Codegen (all args passed as pointers, mode affects ownership)
```

## Design Options

### Option A: ParamMode::Const

Add `Const` to the existing ParamMode enum:

```rust
pub enum ParamMode {
    In,     // by-val (default)
    Out,    // by-mut-ptr
    Ref,    // by-ref
    Mut,    // by-mut-ref
    Const,  // compile-time value
}
```

**Pros**: Minimal structural changes, fits existing pattern.
**Cons**: Conflates compile-time evaluation with runtime passing semantics.

### Option B: Separate `is_const` Flag (Recommended)

Add a separate flag orthogonal to ParamMode:

```rust
pub struct FunParam<'db> {
    pub name: InternedText<'db>,
    pub mode: ParamMode,        // In, Out, Ref, Mut (runtime semantics)
    pub is_comptime: bool,      // compile-time evaluation required
    pub type_hint: TypeHint<'db>,
}
```

**Pros**:
- Clear separation: `mode` = runtime passing, `is_comptime` = evaluation time
- Allows combinations: `const ref x: T` (compile-time known reference)
- Path to type parameters: `const T: type` uses same flag

**Cons**: Two dimensions to track instead of one.

### Recommendation

**Option B** better serves the stated priorities:
- **Simplicity**: Each flag has one meaning
- **Separation of concerns**: Runtime vs compile-time are orthogonal
- **Future path**: Types as comptime values use same mechanism

## Specialization Strategy

### Whole-Program Advantage

As a whole-program compiler, datalove can:
1. See all call sites before codegen
2. Enumerate all unique comptime argument combinations
3. Generate exactly the needed specializations
4. No need for on-demand instantiation machinery

### Specialization Points

The specialization pass should occur **after type checking, before IR lowering**:

```
Parse → Resolve → Typecheck → [SPECIALIZE] → Lower → Assemble
                                   ↓
                         Collect comptime calls
                         Group by (func, const_args)
                         Generate specialized ASTs
```

This location:
- Has full type information (can verify const-ness)
- Can modify function list before lowering
- Keeps lowering pass unchanged (it just sees more functions)
- Allows Salsa memoization of specializations

### Specialization Algorithm

```
1. COLLECT PHASE (per module)
   For each function call in typechecked AST:
     If callee has comptime params:
       Extract const argument values
       Record (callee, const_values) → call_site

2. GROUP PHASE (whole program)
   For each (func, const_args) combination:
     Generate unique specialization key: "func_name$$const1_const2"
     Create specialized function entry

3. GENERATE PHASE (per specialization)
   Clone function AST
   Replace comptime params with local const bindings
   Remove comptime params from signature
   Add to module's function list

4. REWRITE PHASE (per module)
   Replace calls to generic functions with specialized versions
```

### Example Transformation

Input:
```
fun format_width(const width: i32, s: string) -> string
    // ... use width ...
end fun

let a = format_width(10, "hi")
let b = format_width(20, "hello")
```

After specialization:
```
fun format_width$$10(s: string) -> string
    const width = 10
    // ... use width ...
end fun

fun format_width$$20(s: string) -> string
    const width = 20
    // ... use width ...
end fun

let a = format_width$$10("hi")
let b = format_width$$20("hello")
```

## Implementation Plan

### Phase 1: Parsing & AST (Minimal)

**Files to modify:**
- `datafun-ast/src/ast.rs`: Add `is_comptime: bool` to FunParam
- `datafun-parser/src/statement.rs`: Parse `const` keyword before param name

**Syntax:**
```
fun example(const n: i32, ref data: T) -> R
```

**Parser changes (~20 lines):**
```rust
// In parse_param():
let is_comptime = if self.check(Keyword::Const) {
    self.advance();
    true
} else {
    false
};
// ... existing mode parsing ...
```

### Phase 2: Type Representation (Minimal)

**Files to modify:**
- `datafun-common/src/lib.rs`: Add `param_comptime: Vec<bool>` to TypeFunction

```rust
#[salsa::tracked]
pub struct TypeFunction<'db> {
    pub param_types: Vec<Type<'db>>,
    pub param_modes: Vec<ParamMode>,
    pub param_comptime: Vec<bool>,  // NEW
    pub return_type: Type<'db>,
}
```

### Phase 3: Type Checking (Moderate)

**Files to modify:**
- `datafun-tycheck/src/synthesize.rs`: Validate comptime args are const expressions
- `datafun-tycheck/src/context.rs`: Track comptime arg values for specialization

**Key validation:**
```rust
fn check_comptime_arg(arg: ExprFun, expected_type: &Type) -> Result<ConstValue, TypeError> {
    // Use existing const evaluation machinery
    // Return error if not evaluable at compile time
}
```

### Phase 4: Specialization Pass (Core Work)

**New files:**
- `datafun-specialize/src/lib.rs`: New crate for specialization logic
- `datafun-specialize/src/collect.rs`: Collect comptime call sites
- `datafun-specialize/src/generate.rs`: Generate specialized functions

**Key types:**
```rust
/// Unique identifier for a specialization
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct SpecializationKey {
    pub base_func: (ModuleId, InternedText),
    pub const_args: Vec<ConstValue>,
}

/// Result of specialization analysis
pub struct SpecializationPlan {
    /// Map from generic function to its specializations
    pub specializations: HashMap<FuncId, Vec<SpecializationKey>>,
    /// Map from call site to specialized function
    pub call_rewrites: HashMap<CallSiteId, FuncId>,
}
```

**Integration point in compiler:**
```rust
// In compile_modules() or lower_module_graph():
let typecheck_result = typecheck_module_graph(...);
let spec_plan = specialize_module_graph(db, typecheck_result); // NEW
let lowering_result = lower_module_graph_with_spec(db, typecheck_result, spec_plan);
```

### Phase 5: IR & Lowering (Minimal)

The lowering pass should remain largely unchanged:
- Specialized functions are just regular functions after specialization
- Comptime params become local const bindings in the specialized body
- Call sites just call the specialized function by name

**Minor changes:**
- `datafun-ir/src/lib.rs`: Maybe add `is_comptime` to IR ParamMode if needed
- `datafun-lower/src/lib.rs`: Skip comptime params when lowering specialized functions

## Compile-Time Considerations

### Cost Model

For a scripting language prioritizing compile speed:

| Operation | Cost |
|-----------|------|
| Parse `const` keyword | Negligible |
| Track comptime flag | Negligible |
| Detect const arguments | O(1) per call site |
| Evaluate comptime args | Reuse existing CTFE |
| Generate specializations | O(specializations × function_size) |
| Clone AST for specialization | Moderate - can optimize |

### Optimizations for Compile Speed

1. **Lazy specialization key hashing**: Only hash when grouping
2. **AST sharing**: Specialized functions share parsed sub-expressions
3. **Memoization via Salsa**: Same const args → cached specialization
4. **Incremental**: Only re-specialize changed functions

### Specialization Explosion Prevention

Limit specialization to avoid compile-time blowup:

1. **Depth limit**: Max 3 levels of nested comptime calls
2. **Instance limit**: Max 100 specializations per function
3. **Size limit**: Max 10 comptime params per function
4. **Error on exceeded**: Clear error message, not silent degradation

## Future: Type Parameters

The `const` mechanism naturally extends to type parameters:

```
fun identity(const T: type, x: T) -> T
    x
end fun
```

When `T: type` is comptime:
- Call site provides concrete type
- Specialization substitutes type throughout function
- No runtime type representation needed

This requires:
- `IrType` values as comptime constants
- Type substitution in specialization
- But the comptime machinery is the same

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Specialization explosion | Medium | High | Instance limits, clear errors |
| Compile time regression | Medium | Medium | Benchmark suite, lazy evaluation |
| Complex interaction with CTFE | Low | Medium | CTFE already handles function calls |
| Breaking existing code | Very Low | High | `const` is new syntax, opt-in |

## Recommended Implementation Order

1. **Phase 1-2** (1 day): Parsing and type representation
2. **Phase 3** (2 days): Type checking with validation
3. **Phase 4** (3-5 days): Specialization pass (core work)
4. **Phase 5** (1 day): IR adjustments and testing
5. **Polish** (2 days): Error messages, edge cases, docs

Total estimate: 9-11 days for basic implementation.

## Alternatives Considered

### Alternative: No Specialization (Just Inlining)

Always inline comptime-param functions at call sites.

**Pros**: Simpler, no new pass.
**Cons**: Code bloat, poor cache behavior, loses function identity.

### Alternative: Runtime Specialization Cache

Specialize lazily at runtime, cache results.

**Pros**: No compile-time cost.
**Cons**: Violates compile-time priority, complex runtime, JIT territory.

### Alternative: Type-Directed Specialization Only

Only specialize when comptime args affect types (like Rust).

**Pros**: Fewer specializations.
**Cons**: Less powerful, can't optimize on values like loop bounds.

## Conclusion

Comptime arguments are feasible with moderate implementation effort. The existing CTFE
infrastructure provides the foundation. Key design decisions:

1. **Separate `is_comptime` flag** - cleanest separation of concerns
2. **Post-typecheck specialization pass** - leverages existing pipeline
3. **Whole-program enumeration** - exploits compiler's global view
4. **Instance limits** - prevents compile-time explosion

The implementation fits well with the stated priorities of simplicity, separation of
concerns, and compile-time performance (since most work is bounded AST manipulation).
