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
Do that really well.
Then add I/O to it &mdash; carefully.




<br>




> **Latest news**: [Native riders](posts.html) &middot; updated 2026-03-27.

> **Roadmap**: [18 of 21 complete](roadmap.md) &middot; updated 2026-09-16.




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
    <p>With an <em>interactive scripting environment</em>
       for <em>rapid script iteration</em>,
       with incremental typechecking and evaluation,
       and a <em>fully-reversible REPL</em>
       where undo rewinds both the typechecker and the evaluator.</p>
  </div>
  <div class="thing-box">
    <p>With a modern compilation and execution architecture with
       <em>fully memoized and parallelized</em> compilation,
       an IR-based interpreter with per-function <em>JIT</em>,
       optionally compiling to <em>statically-linked binaries</em>.</p>
  </div>
</div>




<br>

---

<br>
<div id="power">

Datalove is built from <bold>[<em>| 3 sublanguages |</em>]</bold> of increasing power:

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
  title = "The Dispossessed",
  author = "Ursula K. Le Guin",
  year = 1974,
  rating = 4.7,
  available = true,
  genres = : #{enum { atom Fiction, atom Utopia }} / #{ atom Fiction, atom Utopia },
  subtitle = some "An Ambiguous Utopia",
  status = atom InPrint,
  translations = %{ "fr" = true, "de" = true, "jp" = false },
}
```

It includes first-class _tables_ (dataframes / structs-of-arrays),
and _tensors_ (multidimensional arrays).

```datalove
: {| title: string, author: string, year: i32 |} / {|
  title,                        author,       year
  "The Dispossessed",           "Le Guin",    1974
  "The Player of Games",        "Banks",      1988
  "A Psalm for the Wild-Built", "Chambers",   2021
|}
```

```datalove
[|
  1 0 0,
  0 1 0,
  0 0 1
|]
```




### [| 2, Datalove Functions |]

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

```datalove
require module sys/std/list
import list.len

type Book: {
  title: string,
  author: string,
  year: i32,
  rating: f32,
  available: bool,
  genres: #{enum { atom Fiction, atom Utopia, atom SciFi }},
  subtitle: ?string,
  status: enum { atom InPrint, atom OutOfPrint },
  translations: %{string = bool},
}

fun reserve_book(mut db: [Book], ref title: string): !()
  var i: index = 0
  loop while i .< len(ref db)
    if db[i]!.title == title
      if db[i]!.available
        set db[i]!.available = false
        ret ok ()
      else
        ret er error atom BookNotAvailable
      end if
    end if
    set i = i +! 1
  end loop
  ret er error atom BookNotFound
end fun
```




### [| 3, Datalove with Side Effects |]

The complete language with I/O-bearing procedures,
owned native pointers, objects with identity,
and stack unwinding.
_This language is not implemented yet,
and mostly not discussed in the documentation_,
but I do have a [vision](principles.md) about what it will be.



<br>




---

<div class="toc toc-repeat-2">

- [Datalove Literals](datalit.md)
- [Datalove Functions](datafun.md)
- [Principles](principles.md)

</div>

---

<div class="toc toc-repeat-7">

- [Lexical Structure](lexer.md)
- [Datalove Types](types.md)
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
- [Concise Feature List](features.md)

</div>

---
