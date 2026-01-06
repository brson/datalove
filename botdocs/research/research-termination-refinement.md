# Research: Termination Detection and Refinement Types for Datafun

Research on how termination detection and refinement types might integrate with
datafun's restricted type system: linear types, pure functions, no exceptions,
loop induction variables.

## Termination Detection

### Background

Termination analysis determines whether a program halts for all inputs. While
undecidable in general (halting problem), restricted languages admit decidable
fragments.

Key techniques:

1. **Ranking Functions**: Find a function from program states to a well-founded
   set that strictly decreases on each step. Proving such a function exists
   guarantees termination.

2. **Size-Change Termination (SCT)**: Track how data values change across
   function calls. If every infinite call sequence causes infinite descent on
   some well-founded data, the program terminates. This is decidable
   (PSPACE-complete) and works well for functional languages.

3. **Structural Recursion** (Agda/Idris approach): Accept only recursive calls
   on strict subexpressions of arguments. No separate ranking function needed -
   the data structure itself serves as the measure.

### Why Datafun is Well-Suited

Datafun has properties that make termination analysis more tractable:

1. **Explicit Loop State**: Loop carries make iteration state explicit:
   ```datalove
   loop carry (i = n)
     if i <= @0
       break
     end if
     continue(i - @1)
   end loop
   ```
   The carry `i` is the only state that changes - no hidden mutations.

2. **Pure Functions**: No side effects means function behavior depends only on
   inputs. No global state to track.

3. **Linear Types**: Each value is used exactly once (unless Copy). This
   constrains aliasing and simplifies reasoning about value flow.

4. **Total Ordering on All Data**: All pure data types support total order.
   This provides well-founded orderings for ranking functions.

5. **No Exceptions**: Control flow is explicit - no hidden early exits.

6. **SSA-Based IR**: The CFG representation maps cleanly to termination analysis
   frameworks that work on control flow graphs.

### Possible Approaches for Datafun

#### Approach 1: Size-Change on Carries

Analyze loop carries for size-change termination:

```datalove
// TERMINATES: i strictly decreases each iteration
loop carry (i = n)
  if i <= @0; break; end if
  continue(i - @1)
end loop
```

The size-change graph shows `i` decreases on every `continue`. Combined with
the base case check, this proves termination.

For this we need:
- Define "size" for each type (structural size, numeric value, etc.)
- Build size-change graphs from CFG edges
- Check the SCT condition: every infinite path has infinite descent

#### Approach 2: Structural Recursion for Functions

Require recursive calls to operate on structurally smaller arguments:

```datalove
fun length(xs: [int]): int
  if xs.is_empty()
    ret 0
  end if
  let (_, tail) = xs.split_first()!
  ret 1 + length(tail)  // tail is strict substructure of xs
end fun
```

This is the Agda/Idris approach. Reject functions where recursive calls aren't
on subexpressions.

#### Approach 3: Bounded Loops with Refinement Types

Combine with refinement types (see below) to express loop bounds:

```datalove
fun sum_to(n: {n: u32 | n <= 1000}): u32
  loop carry (i: u32 = @0, acc: u32 = @0)
    if i >= n
      break(acc)
    end if
    continue(i +! @1, acc +! i)
  end loop bring (result: u32)
  ret result
end fun
```

The refinement `n <= 1000` bounds the iteration count.

#### Approach 4: Totality Annotations

Like Idris, allow optional totality assertions:

```datalove
@total
fun factorial(n: u32): u32
  if n <= @1
    ret @1
  end if
  ret n * factorial(n - @1)  // n decreases
end fun
```

The `@total` annotation triggers termination checking. Functions without it
may be partial (non-terminating).

### Recommended Direction

Given datafun's design:

1. **Start with carry analysis**: The explicit carry mechanism is a natural
   fit for size-change analysis. Implement SCT on loop carries first.

2. **Require decreasing carries for totality**: Loops marked `@total` must
   have at least one carry that strictly decreases on every `continue` path.

3. **Leverage well-founded orderings**: All datafun types have total order.
   Use this for ranking functions on non-integer carries.

4. **Integrate with functions**: Extend to recursive functions using the same
   size-change framework. Track parameter sizes across call edges.

### Example: Provably Terminating Loop

