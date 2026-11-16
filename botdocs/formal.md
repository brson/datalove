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
  hint. It holds with aliases too (`Hint.convertWith_wf`), so long as each
  names a well-formed type: one is put where the alias is used, which keeps
  it well formed (`Ty.WF.lift`) because the conversion lets an alias through
  only with no `rec` in it in a key (T065) and no type parameter in it under
  a `rec` (T066). A script's aliases are converted that way in turn, so every
  signature `resolveNames` builds has well-formed types
  (`Fun.resolveNames_wf`).
- **`expose` ends** (`Ty.expose_not_mu`): on a well-formed type, unfolding as
  many times as the type has leading `rec`s leaves a head that is not a
  `rec`. The Rust loops until it gets there, and would not stop on
  `rec X X`; T064 is what rules that out.
- **A well-formed recursive type does not expose to a number**
  (`Ty.expose_not_numeric`), since its body uses its name.
- **A literal that checks is a value of its type** (`Lit.check_sound`); see
  [Literal typing](#user-content-literal-typing).
- **Inference fails only where its comments say** (`Lit.infer_errors`,
  `Fun.infer_errors`, in `Proofs/Infer.lean`): datalit's only on a
  malformed hint or a decision that depends on what a type parameter stands
  for (F110), datafun's only on F110, each save running out of fuel.
  Datalit's types hold no type parameter, so it never meets F110, but that
  is not proved. Unification panics where a variable it just pruned is not
  free. In Lean's logic a panic is not an error: it is the monad's default,
  which here succeeds and changes nothing, so a theorem about which errors
  can occur says nothing of it, and the proofs show instead that no panic is
  reached. A variable is never bound to another variable (`Lit.Inv`), which
  every step of the unifier, `resolve` and inference keeps, so pruning finds
  a free root or a type that is not a variable. Binding one variable to
  another in place of linking them breaks the proof.
- **In datafun, every early return fits the function, `completes` is
  sound, a `match` covers its enum, and checking a generic once is
  enough**; see [Statements](#user-content-statements).
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
  `type Json = rec J enum { atom Null, term Arr [J] }` and `a = atom Null`,
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

Inference, unification and synthesis run on fuel, since unification has no
structural measure, each call spending one and running out an error, so
they are total functions Lean can reason about. Checking is structural: it
takes synthesis, for a `data` or `error` payload, and Check-Subsume as
parameters (`checkWith`), and the oracle's `check` is it given the real
ones. Of checking one thing is proved, `Lit.check_sound`: a literal that
checks against a type denotes a value of that type. What a type's values are
(`HasType`, in `Proofs/LitSound.lean`) is written from the doc's tables, not
from the checker: an integer in the type's range, a hex literal that fits its
bits, a tensor with as many elements as its shape, a table whose header is
the type's and whose rows have one element per column, an atom or term that
is a variant of the enum, a recursive type's values its unfolding's. The
theorem holds for any synthesis, and for any Check-Subsume that never
succeeds, which the real one does not (`Lit.subsume_fails`): it always ends
in T032, or in what inference threw first. So `Lit.check_sound'` says it of
the real checker, with no premise. Dropping the tensor element-count check
from the checker breaks the proof, as it should.

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
functions, generic or not, type aliases and statements: `let` and `var`, binding a name or a
pattern, `set` with or without an operator and through fields and indexes,
`if` and the binding `if`, `loop` and `loop while`, `for`, `match`,
`break`, `continue`, `ret`, `debuglog` and `call`; datalit's literal forms,
which datafun types by datalit's rules with a context added; and datafun's
variables, places with field and index steps, fields and indexes of what is
not a place, operators, `?`, `!`, `@` and calls, each in a borrowed position
or not, and consts. Worlds of modules are in it too (`checkWorld`). A
module is a list of declarations under its requires and imports: its consts
are checked as a script's are, but a `?`, `!` or other early return in one
is F105, not F049, since there is no function to return from. A module
exports its functions' signatures and nothing else, so an import of a const
is F002 as an import of a missing function is; one through an alias nothing
is required under is F078, one of a name the module defines a function by
F106, and a second of one name F059 if it comes from another module. A
qualified call `a.f` is a call of the name `a.f`, which every function of a
required module is in scope under, and `a.` marks the alias as required, so
that a call through one that is not is F078 rather than F002. A const's
initializer names only consts (F058), a function body sees the consts above
it, a const argument names one (F076), and in a generic neither a const
parameter's type (F077) nor a const's (F088) mentions a type parameter. A
function's type
parameters are names for themselves, in scope in its signature and body as
aliases are; one is in a class only by its bound, a set or map over one
needs an order (F061), and a call infers what each stands for from its
arguments and then from what it is expected to be, as the Rust does, then
holds the answers to their bounds (F092) and reports one nothing decided
(F101). A script's
aliases and its functions' signatures are resolved first, in the order they
are written, each seeing the aliases above it, as the Rust's name
resolution does; hint conversion is datalit's with the aliases in scope
(`Hint.convertWith`), and so is its `rec` checking. Inference is
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
with them, reads a field or element of them, borrowed or not, or sets one,
or changes a generic's bound, uses its type parameters, or calls it with
what is at hand. One more puts, at the top of a generic's body, a statement
where inference must decide something of a type parameter's value: a
literal beside it, a part read off it, it adapted, met by a type with
parts or passed with a literal, or a `rec` hint holding it. Some of these
are F110 and some are clean, so the Rust's marking of what depends on a
type parameter is held to the model's failing; without it worldgen reached
F110 about once in two thousand scripts. The Rust's serde AST (`datalove-datafun-ast/src/ast_serde.rs`) is
the oracle's input, read as it is (`formal/Datalove/FunJson.lean`); a script
using a form outside the slice is skipped and counted. A corpus of cases
written by hand runs first; worldgen's parameters are mostly scalars, so it
is the corpus that reaches F069 to F073 most, and F101, since every generic
worldgen writes names its type parameter in a parameter. Thirty thousand
scripts from a random seed agree on every diagnostic, all but the 1216 that
do not parse inside the slice, reaching every statement code the slice has.

Diagnostics say nothing of a script that checks, and worldgen writes a hint
on nearly every binding, so each generated script also runs with the hints
dropped from its `let`s and `var`s. Where both sides check a script cleanly,
the types they give each `let`, `var` and `const` with a value and no hint
are compared too: the oracle lists the model's by `Fun.traceScript`, in
`FunTrace.lean`, which synthesizes each in the context the checker reaches
it in, and the test lists the Rust's from its expression types. A thousand
seeds compare about 4700 such types, and all agree.

`runs_agree` holds the runtime to the model's evaluation. Each generated
script, as written and changed, that checks gets a driver: for each function
that is not generic and whose parameters take literals, top-level
`debuglog`s of calls with literals of their types, at and near their ranges'
ends. The model runs it (`Fun.runScript`, the oracle printing each logged
value as `debuglog` does, `Fun.showVal` in `FunShow.lean`), and so does the
CLI. Every line must agree, up to what the model does not carry, a string's
text, a float, an error's message, a map's or set's entries, which match
anything; a script the model runs to its end, or to an early return, the CLI
must too, printing what it returned early last. Where the model stops at
what it does not run, what it logged must be what the CLI logged first. A
script a later phase refuses, the ownership analysis say, is skipped and
counted. A corpus written by hand runs first, for what generated functions
rarely do with their parameters: checked and optional arithmetic at its
bounds, division, indexes in and out of range, `for`, `loop`, recursion,
`match`, a binding `if`, `!` and `set` through an index, and `mut` and `out`
arguments written back, an early return's included, from calls inside
larger expressions, read later in them, and returned early from after,
under a hint or not. The model must run each corpus script to its end: one
it stops in would compare only a prefix, which a gap in the model passes.
`FORMAL_FUN_RUN_OF=<file>` with the `run_of` test prints what the model
logs of a file and how it ends, to find where it stops. For this test worldgen
writes `mut`, `out` and `ref` parameters, and the driver passes a `mut` or
`out` argument as a variable, set or not, binds the call's value, and logs
the variables after it; a thousand seeds write back through about 170
scripts' calls. A thousand
seeds run about 870 scripts, logging about 4200 lines, all agreeing, a
quarter of them with something the model does not carry.

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
- **Aliases, patterns and `match` agreed on the first run**, the corpus
  and the generated scripts alike, worldgen now writing enums under aliases
  and matching on them, and a mutation damaging a `match`'s cases. Writing
  them down settled recovery the doc had not said, now in
  [Going on after an error](datafun-typing-rules.md#user-content-going-on-after-an-error).
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
  element had no F072 check, and was read by cloning it, with no `@`
  written. Both are now refused outside a borrowed position
  ([the issue](issues.md#user-content-a-non-copy-part-of-a-temporary-can-only-be-borrowed)
  says why), and a borrowed one is read through a reference: `f()[0]?@`
  cloned twice, and `debuglog f().name` leaked the temporary. The backend
  fixture `45_borrowed_part_of_a_temporary` holds both.
- **Generics agreed on the first run**, worldgen writing generic functions
  of a dozen shapes and calling them, mutations changing, dropping or adding
  a bound, using a type parameter in a body and calling a generic with what
  is at hand. The model's own call rule was wrong where nothing had tested
  it: it reported a mode marker that disagreed with its parameter as it
  reached each argument, where the Rust reports every one before checking
  any, so `call g(1, ref true)` lost its second F057.
- **Instances of clean generics are clean.** `instances_check` appended an
  instance of every generic function in thirty thousand generated scripts
  the Rust checks cleanly, 51279 instances at type arguments their bounds
  admit, and the Rust and the model reported nothing on any of them.
- **An early return did not see through a recursive return type.** `?`,
  `!`, the checked and optional operators and a `set` target's index asked
  whether the function returns an option or a result without unfolding a
  `rec`, so a function returning `rec L ?[L]` could not use `?`, though
  `ret none` checked against it and the model, which unfolds as the doc's
  meeting at the head says, let it through. Reading the Rust for modules
  found it; the generator rarely writes a recursive return type. The backend
  fixture `46_early_return_into_a_recursive_type` holds it.
- **Consts agreed on the first run**, but for the model's own `set`, which
  refused only a `let`, where the Rust refuses anything that is not a `var`,
  a const among them. Writing it down found the Rust marking whatever
  binding a failed const's name had as a const: an outer `let` of the name
  became one, could then be passed to a const parameter, and was asked
  about type parameters for F088. A const that binds nothing now touches no
  other binding (error fixture `130_failed_const_leaves_the_name_alone`).
  F089, a const that depends on
  itself through a function, cannot arise in a script, where a function
  sees only the consts above it; it is a module's, and the model leaves it
  out.
- **Checking a negation against a float did not borrow its operand.**
  Synthesizing `-e` reads `e` in a borrowed position, but checking it
  against `f32` or `f64` passed the type down to `e` outside one, so
  `let y: f32 = -(mk().r!)` was F070 where `let y = -(mk().r!)` was accepted.
  A generic showed it: with `T is float`, `let y: T = -(mk(x).r!)` takes
  the synthesizing path, and its instance at `f32` the other, so the generic
  was accepted and its instance refused. Working out what the check-once
  theorem below would need found it. The operand is now borrowed either way;
  the backend fixture `45_borrowed_part_of_a_temporary` holds it.
- **Inference decided some things by what a type parameter stands for.**
  A literal beside a `T`, as in `[x, 1]` or `x == 1`, stayed a free
  variable under the generic, where under an instance at `u8` it became a
  `u8`; reading a `T`'s field or element, and a `rec` hint holding `T`, went
  likewise. Each such generic was refused anyway, with F016, F048, F090 or
  T066, for a literal is never of a type parameter's type: across twelve
  thousand scripts and twenty thousand instances none was accepted. But
  that made the check-once theorem rest on a claim about checking as a
  whole. Inference now reports such a decision itself (F110), in the Rust
  by marking what was inferred and in the model by failing, so the generic
  is refused where inference is asked, and on a generic that checks,
  inference steps with inference under every instance. The error fixture
  `131_literal_meets_a_type_parameter` holds it.
- **Worldgen could recurse without end.** An argument of a call may itself
  be a call, with no bound, so a function of enough parameters drew calls
  inside calls until the stack overflowed: past one expected call per call,
  which three parameters reach at a call probability of a half. Calls now
  nest at most six deep, checked after the draw, so that every seed short of
  the bound writes what it wrote before; three hundred default-configured
  worlds are byte for byte the same.

Four things are proved of it:

- **Every early return fits the function** (`Fun.checkFun_early_returns_fit`,
  in `Proofs/EarlyReturn.lean`). A function whose check reports nothing has
  every early return in its body, from `?`, `!`, `-?`, `-!`, an index's `?`
  or `!`, read or in a `set` target, or a checked or optional operator, a
  compound `set`'s included, at any depth of any expression of any
  statement, one its return type can carry: `none` only where it returns an option, an error only where it
  returns a result. Underneath it, `Fun.checkF_guarded` says the same of any
  expression that checks or synthesizes. Taking `fits` out of `?`'s rule
  breaks the proof, as taking F049 out of a place's index step or a compound
  `set` does.
- **`completes` is sound** (`Fun.completes_sound`, in
  `Proofs/Completes.lean`). A body it says cannot complete has no execution
  that finishes normally, against a semantics of control flow in which any
  statement holding an expression may return early, a condition may go either
  way, and a loop runs until a `break`, a return or, for `loop while`, its
  condition, as a `for` does, and a `match` runs one of its arms or its
  default. So a function F063 lets through always returns. Saying a
  `loop while` cannot complete breaks the proof.
- **A `match` that is let through covers its enum**
  (`Fun.checkMatch_exhaustive`, in `Proofs/Exhaustive.lean`). A `match`
  whose check reports nothing takes apart an enum its input synthesizes,
  names each variant at most once, each with a case of its kind of a variant
  the enum has, and with no default names every variant: a value of the
  enum reaches exactly one case, or the default. That is what lets the
  semantics above, and the Rust's `reachable.rs`, give a `match` no way of
  falling through. Taking out F100 or F097 breaks the proof. It finds a
  variant by name, as the Rust does, so "exactly one" rests on an enum's
  variant names being distinct. The parser refuses an enum that names a
  variant twice (D038), and the conversion sorts the variants by name, so
  under aliases that do the same, every enum it makes names its variants in
  strictly increasing order, and so each once (`Hint.convertWith_namesOnce`,
  in `Proofs/Variants.lean`). The order is what lets a type's equality be
  structural: an enum's variants written in another order sort to the same
  ones (`Variants.sortByName_perm`). Lean's core now proves the order on
  strings strict and total, which is all the proof needed of it.
- **Checking a generic once is enough** (`Fun.checkFun_instance`, in
  `Proofs/InstanceStmt.lean`, on `Proofs/Instance.lean` and
  `Proofs/InstanceCheck.lean`). A generic function whose check reports
  nothing has instances whose checks report nothing: its body checked with
  each type parameter an alias for a type argument the parameter's bound
  admits, no bounds in scope, and its parameter and return types
  substituted, F063 included. That is what compiling a generic once and
  running it at the types its calls pick relies on. Under it,
  `Fun.synthF_inst` and `Fun.checkF_inst` say the same of every expression,
  at the substituted type, and `Fun.checkStmtF_inst` of every statement.
  The type arguments have no type parameter and no `rec` in them
  (`Ground`); with one, `expose` would not commute with substituting it, and
  an instance spelling a recursive type into a key would be T065, which
  only refuses the spelling ([the issue](issues.md#user-content-t065-refuses-a-recursive-key-only-where-it-is-written)).
  The theorem takes inference as a parameter, as the theorems above do, and
  asks of it that under an instance it answer what it answered under the
  generic, substituted, wherever the generic checked quietly (`InstEnv`).
  That is proved of the real inference (`Fun.inferEnv_inst`, in
  `Proofs/Lockstep.lean`), so `Fun.checkFun_instance_real` states the
  theorem with nothing assumed of inference; `instances_check` holds the
  Rust to its conclusion, below. A signature names only its own type
  parameters and is well formed (`SigsOk`), which is proved of what
  `resolveNames` builds (`Fun.resolveNames_sigsOk`, in `Proofs/Sigs.lean`),
  so the theorem holds of a script without assuming it;
  and a body declares no type alias, which the parser refuses (P074). Making a type
  parameter copyable breaks the proof, as letting a bound satisfy another at
  a call does: each would let a generic through whose instance the Rust
  refuses.

- **A datafun expression that checks evaluates to a value of its type**
  (`Fun.evalS_sound`, in `Proofs/EvalSound.lean`, of the evaluator in
  `FunEval.lean`), for every form: literals, variables, places, hints,
  operators, `?`, `!`, `@`, fields, indexes and calls. Where an expression
  checks quietly in a context whose variables line up with its own, each
  that is set holding a value of its type (`Agree`), evaluating it gives a
  value of its type (`Lit.HasType`, datalit's), the variables, which a call
  may write to, lining up still; or returns early with a value of the
  function's return type: never an operand of the wrong kind, a checked
  operator's result outside its type's range, or an early return the
  function's type does not take, or an index or field read off the wrong
  kind of value. An index with nothing at its key returns early as its `?`
  or `!` says, and a tensor's of rank above one is a slice, of the size its
  shape says. A call goes to whatever runs calls, and the theorem holds for
  any that do what calls may (`CallOk`), below. What an operator does depends on its operands' type, as
  `a +! b` overflows at `u8`'s bounds or `i8`'s, so evaluation follows the
  checker one function for one and takes the types it gives, as a compiled
  program carries them. A string, a float, `data` and `error` carry nothing
  in the model, so a comparison, or a map lookup, is answered where the
  values' contents are modelled, numbers and booleans and what is built of
  them, and is otherwise not run. It holds for any inference, since what inference finds is checked
  before it is used, and of contexts with no type parameter's bound, as an
  instance's are, so of a generic through the check-once theorem. Proving it
  changed two things. A hex literal's value is also the integer it spells
  (`HasType.hexInt`), since `@` takes a `u8` to an `i16`, which no hex
  literal is of. And an enum takes a term by its first variant of that
  name, as checking a term against it does, where `@` asked of any
  (`Variants.hasTerm`); the two differ only for an enum naming a variant
  twice, which the parser refuses.
- **A datafun block that checks keeps its variables at their types**
  (`Fun.execBlock_sound`, in `Proofs/ExecSound.lean`, of the statements run
  in `FunExec.lean`), for every statement. Where a block
  checks quietly in a context with no type parameter's bound, and its
  variables line up with the context's, each that is set holding a value of
  its type (`Agree`), running it leaves its scope's variables so, with more
  bound. Otherwise it breaks or continues inside a loop; or it returns early with a value
  of the return type, or with none from a function that returns nothing.
  It may also stop where the model has not
  gone: a call written back to more than a variable, or with a type
  argument that has a `rec` in it; a read of a variable declared with no value, which
  the ownership analysis refuses (O005); a `set` through a tensor's slice; a
  `for` with an index over more elements than `index` counts; and a loop run
  more often than it was given, or calls nested deeper than they were given.
  It breaks only where the block has a `break` of its own (`hasBreak`), and
  ends normally only where control can reach its end (`completes`). As with expressions, running follows the checker
  one function for one, asking it for the context after each statement, and
  a `match` walks its cases as `checkCases` does, each at one less fuel. A
  scope is a prefix of the variables, so leaving a block drops what it bound
  and keeps what it set.
- **Calls into a checked program are sound** (`Fun.runCall_ok`, in
  `Proofs/CallSound.lean`, of `runCall` in `FunCall.lean`). A call runs the
  function's body as an instance, in the context the check-once theorem
  checks it in, with its parameters bound to the arguments and the consts in
  scope to their values; what `ret e` gives is the call's value, and a
  `ret`, or a body that ends, gives the unit. Where every function's body
  checks quietly at any ground type arguments its bounds admit, F063
  included, and its consts' values are of their types (`ProgOk`), calls do
  what the two theorems above ask of them: give a value of the function's
  return type, or stop at what is not run yet, at a variable read before it
  is set, or nested deeper than they were given. A function that returns a
  value cannot end without `ret`: its body is checked not to complete
  (F063), and a block that ends normally completes. A body runs outside any
  loop, so no `break` escapes it. What `ProgOk` asks of a body follows from
  its declaration checking quietly in a script (`Fun.body_checks`, by the
  check-once theorem), where the body declares no type alias and its
  signature's type parameters are its declaration's; that a script's consts
  hold values of their types is left to running a script, below.

  A call with a `mut` or `out` parameter runs wherever it is. Evaluation
  reads and writes the variables as it goes (`EvalT`, a state over them),
  so a call's arguments are copied in, an `out` one as no value, and what
  each parameter came to is written back to its argument's variable, as
  `paramsAfter` reads it from the body's variables when it returns, an early
  return's included; a variable read later in the same expression reads
  what it came to, as `f(mut a) + a` does in the Rust. A return, by `ret` or
  early from inside an expression, is `Stop.returned`, carrying the
  variables as they were, for that. Copying in and out is what writing in
  place does, the ownership analysis keeping an argument from being read
  another way during the call, and keeping anything later in an expression
  from changing what an earlier operand reads by reference (O017), so that
  reading every operand as it is evaluated, as the model does, agrees with
  the Rust, which reads a copy one then and others when they are used. A
  `set` runs its value, then its keys, and only then reaches its place, as
  the variable is once they have run (`setPlan`, `writeSteps`). The theorems for expressions are of
  evaluating from any variables that line up with the context (`Agree`),
  and say they still line up after (`SoundT`); what a parameter comes to is
  of its type, so writing it back keeps them so (`Fun.writeBack_sound`), a
  `mut` or `out` argument checking against its parameter's type, so that
  the variable is of it. An argument into a field or element of a variable,
  `mut xs[i]!` or `out p.f`, evaluates its keys with the arguments, as the
  Rust takes its pointer then (`evalPath`), and is written back through the
  same steps with those keys' values, as a `set` writes (`writeSteps`); the
  steps and keys are of the types `setPlan` gives them (`PathOk`), so the
  variable keeps its type, an index that is gone by then returning early as
  its mode says. A hint on such an argument says only its type: it is
  evaluated and written back as the place under it (`unhint`), which checks
  without the hint at the same fuel (`unhint_check`).
- **Passing by pointer is copying in and out where nothing written is
  shared** (`PassBy.ptr_eq_copy`, in `Proofs/PassBy.lean`). The model's
  copy-in, copy-out is the meaning of a call; the Rust passes every
  parameter by pointer. Of a callee abstracted to steps that each set a
  parameter to a function of some of them, writing only its writable
  (`mut`, `out`) ones, run by pointer into the caller's store or on copies
  written back at the end: where no other parameter's location is a writable
  one's (`Unshared`), the store ends the same and the callee reads the same
  at every step (`PassBy.params_agree`). `Unshared` is what O010 and
  lowering's early copy of a copy `in` argument give between them, the copy
  being a location of its own; without it they differ
  (`PassBy.shared_differs`, `f(mut a, a)` by pointer twice). A location may
  be a part of a variable, `mut xs[i]!`, as well as a variable: locations
  here are one or apart, and O010, comparing arguments by their variable
  and the fields before any index, leaves no two that overlap where either
  is written: distinct fields are apart, and a place holding another, or
  two elements of one collection, are refused. Values here are never
  freed, so a `ref` argument moved under it, also O010, is outside this.

- **A script that checks runs soundly** (`Fun.runScript_sound`, in
  `Proofs/RunSound.lean`, of `runScript` in `FunRun.lean`). Run statement by
  statement as `checkScript` checks them, each top-level `debuglog` logs a
  value of the type the checker gives it, and the script ends, or returns
  early with a value of its type `!()`, or stops at what is not run yet, at a
  variable read before it is set, or where a loop or calls ran as long as
  they were given. It puts the theorems above together: each function goes
  into the table calls run as its declaration is reached, with the values of
  the consts it sees, and is as `ProgOk` asks, by the check-once theorem and
  since F088 keeps a top-level const's type free of type parameters. A call
  of a function declared below it is not run yet. A function goes into the
  table only where its signature's type parameters are its declaration's, its
  parameters as many as its signature's, and its body declares no type alias;
  `resolveNames` and the parser make that so, which is not yet proved, so the
  runner checks it.
- **Inference under a generic steps with inference under its instances**
  (`Fun.inferEnv_inst`, in `Proofs/Lockstep.lean`). Substituting type
  arguments touches only known types, so a variable keeps its number, and
  a state is substituted variable by variable. Run from a substituted state,
  inference under an instance ends in the substituted state with the
  substituted answer, or fails as the generic's did, unless the generic's
  failed with F110 or ran out of fuel (`Lit.Lock`). Each decision either
  comes out the same, since a ground type argument keeps the head of a type
  whose head is no type parameter, or is one that depends on what a type
  parameter stands for, which is F110 under the generic: a literal's class
  against `T`, a value adapted to or from a type holding one that does not
  adapt, `T` met by a type with parts or read for its parts, and a hint with
  `T` inside a `rec` (T066). A generic that checks quietly met neither F110
  nor running out, which is how the theorem's assumption is discharged. A
  hint reads the same under the substituted aliases, failures included
  (`Fun.convertIn_inst`), and a call's signature instantiates the same,
  being well formed, so that no `rec` in it holds a type parameter
  (`Fun.instantiate_inst`). Nothing is assumed: `#print axioms` shows only
  Lean's three.

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
  seeds. `fun_tests` includes `instances_check`, which takes each generated
  script the Rust checks cleanly, before and after the mutations that touch
  generics, appends an instance of each of its generic functions, its type
  parameters declared as aliases of type arguments their bounds admit, and
  requires the Rust and the model both to report nothing: the check-once
  theorem's conclusion, held to the Rust and its inference. It also
  includes `world_typing_agrees`, which takes worldgen's worlds of up to
  three modules, each requiring and importing from the ones before it and
  calling them qualified, changes a module at a time as the scripts are,
  checks them through the Rust's module graph, and holds each module's codes
  to `checkWorld`'s. A diagnostic is the module's whose source its first
  label points into. Over five thousand seeds 3480 worlds agree and none
  differ; the rest are skipped, for a mutation that leaves a statement at
  the top level, where the model takes only declarations, or for one that
  leaves a require or import below a declaration.
- Every script `fun_tests` makes parses, mutated or not, and `rust_side`
  fails the test on one that does not, before type checking it: the type
  checker is not made to run on what a parse error leaves, which the CLI
  never does, and a syntax change the generators have not followed fails
  loudly rather than thinning the tests. The text mutations keep to what
  parses: an operator swap passes over a hint's `/`, a `set` is through a
  variable, a step from a literal parenthesizes it, a bound names one of the
  generic's own type parameters, and a line added after a `match` goes
  before it instead.
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

- **`f32 ⇝ f64` is exact.** Needs a model of IEEE 754.
- **Order independence.** The doc claims that where unification's
  constraints all hold and no `rec` is involved, the order of a
  collection's elements does not change what it synthesizes. With `rec` it
  does not hold ([the issue](issues.md#user-content-a-list-with-a-recursive-field-checks-in-one-order-and-not-the-other)).
  Without, it is tested, not proved: `lit_tests`'s `order_does_not_matter`
  puts generated pairs in a list both ways, holds the Rust to the model on
  each, and requires that where either order checks both do, to one type;
  thirty thousand seeds give nearly twenty thousand pairs, eight thousand
  of which check, and no failure. Removing its `rec` filter finds the issue
  again. A proof needs that unification computes the unique most general
  solution of its constraints, since two orders do not step together as a
  generic and its instance do; it is better left until the canonical form
  the issue proposes settles what a recursive type's spelling is.
- **Calls the model does not run.** A call with a recursive type argument
  stops, until the check-once theorem takes one, and so does a call of a
  function a script declares below it.
- **Recursive type arguments.** The check-once theorem takes type arguments
  with no `rec` in them (`Ground`), until T065 says whether a recursive key
  is allowed.
- **F089, and requires that fail.** The Rust finds a const that depends on
  itself through a function by what its checker looked up, and the model
  does not track that; `world_typing_agrees` skips any world the Rust
  reports it in, though worldgen has not yet written one. The model takes
  only worlds whose modules require modules before them, once each under
  one alias, with every require and import above the declarations: a
  missing module, a cycle, an alias taken twice, a rider and a name used
  above its import are the Rust's alone.
- **The random generator rarely makes a type that starts with two `rec`s**,
  so the transitivity chains in `law_tests` would not have found the third
  hole above; the exhaustive test does.
