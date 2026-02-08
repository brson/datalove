# Datalog-Style Programming with Atoms, Tags, and Enums

Atoms, tags, enums, and sets together form a natural basis
for relational and logic-oriented programming in datalove.
This document explores the patterns that emerge,
with a focus on ground Datalog --
the subset of logic programming that needs
no backtracking or unification.


## The Core Insight

A recurring pattern across many use cases:

1. Define a domain as an enum of atoms and tags.
2. Represent knowledge/state as a set of enum values.
3. Write inference rules as pure functions `set -> set` that grow the set.
4. Iterate to a fixed point.

This is Datalog.
It falls out of datalove's existing primitives
without special logic programming syntax.


## Pattern: Propositional Reasoning with Atom Sets

Atoms as propositions, sets as interpretations.

```datalove
type Prop: enum {
  atom Raining,
  atom Cold,
  atom Wet,
  atom StayInside,
}

let world: set { Prop } = set { atom Raining@, atom Cold@ }

// Forward-chaining rules as set transformations.
fun infer(facts: set { Prop }): set { Prop }
  var result = facts
  if contains(facts, atom Raining@)
    set result = insert(result, atom Wet@)
  end if
  if contains(facts, atom Wet@) and contains(facts, atom Cold@)
    set result = insert(result, atom StayInside@)
  end if
  ret result
end fun

// Fixed point: keep applying rules until stable.
fun saturate(facts: set { Prop }): set { Prop }
  let next = infer(facts)
  if next == facts
    ret facts
  end if
  ret saturate(next)
end fun
```

Propositional Datalog: monotone set growth to a fixed point.


## Pattern: Relational Programming with Tagged Tuples

Tags carry data. A set of tags is a relation.

```datalove
type Fact: enum {
  tag Parent (string, string),
  tag Ancestor (string, string),
}

let db: set { Fact } = set {
  tag Parent ("alice", "bob")@,
  tag Parent ("alice", "carol")@,
  tag Parent ("bob", "dave")@,
}

// Rules:
//   parent(X,Y)                   => ancestor(X,Y)
//   parent(X,Z), ancestor(Z,Y)   => ancestor(X,Y)
fun derive(facts: set { Fact }): set { Fact }
  var result = facts

  // Direct: every parent is an ancestor.
  for fact in facts
    match fact
    case tag Parent pair
      set result = insert(result, tag Ancestor pair@)
    case default
    end match
  end for

  // Transitive: join Parent with Ancestor.
  for f1 in facts
    for f2 in facts
      match f1
      case tag Parent p
        match f2
        case tag Ancestor a
          if p.1 == a.0
            set result = insert(result, tag Ancestor (p.0, a.1)@)
          end if
        case default
        end match
      case default
      end match
    end for
  end for

  ret result
end fun
```

Relations are sets of tags,
rules are functions over those sets,
evaluation iterates to a fixed point.


## Pattern: Finite Lattices and Abstract Interpretation

Atoms are perfect for finite lattice elements.

```datalove
type Sign: enum {
  atom Bottom,
  atom Neg,
  atom Zero,
  atom Pos,
  atom Top,
}

fun join(a: Sign, b: Sign): Sign
  if a == b
    ret a
  end if
  match a
  case atom Bottom
    ret b
  case default
    match b
    case atom Bottom
      ret a
    case default
      ret atom Top@
    end match
  end match
end fun

fun abstract_add(a: Sign, b: Sign): Sign
  match a
  case atom Pos
    match b
    case atom Pos
      ret atom Pos@
    case atom Zero
      ret atom Pos@
    case atom Neg
      ret atom Top@
    case atom Bottom
      ret atom Bottom@
    case atom Top
      ret atom Top@
    end match
  case atom Zero
    ret b
  case atom Neg
    match b
    case atom Neg
      ret atom Neg@
    case atom Zero
      ret atom Neg@
    case atom Pos
      ret atom Top@
    case atom Bottom
      ret atom Bottom@
    case atom Top
      ret atom Top@
    end match
  case atom Bottom
    ret atom Bottom@
  case atom Top
    ret atom Top@
  end match
end fun
```

Abstract interpretation computes fixed points over lattices.
Atom-enums define the lattice, match defines transfer functions,
iteration reaches a fixed point -- same shape as Datalog saturation.


## Pattern: Multi-Valued Logic

Atoms express richer truth values than bool.

