# Recursive Structural Types

A design for recursive types in datalit and Datafun: an inline `rec` binder,
iso-recursive semantics, and recursion that must pass through a collection
rather than a box. Written October 2026 in response to
`mandocs/drafts/recursive-structural-types.md`; not implemented. Checked on
paper against the AWFY benchmarks that need recursive data. The decisions
for a first cut are in [Decisions](#user-content-decisions). Phase 1,
datalit, is implemented (October 2026), and Datafun refuses recursive types
(F090) until phase 2; see [Phase 1 as built](#user-content-phase-1-as-built).

## Contents

- [The problem](#user-content-the-problem)
- [The design in brief](#user-content-the-design-in-brief)
- [Decisions](#user-content-decisions)
- [Syntax](#user-content-syntax)
- [The guard rule](#user-content-the-guard-rule)
- [What no box costs](#user-content-what-no-box-costs)
- [Iso-recursive, with roll and unroll inserted](#user-content-iso-recursive-with-roll-and-unroll-inserted)
- [The type checker](#user-content-the-type-checker)
- [Borrowing match](#user-content-borrowing-match)
- [Runtime and backends](#user-content-runtime-and-backends)
- [The IR and the backends](#user-content-the-ir-and-the-backends)
- [Against the AWFY benchmarks](#user-content-against-the-awfy-benchmarks)
- [Prior art](#user-content-prior-art)
- [Phase 1 as built](#user-content-phase-1-as-built)
- [What exists, what is missing](#user-content-what-exists-what-is-missing)
- [Phases](#user-content-phases)
- [Open questions](#user-content-open-questions)

## The problem

Datalove cannot write a tree. An alias cannot name itself
(`type L: ?(int, L)` is F064), and datalit has no aliases at all, because
it is an expression language with no statements. The obvious fixes each break
a constraint the draft sets:

- A statement-level alias does not exist in datalit.
- A `box` type gives datalit text a runtime meaning that the data format has
  no use for.
- Implicit boxing breaks "values are stored inline, never implicitly boxed"
  (`mandocs/principles.md`).

So a recursive type needs a name that lives inside one type expression, and
an indirection that is already in the language.

## The design in brief

`rec Name T` binds `Name` within `T` and nowhere else. Every use of `Name`
must sit under a heap-backed collection between the use and its binder, which
keeps the inline size finite without a new type. The types are iso-recursive:
a `rec` type differs from its one-level unfolding. Rolling and unrolling are
no-ops at run time, though, and the checker inserts them, so a literal never
mentions them. Internally the binder becomes a de Bruijn index. Type equality
then remains the derived `PartialEq`, and salsa hashing is unaffected.

## Decisions

Settled before implementation, October 2026:

- **The keyword is `rec`.**
- **The binder name is kept** in a field that equality and hashing ignore,
  and a diagnostic may show a stale name after salsa backdates a memo.
- **The first cut is limited.** Lifting any of these limits later breaks
  nothing:
  - no recursion in a map's key or a set's element;
  - no type parameter inside a `rec` body, so no generic `rec` types;
  - no recursion groups, though one binder may nest inside another;
  - no `rec` type across the native rider boundary.
- **Borrowing `match` is deferred.** Recursive enums can be built and
  consumed, but not read in place, until it lands.
- **Datalit first.** It is self-contained and has no IR, so it proves the
  checker design before Datafun takes it up.

## Syntax

```datalove
: rec Node { name: string, kids: [Node] } / {
  name = "root",
  kids = [
    { name = "a", kids = [] },
    { name = "b", kids = [{ name = "c", kids = [] }] },
  ],
}
```

The value is ordinary datalit. Only the type is new. JSON, the canonical
recursive data, needs nothing further:

```datalove
type Json: rec J enum {
  atom Null,
  term Bool bool,
  term Num f64,
  term Str string,
  term Arr [J],
  term Obj %{string = J},
}
```

- `rec` starts a type, so it joins the "type alias, type parameter" row of
  the reserved-word table (botspec Section 2.1). The parser recognizes it by
  the word alone, as that section requires.
- `type Json: rec J ...` is an ordinary alias whose value is a `rec` type.
  A self-referencing alias without `rec` stays F064. A later ergonomic mode
  could desugar it.
- A `rec` name that shadows an alias, a type parameter or an outer `rec`
  name is an error.
- A binder that is never used, `rec L int`, is an error, so that each type
  has one spelling.

## The guard rule

Each use of a bound name must have at least one of `[_]`, `%{_ = _}`, `#{_}`
or `{| |}` between it and its own binder. Structs, tuples, `?`, `!`, terms,
enums and inner `rec` binders do not count.

```datalove
: rec L { elt: int, next: [L] }        // ok
: rec L { elt: int, next: ?L }         // error: L is not behind a collection
: rec L L                              // error
: rec A { b: rec B { a: A, bs: [B] } } // error: B is guarded, A is not
: rec L [L]                            // ok: [], [[]], ... are all finite
```

This is Algol 68's "shielding", with the collections in the role of `ref`.
Algol also requires a `struct` on every cycle (the "yang" half of its
yin-yang rule). That requirement exists to rule out ambiguous coercions, and
Datalove has no implicit coercions, so the requirement is left out.

The check runs during hint conversion in time linear in the type. A stack
records the collection depth at each binder. A use is legal when the current
collection depth is greater than the depth at its own binder.

## What no box costs

Recursion with a fixed arity has to go through a list:

```datalove
: rec List { elt: int, next: [List] }     // at most one, by convention
: rec Tree { v: int, kids: [Tree] }       // exactly two, by convention
```

The type stops enforcing the arity. Each slot also holds a list handle where
an option of a pointer would have been. Most recursive data is rose trees,
JSON, or ASTs whose children are lists, and none of that pays. Cons lists and
binary search trees do pay.

In the AWFY benchmarks the arity went unenforced in List, Towers and
Richards, but expressing the structure was never a problem
([Against the AWFY benchmarks](#user-content-against-the-awfy-benchmarks)).

If the cost matters, the smallest fix is to make the guard a slot of its own.
That fix is a box under another name, and should be decided as one. This plan
does not include it.

## Iso-recursive, with roll and unroll inserted

Let `T = rec X body`. Unfolding `T` substitutes `T` for `X` in `body`. `T`
and its unfolding are distinct types with the same layout, because `rec` is
not a box. Rolling and unrolling therefore do nothing at run time, and
inserting them silently does not hide any work.

- **Roll** happens only in checking mode, when a struct, tuple, list or enum
  literal is checked against an expected `rec` type. The literal is checked
  against the unfolding. This covers every datalit file and every hinted
  Datafun literal.
- **Unroll** happens at every elimination: field projection, tuple index,
  indexing, `match`, `for`, and destructuring. The head of a `rec` type is
  exposed first:

  ```
  n          : rec Node { name: string, kids: [Node] }
  n.kids     : [rec Node { name: string, kids: [Node] }]
  n.kids[0]  : rec Node { ... }
  ```

  A chain of projections lands back on the `rec` type rather than on a
  growing unfolding.
- **Anything else is a mismatch.** A value whose type is the unfolding, such
  as an unhinted literal bound to a variable, is not accepted where the `rec`
  type is expected. Adapt (`@`) performs the roll: it already means "clone,
  widen or coerce", and roll is the coercion that costs nothing. Unroll never
  needs `@`, because elimination does it.

## The type checker

The changes, against `crates/datalove-datalit/src/tycheck/types.rs` and the
Datafun checker:

1. **Two variants** in the datalit `Type`: `Rec(TypeRec { name, body })` and
   `RecVar(u32)`, a de Bruijn index where 0 is the innermost binder.
   `Var(InternedText)`, the generic type parameter, is unaffected.
2. **Hint conversion** carries a binder stack. It resolves names to indices
   and runs the guard and unused-binder checks. This is the only place a free
   `RecVar` exists.
3. **Every expression's type is closed.** Unfolding substitutes a closed
   `Rec`, so no index ever needs shifting, and no `RecVar` escapes its
   binder. A debug assertion at the conversion boundary enforces this.
4. **Equality is the derived `PartialEq` and `Hash`.** De Bruijn indices
   make alpha-equivalence syntactic. Equi-recursive types would instead need a
   coinductive equality check (Amadio and Cardelli) at every comparison. Salsa
   needs `Hash` to agree with `Eq`, so they would also need every type graph
   minimized before it is hashed, and the order-insensitive enums make that
   minimization harder. This is the main reason to choose iso.
5. **The binder name is not part of a type's identity.** Otherwise
   `rec A {..A..}` and `rec B {..B..}` would be different types. The name
   lives in a field whose `Eq` and `Hash` ignore it, so diagnostics can print
   `rec Node`. The cost: salsa may backdate a memo whose only change is the
   name, and a diagnostic could then show a stale binder name. The fallback
   is no stored name and a generated `rec Self`.
6. **An `expose` helper** unfolds a `rec` at the head of a type. Every
   elimination site calls it before matching on the type's shape. Auditing
   those sites is the main work. A site that is missed reports a spurious
   mismatch, never an unsound acceptance, and tests over a recursive struct,
   enum and list will find the misses.
7. **An unfold costs O(body)**, one level per elimination, and types never
   grow along a chain. If profiling finds the clones, interning `Rec` bodies
   makes the substituted copy O(1).
8. **Generic unification is structural.** `Rec` unifies only with `Rec`, and
   inference never unfolds a type. That keeps equi-recursion from coming back
   through the unifier.
9. **Mutual recursion nests.** An inner binder may refer to an outer one:

   ```datalove
   rec E enum {
     term Lit int,
     term Block [rec S enum { term Let E, term Do E }],
   }
   ```

   This is the usual iso-recursive limitation. The `S` obtained by projecting
   out of `E` is not syntactically equal to a separately written
   `rec S ... rec E ...`, so which type is outermost matters. Wasm GC solves
   this with `rec` groups. Data files can likely live without them.

Every operation stays linear in the size of the type, and equality does not
change. The new code is one conversion pass, one substitution, one helper,
and an audit of the elimination sites.

## Borrowing match

Deferred ([Decisions](#user-content-decisions)). `match` consumes its input (botspec Section 8.8). A recursive struct can be
read without moving it, because a field projection is a borrow:
`f(ref n.kids[0]?)`. A recursive enum cannot. Reading a `Json` or an AST
means moving the value or cloning every level, so recursive enums can be
built but not read in place. A borrowing `match` would fix that:

```datalove
match ref v
case term Arr items        // items is a ref borrow of the payload
    call walk(ref items)
case default
end match
```

The form follows `for item in ref xs`, where the marker already means "the
names this binds are borrows". `match mut v` would follow `for ... in mut`
in the same way, binding payloads that may be written but not moved. The
checker unrolls `v`'s type before matching, as at every elimination. The
ownership analysis treats each binding as a borrow rooted at `v`, the same
rule that covers `ref a[i]?`.

Borrowing `match` does not depend on recursive types and is useful without
them. Recursive enums are what make it necessary.

## Runtime and backends

These come from reading the code; none of them has been tried yet.

- **Descriptors become cyclic.** A `rec` type's `TyDesc` points to itself
  through the collection's element descriptor. `TyDesc` children are raw
  pointers (`crates/datalove-rtdt/src/lib.rs`), so the cycle can be
  represented. Building it has to tie the knot: allocate, memoize under the
  closed `Rec` type, then fill in. A naive recursion from `Type` never ends.
- **Walkers over types must stop at collections.** `is_plain` and
  `computed_flags` are examples. The guard rule makes stopping there correct,
  since every cycle passes through a collection and a collection is never
  plain. `is_plain` already returns `false` at a list, map or set without
  descending (`crates/datalove-rtdt/src/lib.rs`), so it needs no change.
- **Generated per-type glue must be memoized.** Any backend that emits drop,
  clone, equality or print code by recursing over a type needs to memoize on
  `Rec`, so that the generated functions call each other instead of expanding
  forever.
- **Deep values overflow the stack.** Drop, clone, equality and printing
  recurse over values. They terminate, but a linked list of a million
  elements built as `[List]` will overflow the Rust stack, as a `Box` linked
  list does in Rust. This failure mode is new, and the lists of children in
  rose trees will reach it less often than cons lists will. The AWFY
  benchmarks never recurse more than 15 deep, so they will not catch it. It
  needs its own test.

## The IR and the backends

The type checker is the smaller part of the work. Behind it:

- **`IrType` is a second type tree.** `crates/datalove-datafun-ir/src/lib.rs`
  mirrors the checker's `Type` with boxed children. Lowering a `rec` type into
  today's `IrType` would never terminate, so `IrType` needs its own `Rec` and
  `RecVar` and its own `expose`. That reaches every IR consumer: the passes,
  ownership, layout, monomorphization, the interpreter, ctfe, Cranelift and
  C AOT. A rough count of matches on aggregate variants is 41 in the IR crate,
  36 in `crates/datalove-datafun-common/src/generics.rs`, about 40 in the
  Datafun checker and about 70 in datalit, plus the backends.
- **Descriptor builders memoize too late.** A builder that files a
  descriptor after making it recurses before the entry exists. Datalit's
  `TyDescTable` ties the knot now (`create_rec`); the Cranelift emitter
  (`tydesc_emit.rs`) and C AOT's `collect_type_recursive` and `type_depth`
  still have the old shape, and need a placeholder in their caches before
  they descend.
- **C AOT orders its definitions by depth** (`type_depth`, sorted in
  `crates/datalove-datafun-c-aot/src/lib.rs`). With cycles that becomes
  forward declarations of the descriptors, then their definitions.
- **Fat generics instantiate descriptors at run time** from templates. A
  generic `rec` type would need the knot tied inside instantiation, which is
  one reason the first cut refuses type parameters in `rec` bodies.

## Against the AWFY benchmarks

AWFY benchmarks that could not be written because Datalove has no recursive
types are listed in [the AWFY survey](research/research-awfy-benchmarks.md).
Checked on paper against the AWFY JavaScript sources:

| Bench | Recursive type | Verdict |
|---|---|---|
| Storage | `rec T [?T]` | Ports directly |
| Json | `rec J enum { ..., term Arr [J], term Obj {...} }` | Ports directly |
| Towers | `rec D { size: u32, next: [D] }` | Ports, with swaps so moves do not allocate |
| List | `rec E { val: u32, next: [E] }` | The type ports; sharing makes the algorithm allocate |
| Richards | `rec P { link: [P], ... }` for packets | Packets port; tasks stay an arena |
| CD | Red-black tree with parent links | No: parent links are cycles |
| DeltaBlue | Constraint graph | No: a graph |
| Havlak | Control-flow graph, loop tree with parents | No, and not needed: blocks are numbered |

- **Storage** is a tree of arrays seven deep, whose leaves hold 1 to 10 empty
  slots. `rec T [?T]` is that shape exactly. A leaf is
  `list.repeated(ref : ?Tree / none, n)`, and nothing reads the tree.
- **Json** is a rose tree. The parser pushes into `[Json]`, and `term`
  literals roll implicitly. A term in a variable already needs `@` to widen
  into an enum (botspec Section 3.6), so rolling through `@` is the existing
  idiom. Its `verifyResult` is the one place that reads the tree. A consuming
  `match` works there, because the result is checked once.
- **Towers** keeps each pile as a `[Disk]` of length 0 or 1. With a fresh
  `[]` per move, each of the 8191 moves allocates. Swapping the emptied
  buffers through `mem.swap` and `mut piles[p]!` avoids that, provided an
  emptied list keeps its capacity:

  ```datalove
  let t = list.pop(mut piles[p]!)
  call mem.swap(mut piles[p]!, mut t.next)
  call mem.swap(mut d.next, mut piles[q]!)
  call list.push(mut piles[q]!, d)
  ```

- **List** returns one of its arguments from `tail(x, y, z)` and passes each
  list to two or three recursive calls. With owned values the arguments are
  `ref` and each result is cloned. A reimplementation counted 2809 calls, of
  which 2107 are base cases, and 16,451 cloned nodes, where AWFY allocates 31.
  That turns a benchmark of calls and pointer chasing into one of allocation.
  Every result is a suffix of one of the three original lists, so a faithful
  port returns `(which, skip)` and walks the originals by `ref`. With no
  `ref` local, the walks are recursive, as in `f(ref x[0]?.next)`.
- **Richards** packets are owned by one queue at a time. Appending at the end
  of a queue forwards `mut queue[0]!.link` down a recursion. Peeking and
  writing a packet in place is `mut record.work_in[0]!`. Tasks are shared
  between the `link` chain and the task table, so they stay an arena.
- **CD** walks `x.parent.parent` in `put` and `remove`. A red-black tree
  without parent links fits `rec N { ..., left: [N], right: [N] }`, but that
  is a different algorithm, and its deletion is the hard part.

Two things stand out. First, the guard rule never got in the way. Second,
what blocks the remaining benchmarks is sharing, which no owned recursive
type provides.

## Prior art

| Precedent | What it does | What it shows |
|---|---|---|
| OCaml `as 'a` | ``[ `Nil \| `Cons of int * 'a ] as 'a`` binds a name inside one type expression | An inline binder that is not an alias |
| Zig `@This()` | Names the innermost enclosing anonymous type | An innermost-only binder suffices for most uses |
| Algol 68 | Structural modes; recursion must be shielded by `ref` or `proc` | The guard rule, and its well-formedness check |
| Rust | `Box<Self>` is required, but `Vec<Self>` is not | Collections already break the size cycle |
| Swift `indirect` | Marks the indirection on the type only; construction is implicit | The marker belongs in the type, not in values |
| Wasm GC | Iso-recursive types; references inside a `rec` group become indices | Canonical indices make structural equality syntactic |
| CUE | `#List: { head: _, tail: null \| #List }`; a cycle with no exit is an error | Recursion in a structural configuration language |
| Elm | A self-referencing structural alias is an error; use a custom type | Datalove's current position, with no nominal escape |
| Dhall | No recursive types; Boehm-Berarducci encodings instead | What refusing costs a declarative language |
| JSON Schema | `{"$ref": "#"}`; Relative JSON Pointer counts ancestors | Self-reference in a serialization format |

References:

- [Algol 68 Jargon File: Well-Formedness](http://go-leon.eu/jemarch/a68-jargon/Well_002dFormedness.html)
  and [Yin-Yang](https://jemarch.net/a68-jargon/Yin_002dYang.html)
- [WebAssembly 3.0 specification](https://webassembly.github.io/spec/core/_download/WebAssembly.pdf)
- Andreas Rossberg, [Mutually Iso-Recursive Subtyping](https://dl.acm.org/doi/pdf/10.1145/3622809),
  OOPSLA 2023
- [The CUE Language Specification](https://cuelang.org/docs/reference/spec/),
  "Structural cycles"
- Amadio and Cardelli, "Subtyping Recursive Types", TOPLAS 1993
- Pierce, *Types and Programming Languages*, chapters 20 and 21

## Phase 1 as built

Datalit has recursive types, specified in botspec Section 3.8 and
[the datalit typing rules](datalit-typing-rules.md) (`Check-Roll`, and
T062 to T065 under "Well-formed types"); the grammar is in
[the datalit grammar](datalit-ebnf.md) (D056, D057). The fixtures are the
`rec_*` and `err_rec_*` files in `crates/datalove-datalit/tests/fixtures/`
and the `rec_*` files in `crates/datalove-rt-tests/tests/fixtures/roundtrip`,
which instantiate each shape and pretty-print, clone, compare and drop it
through the runtime. What differed from the plan, or was found on the way:

- **The parser reads only `rec Name T`.** A use of the name is a bare name in
  type position, which already parses as an alias; converting the hint tells
  the two apart with `RecScope`. The binder's name is filed in the alias span
  table, which is what lets Datafun's F090 point at it.
- **Inference needed one rule.** A list whose first element is an unhinted
  literal and whose second is hinted with the `rec` type kept the literal's
  structure as the element type, which resolves to the unfolding, and its
  `[]` was left open. Now a variable holding a literal's parts takes the
  `rec` type when it meets one, and the parts unify with the unfolding. The
  same meeting inside a tuple's or struct's field, which is no variable, is
  still order-dependent.
- **Descriptors are tied through a scratch table.** `TyDescTable::create_rec`
  files the type's descriptor before making the unfolding's, and the
  descriptor stands meanwhile as the unfolding made once already in a table
  of its own, where the type is a stand-in. That copy is what an option or a
  map of the type reads its size from while it is made. Nested binders pass
  the types in progress down to the scratch tables, which otherwise recursed
  without end. Each `rec` is made twice, and each nested one twice more.
- **Miri found a fault.** The descriptor's pointer was taken from its box
  before the box was moved into storage, and moving a box invalidates such a
  pointer, so the write through it and the box's free were undefined.
- **`eq_tydesc` and `cmp_tydesc` walk descriptors structurally**
  (`crates/datalove-rt/src/c/impls/cmp.rs`) and stop at pointer identity.
  Within one table a `rec` type and its unfolding meet at the type's filed
  descriptor after one level, so they finish. Two tables' descriptors of one
  `rec` type, such as datalit's and Cranelift's, never meet, and the walk
  would not end. Phase 4 has to carry the pairs being compared and take a
  pair seen again as equal. Nothing in phase 1 compares two tables'
  descriptors.
- **Datafun refuses `rec` (F090)** in its two hint conversions
  (`crates/datalove-datafun-common/src/lib.rs`) and in the type of data a
  `require data` takes in with no type declared
  (`crates/datalove-datafun-tycheck/src/datafile.rs`), which would otherwise
  bring a `rec` type into a program. Behind the refusal, `todo!()`s stand at
  the sites phase 2 and 3 replace: `IrType::from_type_hint` and
  `from_datalit_inner` in `crates/datalove-datafun-ir/src/lib.rs`, and the
  generated-literal paths that never see a `rec` (`ast_gen.rs`,
  `funlit_equiv.rs`). The generator's coverage test lists `Rec` as not yet
  generated. `generics.rs`, `is_copy_type`, `equality_blocker` and worldgen's
  `is_linear_type` have their real answers already.

## What exists, what is missing

What exists:

- Structural types as derived-equality trees.
- Aliases that expand to the types they name.
- Bidirectional checking, which always knows the expected type of a hinted
  literal.
- Collections that own heap storage.
- `TyDesc` children as raw pointers.
- Datalit's recursive types, from the parser to runtime values.

What is missing:

- `rec` in Datafun: lifting F090, `expose` at the Datafun checker's
  elimination sites, and roll through `@`.
- `Rec` and `RecVar` in `IrType`, with `expose` at every IR consumer.
- Knot-tying construction of descriptors in Cranelift and C AOT, memoized
  per-type glue in every backend, and descriptor comparison that ends on
  cycles.
- A borrowing `match ref` and `match mut`, deferred.
- Some answer to drop, clone, equality and printing of deep values.

## Phases

1. **Datalit.** Done; see [Phase 1 as built](#user-content-phase-1-as-built).
2. **The Datafun checker.** `rec` in Datafun types, `expose` at its
   elimination sites, and roll through `@`.
3. **The IR.** `Rec` and `RecVar` in `IrType`, lowering, and `expose` in the
   passes, ownership and layout.
4. **Backends.** Knot-tied descriptors in Cranelift and C AOT, memoized glue
   in the interpreter, the JIT and C AOT, and `eq_tydesc` and `cmp_tydesc`
   that end on cycles. Run the engine tests on
   recursive values. Port Storage, Json and Towers to `benchvs` as the first
   real programs.
5. **Deep values.** Measure the depth at which drop and clone overflow, then
   decide between explicit work stacks and a documented limit.

## Open questions

- Is the loss of "at most one" and "exactly two" through `[_]` acceptable,
  or will the guard eventually need a slot of its own?
- Does real data need mutual-recursion groups, or is nesting enough?
- When does borrowing `match` land?
- Does an emptied list keep its buffer through `list.pop`? Towers'
  allocation-free moves depend on it.
- Should List be ported with cloning, which measures allocation, or with
  `(which, skip)` handles, which leaves recursive types little to do?
- Should map keys and set elements be allowed to recurse, once the first
  cut is in? The values are finite, so structural order is defined, but every
  comparison is deep.
