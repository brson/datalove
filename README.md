# datalove - data|is·my·love|language

An simple and expressive scripting language
for efficient data modeling
and transformation,
with a batteries included standard library.


## A Tower of Love

Datalove is built from three cleanly-scoped strict
sublanguages of increasing power.


### Datalove Literals ("Datalit")

> File extension `.dlt`

The tiny and comprehensible foundation of Datalove,
a strongly-typed pure-data language
for expressing most typical data structures:

- booleans, fixed integers and bigints, floats
- anonymous and named tuples, structs, and enums
- lists and strings, maps and sets
- option and result
- `data` of any of the above, with runtime introspection and reflection
- `error` of any of the above, but for error handling
- (aspirational) datetimes, subrange ints, bitsets
- (aspirational) tables, graphs, multidimensional arrays

Datalit is a serialization format:

```datalove
; Personal info
{
  name = "Ada",
  born = 1815,
  interests = ["mathematics", "poetry", "music"],
  address_book = set {
    struct AddressEntry {
      kind = enum Friend,
      name = "Charles",
    },
    struct AddressEntry {
      kind = enum Family,
      name = "George",
    },
  },
}
```

All expressions can be type-hinted.
The syntax for this is `: <type> / <expr>`.
You'll probably get used to it.

```datalove
; This is the type of the literal we're about to write.
; When you see ":" in Datalove it is always followed by a type.
: {
  name: string,
  born: u32,
  interests: [string],
  address_book: set<
    struct AddressEntry {
      kind: enum { Friend, Family },
      name: string,
    }
  >,
} / {              ; After "/" is the literal expression.
  name = "Ada",    ; We can throw a type hint anywhere.
  born = 1815,
  interests = : [string] / [
    "mathematics", "poetry", "music",
  ],
  address_book = set {
    struct AddressEntry {
      kind = enum Friend,
      name = "Charles",
    },
    struct AddressEntry {
      kind = enum Family,
      name = "George",
    },
  },
}
```

Datalit is strongly statically typed
but supports free non-destructive coercions,
and other lightweight conversions.
All types are owned tree-shaped value-types
and do not support interior mutability, native pointers, or cycles.

The shapes of Datalit types and type descriptors are fully
specified at runtime and form the basis of the Datalove
runtime ABI.

If you understand Datalit you understand 80% of Datalove.


### Datalove Functions ("Datafun")

> File extension `.dfs` (script), `.dfm` (modules)
>
> Example [demo-datafun-script.dfs], [demo-datafun-module.dfm].

A simple functional language built on the datalit type system.
†


Datafun may be defined in modules or in scripts, as in a repl.

todo

Note that the Datalit type system has extremely nice properties that
enable: pure functions, full or partial memoization,
undo/redo, rewind/replay, prolog-style choice points, backtracing, and multi-determinism;
but these capabilities are surfaced with a familiar looking and feeling language -
functional and logic programming become powerful extensions to
less-restricted imperative programming.

If you understand Datafun you understand 90% of Datalove.


### Full-on Datalove

> File extension `.dls` (script), `.dlm` (module)
>
> Example [demo-datalove-script.dls], [demo-datalove-module.dlm].

- `proc`
- owned native pointers,
  linear, non-clonable, non-mathable,
  just tokens with provenance,
  runtime can assume these are unaliased
- `obj` - structs with object identity and encapsulation.
  These are the unit of abstraction.
  Still value types, not like C# classes.

If you understand Datalove then objective achieved.
It is a simple language.


## Features

### A massive and battle-tested standard library.



### Statically typed with ergonomic coercions and runtime reflection

Datalove is statically typed but feels flexible and fast
like a dynamic scripting language.

### Fast incremental compilation and execution with hot-reloading,
    rewind-and-replay, rewind-and-rewrite-history.

### Interpreted or compiled, JIT or AOT

### REPL-first design

### Fully WASM-compatible toolchain


## Aspirational features

### Interpreted and 

### Incremental computation, memoized predicates, choice points, and logic programming.

### Virtualized I/O for simulation and record-and-replay.




## Datalove Literals

For storage and transmission of structured and optionally typed data ala JSON.
It is a subset of the expression language in Datalove Script.

The types are all plain old data without cycles.
We call these types "pure types",
and if you understand these types you will understand most of the type system.

Also known as "the data language",
or "data expressions",
since datalove script also has expressions
with a superset syntax and types.

Extension `.dlt`.

See [`demo-data.dlt`] for an example.


## Datalove Script

The scripting language,
dynamically and incrementally interpreted or compiled.
Read-eval-print loop.
Strongly typed with lightweight coercions.
Task oriented, non-async, local heaps.
Global heap can transfer ownership of plain-old-data.
Additional pointer, GC/RC, I/O, and interior-mutable types.
Distinct functions (plain-data, comptime)
and procedures (I/O, interior mutation).
Fast incremental whole-program compilation, no eval.
Introspection and reflection.

The syntax is a strict superset of Datalove Modules.
Extension `.dls`.

See [`demo-script.dls`] for an example.

## Datalove Modules

The syntax is a strict superset of Datalove Expressions.
Extension `.dlm`.

See [`demo-module.dlm`] for an example.


## Design notes

### † Pure functions + mutable-reference argument modes

Datalove at first does not look pure functional because
it has mutable by-reference arguments:

```datalove
```

etc.


### Allocations

Besides encoding heaps in types,
having local and global heaps,
and enforcing a light syntax
on allocating conversions,
Datalove is not concerned with providing
control over the allocator;
but instead only with providing data access patterns
and metadata with which the allocator can perform optimally.

### Garbage collection

Datalit and Datafun do not require a GC.
Datalove may experiment with interior pointers and GCs in the future.

### Error handling

The result type is a language type,
and is dedicated the `!` sigil &mdash;
if you see `!` you are looking at error handling.

The error type is a dynamic type that can hold
any type (an existential type).

### Threading and concurrency

For simplicity and OS-mechanical sympathy,
Datalove is a multithreaded language;
no lightweight tasks or async-await.
We may experiment with callback-based
asynchrony and Gleam-style inline continuation syntax.

### Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.

## REPL

The script is REPL-first and intends to advance the
state of the art in REPLs.
With pure data statements and virtualized I/O the repl
supports undo/redo and record-and-replay.

The repl is a modern Ratatui application.
The datafun interpreter runs on the web
as a egui_ratatui application.

## Trailing commas and separators

Allowed and optional in all sequence forms of course.


## Influences

- Rust. general syntax, general mechanical sympathy concerns,
  data structure definitions, float total_cmp.
- Zig. `?` / `!`, aspects of anonymous structs
- JSON5. baseline markup language requirements
- Python. negative inspiration - awkward dicts,
  heavyweight class abstractions
- Polars / Pandas. tables
- TigerBeetle. DST / virtualized I/O
- Mercury. argument modes and logic applications


## Roadmap

- datalit - get this fairly polished before moving on
  - `datalove lit-tycheck` - run the type checker and report
  - `datalove lit-pretty` - pritty printer
  - `datalove lit-op` - run built-in operations
    > e.g. `datalove lit-op <expr1> eq <expr2>`
  - tables proof of concept
- datafun - the pure-data language
  - repl - this will be the primary driver of development soon
  - modules and standard library


## Wants

- logic programming features:
  - generators, choice points, memoization
- explicit linear-type destructors