```datalove
// Three-valued logic (Kleene).
type Tri: enum {
  atom T,
  atom F,
  atom U,
}

fun tri_and(a: Tri, b: Tri): Tri
  match a
  case atom F
    ret atom F@
  case atom T
    ret b
  case atom U
    match b
    case atom F
      ret atom F@
    case default
      ret atom U@
    end match
  end match
end fun

// Four-valued logic (Belnap) for paraconsistent reasoning.
type Belnap: enum {
  atom Neither,
  atom True,
  atom False,
  atom Both,
}
```

Useful for knowledge bases with incomplete or contradictory information --
exactly what arises in logic programming with negation.


## Pattern: Constraint Propagation

```datalove
type CellValue: enum {
  atom Any,
  tag Exactly int,
  tag OneOf set { int },
  atom Contradiction,
}

fun constrain(cell: CellValue, must_not_be: int): CellValue
  match cell
  case atom Any
    ret cell
  case tag Exactly v
    if v == must_not_be
      ret atom Contradiction@
    end if
    ret cell
  case tag OneOf candidates
    let remaining = remove(candidates, must_not_be)
    if is_empty(remaining)
      ret atom Contradiction@
    end if
    if size(remaining) == 1
      ret tag Exactly (first(remaining))@
    end if
    ret tag OneOf remaining@
  case atom Contradiction
    ret cell
  end match
end fun
```

The enum represents a cell's domain.
Atoms mark the extremes (unconstrained, contradictory),
tags carry domain data.
Propagation iterates to a fixed point -- same shape again.


## Pattern: State Machines

```datalove
type ConnState: enum {
  atom Idle,
  atom Connecting,
  tag Connected int,
  atom Closing,
  tag Failed string,
}

fun on_event(state: ConnState, event: Event): ConnState
  match state
  case atom Idle
    match event
    case atom Connect
      ret atom Connecting@
    case default
      ret state
    end match
  case atom Connecting
    match event
    case tag Success fd
      ret tag Connected fd@
    case tag Error reason
      ret tag Failed reason@
    case default
      ret state
    end match
  case default
    todo!()
  end match
end fun
```

The atom/tag distinction is visually clear:
signal states (Idle, Connecting, Closing) are atoms,
data-carrying states (Connected, Failed) are tags.

A state machine is also a relation `transition(State, Event, NextState)`.
Written as a function here,
but could equivalently be a set of tagged triples (the relational pattern).


## Pattern: Evidence-Carrying Judgments

Tags carry witnesses alongside conclusions.

```datalove
type Judgment: enum {
  tag Holds (string, string),
  tag Refuted (string, string),
  tag Conditional (string, string),
}
```

A knowledge base is a set of judgments.
Inference adds new judgments citing existing ones as evidence.
This is a flat (non-recursive) form of proof terms.
For deeper proof trees, recursive types or
a separate proof-tree structure would be needed.


## What Atoms/Tags Buy Over Plain Structs

The key property: a named, matchable, discriminated value
that exists as its own type.

- An atom in a set is self-describing by its name.
- A tag in a set carries both meaning (name) and data (payload).
- Different "shapes" of facts coexist in one set via enums.
- Match gives exhaustive case analysis -- essential for correct inference rules.
- The `@` operator lets individual facts (atoms/tags) compose cleanly
  into relations (enum sets) without up-front wrapping.

In languages where variants are constructors, not types
(Haskell, Rust, OCaml regular variants),
you can't have a standalone `Parent("alice", "bob")` --
it must be wrapped in its enum type from the start.
In datalove, `tag Parent ("alice", "bob")` is a type and a value,
and `@` widens it when placed in a set with other fact types.
That's a real ergonomic win for relational programming.


## Syntactic Support: Design

The patterns above work with existing primitives,
but the nested for/match/if loops are verbose
and obscure the relational intent.
Three constructs could make the patterns direct:
a `from` comprehension, a `fixpoint` loop, and named rules.

They share a common core:
**comprehensions over sets with pattern-matching and implicit equijoins.**


### The `from` comprehension

The fundamental building block.
A `from` expression iterates over a set,
pattern-matches its elements,
and produces a new set or sequence.

Basic form -- filter and project (query):

```datalove
// Who are alice's children?
let children = from facts
  given tag Parent ("alice", child)
  select child
end from
// children: set { string }
```

`given` matches elements of the source set against a tag (or atom) pattern.
Literal values in the pattern are equality constraints.
Bare names are binding positions.