```datalove
@total
fun count_down(n: u32): u32
  loop carry (i = n, sum: u32 = @0)  // i is the "fuel"
    if i == @0
      break(sum)
    end if
    continue(i - @1, sum +! i)  // i strictly decreases
  end loop bring (result: u32)
  ret result
end fun
```

Termination proof:
- `i` starts at `n` (finite)
- Each `continue` decreases `i` by 1
- Loop exits when `i == 0`
- Therefore: at most `n` iterations

---

## Refinement Types

### Background

Refinement types enrich base types with logical predicates:

```
{v : T | P(v)}  -- values of type T satisfying predicate P
```

Liquid Types automate refinement inference using SMT solvers and predicate
abstraction. The key insight: if predicates come from a decidable logic,
type checking remains decidable.

### Why Datafun is Well-Suited

1. **Existing Type Hints**: The `: type / expr` syntax already provides
   explicit type context. Extending to refinements is natural.

2. **Checked Arithmetic**: Operators like `/!` and `/?` already express
   preconditions (non-zero divisor) and postconditions (overflow handling).
   Refinements could express these statically.

3. **No Subtyping Complications**: Structural types with explicit coercions
   simplify refinement subtyping.

4. **Total Order**: All types have decidable equality and ordering, which
   are the predicates SMT solvers handle best.

5. **Pure Functions**: No aliasing or mutation to track - predicates can
   focus on values.

### Possible Syntax

Building on datafun's existing syntax:

```datalove
// Refined type with predicate
: {v: u32 | v != 0} / divisor

// Function with refined parameters
fun safe_div(a: u32, b: {b: u32 | b != 0}): u32
  ret a / b  // safe: b guaranteed non-zero
end fun

// Refined return type
fun absolute(x: i32): {r: u32 | r >= 0}
  if x >= @0
    ret : u32 / x
  else
    ret : u32 / (-x)
  end if
end fun
```

Alternative sigil-based syntax (more concise):

```datalove
// Using & for refinement predicates
fun safe_div(a: u32, b: u32 & != 0): u32
  ret a / b
end fun

// Or dedicated keyword
fun safe_div(a: u32, b: u32 where b != 0): u32
  ret a / b
end fun
```

### What Could Be Verified

1. **Division by Zero**:
   ```datalove
   fun div(a: int, b: {b: int | b != 0}): int
     ret a / b  // statically safe
   end fun
   ```

2. **Array Bounds**:
   ```datalove
   fun get(arr: [T], i: {i: u32 | i < arr.len()}): T
     ret arr[i]  // in bounds
   end fun
   ```

3. **Integer Ranges**:
   ```datalove
   fun to_u8(n: {n: u32 | n <= 255}): u8
     ret : u8 / n  // no truncation
   end fun
   ```

4. **Non-Empty Collections**:
   ```datalove
   fun first(xs: {xs: [T] | xs.len() > 0}): T
     ret xs[0]  // safe
   end fun
   ```

5. **Option Elimination**:
   ```datalove
   fun unwrap(opt: {opt: ?T | opt.is_some()}): T
     // guaranteed to be Some
   end fun
   ```

### Integration with Existing Features

**With Checked Arithmetic**:

Currently:
```datalove
fun div_checked(a: u32, b: u32): !u32
  ret a /! b  // early-returns error if b == 0
end fun
```

With refinements:
```datalove
fun div_safe(a: u32, b: {b: u32 | b != 0}): u32
  ret a / b  // no runtime check needed
end fun
```

**With Linear Types**:

Refinements could express resource states:
```datalove
// Hypothetical: track file state
fun read(f: {f: File | f.is_open()}): (File, string)
  // guaranteed open
end fun
```

**With Loop Carries**:

Refinements could express loop invariants:
```datalove
loop carry (i: {i: u32 | i <= n} = @0)
  // invariant: i always <= n
  if i >= n; break; end if
  continue(i + @1)  // maintain invariant
end loop
```

### Implementation Considerations

1. **Predicate Language**: Restrict to decidable fragments:
   - Linear integer arithmetic (Presburger arithmetic)
   - Equality and ordering on all types
   - Boolean connectives
   - No quantifiers initially

2. **SMT Backend**: Use Z3 or similar for constraint solving.

