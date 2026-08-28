# Const Parameter Specialization: Research & Design

> **Status, revised after implementation.** The union-branch approach described
> here was built and works, but the case made for it below does not survive
> contact with the implementation, and its extension to type arguments does not
> work at all.
>
> - Union-branch was chosen for code size and compile time. It achieves neither:
>   `build_dispatch_blocks` clones the body once per instantiation, so it is
>   monomorphization plus a `Switch` on a value that is a compile-time constant
>   at every call site. See the corrected tables below, marked WRONG.
> - "Extension to Type Arguments" reaches the right question and stops. Both
>   paths need `x` and `y` to have one type in a function whose branches give
>   them different ones. That is not a gap in the sketch, it is the reason the
>   approach does not extend: union-branch keeps one signature by replacing a
>   const parameter with a tag, and a type parameter changes the signature.
> - The recommendation of Zig-style `const T: type` was made without weighing
>   what it costs in tooling and incremental compilation, which are the things
>   this compiler is built for.
>
> The direction that replaced this is in [Generics and Specialization](plan-generics.md).
> This document is kept for its survey of the literature, which is still good,
> and as the record of how the question was reached.

## Overview

This document analyzes the feasibility of adding Zig-style const parameter specialization to datalove,
using `const` as the argument modifier. Const parameters are function parameters whose
values must be known at compile time, enabling function specialization based on those values.

```datalove
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
- **Future path**: Types as const values use same mechanism

## Specialization Strategy

### Whole-Program Advantage

As a whole-program compiler, datalove can:
1. See all call sites before codegen
2. Enumerate all unique const parameter combinations
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
     If callee has const parameters:
       Extract const argument values
       Record (callee, const_values) → call_site

2. GROUP PHASE (whole program)
   For each (func, const_args) combination:
     Generate unique specialization key: "func_name$$const1_const2"
     Create specialized function entry

3. GENERATE PHASE (per specialization)
   Clone function AST
   Replace const parameters with local const bindings
   Remove const parameters from signature
   Add to module's function list

4. REWRITE PHASE (per module)
   Replace calls to generic functions with specialized versions
```

### Example Transformation

Input:
```datalove
fun format_width(const width: i32, s: string) -> string
    // ... use width ...
end fun

let a = format_width(10, "hi")
let b = format_width(20, "hello")
```

After specialization:
```datalove
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
```datalove
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
- `datafun-tycheck/src/synthesize.rs`: Validate const args are const expressions
- `datafun-tycheck/src/context.rs`: Track const arg values for specialization

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
- `datafun-lower/src/lib.rs`: Skip const parameters when lowering specialized functions

## Compile-Time Considerations

### Cost Model

For a scripting language prioritizing compile speed:

| Operation | Cost |
|-----------|------|
| Parse `const` keyword | Negligible |
| Track comptime flag | Negligible |
| Detect const arguments | O(1) per call site |
| Evaluate const args | Reuse existing CTFE |
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
3. **Size limit**: Max 10 const parameters per function
4. **Error on exceeded**: Clear error message, not silent degradation

## Future: Type Parameters

The `const` mechanism naturally extends to type parameters:

```datalove
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

Always inline const-param functions at call sites.

**Pros**: Simpler, no new pass.
**Cons**: Code bloat, poor cache behavior, loses function identity.

### Alternative: Runtime Specialization Cache

Specialize lazily at runtime, cache results.

**Pros**: No compile-time cost.
**Cons**: Violates compile-time priority, complex runtime, JIT territory.

### Alternative: Type-Directed Specialization Only

Only specialize when const args affect types (like Rust).

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

```datalove
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

```datalove
-- Higher-order
let f = if cond then add1 else mul2
f(x)

-- Defunctionalized
enum Func { Add1, Mul2 }
let f = if cond then Func::Add1 else Func::Mul2
apply(f, x)  // dispatches on tag
```

**Connection to union-branch**: Union-branch is essentially *defunctionalization of
const values*. We take the open set of possible values and close it into an enum,
then dispatch via `apply` (match).

#### 6. Partial Evaluation (Jones, Gomard, Sestoft)