`select` projects the bound variables into the output set.
The result type is inferred from the selected expression.

Multiple `given` clauses express joins:

```datalove
// Who are common ancestors of alice and bob?
let common = from facts
  given tag Ancestor (anc, "alice")
  given tag Ancestor (anc, "bob")
  select anc
end from
```

The shared variable `anc` creates an equijoin condition.
This desugars to nested iteration with a filter
on the shared binding.

Producing new tagged facts (derive):

```datalove
// Derive direct ancestor facts from parent facts.
let direct = from facts
  given tag Parent (x, y)
  yield tag Ancestor (x, y)@
end from
// direct: set { Fact }
```

`yield` produces enum values for the output set.
`select` projects out component values;
`yield` constructs new atoms/tags.


#### `from` desugaring

A single `given`:

```datalove
from facts
  given tag Parent (x, y)
  yield tag Ancestor (x, y)@
end from
```

Desugars to:

```datalove
var __result: set { Fact } = set {}
for __elem in facts
  match __elem
  case tag Parent __payload
    let (x, y) = __payload
    set __result = insert(__result, tag Ancestor (x, y)@)
  case default
  end match
end for
__result
```

A join (two `given` clauses sharing variable `z`):

```datalove
from facts
  given tag Parent (x, z)
  given tag Ancestor (z, y)
  yield tag Ancestor (x, y)@
end from
```

Desugars to:

```datalove
var __result: set { Fact } = set {}
for __e1 in facts
  match __e1
  case tag Parent __p1
    let (x, z) = __p1
    for __e2 in facts
      match __e2
      case tag Ancestor __p2
        if __p2.0 == z
          let y = __p2.1
          set __result = insert(__result, tag Ancestor (x, y)@)
        end if
      case default
      end match
    end for
  case default
  end match
end for
__result
```

The shared variable `z` becomes the equality filter.
Each additional `given` clause adds one level of nested iteration.


#### Guards with `where`

Between `given` and `select`/`yield`, a `where` clause
filters on bound variables:

```datalove
from facts
  given tag Age (person, age)
  where age .> 18
  yield tag Adult person@
end from
```

`where` is an arbitrary boolean expression over bound names.
It desugars to an `if` guard wrapping the body.


#### Multiple source sets

A `given` clause can name a different source set:

```datalove
from parents
  given tag Parent (x, y)
from ages
  given tag Age (y, age)
  where age .> 18
  select (x, y, age)
end from
```

Each `from` introduces a new source set for subsequent `given` clauses.
The shared variable `y` creates a cross-set join.

If this is too noisy, the simpler design is
one source set per comprehension
and explicit joins via multiple comprehensions
or via indexing functions.
One-source-set is cleaner and covers most Datalog patterns;
cross-set joins can wait.


#### `from` over atoms

Works for atom-only enums too:

```datalove
let is_wet = from world
  given atom Raining
  select true
end from
```

Returns a `set { bool }` --
nonempty if `atom Raining` is in the set,
empty otherwise.
More useful as a membership test via `any`:

```datalove
let is_wet: bool = any from world
  given atom Raining
end from
```

`any` returns true if the comprehension matched at least once.
`none` (or `not any`) for the negative.


### The `fixpoint` loop

Iterates a body expression until the result equals the input.
Uses the same `name = init` pattern as `loop carry`.

```datalove
let all_facts = fixpoint facts = db
  let direct = from facts
    given tag Parent (x, y)
    yield tag Ancestor (x, y)@
  end from
  let transitive = from facts
    given tag Parent (x, z)
    given tag Ancestor (z, y)
    yield tag Ancestor (x, y)@
  end from
  union(facts, direct, transitive)
end fixpoint
```

Semantics:

1. Bind `facts` to `db`.
2. Evaluate the body. It returns a new set.
3. If the new set equals `facts`, return it.
4. Otherwise, bind `facts` to the new set and repeat from 2.

The body is a block that must return the same type as the initial value.
It can contain arbitrary statements --
`let`, `var`, `from` comprehensions, function calls.
The last expression is the "next" value.

The `fixpoint` form requires `==` on the carried type.
For sets of atoms/tags, structural equality is natural.


#### Monotonicity and termination

Datalog-style fixpoints terminate
when the step function is **monotone** (only adds, never removes)
and the domain is **finite**.

For atom-only enums, the domain is always finite --
there are finitely many atoms.
Termination is guaranteed for monotone rules.

