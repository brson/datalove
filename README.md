# datalove - data|is·my·love|language

There are currently three progressively-capable sublanguages.


## Datalove Expressions

For storage and transmission of structured and optionally typed data ala JSON.
It is a subset of the expression language in Datalove Script.

The types are all plain old data without pointers.

Extension `.dle`.


## Datalove Script

The scripting language,
dynamically and incrementally interpreted or compiled.
Strongly typed with lightweight coercions.
Task oriented, non-async, local heaps.
Global heap can transfer ownership of plain-old-data.
Additional pointer, GC/RC, I/O, and interior-mutable types.
Distinct functions (plain-data, comptime)
and procedures (I/O, interior mutation).
Fast incremental whole-program compilation, no eval.

The syntax is a strict superset of Datalove Modules.
Extension `.dls`.


## Datalove Modules

The scripting language

The syntax is a strict superset of Datalove Expressions.
Extension `.dlm`.


