# Logic Programming for Datalove: Funs as Predicates

**Date**: 2025-10-12
**Topic**: Exploring logic programming features for datalove funs with argument modes

## Executive Summary

This document explores how datalove funs with argument modes could support logic programming features, particularly **backtracking** and **multiple solutions**. Your intuition is correct: funs with controlled argument aliasing, operating on pure data, have similar expressive power to Mercury predicates.

**Key Insights**:
- Logic programming = relations that can run in multiple directions
- Backtracking = systematically exploring multiple solutions
- Modes + pure data = natural foundation for logic features
- Memoization makes repeated backtracking efficient
- Fits perfectly with undo/redo and replay debugging

## 1. Logic Programming 101

### What is Logic Programming?

**In one sentence**: Logic programming lets you describe *relationships* (what is true) rather than *procedures* (how to compute), and the system figures out how to find solutions.

**Key Difference from Functional Programming**:

| Functional | Logic |
|------------|-------|
| Functions: `input → output` | Relations: `value1 ↔ value2` |
| One direction | Multiple directions |
| How to compute | What is true |
| One solution | Multiple solutions |

### Simple Example: Parent Relationship

**Functional (one direction)**:
```python
def children_of(parent):
    if parent ≡ "Alice": return ["Bob", "Carol"]
    if parent ≡ "Bob": return ["Dave"]
    return []
```

**Logic (bidirectional)**:
```prolog
parent(alice, bob).
parent(alice, carol).
parent(bob, dave).

% Query: Who are Alice's children?
?- parent(alice, X).
X = bob ;
X = carol.

% Query: Who is Bob's parent?
?- parent(X, bob).
X = alice.
```

The **same relation** works in different directions!

### How Backtracking Works

When a logic system searches for solutions:

1. **Choice point**: When multiple options exist, pick one
2. **Continue**: Try to complete the solution with that choice
3. **Success**: Found a solution! (can ask for more)
4. **Failure**: This choice didn't work, **backtrack** to last choice point
5. **Try next**: Try the next alternative at that choice point
6. **Repeat**: Keep going until all possibilities exhausted

**Example: List Splitting**

```prolog
% append(L1, L2, L3) means L1 + L2 = L3
append([], L, L).
append([H|T1], L2, [H|T3]) :- append(T1, L2, T3).

% Query: Split [1,2,3] into two lists
?- append(X, Y, [1,2,3]).
X = [], Y = [1,2,3] ;
X = [1], Y = [2,3] ;
X = [1,2], Y = [3] ;
X = [1,2,3], Y = [].
```

Backtracking explores all possible splits!

## 2. Mercury's Approach

Mercury adds **modes** and **determinism** to logic programming.

### Determinism Categories

| Category | Can Fail? | Solutions | Example |
|----------|-----------|-----------|---------|
| `det` | No | Exactly 1 | `2 + 2 = 4` |
| `semidet` | Yes | 0 or 1 | `x > 5` (test) |
| `multi` | No | 1 or more | Generate all splits |
| `nondet` | Yes | 0 or more | Search with failure |
| `failure` | Always | 0 | Always fails |

### Modes Specify Direction

Same predicate, different modes:

```mercury
% Type declaration
:- pred append(list(T), list(T), list(T)).

% Mode 1: Concatenate two known lists
:- mode append(in, in, out) is det.
% Usage: append([1,2], [3,4], X) => X = [1,2,3,4]

% Mode 2: Split a known list
:- mode append(out, out, in) is multi.
% Usage: append(X, Y, [1,2,3]) => multiple solutions!

% Mode 3: Check if a split is valid
:- mode append(in, in, in) is semidet.
% Usage: append([1], [2,3], [1,2,3]) => succeeds
```

The compiler **generates different code** for each mode!

### Concrete Example: Parent/Child

```mercury
% Facts: parent(Parent, Child)
:- pred parent(person, person).

% Different modes, different determinism
:- mode parent(in, in) is semidet.   % Test: Is X a parent of Y?
:- mode parent(in, out) is nondet.   % Query: Who are X's children?
:- mode parent(out, in) is nondet.   % Query: Who are Y's parents?
:- mode parent(out, out) is multi.   % Generate: All parent-child pairs

% Facts
parent(alice, bob).
parent(alice, carol).
parent(bob, dave).

% Usage examples:
% parent(alice, bob)           => succeeds (semidet)
% parent(alice, X)             => X = bob ; X = carol (nondet)
% parent(X, bob)               => X = alice (nondet)
% parent(X, Y)                 => all pairs (multi)
```

