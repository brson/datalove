# Datalove Roadmap


## Next checkpoint

- [ ] top-level documentation
- [x] diagnostics
- [ ] module and script compile reactivity
- [ ] script interpreter reactivity
- [x] tensors and tables
- [ ] table column projections
- [x] adapt opt / clone and coerce / @
- [x] logic ops
- [x] type aliases
- [x] ctfe
- [ ] const argument specialization
- [ ] generic functions for built-in generic types
- [ ] basic repl,
- [x] script runner
- [x] interpreter
- [x] jit
- [x] aot
- [ ] compute-only core library
- [ ] core->runtime calls
- [ ] intrinsics and core functions
- [x] field projections
- [ ] match for enums




## In progress

- fix primary expression parsing
- enums and match
- human docs
- const args



## On deck

- new repl
- rtcalls - waiting for design
- workspaces - waiting for design
- match syntax / switch ir instruction
  - use switch during specialization
- std.string - waiting for rtcalls
- assert statements - needed for std_tests?
- fully reactive scripts, undo/redo
- generics
- datalit syntax cleanup
  - map set type syntax




## Backlog

- inline/jit tuning
- tree-sitter
- 128-bit ints
- remove data / error keywords - rely on ~ coercion?
- testable docs
- multiple scripts
- analysis caching
- type declarations
- simple multithreading




## Backburner

- fix rt error/alloc semantics
- native riders - built-in runtime for now
- pipeline tools
- worldfile generator - waiting on pipeline stability
- closures
- const parameter specialization
- allocation statistics
- jit statistics
- cleanup and specify bidirectional typechecking
- sourceless parsing
- lsp - needs a project/workspace concept
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend
- improve runtime implementation unsafety



## Far future wants

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
