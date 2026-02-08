# Atom/Tag Keyword Brainstorm

The two concepts:

- **"atom"**: named unit type, no payload (`atom Foo`)
- **"tag"**: named wrapper carrying a typed payload (`tag Foo int`)

Warts with current design:
two keywords for atom/tag,
and the keywords are different lengths (4 vs 3).

Goal: find pairs with the same character count.
Either `atom` or `tag` can be reused.


## 3-char pairs (keeping `tag`)

**`sym` / `tag`** --
"symbol" for the bare name, "tag" for the data-carrying one.
Established precedent from Lisp/Erlang/Ruby.

```datalove
type Shape: enum {
  sym Circle,
  tag Rect (f32, f32),
}
let s = sym Circle@
match s
case sym Circle
case tag Rect dims
end match
```

**`key` / `tag`** --
"keyword" in the Clojure sense.
But `key` has map/dict connotations that might mislead.


## 4-char pairs (keeping `atom`)

**`atom` / `wrap`** --
Atom is indivisible, wrap wraps a value.
Clear, immediate semantics.

```datalove
type Shape: enum {
  atom Circle,
  wrap Rect (f32, f32),
}
let s: Shape = wrap Rect (1.0, 2.0)@
match s
case atom Circle
case wrap Rect dims
end match
```

**`atom` / `hold`** --
"Holds" a value. Slightly more noun-like than wrap.

**`atom` / `pack`** --
Packs a value in. Short, punchy.


## 4-char pairs (new for both)

**`bare` / `wrap`** --
Direct semantic opposition: bare carries nothing, wrap carries something.

```datalove
type Shape: enum {
  bare Circle,
  wrap Rect (f32, f32),
}
```

**`flag` / `slot`** --
Flag marks presence, slot holds data. Both concrete nouns.

**`name` / `wrap`** --
A name stands alone, a wrap carries something.

**`sign` / `term`** --
From logic. A sign is a bare signifier, a term carries structure.


## Logic/relational-inspired pairs

The atom/tag distinction maps onto several logic/relational distinctions:

- **atom concept**: no arguments, bare named truth --
  like a proposition, a constant, a nullary predicate, a Prolog atom.
- **tag concept**: carries arguments/data --
  like a predicate, a compound term, a functor applied to args.

**`atom` / `term`** (4/4) --
Directly from FOL/Prolog.
In Prolog, `foo` is an atom, `foo(1, bar)` is a compound term.
This is literally the same distinction.

```datalove
type Shape: enum {
  atom Circle,
  term Rect (f32, f32),
}
match s
case atom Circle
case term Rect dims
end match
```

Pro: precise, well-precedented.
Con: "term" is a bit generic in everyday English.

**`prop` / `pred`** (4/4) --
Proposition (0-ary, no arguments) vs predicate (takes arguments).
THE distinction from logic that separates propositional logic from predicate logic.

```datalove
type Shape: enum {
  prop Circle,
  pred Rect (f32, f32),
}
```

Pro: the conceptual mapping is perfect.
Con: `pred` reads like "predecessor" to many programmers.

**`atom` / `form`** (4/4) --
Atom (indivisible) vs form (structured shape).
"Form" from philosophy/logic --
the Platonic form is the abstract shape/structure of a thing.
Also "formula" in logic.

```datalove
type Shape: enum {
  atom Circle,
  form Rect (f32, f32),
}
```

Pro: `form Rect (f32, f32)` reads very naturally.
Con: "form" is moderately overloaded (HTML forms, etc).

**`fact` / `term`** (4/4) --
From Datalog. A fact is a ground assertion, a term carries structure.

Pro: "fact" fits for a bare named truth in a data-oriented language.
Con: `fact Quit` reads slightly odd --
facts in Datalog can also carry data.

**`prop` / `term`** (4/4) --
Proposition (bare truth claim) vs term (structured expression).

Pro: clean pairing from two levels of logic.
Con: mixing vocabulary from propositional logic and term algebra.

**`sym` / `rel`** (3/3) --
Symbol (bare name) vs relation (carries attributes/columns).
Relational/Datalog flavor.

```datalove
type Shape: enum {
  sym Circle,
  rel Rect (f32, f32),
}
```

Pro: `rel` is fitting for a data-oriented language --
a relation IS a named thing that carries structured data.
Con: `rel` might read as "relative."


## Overall ranking

1. **`atom` / `term`** (4/4) -- Prolog precedent is exact, both words familiar, reads well in all positions.
2. **`atom` / `wrap`** (4/4) -- sheer clarity of meaning.
3. **`atom` / `form`** (4/4) -- evocative, reads naturally with payloads.
4. **`prop` / `pred`** (4/4) -- conceptually perfect but `pred` has readability issues.
5. **`sym` / `tag`** (3/3) -- best short pair.
6. **`sym` / `rel`** (3/3) -- most "datalove-flavored" short pair.