### Collecting Solutions

Mercury provides a `solutions` module to collect multiple solutions:

```mercury
:- import_module solutions.

% Collect all children of Alice
main(!IO) :-
    solutions(
        (pred(Child::out) is nondet :- parent(alice, Child)),
        Children
    ),
    % Children = [bob, carol]
    io.write_line(Children, !IO).
```

### Aggregate Over Solutions

```mercury
% Count number of children
count_children(Parent, Count) :-
    aggregate(
        (pred(Child::out) is nondet :- parent(Parent, Child)),
        (pred(_Child::in, Acc0::in, Acc::out) is det :- Acc = Acc0 + 1),
        0,
        Count
    ).
```

## 3. What Backtracking Looks Like

### Multi-Determinism Example

```mercury
% Generate all ways to split a list
:- pred split(list(T)::in, list(T)::out, list(T)::out) is multi.

split(List, Left, Right) :-
    append(Left, Right, List).

% Usage:
split([1,2,3], L, R)
% Solution 1: L = [],      R = [1,2,3]
% Solution 2: L = [1],     R = [2,3]
% Solution 3: L = [1,2],   R = [3]
% Solution 4: L = [1,2,3], R = []
```

### Search Example: N-Queens

```mercury
% Place N queens on an NxN chessboard (no attacks)
:- pred queens(int::in, list(int)::out) is nondet.

queens(N, Queens) :-
    range(1, N, Rows),
    permutation(Rows, Queens),  % Try different arrangements (backtracking!)
    safe(Queens).               % Check if placement is valid

% safe/1 checks no queens attack each other
% permutation/2 is nondet - generates all permutations
```

The system **automatically backtracks** when `safe` fails, trying the next permutation.

### Interfacing with Det Code

When calling `multi`/`nondet` from deterministic code:

**Option 1: Take first solution**
```mercury
:- pred first_child(person::in, person::out) is semidet.

first_child(Parent, Child) :-
    promise_equivalent_solutions [Child] (
        parent(Parent, Child)
    ).
```

**Option 2: Collect all solutions**
```mercury
:- pred all_children(person::in, list(person)::out) is det.

all_children(Parent, Children) :-
    solutions(
        (pred(C::out) is nondet :- parent(Parent, C)),
        Children
    ).
```

## 4. Datalove Funs as Predicates

Your intuition is spot-on! Datalove funs with modes can support logic programming features.

### Current Foundation

From your design:
- **Funs**: Pure functions operating on owned, pure-data, non-cyclic values
- **Argument modes**: `in`, `out`, `di`, `uo` (like Mercury!)
- **Controlled aliasing**: Ownership and uniqueness tracking
- **Pure data**: No side effects, no cycles

This is **perfect** for logic programming!

### Key Insight: Funs vs Predicates

| Feature | Fun | Predicate |
|---------|-----|-----------|
| Return type | Exactly one value | 0+ solutions |
| Modes | Specify ownership | Specify + direction |
| Determinism | Always `det` | `det`/`semidet`/`multi`/`nondet` |
| Backtracking | No | Yes |

**Proposal**: A fun with `multi` or `nondet` determinism is essentially a **generator**.

### Syntax Proposal

Building on your existing syntax:

```rust
// Current: Deterministic fun
fun add(x: @u32 ^in, y: @u32 ^in) -> @u32 ^out

// Proposed: Multi-deterministic fun (generator)
fun split(list: [@T] ^in) -> ([@T] ^out, [@T] ^out) ^multi

// Alternative syntax: Use determinism as attribute
#[multi]
fun split(list: [@T] ^in) -> ([@T] ^out, [@T] ^out)
```

Where `^multi` means "yields multiple solutions".

### Implementation: Generators/Iterators

A `multi` fun is essentially a **generator** (iterator/stream):

```rust
// Conceptually compiles to:
fn split<T>(list: Vec<T>) -> impl Iterator<Item = (Vec<T>, Vec<T>)> {
    (0..=list.len()).map(|i| list.split_at(i))
}
```

