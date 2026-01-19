# Advanced PLT Directions for Datalove

Based on the restrictive type system: linear types, purity, parameter modes, whole-program compilation.

---

## High-Value Directions

### 1. Quantitative Type Theory (QTT)

Linear types are binary (0 or 1 uses). QTT generalizes to semiring annotations - "use exactly 2 times", "use at most 3 times", "use any number of times":
- `ω` (unrestricted) for copy types
- `1` (linear) for current linear types
- `0` for erased/phantom types (compile-time only data)
- Arithmetic on usages

Pairs naturally with existing `in`/`ref`/`mut`/`out` modes.

### 2. Refinement Types / Liquid Types

SMT-backed predicates on types:
```
type NonZero: { n: u32 | n != 0 }
type Sorted: { xs: [@int] | is_sorted(xs) }
```
Purity makes refinement checking tractable - no side effects to invalidate predicates. Could eliminate runtime bounds checks, prove division safety, etc.

### 3. Sized Types for Termination

Agda-style sized types track structural recursion:
```
type List<A, s: Size> = @enum { Nil, Cons(A, List<A, s-1>) }
```
Combined with the "almost-total" goal, gives provable termination for many functions. Infinite loops become opt-in via a `diverge` annotation.

### 4. Session Types

Parameter modes hint at protocol structure. Full session types could verify:
- Message ordering in protocols
- Deadlock freedom
- Protocol completion

Linear types guarantee each channel endpoint is used exactly once per step.

### 5. Graded Modal Types (Granule-style)

Track resource usage precisely through grades:
- Sensitivity analysis (how much does output change per input change?)
- Security levels (secret vs public data)
- Memory usage bounds

Existing heap annotations (`@`/`#`) could be generalized this way.

---

## Medium-Term Explorations

### 6. Mercury-style Bidirectional Modes

Full implementation would give:
- `in`/`out` determinism inference
- Automatic inverse function generation
- Relational programming where appropriate

### 7. Staged Computation / Multi-Stage Programming

Purity + whole-program compilation enables aggressive staging:
- Compile-time partial evaluation
- Quote/unquote for metaprogramming
- Guaranteed specialization

### 8. Algebraic Effects with Linear Handlers

Linear types make effect handlers safer:
- Exactly-once handling guaranteed
- Resource cleanup provable
- No "use continuation twice" bugs

### 9. Row Polymorphism for Structs/Enums

Anonymous structs/enums could gain row polymorphism:
```
fun add_field<R>(x: { R }): { new_field: u32, R }
```
Enables extensible records without nominal types.

### 10. Lens/Optic System

With pure functions and parameter modes, first-class optics:
- `view` (read), `update` (write), `traverse` (batch)
- Compose naturally
- Type-safe nested access

---

## Speculative / Research-Adjacent

### 11. Proof-Carrying Code

Embed simple proofs in types, verify at compile time:
```
fun binary_search(xs: Sorted, target: int): ?usize
  // compiler knows xs is sorted, can verify algorithm correctness
```

### 12. Automatic Differentiation Types

Linear types map well to AD:
- Forward mode: linear functions preserve linearity
- Reverse mode: needs careful handling but tractable
- Could type-check gradient computations

### 13. Capability-Safe I/O (for Full Datalove layer)

When adding effects, use capabilities:
```
fun read_file(cap: FileRead, path: string): !string
```
Capability is linear - can't duplicate access rights.

### 14. Incremental/Self-Adjusting Computation

Salsa infrastructure + purity enables:
- Automatic incrementalization
- Change propagation tracking
- Minimal recomputation on input change

### 15. Compact Memory Representations

Whole-program analysis + linear types could enable:
- Automatic unboxing
- Struct-of-arrays transformations (tables already exist)
- Cache-conscious layout optimization

---

## Most Promising Starting Points

Ranked by leverage given existing infrastructure:

1. **Refinement types** - High payoff, purity makes it tractable
2. **Sized types** - Natural fit for termination checking goal
3. **QTT/graded types** - Generalizes what already exists
4. **Mercury modes** - Already on the radar, extends parameter modes

The combination of refinement types + sized types approaches a dependently-typed language without full dependent types - "lightweight dependent types."
