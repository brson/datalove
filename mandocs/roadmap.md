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




## Checkpoint 1 - Compiler Architecture

Initial prototype.
Thorough compiler architecture, bare language.

- [ ] basic documentation
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




## In progress

- human docs
- const param specialization - mostly done?
- fully reactive scripts, undo/redo


## On deck

- tensor std features
- assert and using it in favor of hacks in test harnesses
- closures
- table row polymorphism, column projections etc



## Checkpoint 2 - REPL

Focus on ergonomic REPL experience.

- [ ] UX
- [ ] module and script compile reactivity
- [ ] script interpreter reactivity
- [ ] script unit undo/redo
- [ ] assert statements
- [ ] auto-adapt and other conveniences
- [ ] rational numbers
- [ ] monadic math ops
- [ ] ?/! polymorphism
- [ ] type sythesis for all literal forms
- [ ] enum subtyping
- [ ] filesystem workspaces
- [ ] multiple concurrent interactive scripts
- [ ] std docs, in-repl docs, generated docs




## Backburner

- filesystem workspaces
- doc generator and std docs
- multiple scripts
- pipeline tools
- simple multithreading
- testable docs
- tree-sitter
- 128-bit ints
- fix rt error/alloc semantics
- inline/jit tuning
- allocation statistics
- jit statistics
- sourceless parsing
- lsp - needs a project/workspace concept
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend
- improve runtime implementation unsafety




## Far future wants

- datalog rule and reduction sugar
- gadts
- heap types and global heap
- named types
- first-class type variables
- termination proofs
- refinement types
- autodiff
- zipper heaps
- virtualized I/O
- linear types with explicit dtors
- bidirectionality, multi-determinism, choice-points ala mercury
