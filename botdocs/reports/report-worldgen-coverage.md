# What worldgen covers

Written after being asked how we could be confident the generator covers the
language, and how to improve it.

## The old answer was a list

There was a coverage test. It counted twenty-three constructs by searching the
generated text for substrings and asserted things like "at least 10 if
statements". The trouble with a list is that it only reports on what someone
thought to put in it, and it was green throughout the stretch where
`gen_type_hint` was clamped to leaf types and no generated signature held a
list, a map, a tuple or an option. Nothing was watching for those, so nothing
said they had gone.

## The new answer is the AST

`coverage_tests.rs` takes its checklist from the compiler: every variant of
`Statement`, `ExprFunKind`, `BinOp`, `UnaryOp`, `ParamMode` and datalit's
`TypeHint`. Worldfiles are generated, parsed with the real parser, and walked.
Anything never reached has to be named in `NOT_YET_GENERATED` with a reason,
and anything named there that *is* reached fails too, so the list cannot rot in
either direction.

The roll and the match come from one macro invocation, and the match is
exhaustive, so adding a variant to any of those enums is a compile error until
someone says whether the generator writes it.

Two things the enum roll cannot see are listed by hand in `ALL_SHAPES`: a form
that is one variant with a field present or absent. A bare `loop` and a
`loop while` are both `Statement::Loop` and lower differently; so do an `if`
with an else and one without. That list is hand-written and can go stale, which
is the thing this report is otherwise against, but there is nowhere else to
take it from.

## Count by parsing, not by searching

The first pass of this measurement was done with regular expressions over the
generated text, and it was wrong in both directions:

- Field projection reported at 100%. It was matching the dots in
  `import io_0.fn0`. It is in fact never generated.
- Division reported at 100%. It was matching the slash in `: u32 / 5`.
- `.<=`, `.>=`, `.=` and `.!=` reported as never generated. They are generated,
  about ten times in three hundred worldfiles between them.
- Type aliases reported as used. What the text pass was seeing was `T` in a
  generic signature, which parses as `TypeHint::Alias` like any other name.
  `TypeParams` now tells those apart, and no *alias* was ever referred
  to.

A generated corpus is the wrong thing to measure with a regular expression,
because the whole point of it is that it contains things nobody predicted.

## Where it stands

Over 300 worldfiles, counting nodes rather than files. The measurement above
came first; what follows is after filling the cheapest gaps it named.

Well covered: `let` (6133), `fun` (2371), `ret` (2153), calls (1599), `var`
(551), all fourteen numeric types, `string`, `list`, `set`, `map`, `tensor`,
tuples, structs, `option`, `result`, `data`, `error`, `some`/`ok`/`er`, `@`,
generics (1204 functions, 509 of them bounded).

**Every operator in the language is now generated.** The checked and optional
arithmetic were missing because a fixed integer has no bare arithmetic and
nothing wrote the other kind. Both early-return through the enclosing
function, so which is available is decided by what that function returns --
`+!` where it returns a result, `+?` where it returns an option, and neither
in a script fragment. They are thin, one to six occurrences each in 300
worldfiles, because that coincidence is uncommon.

Filling them turned up that `gen_expr` had been picking among its options by a
ladder of conditions that only let one through when the others were false; a
fourth option bolted on the same way fired four times in sixty worldfiles. It
is a list of what is available now, picked from at random, which is what the
ladder had been approximating.

**Type aliases are referred to** (464 times), rather than declared and left.
`type_alias_usage_probability` was in the config and read by nothing. The type
that comes back from `gen_type_hint_and_spelling` is the structural one --
that is what a value gets built from, and what later statements match against
-- and only the way it is written down changes.

**`if opt |value|` is generated.** It is the only way the language has of
getting at an option's payload, and it moves out of what it destructured, so
the scrutinee has to be something the body is allowed to move and is marked
consumed before either branch is written. The result form wants an `else |err|`
as well, which is not generated, and the typechecker refuses the binding
without it.

**All four parameter modes.** `ref`, `mut` and `out` each ask something of the
call site -- a `ref` wants a binding to borrow, a `mut` or an `out` wants a
`var` to write through -- so a function taking one is only callable from
somewhere that has it, and `can_call` is what decides that. An `out` parameter
holds nothing until the body writes it, so the body opens by doing that.

**Field projection and indexing**, both of which the typechecker allows only
for a copy type: taking a heap field or element out would move it. `v.a` and
`v[i]?` parse as a place with a step rather than as `FieldProj` or `Index`,
which are for a base that is not a place, so the roll counts them under two
shapes instead.

**`v?` and `v!`**, which move out of what they unwrap.

