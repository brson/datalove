# Research Report: Linear and Affine Type Systems

Research on linear and affine type systems for the Datalove language, including precedents, semantic requirements, and implementation considerations.

## Core Definitions

**Substructural type systems** are based on restricting three structural rules from logic:
- **Exchange**: Variables can be used in any order
- **Weakening**: Variables can be discarded without use
- **Contraction**: Variables can be duplicated and used multiple times

This creates four categories:

1. **Linear types** (exchange only): Must be used **exactly once**
   - Cannot be copied, cannot be discarded
   - Corresponds to linear logic's resource interpretation

2. **Affine types** (exchange + weakening): Used **at most once**
   - Cannot be copied, but can be discarded
   - Rust uses this model

3. **Relevant types** (exchange + contraction): Used **at least once**
   - Can be copied, but cannot be discarded

4. **Unrestricted types** (all rules): Used **any number of times**
   - Normal types in most languages

## Formal Semantic Requirements

### Basic Type Rules for Linear Lambda Calculus

The key difference in linear systems is **context splitting**:

**Multiplicative connectives** (⊗, ⊸):
```
Γ ⊢ e1 : A    Δ ⊢ e2 : B
─────────────────────────  (contexts split)
Γ, Δ ⊢ (e1, e2) : A ⊗ B
```

**Additive connectives** (&):
```
Γ ⊢ e1 : A    Γ ⊢ e2 : B
─────────────────────────  (same context)
Γ ⊢ (e1, e2) : A & B
```

**Linear function application**:
```
Γ ⊢ f : A ⊸ B    Δ ⊢ e : A
─────────────────────────────
Γ, Δ ⊢ f e : B
```

Key invariant: Every variable in the context must be used exactly once in the expression.

### Core Semantic Properties

1. **Single-use guarantee**: Each linear value is consumed exactly once
2. **Explicit destruction**: Linear values must have explicit destructors
3. **No implicit dropping**: Compiler must reject unused linear values
4. **Resource safety**: Enables safe manual memory management without GC
5. **Type safety**: Well-typed programs can't misuse resources

## Language Precedents

### Rust (Affine Types)

**Model**: Affine ownership with move semantics

**Key features**:
- Values are moved by default (not copied)
- `Drop` trait provides destructors
- Cannot implement both `Copy` and `Drop`
- Borrow checker manages aliasing

**Modes**: `mut` (unique mutable), `&` (shared immutable), `&mut` (unique mutable borrow)

**Limitations**: Complex borrow checker, affine not strictly linear

### Clean (Uniqueness Types)

**Model**: Uniqueness typing for in-place updates

**Key features**:
- Single-threaded guarantee for unique values
- Enables destructive updates while maintaining referential transparency
- Used for I/O, arrays, and efficient data structures

**Distinction**: Uniqueness guarantees no other references exist; linearity prevents creating new references.

### Mercury (Mode System + Uniqueness)

**Model**: Modes track instantiation states with uniqueness annotations

**Key features**:
- `in` (bound → bound), `out` (free → bound)
- `ui` (unique → unique): unique input
- `uo` (free → unique): unique output
- `di` (unique → dead): destructive input (consumes)

**Semantics**: `di` mode explicitly marks when values are consumed and become inaccessible

**Applications**: I/O state threading, array updates, C++ object management

### ATS (Linear Types + Dependent Types)

**Model**: Full linear types with theorem proving

**Key features**:
- Linear types for resource management
- Dependent types for correctness proofs
- Can prove memory safety, no leaks, no buffer overflows
- Every function simultaneously constructs return value and destroys parameter (in linear logic view)

### Austral (Modern Linear Types)

**Model**: "Rust: The Good Parts" with stricter linearity

**Syntax**: `database!` suffix denotes linear types

**Key features**:
- Simpler than Rust's borrow checker
- Linear capabilities for security
- Operations consume and return new handles: `(db1, result) = query(db, "SELECT ...")`

**Philosophy**: Brutal simplicity, short spec, understandable by single person

**Example**:
```austral
database! connect(string path);
pair<database!, result_set> query(database! db, string query);
void close(database! db);

// Usage
database! db = connect("path");
auto [db1, set1] = query(db, "SELECT ...");
auto [db2, set2] = query(db1, "INSERT ...");
close(db2);
```

