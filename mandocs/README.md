# The Datalove Language Guide

Datalove is an interactive scripting language
for efficient data modeling and transformation.
It is strongly and statically typed,
but with ergonomic coercions
and an efficient compiler pipeline.

Datalove is built around one core idea:
first let us define a simple but complete language
for writing, typing, serializing, and transforming a sufficient variety
of modern pure data types.
Let's do that really well.
Then we'll add I/O to it &mdash; carefully.




<br>




> **Roadmap**: [11 of 22 complete](roadmap.md) &middot; updated 2026-02-01.

> **Latest news**: [Const param specialization](posts.html) &middot; updated 2026-01-31.




<br>




<h2><center>4 Things to Remember <br> about Datalove</center></h2>

<div class="four-things-grid">
  <div class="thing-box">
    <p>Part <em>data serialization, configuration and interchange format</em>
       for common <em>modern</em> data types.</p>
  </div>
  <div class="thing-box">
    <p>Part <em>pure-functional language</em>
       that reads like an imperative language,
       with a simple but sophisticated <em>linear type system</em>,
       and a focus on <em>numerical correctness.</em></p>
  </div>
  <div class="thing-box">
    <p>todo</p>
  </div>
  <div class="thing-box">
    <p>Modern compilation and execution architecture with
       <em>fully memoized and parallelized</em> compilation,
       <em>rapid script iteration</em>,
       interactive (<em>REPL</em>) interpreter with <em>JIT</em>,
       compiles to <em>staticly-linked binaries</em>.</p>
  </div>
</div>




<br>

---

<br>
<div id="power">

Datalove is built from <bold>[<em>| 3 sublanguages |</em>]</bold> of increasing power.

</div>
<br>

---

<br>




### [| 1, Datalove Literals |]

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
  genres = [atom Fiction, atom Reality],
  subtitle = none,
}
```

It includes first-class _tables_ (dataframes / structs-of-arrays),
and _tensors_ (multidimensional arrays).

```datalove
{|
  title,              author,          year
  "1984",             "Orwell",        1949
  "Brave New World",  "Huxley",        1932
  "Fahrenheit 451",   "Bradbury",      1953
|}
```

```datalove
[|
  1 0 0,
  0 1 0,
  0 0 1,
|]
```




### [| 2, Datalove Functions |]

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

```datalove
type Book: {
  title: string,
  author: string,
  year: i32,
  rating: f32,
  available: bool,
  genres: [enum { atom Fiction, atom Reality }],
}

fun reserve_book(mut db: #{Book}, ref book: Book): !()
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




### [| 3, Datalove with Side Effects |]

The complete language with I/O-bearing procedures,
owned native pointers, objects with identity,
and stack unwinding.
_This language is not implemented yet,
and mostly not discussed in the documentation_,
but I do have a [vision](vision.md) about what it will be.



<br>




---

<div class="toc toc1">

- [Datalove Literals](datalit-types.md)
- [Datalove Functions](datafun.md)
- [Principles](principles.md)
- [Vision](vision.md)

</div>

---

<div class="toc toc2">

- [Lexical Structure](lexer.md)
- [Types, Literals, and Destructuring](types-lits-destr.md)
- [Modules, Functions and Scripts](modules-functions-scripts.md)
- [Control Flow](control-flow.md)
- [Operators](operators.md)
- [Optional and Result Types and Operations](checked-types.md)
- [Moves, Copies, References](moves-etc.md)
- [Constant Evaluation](const-eval.md)
- [Datalove Worlds](worlds.md)
- [Datalove Literals Runtime Types](datalit-runtime-types.md)
- [Novelties](novelties.md)
- [Roadmap](roadmap.md)
- [Influences](influences.md)

</div>

---

<div class="toc toc3">

- [Compiler Guide](compiler-guide.md)
- [Issues](issues.md)
- [Design Notes](design-notes.md)
- [Future Designs](future-designs.md)
- [Potential Changes](potential-changes.md)
- [Testing](testing-tower.md)
- [Features](features.md)

</div>

---

<div class="toc toc4">

- [More Datalit Types](more-datalit-types.md)
- [Total Functions](total-functions.md)
- [Panicking](panicking.md)
- [Script Semantics](script-semantics.md)
- [REPL UI](repl-ui.md)
- [Zipper Heaps](zipper-heaps.md)

</div>

---