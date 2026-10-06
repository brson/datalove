# Datalove Principles

Datalove is a strongly-typed scripting language
for data modeling and transformation,
built on a comprehensive definition of the data types
needed for modern computing tasks.

With Datalove I aim to reinspire the
programming feel I had growing up in the 90s
when the stack was small and comprehensible,
and we built native applications that were fast
because they worked closely with the physical machine.

But I aim to do this with the benefit of decades
of experience and evolution in compiler architecture and type theory:
the best of modern practical programming language theory in a simple package,
with a focus on correctness uncommon in scripting languages.


## Contents

<div class="toc toc-repeat-7">

- [Glorify the plain old data types](#user-content-glorify-the-plain-old-data-types)
- [Advance interactive scripting design](#user-content-advance-interactive-scripting-design)
- [Determinism and reproducibility](#user-content-determinism-and-reproducibility)
- [Nothing happens that was not written](#user-content-nothing-happens-that-was-not-written)
- [Numerical correctness](#user-content-numerical-correctness)
- [Power through restricted language design](#user-content-power-through-restricted-language-design)
- [Straightforward syntax with minimal sugar](#user-content-straightforward-syntax-with-minimal-sugar)
- [Strict baseline first; controlled ergonomics later](#user-content-strict-baseline-first-controlled-ergonomics-later)
- [Sigil-logic](#user-content-sigil-logic)
- [Small enough to understand and remember](#user-content-small-enough-to-understand-and-remember)
- [Mechanical and machine-model sympathy](#user-content-mechanical-and-machine-model-sympathy)
- [Modern compiler and execution architecture](#user-content-modern-compiler-and-execution-architecture)
- [Fast compilation over fast execution](#user-content-fast-compilation-over-fast-execution)

</div>



<!-- What Datalove is for -->

## Glorify the plain old data types

Good data design leads to good algorithms,
and most data types should be plain old data,
not objects. In the design of Datalove,
consideration for the capabilities,
memory layout and syntax of data structures comes first,
and all else derives from that.
We maintain a strong distinction between the declarative
language model of Datalove literals and the imperative/procedural languages built on top of it:
understanding Datalove literals precedes understanding Datalove.


## Advance interactive scripting design

Datalove applies state-of-the-art compiler design
to enable a modern interactive scripting experience with capabilities
beyond standard REPLs and notebooks, suitable for rapid
prototyping of data experiments that evolve into maintainable production data transformation pipelines.
The scripting environment features fast startup and compilation
and supports incremental recompilation and side-effect-free reevaluation as prior
statements are modified.



<!-- What Datalove promises -->

## Determinism and reproducibility

Datalove functions are side-effect free, pure and often total.
Datalove procedures use virtualized I/O.
We leverage language properties to create an execution environment
where both computation and I/O can be rolled back and selectively
replayed, which is particularly useful in the scripting environment to edit
and retry the history.


## Nothing happens that was not written

The surface syntax of Datalove translates directly to what happens
at the execution layer, and all is stated explicitly.
Integers are never automatically truncated or widened.
Types are never automatically coerced.
Argument passing modes are explicit.
All error handling and early returns are expressed consistently in the syntax.
What you see is always exactly what you get.
Datalove is verbose, and intentionally so.


## Numerical correctness

It is no longer acceptable to silently overflow and get wrong results.

Integer types never lose information.
Fixed-size integer types never overflow silently.
Integer division by zero must be handled.
The syntactic and cognitive overhead this necessarily imposes is eased
by language features to whatever extent is reasonable.


## Power through restricted language design

Datalove has a strong linear type system.
Datalove functions are pure and nearly total,
with no side effects and no exceptional control flow,
and have other restrictive properties.
Strong restrictions lead to simple
compiler analysis where the code you see lowers
directly to the code you expect,
requiring little reconstruction to regain performance.

Datalove has a linear type system,
but it does not have first-class reference types.
Safe first-class references without GC are the most
difficult feature to reason about in Rust, and they add great language and compiler complexity.




<!-- How Datalove reads -->

## Straightforward syntax with minimal sugar

Datalove's syntax is a throwback to the Pascal family and Visual Basic,
line-oriented, with keywords everywhere that are easy
for human eyes to scan and computers to parse.
It is happily verbose.
It provides the basic features necessary to write algorithms,
but does not bloat the compiler and spec with syntactic niceties.


## Strict baseline first; controlled ergonomics later

The baseline Datalove language of today is spartan and strict,
likely too verbose to be practical as the quick-prototyping interactive
language it is intended to be.
This is an intentional development strategy to develop
the language semantics and architectural capabilities necessary
to build upon. The ultimate surface language for casual
interactivity is expected to be more ergonomic,
and we will get there by incrementally and experimentally developing
ergonomic modes that reduce boilerplate and transformations between
which can be mechanically analyzed and applied.


## Sigil-logic

Datalove reserves some symbols to strongly mean one thing.

| When you see | it means                       |
|--------------|--------------------------------|
| `:`          | type                           |
| `=`          | value binding                  |
| `?`          | option                         |
| `!`          | result                         |
| `@`          | adapt (clone / widen / coerce) |
| `;`          | statement break / newline      |

Likewise some bracket pairs are strongly suggestive.

| When you see | it means                        |
|--------------|---------------------------------|
| `<` … `>`    | generics                        |
| `{` … `}`    | struct-like, structs and tables |
| `[` … `]`    | array-like, lists and tensors   |


## Small enough to understand and remember

Datalove is a small language,
its features driven directly by my personal needs.
It doesn't need to be a general-purpose language.
It doesn't need to please anybody.
This affords us the freedom to ruthlessly omit features.
Large languages are difficult to understand, difficult to maintain,
difficult to extend, and are slow to compile.


<!-- How Datalove is built -->

## Mechanical and machine-model sympathy

Datalove aims for an execution model and memory layout that
is obvious from the syntax, and one that can be executed
efficiently by modern computers without complex compiler transformations
that exhibit performance characteristics that are difficult for
human authors to reason about.

Inline values, no implicit boxing.
Plain threads. No green threads. No async/await state machines.


## Modern compiler and execution architecture

Full memoization for efficient recompilation and compiler queries.
Efficient IR-based interpreter with per-function JIT, or ahead-of-time compilation,
via Cranelift or to C source code.
Incremental typechecking and evaluation with full JIT support,
undo/redo, virtualized I/O with record/replay.

We establish broad architecture-level capabilities early
to inform and restrict the design trajectory of the language.


## Fast compilation over fast execution

Datalove is foremost a scripting language,
and even though it is statically typed and the data and execution model
are oriented toward mechanical sympathy,
tradeoffs are gladly made in service of fast compilation times.
Datalove does not seek ultimate performance.


