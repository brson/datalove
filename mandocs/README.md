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
// Booleans
let available: bool = true

// Fixed integers with explicit heap
let pages: u32 = 384
let year: i32 = 1984

// Arbitrary precision bigint
let library_id: int = 9780451524935

// Floats
let rating: f32 = 4.5

// Strings
let title: string = "Nineteen Eighty-Four"
let author: string = "George Orwell"

// Anonymous tuple - a (title, year) pair
let published: (string, i32) = ("Brave New World", 1932)

// Anonymous struct - a book record
let book: { title: string, author: string, pages: u32 } = {
  title = "Fahrenheit 451",
  author = "Ray Bradbury",
  pages = 158
}

// Anonymous enum - genre classification
let genre: enum { Fiction, NonFiction, Reference } = enum Fiction

// List of ratings
let reviews: [f32] = [4.5, 4.0, 5.0, 3.5]

// Map from ISBN to title
let catalog: map<int, string> = map {
  9780451524935 = "Nineteen Eighty-Four",
  9780060850524 = "Brave New World"
}

// Set of authors in the collection
let authors: set<string> = set { "Orwell", "Huxley", "Bradbury" }
```




It also includes _tables_ (dataframes / structs-of-arrays).

```datalove
// Book catalog as a table (columnar layout)
: {|
  title: string,
  author: string,
  year: i32
|} / {|
  title,              author,          year
  "1984",             "Orwell",        1949
  "Brave New World",  "Huxley",        1932
  "Fahrenheit 451",   "Bradbury",      1953
|}
```




It also includes optional and result types.

```datalove
// Optional subtitle - some books have one, some don't
let subtitle: ?string = none
let with_sub: ?string = "A Novel"

// Result type - checking out might fail
let checkout: !string = "Checked out successfully"
let failed: !string = error "Book not available"
```

It includes two dynamic types:
`data`, for general dynamic typing;
and `error`, the payload for result types.

```datalove
// Dynamic data - heterogeneous catalog entries
let entry1: data = data "1984"
let entry2: data = data 384
let entry3: data = data true

// Typed data payload
let typed_entry: data = data : { title: string, pages: u32 } / {
  title = "1984",
  pages = 384
}

// Error values carry diagnostic information
let err: error = error "Book not found in catalog"
```




### Datalove Functions

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

```datalove
// Calculate total page count with checked arithmetic
fun total_pages(a: u32, b: u32, c: u32): !u32
  let sum = a +! b
  ret sum +! c
end fun

// Look up a book's availability
fun is_available(copies: u32): bool
  if copies .> 0
    ret true
  else
    ret false
  end if
end fun

// Safe division for computing averages
fun average_rating(total: int, count: int): ?int
  ret total /? count
end fun

// Using the functions
let dystopian_pages = total_pages(328, 311, 158)
let can_checkout = is_available(3)
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