### Example: Append with Multiple Modes

```rust
// Type signature with multiple mode declarations
fun append<T>(
    left: [@T],
    right: [@T],
    result: [@T]
)

// Mode 1: Concatenate (deterministic)
mode append(^in, ^in, ^out) -> ^det

// Mode 2: Split (multi-deterministic)
mode append(^out, ^out, ^in) -> ^multi

// Mode 3: Check (semi-deterministic)
mode append(^in, ^in, ^in) -> ^semidet

// Implementation for mode 1 (in, in, out)
impl append_concat {
    fun body(left: [@T] ^in, right: [@T] ^in) -> [@T] ^out {
        // Concatenate: result = left + right
        list_concat(left, right)
    }
}

// Implementation for mode 2 (out, out, in)
impl append_split {
    #[multi]
    fun body(result: [@T] ^in) -> ([@T] ^out, [@T] ^out) {
        // Generate all splits
        for i in @0..=result.len() {
            yield result.split_at(i)
        }
    }
}

// Implementation for mode 3 (in, in, in)
impl append_check {
    fun body(left: [@T] ^in, right: [@T] ^in, expected: [@T] ^in) -> @bool {
        // Check: left + right ≡ expected
        list_concat(left, right) ≡ expected
    }
}
```

### Example: Parent Relation

```rust
// Define a relation as a fun with multiple modes
fun parent(p: Person, c: Person)

// Mode 1: Check if relationship exists
mode parent(^in, ^in) -> ^semidet {
    // Returns @?() - Some if true, none if false
    lookup_db(p, c)
}

// Mode 2: Find children
mode parent(^in, ^out) -> ^multi {
    // Yields multiple children
    for child in children_of(p) {
        yield child
    }
}

// Mode 3: Find parents
mode parent(^out, ^in) -> ^multi {
    // Yields multiple parents
    for par in parents_of(c) {
        yield par
    }
}

// Mode 4: Generate all pairs
mode parent(^out, ^out) -> ^multi {
    for (p, c) in all_pairs() {
        yield (p, c)
    }
}
```

## 5. Connection to Your Use Cases

Your interest in logic programming aligns **perfectly** with your other goals!

### 5.1 Undo/Redo

Logic programming's backtracking is **built-in undo**!

```rust
// Command pattern with undo via backtracking
fun edit_operation(
    old_state: State ^in,
    new_state: State ^out
) -> ^multi

// Forward: old_state -> new_state (apply edit)
mode edit(^in, ^out) -> ^det

// Backward: new_state -> old_state (undo edit)
mode edit(^out, ^in) -> ^multi  // May have multiple undo paths!

// Example: Text editing
fun insert_char(
    text: @string ^in,
    pos: @u32 ^in,
    char: @char ^in
) -> @string ^out

// Undo mode: Remove char
mode insert_char(
    text: @string ^out,    // Before state
    pos: @u32 ^in,         // Known position
    char: @char ^out       // Which char was there?
) -> ^multi
```

**Key insight**: Backtracking naturally explores undo paths!

### 5.2 Memoization

Pure data + deterministic results = **perfect memoization**!

```rust
// Memoize a multi fun
#[memoized]
#[multi]
fun split(list: [@T] ^in) -> ([@T] ^out, [@T] ^out)

// First call: Compute and cache all solutions
split([1,2,3])
// => Yields ([], [1,2,3]), ([1], [2,3]), ...
// => Caches: [1,2,3] -> [([], [1,2,3]), ([1], [2,3]), ...]

// Second call: Return cached solutions
split([1,2,3])
// => Instant! Return cached iterator
```

**Benefits**:
- Multi funs return deterministic solution sets
- Pure data means results never change
- Cache hit = instant replay of all solutions

### 5.3 Checkpoint/Replay Debugging

Logic programming with pure data enables **deterministic replay**!

**Scenario**: Debug a complex search

```rust
// Record all choice points during execution
#[record_choices]
#[multi]
fun search(space: SearchSpace ^in) -> Solution ^out {
    for candidate in generate_candidates(space) {
        if is_valid(candidate) {
            yield candidate
        }
    }
}

// Later: Replay with same inputs
#[replay_from_checkpoint]
fun debug_search(space: SearchSpace ^in, checkpoint: Checkpoint ^in) {
    // Deterministic replay - same choices, same order
    // Can inspect state at any choice point
}
```