For tag-carrying enums where the payload domain is finite
(e.g., tags over fixed-width integers or strings from a known set),
termination is also guaranteed.

For tags over unbounded types like `int` or `string`,
termination depends on the rules.
A non-terminating fixpoint is possible
if a rule generates unbounded new values.

The `total` annotation could apply:

```datalove
total fixpoint facts = db
  // compiler proves termination
end fixpoint
```

The compiler would check:
is the set type finitely bounded?
Is the body monotone?
If both, mark as total.

For unbounded domains, a fuel/iteration-limit escape hatch:

```datalove
fixpoint facts = db limit 1000
  // aborts (or returns current state) after 1000 iterations
end fixpoint
```


#### Relation to `loop carry`

`fixpoint` is a specialization of `loop carry`
where the exit condition is "state didn't change."
Written manually:

```datalove
let all_facts = loop carry (facts = db)
  let next = derive(facts)
  if next == facts
    break facts
  end if
  continue next
end loop
```

`fixpoint` eliminates this boilerplate.
The two forms coexist --
use `loop carry` when the termination condition
is something other than equality.


### Named rules

For reuse and clarity,
rules can be named functions
that return `from` comprehensions.
No new syntax needed beyond `from`:

```datalove
fun direct_ancestors(ref facts: set { Fact }): set { Fact }
  ret from facts
    given tag Parent (x, y)
    yield tag Ancestor (x, y)@
  end from
end fun

fun transitive_ancestors(ref facts: set { Fact }): set { Fact }
  ret from facts
    given tag Parent (x, z)
    given tag Ancestor (z, y)
    yield tag Ancestor (x, y)@
  end from
end fun

let all_facts = fixpoint facts = db
  union(facts, direct_ancestors(facts), transitive_ancestors(facts))
end fixpoint
```

A `ruleset` grouping could be sugar
for applying multiple rule functions and unioning the results:

```datalove
ruleset ancestry(facts: set { Fact }): set { Fact }
  from facts
    given tag Parent (x, y)
    yield tag Ancestor (x, y)@
  end from
  from facts
    given tag Parent (x, z)
    given tag Ancestor (z, y)
    yield tag Ancestor (x, y)@
  end from
end ruleset
```

The `ruleset` body contains multiple `from` blocks.
The result is the union of the input set
with all the `from` results.
This is sugar for a function
that evaluates each `from` and unions everything.

Then fixpoint becomes:

```datalove
let all_facts = fixpoint facts = db
  ancestry(facts)
end fixpoint
```


### Negation

Stratified negation-as-failure.
An `unless` clause in a `from` comprehension
checks that a pattern does NOT match any element:

```datalove
from facts
  given tag Person name
  unless tag Parent (name, _) in facts
  yield tag Childless name@
end from
```

`unless` desugars to a nested iteration
that checks no element matches,
equivalent to `not any(...)`:

```datalove
for __elem in facts
  match __elem
  case tag Person name
    let __found = any from facts
      given tag Parent (name, _)
    end from
    if not __found
      set __result = insert(__result, tag Childless name@)
    end if
  case default
  end match
end for
```

Negation creates stratification requirements:
a rule with `unless` on relation R
must be in a later stratum than rules that derive R.
The fixpoint evaluates one stratum at a time, bottom to top.

For the first version, negation could be deferred entirely.
Positive Datalog (no negation) is already expressive
and avoids stratification complexity.


### Aggregation

Aggregates over matched results.
A `count`, `sum`, `min`, `max`, or `collect` keyword
replaces `select`/`yield`:

```datalove
// How many children does alice have?
let n = from facts
  given tag Parent ("alice", _)
  count
end from
// n: int

// Collect all children into a list.
let kids = from facts
  given tag Parent ("alice", child)
  collect child
end from
// kids: [string]

// Sum of ages.
let total = from facts
  given tag Age (_, age)
  sum age
end from
```

`count` desugars to an accumulator loop.
`collect` produces a list (ordered by iteration order of the set).
`sum`, `min`, `max` desugar to fold-style accumulation.

Aggregation is useful but not essential for core Datalog.
It could be deferred to a later version.


### Full example: ancestry knowledge base

Putting it all together.

