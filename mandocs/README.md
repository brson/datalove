# The Datalove Guide

Datalove is
a simple and expressive scripting language -
strongly and statically typed -
for efficient data modeling and transformation,
with a monumental standard library.

Datalove is built around a simple idea:
first let us define a very simple but complete language
for writing, typing, serializing, and transforming a variety
of modern pure data types.
Let's do that really well.
Then we'll add I/O to it &mdash; carefully.






- [Features](features.md)
- [Design Notes](design-notes.md)
- [Novelties](novelties.md)
- [Datalit Types](datalit-types.md)
- [Datalit Runtime Types](datalit-runtime-types.md)
- [Testing Tower](testing-tower.md)
- [Influences](influences.md)






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

