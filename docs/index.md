# The Datalove Guide

Datalove is
a simple and expressive scripting language -
strongly and statically typed -
for efficient data modeling and transformation,
with a monumental standard library.

Datalove is built around a simple idea:
first let us define a very simple but complete language
for writing, typing, and transforming a comprehensive
set of modern pure data types.
Let's do that really well.
Then we'll add I/O to it &mdash; carefully.







## Datalove novelties for language enthusiasts

Matched-brace / token-tree lexing:
all braces are matched (`{ }`, `( )`, `[ ]`, `< >`).
Statement oriented, with statements that span multiple lines without statement terminators.
Pascal, Python, and Rust-influenced syntax.

Linear types and argument modes.
No reference types, no GC.
Ergonomic coercions.

Structural and nominal typing.
First-class option and result types (`?T`, `!T`).

Pure functions `fun` and I/O procedures (`proc`).

Fully-incremental compilation.
Whole-world compilation, all modules and other inputs must be known upfront.
Incremental script execution.
Interpreter, JIT and AOT.






## What is Datalove?

Datalove is built from three cleanly-scoped strict sublanguages of increasing power:

### [Datalove Literals (Datalit)](reference/datalit/index.md)

The tiny and comprehensible foundation of Datalove, a strongly-typed and
declarative pure-data language for expressing typical data structures. File
extension: `.dlt`

### [Datalove Functions (Datafun)](reference/datafun/index.md)

A simple pure-functional language that feels like an imperative language, built
on the datalit type system. File extensions: `.dfs` (script), `.dfm` (modules)

### [Full-on Datalove](reference/datalove/index.md)

The complete language with procedures, owned native pointers, and objects. File
extensions: `.dls` (script), `.dlm` (module)

