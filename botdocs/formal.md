# Proving the Type System

How datalove's type rules are modelled in Lean, what is proved of the model,
and how the model is held to the Rust. Explanation and how-to; written
November 2026, when the first part, the type language of datalit, went in,
and extended as the second, datalit's literal typing, and the third, a slice
of datafun's statements, did.

## Contents

- [The problem](#user-content-the-problem)
- [The shape of the answer](#user-content-the-shape-of-the-answer)
- [What is modelled](#user-content-what-is-modelled)
- [What is proved](#user-content-what-is-proved)
- [What ties the model to the code](#user-content-what-ties-the-model-to-the-code)
- [What it found](#user-content-what-it-found)
- [Literal typing](#user-content-literal-typing)
- [Statements](#user-content-statements)
- [Running it](#user-content-running-it)
- [Keeping it true](#user-content-keeping-it-true)
- [What is not covered](#user-content-what-is-not-covered)

## The problem

The typing rules in [Datalit Typing Rules](datalit-typing-rules.md) and
[Datafun Typing Rules](datafun-typing-rules.md) say what the checker accepts,
and the docs state properties of them: `≡` is syntactic, `@` never narrows,
a recursive type and its unfolding stand for each other. Nothing checks
either that the properties hold or that the code does what the rules say. A
property that fails shows up, if at all, as a program a user expected to
check and that did not.

## The shape of the answer

Three pieces, each covering what the others cannot:

1. **A model in Lean 4** (`formal/`), which transcribes the Rust functions
   case for case, so that what it computes is what the Rust computes.
2. **Proofs about the model**, which say a property holds for every type, not
   only the ones a test happened to try.
3. **Differential tests** (`crates/datalove-formal-tests`), which run the
   model as an oracle and the Rust side by side on generated inputs and
   compare every answer. They are what make the proofs say something about
   the Rust: a proof is about the transcription, and the tests say the
   transcription is faithful.

The Rust is not translated into Lean mechanically. It runs on salsa and
interned names, which tools such as Aeneas do not take; a hand transcription,
checked by testing, is cheaper and follows refactors without ceremony.

Beside them, the **law tests** check the properties the model proves directly
against the Rust, with no Lean needed, so that they run in `just test`. A law
the Rust breaks is one the model cannot prove, and the law test finds it
faster than a failed proof says why.

## What is modelled

The type language of datalit, as `crates/datalove-datalit/src/tycheck/types.rs`
has it. Each Lean definition names the Rust function it transcribes.

| Lean | Rust | File |
|------|------|------|
| `Ty` and its lists | `Type` | `Datalove/Ty.lean` |
| `Ty.subst`, `unfold`, `expose` | `substitute`, `TypeRec::unfold`, `expose` | `Datalove/Ty.lean` |
| `Ty.equiv` | `types_equivalent` | `Datalove/Relations.lean` |
| `Ty.meet` | `types_equivalent_at_head` | `Datalove/Relations.lean` |
| `Ty.canWiden`, `Ty.canAdapt` | `can_widen_to`, `can_adapt_to` | `Datalove/Relations.lean` |
| `Ty.isCopy` | `is_copy_type` (datafun-tycheck) | `Datalove/Classes.lean` |
| `Hint`, `Hint.convert` | `TypeHint`, `convert_type_hint` with `NoAliases` | `Datalove/Hint.lean` |
| `Ty.WF` | the conditions T062, T064 and T065, and T066 for a type parameter as a `Ty`, stated without names | `Datalove/WF.lean` |

`Type::Rec` is `Ty.mu`, because `Ty.rec` is the name Lean gives the
recursor. A `rec`'s name is no part of the model, as it is no part of the
Rust type's equality. The hint conversion is modelled step for step,
`RecScope` and all, so that the first error the model reports, and the name
it reports it at, are the Rust's.

## What is proved

Under `formal/Datalove/Proofs/`:

- **`≡` is equality** (`Ty.equiv_iff_eq`). In the representation the Rust
  uses, with enum variants sorted and `rec`s by de Bruijn index,
  `types_equivalent` is syntactic equality. So it is an equivalence relation,
  and the derived `PartialEq` on `Type` agrees with it.
- **Integer `@` preserves every value** (`Ty.adapt_int_preserves`): when
  `@` converts one integer type to another, every value of the first is a
  value of the second, for any width of `index` and `offset`.
- **`@` never narrows** (`Ty.adapt_numeric_antisymm`), and numeric `@` is
  transitive (`Ty.adapt_numeric_trans`).
- **`≈` is an equivalence relation** (`Ty.meet_iff` and the laws after it
  in `Proofs/Meet.lean`): two types meet just when they expose alike.
- **The conversion makes only well-formed types** (`Hint.convert_wf`). The
  checks `convert_in` makes as it goes, by name and with a mutable scope, add
  up to the conditions `Ty.WF` states by index. So every theorem below that
  assumes a well-formed type covers every type the checker builds from a
  hint.
- **`expose` ends** (`Ty.expose_not_mu`): on a well-formed type, unfolding as
  many times as the type has leading `rec`s leaves a head that is not a
  `rec`. The Rust loops until it gets there, and would not stop on
  `rec X X`; T064 is what rules that out.
- **A well-formed recursive type does not expose to a number**
  (`Ty.expose_not_numeric`), since its body uses its name.
- **A literal that checks is a value of its type** (`Lit.check_sound`); see
  [Literal typing](#user-content-literal-typing).
- **In datafun, every early return fits the function, and `completes` is
  sound**; see [Statements](#user-content-statements).
- **A recursive type meets its one-step unfolding** (`Ty.meet_unfold`), on
  well-formed types, however many `rec`s it starts with.
- **`@` is reflexive and transitive** on well-formed types (`Ty.adapt_refl`,
  `Ty.adapt_trans`), so it never needs to be written twice.

The build fails on any warning, and a `sorry` is one, so a proof left
unfinished cannot pass for a finished one. Every theorem rests on Lean's
three standard axioms and nothing else: `#print axioms` on any of them shows
no `sorryAx`, and no `Lean.ofReduceBool`, which `native_decide` would bring
in. The finite case splits use `decide +kernel`, which the kernel checks.

## What ties the model to the code

`oracle_tests` generates type hints from seeds, biased toward `rec`s and
toward name uses that are guarded, and asks both sides:

- what each hint converts to, or the first thing wrong with it, by kind and
  name;
- for each type the hints convert to, whether it is copy, whether the model
  finds it well formed, and what it exposes to. The Rust's answer to the
  second is always yes, since these are the types it accepted: this checks
  that the model accepts whatever the Rust does, and not the reverse, which
  `Hint.convert_wf` covers for the model's own conversion;
- for pairs of those types, `≡`, `≈` and `⇝`. Random pairs are rarely
  related, so most pairs are built to be: each type with itself, its
  one-step unfolding and its full unfolding, every pair of scalars, each enum variant with its enum and the
  recursive type around it, and each atom or term with an enum that lists it.

The oracle reads JSON lines and answers in kind; the encoding is in
`formal/Datalove/Json.lean`. `just test-formal` runs a thousand seeds by default, which ask about
forty-five thousand questions and take a few seconds; the test alone defaults
to two hundred.

`law_tests` checks, against the Rust alone, that `≈` and `⇝` compose on every
chain of two related pairs the generator makes (about two and a half thousand
chains at the default fifty seeds), and, exhaustively over every hint of up to
seven nodes in a small grammar with three `rec` names, that each recursive
type meets and adapts both ways to its one-step and its full unfolding. The
grammar has three names because that is what a type beside its one-step
unfolding takes, and the test counts the types that start with more than one
`rec` (108 at the default size), which are the ones that matter.

## What it found

`@` was not transitive, for three reasons. Writing the transitivity proof
found the first two, and the law tests confirmed them before either was
fixed:

- An atom or term did not adapt to a recursive enum. With
  `type Json: rec J enum { atom Null, term Arr [J] }` and `a = atom Null`,
  `let j: Json = a@` was F091, though `let e: enum { atom Null, term Arr [Json] } = a@`
  and then `let j: Json = e` both checked, and so did the literal
  `let j: Json = atom Null`.
- A recursive type whose unfolding is a term, `rec X term A [X]`, did not
  adapt to an enum listing `term A [rec X term A [X]]`, though its unfolding
  did.

`can_adapt_to` now looks up its table of conversions with both sides
exposed. Lowering needed no change: it sees types already exposed. The
backend fixture `43_adapt_into_recursive_enum` runs both cases on every
backend.

The third was missed. The first version of this work proved transitivity
assuming that two well-formed recursive types which unfold alike are one
type, and tested the assumption without finding a counterexample. A review
found one: `rec L rec M [(L, M)]` and its one-step unfolding
`rec N [(rec L rec M [(L, M)], N)]`. Writing the second takes three `rec`
names, and the exhaustive search had two. Meeting at the head compared one
side's unfolding with the other side as it was, so each of the two met their
common unfolding and not each other, and `@` from one to the other was F091.
`types_equivalent_at_head` now unfolds both sides, which makes `≈` equality
of unfoldings, and the assumption is gone. The backend fixture
`44_recursive_types_unfolding_alike` holds the case. Cranelift AOT could not
compile such a type at all, with or without the change: the two types each
reserved the descriptor of their common unfolding, and now share it.

Two things the tests confirmed that the docs only claimed: `≡` and the derived
`PartialEq` agree, and the conversion reports the same first error as a
direct reading of the rules would, in every one of the error kinds.

## Literal typing

The second part takes the typing of datalit's expressions: inference with
literal classes and unification, the defaults, checking and synthesis
(`formal/Datalove/Lit.lean`). It is a different kind of model from the first.
The type relations were transcribed from the Rust, so that proofs about them
are about the code. The literal typing is written from
[the typing rules](datalit-typing-rules.md) instead, rule by rule, so that
holding the Rust to it holds the code to the doc. Where the doc leaves a
choice open, the model says which it made. It is not wholly independent of
the code: it was written by someone who had read the checker first, and its
structure follows the checker's where the doc allows.

Inference, unification and synthesis are `partial`, since unification has no
structural measure, and serve only as the oracle. Checking is total: it
takes synthesis, for a `data` or `error` payload, and Check-Subsume as
parameters (`checkWith`), and the oracle's `check` is it given the partial
ones. Of checking one thing is proved, `Lit.check_sound`: a literal that
checks against a type denotes a value of that type. What a type's values are
(`HasType`, in `Proofs/LitSound.lean`) is written from the doc's tables, not
from the checker: an integer in the type's range, a hex literal that fits its
bits, a tensor with as many elements as its shape, a table whose header is
the type's and whose rows have one element per column, an atom or term that
is a variant of the enum, a recursive type's values its unfolding's. The
theorem holds for any synthesis, and for any Check-Subsume that never
succeeds. The real one always ends in T032, but it calls the partial
inference, so Lean takes that as a premise rather than a fact. Dropping the
tensor element-count check from the checker breaks the proof, as it should.

`lit_tests` holds the Rust to it. It starts from an expression `ast_gen`
makes, or from a literal of a type the hint generator above makes, which
brings in recursive types, and changes it up to three times at random: a
hint dropped, replaced, or stripped from a whole part so that inference types
it; a part swapped for another expression; an integer or hex literal at the
edge of a range; a variant renamed, or made an atom from a term or the
reverse; a struct field dropped or renamed, a tuple lengthened, a table
column renamed. Each is printed, parsed back, and synthesized, and the type
the Rust synthesizes, or the code of the first diagnostic it reports, is
compared with the oracle's. A hinted expression `: T / e` is checked against
`T`, so both modes are asked about.

Before the generated cases it runs a corpus: every example the typing rules
give, with what the doc says each synthesizes or is reported as, held to the
doc as well as to the oracle, and the cases found by hand. A hundred thousand
generated expressions reach every code in the doc's table but four: T017, a
parse error, which the test skips; T066, which is datafun's; and T049 and
T056, a tensor's element count and a table row's width, which the parser
refuses before the checker sees them, as the doc says.

It found one place where the code and the doc differed. The doc gives an
atom literal's class as admitting `atom a` and any enum, and a term
literal's as admitting `term a T` and any enum. The code's let either admit
the other kind of the same name, so in `[atom A, : term A int / term A 1]`
the atom's variable took the hinted term's type, the list became a list of
terms, and the atom was reported (T032), where the doc's rule that the first
in source order keeps the shared type reports the hint (T040). The generator
did not find this; a case written by hand did, and the code now follows the
doc. The datalit fixture `79_atom_then_hinted_term_of_its_name` holds it in
`just test`.

## Statements

The third part takes datafun: a slice of
[its typing rules](datafun-typing-rules.md), written from the doc as the
literal typing was (`formal/Datalove/Fun.lean`). The slice is scripts of
functions and statements: `let`, `var`, `set` with or without an operator
and through fields and indexes, `if` and the binding `if`, `loop` and
`loop while`, `for`, `break`, `continue`, `ret`, `debuglog` and `call`;
datalit's literal forms, which datafun types by datalit's rules with a
context added; and datafun's variables, places with field and index steps,
fields and indexes of what is not a place, operators, `?`, `!`, `@` and
calls, each in a borrowed position or not. It leaves out generics, modules
and requires, consts, type aliases, patterns and `match`. Inference is
datalit's, extended by the doc's table: the `@` class lives in `Lit.lean`'s
unifier, as datafun's lives in datalit's in the Rust, and so does an
`unknown` class for what inference cannot read, which the Rust marks by a
variable's origin.

The checker reports what is wrong and goes on, so the model's answer for a
script is every diagnostic code, in order, and so is what it is held to. That
holds the model to the Rust's recovery as well as its rules: what is bound
after a failed `let`, which bodies of a binding `if` are checked, where a
call stops. The doc said nothing of that, and now says what the tests found
([Going on after an error](datafun-typing-rules.md#user-content-going-on-after-an-error)).

`fun_tests` holds the Rust to it. Scripts come from worldgen, configured to
stay inside the slice, as the single module it writes, checked as a script.
Worldgen's bodies use their bindings little, so each script is changed up to
three times a line at a time: a type or value changed, an operator swapped,
`?`, `!` or `@` added, a name unbound, a `ret` dropped, a stray `break`,
`set` or `call`, or a statement added that computes from the parameters and
locals in scope, unwraps them, matches on them with a binding `if`, calls
with them, reads a field or element of them, borrowed or not, or sets one. The Rust's serde AST (`datalove-datafun-ast/src/ast_serde.rs`) is
the oracle's input, read as it is (`formal/Datalove/FunJson.lean`); a script
using a form outside the slice is skipped and counted. A corpus of cases
written by hand runs first; worldgen's parameters are mostly scalars, so it
is the corpus that reaches F069 to F073 most. Thirty thousand scripts from a
random seed agree on every diagnostic, all but the 1773 that do not parse
inside the slice, reaching every statement code the slice has.

What it found:

- **No rule of the slice differs** between the doc and the code. The
  differences were all recovery, which the doc did not cover, and which the
  model now follows.
- **The serde AST dropped `loop while`'s condition.** It is what the parser's
  snapshot tests compare, so a `loop while` lost its condition there with
  none of them noticing. It is now kept.
- **A negated literal ignored its own hint.** Check-NegLit range-checked
  `-n` against the expected integer type whether or not `n` carried a hint,
  so `let a: i8 = -(: string / 8)` was accepted, and ran, the hint never
  looked at. The rule now applies to a bare literal only, and a hinted one is
  checked as any negation is: there F016, and F026 for `-(: i64 / 8)`. A
  mutation that changed a binding's type from `int` to `u8` found it; the
  error fixture `128_negated_literal_with_a_hint` holds it.
- **Two of the model's own rules were wrong**, found when projections
  widened what the tests reach. A literal form with no rule for the
  expected type inferred itself first, so `ret some x` with `x` unbound was
  F001 in the model and F016 in the Rust, which only describes the literal
  there. And the model threw where inference could not read a name, where
  the Rust leaves a variable to be synthesized, which decides between F001
  and T016 in `(x, none)`. Both now follow the Rust, and the doc says how
  the second goes.
- **A temporary's field was refused where its element was not.**
  `f().name` of a non-copy field was F070, while `f()[0]?` of a non-copy
  element had no F072 check, though nothing is left behind in a temporary
  either way. F070 now applies to a field of a place only. Lowering was not
  ready for it: it read the field out of the temporary and then dropped the
  temporary whole, and borrowing a field of one leaked the temporary. The
  field is now cloned out, as an element is, and a borrowed one's temporary
  is dropped once the borrow is done; the backend fixture
  `45_field_of_a_temporary` holds both.
- **Worldgen could recurse without end.** An argument of a call may itself
  be a call, with no bound, so a function of enough parameters drew calls
  inside calls until the stack overflowed: past one expected call per call,
  which three parameters reach at a call probability of a half. Calls now
  nest at most six deep, checked after the draw, so that every seed short of
  the bound writes what it wrote before; three hundred default-configured
  worlds are byte for byte the same.

Two things are proved of it:

- **Every early return fits the function** (`Fun.checkFun_early_returns_fit`,
  in `Proofs/EarlyReturn.lean`). A function whose check reports nothing has
  every early return in its body, from `?`, `!`, `-?`, `-!`, an index's `?`
  or `!`, read or in a `set` target, or a checked or optional operator, at
  any depth of any expression of any statement, one its return type can
  carry: `none` only where it returns an option, an error only where it
  returns a result. Underneath it, `Fun.checkF_guarded` says the same of any
  expression that checks or synthesizes. Taking `fits` out of `?`'s rule
  breaks the proof, as taking F049 out of a place's index step does.
- **`completes` is sound** (`Fun.completes_sound`, in
  `Proofs/Completes.lean`). A body it says cannot complete has no execution
  that finishes normally, against a semantics of control flow in which any
  statement holding an expression may return early, a condition may go either
  way, and a loop runs until a `break`, a return or, for `loop while`, its
  condition, as a `for` does. So a function F063 lets through always
  returns. Saying a `loop while` cannot complete breaks the proof.

To make these provable, checking was made total: it takes inference as a
parameter (`InferEnv`), which only says what types are and so cannot move an
early return, and it runs on fuel, every call spending one, so that the
recursion between checking and synthesis needs no measure. Running out is a
failure, never a success, so the theorems hold for any fuel; the oracle gives
a million. A statement's expression checks run on a fresh list of
diagnostics, added to the statement's after, so that a statement that adds
none is one whose checks all succeeded. And the statement checker is written
with its binds explicit, the `match` and `if` of `do` notation otherwise
copying what follows into every branch. Each of these leaves the answers as
they were, which `fun_tests` confirms.

## Running it

Lean comes through elan; `formal/lean-toolchain` pins the version, which
`lake` fetches on first use.

- `just formal-build` builds the model and the oracle, checking every proof.
  About twenty seconds from clean.
- `just test-formal SEED COUNT` builds, then runs `oracle_tests`,
  `lit_tests` and `fun_tests` over `COUNT` seeds from `SEED` (`random` draws one), as every generative suite takes its
  seeds.
- `law_tests` runs in `just test`. `FORMAL_LAWS_SEED`, `FORMAL_LAWS_COUNT` and
  `FORMAL_LAWS_SIZE` widen it. Size 8 takes a few seconds in release; size 10
  runs out of memory, and is also the smallest at which two distinct types
  that unfold alike can both be written, which is why the test checks each
  type against its own one-step unfolding rather than looking for such pairs.

Neither Lean step is in `just test`, which has no Lean. CI runs both, in a
job of their own that installs elan (`.github/workflows/ci.yml`).

## Keeping it true

A change to a function in the table under
[What is modelled](#user-content-what-is-modelled) needs the same change to
its Lean transcription, or `oracle_tests` fails on the cases that differ.
The change may then break a proof, which is the point: either the proof is
repaired, or the change broke a property the language promised, and the docs
or the change are wrong.

A new type constructor needs a case in `Ty` and `Hint`, in every function
over them, in the JSON encoding on both sides, and in the generator. Lean
says which functions lack the case; the Rust's exhaustive matches say the
same on its side.

A change to the literal typing rules needs the doc, `Lit.lean` and the code to
change together; `lit_tests` fails on whichever is left behind. Add a case to
its corpus for anything the change is about, since the generator may not find
it, as it did not find the atom and term case.

## What is not covered

In rough order of what to do next:

- **Enum variants are sorted and named once.** The model, like the Rust,
  relies on both and states neither.
- **`f32 ⇝ f64` is exact.** Needs a model of IEEE 754.
- **Inference and unification.** The doc claims that where unification's
  constraints all hold, the order of a collection's elements does not change
  what it synthesizes. Proving it needs unification made total, with a
  measure or fuel, and `Lit.lean`'s inference is partial.
- **What `ret e` returns.** The early-return theorem covers the returns an
  expression makes on its own. A `ret e` is checked against the return type,
  but that the value is one of the type needs a value semantics for datafun's
  expressions, which only datalit's literals have (`Lit.check_sound`).
- **A compound `set`'s own early return.** `set x +!= 1` returns early as
  `x +! 1` does, and the checker requires that it fit (F049), but the
  early-return theorem does not yet count it among a statement's.
- **The rest of datafun.** Generics, whose substitution lemma is what
  monomorphization relies on, modules, consts, patterns and `match` are
  outside the slice. The serde AST already
  carries them; the model and the generator's configuration are what widen.
- **Aliases and type parameters in hints.** The model takes datalit's
  hints, with no aliases or type parameters, so T063 for an alias and the
  conversion's T066 check are not modelled. `Ty.WF` states T066 for a type
  parameter already in a `Ty`, and the generator makes few of those.
- **The random generator rarely makes a type that starts with two `rec`s**,
  so the transitivity chains in `law_tests` would not have found the third
  hole above; the exhaustive test does.
