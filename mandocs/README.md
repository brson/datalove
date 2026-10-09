# The Datalove Language Guide

Datalove is an interactive scripting language
for efficient data modeling and transformation.
It is strongly and statically typed
and has an efficient compiler pipeline.

Datalove is built around one core idea:
first let us define a simple but complete language
for writing, typing, serializing, and transforming a sufficient variety
of modern data types.
Do that really well.
Then add I/O to it &mdash; carefully.




<br>




> **Latest news**: [Compound assignment](posts/) &middot; updated 2026-10-09.

> **Roadmap**: [18 of 19 complete](roadmap.md) &middot; updated 2026-10-02.




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
       for rapid script iteration,
       with <em>incremental recompilation</em> and <em>incremental reevaluation</em>.
  </div>
  <div class="thing-box">
    <p>With a modern compilation and execution architecture with
       <em>fully memoized and parallelized</em> compilation,
       a bytecode interpreter with per-function <em>JIT</em>,
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
  author = { name = "Ursula K. Le Guin", born = 1929 },
  published = 1974,
  rating = 4.7,
  subtitle = some "An Ambiguous Utopia",
  tags = #{ "utopia", "anarchism", "physics" },
  status = atom InPrint,
  translations = %{
    "fr" = "Les Dépossédés",
    "de" = "Planet der Habenichtse"
  },
  on_loan = false,
}
```

It includes first-class _tables_ (dataframes),
and _tensors_ (multidimensional arrays).

```datalove
{|
  title,                        author,      published
  "The Dispossessed",           "Le Guin",   1974
  "The Player of Games",        "Banks",     1988
  "A Psalm for the Wild-Built", "Chambers",  2021
|}
```

```datalove
// Loans per month at each branch, January to April.
[|
  12  15   9  22,
   8   7  14  11,
  30  28  31  26
|]
```




### [| 2, Datalove Functions |]

A simple pure-functional language that feels like an imperative language,
built on the Datalove Literals type system.
Strongly and statically typed, structural and linear,
it includes constants with full compile-time function evaluation,
a simple acyclic module system, and reactive script units that incrementally
recompile and reevaluate when changed.

```datalove
require module sys/std/list
require module sys/std/u32

type Book: {
  title: string,
  author: string,
  price_cents: u32,
  on_loan: bool,
}

// Lend a book out, or say why it can't be.
fun lend(mut shelf: [Book], ref title: string): !()
  var i: index = 0
  loop while i .< list.len(ref shelf)
    if shelf[i]!.title == title
      if shelf[i]!.on_loan
        ret er error "already on loan"
      end if
      set shelf[i]!.on_loan = true
      ret ok ()
    end if
    set i = i +! 1
  end loop
  ret er error "not in the catalog"
end fun

// A quarter a day, but never more than the book is worth.
fun fine_cents(days_late: u32, price_cents: u32): u32
  ret u32.min(u32.mul_saturating(days_late, 25), price_cents)
end fun

var shelf: [Book] = [
  {
    title = "The Dispossessed",
    author = "Ursula K. Le Guin",
    price_cents = 1599,
    on_loan = false,
  },
  {
    title = "The Player of Games",
    author = "Iain M. Banks",
    price_cents = 1299,
    on_loan = true,
  },
]

debuglog lend(mut shelf, ref "The Dispossessed")
debuglog lend(mut shelf, ref "The Player of Games")
debuglog lend(mut shelf, ref "Dune")

debuglog (fine_cents(3, 1599), fine_cents(400, 1599))
```




### [| 3, Datalove with Side Effects |]

The complete language with I/O-bearing procedures,
owned native pointers, objects with identity,
and stack unwinding.
_This language is not implemented yet,
and mostly not discussed in the documentation_,
but I do have a vision about what it will be.



<br>




---

<div class="toc toc-repeat-2">

- [Datalove Literals](datalit.md)
- [Datalove Functions](datafun.md)
- [Principles](principles.md)

</div>

---

<!--
<div class="toc toc-repeat-2">

- [Lexical structure](lexer.md)
- [Types](types.md)
- [Control Flow](control-flow.md)
- [Operators](operators.md)
- Optional and result types
- Error handling
- Constant evaluation
- [Runtime type representation](runtime-types.md)

- [Types, literals, and destructuring](types-lits-destr.md)
- [Novelties](novelties.md)
- [Concise feature list](features.md)

</div>

---
-->