**More functions returning an option or a result.** What a function returns is
what decides whether its body may write anything that early-returns, and with
plain return types only, seventeen expressions in four hundred and sixty were
written anywhere that could. At about a third, the checked and optional
arithmetic went from one-to-six occurrences each to fifteen-to-twenty-seven,
and `?` began to appear at all.

Thin: `if` (39), `loop` (56), `set` statements (71), `and`/`or`/`xor`, `.>`,
`.>=`, `.=`, `.!=`, places with an index step (3), `if` with a binding (5).

Still never generated, 13 of roughly 110 things counted:

| category | what |
|---|---|
| statements | `const`, `continue`, `native fun`, `match` |
| expressions | postfix `!`, a projection or an index off something that is not a place, hex literals, `table`, atoms, `term`, enum literals, `icall` |
| types | `atom`, `term`, `enum`, `table` |
| shapes | `if r \|value\| else \|err\|` |

## What it found

Four compiler bugs, each from a construct the generator had only just started
writing.

**A set's leaves were strung together by a pointer nobody initialized.**
Fixed. `alloc_leaf_node` wrote the node's tag and its length and left `next`
alone, and what the allocator hands back is not zeroed. So the chain through
the leaves -- which everything that reads a set in order walks -- ended
wherever the recycled block happened to hold a zero, and walked into whatever
it held otherwise. The map's leaves had always initialized theirs.

It took a freed block with that byte non-zero to show, which is why it stood:
a set built early in a run is cut from fresh pages, and fresh pages are zero.
The way in was a tensor handed to a generic, which boxes it on the heap and
frees the box once it is unpacked -- a forty-byte block holding, among other
things, the tensor's element count of one. The next set's leaf was cut from
that block, read its `next` as the address `0x1`, and took the printer down.
Seed 55448; `133_set_leaf_chain`.

**An index whose answer nobody read took the out-of-bounds arm.** Fixed.
`l[i]?` lowers to a get and a branch on whether the index was in bounds, and
the get defines two values: the element and that flag. Dead-code removal asked
each instruction what it defined and was told about the element only, so a get
whose element nothing read was dropped and the branch on the flag was kept,
reading a value nothing defines. `UnwrapOption`, `UnwrapResult` and the two
checked arithmetic instructions were already listed as defining a flag as
well; the three collection gets are the same shape and were not.
`134_index_flag_liveness`.

**A map indexed by a temporary leaked it.** Fixed. A get borrows its key, so
anything made to hand one over is the expression's to let go of -- on both
ways out, since the index may be out of bounds. `m["a"]` leaked the `"a"`.
Both places that lower an index had it.

**An early return from inside a branch let go of nothing.** Fixed, and the
worst of the four. An early return has to drop everything the function still
owns, and which bindings those are is worked out per statement and looked up
by the statement's id. Ownership analysis numbers every statement it walks,
nested ones included; lowering set the current id from the statement's
position among the *top-level* ones. The two agree exactly as long as nothing
nests, and an `if` with a statement in it puts them out of step for the rest of
the function.

So a `?`, a `!`, or a checked overflow inside a branch looked up the enclosing
statement's drops, found none, and left without dropping anything. The same
shape at the top of a body was right all along, which is what made the early
return look like it worked. `806_int_slot_reassign_loop` had been recording the
wrong answer -- its IR dump gained the two drops the overflow path owed --
and `135_nested_early_return_drops` covers the shapes.

## What this still does not tell us

**Generated is not exercised.** The 1000-seed test only typechecks, so it never
runs ownership analysis and never executes anything. Only the dual test does,
and it runs 20 seeds by default. A construct can be covered here and never have
been compiled by a backend.

**Unigram coverage is the weakest kind.** Every bug the generator has turned up
was at an intersection: a generic over a map at `data` on one side; a tensor
of a tuple holding a heap value; a call to a function returning `()`. A roll of
node kinds calls all of those covered. What is wanted is pairs -- each
parameter mode against each type class, each generic shape against each
collection, each operator against each width.

**It says nothing about the compiler.** The other half of the question is which
compiler paths the corpus reaches, which wants line coverage of the compiler
crates under the dual test rather than anything measurable from the generated
text.

## Next

1. `match`, which is the largest thing left and has its own lowering.
2. `if r |value| else |err|`, postfix `!`, and a projection or an index off
   something that is not a place.
3. `const`, enums, terms, tables, atoms.
4. Raise the `if` and `loop` weights. They are where D007 and D008 and the exit
   drops live, and the four bugs above say what nesting is worth.
5. Log the silent fallbacks. `gen_set` and friends fall back to `gen_let` when
   they cannot proceed, so a construct can be rare because it keeps failing to
   build rather than because it was weighted that way, and nothing says which.