3. **Inference**: Liquid type inference could infer many refinements
   automatically from usage context.

4. **Subtyping**: `{v: T | P}` is subtype of `{v: T | Q}` when `P => Q`.
   This requires SMT queries.

5. **Error Messages**: Show counter-examples when refinement checks fail.

### Recommended Direction

1. **Start with explicit refinements**: Require programmers to write
   predicates initially. Don't attempt inference yet.

2. **Focus on numeric predicates**: `==`, `!=`, `<`, `<=`, `>`, `>=`,
   `+`, `-`, `*` on integers. These are decidable via SMT.

3. **Integrate with existing operators**: Make `/!` and `/?` unnecessary
   when refinements guarantee safety.

4. **Gradual adoption**: Refinements are optional. Unconstrained types
   are `{v: T | true}`.

### Example: Bounded Vector Access

```datalove
// Define refined index type
typealias BoundedIndex(n: u32): {i: u32 | i < n}

// Safe vector access
fun get<T>(vec: [T], idx: BoundedIndex(vec.len())): T
  ret vec[idx]  // bounds check eliminated
end fun

// Usage requires proving bounds
fun first_or_default<T>(vec: [T], default: T): T
  if vec.len() > @0
    // Here vec.len() > 0, so 0 < vec.len()
    // Therefore @0 : BoundedIndex(vec.len())
    ret get(vec, @0)
  else
    ret default
  end if
end fun
```

---

## Synergies: Termination + Refinement Types

The two features reinforce each other:

1. **Bounded Recursion**: Refinements can express the "fuel" for termination:
   ```datalove
   @total
   fun fib(n: {n: u32 | n <= 40}): u64
     // Bounded input guarantees termination
   end fun
   ```

2. **Loop Bounds from Refinements**: If a carry has a refinement bound,
   termination follows:
   ```datalove
   loop carry (i: {i: u32 | i <= n} = @0)
     if i >= n; break; end if
     continue(i + @1)
   end loop
   // Terminates: i bounded by n, increases each iteration
   ```

3. **Termination as Refinement**: Could express termination as a type-level
   property:
   ```datalove
   // Function type with termination guarantee
   : (u32 -> u32) & total
   ```

4. **Resource Bounds**: Refinements could bound resource usage (time/space),
   which implies termination:
   ```datalove
   @time(O(n))
   fun linear_search(arr: [T], target: T): ?u32
     // Must terminate in O(n) time
   end fun
   ```

---

## Summary

Datafun's restricted semantics make both features more tractable:

| Property | Termination Benefit | Refinement Benefit |
|----------|--------------------|--------------------|
| Linear types | Simpler alias analysis | Simpler predicate tracking |
| Pure functions | No hidden state | Predicates depend only on inputs |
| Explicit carries | Direct iteration state | Natural loop invariants |
| No exceptions | No hidden control flow | No exception paths to model |
| Total ordering | Well-founded measures | Decidable comparisons |
| SSA IR | CFG-based analysis | SSA simplifies data flow |

**Recommended prioritization**:

1. Termination checking for loops via size-change on carries
2. Simple refinement predicates (numeric constraints)
3. Integration between the two (bounds => termination)
4. Inference and more complex predicates

---

## Sources

- [Termination Competition 2025](https://termination-portal.org/wiki/Termination_Competition_2025)
- [Proving Program Termination (CACM)](https://cacm.acm.org/research/proving-program-termination/)
- [Size-Change Termination Principle](https://www.semanticscholar.org/paper/The-size-change-principle-for-program-termination-Lee-Jones/ab8e798b8cf4b6ddc2bc80e5cdabbf6b9df14b0d)
- [Agda Termination Checking](https://agda.readthedocs.io/en/latest/language/termination-checking.html)
- [Idris Totality Checking](https://docs.idris-lang.org/en/latest/faq/faq.html)
- [LiquidHaskell Tutorial](https://ucsd-progsys.github.io/liquidhaskell-tutorial/Tutorial_01_Introduction.html)
- [Usability Barriers for Liquid Types (PLDI 2025)](https://dl.acm.org/doi/10.1145/3729327)
- [Generic Refinement Types](https://dl.acm.org/doi/10.1145/3704885)
- [Refinement-Types Driven Development (2025)](https://arxiv.org/abs/2509.15005)
