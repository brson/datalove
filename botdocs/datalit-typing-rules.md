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

### Literals

```
---------------- (Syn-Bool)        ---------------- (Syn-Bool)
⊢ true ⇒ bool                      ⊢ false ⇒ bool

---------------- (Syn-Int)         ---------------- (Syn-Hex)
⊢ n ⇒ int                          ⊢ h ⇒ int

---------------- (Syn-Float)       ---------------- (Syn-String)
⊢ r ⇒ f64                          ⊢ s ⇒ string
```

An unannotated number gets the type that keeps what was written: `int` for
integer and hex literals, `f64` for floats. Any other width is had by
checking (a hint, or in datafun an expected type).

### Annotation

```
⊢ e ⇐ T
------------------ (Syn-TypedExpr)
⊢ (: T / e) ⇒ T
```

This is the annotation rule. It is how a term with no synthesizable type,
such as `none`, gets one, and how a literal gets a width other than its
default.

### Tuples and structs

```
⊢ eᵢ ⇒ Tᵢ
------------------------------------- (Syn-AnonTuple)
⊢ (e₁, ..., eₙ) ⇒ (T₁, ..., Tₙ)

⊢ eᵢ ⇒ Tᵢ
------------------------------------------------- (Syn-AnonStruct)
⊢ {ℓ₁ = e₁, ..., ℓₙ = eₙ} ⇒ {ℓ₁: T₁, ..., ℓₙ: Tₙ}
```

### Collections

A non-empty collection takes its element type from its first element; every
later element must synthesize an equivalent type. An empty one synthesizes
unit as its element type.

```
n ≥ 1    ⊢ e₁ ⇒ T    ⊢ eᵢ ⇒ Tᵢ    Tᵢ ≡ T
----------------------------------------- (Syn-List)        ------------- (Syn-List)
⊢ [e₁, ..., eₙ] ⇒ [T]                                       ⊢ [] ⇒ [()]

n ≥ 1    ⊢ e₁ ⇒ T    ⊢ eᵢ ⇒ Tᵢ    Tᵢ ≡ T
----------------------------------------- (Syn-Set)         --------------- (Syn-Set)
⊢ #{e₁, ..., eₙ} ⇒ #{T}                                     ⊢ #{} ⇒ #{()}

n ≥ 1    ⊢ e₁ ⇒ K    ⊢ e₁' ⇒ V    ⊢ eᵢ ⇒ Kᵢ    ⊢ eᵢ' ⇒ Vᵢ    Kᵢ ≡ K    Vᵢ ≡ V
------------------------------------------------------------------------------ (Syn-Map)
⊢ %{e₁ = e₁', ..., eₙ = eₙ'} ⇒ %{K = V}

-------------------------- (Syn-Map)
⊢ %{} ⇒ %{() = ()}
```

### Tensors and tables

```
m = d₁ × ... × dₖ    m ≥ 1    ⊢ e₁ ⇒ T    ⊢ eᵢ ⇒ Tᵢ    Tᵢ ≡ T
------------------------------------------------------------- (Syn-Tensor)
⊢ [| d₁ ... dₖ | e₁ ... eₘ |] ⇒ [|T, k|]

------------------------------------------- (Syn-Tensor)
⊢ [| d₁ ... dₖ | |] ⇒ [|(), k|]
```

The rank `k` is the number of extents in the shape. A tensor with elements
has exactly `d₁ × ... × dₖ` of them; one with none, whose shape has a zero
extent, synthesizes unit as its element type.

```
m ≥ 1    |ēⱼ| = n    ⊢ e₁ᵢ ⇒ Tᵢ    ⊢ eⱼᵢ ⇒ Tⱼᵢ    Tⱼᵢ ≡ Tᵢ
--------------------------------------------------------------- (Syn-Table)
⊢ {| ℓ₁, ..., ℓₙ; ē₁; ...; ēₘ |} ⇒ {| ℓ₁: T₁, ..., ℓₙ: Tₙ |}

------------------------------------------------------- (Syn-Table)
⊢ {| ℓ₁, ..., ℓₙ |} ⇒ {| ℓ₁: (), ..., ℓₙ: () |}
```

Every row has one element per column. Each column takes its type from the
first row, as a list's element type comes from its first element.

### Constructors

```
⊢ e ⇒ T                       ⊢ e ⇒ T
------------------ (Syn-Some)   ---------------- (Syn-Ok)
⊢ some e ⇒ ?T                   ⊢ ok e ⇒ !T

⊢ e ⇒ T                       ⊢ e ⇒ T
------------------ (Syn-Data)   ---------------------- (Syn-Error)
⊢ data e ⇒ data                 ⊢ error e ⇒ error
```

`data` and `error` are existential packages: the payload's type is forgotten
in the result's type, but the payload must still have one of its own. So
`data none` is an error and `data : ?u32 / none` is not.

