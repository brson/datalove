# Datalove Principles


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


## Mechanical sympathy

Inline values, no implicit boxing.
Threads are simple and directly supported by the OS:
No green threads. No async/await state machines.


## Numerical correctness

Overflow must be handled,
early-return checked operations and widening to ease the burden.



