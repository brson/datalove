# Atom / Tag / Enum Design: PL Landscape

Research on precedents for Datalove's planned atom/tag/enum/match design.

## The Datalove Design

Three type formers:

- **atom**: named unit type, no payload. `atom Foo` is both value and type.
  Two atoms match by name (structural).
- **tag**: named wrapper with typed payload. `tag Foo 1` has type `tag Foo int`.
  Two tags match by name + payload type (structural).
- **enum**: closed union of atoms and tags.
  `enum { atom Foo, tag Bar int }` sums them.

Atoms and tags exist as standalone types.
Enums collect them.
The `@` operator widens an atom or tag into an enum type.
`match` destructures enums.

## 1. Atoms: Precedents

The concept of a named singleton value is well-established.

**Erlang/Elixir atoms.**
Runtime-interned symbols compared by identity.
In Elixir v1.17+ (set-theoretic type system),
individual atoms like `:ok` are their own types,
subtypes of `atom()`.
This is the closest precedent for "atom as type."

**Prolog atoms.**
Fundamental term type. Structurally compared.
No type-level significance (Prolog is untyped).

**Lisp/Scheme symbols.**
Interned, `eq?`-comparable. Used as data and as code identifiers.
Not a type-level concept in standard Scheme;
Typed Racket has `Symbol` but not singleton symbol types.

**Ruby/Crystal symbols.**
`:foo` syntax. Interned strings used as lightweight identifiers.
Crystal's type system includes symbol literal types in unions.

**TypeScript string literal types.**
`"foo"` as a type is a singleton type.
Purely compile-time (erased at runtime).
Structural. Used as discriminants in tagged unions.

**Scala literal types (SIP-23).**
`42` and `"foo"` can appear in type position.
`ValueOf[T]` provides runtime access.

### Where Datalove's atoms sit

Closest to Elixir's set-theoretic atoms:
an atom is both a value and a type, structural, can stand alone.
The `atom` keyword is syntactic sugar that Elixir lacks
(Elixir atoms are just lowercase identifiers or `:symbols`).

Datalove atoms are more disciplined than Erlang atoms:
they participate in a static type system
and can only appear in enum positions (not as arbitrary values everywhere).

## 2. Tags: Precedents

A "tag" is a named constructor carrying a typed payload,
where the constructor itself is a type.

**OCaml polymorphic variants** --- closest precedent.
`` `Foo of int`` creates a value of type `` [> `Foo of int] ``.
The variant exists independently of any type declaration.
Structurally typed, open (row-polymorphic).
Can be combined into unions: `` [`Foo of int | `Bar] ``.

Key differences from Datalove tags:
- OCaml polymorphic variants are open (row variables);
  Datalove enums are closed.
- OCaml uses backtick syntax, no keyword.
- OCaml polymorphic variants have known ergonomic issues
  (complex error messages, performance overhead from hash-based dispatch).

