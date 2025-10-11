# datalove - data|is·my·love|language

An expressive scripting language
for efficient data modeling
and transformation,
with a batteries included standard library.

## Features

- Statically typed with ergonomic coercions and runtime introspection.
- Fast incremental processing with in-place updates, REPL-first.
- First class option and result types.


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

Extension `.dle`.

See [`demo-data.dle`] for an example.


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

### Error handling

The result type is a language type,
and is dedicated the `!` sigil &mdash;
if you see `!` you are looking at error handling.

The error type is a dynamic type that can hold
any type (an existential type).


### Floats and total ordering

There are two flavors of equality and comparison &mdash;
one mostly by procedural and logical program code,
and one mostly used by containers.
The only difference is the treatment of floats.

- `eq` - NaN != NaN; +0.0 == -0.0
- `cmp` - NaN != NaN; +0.0 == -0.0
- `eq_unique` - All float bit patterns are distinct.
  This is likely only needed for hashing.
- `cmp_total` - Total order for floats.
  Needed for B-tree maps.
