# Datalove Principles


## Simple and complete syntax and representation for data types




## Restrictive type systems enable better compilers




## Modern compiler and execution architecture

Full memoization for efficient recompilation and compiler queries (LSPs).
Efficient IR-based interpreter with per-function JIT, or ahead-of-time compilation.
Incremental script (REPL) typechecking and evaluation with full JIT support.
Rollback, record/replay, virtualized I/O.


## Implementation simplicity

Given that we're already committed to
some sophisticated architectural decisions,
we also need to minimize the cognitive burden
of maintaining the compiler and understanding the language.


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

  