# datalove - data|is·my·love|language

An expressive scripting language
for efficient data modeling
and transformation,
with a batteries included standard library.

## Features

- Statically typed with ergonomic coercions and runtime introspection.
- Fast incremental processing with in-place updates, REPL-first.
- First class option and result types.


## Datalove Expressions

For storage and transmission of structured and optionally typed data ala JSON.
It is a subset of the expression language in Datalove Script.

The types are all plain old data without cycles.

Also known as "the data language",
or "data expressions",
since datalove script also has expressions
with a superset syntax and types.

Extension `.dle`.


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


## Datalove Modules

The syntax is a strict superset of Datalove Expressions.
Extension `.dlm`.


