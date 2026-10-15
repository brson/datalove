# Datalit Typing Rules

The type system of datalit, the data-literal language, as a bidirectional
system of inference rules. This is a reference: it states what the checker in
`crates/datalove-datalit/src/tycheck` accepts, rule by rule, and the rule
names match the `Rule:` comments there. Revised October 2026 against that
code. The datafun rules in [Datafun Typing Rules](datafun-typing-rules.md)
extend these.

## Contents

- [Notation](#user-content-notation)
- [Syntax](#user-content-syntax)
- [Type equivalence](#user-content-type-equivalence)
- [Synthesis](#user-content-synthesis)
- [Inference](#user-content-inference)
- [Checking](#user-content-checking)
- [Rule selection](#user-content-rule-selection)
- [Consequences](#user-content-consequences)
- [Diagnostics](#user-content-diagnostics)
- [Implementation](#user-content-implementation)
- [References](#user-content-references)

## Notation

The system is bidirectional in the style of Pierce and Turner and of Dunfield
and Krishnaswami's survey (see [References](#user-content-references)). It has
two typing judgments:

```
⊢ e ⇒ T      e synthesizes T: T is an output, computed from e
⊢ e ⇐ T      e checks against T: T is an input, given by the context
```

A datalit is a closed term: it has no variables, so there is no context `Γ`,
and the turnstile has nothing on its left. Datafun adds one.

A rule has premises above the line and a conclusion below, with its name on
the right. A rule with no premises is an axiom. Side conditions that are not
typing judgments, such as a range test, are written as premises too.

```
premise₁    ...    premiseₙ
---------------------------- (Rule-Name)
conclusion
```

Rules are read bottom-up, as the checker runs them: to derive the conclusion,
derive each premise. Every rule is *mode-correct*: a type that is an input to
a premise is known from the conclusion's inputs or from an earlier premise's
outputs, so the rules describe an algorithm and not only a relation.

Metavariables:

| Symbol | Ranges over |
|--------|-------------|
| `T`, `U`, `K`, `V` | types |
| `I` | integer types: `u8` to `u64`, `i8` to `i64`, `index`, `offset`, `int` |
| `e` | expressions |
| `n` | integer literals, read as their value in ℤ (sign included) |
| `h` | hex literals, read as their value in ℕ |
| `r` | float literals |
| `s` | string literals |
| `ℓ` | field and column names |
| `a` | atom and term names |
| `E` | enum types |

`e₁, ..., eₙ` is a sequence of length `n ≥ 0`, and a premise containing `i`
stands for one premise for each `i ∈ 1..n`.

## Syntax

The abstract syntax of types:

```
T ::= bool | u8 | u16 | u32 | u64 | i8 | i16 | i32 | i64
    | index | offset | f32 | f64 | int | string | data | error
    | (T₁, ..., Tₙ)                    tuple; () is unit
    | {ℓ₁: T₁, ..., ℓₙ: Tₙ}            struct
    | [T]                              list
    | #{T}                             set
    | %{K = V}                         map
    | ?T                               option
    | !T                               result
    | [|T, k|]                         tensor of rank k ≥ 1
    | {| ℓ₁: T₁, ..., ℓₙ: Tₙ |}        table
    | atom a                           atom
    | term a T                         term
    | enum {v₁, ..., vₙ}               enum; enum {} is legal and has no values

v ::= atom a | term a T                enum variant
```

`!T` has no error type parameter: the failure of every result is an `error`.

The abstract syntax of expressions:

```
e ::= true | false | n | h | r | s
    | (e₁, ..., eₙ)                    tuple; one element needs a trailing comma
    | {ℓ₁ = e₁, ..., ℓₙ = eₙ}
    | [e₁, ..., eₙ]
    | #{e₁, ..., eₙ}
    | %{e₁ = e₁', ..., eₙ = eₙ'}
    | [| d₁ ... dₖ | e₁ ... eₘ |]      tensor, shape d₁ × ... × dₖ, row-major
    | {| ℓ₁, ..., ℓₙ; ē₁; ...; ēₘ |}   table, each row ēⱼ = eⱼ₁, ..., eⱼₙ
    | none | some e | ok e | er e
    | data e | error e
    | atom a | term a e | enum {e}
    | : T / e                          type hint (annotation)
```

The concrete syntax is in [Datalit Grammar](datalit-ebnf.md). Two forms
differ from it here. A tensor's shape is written in the abstract syntax as a
header, though the concrete syntax usually gives it by separators
(`[| 1 2, 3 4 |]` is `[| 2 2 | 1 2 3 4 |]`). Parentheses around a single
expression are not a node: `(e)` is `e`, and `: T / (: U / e)` is two nested
hints. (The AST keeps a `Group` node only to hold the inner of two such
hints apart; `Syn-Group` and `Check-Group` pass straight through it.)

## Type equivalence

`T ≡ U` is syntactic equality, with one exception: an enum's variants are a
set, so their order does not matter. The checker sorts variants by name when
it builds an enum type, which makes equivalence plain structural equality on
the sorted form.

```
enum {v₁, ..., vₙ} ≡ enum {v_π(1), ..., v_π(n)}     for any permutation π
```

Everything else is positional and exact:

- Tuples have equal length and equivalent elements in order.
- Structs and tables have the same field names in the same order, with
  equivalent types. `{x: u32, y: bool} ≢ {y: bool, x: u32}`.
- `atom a ≡ atom a'` iff `a = a'`; `term a T ≡ term a' T'` iff `a = a'` and
  `T ≡ T'`.
- `[|T, k|] ≡ [|T', k'|]` iff `T ≡ T'` and `k = k'`.
- Distinct numeric types are never equivalent. There is no subtyping and no
  implicit widening: `u8 ≢ u16`, `f32 ≢ f64`.
- A type alias is a name for a structural type, so it is not a type of its own
  (datalit has no aliases; datafun expands them before comparing).

## Synthesis

A hinted expression synthesizes its hint:

```
⊢ e ⇐ T
------------------ (Syn-TypedExpr)
⊢ (: T / e) ⇒ T
```

This is the annotation rule. It is how a term with no synthesizable type,
such as `none`, gets one, and how a literal gets a width other than its
default.

Any other expression synthesizes in three steps: infer a type for it with
variables where its parts must agree ([Inference](#user-content-inference)),
give each variable nothing decided its default, and check the expression
against the type that results:

```
⊢ e ⇝ τ    T = default(τ)    ⊢ e ⇐ T
------------------------------------- (Syn-Infer)
⊢ e ⇒ T
```

`default(τ)` is undefined when a variable with no default is left, from a
`none`, `er e`, `enum {e}` or empty collection that nothing decided, and the
expression does not synthesize (T016, at the expression that left it open).

The check is what reads each literal as the type it was inferred to have, and
what reports a disagreement: inference never fails, it only leaves undecided
what it could not decide. So synthesis and checking report the same faults
with the same diagnostics.

Some consequences, by example:

| Expression | Synthesizes |
|---|---|
| `5`, `0x10`, `1.5` | `int`, `int`, `f64` |
| `[1, 2]` | `[int]` |
| `[1, : u8 / 2]`, `[: u8 / 1, 2]` | `[u8]` |
| `[some 1, none]` | `[?int]` |
| `[[], [1]]` | `[[int]]` |
| `[(1, : u8 / 1), (: u8 / 2, 2)]` | `[(u8, u8)]` |
| `{| x; 1; : u8 / 2 |}` | `{| x: u8 |}` |
| `atom A`, `term A 1` | `atom A`, `term A int` |
| `data e`, `error e` | `data`, `error`, with `e` synthesized on its own |
| `[]`, `none`, `er e`, `enum {e}`, `{| x |}`, `[none]` | nothing (T016) |
| `{| |}` | `{| |}`: no columns, so nothing open |

An atom or term literal synthesizes its own singleton type, not an enum.
`data` and `error` are existential packages: the payload's type is forgotten
in the result's type, but the payload must still synthesize one of its own.
So `data none` and `data []` are errors and `data : ?u32 / none` is not.

## Inference

`⊢ e ⇝ τ` infers for `e` a type `τ` that may hold type variables `α`. Each
variable carries a class, which says what it stands for and what type it takes
if nothing decides:

| Class | From | Admits | Default |
|---|---|---|---|
| `open` | `none`, `er`, `enum {e}`, an empty collection | any type | none |
| `int` | an integer literal | integer types | `int` |
| `hex` | a hex literal | unsigned integers, `index`, `int`, `f32`, `f64` | `int` |
| `float` | a float literal | `f32`, `f64` | `f64` |
| `atom a` | `atom a` | `atom a`, any enum | `atom a` |
| `term a τ` | `term a e` | `term a T`, any enum | `term a default(τ)` |

"Admits" is by the type's head; whether the value is in range, or the variant
in the enum, is for the check that follows to say.

The rules, with `α:c` a fresh variable of class `c`:

```
--------------- (Inf-Bool)         ------------------ (Inf-String)
⊢ true ⇝ bool                      ⊢ s ⇝ string

α:int            α:hex            α:float
------- (Inf-Int)   ------- (Inf-Hex)   ------- (Inf-Float)
⊢ n ⇝ α          ⊢ h ⇝ α          ⊢ r ⇝ α

------------------ (Inf-TypedExpr)
⊢ (: T / e) ⇝ T

⊢ eᵢ ⇝ τᵢ                                  ⊢ eᵢ ⇝ τᵢ
------------------------------- (Inf-Tuple)   ----------------------------------------------- (Inf-Struct)
⊢ (e₁, ..., eₙ) ⇝ (τ₁, ..., τₙ)              ⊢ {ℓ₁ = e₁, ..., ℓₙ = eₙ} ⇝ {ℓ₁: τ₁, ..., ℓₙ: τₙ}

α    ⊢ eᵢ ⇝ τᵢ    α ≐ τᵢ                   α    ⊢ eᵢ ⇝ τᵢ    α ≐ τᵢ
------------------------- (Inf-List)       --------------------------- (Inf-Set)
⊢ [e₁, ..., eₙ] ⇝ [α]                      ⊢ #{e₁, ..., eₙ} ⇝ #{α}

α    β    ⊢ eᵢ ⇝ κᵢ    ⊢ eᵢ' ⇝ νᵢ    α ≐ κᵢ    β ≐ νᵢ
------------------------------------------------------ (Inf-Map)
⊢ %{e₁ = e₁', ..., eₙ = eₙ'} ⇝ %{α = β}

α    ⊢ eᵢ ⇝ τᵢ    α ≐ τᵢ
------------------------------------------ (Inf-Tensor)
⊢ [| d₁ ... dₖ | e₁ ... eₘ |] ⇝ [|α, k|]

αᵢ    ⊢ eⱼᵢ ⇝ τⱼᵢ    αᵢ ≐ τⱼᵢ
--------------------------------------------------------- (Inf-Table)
⊢ {| ℓ₁, ..., ℓₙ; ē₁; ...; ēₘ |} ⇝ {| ℓ₁: α₁, ..., ℓₙ: αₙ |}

⊢ e ⇝ τ                 ⊢ e ⇝ τ                α:open             α:open
--------------- (Inf-Some) ------------- (Inf-Ok) -------------- (Inf-None) --------------- (Inf-Er)
⊢ some e ⇝ ?τ           ⊢ ok e ⇝ !τ            ⊢ none ⇝ ?α        ⊢ er e ⇝ !α

------------------ (Inf-Data)      ---------------------- (Inf-Error)
⊢ data e ⇝ data                    ⊢ error e ⇝ error

α:atom a              ⊢ e ⇝ τ    α:term a τ          α:open
------------ (Inf-Atom)   ------------------ (Inf-Term)   --------------- (Inf-Enum)
⊢ atom a ⇝ α          ⊢ term a e ⇝ α                 ⊢ enum {e} ⇝ α
```

The shared variable of an empty collection, or of an empty table's columns,
is `open`; with elements it is decided by them. The variable is what the
elements are unified through, so `[]` names the list as needing a type and
`[none]` names the `none`. A hinted expression, and the payload of `data` and
`error`, is not looked into: checking against the hint, or synthesizing the
payload on its own, types what is inside.

**Unification.** `τ ≐ τ'` is first-order unification, structurally on the
type formers, with an enum's variants matched by name. A variable binds to a
type only if its class admits the type, and two variables merge only if some
type could be of both classes:

| Classes | Merged |
|---|---|
| `open` and `c` | `c` |
| `int` and `hex` | `int` |
| the same literal class | that class |
| `atom a` and `atom a` | `atom a` |
| `term a τ` and `term a τ'` | `term a τ`, with `τ ≐ τ'` |
| anything else | not merged |

Binding a `term a τ` variable to `term a T`, or to an enum whose variant `a`
has payload `T`, also unifies `τ ≐ T`.

A constraint that cannot hold is not an error here. The two sides are left
as they are and each goes on to its own type, the first in source order
keeping the variable it shares with what came before. Checking then finds the
later one does not fit: in `[1, 2, "three"]` the list is a `[int]`, and
`"three"` is reported against `int` (T022). Where the constraints do hold, the
solution does not depend on order, so `[1, : u8 / 2]` and `[: u8 / 2, 1]` are
both `[u8]`.

Every node is inferred once and every unification is union-find, so inference
is linear in the expression, up to the inverse-Ackermann factor and the size
of the types unified. The check after it is one more linear walk.
## Checking

### Annotation

```
T₀ ≡ T    ⊢ e ⇐ T
--------------------- (Check-Hinted)
⊢ (: T₀ / e) ⇐ T
```

A hint names the type of the expression it is on, and that must be the
expected type. It is not a conversion: `: [u16] / [: u8 / 42]` fails, because
the element is a `u8` where a `u16` is expected (T040).

### Subsumption

```
⊢ e ⇒ T'    T' ≡ T
--------------------- (Check-Subsume)
⊢ e ⇐ T
```

The mode switch: an expression that synthesizes can be checked against any
equivalent type. Because `≡` is equality and not subtyping, this rule never
converts anything. It is the fallback, used only when no rule below matches
the pair of expression form and expected type (see
[Rule selection](#user-content-rule-selection)). Every form but `true`,
`false` and a string has a rule below for each type head it can have, so for
those forms the fallback is reached only on a head mismatch, and fails.

### Numeric literals

```
n ∈ ⟦I⟧                       r a float literal    F ∈ {f32, f64}
-------------- (Check-Int)    ------------------------------------ (Check-Float)
⊢ n ⇐ I                       ⊢ r ⇐ F

h ∈ ⟦I⟧    I ∈ {u8, u16, u32, u64, index, int}
---------------------------------------------- (Check-Hex)
⊢ h ⇐ I

h < 2³²                       h < 2⁶⁴
-------------- (Check-Hex)    -------------- (Check-Hex)
⊢ h ⇐ f32                     ⊢ h ⇐ f64
```

`⟦I⟧` is the set of integers type `I` represents:

| `I` | `⟦I⟧` |
|-----|-------|
| `uN` | `[0, 2ᴺ - 1]` |
| `iN` | `[-2ᴺ⁻¹, 2ᴺ⁻¹ - 1]` |
| `index` | `[0, 2ʷ - 1]`, `w` = 32, or 64 with the `index-64` feature |
| `offset` | `[-2ʷ⁻¹, 2ʷ⁻¹ - 1]`, the same `w` |
| `int` | ℤ |

A hex literal is a bit pattern. It checks against the unsigned integer types,
`int`, and the floats, as the IEEE 754 encoding of the float. It has no sign
and does not check against a signed type: `: i32 / 0xFF` falls through to
`Check-Subsume`, where `int ≢ i32`.

An integer literal does not check against a float type and a float literal
does not check against an integer type. Both fall through to `Check-Subsume`
and fail.

### Tuples and structs

```
⊢ eᵢ ⇐ Tᵢ
------------------------------------- (Check-AnonTuple)
⊢ (e₁, ..., eₙ) ⇐ (T₁, ..., Tₙ)

⊢ eᵢ ⇐ Tᵢ
------------------------------------------------- (Check-AnonStruct)
⊢ {ℓ₁ = e₁, ..., ℓₙ = eₙ} ⇐ {ℓ₁: T₁, ..., ℓₙ: Tₙ}
```

The literal and the type have the same length, and in a struct the same
field names in the same order. A tuple of the wrong length is T038; a struct
with the wrong number of fields is T039, and with a field out of place T042.

### Collections

```
⊢ eᵢ ⇐ T                          ⊢ eᵢ ⇐ T
------------------------ (Check-List)   ------------------------ (Check-Set)
⊢ [e₁, ..., eₙ] ⇐ [T]              ⊢ #{e₁, ..., eₙ} ⇐ #{T}

⊢ eᵢ ⇐ K    ⊢ eᵢ' ⇐ V
-------------------------------------- (Check-Map)
⊢ %{e₁ = e₁', ..., eₙ = eₙ'} ⇐ %{K = V}
```

With `n = 0` these have no premises, so an empty collection checks against a
collection of any element type.

### Tensors and tables

```
d₁ × ... × dₖ = m    ⊢ eᵢ ⇐ T
----------------------------------------- (Check-Tensor)
⊢ [| d₁ ... dₖ | e₁ ... eₘ |] ⇐ [|T, k|]

|ēⱼ| = n    ⊢ eⱼᵢ ⇐ Tᵢ
--------------------------------------------------------------- (Check-Table)
⊢ {| ℓ₁, ..., ℓₙ; ē₁; ...; ēₘ |} ⇐ {| ℓ₁: T₁, ..., ℓₙ: Tₙ |}
```

The shape's rank must be the type's `k` (else T048) and the element count
must match the shape (T049). The table's columns must be the type's, by name
and in order (T054 for the count, T055 for a name), and every row has one
element per column (T056).

### Option and result constructors

```
------------- (Check-None)    ⊢ e ⇐ T
⊢ none ⇐ ?T                   ----------------- (Check-Some)
                              ⊢ some e ⇐ ?T

⊢ e ⇐ T                       ⊢ e ⇐ error
--------------- (Check-Ok)    ---------------- (Check-Er)
⊢ ok e ⇐ !T                   ⊢ er e ⇐ !T
```

The payload of `er` is the error, so a failure is written
`er error "failed"`. There is no rule `⊢ error e ⇐ !T`: an error does not
become a result by being where one is expected.

### Data and error

```
⊢ e ⇒ T                         ⊢ e ⇒ T
--------------------- (Check-Data)   ---------------------- (Check-Error)
⊢ data e ⇐ data                 ⊢ error e ⇐ error
```

These are derivable from `Syn-Data`, `Syn-Error` and `Check-Subsume`; the
checker handles them directly. There is no rule `⊢ e ⇐ data` for other
forms: a value is a `data` only when written as one.

### Atoms, terms and enums

```
------------------- (Check-Atom)    atom a ∈ E
⊢ atom a ⇐ atom a                   ---------------- (Check-AtomVariant)
                                    ⊢ atom a ⇐ E

⊢ e ⇐ T                             term a T ∈ E    ⊢ e ⇐ T
------------------------- (Check-Term)    -------------------------- (Check-TermVariant)
⊢ term a e ⇐ term a T               ⊢ term a e ⇐ E

⊢ e ⇐ E
------------------- (Check-Enum)
⊢ enum {e} ⇐ E
```

`v ∈ E` means `v` is one of `E`'s variants. These rules are what make an atom
or term literal a value of an enum: they apply to literals only, and there is
no subtyping `atom a <: E` behind them. In datafun, an atom held in a
variable keeps its singleton type and is widened to an enum with `@`.

A name mismatch, or a variant that is not in the enum, is T057; an enum
literal checked against a type that is not an enum is T058.

## Rule selection

The rules are syntax-directed: for a given expression form, and in checking
mode a given head constructor of the expected type, at most one rule applies.
The checker (`check` in `check.rs`) decides which in this order:

1. A hinted expression is checked by `Check-Hinted`, whose premise
   `⊢ e ⇐ T` continues with the steps below.
2. If a rule above matches the form of `e` and the head of `T`, it is the
   only rule tried. Failing a premise is an error; the checker does not fall
   back to subsumption. So `[1, 2] ⇐ [u8]` uses `Check-List` and checks each
   element against `u8`.
3. `none` or `er e` against anything but an option or result is T032.
4. Otherwise `Check-Subsume` applies. For `true`, `false` and string
   literals its failure is reported as T022. For anything else it always
   fails, reported as T032 with the expression's inferred type, `_` standing
   for what is undecided: `: int / [none]` finds `[?_]`.

A derivation is therefore unique when it exists, and the checker either
finds it or reports the first premise that fails.

## Consequences

Some properties follow from the rules and are worth stating outright.

- **No implicit widening.** No rule relates two different numeric types, so a
  `u8` is never accepted where a `u16` is. Datalit has no operators and so no
  way to convert; datafun converts with `@`.
- **Literals are polymorphic, values are not.** `42` checks against any
  integer type it fits and synthesizes `int`, and an atom literal checks
  against any enum that lists it and synthesizes its singleton type. Neither
  is subtyping: a literal is a value of every type that holds it.
- **Parts that must agree are typed together.** A literal goes along with
  whatever beside it has a type of its own, wherever that is, so
  `[: u8 / 1, 2]` is a `[u8]`. With nothing to go along with, a literal takes
  its default.
- **Empty collections need a type.** `[]` checks against every list type and
  synthesizes nothing; the same holds for sets, maps, tensors and tables with
  no rows. An empty table or tensor still has a shape the syntax fixes: a
  table's header names its columns and a tensor's shape gives its rank, so
  `: {| x: int |} / {||}` is T054 and `: [|u8, 2|] / [||]` is T048. Only the
  element type is free.
- **Enum literals parse more than check.** The grammar takes any expression
  inside `enum { }`; only an atom or term passes `Check-Enum`, so
  `enum { 1 }` is T032.
- **A tensor's shape is the parser's business.** The parser refuses a body
  that does not fill its shape (D041), so the element-count premise of
  `Check-Tensor` (T049) only guards ASTs built some other way. An empty
  tensor such as `[| 0 3 | |]` is the one with no elements.

## Diagnostics

| Code | Mode | Meaning |
|------|------|---------|
| T005-T012 | check | Integer or hex literal out of range for `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, in that order |
| T013 | check | Hex literal too wide for an `f32` bit pattern |
| T014 | check | Hex literal too wide for an `f64` bit pattern |
| T015 | check | Integer or hex literal out of range for `index` |
| T016 | synth | A type left open with nothing to decide it: by `none`, `er e`, `enum {e}` or an empty collection |
| T017 | synth | Expression contains a parse error |
| T022 | check | `true`, `false` or a string literal against another type |
| T032 | check | `Check-Subsume` failed, or `none` / `er` against a non-option / non-result |
| T038 | check | Tuple length mismatch |
| T039 | check | Struct field count mismatch |
| T040 | check | Hint differs from the expected type |
| T042 | check | Struct field out of order or misnamed |
| T048 | check | Tensor rank mismatch |
| T049 | check | Tensor element count does not match the shape |
| T054 | check | Table column count mismatch |
| T055 | check | Table column name mismatch |
| T056 | both | Table row with the wrong number of elements |
| T057 | check | Atom or term is not the expected one, or not a variant of the expected enum |
| T058 | check | Enum literal against a type that is not an enum |
| T059 | hint | A bare name in a hint; datalit has no type aliases |
| T061 | check | Integer literal out of range for `offset` |

## Implementation

The checker is `synthesize`, `check` and `infer` in
`crates/datalove-datalit/src/tycheck/`. `infer.rs` holds the inferred types
(`PType`), the classes, and the `Unifier`, a union-find over the variables of
one synthesis; datafun's inference
(`crates/datalove-datafun-tycheck/src/infer.rs`) uses the same unifier.
`type_check` in `api.rs` synthesizes a root expression;
`type_check_with_expected` checks it against a type the caller supplies,
which is how datafun checks a `require data` file against its declared type.

`Type` in `types.rs` has one variant per type former above, plus `Var`, a
type parameter of an enclosing datafun generic function. Datalit itself
never produces a `Var`; `Var(a) ≡ Var(b)` iff `a = b`, and a `Var` is
equivalent to nothing else.

The diagnostic codes T018 to T021, T051, T052 and T060 were for elements that
differed from the first under the first-element rule this replaces. They are
retired: a disagreement is now reported by the check, with that check's code.

## References

- Benjamin C. Pierce and David N. Turner, "Local Type Inference", TOPLAS
  2000. The origin of the synthesis/checking split.
- Jana Dunfield and Neel Krishnaswami, "Bidirectional Typing", ACM
  Computing Surveys 2021. Notation, mode-correctness, the annotation and
  subsumption rules, and the Pfenning recipe: introduction forms check,
  elimination forms synthesize.
- Benjamin C. Pierce, *Types and Programming Languages*, MIT Press 2002.
  Inference-rule notation.