```datalove
type Fact: enum {
  tag Parent (string, string),
  tag Ancestor (string, string),
  tag Childless string,
  tag Person string,
}

let db: set { Fact } = set {
  tag Person "alice"@,
  tag Person "bob"@,
  tag Person "carol"@,
  tag Person "dave"@,
  tag Parent ("alice", "bob")@,
  tag Parent ("alice", "carol")@,
  tag Parent ("bob", "dave")@,
}

// Saturate: derive all ancestors.
let kb = fixpoint facts = db
  let direct = from facts
    given tag Parent (x, y)
    yield tag Ancestor (x, y)@
  end from
  let transitive = from facts
    given tag Parent (x, z)
    given tag Ancestor (z, y)
    yield tag Ancestor (x, y)@
  end from
  union(facts, direct, transitive)
end fixpoint

// Query: who are alice's descendants?
let descendants = from kb
  given tag Ancestor ("alice", who)
  select who
end from
// descendants: set { string } = { "bob", "carol", "dave" }

// Query: is alice an ancestor of dave?
let yes = any from kb
  given tag Ancestor ("alice", "dave")
end from
// yes: bool = true

// Query: how many descendants does alice have?
let n = from kb
  given tag Ancestor ("alice", _)
  count
end from
// n: int = 3
```


## REPL integration

The `from` comprehension and `fixpoint` loop
are natural in a REPL session.
Incremental exploration of a knowledge base:

```
> let db = set { tag Parent ("alice", "bob")@, tag Parent ("bob", "carol")@ }

> from db given tag Parent (x, y) select (x, y)
=> { ("alice", "bob"), ("bob", "carol") }

> let kb = fixpoint f = db
    union(f, from f given tag Parent (x,z), tag Ancestor (z,y) yield tag Ancestor (x,y)@ end from,
              from f given tag Parent (x,y) yield tag Ancestor (x,y)@ end from)
  end fixpoint

> from kb given tag Ancestor ("alice", who) select who
=> { "bob", "carol" }
```

Fits the REPL's incremental, exploratory style.
Each `from` is a standalone expression that returns a set.
The REPL prints it.
Undo/redo works because everything is pure.


## Design summary

| Construct | Purpose | Returns | Keywords |
|-----------|---------|---------|----------|
| `from ... given ... select ... end from` | Query (filter + project) | `set { T }` | `from`, `given`, `select` |
| `from ... given ... yield ... end from` | Derive (produce new facts) | `set { Enum }` | `from`, `given`, `yield` |
| `from ... given ... count` | Aggregate | `int` | `from`, `given`, `count` |
| `from ... given ... unless ...` | Negation | (modifies filter) | `unless` |
| `from ... given ... where ...` | Guard | (modifies filter) | `where` |
| `fixpoint name = init ... end fixpoint` | Iterate to stability | same as init | `fixpoint` |
| `ruleset name(...) ... end ruleset` | Group rules | `set { Enum }` | `ruleset` |

New keywords: `from`, `given`, `yield`, `fixpoint`.
Optional/deferrable: `unless`, `ruleset`, `count`/`sum`/`min`/`max`/`collect`.
`select` and `where` are likely already reserved or unambiguous.


## Relationship to top-down logic programming

The research-logic-programming doc explores Mercury-style
modes, determinism, and backtracking.
The constructs here are the **bottom-up** (forward-chaining) complement:

| | Bottom-Up (Datalog) | Top-Down (Prolog/Mercury) |
|-|----------------------|--------------------------|
| Strategy | Saturate all facts | Goal-directed search |
| Termination | Always (finite domains) | Depends on program |
| Backtracking | None needed | Core mechanism |
| Implementation | Set iteration + fixpoint | Choice points + stack |
| Syntax | `from`/`given`/`fixpoint` | `multi`/`nondet`/modes |

Both operate on the same representation layer: atoms, tags, and enums.
Bottom-up is simpler to implement and reason about.
Top-down is more flexible for open-ended search.
They coexist naturally,
with bottom-up Datalog for closed-world inference
and top-down search for goal-directed queries.


## Implementation priority

1. `from` comprehension with single `given` and `select`.
   Minimal useful query. Desugars to for/match.
2. `from` with multiple `given` clauses (joins).
   Enables relational programming.
3. `from` with `yield` (deriving new facts).
4. `fixpoint` loop.
   Enables Datalog saturation.
5. `where` guards.
6. `any`/`none` wrappers for membership tests.
7. Aggregation (`count`, `sum`, `collect`).
8. `unless` (stratified negation).
9. `ruleset` sugar.