**Erlang tagged tuples.**
Convention of `{ok, Value}`, `{error, Reason}`.
Not a language feature --- just a coding pattern.
No type enforcement (until Elixir's type system).

**TypeScript object types with discriminants.**
`{ kind: "rect", w: number, h: number }` is a type.
The discriminant field is a literal type.
Each branch of a discriminated union is a standalone type.
Structural, but the "tagging" is a field, not a wrapper.

**Haskell newtypes / constructors.**
`newtype Foo = Foo Int` creates a nominal wrapper.
The constructor `Foo` is not a type; it's a value-level function.
With DataKinds, constructors can be promoted to the type level,
but this is a different mechanism.

**PureScript Variant type.**
Uses row polymorphism.
`Variant (foo :: Int, bar :: String)` gives open tagged unions
where each label+type is a "tag" in the row.

### Where Datalove's tags sit

Most similar to OCaml polymorphic variants
but with closed rather than open union semantics.
The `tag` keyword makes the construction explicit
(OCaml uses backtick which is easy to miss).

The key design choice: tags are types, not just constructors.
`tag Foo int` is a type you can annotate variables with,
pass as a type argument, etc.
This is shared with OCaml polymorphic variants and TypeScript object types,
but not with Haskell/Rust/OCaml regular variants.

## 3. Variants as Types: Precedents

In most ADT systems (Haskell, OCaml regular, Rust, Swift, F#),
variants are constructors, not types.
`Some(42)` has type `Option<i32>`, not type `Some<i32>`.
You can't annotate a variable with the type of a single variant.

Languages where variants ARE types:

**TypeScript.**
Each member of a union is a full type.
`string | number` --- each side is a type.
Discriminated unions: each branch is a type that can be used standalone.

**OCaml polymorphic variants.**
`` `Foo of int`` is a type.
`` [`Foo of int | `Bar] `` is a union of types.
Each variant exists independently.

**Ceylon.**
`of` clause lists subtypes.
Each case is a distinct type that can appear in type expressions.

**Scala 3.**
Singleton enum cases have their own singleton types.
`case Foo` in an enum has type `Foo.type`.

**Elixir set-theoretic types.**
`:ok` is a type. `{:ok, integer()}` is a type.
Union types combine them: `:ok | {:ok, integer()} | :error`.

**Crystal.**
Union types `Int32 | String` where each member is a type.
Symbol literal types can participate in unions.

**MLsub / algebraic subtyping (Dolan, 2017).**
Academic work combining ML polymorphism with structural subtyping.
Union and intersection types with principal type inference.
Variant types where each injection creates a distinct type.

### Where Datalove sits

Datalove's approach of making each variant a type
aligns with the structural/union-type tradition
(TypeScript, OCaml polymorphic variants, Elixir, Crystal)
rather than the nominal/ADT tradition
(Haskell, Rust, OCaml regular variants).

The `enum` keyword then acts as a closed union operator,
like TypeScript's `|` but with exhaustiveness checking.

## 4. Two Kinds of Variant: Atom vs Tag

This is the most unusual aspect of the design.

**Standard ADTs** use a unified syntactic form.
Haskell: `data T = Foo | Bar Int` --- both are "constructors,"
Foo is nullary, Bar is unary. Same syntax, same concept.

**Rust** is the closest precedent for distinct variant forms.
Three syntactic forms: unit (`Foo`), tuple (`Bar(i32)`), struct (`Baz { x: i32 }`).
But all are "variants" conceptually; the distinction is about payload shape,
not about a fundamental atom-vs-tag divide.

**Erlang convention.**
Bare atoms (`ok`, `error`) vs tagged tuples (`{ok, Value}`).
Two fundamentally different data shapes used in the same union context.
This is the cultural precedent ---
Datalove's atom/tag distinction formalizes what Erlang does by convention.

**Type theory.**
Sum types use injections: `inl: A -> A + B`, `inr: B -> A + B`.
A "unit variant" is just injection of the unit type: `inl: () -> () + B`.
No fundamental distinction; unit variants are data variants with `()` payload.

**Row polymorphism.**
Labels map to types. A label with no data maps to `()`.
No structural distinction between "label present with no data"
and "label present with data."

**Protocol Buffers `oneof`.**
All fields carry some type. Empty messages are valid but
there's no concept of "payload-less variant."

### Assessment

Having two syntactically distinct variant forms is unusual.
Most languages treat unit variants as a special case of data variants.

The strongest precedent is Erlang's atom-vs-tagged-tuple idiom,
which Datalove makes explicit and type-checked.

The practical benefit: `atom Foo` is visually distinct from `tag Bar int`,
making it immediately clear whether a variant carries data.
The cost: two keywords and two concepts where most languages have one.

Note that `atom Foo` is NOT equivalent to `tag Foo ()`.
An atom has no payload at all; a tag always has exactly one.
This is a meaningful distinction in a language with move semantics,
even though type theory would normally equate them.

## 5. The `@` Coercion Operator

Widening from atom/tag to enum: `atom Foo@` or `tag Bar 1@`.

**TypeScript** does implicit widening (assignability).
`const x: string | number = "hello"` just works.

**OCaml polymorphic variants** also do implicit subtyping.
`` let x: [`Foo | `Bar of int] = `Foo `` just works.

**Scala** implicit widening for enum cases.

**Datalove** makes this explicit with `@`.
This is distinctive. No other language I found
uses an explicit operator for variant-to-union widening.

The rationale (from report-expr-primary.md):
explicit coercion keeps the type system simpler
and interacts well with primary-expression parsing
(`tag Bar 1@` parses as `(tag Bar 1)@`).

## 6. Connection to GADTs and Const Specialization

From report-gadts.md, enums with const indices:

```
enum Expr<const TAG: TypeTag>
    LitInt(i64)   where TAG == TInt
    LitBool(bool) where TAG == TBool
end enum
```

The atom/tag/enum design provides a clean foundation here.
In a GADT world, the variants of a const-indexed enum
are still atoms and tags, but with where-clause constraints.
The `match` statement already refines by variant;
adding const-index refinement is an orthogonal extension.

From const-param-specialization.md, the union-branch strategy:
enums of const values drive code specialization.
The enum type itself is the defunctionalization target.
This creates a deep connection between
the user-facing `enum` type and the compilation strategy.

## Summary: Placement in the Landscape

| Concept | Closest Precedents | Novelty |
|---------|-------------------|---------|
| `atom` as type | Elixir set-theoretic atoms, TypeScript literal types | Low --- well-established |
| `tag` as type | OCaml polymorphic variants | Low-medium --- variants-as-types is known |
| `enum` as closed union of variants-that-are-types | TypeScript discriminated unions, OCaml polyvar unions | Low --- standard union type |
| Two syntactic variant forms (atom vs tag) | Erlang convention (formalized), Rust 3-form variants | Medium-high --- unusual |
| Explicit `@` coercion | None found | High --- distinctive |
| Closed (not open/row-polymorphic) variants-as-types | Ceylon, Elixir | Medium --- most variants-as-types systems are open |

The overall design is a synthesis of:
- Erlang's atom/tagged-tuple convention (made type-safe)
- OCaml polymorphic variants (but closed, not open)
- TypeScript discriminated unions (but with dedicated syntax)

The most unusual aspects are the explicit atom/tag distinction
and the `@` coercion operator. These are defensible design choices
that trade some familiarity for clarity and parsing simplicity.

### Sources

- Elixir v1.17 set-theoretic types: https://elixir-lang.org/blog/2024/06/12/elixir-v1-17-0-released/
- OCaml polymorphic variants: https://ocaml.org/manual/polyvariant.html
- Real World OCaml, Variants chapter: https://dev.realworldocaml.org/variants.html
- Gaster & Jones, "A Polymorphic Type System for Extensible Records and Variants" (1996)
- TypeScript discriminated unions: https://www.typescriptlang.org/docs/handbook/2/narrowing.html
- Dolan, "Algebraic Subtyping" (2017), POPL: https://dl.acm.org/doi/10.1145/3009837.3009882
- Ceylon enumerated types: https://ceylon-lang.org/blog/2012/01/25/enumerated-types/
- Scala 3 enums: https://docs.scala-lang.org/scala3/reference/enums/enums.html
- Harper & Morrisett, "Compiling Polymorphism Using Intensional Type Analysis" (1995), POPL
- Crystal union types: https://crystal-lang.org/reference/syntax_and_semantics/union_types.html
- PureScript Variant: https://pursuit.purescript.org/packages/purescript-variant
