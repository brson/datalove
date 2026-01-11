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




<br>

### 4 things to remember about Datalove

<div class="four-things-grid">
  <div class="thing-box">
    <p>A data serialization, configuration and interchange format
       for common data types,
       with a focus on numerical correctness.</p>
  </div>
  <div class="thing-box">
    <p>A pure-functional language
       that reads like an imperative language,
       with a simple but sophisticated linear type system.</p>
  </div>
  <div class="thing-box">
    <p>Simple and complete <em>syntax and machine representation</em>
       of common data types for <em>interchange and transformation.</em></p>
  </div>
  <div class="thing-box">
    <p>Modern compilation and execution architecture with
       <em>rapid recompilation</em> and <em>script</em> iteration,
       interactive / <em>REPL</em>,
       <em>AOT</em>-render to <em>staticly-linked</em> binaries.</p>
  </div>
</div>

<!--    
  Novel statement- and line-oriented lexical structure.
-->


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
dynamically-typed data and error types.

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
_This language is not implemented yet,
and mostly not discussed in the documentation._




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
