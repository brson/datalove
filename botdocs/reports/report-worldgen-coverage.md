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

**A field or an element off a value nobody keeps.** `f().0` and `f()?[i]?`,
as against `v.a` and `v[i]?`. The second pair is a place and a step from it,
which the place walker lowers; the first is a value the call made, which the
expression is the only owner of, and only that pair is a `FieldProj` or an
`Index`.

Mostly the index has to look through a wrapper -- a callable function answers
with `![u32]` far more often than with `[u32]`, now that most returns are
fallible -- so it unwraps and then indexes, `f()?[i]?`, with both marks the one
the enclosing function takes. Mixing them is a type error.

The candidate is settled without writing the call, and the call is only written
if the choice is taken. Writing it while collecting candidates meant every
expression generated a call whose arguments are expressions, which is not a
recursion that stops: the 1000-seed test overflowed its stack.

**Consts**, at module top level and in a body, and read without being
consumed. A const names a value rather than a place, so each mention produces
one of its own -- it is the one name in the language a linear type can be read
from twice with no `@`, and everything else the generator writes is moved on
first use. Bound to literals: a const expression may name only other consts,
and a module-level one that calls a function which itself names a const is a
cycle.

Some shapes of const are kept out of the corpus, because the compiler falls
short of what the spec says a const may hold, which is "anything a function can
compute". Two of the reasons were looked into and fixed; what is left is
written down below under "Consts, in more detail".

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

**Tables**, the last of the language's types nothing built. A table is written
as a type and as a literal -- `{| x: u32, y: string |}` and `{| x, y; 1, "a"
|}` -- and it is now one of the types a value can be asked for, so it turns up
wherever any other does: bound, passed, returned, cloned, dropped, held in a
list or a set or a map, and written into a constant.

What a table *cannot* do is anything else. The spec gives it one operation --
a column projection, `t.x`, yielding a list view -- and that is not
implemented; nor is indexing a row, nor a length, nor iteration. There is no
`table.dfm` in `sys/std`. So the corpus builds tables, moves them about, and
drops them -- which is worth doing, since a table with a list column and a map
column has to let go of both -- and that is the whole of what there is to
write. Nothing reads a value back out of one, because there is no way to.

Thin: `if` (39), `loop` (56), `set` statements (71), `and`/`or`/`xor`, `.>`,
`.>=`, `.=`, `.!=`, places with an index step, `if` with a binding.

Still never generated, 3 of roughly 110 things counted:

| category | what |
|---|---|
| statements | `native fun` |
| expressions | hex literals, `icall` |

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

**A function naming a module const could not call a generic that builds
anything.** Fixed. Which calls have to hand a descriptor over is worked out by
closing the shapes over the call graph, and that was done once, over everything
lowered at the time. A function naming a module-level const is not lowered at
that time: a const is evaluated against what is already lowered, so those
functions are held back and lowered afterwards, in a second round the closing
never saw. One of them calling a generic that builds a list, a set or a map
was compiled without the descriptor the call had to pass, and the interpreter
came down on "a shape built with is one this function declared". A generic
building nothing was fine, which made it look like a problem with consts.
`062_const_then_generic`.

**Leaving a function from inside a condition let go of nothing.** Fixed. A
`?`, a `!` or a checked overflow early-returns from wherever it is written, and
a condition is one of those places. An `if` had its condition's moves analysed
but nothing recorded for a way out of it, and a `loop` did not look at its
condition at all -- not for moves and not for leaving. A loop's is the more
surprising, being read again every round.
`139_condition_early_return`.

**A field read off a value nobody keeps leaked the rest of it.** Fixed. A
projection whose base is not a place reads one field out of a value made for
it, and left the value alone afterwards. Only a copy field can be projected at
all, so the field is never what leaks -- it is always the siblings, and a
tuple of a `u32` and a `string` projected at the number leaked the string, with
the right answer and nothing said. `138_projection_off_a_value`.

**An integer under a hint that is not an integer type brought the compiler
down.** `check_int_fits_type` panicked rather than reporting. `let v: bool = :
bool / 0` is a thing a person can write, and saying so is the answer. The
generator was writing `: bool / 0` for a map keyed by `bool`, which is how it
was found and is also a generator bug, fixed by only offering an
integer-keyed map to index.

And four from the tables, which nothing had ever built. Three were in the
backends and one was not about tables at all.

**A table with no rows could not be built.** The runtime is handed a
descriptor for a row every time one is pushed, and a row is a tuple of the
columns. The cranelift backend emitted descriptors only for types something
mentioned, and a table with rows mentions that tuple in the rows themselves.
An empty one mentions it nowhere. A row's descriptor is emitted with the
table's own now.