**Recording backtracking**:
```rust
// Trace structure
struct BacktraceLog {
    choices: [@{
        choice_point: @u32,
        alternative: @u32,
        input_state: State,
        output_state: @?State
    }]
}

// Each backtrack is recorded
// Replay by following the same sequence
```

### 5.4 Record-Replay for Testing

```rust
// Record a REPL session with nondeterministic operations
struct REPLSession {
    commands: [Command],
    choices: BacktraceLog  // Records which solutions were taken
}

// Replay exactly
fun replay_session(session: REPLSession ^in) {
    for (cmd, choice) in zip(session.commands, session.choices) {
        execute_with_choice(cmd, choice)
    }
}
```

## 6. Incremental Feature Roadmap

Start simple, build up to full backtracking.

### Phase 1: Multiple Return Values (Deterministic Iterator)

**Features**:
- Fun can return multiple values (generator/iterator)
- But still deterministic - same input always yields same sequence
- No backtracking yet

**Syntax**:
```rust
#[yields]  // Or use generator syntax
fun range(start: @u32 ^in, end: @u32 ^in) -> @u32 {
    for i in start..end {
        yield i
    }
}

// Usage
for x in range(@0, @10) {
    print(x)
}
```

**Implementation**: Compile to Rust generators/iterators.

**Use cases**:
- Lazy sequences
- Stream processing
- Efficient iteration

### Phase 2: Semidet Functions (Optional Return)

**Features**:
- Fun may or may not return a value
- Returns `@?T` (option type)
- Natural extension of your existing `@?` type

**Syntax**:
```rust
fun find(list: [@T] ^in, predicate: (T -> @bool) ^in) -> @?T {
    for item in list {
        if predicate(item) {
            return @some(item)
        }
    }
    @none  // Failed to find
}
```

**Determinism**: `semidet` - 0 or 1 solution.

### Phase 3: Multi Functions (Generator)

**Features**:
- Fun yields multiple solutions
- Still deterministic - same inputs, same solution sequence
- No choice points or backtracking yet

**Syntax**:
```rust
#[multi]
fun split(list: [@T] ^in) -> ([@T], [@T]) {
    for i in @0..=list.len() {
        yield list.split_at(i)
    }
}

// Collect all solutions
let all_splits = split([1,2,3]).collect()
// => [([], [1,2,3]), ([1], [2,3]), ([1,2], [3]), ([1,2,3], [])]
```

**Implementation**: Iterator that yields multiple values.

**Use cases**:
- Generate combinations
- Explore solution space
- Testing (generate test cases)

### Phase 4: Bidirectional Modes (Same Fun, Multiple Directions)

**Features**:
- Same fun, multiple mode implementations
- Compiler selects based on which arguments are known
- Still no backtracking - each mode is deterministic

**Syntax**:
```rust
// Declare fun with modes
fun append<T>(left: [@T], right: [@T], result: [@T])

// Mode 1: (in, in, out) - concatenate
mode append(^in, ^in, ^out) -> ^det {
    list_concat(left, right)
}

// Mode 2: (out, out, in) - split
mode append(^out, ^out, ^in) -> ^multi {
    for i in @0..=result.len() {
        yield result.split_at(i)
    }
}

// Compiler selects mode based on usage:
let result = append([1,2], [3,4], _)      // Mode 1
let (l, r) = append(_, _, [1,2,3,4])      // Mode 2
```

**Implementation**: Mode inference at compile time, generate specialized code.

**Use cases**:
- Reversible operations
- Symmetric relations
- Undo/redo

### Phase 5: Backtracking (Nondet with Choice Points)

**Features**:
- Full logic programming with backtracking
- Choice points + automatic retry
- Nondet - may fail, multiple solutions

