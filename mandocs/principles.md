# Datalove Principles

Datalove is a strongly-typed scripting language
for data modeling and transformation.

<!--
What's wrong with scripting languages?
I started datalove because expressing a struct in Python sucks.
-->

Datalove is built on a small and restrictive core language.
Dependencies are explicit,
functions are pure, values are uniquely owned,
and nothing happens that was not written.
Those restrictions make modern features easy:
fast compilation, compile-time evaluation,
incremental recompilation,
and scripts whose history can be edited, rolled back and deterministically replayed.


## The Principles

<div class="toc toc-repeat-4">

- [Glorify the plain old data types](#user-content-glorify-the-plain-old-data-types)
- [Power through restricted language design](#user-content-power-through-restricted-language-design)
- [Nothing happens that was not written](#user-content-nothing-happens-that-was-not-written)
- [Numerical correctness](#user-content-numerical-correctness)
- [Strict baseline first; controlled ergonomics later](#user-content-strict-baseline-first-controlled-ergonomics-later)
- [Small enough to understand and remember](#user-content-small-enough-to-understand-and-remember)
- [Fast compilation over fast execution](#user-content-fast-compilation-over-fast-execution)

</div>


## Glorify the plain old data types

Good data design leads to good algorithms,
and most data should be plain old data, not objects.
Datalove designs its data structures first,
their capabilities, memory layout and literal syntax,
and derives everything else from them.
The declarative language of Datalove literals
is distinct from the imperative languages built on top of it,
and understanding that core data model comes before understanding Datalove.


## Power through restricted language design

Datalove functions are pure,
with no side effects and no exceptional control flow.
Values are uniquely owned,
with no shared mutable state.
These restrictions give up flexibility,
and in exchange Datalove can do things
that are impractical in other languages.

Any function can run at compile time,
so constants and const parameters are computed
with the same code that runs at runtime.
Loops and integer arithmetic are constrained enough
that many functions can be proven to terminate (in theory...),
so compile-time evaluation is known to finish.

Any statement in a script can be edited and reevaluated,
and its history rolled back and replayed,
because nothing it did escaped into hidden state.
Procedures, when they arrive, extend this to I/O
by running it through a virtualized layer
that can be recorded and replayed.
That is the environment Datalove is for:
past REPLs and notebooks,
where a data experiment can grow into a production pipeline
without being rewritten.

The same restrictions keep the compiler simple.
It needs little analysis to recover performance,
and users see that simplicity as fast compilation
and predictable performance.


## Nothing happens that was not written

The surface syntax translates directly to what happens at execution,
and memory layout is obvious from the types.
Types are never implicitly coerced.
Values are stored inline, never implicitly boxed.
Argument passing modes are explicit.
Error handling and early returns always appear in the syntax.
What you see is what runs.


## Numerical correctness

It is no longer acceptable to silently overflow and get wrong results.

Numbers are never implicitly widened or truncated.
Fixed-size integers have no bare arithmetic.
If you aren't going think hard about your integer math,
use bigints and don't sweat it.
If you do fixed-sized integer math,
we're going to make you confront those overflows directly in the syntax
with checked operations.
(also you can wrap or saturate, but you better think doubly-hard).


## Strict baseline first; controlled ergonomics later

All this explicitness makes Datalove verbose, and intentionally so.
The syntax is a throwback to the Pascal family and Visual Basic:
line-oriented, with keywords everywhere,
easy for human eyes to scan and for computers to parse,
and without syntactic niceties that bloat the compiler and spec.

The baseline language of today is spartan and strict.
That is deliberate:
the semantics and architecture come first,
because they constrain everything built on top of them.
The surface language for casual interactivity will be more ergonomic,
and we will get there experimentally,
with modes that remove boilerplate
and translate mechanically to and from the strict baseline.


## Small enough to understand and remember

Datalove is a small language,
its features driven directly by my personal needs.
It doesn't need to be a general-purpose language.
It doesn't need to please anybody.
That is the freedom to ruthlessly omit features.
Large languages are hard to understand, maintain and extend,
and they are slow to compile.


## Fast compilation over fast execution

Datalove is foremost a scripting language.
Its data and execution model are designed for mechanical sympathy,
which leads to both fast compile times and fast execution times,
but when the two conflict, compile times win.
Datalove does not seek ultimate performance.
