# Datalove Roadmap




## Checkpoint 1 - Compiler Architecture

Initial prototype.
Thorough compiler architecture, bare language.

- [ ] documentation
- [x] diagnostics
- [ ] workspaces
- [x] tensors and tables
- [x] indexing
- [ ] table column projections
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

- const param specialization - mostly done?
- index projections in generics and fat references
- std
- tensors
- tables


## On deck

- human docs
- fully reactive scripts, undo/redo




## Checkpoint 2 - REPL

Focus on ergonomic REPL experience.

- [ ] module and script compile reactivity
- [ ] script interpreter reactivity
- [ ] script unit undo/redo
- [ ] assert statements - needed for std_tests?
- [ ] int auto-clones
- [ ] type sythesis for all literal forms
- [ ] enum subtyping




## Backburner

- multiple scripts
- pipeline tools
- analysis caching
- simple multithreading
- testable docs
- tree-sitter
- 128-bit ints
- fix rt error/alloc semantics
- worldfile generator - waiting on pipeline stability
- closures
- const parameter specialization
- inline/jit tuning
- allocation statistics
- jit statistics
- cleanup and specify bidirectional typechecking
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
- true linear types with explicit dtors
- bidirectionality, multi-determinism, choice-points ala mercury
