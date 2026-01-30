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

## Union-Branch Specialization: Literature & Landscape

This section analyzes the **union-branch approach** as an alternative to full monomorphization,
placing it within the broader PL literature on generics implementation.

### The Generics Implementation Spectrum

The literature identifies three major strategies for implementing parametric polymorphism:

| Strategy | Examples | Code Size | Runtime Cost | Compile Cost |
|----------|----------|-----------|--------------|--------------|
| **Type Erasure** | Java, OCaml (default) | O(1) | Boxing overhead | O(1) |
| **Full Monomorphization** | Rust, C++, MLton | O(n×f) | Optimal | O(n×f) |
| **Hybrid/Partial** | Go, Swift, GHC | O(k×f), k≤n | Near-optimal | O(k×f) |

The union-branch approach falls into the **hybrid** category, specifically as a form of
**intensional polymorphism** applied to values rather than types.

### Related Techniques in Literature

#### 1. Intensional Type Analysis (Harper & Morrisett, 1995)

Harper and Morrisett introduced [intensional polymorphism](https://dl.acm.org/doi/10.1145/199448.199475)
for compiling polymorphic languages without boxing. The key idea: dispatch on type structure
at runtime using a `typecase` construct.

```
typecase T of
  int => ... use int operations ...
  bool => ... use bool operations ...
  T1 * T2 => ... use pair operations ...
```

**Connection to union-branch**: Our approach applies the same principle to *values* rather
than types. Instead of `typecase` over type structure, we `match` over a closed enum of
known compile-time values:

```
match comptime_arg of
  Value1 => ... const-fold with Value1 ...
  Value2 => ... const-fold with Value2 ...
```

#### 2. Dictionary Passing (Wadler & Blott, 1989; GHC)

Haskell's type classes are compiled via [dictionary passing](https://wiki.haskell.org/Inlining_and_Specialisation):
each type class constraint becomes a runtime dictionary argument containing method implementations.

```haskell
-- Source
sort :: Ord a => [a] -> [a]

-- Compiled (conceptually)
sort :: OrdDict a -> [a] -> [a]
sort dict xs = ... (compare dict) ...
```

GHC then [specializes aggressively](https://reasonablypolymorphic.com/blog/specialization/)
when concrete types are known, eliminating dictionary overhead.

**Connection to union-branch**: The union tag functions like a minimal dictionary—it carries
just enough information to select the right code path. Unlike full dictionaries, the tag
is a simple integer discriminant rather than a vtable pointer.

#### 3. GCShape Stenciling (Go 1.18+)

Go's generics use [GCShape stenciling](https://github.com/golang/proposal/blob/master/design/generics-implementation-gcshape.md):
one code copy per GC shape (memory layout), with a dictionary for type-specific operations.

```go
// Conceptually: one stencil for all pointer types
func Map[T any](slice []T, f func(T) T) []T
// becomes
func Map_gcshape_ptr(dict, slice, f) []any
```

**Connection to union-branch**: Both approaches generate fewer copies than full monomorphization
by grouping instantiations. GCShape groups by memory layout; union-branch groups by
explicitly listing all values and dispatching via match.

#### 4. Swift Witness Tables

Swift uses [protocol witness tables](https://developer.apple.com/videos/play/wwdc2016/416/)
for existentials and generic dispatch, with aggressive specialization for concrete types.

**Connection to union-branch**: Swift's "unspecialized generic" with witness tables is
similar to our union-branch approach—runtime dispatch through a table/tag, with the
compiler specializing hot paths.

#### 5. Defunctionalization (Reynolds, 1972)

[Defunctionalization](https://blog.sigplan.org/2019/12/30/defunctionalization-everybody-does-it-nobody-talks-about-it/)
transforms higher-order programs into first-order ones by replacing function values with
data constructors:

```
-- Higher-order
let f = if cond then add1 else mul2
f(x)

-- Defunctionalized
enum Func { Add1, Mul2 }
let f = if cond then Func::Add1 else Func::Mul2
apply(f, x)  // dispatches on tag
```

**Connection to union-branch**: Union-branch is essentially *defunctionalization of
comptime values*. We take the open set of possible values and close it into an enum,
then dispatch via `apply` (match).

#### 6. Partial Evaluation (Jones, Gomard, Sestoft)

[Partial evaluation](https://en.wikipedia.org/wiki/Partial_evaluation) specializes programs
by evaluating static (known) inputs at compile time, leaving residual code for dynamic inputs.

**Connection to union-branch**: Within each branch of the union dispatch, we perform
partial evaluation—the comptime value is static, enabling constant folding, dead code
elimination, and loop unrolling within that branch.

### The Union-Branch Approach Formalized

Given a function with comptime parameter:

```
fun f(const c: T, x: U) -> R
    ... body using c ...
end fun
```

And call sites with values `{v1, v2, ..., vn}`, the **union-branch transformation** produces:

```
enum ComptimeC { V1, V2, ..., Vn }

fun f_unified(c_tag: ComptimeC, x: U) -> R
    match c_tag
        V1 =>
            const c = v1
            ... body using c ...  // const-folded
        V2 =>
            const c = v2
            ... body using c ...  // const-folded
        ...
    end match
end fun
```

Call sites transform: `f(v1, x)` → `f_unified(ComptimeC::V1, x)`

### Comparison: Full Monomorphization vs Union-Branch

| Aspect | Full Mono | Union-Branch |
|--------|-----------|--------------|
| Code copies | N functions | 1 function, N branches |
| Instruction cache | Poor (N copies) | Good (1 function) |
| Branch prediction | Perfect (no branches) | Dependent on value distribution |
| Const propagation | Full | Full (within branch) |
| Inlining | Each copy inlinable | Function inlinable, branches not |
| Debug symbols | N function entries | 1 entry, complex CFG |
| Compile time | O(N × size) | O(size + N × branch) |

### When Union-Branch Wins

1. **Many instantiations, small body**: Union overhead amortized
2. **Shared prefix/suffix code**: Not duplicated across branches
3. **Instruction cache pressure**: Single function stays hot
4. **Compile time critical**: Less code generation
5. **Value distribution skewed**: Branch predictor effective

### When Full Mono Wins

1. **Few instantiations**: No point in union overhead
2. **Large body with value-dependent control flow**: Branches nest poorly
3. **Inlining critical**: Full mono exposes more to optimizer
4. **Link-time optimization available**: LTO deduplicates anyway

### Theoretical Classification

The union-branch approach can be characterized as:

- **Intensional polymorphism** at the value level (runtime dispatch on static values)
- **Defunctionalization** of the comptime value space
- **Partial monomorphization** (one function, multiple specialized paths)
- **Type-preserving** (no boxing, values maintain concrete types within branches)

This places it in the same family as:
- Go's GCShape stenciling
- GHC's dictionary specialization
- Swift's unspecialized generics with witness tables
- JIT polymorphic inline caches (but at compile time)

### Implementation Sketch for Datalove

```rust
/// Collected comptime values for a parameter
struct ComptimeValueSet {
    param_index: usize,
    values: Vec<ConstValue>,  // all values seen at call sites
}

/// Transform a function to union-branch form
fn union_branch_transform(
    func: &StmtFun,
    value_sets: &[ComptimeValueSet],
) -> StmtFun {
    // 1. Build enum type for each comptime param
    // 2. Wrap body in nested match on enum tags
    // 3. Within each branch, bind const to concrete value
    // 4. Existing const-folding handles the rest
}

/// Rewrite call site
fn rewrite_call(
    call: &ExprCall,
    value_map: &HashMap<ConstValue, EnumVariant>,
) -> ExprCall {
    // Replace const arg with enum variant constructor
}
```

### Hybrid Strategy: Adaptive Specialization

The optimal approach may combine both strategies:

```
if num_instantiations <= MONO_THRESHOLD {
    full_monomorphization()
} else if num_instantiations <= UNION_THRESHOLD {
    union_branch_transform()
} else {
    error("too many instantiations")
}
```

Suggested thresholds:
- `MONO_THRESHOLD = 4`: Few enough that code duplication is fine
- `UNION_THRESHOLD = 64`: Beyond this, even union-branch is expensive

### References

- Harper, R. & Morrisett, G. (1995). [Compiling Polymorphism Using Intensional Type Analysis](https://dl.acm.org/doi/10.1145/199448.199475). POPL.
- Reynolds, J. C. (1972). Definitional Interpreters for Higher-Order Programming Languages. ACM Annual Conference.
- Wadler, P. & Blott, S. (1989). How to Make Ad-Hoc Polymorphism Less Ad Hoc. POPL.
- Jones, N., Gomard, C., & Sestoft, P. (1993). [Partial Evaluation and Automatic Program Generation](https://www.cs.utexas.edu/~novak/jonesgomardsestoft.pdf). Prentice Hall.
- Go Team. (2022). [Generics Implementation - GCShape Stenciling](https://github.com/golang/proposal/blob/master/design/generics-implementation-gcshape.md).
- Apple. (2016). [Understanding Swift Performance](https://developer.apple.com/videos/play/wwdc2016/416/). WWDC.
- Sandy Maguire. (2019). [GHC's Specializer: Much More Than You Wanted to Know](https://reasonablypolymorphic.com/blog/specialization/).
- Weeks, S. (2006). [Whole-Program Compilation in MLton](https://dl.acm.org/doi/10.1145/1159876.1159877). ML Workshop.

## Conclusion

Comptime arguments are feasible with moderate implementation effort. The existing CTFE
infrastructure provides the foundation.

### For Full Monomorphization

Key design decisions:
1. **Separate `is_comptime` flag** - cleanest separation of concerns
2. **Post-typecheck specialization pass** - leverages existing pipeline
3. **Whole-program enumeration** - exploits compiler's global view
4. **Instance limits** - prevents compile-time explosion

### For Union-Branch Approach

Key design decisions:
1. **Defunctionalize comptime values** into closed enums
2. **Single function with match dispatch** - better icache, compile time
3. **Const-fold within branches** - reuse existing CTFE infrastructure
4. **Adaptive threshold** - mono for few instances, union for many

The union-branch approach places datalove in well-established PL territory alongside
Go's GCShape stenciling, GHC's dictionary specialization, and Swift's witness tables.
It trades optimal codegen for reduced code size and compile time—appropriate for a
scripting language prioritizing fast iteration.