**Syntax**:
```rust
// Nondet fun - may fail, try alternatives
#[nondet]
fun solve_puzzle(puzzle: Puzzle ^in) -> Solution {
    // Try different strategies (choice points)
    let strategy = choose([
        Strategy::Greedy,
        Strategy::Backtrack,
        Strategy::Heuristic
    ]);

    let partial = apply_strategy(puzzle, strategy);

    // If this fails, automatically backtrack and try next strategy
    if is_complete(partial) {
        yield partial
    } else {
        // Recursively solve sub-puzzles (more choice points)
        for sub in split_puzzle(partial) {
            let sub_solution = solve_puzzle(sub);
            if is_valid(sub_solution) {
                yield combine(partial, sub_solution)
            }
        }
    }
}
```

**Implementation**: Either:
- Explicit continuation-passing style
- Stack of choice points
- Or compile to a state machine

**Use cases**:
- Complex search problems
- Constraint solving
- Planning

### Phase 6: Committed Choice (cc_multi)

**Features**:
- Like `multi` but commits to first solution
- Prevents exhaustive search when unnecessary
- Better performance

**Syntax**:
```rust
#[cc_multi]  // Committed choice multi
fun first_valid_split(list: [@T] ^in) -> ([@T], [@T]) {
    for i in @0..=list.len() {
        let (left, right) = list.split_at(i);
        if is_valid_split(left, right) {
            return (left, right)  // Commit - don't explore more
        }
    }
}
```

## 7. Syntax Design Considerations

### Option A: Determinism Attributes

```rust
#[det]       // Default - exactly one solution
#[semidet]   // 0 or 1 solution (returns @?)
#[multi]     // 1+ solutions (generator)
#[nondet]    // 0+ solutions (generator, may fail)
#[cc_multi]  // Committed choice multi
```

**Pros**: Familiar attribute syntax, explicit.
**Cons**: Verbose.

### Option B: Return Type Indicates Determinism

```rust
fun foo() -> T           // det - single value
fun foo() -> @?T         // semidet - optional value
fun foo() yields T       // multi - generator
fun foo() tries T        // nondet - may fail generator
```

**Pros**: Concise, clear from signature.
**Cons**: New keywords.

### Option C: Mode Syntax Extension

```rust
fun append(
    left: [@T] ^in,
    right: [@T] ^in
) -> [@T] ^out ^det

fun split(
    list: [@T] ^in
) -> ([@T] ^out, [@T] ^out) ^multi
```

**Pros**: Consistent with mode syntax.
**Cons**: Gets verbose.

### Recommendation: Combination

- Default is `det` (no annotation needed)
- Use return types for `semidet` (@?) and `multi` (yields)
- Use attributes for `nondet` and `cc_multi`

```rust
// Det (implicit)
fun add(x: @u32 ^in, y: @u32 ^in) -> @u32

// Semidet (use @?)
fun find(list: [@T] ^in, pred: (T -> @bool) ^in) -> @?T

// Multi (use yields)
fun split(list: [@T] ^in) yields ([@T], [@T])

// Nondet (use attribute + yields)
#[nondet]
fun search(space: Space ^in) yields Solution
```

## 8. Implementation Strategy

### Start with Rust Iterators

Phase 1-3 compile directly to Rust iterators:

```rust
// Datalove
fun split(list: [@T] ^in) yields ([@T], [@T])

// Compiles to
fn split<T>(list: Vec<T>) -> impl Iterator<Item = (Vec<T>, Vec<T>)> {
    (0..=list.len()).map(move |i| {
        let left = list[..i].to_vec();
        let right = list[i..].to_vec();
        (left, right)
    })
}
```

### Multiple Modes = Multiple Implementations

```rust
// Datalove
fun append<T>(left: [@T], right: [@T], result: [@T])

mode append(^in, ^in, ^out) -> ^det { ... }     // append_concat
mode append(^out, ^out, ^in) -> ^multi { ... }  // append_split

// Compiles to two separate Rust functions
fn append_concat<T>(left: Vec<T>, right: Vec<T>) -> Vec<T> { ... }
fn append_split<T>(result: Vec<T>) -> impl Iterator<Item = (Vec<T>, Vec<T>)> { ... }

// Compiler selects based on usage
```

### Backtracking via State Machine

For Phase 5 (nondet with backtracking):