**Error prevention**:
- Double-free: `close(db); close(db);` is a compiler error
- Use-after-free: Cannot call `query(db, ...)` after `close(db)`
- Concurrency: Passing linear value to thread consumes it in originating thread

### Vale (Linear-Aliasing Model)

**Model**: Linear ownership + safe mutable aliasing

**Key features**:
- Explicit destruction required for owned values
- Generational references enable safe aliasing with runtime checks
- Regions for zero-cost immutable borrowing
- Four basic types: owned `^T`, reference `&T`, value `T`, copy types

**Innovation**: Combines linear types with practical aliasing patterns (observers, callbacks)

**Core principle**: "Every object's lifetime is determined by exactly one owner, yet we can also have as many mutable references to it as we want."

**Safe aliasing**: Generational references perform runtime checks (8-byte generation number) to verify objects still exist, with overhead eliminable through static analysis.

### Futhark (Uniqueness for Arrays)

**Model**: Uniqueness types for parallel array programming

**Key features**:
- In-place array updates via `with` expressions
- Consumption annotations: `*a -> b` (consumes), `a -> b` (doesn't)
- Syntax-driven alias analysis
- Cost model: updates proportional to value size, not array size

**Trade-offs**: Struggles with higher-order functions, consumption polymorphism

**Key insight**: "Aliases never affect the semantics of the program, only whether it will type check."

## Baseline Semantic Requirements

### 1. Type Qualifiers

Your system needs at minimum:
- **Linear** (`!T` or similar): must use exactly once
- **Copy** (`T` or `@T`): unrestricted use
- Optional: **Affine** (at most once), **Unique** (no aliases)

### 2. Ownership and Moves

```
let x: !Database = connect();
let y = x;  // x is moved, now invalid
// use(x);  // ERROR: x was moved
```

### 3. Explicit Destructors

Linear types require named destructor functions:
```
x.close()           // explicit destruction
drop(x)             // or generic drop function
x.into_destructor() // or conversion to destructor
```

Key decision: Should destructors:
- Return `()` (unit)?
- Return values (like `close(db) -> Summary`)?
- Be selected from multiple options (Higher RAII)?

### 4. Context Splitting in Type Rules

Functions consuming linear arguments must split contexts:
```
Γ ⊢ f : (!A, B) → C    Δ₁ ⊢ e₁ : !A    Δ₂ ⊢ e₂ : B
───────────────────────────────────────────────────────
Γ, Δ₁, Δ₂ ⊢ f(e1, e2) : C
[where Δ₁ ∩ Δ₂ = ∅ for linear variables]
```

### 5. Borrowing (Optional but Recommended)

Temporary access without consuming:
```
read_borrow: &!T → U      // immutable borrow
write_borrow: &mut !T → U // mutable borrow
```

Rust-style: Borrows must not outlive owner.
Mercury-style: Modes like `di` (consumes) vs `ui` (preserves uniqueness).

### 6. Copy vs. Linear Separation

- Primitives (int, bool, float): copy
- Compound types containing linear fields: linear
- Explicit `Copy` trait/marker prevents `Drop`

### 7. Error Handling Integration

With your `!` for errors and `error` type:
```
fn open(path: string) -> !(File, error)
// Returns linear File or error
// Both branches must handle the linear result
```

### 8. Collections and Linearity

Key questions:
- Is `[!T]` a list of linear values?
- How to iterate without consuming?
- Ownership transfer: `list.pop() -> !T` moves out

### 9. Type System Integration

From your `typing-rules.md`:
- Add linear qualifier to your heap syntax: `@!T` (linear local), `#!T` (linear global)?
- Synthesis vs. checking: Linear types typically require checking mode
- Coercions: Never implicitly copy linear types

### 10. Scope and Lifetime

Linear values must be consumed before scope ends:
```
{
    let x: !Database = connect();
    // ... use x ...
}  // ERROR if x not explicitly destroyed
```

## Design Patterns Enabled

**Higher RAII** (from Vale):
1. **Cache invalidation**: Linear tokens force cache updates when primary data changes
2. **Promise/future resolution**: Linear futures prevent forgotten result handling
3. **Transaction commit/rollback**: Linear transactions must be explicitly resolved
4. **Message handling**: Linear messages ensure processing guarantees
5. **State machine transitions**: Prevent incomplete state changes via linear intermediates
6. **Memory leak prevention**: Force proper cleanup in collectionized structures
7. **Decision enforcement**: Guarantee choices are made (e.g., must commit or rollback)

**Resource Safety**:
- File handles must be closed
- Database connections can't be used after close
- Network sockets properly shut down
- Memory explicitly freed

**Concurrency Safety**:
- Thread messages consumed exactly once
- Channel endpoints tracked
- No shared mutable state without explicit handling

## Implementation Considerations

### 1. Type Checking Algorithm

Bidirectional typing with linear context:
```rust
struct LinearCtx {
    linear_vars: HashMap<Var, Type>,  // must use exactly once
    unrestricted_vars: HashMap<Var, Type>,  // use any times
}

fn check_linear(ctx: &mut LinearCtx, expr: Expr) {
    // Track which linear vars are used
    // Error if used twice or not used
}
```

### 2. Move Semantics

After `let y = x` where `x: !T`:
- Mark `x` as moved in context
- Error on subsequent `x` uses
- Similar to Rust's move checker

### 3. Destructor Calls

Options:
- Explicit method call: `x.destroy()`
- Generic function: `drop(x)`, `close(x)`
- Pattern matching that consumes: `match x { ... }`
- Conversion to destructor type

### 4. Error Messages

Critical for usability:
```
error: linear value `db` not used
  --> main.dl:5:9
   |
5  |     let db = connect();
   |         ^^ value must be explicitly destroyed
   |
help: call `db.close()` before end of scope
```

## Recommendations for Datalove

Given your project context (from README.md and typing-rules.md):

### 1. Start with affine types (like Rust)

- Easier to implement than strict linear
- More forgiving for users
- Can be tightened to linear later

### 2. Explicit destructor syntax

```datalove
x.destroy()  // or
drop x       // or
consuming x  // your choice
```

### 3. Integrate with your heap model

```datalove
@!File    // linear type on local heap
#!Buffer  // linear type on global heap
```

### 4. Type qualifier syntax

```datalove
: !Connection / connect("localhost")
let conn: !Connection = connect("localhost")
```

### 5. Simple borrowing (defer complex lifetimes)

```datalove
&!T for immutable borrow (read-only)
&mut !T for mutable borrow
```

### 6. Start with function-level linearity

- Don't need region polymorphism initially
- Function parameters clearly marked linear or not
- Return values similarly marked

### 7. Integration with existing type system

Extend your bidirectional typing from `typing-rules.md`:

```
Rule: Check-Linear
e ⇐ !T
Linear context tracks e as consumed
────────────────────────────────────
e must be explicitly destroyed before scope ends

Rule: Syn-Destructor
x has type !T
destructor(x) is valid destructor for !T
────────────────────────────────────
destructor(x) ⇒ ReturnType
x marked as consumed in linear context
```

### 8. Conflict resolution with error syntax

Current: `@!T` means Result type
Proposed: `!T` means linear type

Consider:
- Option A: `lin T` or `linear T` for linear types
- Option B: `@~T` or similar for linear types
- Option C: Keep `!T` for linear, use `?!T` for linear result types
- Option D: Use `^T` (like Vale) for owned/linear types

## Further Reading

### Academic Papers
- Girard (1987): "Linear Logic" - foundational work
- Dunfield & Krishnaswami (2013): "Complete and Easy Bidirectional Typechecking for Higher-Rank Polymorphism"
- Walker (2005): "Substructural Type Systems"

### Language Documentation
- Rust: https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html
- Austral: https://austral-lang.org/linear-types
- Vale: https://vale.dev/linear-aliasing-model
- Mercury: https://www.mercurylang.org/information/doc-latest/mercury_ref/Modes.html
- Futhark: https://futhark-lang.org/blog/2022-06-13-uniqueness-types.html

### Articles and Blog Posts
- "Higher RAII, and the Seven Arcane Uses of Linear Types" (Verdagon): https://verdagon.dev/blog/higher-raii-uses-linear-types
- "Ownership" (Without Boats): https://without.boats/blog/ownership/
- "Linear types for programmers" (Twey): https://twey.io/for-programmers/linear-types/

## Summary

Linear and affine type systems provide powerful guarantees for resource management without garbage collection. The research strongly suggests that combining affine types with explicit destructors (like the Vale/Austral approach) provides the best balance of safety and usability for a new language like Datalove.

Key takeaway: Start simple with affine types and explicit destructors, integrate cleanly with your existing type system and heap model, and expand capabilities incrementally based on user needs.
