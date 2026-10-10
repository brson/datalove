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




## Checkpoint 2 - Interactive scripts

Focus on ergonomic scripting experience and the REPL.

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
- [ ] pipeline tools



## In progress

- bytecode
- backend optimization
- human docs
- reactive scripts
- repl ux




## On deck

- collection iteration
- saturating and wrapping math ops




## Backburner

- tensor std features
- assert and using it in favor of hacks in test harnesses
- closures
- table row polymorphism, column projections etc
- doc generator and std docs
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

