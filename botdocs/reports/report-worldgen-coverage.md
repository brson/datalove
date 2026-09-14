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

**Enums, atoms, terms, and `match`.** An enum is what a `match` takes apart,
and a match has to name the variants of the type it is matching, so the enums
are declared per module as named type aliases -- `type Enum0_0: enum { atom
Enum0_0V0, term Enum0_0V1 u64 }` -- and a `match` is written over a binding of
one. The arms cover every variant, or some of them and a `case default`;
without a default the match must be exhaustive, and a default over an already
exhaustive match is refused as unreachable. A term arm binds its payload.

The input is moved by the match, and the arms are branches, so the same rule an
`if` keeps applies: the scrutinee has to be something the body may move, and no
arm may move what was declared outside it. Atoms and terms are also written on
their own, as their own types.

**Both ways of saying an enum value.** `(atom Red)@` widens the variant into
the enum; `enum { atom Red }` names the enum and lets the variant be checked
against it. They take different paths through the compiler -- the coercion
builds the variant, the literal *is* the variant, checked -- and the first of
those was wrong for two years' worth of variants until this turned it up.

**`?` and `!` driven by what is in scope**, rather than by a type wanted
somewhere. Waiting for a binding of the right shape to be wanted at exactly
the right type left `!` unwritten across three hundred worldfiles at a
stretch, and left the roll flipping between runs. A statement that unwraps
something in reach and binds what comes out is reliable: 22 and 15 in 300.

**Both `if` bindings.** `if opt |value|` and `if r |value| else |err|` are the
two ways the language unwraps without early-returning, and the result form's
else branch has to be there and has to bind: the typechecker refuses one
without it. Both move out of what they destructure, so the scrutinee has to be
something the body may move.

Reached for on its own rather than waiting on the `if` roll and the binding
roll to coincide -- which had written the option form three times in three
hundred worldfiles and the result form once, and had the roll flickering
between runs. It is 140 and 40 now, and `if` altogether went from 44 to 179,
which is the other half of a to-do on this list: `if` is where D007 and D008
and the exit drops live.

The six forms that kept reaching zero between runs now have floors in
`test_core_constructs_are_common`, so losing one is a failure rather than a
shrug.

**`continue`**, which had been left out under a note saying it would make a
loop's final break unreachable. It would, written bare: everything after it in
the same block is unreachable, the `break` included, and that break is what
keeps a generated loop from running forever. Inside a branch, what follows is
reached on the rounds that do not take it.

The guard is a flag put out by the branch rather than a generated condition. A
condition that happens to be `true` continues every round and never reaches the
break, which is a loop that does not end -- the first thing this wrote was
`loop / if true / continue`. With a flag the first round goes back and the
second falls through. What that round has bound by then is the loop's to let
go of on the way, which is a path nothing else in the corpus takes.

Thin: `if` (39), `loop` (56), `set` statements (71), `and`/`or`/`xor`, `.>`,
`.>=`, `.=`, `.!=`, places with an index step, `if` with a binding.

Still never generated, 5 of roughly 110 things counted:

| category | what |
|---|---|
| statements | `const`, `native fun` |
| expressions | a projection or an index off something that is not a place, hex literals, `table`, `icall` |
| types | `table` |

## What it found

Seven compiler bugs, each from a construct the generator had only just started
writing. Four from the first round of gap-filling:

**A set's leaves were strung together by a pointer nobody initialized.**
`alloc_leaf_node` wrote the node's tag and its length and left `next` alone,
and what the allocator hands back is not zeroed, so the chain through the
leaves ended wherever the recycled block happened to hold a zero and walked
into whatever it held otherwise. The map's leaves had always initialized
theirs. Seed 55448; `133_set_leaf_chain`.

**An index whose answer nobody read took the out-of-bounds arm.** `l[i]?`
lowers to a get and a branch on an in-bounds flag, and the get defines two
values. Dead-code removal was told about the element only, so a get whose
element nothing read was dropped and the branch on the flag was kept, reading
a value nothing defines. `134_index_flag_liveness`.

**A map indexed by a temporary leaked it.** A get borrows its key, and nothing
let go of one made to hand over. Both ways out want it, since the index may be
out of bounds.

**An early return from inside a branch let go of nothing.** Ownership analysis
numbers every statement it walks, nested ones included; lowering set the
current id from the statement's position among the *top-level* ones. Those
agree only while nothing nests. So a `?`, a `!`, or a checked overflow inside a
branch looked up the enclosing statement's drops, found none, and left holding
everything. `806_int_slot_reassign_loop` had been recording the wrong answer.
`135_nested_early_return_drops`.

Three more from the enums:

**Every atom widened into an enum came out as the wrong variant.** An enum
keeps a discriminant saying which variant it holds, and an atom has nothing to
copy across, being zero-sized. The coercion copied anyway, wrote nothing where
the discriminant goes, and left whatever the slot held -- zero in a fresh
frame. `(atom Red)@` into `enum { atom Red, atom Blue }` was Blue, and a
`match` on it took the Blue arm. A term had to be cloned rather than handed
over as well, since `@` borrows and the enum owns what it is built from.
`136_enum_widening`.

**A term built from a binding freed it twice.** A term is its payload under a
name and takes it, and the ownership analysis walked into a `some`, an `ok`,
an `er`, a `data` and an `error` to say so, and fell through to doing nothing
for a term. Nothing was marked moved, so the term and what it was built from
were both dropped. An enum literal had the same gap.
`137_term_payload_ownership`.

**An integer under a hint that is not an integer type brought the compiler
down.** `check_int_fits_type` panicked rather than reporting. `let v: bool = :
bool / 0` is a thing a person can write, and saying so is the answer. The
generator was writing `: bool / 0` for a map keyed by `bool`, which is how it
was found and is also a generator bug, fixed by only offering an
integer-keyed map to index.

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

1. A projection or an index off something that is not a place -- `f().0`,
   `f()[i]?` -- which is what the `FieldProj` and `Index` nodes are for.
2. `const`, and tables.
3. Log the silent fallbacks. `gen_set` and friends fall back to `gen_let` when
   they cannot proceed, so a construct can be rare because it keeps failing to
   build rather than because it was weighted that way, and nothing says which.
4. `native fun` and `icall` each want something the generator does not have: a
   rider to resolve against, and the names of the intrinsics.
5. The interactions, which is where every bug so far has been. A roll of node
   kinds says nothing about a generic over a map at `data` on one side, or a
   checked overflow inside a branch of a function returning a result.
