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

> Datalove is a testbed for my personal compiler and language design experiments.
> There is no recommended way to install or test.




<br>

<h2><center>4 things to remember about Datalove</center></h2>

<div class="four-things-grid">
  <div class="thing-box">
    <p>A <em>data serialization, configuration and interchange format</em>
       for common data types,
       with a focus on <em>numerical correctness.</em></p>
  </div>
  <div class="thing-box">
    <p>A <em>pure-functional language</em>
       that reads like an imperative language,
       with a simple but sophisticated <em>linear type system.</em></p>
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




<br>
<center>Datalove is built from three sublanguages of increasing power.</center>
<br>



### Datalove Literals

The tiny and comprehensible foundation of Datalove, a strongly-typed and
declarative pure-data language for expressing typical data structures.

It includes booleans, fixed integers and bigints, floats;
anonymous tuples, structs, and enums;
strings, lists, maps and sets.

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

It also includes _tensors_ (multi-dimensional arrays)
and _tables_ (dataframes / structs-of-arrays).

```datalove
todo
```

It also includes optional and result types.

```datalove
todo
```

It includes two dynamic types:
`data`, for general dynamic typing;
and `error`, the payload for result types.

```datalove
todo
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
and mostly not discussed in the documentation_,
but I do have a [vision](vision.md) about what it will be.




---



- [10 Minute Intro](intro.md)
- [Principles](principles.md)
- [Lexical Structure](lexer.md)
- [Datalove Literals](datalit-types.md)
- [Modules, Functions and Scripts](modules-functions-scripts.md)
- [Control Flow](control-flow.md)
- [Operators](operators.md)
- [Optional and Result Types and Operations](checked-types.md)
- [Heaps and Multithreading](heaps.md)
- [Roadmap](roadmap.md)
- [Vision](vision.md)

---

- [Datalove Literals Runtime Types](datalit-runtime-types.md)
- [Compiler Guide](compiler-guide.md)
- [Testing](testing-tower.md)
- [Influences](influences.md)

---

- [Features](features.md)
- [Design Notes](design-notes.md)
- [Novelties](novelties.md)
