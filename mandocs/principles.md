# Datalove Principles




## Glorify the plain old data types

Good data design leads to good algorithms,
and most data types should be plain old data,
not objects. In the design of Datalove
consideration for the capabilities,
memory layout and syntax of data structures comes first,
and all else derives from that.




## Advance interactive scripting design

Datalove applies state of the art compiler design
to enable a modern interactive scripting experience with capabilities
beyond standard REPLs and notebooks, suitable for rapid
prototyping of data experiments that evolve into maintainable production data transformation pipelines.
The scripting environment features fast startup and compilation
and supports incremental recompilation and side-effect-free reevaluation as prior
statements are modified.




## Modern compiler and execution architecture

Full memoization for efficient recompilation and compiler queries.
Efficient IR-based interpreter with per-function JIT, or ahead-of-time compilation,
via Cranelift or to C source code.
Incremental script (REPL) typechecking and evaluation with full JIT support,
undo/redo, virtualized I/O with record/replay.

We establish broad architecture-level capabilities early
to inform and restrict the design trajectory of the language.




## Minimal compiler passes, simple analysis

The type system is strong but simple and restrictive.
We want to have the startup speed of dynamic scripting languages,
and must be ruthless about limiting features
to establish a performance baseline.
A simple implementation makes maintenance easier,
enables quick development.




## Straightforward syntax with minimal sugar

Datalove's syntax is a throwback to the Pascal family and Visual Basic,
line oriented, with keywords everywhere that are easy
for human eyes to scan and computers to parse.
It is happily verbose.
It provides the basic features necessary to write algorithms,
but does not bloat the compiler and spec with syntactic nicities.




## No first-class references

Datalove has a linear type system
but it does not have first-class reference types.
Borrowing is difficult to reason about.

We'll instead push other techniques as far as we can,
argument modes and other reference bindings,
Gleam-style `use` expressions for ergonomic continuations,
making some cloning easy and idiomatic,
perhaps borrow some ideas from Dada.




## Mechanical and machine-model sympathy

Datalove aims for an execution model and memory layout that
is obvious from the syntax, and one that can be executed
efficiently by modern computers without complex compiler transformations
that exhibit performance characteristics that are difficult for
human authors to reason about.

Inline values, no implicit boxing.
Plain threads. No green threads. No async/await state machines.

Syntax lowers trivially to SSA-based IRs
to minimize analysis-based reconstruction during codegen
and support human understanding of the performance model.




## Balance between the tradeoffs of fast compilation and fast execution

Datalove is foremost a scripting language,
and even though it is statically typed and the data and execution model
are oriented toward mechanical sympathy,
tradeoffs are gladly made in service of fast compilation times.
Datalove does not seek ultimate performance.




## Numerical correctness

It is no longer acceptable to silently overflow and get wrong results.

Numeric types never lose information.
They never truncate automatically, nor widen.
Fixed-size numeric types never overflow silently.
Divide by zero must be handled.
The syntactic and cognitive overheads this necessarily imposes are eased
by language features to any extent reasonable.




## Sigil-logic

Datalove reserves some symbols to strongly mean one thing.

| When you see | it means                  |
|--------------|---------------------------|
| `:`          | type                      |
| `?`          | option                    |
| `!`          | result                    |
| `@`          | adapt (clone / widen )    |
| `;`          | statement break / newline |

Likewise some bracket pairs are for one thing.

| When you see | it means |
|--------------|----------|
| `<` … `>`    | generics |
| `{` … `}`  | struct-like, structs and tables |
| `[` … `]`  | array-like, lists and tensors |




## Restrictive type systems enable advanced features

Datalove has a strong linear type system.
Datalove functions are pure and nearly total,
with no side-effects and no exceptional control-flow,
and hove other restrictive properties.
Strong restrictions lead to simple
compiler analysis where the code you see lowers
directly to the code you expect,
requiring little reconstruction to regain performance.
It creates a canvas for advanced experiments in language
design including simple compile-time evalution,
global analysis, termination proofs, runtime memoization,
logic programming via choice points.
Few language-design doors are closed.

Datalove _procedures_ allow code to escape this restrictive regime
around the edges of the program through I/O and other side-effects.
