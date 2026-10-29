# Datalove Roadmap

Datalove is in a research and prototype stage.
Neither the design nor implementation are complete.

I am currently focused on end-to-end compiler architecture
to ensure all my primary goals are feasible,
as well as working on the CLI repl experience
for my own use and to shake out bugs.

The surface language is spartan,
as is the standard library,
and the language can perform no direct I/O.

- [Checkpoint 2 - Interactive scripts](#user-content-checkpoint-2---interactive-scripts)
- [In progress](#user-content-in-progress)
- [On deck](#user-content-on-deck)
- [Retired checkpoints](#user-content-retired-checkpoints)
  - [Checkpoint 1 - Compiler architecture](#user-content-checkpoint-1---compiler-architecture)




## Checkpoint 2 - Interactive scripts

Focus on ergonomic scripting experience and the REPL.

- [ ] interactive CLI UX
- [ ] module and script compile reactivity
- [ ] script interpreter reactivity
- [ ] repl error and abort semantics
- [ ] assert statements
- [ ] rational numbers
- [ ] numeric ergonomics
- [ ] ?/! polymorphism
- [ ] enum subtyping
- [x] filesystem workspaces
- [ ] multiple concurrent interactive scripts
- [ ] std docs, in-repl docs, generated docs
- [ ] pipeline I/O
- [ ] virtual I/O for all existing file access




## In progress

- repl ux
- script reactivity
- recursive types


## On deck

- work stacks for runtime traversals
- interactive math ergonomics
- rational numbers
- saturating and wrapping math ops


## Punted

- tuned generics specialization
- lazy lowering
- lazy lowering of consts




### Retired checkpoints


## Checkpoint 1 - Compiler architecture

Initial prototype.
Thorough compiler architecture, bare language.

- [x] basic documentation
- [x] diagnostics
- [x] tensors and tables
- [x] indexing
- [x] adapt opt / clone and coerce / @
- [x] logic ops
- [x] type aliases
- [x] ctfe
- [x] generic functions for built-in generic types
- [x] basic repl,
- [x] script runner
- [x] interpreter
- [x] jit
- [x] aot
- [x] compute-only std library
- [x] std->runtime calls
- [x] intrinsics and core functions
- [x] field projections
- [x] match for enums

