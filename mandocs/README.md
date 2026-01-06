# The Datalove Guide

Datalove is
a simple and expressive interactive scripting language &mdash;
strongly and statically typed, but with ergonomic coercions &mdash;
for efficient data modeling and transformation.

Datalove is built around one core idea:
first let us define a simple but complete language
for writing, typing, serializing, and transforming a sufficient variety
of modern pure data types.
Let's do that really well.
Then we'll add I/O to it &mdash; carefully.

<!--

As a Rust programmer who sometimes
scripts and prototypes in Python,
Datalove was born from my dissatisfaction with
Python's weak typing and inconsistent
facilities for expressing simple data structures.
Datalove is most usefully compared to Python, JavaScript, and Julia,
through a Rustic linear-typing lens.

-->

> Datalove is a testbed for my personal compiler and language design experiments.
> There is no recommended way to install or test.




### 4 Things to know about Datalove

<div class="four-things-grid">
  <div class="thing-box">
    <h4>Simple Types</h4>
    <p>A clean, orthogonal type system built from familiar primitives: tuples, structs, enums, lists, sets, maps, and tensors.</p>
  </div>
  <div class="thing-box">
    <h4>Pure Functions</h4>
    <p>Imperative-feeling syntax with pure functional semantics. No hidden state, no surprises.</p>
  </div>
  <div class="thing-box">
    <h4>Static Typing</h4>
    <p>Strongly and statically typed with ergonomic inference. Catch errors at compile time, not runtime.</p>
  </div>
  <div class="thing-box">
    <h4>Data First</h4>
    <p>Designed for data transformation. Serialize, query, and reshape structured data with ease.</p>
  </div>
</div>




Datalove is built from three cleanly-scoped strict sublanguages of increasing power:
[Datalove Literals](#user-content-datalove-literals),
[Datalove Functions](#user-content-datalove-functions),
and [Datalove with Side Effects](#user-content-datalove-with-side-effects).




### Datalove Literals

The tiny and comprehensible foundation of Datalove, a strongly-typed and
declarative pure-data language for expressing typical data structures.

It includes booleans, fixed integers and bigints, floats;
anonymous tuples, structs, and enums;
strings, lists, maps and sets, tensors;
option and result;
existential runtime-typed data and error types.

```datalove
{
  name = "Ada",
  born = 1815,
  interests = ["mathematics", "poetry", "music"],
  address_book = set {
    {
      kind = enum Friend,
      name = "Charles",
    },
    {
      kind = enum Family,
      name = "George",
    },
  },
}
```




### Datalove Functions

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

```datalove
typealias Person: {
  name: string,
  born: int,
  interests: [string],
  address_book: set {
    {
      kind: enum { Friend, Family },
      name: string,
    }
  }
}

fun get_friends(ref person: Person): [string]
  // todo
end fun
```




### Datalove with Side Effects

The complete language with I/O-bearing procedures,
owned native pointers, objects with identity,
and stack unwinding.

> This language is not implemented yet,
  and mostly not discussed here.




---




- [Features](features.md)
- [Principles](principles.md)
- [Datalove Compared To...](comparisons.md)
- [Design Notes](design-notes.md)
- [Novelties](novelties.md)
- [Datalit Types](datalit-types.md)
- [Datalit Runtime Types](datalit-runtime-types.md)
- [Compiler Guide](todo.md)
- [Testing](testing-tower.md)
- [Influences](influences.md)
