# Datalove Principles


## Modern compiler architecture

Full memoization for efficient recompilation and compiler queries (LSPs).


## Implementation simplicity

Given that we're already committed to
some sophisticated architectural decisions,
we also need to minimize the cognitive burden
of maintaining the compiler and understand the language.


## Minimal syntax sugar

Provide the core features necessary,
avoid bloating the compiler with extra language features.


## Mechanical sympathy

Threads are simple and directly supported by the OS:
No green threads. No async/await state machines.