[Partial evaluation](https://en.wikipedia.org/wiki/Partial_evaluation) specializes programs
by evaluating static (known) inputs at compile time, leaving residual code for dynamic inputs.

**Connection to union-branch**: Within each branch of the union dispatch, we perform
partial evaluation—the const value is static, enabling constant folding, dead code
elimination, and loop unrolling within that branch.

### The Union-Branch Approach Formalized

Given a function with const parametereter:

```datalove
fun f(const c: T, x: U) -> R
    ... body using c ...
end fun
```

And call sites with values `{v1, v2, ..., vn}`, the **union-branch transformation** produces:

```datalove
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

**WRONG.** Kept as written for the record; corrected immediately below.

| Aspect | Full Mono | Union-Branch |
|--------|-----------|--------------|
| Code copies | N functions | 1 function, N branches |
| Instruction cache | Poor (N copies) | Good (1 function) |
| Branch prediction | Perfect (no branches) | Dependent on value distribution |
| Const propagation | Full | Full (within branch) |
| Inlining | Each copy inlinable | Function inlinable, branches not |
| Debug symbols | N function entries | 1 entry, complex CFG |
| Compile time | O(N × size) | O(size + N × branch) |

The error is in the last two rows, and it invalidates the first two. A branch is
not cheaper than a copy of the body, it *is* a copy of the body: the transform
clones every original block once per instantiation. Corrected:

| Aspect | Full Mono | Union-Branch |
|--------|-----------|--------------|
| Code copies | N bodies | N bodies, plus a dispatch block |
| Instruction cache | N copies | The same N copies in one symbol |
| Branch prediction | No branch | A `Switch` on a per-call-site constant |
| Const propagation | Full | Full (within branch) |
| Inlining | Each copy inlinable | One oversized body, inlined whole or not at all |
| Debug symbols | N function entries | 1 entry, complex CFG |
| Compile time | O(N × size) | O(N × size) + dispatch |
| JIT tiering | Per instantiation | All instantiations share one call count |

The last row is specific to this compiler. `optimizing.rs` inlines at 50 calls and
JIT-compiles at 100, both counted per function, so fusing the instantiations means a
hot one cannot tier without dragging the cold ones with it.

### When Union-Branch Wins

**Mostly WRONG.** Points 1, 3 and 4 assume the body is shared across branches. It is
not, so the union overhead is not amortized, the single function is the same total code,
and there is no code generation saved. Point 5 describes a branch that need not exist.
Point 2 is the only real one, and it is small.

1. **Many instantiations, small body**: Union overhead amortized
2. **Shared prefix/suffix code**: Not duplicated across branches
3. **Instruction cache pressure**: Single function stays hot
4. **Compile time critical**: Less code generation
5. **Value distribution skewed**: Branch predictor effective

The case where a tag genuinely beats monomorphization is the one this design does not
have: when the tag is *not* known at the call site. Julia's world-splitting branches
over up to four candidate types precisely because dispatch is otherwise dynamic. Here
the const-binding-only restriction guarantees the value is static, so the dispatch is
overhead by construction.

### When Full Mono Wins

1. **Few instantiations**: No point in union overhead
2. **Large body with value-dependent control flow**: Branches nest poorly
3. **Inlining critical**: Full mono exposes more to optimizer
4. **Link-time optimization available**: LTO deduplicates anyway

### Theoretical Classification

The union-branch approach can be characterized as:

- **Intensional polymorphism** at the value level (runtime dispatch on static values)
- **Defunctionalization** of the const value space
- **Partial monomorphization** (one function, multiple specialized paths)
- **Type-preserving** (no boxing, values maintain concrete types within branches)

This places it in the same family as:
- Go's GCShape stenciling
- GHC's dictionary specialization
- Swift's unspecialized generics with witness tables
- JIT polymorphic inline caches (but at compile time)

### Implementation Sketch for Datalove

```rust
/// Collected const values for a parameter
struct ComptimeValueSet {
    param_index: usize,
    values: Vec<ConstValue>,  // all values seen at call sites
}

/// Transform a function to union-branch form
fn union_branch_transform(
    func: &StmtFun,
    value_sets: &[ComptimeValueSet],
) -> StmtFun {
    // 1. Build enum type for each const parameter
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

## Extension to Type Arguments: Two Paths

Having established union-branch for const *values*, we can extend to type arguments
via two distinct paths: **Rust-style type parameters** or **const parameter types**.

### Path 1: Rust-Style Type Parameters (Separate Namespace)

In Rust, type parameters live in a separate namespace from values:

```rust
fn swap<T>(x: T, y: T) -> (T, T) { (y, x) }
```

#### How Union-Branch Extends

The union-branch approach generalizes naturally. Instead of defunctionalizing *values*
into an enum, we defunctionalize *types* into a type-level enum:

```datalove
// Source
fun swap<T>(x: T, y: T) -> (T, T)
    (y, x)
end fun

// Called with T = i32, string, {a: bool} in program

// Union-branch transformation
enum TypeTag_swap { I32, String, AnonStruct_a_bool }

fun swap_unified(type_tag: TypeTag_swap, x: ???, y: ???) -> ???
    match type_tag
        I32 =>
            // T = i32, use i32 operations
            (y as i32, x as i32)
        String =>
            // T = string, use string operations
            (y as string, x as string)
        AnonStruct_a_bool =>
            // T = {a: bool}, use struct operations
            (y as {a: bool}, x as {a: bool})
    end match
end fun
```

#### The Challenge: What Are x and y's Types?

In the unified function, `x` and `y` can't have a single static type. Options:

**Option 1a: Maximal Layout Union**

```
// x and y are stored in a union large enough for any variant
struct UnifiedArg {
    data: [u8; MAX_SIZE],  // sized for largest type
    // or: data: *mut u8 for heap-allocated types
}
```

Within each branch, reinterpret the bytes as the concrete type. This is what
intensional polymorphism does—the type tag tells you how to interpret the bits.

**Option 1b: Pointer + Size Descriptor**

```datalove
// All args passed as (pointer, size, alignment)
fun swap_unified(
    type_tag: TypeTag_swap,
    x_ptr: *u8, x_size: usize,
    y_ptr: *u8, y_size: usize,
) -> (*u8, usize)
```

The type tag determines how to copy/move the bytes. This is closer to Go's approach.

**Option 1c: Boxed Representation**

```datalove
// All args boxed to uniform representation
fun swap_unified(type_tag: TypeTag_swap, x: Box<Any>, y: Box<Any>) -> Box<Any>
```

Defeats much of the purpose, but simplest. Only viable if you're okay with allocation.

#### Type-Dependent Operations

The harder problem: what if the function body *uses* type-specific operations?

```datalove
fun process<T>(x: T) -> i32
    x.size()  // T must have a size() method
end fun
```

This requires **trait bounds** (Rust) or **concepts** (C++). With union-branch:

```datalove
enum TypeTag_process { Vec_i32, String, MyCollection }

fun process_unified(type_tag: TypeTag_process, x: ???) -> i32
    match type_tag
        Vec_i32 => (x as list<i32>).len()     // list has len()
        String => (x as string).len()          // string has len()
        MyCollection => (x as MyCollection).size()  // custom method
    end match
end fun
```

The union-branch approach makes trait bounds less necessary at the type system level—
the compiler knows all instantiations and can verify each branch type-checks.

#### Rust-Style Summary

| Aspect | Implementation |
|--------|----------------|
| Type parameter syntax | `fun foo<T>(x: T)` |
| Type namespace | Separate from values |
| Union-branch enum | Over types, not values |
| Argument passing | Maximal layout or pointer+descriptor |
| Type operations | Resolved per-branch |
| Trait bounds | Optional (can verify per-branch instead) |

### Path 2: Zig-Style Comptime Types (Types as Values)

In Zig, types are first-class const values:

```zig
fn swap(comptime T: type, x: T, y: T) -> struct { T, T } {
    return .{ y, x };
}
```

The key insight: **`type` is just another type**, and type values flow through
the same comptime machinery as integer values.

#### How Union-Branch Extends

Since types are values, they're handled identically to const integers:

```datalove
// Source
fun swap(const T: type, x: T, y: T) -> (T, T)
    (y, x)
end fun

// Called with T = i32, T = string, T = {a: bool}

// Union-branch transformation (same as values!)
enum ComptimeT { Type_i32, Type_string, Type_AnonStruct_a_bool }

fun swap_unified(t_tag: ComptimeT, x: ???, y: ???) -> ???
    match t_tag
        Type_i32 =>
            const T: type = i32
            // Now T is bound, x: T means x: i32
            (y, x)  // typed as (i32, i32)
        Type_string =>
            const T: type = string
            (y, x)  // typed as (string, string)
        ...
    end match
end fun
```

#### What Is `type`?

Need a compile-time representation for types. Options:

**Option 2a: IrType as ConstValue**

```rust
// Already have IrType enum - make it a ConstValue variant
enum ConstValue {
    // ... existing variants ...
    Type(IrType),  // NEW: type values
}
```

Now `const T: type = i32` creates `ConstValue::Type(IrType::I32)`.

**Option 2b: Type Descriptors**

```rust
// Types represented as structured descriptors
struct TypeDescriptor {
    kind: TypeKind,
    size: usize,
    alignment: usize,
    fields: Option<Vec<FieldDescriptor>>,
    // ...
}
```

Richer but more complex. Needed for reflection/introspection.

#### Dependent Types (Light)

const parameter types create a form of dependent typing:

```datalove
fun make_array(const T: type, const N: i32) -> [T; N]
    // Return type depends on const values T and N
```

With union-branch, this "just works":

```datalove
enum ComptimeT { Type_i32, Type_bool }
enum ComptimeN { N_4, N_8, N_16 }

// Nested union-branch
fun make_array_unified(t_tag: ComptimeT, n_tag: ComptimeN) -> ???
    match t_tag
        Type_i32 => match n_tag
            N_4 => ... return [i32; 4] ...
            N_8 => ... return [i32; 8] ...
            N_16 => ... return [i32; 16] ...
        Type_bool => match n_tag
            N_4 => ... return [bool; 4] ...
            ...
```

Each branch has a concrete return type. The "dependent" return type is resolved
per-branch at compile time.

#### Type Operations at Comptime

Zig allows computing on types:

```zig
fn Pair(comptime A: type, comptime B: type) type {
    return struct { first: A, second: B };
}

const MyPair = Pair(i32, bool);  // MyPair is a type!
var x: MyPair = .{ .first = 42, .second = true };
```

With union-branch + CTFE:

1. **CTFE evaluates `Pair(i32, bool)`** → returns `ConstValue::Type(struct{first:i32, second:bool})`
2. **Union-branch collects** all type-returning comptime calls
3. **Type values substituted** into dependent positions

```datalove
// After specialization
const MyPair: type = {first: i32, second: bool}  // CTFE result
var x: {first: i32, second: bool} = ...          // MyPair substituted
```

#### Zig-Style Summary

| Aspect | Implementation |
|--------|----------------|
| Type parameter syntax | `fun foo(const T: type, x: T)` |
| Type namespace | Same as values (comptime) |
| Type representation | `ConstValue::Type(IrType)` |
| Union-branch enum | Same machinery as values |
| Dependent types | Resolved per-branch |
| Type computation | CTFE returns type values |

### Comparison: Rust-Style vs Zig-Style

| Aspect | Rust-Style | Zig-Style |
|--------|------------|-----------|
| Conceptual model | Types and values separate | Types are const values |
| Syntax | `<T>` type parameters | `const T: type` argument |
| Type bounds | Traits/concepts | Ad-hoc (check at instantiation) |
| Type computation | Associated types only | Full CTFE on types |
| Implementation complexity | Higher (two namespaces) | Lower (unified) |
| Familiarity | Mainstream (Rust, C++, Java) | Niche (Zig) |
| Expressiveness | Bounded | Full dependent types (light) |

### Recommendation for Datalove

**SUPERSEDED.** See [Generics and Specialization](plan-generics.md). The reasoning
below is sound about Zig-style being the better fit *for union-branch*, and that is
the problem: union-branch does not extend to type parameters, so fitting it is not a
recommendation. What the comparison leaves out is the cost of template-style generics
in tooling, error locality and incremental compilation, which are the properties this
compiler is organised around.

**Zig-style is more natural** given the union-branch approach:

1. **Uniform machinery**: Types as const values use same enum/match as integers
2. **Simpler implementation**: One namespace, one specialization mechanism
3. **More expressive**: Type computation for free via CTFE
4. **Consistent syntax**: `const n: i32` and `const T: type` parallel

The main downside is unfamiliarity—users expect `<T>` syntax. But Zig has proven
the `comptime T: type` model is learnable and arguably clearer.

### Implementation Roadmap

**Phase A: Const Values (Current Focus)**
```datalove
fun repeat(const n: i32, s: string) -> string
```
- Union-branch over value enums
- CTFE for const evaluation
- Foundation for everything else

**Phase B: Type as ConstValue**
```
const MyType: type = i32
```
- Add `ConstValue::Type(IrType)`
- CTFE can return types
- Type aliases via const bindings

**Phase C: Comptime Type Parameters**
```datalove
fun identity(const T: type, x: T) -> T
```
- Parameter type `T` depends on const arg
- Union-branch over type enums
- Per-branch type substitution

**Phase D: Type Computation**
```datalove
fun Pair(const A: type, const B: type) -> type
    {first: A, second: B}
end fun
```
- Functions returning types
- CTFE evaluates type expressions
- Full Zig-style dependent types (light)

### Code Sketch: Type as ConstValue

```rust
// In datafun-ir/src/lib.rs
enum ConstValue {
    // ... existing ...

    /// A type value (for comptime type parameters)
    Type(Box<IrType>),
}

// In datafun-tycheck
fn check_comptime_type_arg(
    ctx: &mut TypeContext,
    arg: ExprFun,
) -> Result<IrType, TypeError> {
    // Evaluate arg at compile time
    let const_val = ctx.evaluate_const(arg)?;
    match const_val {
        ConstValue::Type(ir_type) => Ok(*ir_type),
        _ => Err(TypeError::ExpectedType { got: const_val }),
    }
}

// In specialization pass
fn union_branch_for_types(
    func: &StmtFun,
    type_sets: &[HashSet<IrType>],  // all types seen per param
) -> StmtFun {
    // Build enum: enum TypeTag { Type_i32, Type_string, ... }
    // Wrap body in match
    // Within each branch, substitute concrete type
}
```

### The Deep Connection to Intensional Polymorphism

This brings us full circle to Harper & Morrisett:

| Their Work | Our Extension |
|------------|---------------|
| `typecase T of int => ...` | `match type_tag of Type_i32 => ...` |
| Type representations at runtime | Type enum discriminants at runtime |
| Dispatch on type structure | Dispatch on closed type set |
| Enables unboxed polymorphism | Enables per-branch specialization |

The difference: Harper/Morrisett dispatch on *open* type structure (any type matches
some case). We dispatch on *closed* type sets (only types seen in program). This
is possible because we're a whole-program compiler.

Their insight was: **you don't need to know the type at compile time if you can
dispatch on it at runtime**. Our insight is: **you don't need to monomorphize
if you can dispatch on a finite set of known instantiations**.

## Conclusion

Const parameters are feasible with moderate implementation effort. The existing CTFE
infrastructure provides the foundation.

### For Full Monomorphization

Key design decisions:
1. **Separate `is_comptime` flag** - cleanest separation of concerns
2. **Post-typecheck specialization pass** - leverages existing pipeline
3. **Whole-program enumeration** - exploits compiler's global view
4. **Instance limits** - prevents compile-time explosion

### For Union-Branch Approach

Key design decisions:
1. **Defunctionalize const values** into closed enums
2. **Single function with match dispatch** - better icache, compile time
3. **Const-fold within branches** - reuse existing CTFE infrastructure
4. **Adaptive threshold** - mono for few instances, union for many

The union-branch approach places datalove in well-established PL territory alongside
Go's GCShape stenciling, GHC's dictionary specialization, and Swift's witness tables.
It trades optimal codegen for reduced code size and compile time—appropriate for a
scripting language prioritizing fast iteration.
