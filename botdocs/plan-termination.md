# Design: Termination Checking for Datafun

## Formal Foundation: Size-Change Termination

**Principle**: A program terminates if every infinite execution path would cause infinite descent in some well-founded data values.

**Key insight**: We don't need to find an explicit ranking function. Instead:
1. Build *size-change graphs* showing how values relate across transitions
2. Check that every infinite path has a "thread" that descends infinitely
3. This is decidable (PSPACE-complete, but practical for typical programs)

### Size-Change Graphs

A size-change graph for a transition (e.g., loop iteration, function call) maps source parameters to destination parameters with edges labeled:
- `↓` (strict decrease): destination < source
- `≤` (non-increase): destination ≤ source

Example for `continue(i - 1, acc + i)`:
```
source: (i, acc)
dest:   (i', acc')

Edges:
  i  --↓--> i'    (i - 1 < i)
  acc --?--> acc' (unknown, acc + i could be anything)
```

### The SCT Algorithm

1. **Build graphs**: For each `continue` (loop) or recursive call (function), create a size-change graph
2. **Compute closure**: Compose graphs to get all possible multi-step transitions
3. **Check idempotents**: For each idempotent graph G (where G∘G = G), verify it has at least one `↓` edge on the diagonal (same variable to itself)

If all idempotents have descending self-loops, the program terminates.

## Mapping to Datafun

### Loops with Carries

Datafun's explicit carries are ideal for SCT:

```datalove
loop carry (i = n, acc = @0)
  if i == @0; break; end if
  continue(i - @1, acc + i)
end loop
```

**Size-change graph for this continue**:
- `i → i'`: strict decrease (↓)
- `acc → acc'`: unknown
- `i → acc'`: unknown
- `acc → i'`: unknown

Since `i` strictly decreases to itself, termination is proven.

### Multiple Continue Paths

```datalove
loop carry (i = n)
  if condition1
    continue(i - @1)  // Graph G1: i --↓--> i'
  else if condition2
    continue(i - @2)  // Graph G2: i --↓--> i'
  else
    break
  end if
end loop
```

Both graphs have `i` decreasing, so all paths terminate.

### Lexicographic Ordering

```datalove
loop carry (x = m, y = n)
  if x == @0 && y == @0; break; end if
  if y == @0
    continue(x - @1, n)  // G1: x↓, y unknown
  else
    continue(x, y - @1)  // G2: x≤, y↓
  end if
end loop
```

Closure analysis: G1∘G2, G2∘G1, G1∘G1, G2∘G2 all have descending threads.

### Recursive Functions

```datalove
fun fib(n: int): int
  if n <= @1; ret n; end if
  ret fib(n - @1) + fib(n - @2)
end fun
```

Size-change graphs for calls:
- Call 1: `n → n'` with ↓ (n-1 < n)
- Call 2: `n → n'` with ↓ (n-2 < n)

Both have descending `n`, so termination is proven.

## Size Measures by Type

| Type | Size Measure | Well-Founded |
|------|-------------|--------------|
| `int`, `u32`, etc. | Numeric value | Yes (for non-negative) |
| `[T]` (list) | Length | Yes |
| `{T}` (set) | Cardinality | Yes |
| `(A, B)` (tuple) | Lexicographic on components | Yes |
| `?T` (option) | 0 for none, 1+size(v) for some(v) | Yes |
| Structs | Lexicographic on fields | Yes |

## Algorithm Design

### Phase 1: Extract Transitions

For each loop/function:
1. Find all `continue` statements (loops) or recursive calls (functions)
2. For each, build a size-change graph relating input carries/params to output values

### Phase 2: Classify Edges

For each edge (source_var → dest_expr):
1. **Strict decrease (↓)**: `source - positive_const`, `tail(source)`, etc.
2. **Non-increase (≤)**: `source` unchanged, `source` with monotonic op
3. **Unknown (?)**: anything else

### Phase 3: Compute Closure

Compose graphs transitively until fixed point:
```
cl(G) = G ∪ { G1 ∘ G2 | G1, G2 ∈ cl(G) }
```

Graph composition: edge from x to z exists if path x→y→z exists, with:
- ↓ if either edge is ↓
- ≤ if both edges are ≤
- ? otherwise

### Phase 4: Check SCT Condition

For each idempotent G in closure (where G ∘ G = G):
- Must have at least one edge `v --↓--> v` (same variable)
- If any idempotent lacks such an edge, termination unproven

## Data Structures

```rust
/// Edge label in size-change graph.
enum SizeChange {
    Decrease,   // ↓ strictly smaller
    NonIncrease, // ≤ same or smaller
    Unknown,    // ? no information
}

/// Size-change graph for one transition.
struct SizeChangeGraph {
    /// Source variables (carries for loops, params for functions).
    sources: Vec<String>,
    /// Destination variables.
    destinations: Vec<String>,
    /// Edges: (source_idx, dest_idx) -> label.
    edges: HashMap<(usize, usize), SizeChange>,
}

/// Analysis result for a loop or function.
enum TerminationResult {
    Terminates { evidence: String },
    Unknown { reason: String },
}
```

## Implementation Plan

### Step 1: Improve Edge Detection

Current: only detects `i - constant`.

Add:
- `tail(list)`, `rest(list)` → list shrinks
- `x - y` where y > 0 known → decrease
- `min(x, y)` where x or y decreases → decrease
- Pass-through: `x` unchanged → non-increase

### Step 2: Build Proper Graphs

Current: per-continue pattern matching.

Change to:
- Build full SizeChangeGraph for each continue
- Track all carries, not just first match

### Step 3: Implement Closure

Add graph composition and fixed-point computation.

### Step 4: Check Idempotents

Find idempotent graphs in closure, verify SCT condition.

### Step 5: Extend to Functions

Apply same framework to recursive function calls.

## Deferred: Integration with Refinement Types

Future work: use refinement predicates to strengthen size analysis.

Example: if `n: {n: u32 | n <= 100}`, then loop with `i = n` iterating down has bounded iterations even without explicit decrease detection.

## Sources

- [Agda Termination Checking](https://agda.readthedocs.io/en/latest/language/termination-checking.html)
- [Size-Change Termination (Lee, Jones, Ben-Amram 2001)](https://dl.acm.org/doi/10.1145/360204.360210)
- [SCT Survey (Université de la Réunion)](https://lim.univ-reunion.fr/staff/fred/Enseignement/Term-Cours/lecture7-SCP.pdf)