```rust
// Conceptual compilation of backtracking fun
enum SolverState {
    TryStrategy { puzzle: Puzzle, strategies: Vec<Strategy>, index: usize },
    ApplyStrategy { puzzle: Puzzle, strategy: Strategy },
    SolveSub { partial: Partial, subs: Vec<Puzzle>, index: usize },
    Done,
}

struct SolverIterator {
    state_stack: Vec<SolverState>,
}

impl Iterator for SolverIterator {
    fn next(&mut self) -> Option<Solution> {
        loop {
            match self.state_stack.last_mut() {
                Some(SolverState::TryStrategy { strategies, index, .. }) => {
                    // Try next strategy
                    if *index < strategies.len() {
                        let strategy = strategies[*index];
                        *index += 1;
                        self.state_stack.push(SolverState::ApplyStrategy { ... });
                    } else {
                        // Backtrack - no more strategies
                        self.state_stack.pop();
                    }
                }
                // ... other states
            }
        }
    }
}
```

This is essentially **explicit CPS** (continuation-passing style).

## 9. Example: Practical Use in Datalove

### Scenario: Query a Data Structure

```rust
// Define a relation: person data
struct Person {
    name: @string,
    age: @u32,
    children: [@string]
}

// Database of people
let people: [@Person] = load_data()

// Query fun with multiple modes
fun parent_of(parent_name: @string, child_name: @string, db: [@Person] ^in)

// Mode 1: Check relationship
mode parent_of(^in, ^in, ^in) -> @bool {
    db.iter()
        .filter(|p| p.name ≡ parent_name)
        .flat_map(|p| p.children.iter())
        .any(|c| c ≡ child_name)
}

// Mode 2: Find children
mode parent_of(^in, ^out, ^in) yields @string {
    for person in db {
        if person.name ≡ parent_name {
            for child in person.children {
                yield child
            }
        }
    }
}

// Mode 3: Find parents
mode parent_of(^out, ^in, ^in) yields @string {
    for person in db {
        if person.children.contains(child_name) {
            yield person.name
        }
    }
}

// Usage:
// Check: Is Alice a parent of Bob?
if parent_of("Alice", "Bob", people) { ... }

// Find: Who are Alice's children?
for child in parent_of("Alice", _, people) {
    print(child)
}

// Find: Who are Bob's parents?
for parent in parent_of(_, "Bob", people) {
    print(parent)
}
```

### Scenario: Undo Stack with Backtracking

```rust
// Define reversible operations
fun edit(text: @string, pos: @u32, char: @char) -> @string

// Forward mode: Apply edit
mode edit(^in, ^in, ^in) -> ^det {
    text.insert(pos, char)
}

// Backward mode: Undo edit (find what was there)
mode edit(^out, ^in, ^in) yields (@string, @char) {
    // Multiple possibilities if we don't know original char
    for original_char in possible_chars() {
        let original = text.remove(pos);
        if would_produce(original, pos, char, text) {
            yield (original, original_char)
        }
    }
}

// Undo with perfect information
let original_text = edit(_, pos, char_that_was_inserted)
```

## 10. Advantages for Datalove

### 10.1 Perfect Fit with Pure Data

- Logic programming requires **referential transparency**
- Your pure-data types guarantee this
- No cycles = termination guarantees

### 10.2 Memoization is Natural

- Deterministic multi funs have deterministic solution sets
- Cache: `(input, mode) -> [solutions]`
- Replay is free!

### 10.3 REPL Integration

```rust
// REPL command
> split([1,2,3])
=> ([], [1,2,3])   ; First solution

> next
=> ([1], [2,3])    ; Next solution

> next
=> ([1,2], [3])    ; Next solution

> all
=> [
    ([], [1,2,3]),
    ([1], [2,3]),
    ([1,2], [3]),
    ([1,2,3], [])
]
```

Built-in exploration of solution space!

### 10.4 Checkpoint/Replay

```rust
// Record execution trace
struct Trace {
    inputs: [@Input],
    choices: [@{fun_id: @u32, solution_index: @u32}],
    outputs: [@Output]
}

// Deterministic replay
fun replay(trace: Trace ^in) {
    for (input, choice) in zip(trace.inputs, trace.choices) {
        let solutions = execute_multi(input);
        let output = solutions[choice.solution_index];
        assert output ≡ trace.outputs[i]
    }
}
```

### 10.5 Testing

Generate test cases using multi funs:

```rust
// Property-based testing
#[test]
fun test_append_split_inverse() {
    for list in generate_lists() {
        for (left, right) in split(list) {
            assert append(left, right) ≡ list
        }
    }
}
```

