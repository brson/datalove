# Datalove Roadmap


## Next checkpoint

- documentation
- [x] diagnostics
- module and script compile reactivity
- script interpreter reactivity
- [x] tensors and tables
- table column projections
- clone and coerce (@)
- [x] logic ops
- [x] type aliases
- ctfe
- generic functions for built-in generic types
- basic repl,
- [x] script runner
- [x] interpreter
- [x] jit
- [x] aot
- compute-only core library
- core->runtime calls
- intrinsics and core functions
- [x] field projections
- match for enums




## In progress

- human docs
- ctfe



## On deck

- split ctfe and lowering phases into crates
- fix rt error/alloc semantics
- clone and coerce
- datalit syntax cleanup
  - map set type syntax
  - usize/isize -> index/offset
- workspaces - waiting for design
- riders - waiting for workspaces
- rtcalls - waiting for native riders
- std.string - waiting for rtcalls
- assert statements - needed for std_tests?
- fully reactive scripts, undo/redo
- match
- generics




## Backlog

- tree-sitter
- inlining
- 128-bit ints
- remove data / error keywords - rely on ~ coercion?
- testable docs
- multiple scripts
- analysis caching
- type declarations
- simple multithreading




## Backburner

- pipeline tools
- worldfile generator - waiting on pipeline stability
- closures
- comptime / const evaluation
- allocation statistics
- jit statistics
- cleanup and specify bidirectional typechecking
- sourceless parsing
- lsp - needs a project/workspace concept
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend
- improve runtime implementation unsafety



## Far future wants

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
