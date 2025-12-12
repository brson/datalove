# The Datalove Guide

Datalove is
a simple and expressive scripting language -
strongly and statically typed -
for efficient data modeling and transformation,
with a monumental standard library.


## Do not contribute; do not use

This project is not open to contribution.
Issues and pull requests will be closed without consideration.
Do not use this project.
It is neither stable nor supported.


## Installation

Install from source with [Rust](https://rustup.rs)

```
cargo install datalove-cli
```

Run the REPL with

````
datalove repl
```


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

