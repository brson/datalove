## The Datalove Decree

Datalove on various language design topics.




### 2026-01-20 - Comparative benchmarks

The `benchvs` directory contains a benchmark
suite that compares against other scripting languages:
Python and Julia.

It contains a few single-file scripts and a justfile,
with three common numerical benchmarks in each language.

It uses datalove via its CLI `script` command.
We are concerned with script startup time and execution time.

Run `cd benchvs && just run` to run and report.




### 2026-01-08 - Field projections

Datalove structs and tuples support
reading and writing of fields.

```datalove
let a: { x: u32, y: u32 } = { x = 1, y = 2 }
let b = a.x   // this is a copy
let c = a.y   // another copy

let a: (u32, u32) = (1, 2)
let b = a.0
```

Field projections are allowed in `set` statements:

```datalove
var a: { x: u32, y: u32 } = { x = 1, y = 2 }
set a.x = 3

var a: (u32, u32) = (1, 2)
set a.0 = 3 // integer indexes for tuple fields
```

`set` on move-type fields drops the prior value first.

Through `?` and `!`:

```datalove
let a: ?(u32, u32) = some (1, 2)
let b = a?.0

let a: !(u32, u32) = ok (1, 2)
let b = a!.0
```

Copy-types copy their projections;
move-type projections are disallowed except in reference destinations
or `set` statements.
The kind of projection depends on the destination:
if the destination is a `ref`, `mut`, or `out` params,
or operator operands,
then the projections become ref projections,
the aggregate remains fully constructed after
the call.

```datalove
fun foo(ref b: int): int
  ret b + : int / 0
end fun

let a: (int, int) = (1, 2)
debuglog foo(a.0) // ref-destination projections are ok for move types
```

Nested projections are allowed:

```datalove
var a: { x: (u32, u32), y: u32 } = { x = (1, 2), y = 3 }
set a.x.0 = 3
let b = a.x.0
```









### Allocations

Besides encoding heaps in types,
having local and global heaps,
and enforcing a light syntax
on allocating conversions,
Datalove is not concerned with providing
control over the allocator;
but instead only with providing data access patterns
and metadata with which the allocator can perform optimally.




### REPL

The script is REPL-first and intends to advance the
state of the art in REPLs.
With pure data statements and virtualized I/O the repl
supports undo/redo and record-and-replay.

The repl is a modern Ratatui application.
The datafun interpreter runs on the web
as a egui_ratatui application.




### Trailing commas and separators

Allowed and optional in all sequence forms of course.




### Generics

Functions can have type parameters:

```datalove
fun unwrap_or<T>(self: ?T, default: T): T where {
  T is move,
}
  if self |value|
    ret value
  else
    ret default
  end if
end fun
```

By default type parameters have no capabilities,
`clone`, `move`, `etc`. `where` clauses are
usually needed.

Data structures can not have type parameters.

TODO: How are generics translated?




### Type aliases

Datafun has a structural type system.
We can asign names with `type`.

```datalove
// `Contact` is an alias for a struct.
type Contact: {
  name: string,
  age: int,
}
```

As always, `:` precedes a type definition.
After this `Contact` is an alias for the same structural type -
it is not a new type.

Type aliases can be imported:

```datalove
require local/test/foo

import foo.Contact

fun get_age(ref contact: Contact): int
  ret contact.age
end fun
```

Imported aliases are not re-exported
(nor are imported functions).

Type aliases do not accept type parameters,
but you can alias an instantiation of types with parameters.

```datalove
type myset: set<int>
```

Type aliases may not refer to themselves;
they are not self-recursive.

Type aliases are heap-agnostic,
the heap is applied when the alias is named:

```datalove
type number: int

let foo: @number = 1
```

Type aliases are valid at the module and script level,
not within functions.

Name resolution between type aliases is unidirectional both in scripts and modules.
We do this for efficiency, even though we allow functions to be mutually recursive.

Functions can resolve type aliases in both directions.
This probably implies two passes: one single-direction pass
that resolves everything not a function;
then one for functions.

Valid:

```datalove
type number: int
type mynumber: number
```

Invalid:

```datalove
type mynumber: number // Can't resolve forward reference
type number: int
```

Shadowing primitives or other in-scope aliases is not allowed.

In type hints and type annotations,
aliases are resolved during name resolution,
after parsing,
and either before or part of typechecking,
whatever fits the current model.


