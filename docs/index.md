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

Install prebuilt binaries:

```
curl -L https://up.datalove.todo | sh
```

Or install from source with [Rust](https://rustup.rs):

`cargo install datalove-cli`


## Datalove novelties for language enthusiasts

Linear types and argument modes.
No reference types, no GC.

Matched-brace tree lexing plus newline-sensitive parsing
enables an unambiguous mixture of Python, Pascal, and Rust syntactic features
(with some unfortunate tradeoffs around `<` and `>`).


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


## Key Features

- **Statically typed** with ergonomic coercions and runtime reflection
- **REPL-first design** with undo/redo and rewind/replay
- **Fully WASM-compatible** toolchain
- **Massive standard library** (aspirational)
- **Fast incremental compilation** with hot-reloading


## Quick Look