## 11. Challenges & Solutions

### Challenge 1: Infinite Solutions

Some multi funs could generate infinite solutions:

```rust
fun natural_numbers() yields @u32 {
    for n in @0.. {
        yield n
    }
}
```

**Solution**: Lazy evaluation + take/limit:
```rust
natural_numbers().take(@10)
```

### Challenge 2: Performance of Backtracking

Full backtracking with choice points can be expensive.

**Solution**: Committed choice (`cc_multi`) when appropriate:
```rust
#[cc_multi]  // Take first valid solution, don't backtrack
fun find_any_solution(problem: Problem ^in) -> Solution
```

### Challenge 3: Type System Complexity

Multiple modes = complex type checking.

**Solution**: Start simple (Phase 1-3), add gradually.

### Challenge 4: Debugging Backtracking

Hard to understand why search failed.

**Solution**: Trace mode:
```rust
#[trace_backtracking]
fun search(space: Space ^in) yields Solution
// Logs: "Tried strategy A, failed. Backtracking to choice point 1..."
```

## 12. Comparison to Other Approaches

### vs. Prolog

| Feature | Prolog | Datalove Proposal |
|---------|--------|-------------------|
| Purity | Impure (cuts, I/O) | Pure by default |
| Types | Dynamic/weakly typed | Strong static types |
| Modes | Not explicit | Explicit mode annotations |
| Performance | Unpredictable | More predictable |

### vs. Mercury

| Feature | Mercury | Datalove Proposal |
|---------|---------|-------------------|
| Full language | Yes | Embedded in fun system |
| Modes | Built-in | Built-in |
| Determinism | Built-in | Built-in |
| Data model | General | Pure data only |
| Simplicity | Complex | Start simple, grow |

### vs. miniKanren

| Feature | miniKanren | Datalove Proposal |
|---------|------------|-------------------|
| Embedding | In Scheme/others | In datalove |
| Approach | Deeply embedded DSL | Shallow embedding |
| Unification | General unification | Pattern matching |
| Simplicity | Very simple core | Simple + powerful |

## 13. Recommended Starting Point

### Minimal Viable Logic Features

**Phase 1**: Multi funs (generators)
- Syntax: `fun foo() yields T`
- Semantics: Deterministic iterator
- No backtracking yet
- Compiles to Rust iterators

**Use cases**:
- Lazy sequences
- Generate test cases
- Explore solutions

**Example**:
```rust
fun split(list: [@T] ^in) yields ([@T], [@T]) {
    for i in @0..=list.len() {
        yield list.split_at(i)
    }
}

// Usage
for (left, right) in split([1,2,3]) {
    print(left, right)
}
```

This alone gives you:
- Multiple solutions
- Pure data semantics
- Memoization (cache solution sequences)
- Testing (generate inputs)
- REPL exploration

### Next Steps

After Phase 1 works:
1. Add mode inference (Phase 4)
2. Add semidet (@? returns) (Phase 2)
3. Experiment with backtracking (Phase 5)

## 14. Conclusion

Your intuition is correct: **datalove funs with modes have similar expressive power to Mercury predicates**.

**Key enablers**:
1. **Pure data** = referential transparency (required for logic programming)
2. **Argument modes** = specify direction (in/out/di/uo)
3. **No cycles** = termination guarantees
4. **Memoization** = efficient replay of solutions

**Recommended path**:
- Start with **multi funs as generators** (Phase 1)
- Add **semidet** (@? returns) (Phase 2)
- Experiment with **bidirectional modes** (Phase 4)
- If needed, add **full backtracking** (Phase 5)

**Perfect fit with your goals**:
- **Undo/redo**: Backtracking is built-in undo
- **Memoization**: Pure data + deterministic = perfect caching
- **Checkpoint/replay**: Record choices, replay deterministically
- **REPL**: Explore solution space interactively

This could make datalove uniquely powerful for **interactive data exploration** with logic programming features!

---

**Next Steps**:
1. Design concrete syntax for Phase 1 (multi funs)
2. Implement prototype with Rust iterators
3. Try examples with your data types
4. Experiment with REPL integration

Would you like me to elaborate on any of these areas?
