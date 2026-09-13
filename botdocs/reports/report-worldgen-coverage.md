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

Thin: `if` (39), `loop` (37), `set` statements (71), `and`/`or`/`xor`, `.>`,
`.>=`, `.=`, `.!=`, and each of the overflow operators.

Still never generated, 19 of roughly 110 things counted:

| category | what |
|---|---|
| statements | `const`, `continue`, `native fun`, `match` |
| expressions | postfix `?`, postfix `!`, field projection, indexing, hex literals, `table`, atoms, `term`, enum literals, `icall` |
| parameters | `out`, `mut` |
| types | `atom`, `term`, `enum`, `table` |
| shapes | `if r \|value\| else \|err\|` |

## What it found

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

Found at seed 55448, by a corpus that had only just started writing tensors
through generics. `133_set_leaf_chain` covers it, with a list and a map in
place of the tensor as well.

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

In rough order of what the compiler is most likely to have got wrong:

1. `mut` and `out` parameters, and `match`. New lowering paths, and the
   ownership analysis has the most to say about them.
2. Postfix `?` and `!`, `if r |value| else |err|`, field projection, indexing.
   Fallible and place-forming paths.
3. `const`, enums, terms, tables, atoms.
4. Raise the `if` and `loop` weights. They are where D007 and D008 and the
   exit drops live, and they appear in an eighth of seeds.
5. Make the overflow operators less rare, which wants more functions returning
   a result or an option, or a body that reaches for a fixed integer when it
   is in one.
6. Log the silent fallbacks. `gen_set` and friends fall back to `gen_let` when
   they cannot proceed, so a construct can be rare because it keeps failing to
   build rather than because it was weighted that way, and nothing says which.