**A table constant wrote a type tag of 7.** Rather than ask for the row
descriptor, the constant path built one on the stack, field by field, at
offsets spelled out in a comment, with the tag written as a literal. `Tuple`
was numbered 7 once; it has been 0x40 for a long time. The runtime asserted on
the tag and the program died where the constant was built. It reads the
emitted descriptor now, and sixty lines of hand-building are gone.

**The C backend refused a table constant** and wrote a table literal's rows
into a buffer it had not aligned. Both fixed; `144_table_shapes`.

**A constant of a `data` named a type nothing else in its unit did.** Not a
table bug -- the generator's roll moved when tables were added to it, and
this was underneath. A `data` is packed against a descriptor for what it
holds, and since the value of one is just `Data`, the type it holds appears
nowhere in the unit's values. A const of a `data` over a list of `i64` was the
only mention of `[i64]` anywhere, so no descriptor was emitted and codegen had
nothing to point at. The collector walks constants now.

**Two packed values could not be compared unless both were on the heap.** Also
not a table bug, and the worst of the four. A `data` holds what it was built
from in one of three ways -- behind a pointer, inline beside a descriptor, or
packed into the words themselves -- an `error` is the same three under another
tag, and an `error` is how the error side of a result is held. The comparison
was written three times. The `data` one read all three encodings. The `error`
one read the two words raw, which orders by bit pattern: every negative
integer above every positive one. The result one reached straight for the
value pointer, which only the heap encoding has, and stopped the program on
"value not stored as pointer".

So a set of results whose errors held a `bool` could not be built: inserting
the second compared it against the first and died. The interpreter was fine,
which is what made it a mismatch rather than a crash on both sides. All three
read through one comparison now, and the two hand-rolled ones are gone.
`145_packed_error_ordering`.

## Consts, in more detail

The spec says a const holds anything a function can compute. Seven things
stood in the way, and the last of them was not one of the ones it started as.
All seven are fixed.

**Fixed: a type could not be reconstructed from the descriptor it came with.**
A const is evaluated by running it, and what comes out is read back into
something the compiler can write down again. An `error` carries whatever it was
built from, so reading one means reconstructing a type from the descriptor
beside it -- and that reconstruction handled the scalars and a string and gave
up on the rest, under a note that a complex type would want recursion. A
descriptor carries the whole of a type, so it does. `const C: !index = er error
(a, tuple)` could not be evaluated at all.

**Fixed: a `data` and an `error` could not be read back.** Neither had an arm
in the extractor, so neither could be a const whatever it held. Both are read
through `data_borrow`, which takes the three ways a `data` is packed -- on the
heap, inline with a descriptor, inline with only a tag -- and answers the same
way for each. Borrowed rather than unpacked, because what the evaluator is
looking at still belongs to the frame it was computed in.

`063_const_reads_back` covers both, twelve shapes of them.

**Fixed: a tensor could not be read back.** There was no `ConstValue` that
held one, so a const of a tensor could not be evaluated at all. It has a
variant now, carrying the shape as well as the elements -- a list's elements
are the whole of it, while nine numbers are a three by three or a nine by one
depending only on the shape, and the rank belongs to the type while the extents
belong to the value. Read back by walking the run of elements, and written by
handing that run and the shape to the same call a tensor literal makes.

`142_tensor_consts` runs eight shapes through all four backends, including an
element wider than a word, one carrying a string, and a tensor nested in a
tuple and in an option.

**Fixed: a `data`, an `error` and a result.** Not for any of the reasons
above -- reading one back had been working since the second fix. A `data`
carries its own descriptor at run time and `ConstValue::Data` kept the value
without it, so a backend writing one worked the type out from the value, and a
value cannot always say it: an empty map gives `Map(Unit, Unit)`, which is not
a type the program has and which the cranelift AOT had no descriptor for.

The constant keeps the payload's type beside the value now, the way the tensor
keeps its shape -- the type it was read back as, which is the type the
descriptor named. Four writers again, and a result came with them, its error
side being an `error`. `143_data_const_payload_type`.

So a const holds anything a function can compute, which is what the spec says,
and worldgen writes every type into one.

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

1. Log the silent fallbacks. `gen_set` and friends fall back to `gen_let` when
   they cannot proceed, so a construct can be rare because it keeps failing to
   build rather than because it was weighted that way, and nothing says which.
2. `native fun` and `icall` each want something the generator does not have: a
   rider to resolve against, and the names of the intrinsics.
3. The interactions, which is where every bug so far has been. A roll of node
   kinds says nothing about a generic over a map at `data` on one side, or a
   checked overflow inside a branch of a function returning a result.
