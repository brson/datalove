# The Datalove Guide

Datalove is an interactive scripting language &mdash;
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

<h2><center>4 Things to Remember <br> … about Datalove</center></h2>

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
       interactive (<em>REPL</em>) interpreter with <em>JIT</em>,
       <em>compiles</em> to <em>staticly-linked</em> binaries.</p>
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
// A book record
: {
  title: string,
  author: string,
  year: i32,
  pages: u32,
  rating: f32,
  available: bool,
  genre: enum { Fiction, NonFiction, Reference },
  subtitle: ?string
} / {
  title = "Nineteen Eighty-Four",
  author = "George Orwell",
  year = 1949,
  pages = 328,
  rating = 4.7,
  available = true,
  genre = enum Fiction,
  subtitle = none
}
```

It also includes collections: lists, maps, and sets.

```datalove
// A list of ratings
: [f32] / [4.5, 4.0, 5.0, 3.5]

// ISBN to title mapping
: map<int, string> / map {
  9780451524935 = "Nineteen Eighty-Four",
  9780060850524 = "Brave New World"
}

// Authors in the collection
: set<string> / set { "Orwell", "Huxley", "Bradbury" }
```

It also includes _tables_ (dataframes / structs-of-arrays).

```datalove
// Book catalog as a table
: {| title: string, author: string, year: i32 |} / {|
  title,              author,          year
  "1984",             "Orwell",        1949
  "Brave New World",  "Huxley",        1932
  "Fahrenheit 451",   "Bradbury",      1953
|}
```

It includes optional and result types.

```datalove
: ?string / "A Novel"       // optional with value
: ?string / none            // optional without value

: !string / "Success"       // result with value
: !string / error "Failed"  // result with error
```

It includes two dynamic types:
`data`, for general dynamic typing;
and `error`, the payload for result types.

```datalove
// Heterogeneous data
: data / data "1984"
: data / data 328
: data / data true

// Error payload
: error / error "Book not found"
```




### Datalove Functions

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

```datalove
// A book from the catalog
let book: {
  title: string,
  author: string,
  year: i32,
  pages: u32,
  available: bool
} = {
  title = "Nineteen Eighty-Four",
  author = "George Orwell",
  year = 1949,
  pages = 328,
  available = true
}

// Check if a book can be checked out
fun can_checkout(avail: bool, copies: u32): bool
  if avail and copies .> 0
    ret true
  else
    ret false
  end if
end fun

// Calculate total pages with overflow checking
fun total_pages(a: u32, b: u32): !u32
  ret a +! b
end fun

// Safe average that handles division by zero
fun avg_rating(sum: int, count: int): ?int
  ret sum /? count
end fun

// Use the functions with catalog data
let checkout_ok = can_checkout(true, 3)
let dystopia_pages = total_pages(328, 311)
let rating = avg_rating(47, 10)
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
- [Datalove Worlds](worlds.md)
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
