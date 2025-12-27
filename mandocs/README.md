# The Datalove Guide

Datalove is
a simple and expressive scripting language -
strongly and statically typed -
for efficient data modeling and transformation.

Datalove is built around one core idea:
first let us define a simple but complete language
for writing, typing, serializing, and transforming a variety
of modern pure data types.
Let's do that really well.
Then we'll add I/O to it &mdash; carefully.




---

Datalove is built from three cleanly-scoped strict sublanguages of increasing power:

### Datalove Literals ("Datalit")

The tiny and comprehensible foundation of Datalove, a strongly-typed and
declarative pure-data language for expressing typical data structures.

File extension: `.dlt`

### Datalove Functions ("Datafun")

A simple pure-functional language that feels like an imperative language, built
on the datalit type system.

File extensions: `.dfs` (scripts), `.dfm` (modules)

### Datalove

The complete language with I/O-bearing procedures,
owned native pointers, and objects with identity.

Filue extensions: `.dls` (scripts), `.dlm` (module)

---




## The state of Datalove

Datalove is a work in progress.
This documentation describes Datalove as currently implemented.

Current status:

- Datalit: implemented.
- Datafun modules: implemented.
- Datafun scripts: implemented.
- Datafun interpreter: implemented, analysis-driven. No JIT.

---




- [Features](features.md)
- [Design Notes](design-notes.md)
- [Novelties](novelties.md)
- [Datalit Types](datalit-types.md)
- [Datalit Runtime Types](datalit-runtime-types.md)
- [Testing Tower](testing-tower.md)
- [Influences](influences.md)

