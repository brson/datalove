# Datalove Principles




## Glorify the plain old data types

Good data design leads to good algorithms,
and most data types should be plain old data,
not objects.


## Advance interactive scripting design




## Modern compiler and execution architecture

Full memoization for efficient recompilation and compiler queries (LSPs).
Efficient IR-based interpreter with per-function JIT, or ahead-of-time compilation.
Incremental script (REPL) typechecking and evaluation with full JIT support,
undo/redo, virtualized I/O with record/replay.

We establish broad architecture-level capabilities early
to inform architectural and design decisions.




## Minimal compiler passes, simple analysis

The type system is strong but simple and restrictive.
We want to have the startup speed of dynamic scripting languages,
and must be ruthless about removing features
until some baseline performance has been established.
A simple implementation makes maintenance easier,
enables quick development.




## No first-class references

Datalove has a linear type system
but it does not have first-class reference types.
Borrowing is difficult to reason about.

We'll instead push other techniques as far as we can,
argument modes and other reference bindings,
Gleam-style `use` expressions,
making some cloning easy and idiomatic.




## Minimal syntax sugar

Provide the core features necessary,
avoid bloating the compiler with extra language features.
Syntactic niceties added with careful deliberation.




## Mechanical and machine-model sympathy

Inline values, no implicit boxing.
Threads are simple and directly supported by the OS:
No green threads. No async/await state machines.

Syntax lowers trivially to SSA-based IRs
to minimize analysis-based reconstruction.




## Numerical correctness

Overflow must be handled,
early-return checked operations and widening to ease the burden.




## Sigil-logic

Datalove reserves some symbols to strongly mean one thing.

| When you see | it means                  |
|--------------|---------------------------|
| `:`          | type                      |
| `?`          | option                    |
| `!`          | result                    |
| `@`          | adapt                     |
| `;`          | statement break / newline |

Likewise some bracket pairs are for one thing.

| When you see | it means |
|--------------|----------|
| `{|` … `|}`  | table    |
| `[|` … `|]`  | tensor   |




---

2026/01/06 values sketch

consistent and understandable data syntax, representation, and transformation
machine sympathy / codegen sympathy
type-system simplicity / compilation simplicity
restrictive type systems to enable advanced compiler featurues
statement-oriented readability
memoization, determinism, reprodicibility
linear types
numerical correctness
interactive execution / repl
rapid iteration / fast startup
prototyping and scripting
declarative / imperative, code as data - datalit, token trees
flexible execution modes - repl, script, aot, webcomponents, pipelines

  