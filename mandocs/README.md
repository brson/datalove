# The Datalove Guide

Datalove is an interactive scripting language
for efficient data modeling and transformation.
It strongly and statically typed,
but with ergonomic coercions
and an efficient compiler pipeline.

Datalove is built around one core idea:
first let us define a simple but complete language
for writing, typing, serializing, and transforming a sufficient variety
of modern pure data types.
Let's do that really well.
Then we'll add I/O to it &mdash; carefully.




<br>




> **Roadmap**: [9 of 21 complete](roadmap.md) &middot; updated 2026-01-22.

> **Latest news**: [Precise drops](posts.html) &middot; updated 2026-01-24.




<br>




<h2><center>4 Things to Remember <br> about Datalove</center></h2>

<div class="four-things-grid">
  <div class="thing-box">
    <p>Part <em>data serialization, configuration and interchange format</em>
       for common <em>modern</em> data types,
       with a focus on <em>numerical correctness.</em></p>
  </div>
  <div class="thing-box">
    <p>Part <em>pure-functional language</em>
       that reads like an imperative language,
       with a simple but sophisticated <em>linear type system.</em></p>
  </div>
  <div class="thing-box">
    <p></p>
  </div>
  <div class="thing-box">
    <p>Modern compilation and execution architecture with
       <em>fully-memoized recompilation</em> and <em>rapid script iteration</em>,
       <em>fully-parallelized compiler pipelinie</em>,
       interactive (<em>REPL</em>) interpreter with <em>JIT</em>,
       <em>compiles</em> to <em>staticly-linked</em> binaries.</p>
  </div>
</div>




<br>

---

<br>




Datalove is built from three sublanguages of increasing power.




### Datalove Literals

The tiny and comprehensible foundation of Datalove, a strongly-typed and
declarative pure-data language for expressing typical data structures.

It includes booleans, fixed integers and bigints, floats;
anonymous tuples, structs, and enums;
strings, lists, maps and sets.

```datalove
{
  title = "Nineteen Eighty-Four",
  author = "George Orwell",
  year = 1949,
  rating = 4.7,
  available = true,
  genres = [enum Fiction, enum Reality],
  subtitle = none,
}
```

It includes first-class _tables_ (dataframes / structs-of-arrays).

```datalove
{|
  title,              author,          year
  "1984",             "Orwell",        1949
  "Brave New World",  "Huxley",        1932
  "Fahrenheit 451",   "Bradbury",      1953
|}
```

Also lists, sets, maps, _tensors_ (n-dimensional arrays), typical scalar types.




### Datalove Functions

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

```datalove
type Book: {
  title: string,
  author: string,
  year: i32,
  rating: f32,
  available: bool,
  genres: [enum { Fiction, Reality }],
}

fun reserve_book(mut db: set<Book>, ref book: Book): !()
  for mut db_book in db
    if db_book.title == book.title
      if db_book.available
        set db_book.available = false
        ret ok ()
      else
        ret er error atom BookNotAvailable
      end if
    end if
  end for
  ret er error atom BookNotFound
end fun
```




### Datalove with Side Effects

The complete language with I/O-bearing procedures,
owned native pointers, objects with identity,
and stack unwinding.
_This language is not implemented yet,
and mostly not discussed in the documentation_,
but I do have a [vision](vision.md) about what it will be.



<br>




---

<!-- User-oriented chapters -->

- [Principles](principles.md)


---

<!-- Language -->

- [Lexical Structure](lexer.md)
- [Datalove Literals](datalit-types.md)
- [Modules, Functions and Scripts](modules-functions-scripts.md)
- [Control Flow](control-flow.md)
- [Operators](operators.md)
- [Optional and Result Types and Operations](checked-types.md)
- [Moves, Copies, References](moves-etc.md)
- [Constant Evaluation](const-eval.md)
- [Datalove Worlds](worlds.md)


---

<!-- Runtime -->

- [Datalove Literals Runtime Types](datalit-runtime-types.md)


---

<!-- Lists -->

- [Novelties](novelties.md)
- [Vision](vision.md)
- [Roadmap](roadmap.md)
- [Influences](influences.md)


---

<!-- Developer-oriented chapters -->

- [Compiler Guide](compiler-guide.md)
- [Issues](issues.md)
- [Design Notes](design-notes.md)
- [Future Designs](future-designs.md)
- [Potential Changes](potential-changes.md)
- [Testing](testing-tower.md)


---

<!-- Unsorted junk -->

- [Features](features.md)
- [More Datalit Types](more-datalit-types.md)
- [Total Functions](total-functions.md)
- [Panicking](panicking.md)
- [Script Semantics](script-semantics.md)
- [REPL UI](repl-ui.md)
- [Zipper Heaps](zipper-heaps.md)
