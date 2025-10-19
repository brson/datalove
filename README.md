# Datalove - data|is·my·love|language

An simple and expressive typed scripting language
for efficient data modeling and transformation,
with a batteries included standard library.




## A Tower of Love

Datalove is built from three cleanly-scoped strict
sublanguages of increasing power.




### Datalove Literals ("Datalit")

> File extension `.dlt`

The tiny and comprehensible foundation of Datalove,
a strongly-typed and declarative pure-data language
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
// Personal info
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
// This is the type of the literal we're about to write.
// When you see ":" in Datalove it is always followed by a type.
: {
  name: string,
  born: u32,
  interests: [string],
  address_book: set<struct AddressEntry {
    kind: enum { Friend, Family },
    name: string,
  }>,
} / {                      // After "/" is the literal expression.
  name = "Ada",    
  born = 1815,
  interests = : [string] / [           // Here's another type hint.
    "mathematics", "poetry", "music",
  ],
  address_book = set {
    struct AddressEntry {
      kind = enum Friend,
      name = : string / "Charles",    // And another!
    },
    struct AddressEntry {
      kind = enum Family,
      name = "George",
    },
  },
}
```

Datalit is strongly statically typed,
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

A simple pure-functional language that feels like an imperative language,
built on the datalit type system.
†

```datalove
fun increment(
  accum: int, amount: u8,
): int
  ret accum + amount
end fun
```

That's with bigints. Here's the one with fixed ints,
handling that pesky overflow:

```datalove
fun increment(
  accum: u64, amount: u8,
): ?u64
  ret accum +? amount
end fun
```

A `fun` is a pure total function on Datalit types, can't panic.
It is one of the few types Datafun adds over Datalit.

todo

The Datalit type system has extremely nice properties that
enable: pure functions, total comptime evaluation, full or partial memoization,
undo/redo, rewind/replay, prolog-style choice points, backtracing, and multi-determinism;
but these capabilities are surfaced with a familiar looking and feeling language -
comptime, functional and logic programming become powerful extensions to
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



## Novelties

- brace-tree and newline-sensitivity
- reactive repl
- linear types with explicit destructors
- undo/redo, rewind/replay
- virtualized I/O
- zipper heaps
- multi-determinism, choice-points, and logic programming



## Design notes


### Option and result handling

The option and result types are prefixes like zig:

```
// option
let a: ?u32 = ...
let b: ?[u32] = ...
// result
let c: !u32 = ...
let d: ![u32] = ...
```

Option and result construction:

```
// "some" values are automatically coerced
let a: ?u32 = 3
let b: ?u32 = u32.min_value()
// "none" checks to any option type
let c: ?u32 = none

// "ok" values are automatically coerced
let a: !u32 = 3
let b: !u32 = u32.min_value()
// "error" checks to any result type
let c: ?u32 = error "oops"
```


Destructuring option and result is like zig:

```
let a: ?u8 = 1

var c: u8 = 0
if a |value|
  c = value
else
  c = 255
end if
```

```
let a: !u8 = 1

var c: u8 = 0
if a |value|
  c = value
else |error|
  ret error
end if
```

Note that branching on error values is not like zig
and uses reflection, not shown here.

Early return with postfix `?` and `!`:

```
fun transform_option(val: ?u32): ?u32
  let val = val? // early option return
  ret val +? 1   // early option return on overflow
end

fun transform_result(val: !u32): !u32
  let val = val! // early result return
  ret val +! 1   // early result return on overflow
end
```

### Math ops and error handling

Floats support bare math ops,
`+ - * /` and unary `-`.

Bigints (int) support all but div:
`| - *` and unary `-`.
For div, because of div0 we must use a checked variant.

Fixed ints do not support any bare bath ops, not even unary `-`.

Fixed ints support early-return varieties:

```
let a = 1 +? 1
let a = 1 -? 1
let a = 1 *? 1
let a = 1 /? 1
let a = -?a // yup, early-return negation

let a = 1 +! 1
let a = 1 -! 1
let a = 1 *! 1
let a = 1 /! 1
let a = -!a // yup
```

`-?` unary op is not defined for unsigned ints -
it has a sensible semantic but is a pure footgun.

Bigints support the early-return division but not the others.
Floats don't support early-return math.

If we decide to let funs panic we'll also add panicking variations.


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
