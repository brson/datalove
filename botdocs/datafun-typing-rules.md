# Datafun Typing Rules

The type system of datafun, the expression and statement language, as
inference rules. This is a reference: it states what the checker in
`crates/datalove-datafun-tycheck` accepts. Revised October 2026 against that
code. It extends [Datalit Typing Rules](datalit-typing-rules.md), whose
notation, type equivalence and literal rules it assumes; read that first.
The concrete syntax the rules are written over is in
[Datafun Grammar](datafun-ebnf.md), which also maps each production to the
rules that type it.

## Contents

- [Notation](#user-content-notation)
- [Types](#user-content-types)
- [Expressions](#user-content-expressions)
  - [Inherited rules](#user-content-inherited-rules)
  - [Variables](#user-content-variables)
  - [Arithmetic, comparison and logic](#user-content-arithmetic-comparison-and-logic)
  - [Try operators](#user-content-try-operators)
  - [Adapt](#user-content-adapt)
  - [Calls](#user-content-calls)
  - [Fields and indexes](#user-content-fields-and-indexes)
  - [Rule selection](#user-content-rule-selection)
- [Statements](#user-content-statements)
  - [Patterns](#user-content-patterns)
  - [Bindings](#user-content-bindings)
  - [Assignment](#user-content-assignment)
  - [Control flow](#user-content-control-flow)
  - [Functions](#user-content-functions)
  - [Modules and scripts](#user-content-modules-and-scripts)
- [Diagnostics](#user-content-diagnostics)
- [Implementation](#user-content-implementation)
- [References](#user-content-references)

## Notation

Expressions are typed by the two bidirectional judgments of datalit, now
under a context `Γ`; statements by a third judgment that threads the context
through:

```
Γ ⊢ e ⇒ T        e synthesizes T
Γ ⊢ e ⇐ T        e checks against T
Γ ⊢ s ⊣ Γ'       s is well typed under Γ, and the statements after it see Γ'
```

A context is a list of entries; a later entry for a name shadows an earlier
one:

```
Γ ::= ·
    | Γ, x :^q T          variable x of type T, with binding kind q
    | Γ, α :: b           type parameter α with bound b
    | Γ, ret R            the enclosing function returns R
    | Γ, ret void         the enclosing function declares no return type
    | Γ, loop             inside a loop body

q ::= let | var | const
b ::= any | ord | float | fixedint
```

`Γ(x) = T^q` looks a variable up, `Γ(ret)` is the return entry if there is
one, and `loop ∈ Γ` tests the loop entry. `Γ|const` is `Γ` with every
variable that is not `const` removed. A binding of kind `var` is mutable; the
others are not.

Function signatures live in a global table `Σ`, collected before any body is
checked, so functions may call each other in any order:

```
Σ(f) = ∀α₁::b₁, ..., αₖ::bₖ. (m₁ T₁, ..., mₙ Tₙ) → R
m ::= in | ref | mut | out | const
```

`Σ` is written only where a rule reads it.

Statement sequences and blocks:

```
Γ₀ ⊢ s₁ ⊣ Γ₁    ...    Γₙ₋₁ ⊢ sₙ ⊣ Γₙ
--------------------------------------- (Seq)
Γ₀ ⊢ s₁ ... sₙ ⊣ Γₙ

Γ ⊢ s̄ ⊣ Γ'
------------- (Block)
Γ ⊢ { s̄ }
```

`Γ ⊢ { s̄ }` is a block: the bodies of `if`, `loop` and `match`. What it binds
does not escape it. (Datafun writes no braces; they mark a block here.)

## Types

Datafun's types are datalit's, plus two:

```
T ::= ... datalit types ...
    | α                                type parameter
```

and function types, which appear only in `Σ`. Functions are not values: no
expression has a function type, and a name that is not a variable is not an
expression.

A type alias (`type Point: {x: f32, y: f32}`) is a name for a structural
type and is expanded before types are compared. An unknown name is F064; an
alias defined twice is F066.

Equivalence `T ≡ U` is datalit's, extended by `α ≡ α` and nothing else: a
type parameter is equivalent only to itself.

Several rules test a type against a class:

| Class | Members |
|-------|---------|
| `Fix` | `u8` `u16` `u32` `u64` `i8` `i16` `i32` `i64` `index` `offset` |
| `Float` | `f32` `f64` |
| `Num` | `Fix ∪ Float ∪ {int}` |
| `Eq` | `Num`, `bool`, `string`, `atom a`; and `()`, tuples, structs, `?T`, `term a T` and enums whose components are all in `Eq` |

`Eq` excludes lists, sets, maps, tables, tensors, results, `data`, `error`
and type parameters, at any depth.

A type parameter is in a class only by its bound: `α ∈ Float` when
`α :: float ∈ Γ`, and `α ∈ Fix` when `α :: fixedint ∈ Γ`. Where a rule
below says `Fix` or `Float` without qualification it means the concrete
types only; `Fix⁺` and `Float⁺` include bounded parameters.

## Expressions

### Inherited rules

Every datalit rule holds in datafun, with `Γ` added on the left of each
judgment: annotation (`Syn-TypedExpr`, `Check-Hinted`), subsumption, the
literal rules, tuples, structs, collections, tensors, tables, the option,
result, `data` and `error` constructors, atoms, terms and enums. Their
diagnostics are F-codes here: a mismatch is F016, a missing synthesis rule
F011, a literal out of range F065, and a literal of the wrong shape (tuple
length, struct field count or order, table columns, tensor rank or element
count) F075.

Datafun adds one checking rule for a negated literal, since datafun has a
negation operator and datalit has only signed literals:

```
-n ∈ ⟦I⟧    I ∈ Fix
-------------------- (Check-NegLit)
Γ ⊢ -n ⇐ I
```

And for negation of anything under a float:

```
Γ ⊢ e ⇐ F    F ∈ Float
---------------------- (Check-Neg)
Γ ⊢ -e ⇐ F
```

These make `let x: i32 = -5` and `let y: f32 = -1.5` check.

### Variables

```
Γ(x) = T^q
------------- (Syn-Var)
Γ ⊢ x ⇒ T
```

An unbound name is F001. Inside a `const` initializer the context is
`Γ|const`, so naming a `let`, `var` or parameter there is F058.

### Arithmetic, comparison and logic

Binary operators synthesize through an auxiliary judgment that pairs the two
operands at one type:

```
Γ ⊢ e₁ ~ e₂ ⇒ T        e₁ and e₂ both have type T
```

The rules of `~` decide which operand, if either, gives its type to the
other. They use three syntactic predicates:

- `adapt(e)`: `e` is `e'@`, or `-e'`, `-?e'` or `-!e'` with `adapt(e')`.
- `cons(e)`: `e` is an unhinted `none`, `some e'`, tuple, struct, atom,
  term or enum literal: a construction whose synthesized type, if it has
  one, is a guess.
- `lit(e)`: `e` is an unhinted integer, hex or float literal, or a negation
  of one. A hinted literal such as `: u8 / 1` has a type of its own.

The rules are tried in this order, and each is shown for an operand on the
left; each has a mirror image with the operands swapped.

```
adapt(e₁)    ¬adapt(e₂)    Γ ⊢ e₂ ⇒ T    Γ ⊢ e₁ ⇐ T
---------------------------------------------------- (Pair-Adapt)
Γ ⊢ e₁ ~ e₂ ⇒ T

cons(e₁)    ¬cons(e₂)    ¬lit(e₂)    Γ ⊢ e₂ ⇒ T    Γ ⊢ e₁ ⇐ T
-------------------------------------------------------------- (Pair-Cons)
Γ ⊢ e₁ ~ e₂ ⇒ T

lit(e₁)    ¬lit(e₂)    Γ ⊢ e₂ ⇒ T    T ∈ Num    Γ ⊢ e₁ ⇐ T
----------------------------------------------------------- (Pair-Lit)
Γ ⊢ e₁ ~ e₂ ⇒ T

Γ ⊢ e₁ ⇒ T    Γ ⊢ e₂ ⇒ T'    T ≡ T'
----------------------------------- (Pair-Syn)
Γ ⊢ e₁ ~ e₂ ⇒ T
```

So with `x : u32`, `x == 0` pairs at `u32` by `Pair-Lit`, `x == none` with
`x : ?u32` by `Pair-Cons`, and `a@ .< b` takes `b`'s type by `Pair-Adapt`.
`1 == 2` has a literal on each side, falls to `Pair-Syn`, and compares two
`int`s. Operands that do not pair are F016, "operands must have the same
type", reported before the operator is checked against the type.

Each arithmetic operator `⊕` admits a set of operand types, `Arith(⊕)`, and
may require the enclosing function to return an option or a result:

| `⊕` | `Arith(⊕)` | Requires `Γ(ret)` |
|-----|------------|-------------------|
| `+` `-` `*` | `Float⁺ ∪ {int}` | -- |
| `/` | `Float⁺` | -- |
| `+!` `-!` `*!` | `Fix⁺` | `!U` |
| `/!` | `Fix⁺ ∪ {int}` | `!U` |
| `+?` `-?` `*?` | `Fix⁺` | `?U` |
| `/?` | `Fix⁺ ∪ {int}` | `?U` |

`fits(⊕, Γ)` holds when the third column is `--`, or `Γ(ret)` has the shape
it gives. At script top level `Γ(ret) = !()`, so the `!` forms fit there and
the `?` forms do not.

```
Γ ⊢ e₁ ~ e₂ ⇒ T    T ∈ Arith(⊕)    fits(⊕, Γ)
--------------------------------------------- (Syn-Arith)
Γ ⊢ e₁ ⊕ e₂ ⇒ T

Γ ⊢ e₁ ~ e₂ ⇒ T    T ∈ Num ∪ Float⁺ ∪ Fix⁺
------------------------------------------ (Syn-Ord)        ⊙ ∈ {.<, .>, <=, >=}
Γ ⊢ e₁ ⊙ e₂ ⇒ bool

Γ ⊢ e₁ ~ e₂ ⇒ T    T ∈ Eq ∪ Float⁺ ∪ Fix⁺
----------------------------------------- (Syn-Eq)          ⊙ ∈ {==, !=}
Γ ⊢ e₁ ⊙ e₂ ⇒ bool

Γ ⊢ e₁ ~ e₂ ⇒ bool
---------------------------- (Syn-Logic)                    ⊙ ∈ {and, or, xor}
Γ ⊢ e₁ ⊙ e₂ ⇒ bool
```

A checked or optional operator yields its operand type, not an option or
result: on failure the function returns early, with an error or `none`. An
operator applied to a type it does not admit is F026; one whose early return
does not fit is F049. Comparisons do not chain: `a == b == c` is a parse
error (P067).

Equality compares floats by IEEE 754 wherever they sit, so
`some nan != some nan`. A bounded parameter has the comparisons; an unbounded
or `ord`-bounded one has none, and orders through `sys/std/ord` instead.

In checking mode, an operator whose result type is its operand type passes
the expected type down to both operands:

```
T ∈ Prop(⊕)    fits(⊕, Γ)    Γ ⊢ e₁ ⇐ T    Γ ⊢ e₂ ⇐ T
----------------------------------------------------- (Check-BinOp)
Γ ⊢ e₁ ⊕ e₂ ⇐ T
```

| `⊕` | `Prop(⊕)` |
|-----|-----------|
| `+` `-` `*` | `Float ∪ {int}` |
| `/` | `Float` |
| `+!` `-!` `*!` `+?` `-?` `*?` | `Fix` |
| `/!` `/?` | `Fix ∪ {int}` |
| `and` `or` `xor` | `{bool}` |

`Prop(⊕) ⊆ Arith(⊕)` for each operator, so `Check-BinOp` never accepts an
operator at a type synthesis would refuse. It is how literals and `@` operands
learn their type from context:

```datalove
fun add(): !u32
    ret ok (1 +! 2)       // 1 and 2 are checked against u32
end fun

let c: int = a@ + b@      // with a, b : u32, both widen to int
```

`Check-BinOp` overlaps `Check-Subsume`. The checker tries `Check-BinOp`
first and, if any premise fails, discards its diagnostics and uses
`Check-Subsume` instead; see [Rule selection](#user-content-rule-selection).

Unary operators:

```
Γ ⊢ e ⇒ T    T ∈ Float⁺ ∪ {int}             Γ ⊢ e ⇒ bool
------------------------------ (Syn-Neg)    ------------------ (Syn-Not)
Γ ⊢ -e ⇒ T                                  Γ ⊢ not e ⇒ bool

Γ ⊢ e ⇒ T    T ∈ Fix⁺    Γ(ret) = !U
------------------------------------ (Syn-NegChecked)
Γ ⊢ -!e ⇒ T

Γ ⊢ e ⇒ T    T ∈ Fix⁺    Γ(ret) = ?U
------------------------------------ (Syn-NegOptional)
Γ ⊢ -?e ⇒ T
```

`-!` and `-?` take every fixed-width integer, as the binary checked and
optional operators do. A signed operand fails only at its minimum; an
unsigned one fails for any value but zero. Bare `-` is for floats,
`float`-bounded parameters and `int`: `-x` on an `i32` variable is F026,
though `-5` checks against `i32` by `Check-NegLit`.

### Try operators

```
Γ ⊢ e ⇒ ?T    Γ(ret) = ?U                     Γ ⊢ e ⇒ !T    Γ(ret) = !U
------------------------- (Syn-TryOption)     ------------------------- (Syn-TryResult)
Γ ⊢ e? ⇒ T                                    Γ ⊢ e! ⇒ T
```

`e?` unwraps an option, returning `none` from the function when it is
empty; `e!` unwraps a result, returning its error. These are the elimination
forms of `?T` and `!T`, and they synthesize, as an elimination form does in
the Pfenning recipe. The operand's shape is checked first (F048), then the
return type (F049). `U` and `T` are unrelated: the early return carries no
payload of type `T`.

### Adapt

The postfix `@` clones, and converts where a type is expected. `T ⇝ U`, "a
`T` adapts to a `U`", is the least relation with:

- `T ⇝ U` when `T ≡ U` (a clone).
- Same-sign integer widening: `u8 ⇝ u16 ⇝ u32 ⇝ u64 ⇝ int` and
  `i8 ⇝ i16 ⇝ i32 ⇝ i64 ⇝ int`, transitively.
- Unsigned to wider signed: `u8 ⇝ i16, i32, i64`; `u16 ⇝ i32, i64`;
  `u32 ⇝ i64`; each also `⇝ int`.
- `index ⇝ int`, `offset ⇝ int`, `f32 ⇝ f64`.
- `atom a ⇝ E` when `atom a ∈ E`, and `term a T ⇝ E` when `term a T' ∈ E`
  with `T ≡ T'`.

Nothing else adapts. There is no narrowing, and no enum adapts to another.

```
Γ ⊢ e ⇒ T    T ⇝ U                Γ ⊢ e ⇒ T
------------------ (Check-Adapt)    -------------- (Syn-Adapt)
Γ ⊢ e@ ⇐ U                         Γ ⊢ e@ ⇒ T
```

With an expected type, `@` converts to it; with none, it clones and keeps
its type. `Check-Adapt` failing is F016, "@ operator cannot convert". An
expected type reaches `@` through any checking rule: an annotated `let`, a
`ret`, a call argument, `Check-BinOp`, or the other operand by `Pair-Adapt`.

### Calls

A call's arguments carry markers that must repeat the parameter modes:
`ref e`, `mut e` and `out e` for those modes, and nothing for `in` and
`const`. A marker that disagrees is F057. Whether a `mut` or `out` argument
denotes something assignable is checked by the ownership pass (D011, D012),
not here.

For a function with no type parameters:

```
Σ(f) = (m₁ T₁, ..., mₙ Tₙ) → R    Γ ⊢ eᵢ ⇐ Tᵢ    const-arg(mᵢ, eᵢ)
------------------------------------------------------------------ (Syn-Call)
Γ ⊢ f(m₁ e₁, ..., mₙ eₙ) ⇒ R
```

`const-arg(m, e)` holds when `m ≠ const`, or `e` is a variable `x` with
`Γ(x) = T^const`. Any other argument to a const parameter, a literal
included, is F076. An unknown function is F002 and a wrong argument count
F045, which stops checking the call.

A qualified call `M.f(...)` is the same with `Σ(M.f)`, the function `f` of
the module or rider required as `M`. A qualifier no `require` introduced is
F078; a function the module lacks is F002.

**Generic calls.** For `Σ(f) = ∀ᾱ::b̄. (m₁ T₁, ..., mₙ Tₙ) → R`, the type
arguments are not written. The checker finds a substitution `θ` for `ᾱ` by
one-way matching, not unification:

1. For each `i` in order such that `Tᵢ` mentions some `α`: if `eᵢ`
   synthesizes a type `Uᵢ` that mentions no type parameter still to be
   determined, match `Tᵢ` against `Uᵢ`, binding each `α` at its position in
   `Tᵢ` to what `Uᵢ` has there. The first binding of an `α` wins.
   An argument that does not synthesize (`none`, `er e`) binds nothing.
2. In checking mode against `T'`, match `R` against `T'` to bind any `α`
   still unbound. Arguments take precedence.

```
θ from step 1     dom θ = ᾱ    θ(αⱼ) ∈ bⱼ    Γ ⊢ eᵢ ⇐ θTᵢ    const-arg(mᵢ, eᵢ)
------------------------------------------------------------------------------ (Syn-GenericCall)
Γ ⊢ f(m₁ e₁, ..., mₙ eₙ) ⇒ θR

θ from steps 1-2    dom θ = ᾱ    θ(αⱼ) ∈ bⱼ    Γ ⊢ eᵢ ⇐ θTᵢ    θR ≡ T'    const-arg(mᵢ, eᵢ)
--------------------------------------------------------------------------------------- (Check-GenericCall)
Γ ⊢ f(m₁ e₁, ..., mₙ eₙ) ⇐ T'
```

`θ(α) ∈ b` reads: `any` and `ord` admit every type, `float` admits `Float`,
and `fixedint` admits `Fix`. A bound refused is F016; a parameter left unbound
(`dom θ ≠ ᾱ`) is F011, "nothing here says what `T` is". Because
`Syn-GenericCall` has no expected type to draw on, a call like
`self == zero()` in operand position is refused for that reason.

Argument order matters, because the first argument to reach an `α` fixes
it. With `pick_first<T>(a: T, b: T)`, `pick_first(99, : u32 / 1)` binds
`T = int` from the bare literal and then fails on the `u32`, while
`pick_first(: u32 / 1, 99)` binds `T = u32` and checks `99` against it.
Likewise `let y: u32 = id(5)` is F016: the argument binds `T = int` before
the expected type is consulted.

**Intrinsics.** `icall n(e₁, ..., eₙ)` calls a built-in with signature
`(T₁, ..., Tₙ) → R`. Its arguments are synthesized and compared, not
checked:

```
icall(n) = (T₁, ..., Tₙ) → R    Γ ⊢ eᵢ ⇒ Uᵢ    Uᵢ ≡ Tᵢ
------------------------------------------------------- (Syn-Intrinsic)
Γ ⊢ icall n(e₁, ..., eₙ) ⇒ R
```

So `icall add_wrapping_u32(a, 2)` is F016, since `2` synthesizes `int`; write
`: u32 / 2`. Arguments take no mode markers (F057). An unknown intrinsic or
wrong argument count is F011.

### Fields and indexes

```
Γ ⊢ e ⇒ (T₀, ..., Tₙ₋₁)    i < n             Γ ⊢ e ⇒ {..., ℓ: T, ...}
------------------------------ (Syn-TupleProj)    ------------------------ (Syn-FieldProj)
Γ ⊢ e.i ⇒ Tᵢ                                       Γ ⊢ e.ℓ ⇒ T
```

A projection on a type with no fields, tables included, is F068; a missing
field F067; a tuple index out of range F069.

Indexing is defined over the element of a collection:

| `C` | `key(C)` | `elem(C)` |
|-----|----------|-----------|
| `[T]` | `index` | `T` |
| `%{K = V}` | `K` | `V` |
| `[|T, 1|]` | `index` | `T` |
| `[|T, k|]`, `k > 1` | `index` | `[|T, k-1|]`, a view |

```
Γ ⊢ e ⇒ C    Γ ⊢ k ⇐ key(C)    Γ(ret) = ?U
----------------------------------------- (Syn-IndexOption)
Γ ⊢ e[k]? ⇒ elem(C)

Γ ⊢ e ⇒ C    Γ ⊢ k ⇐ key(C)    Γ(ret) = !U
----------------------------------------- (Syn-IndexResult)
Γ ⊢ e[k]! ⇒ elem(C)
```

An index is fallible: a missing key or an out-of-range index returns early
from the function, as `?` and `!` do (F049 if the return type does not fit).
An index into something that is not a list, map or tensor is F011, and so is
a bare `e[k]` outside a `set` target.

**Borrowed positions.** A projection or index that reads a part of a linear
type moves it out of the aggregate that still holds it, which is refused:
F070 for a field, F072 for an element. The read is allowed in a *borrowed
position*, where nothing is moved: an operand of an operator, an argument
passed `ref`, `mut` or `out`, the argument of `debuglog`, and the inner steps
of a place. A part of a copy type may be read anywhere. Rules that admit a
read only in a borrowed position carry the side condition
`copy(T) ∨ borrowed`; it is left out of the rules above for brevity. A view
(an element of a tensor of rank above 1) passed `mut` or `out` is F073.

### Rule selection

`check_expr` in `check.rs` chooses a rule in this order:

1. A hinted expression `: T₀ / e` compares `T₀` with the expected type
   (F016, "the type hint disagrees"), then continues with `e`.
2. If a checking rule matches the expression form and the expected type, it
   is used. Its failure is an error, except for `Check-BinOp`, which falls
   back to step 3.
3. Otherwise `Check-Subsume`: synthesize, then compare by `≡`. A failure is
   F016, with a note suggesting `@` where `Check-Adapt` would have succeeded.

Synthesis is syntax-directed throughout, except that `~` and generic
matching try their premises in order. The two backtracking points, then,
are `Check-BinOp` and the speculative synthesis in step 1 of generic
matching.

There is no implicit conversion anywhere: no numeric widening, no implicit
`data`, and no implicit enum injection of a non-literal. Each is written:
`@`, `data e`, or `@` again.

## Statements

### Patterns

`⊢ p : T ⊣ Δ` says pattern `p` matches type `T` and binds `Δ`, a list of
`x : T`. A pattern is one level deep.

```
---------------- (Pat-Var)
⊢ x : T ⊣ x : T

------------------------------------------------------- (Pat-Tuple)
⊢ (x₁, ..., xₙ) : (T₁, ..., Tₙ) ⊣ x₁ : T₁, ..., xₙ : Tₙ

{ℓ₁, ..., ℓₙ} = {ℓ'₁, ..., ℓ'ₙ}    ℓᵢ = ℓ'_π(i)
--------------------------------------------------------------------- (Pat-Struct)
⊢ {ℓ₁ = x₁, ..., ℓₙ = xₙ} : {ℓ'₁: T₁, ..., ℓ'ₙ: Tₙ} ⊣ x₁ : T_π(1), ..., xₙ : T_π(n)

--------------------------------- (Pat-Term)    ------------------------ (Pat-Atom)
⊢ term a x : term a T ⊣ x : T                   ⊢ atom a : atom a ⊣ ·
```

A one-element tuple pattern is written `(x,)`; `(x)` is P070. A struct
pattern names every field, in any order; `{ℓ}` is short for `{ℓ = ℓ}`. `()`
matches unit and binds nothing. An enum is not taken apart by
a pattern but by `match`. A pattern that does not fit is F071, reported at
the value.

### Bindings

`Δ^q` is `Δ` with every entry given binding kind `q`.

```
Γ ⊢ e ⇒ T    ⊢ p : T ⊣ Δ            Γ ⊢ e ⇐ T    ⊢ p : T ⊣ Δ
---------------------------- (Let)    -------------------------------- (Let-Hinted)
Γ ⊢ let p = e ⊣ Γ, Δ^let             Γ ⊢ let p: T = e ⊣ Γ, Δ^let
```

`var` is the same with `Δ^var`, and adds an uninitialized form, which takes
a plain name and needs a type:

```
--------------------------------- (Var-Uninit)
Γ ⊢ var x: T ⊣ Γ, x :^var T
```

Whether it is written before it is read is the ownership pass's business
(D005), not the type checker's.

```
Γ|const ⊢ e ⇒ T                         Γ|const ⊢ e ⇐ T
------------------------------ (Const)    -------------------------------- (Const-Hinted)
Γ ⊢ const x = e ⊣ Γ, x :^const T        Γ ⊢ const x: T = e ⊣ Γ, x :^const T
```

`require data p: T` is `const x: T = ⟨file⟩`, where `x` is the last part of
`p`, and the file is checked against `T` by datalit's
`type_check_with_expected`. A missing file is F079; one not of type `T`, or
with no type of its own and no `T` given, or a `T` that is a function type,
is F084, with datalit's own errors as its notes.

Shadowing is allowed: a binding of a name already in scope replaces it for
the statements that follow.

### Assignment

`Γ ⊢ p :=: T` says `p` is a place that may be assigned a `T`. Its root must
be mutable: a `var` binding, or a `mut` or `out` parameter (which enter `Γ`
with kind `var`).

```
Γ(x) = T^var
-------------- (Place-Var)
Γ ⊢ x :=: T

Γ ⊢ p :=: {..., ℓ: T, ...}         Γ ⊢ p :=: (T₀, ..., Tₙ₋₁)    i < n
-------------------------- (Place-Field)    -------------------------------- (Place-Tuple)
Γ ⊢ p.ℓ :=: T                      Γ ⊢ p.i :=: Tᵢ

Γ ⊢ p :=: C    Γ ⊢ k ⇐ key(C)                    Γ ⊢ p :=: %{K = V}    Γ ⊢ k ⇐ K
------------------------------- (Place-Index)    -------------------------------- (Place-Upsert)
Γ ⊢ p[k]? :=: elem(C)                            Γ ⊢ p[k] :=: V
Γ ⊢ p[k]! :=: elem(C)
```

`Place-Upsert`, the bare index that inserts a missing key, applies only to a
map and only as the last step (F074 otherwise). Writing through a view, an
element of a tensor of rank above 1, is F073.

```
Γ ⊢ p :=: T    Γ ⊢ e ⇐ T    fits(p, Γ)
-------------------------------------- (Set)
Γ ⊢ set p = e ⊣ Γ

Γ ⊢ p :=: T    Γ ⊢ e ⇐ T    T ∈ Arith(⊕)    fits(⊕, Γ)    fits(p, Γ)    p not an upsert
---------------------------------------------------------------------------------------- (Set-Compound)
Γ ⊢ set p ⊕= e ⊣ Γ
```

`fits(p, Γ)` holds when `Γ(ret) = ?U` if any step of `p` is `[k]?`, and
`Γ(ret) = !U` if any is `[k]!` (F049): an index partway along the target
returns early as surely as the last one. An undefined
root is F054; an immutable one is F055. A compound operator the place's type
does not admit is F026. The order in which a `set` evaluates its parts is in
[the spec](botspec.md#user-content-81-bindings).

### Control flow

```
Γ ⊢ e ⇐ bool    Γ ⊢ { s̄₁ }    Γ ⊢ { s̄₂ }
---------------------------------------- (If)
Γ ⊢ if e s̄₁ else s̄₂ end if ⊣ Γ

Γ ⊢ e ⇒ ?T    Γ, x :^let T ⊢ { s̄₁ }    Γ ⊢ { s̄₂ }
------------------------------------------------- (If-Some)
Γ ⊢ if e |x| s̄₁ else s̄₂ end if ⊣ Γ

Γ ⊢ e ⇒ !T    Γ, x :^let T ⊢ { s̄₁ }    Γ, y :^let error ⊢ { s̄₂ }
-------------------------------------------------------------- (If-Ok)
Γ ⊢ if e |x| s̄₁ else |y| s̄₂ end if ⊣ Γ
```

A missing `else` is an empty `s̄₂`, except that a result needs its
`else |y|` (F046). A binding `if` whose condition is neither an option nor a
result is F016, and so is `else |y|` on an option. `else if` is a nested `if`
in the `else` block; a binding `if` cannot take part in such a chain (P073).

```
Γ, loop ⊢ { s̄ }                         Γ ⊢ e ⇐ bool    Γ, loop ⊢ { s̄ }
------------------------- (Loop)        --------------------------------------- (Loop-While)
Γ ⊢ loop s̄ end loop ⊣ Γ                Γ ⊢ loop while e s̄ end loop ⊣ Γ

loop ∈ Γ                                loop ∈ Γ
------------------- (Break)             ---------------------- (Continue)
Γ ⊢ break ⊣ Γ                           Γ ⊢ continue ⊣ Γ
```

`break` or `continue` outside a loop is F050 or F051.

```
Γ ⊢ e ⇒ E    E = enum {v₁, ..., vₙ}
each case names a distinct variant of E, of the same kind
for case atom a:     Γ ⊢ { s̄ }
for case term a x:   term a T ∈ E    Γ, x :^let T ⊢ { s̄ }
for case default:    Γ ⊢ { s̄ }
the cases name every vᵢ, or there is a default
---------------------------------------------------------- (Match)
Γ ⊢ match e cases end match ⊣ Γ
```

The input must synthesize an enum; options and results are taken apart by
the binding `if`, not by `match`. Every failure of `Match` is F016: an input
that is not an enum, a case naming no variant, an atom case for a term
variant or the reverse, a duplicate case, and a missing variant with no
default.

```
Γ(ret) = R    Γ ⊢ e ⇐ R           Γ(ret) = void
----------------------- (Ret)      --------------- (Ret-Void)
Γ ⊢ ret e ⊣ Γ                      Γ ⊢ ret ⊣ Γ
```

`ret e` in a void function is F052;
bare `ret` in a function with a return type is F053, `fun f(): ()` included.
A `ret` at the top level of a module is refused by the parser (P077).

```
Γ ⊢ e ⇒ T                         Γ ⊢ f(ā) ⇒ ()
---------------------- (DebugLog)   ------------------------ (Call)
Γ ⊢ debuglog e ⊣ Γ                 Γ ⊢ call f(ā) ⊣ Γ
```

`debuglog` takes any type, in a borrowed position. A `call` statement's
callee must return `()`: a value computed and discarded is F016. `call`
followed by anything but a call is P031.

### Functions

For `fun f<ᾱ>(m₁ x₁: T₁, ..., mₙ xₙ: Tₙ): R where {α :: b, ...}`, with
`Σ(f)` as collected:

```
Γ_f = Γ|const, ᾱ :: b̄, x₁ :^q(m₁) T₁, ..., xₙ :^q(mₙ) Tₙ, ret R̂
Γ_f ⊢ s̄ ⊣ Γ'    R̂ ≠ void  ⟹  ¬completes(s̄)    mᵢ = const  ⟹  Tᵢ mentions no α
------------------------------------------------------------------------------- (Fun)
Γ ⊢ fun f ... s̄ end fun ⊣ Γ
```

where `q(in) = q(ref) = let`, `q(mut) = q(out) = var`, `q(const) = const`,
and `R̂` is `R` when a return type is written and `void` when none is. A
`void` function returns `()`. An unwritten bound is `any`. A const parameter
whose type mentions a type parameter is F077.

The body sees the consts in scope but no other enclosing binding, and no
`loop` entry. A function's own parameters are in `Γ_f` under their modes'
kinds, so `set` on an `in` or `ref` parameter is F055.

`completes(s̄)`, "control can reach the end of `s̄`", is the conservative
reachability test that F063 uses: a function with a return type whose body
completes is F063. It is defined by:

- A sequence completes when each statement in it does.
- `ret`, `break` and `continue` do not complete.
- `if` without `else` completes; with one, when either branch completes.
- `loop while` completes. A bare `loop` completes when its body contains a
  `break` belonging to it, not to a nested loop.
- `match` completes when any arm, or its default, completes.
- Every other statement completes, including those containing `?`, `!` and
  calls.

`native fun` declares a signature implemented by a rider. In a module or
script it is F062. `inline`, as the first statement of a body, marks the
function for inlining; inline functions that call each other in a cycle are
F085.

A type parameter used as a set element or map key, in a signature or in the
type of a binding in the body, needs a bound that implies `ord` (all of them
do, other than `any`); otherwise F061.

A name is defined by one function in a module or script: a second `fun` or
`native fun` of the same name is F086, and so is an `import` of a name a
function there defines. The first definition stays, so the rest of the unit
resolves against it. A `fun`, `native fun` or `type` is only a declaration at
the top level; inside a block it is P074.

### Modules and scripts

**Modules.** A module's top level holds declarations only: `fun`,
`native fun`, `const`, `type`, `require` and `import`. Any other statement
there would never run, and the parser refuses it (P077). `Σ` is collected
from every `fun` and `native fun` first, so order of definition does not
matter. Module-level consts are checked next, before any other statement,
and are in scope throughout the module. Module top level has no `ret` entry,
so `?`, `!` and the checked and optional operators in a module const are
F049 ("no enclosing function").

**Scripts.** A script's top level is checked under `Γ, ret !()`: it is a
body returning a result, so `!`, `+!` and `ret ok ()` work there and `?`
does not. A script is checked as a sequence of units; a later unit sees the
bindings of earlier ones. A unit that is a lone expression is synthesized,
at any type.

**Requires and imports.** `require module p`, `require rider p` and
`require data p` bind an alias; `import` brings functions into `Σ`
unqualified. Their errors are F059 and F078 to F084 below. A `require`
refused is reported where it is written, and calls through its alias are
not reported again.

## Diagnostics

| Code | Meaning |
|------|---------|
| F001 | Undefined variable |
| F002 | Undefined function; an import or qualified call of a function the module or rider does not have |
| F011 | No synthesis rule: `none`, `er e` or an enum literal with nothing expected; a type parameter left unbound at a call; an index into something that is not a list, map or tensor, read or `set`; a bare `e[k]` outside a `set` target; an unknown intrinsic or wrong intrinsic arity |
| F016 | Type mismatch: a failed `Check-Subsume`; operands that do not pair; a collection element that differs from the first; a hint that disagrees with the expected type; `@` that cannot convert; a bound not satisfied; an intrinsic argument; a binding `if` on something that is not an option or result, or `else \|y\|` on an option; a `call` whose callee does not return `()`; every `match` error |
| F026 | An operator applied to a type it does not admit, including a compound `set` |
| F045 | Wrong number of arguments |
| F046 | A binding `if` on a result without `else \|y\|` |
| F048 | `?` on a non-option or `!` on a non-result |
| F049 | An early-return form (`?`, `!`, `[k]?`, `[k]!`, the checked and optional operators, and their compound `set` forms) whose return type does not fit, or outside any function |
| F050 | `break` outside a loop |
| F051 | `continue` outside a loop |
| F052 | `ret e` in a function with no declared return type |
| F053 | Bare `ret` in a function with a return type |
| F054 | `set` on an undefined variable |
| F055 | `set` on an immutable binding |
| F057 | Argument mode marker does not match the parameter's mode |
| F058 | A const initializer names something that is not a const |
| F059 | A name imported twice |
| F061 | A type parameter used as a set element or map key, in a signature or a body binding, without `ord` |
| F062 | `native fun` outside a rider |
| F063 | A function with a return type whose body can complete |
| F064 | Unknown type name |
| F065 | Integer or hex literal out of range for its type |
| F066 | A type alias defined twice |
| F067 | No such field |
| F068 | Projection on a type with no fields |
| F069 | Tuple index out of range |
| F070 | A non-copy field read outside a borrowed position |
| F071 | Destructuring pattern does not fit the value |
| F072 | A non-copy element read outside a borrowed position |
| F073 | A tensor view passed `mut` or `out`, or written by `set` |
| F074 | A bare index in a `set` target that is not the last step, not into a map, or under a compound assignment |
| F075 | A literal whose shape does not fit its type: tuple length, struct fields, table columns, tensor rank or element count |
| F076 | An argument to a const parameter that is not a const binding |
| F077 | A const parameter whose type mentions a type parameter |
| F078 | An import from, or qualified call through, a name no `require` introduced |
| F079 | A `require` of a module, rider or data file that does not exist |
| F080 | The same module or rider required twice in one unit |
| F081 | A module required under an alias another required module already has |
| F082 | `require rider` in a script |
| F083 | A `require` that closes a cycle of modules |
| F084 | A `require data` whose file does not have the given type, has no type of its own and is given none, or is given a function type |
| F085 | `inline` functions calling each other in a cycle |
| F086 | A function defined twice in one module or script, or an import of a name a function there defines |

F047 and F056 are unassigned. F060 is retired. Two parse errors belong to
the same rules: P074, a declaration inside a block, and P077, a statement at
the top level of a module that is not a declaration. The datalit checker's
`TypeError` converts into datafun's, but datafun reports its literal errors
through its own checks, so the datalit T-codes do not appear in datafun
programs except as the notes of F084.

## Implementation

The checker is split by judgment: `synthesize.rs` (`⇒`), `check.rs` (`⇐`),
`statement.rs` (`⊣`), with the context in `context.rs` and the entry points
in `api.rs`. Signatures are collected in `crates/datalove-datafun-resolve`.

The context is `TypeContext`. Its variable map is `Γ`'s variables; its
`type_aliases` overlay holds the type parameters as `Type::Var`, with their
bounds in `type_param_bounds`; the expected return type and
`is_void_function` are `Γ(ret)`; `loop_depth` is the `loop` entry;
`in_const_expr` restricts lookups to `Γ|const`; and `ref_context` and
`mut_context` mark borrowed positions.

`TypeFunction` (`crates/datalove-datafun-common`) holds a signature's
parameter types, modes, const flags and return type. Type parameters and
their bounds are read from the callee's AST at the call, so a native
function, which has none, is never generic.

Expression types are stored in a `BTreeMap` keyed by `ExprKey`: module,
enclosing function and index within it (see
[Salsa Patterns](salsa-patterns.md) for why not a vector). Each call stores
a `ResolvedCallTarget`, the callee's AST, module and type arguments, so later
passes do not look names up again. Qualified calls resolve into
`TypeContext::qualified`, not the name table, so `u8.from_int` and
`i8.from_int` sit side by side; a module's resolve in
`resolve_module_imports`, a script unit's in `typecheck_script_unit`, through
`module_alias_at` for an alias an earlier unit required.

An `AutoAdaptMode` inserts `@` where `Check-Adapt` would succeed instead of
reporting F016. Only tests enable it.

## References

- Jana Dunfield and Neel Krishnaswami, "Bidirectional Typing", ACM
  Computing Surveys 2021. The judgment forms, mode-correctness, and the
  Pfenning recipe: introduction forms check, elimination forms synthesize.
- Benjamin C. Pierce and David N. Turner, "Local Type Inference", TOPLAS
  2000. Synthesizing type arguments from arguments, locally.
- Robert Harper, *Practical Foundations for Programming Languages*, 2nd ed.,
  Cambridge 2016. Contexts, statement judgments and the presentation of
  rules.
