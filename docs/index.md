# Datalove Documentation

**data|is·my·love|language**

An simple and expressive typed scripting language for efficient data modeling and transformation, with a batteries included standard library.

## Quick Links

- [Getting Started](getting-started/index.md)
- [Language Reference](reference/index.md)
- [CLI Reference](cli/index.md)
- [Design Philosophy](design/index.md)

## What is Datalove?

Datalove is built from three cleanly-scoped strict sublanguages of increasing power:

### [Datalove Literals (Datalit)](reference/datalit/index.md)

The tiny and comprehensible foundation of Datalove, a strongly-typed and declarative pure-data language for expressing typical data structures. File extension: `.dlt`

### [Datalove Functions (Datafun)](reference/datafun/index.md)

A simple pure-functional language that feels like an imperative language, built on the datalit type system. File extensions: `.dfs` (script), `.dfm` (modules)

### [Full-on Datalove](reference/datalove/index.md)

The complete language with procedures, owned native pointers, and objects. File extensions: `.dls` (script), `.dlm` (module)

## Key Features

- **Statically typed** with ergonomic coercions and runtime reflection
- **REPL-first design** with undo/redo and rewind/replay
- **Fully WASM-compatible** toolchain
- **Massive standard library** (aspirational)
- **Fast incremental compilation** with hot-reloading

## Learn More

- [Installation Guide](getting-started/installation.md)
- [First Steps Tutorial](getting-started/first-steps.md)
- [Type System Guide](guides/type-system.md)
- [Design Philosophy](design/philosophy.md)
