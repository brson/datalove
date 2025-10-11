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

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.


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