```
------------------------ (Syn-Atom)       ⊢ e ⇒ T
⊢ atom a ⇒ atom a                         -------------------------- (Syn-Term)
                                          ⊢ term a e ⇒ term a T
```

An atom or term literal synthesizes its own singleton type, not an enum.

`none`, `er e` and `enum {e}` have no synthesis rule. Each is a value of many
types (any option, any result, any enum listing the variant), so it needs an
expected type, from a hint or from its position. Synthesizing one is T016.

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
[Rule selection](#user-content-rule-selection)).

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
   literals its failure is reported as T022, and for anything else as T032.

A derivation is therefore unique when it exists, and the checker either
finds it or reports the first premise that fails.

## Consequences

Some properties follow from the rules and are worth stating outright.

- **No implicit widening.** No rule relates two different numeric types, so a
  `u8` is never accepted where a `u16` is. Datalit has no operators and so no
  way to convert; datafun converts with `@`.
- **Literals are polymorphic only under checking.** `42` checks against any
  integer type it fits but synthesizes `int`, so `[: u8 / 1, 2]` fails (T018:
  the second element is an `int`) while `: [u8] / [1, 2]` succeeds.
- **Empty collections.** `[]` checks against every list type and synthesizes
  `[()]`; the same holds for sets and maps. An empty table or tensor still
  has a shape the syntax fixes: a table's header names its columns and a
  tensor's shape gives its rank, so `: {| x: int |} / {||}` is T054 and
  `: [|u8, 2|] / [||]` is T048. Only the element type is free.
- **Enum literals parse more than check.** The grammar takes any expression
  inside `enum { }`; only an atom or term passes `Check-Enum`, so
  `enum { 1 }` is T032.
- **A tensor's shape is the parser's business.** The parser refuses a body
  that does not fill its shape (D041), so the element-count premises of
  `Syn-Tensor` and `Check-Tensor` (T051, T049) only guard ASTs built some
  other way. An empty tensor such as `[| 0 3 | |]` is the one with no
  elements.

## Diagnostics

| Code | Mode | Meaning |
|------|------|---------|
| T005-T012 | check | Integer or hex literal out of range for `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, in that order |
| T013 | check | Hex literal too wide for an `f32` bit pattern |
| T014 | check | Hex literal too wide for an `f64` bit pattern |
| T015 | check | Integer or hex literal out of range for `index` |
| T016 | synth | No synthesis rule: `none`, `er e` or `enum {e}` with nothing expected |
| T017 | synth | Expression contains a parse error |
| T018 | synth | List element differs from the first |
| T019 | synth | Set element differs from the first |
| T020 | synth | Map key differs from the first |
| T021 | synth | Map value differs from the first |
| T022 | check | `true`, `false` or a string literal against another type |
| T032 | check | `Check-Subsume` failed, or `none` / `er` against a non-option / non-result |
| T038 | check | Tuple length mismatch |
| T039 | check | Struct field count mismatch |
| T040 | check | Hint differs from the expected type |
| T042 | check | Struct field out of order or misnamed |
| T048 | check | Tensor rank mismatch |
| T049 | check | Tensor element count does not match the shape |
| T051 | synth | Tensor element count does not match the shape |
| T052 | synth | Tensor element differs from the first |
| T054 | check | Table column count mismatch |
| T055 | check | Table column name mismatch |
| T056 | both | Table row with the wrong number of elements |
| T057 | check | Atom or term is not the expected one, or not a variant of the expected enum |
| T058 | check | Enum literal against a type that is not an enum |
| T059 | hint | A bare name in a hint; datalit has no type aliases |
| T060 | synth | Table element differs from its column's first row |
| T061 | check | Integer literal out of range for `offset` |

## Implementation

The checker is `synthesize` and `check` in
`crates/datalove-datalit/src/tycheck/`, one function per judgment, with
`synthesize_unhinted` for synthesis that ignores a hint the caller has
already compared (`Check-Subsume` after `Check-Hinted`). `type_check` in
`api.rs` synthesizes a root expression; `type_check_with_expected` checks it
against a type the caller supplies, which is how datafun checks a
`require data` file against its declared type.

`Type` in `types.rs` has one variant per type former above, plus `Var`, a
type parameter of an enclosing datafun generic function. Datalit itself
never produces a `Var`; `Var(a) ≡ Var(b)` iff `a = b`, and a `Var` is
equivalent to nothing else.

## References

- Benjamin C. Pierce and David N. Turner, "Local Type Inference", TOPLAS
  2000. The origin of the synthesis/checking split.
- Jana Dunfield and Neel Krishnaswami, "Bidirectional Typing", ACM
  Computing Surveys 2021. Notation, mode-correctness, the annotation and
  subsumption rules, and the Pfenning recipe: introduction forms check,
  elimination forms synthesize.
- Benjamin C. Pierce, *Types and Programming Languages*, MIT Press 2002.
  Inference-rule notation.
