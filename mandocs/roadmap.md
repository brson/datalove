# Datalove Roadmap


## Next checkpoint

- documentation
- [x] diagnostics
- module and script compile reactivity
- script interpreter reactivity
- [x] tensors and tables
- table column projections
- closures
- clone and coerce (@)
- [x] logic ops
- type aliases
- generic functions for built-in generic types
- basic repl,
- [x] script runner
- pipeline tools
- [x] interpreter
- [x] jit
- [x] aot
- compute-only core library
- core->runtime calls
- intrinsics and core functions
- [x] field projections
- match for enums




## In progress

- worldfile generator
- human docs
- script/pipeline compilation unification




## On deck

- clone and coerce
- updates feed
- roadmap status page
- clean up benchmarks
- datalit syntax cleanup
  - map set type syntax
  - usize/isize -> index/offset
- workspaces - waiting for design
- riders - waiting for workspaces
- rtcalls - waiting for native riders
- std.string - waiting for rtcalls
- assert statements - needed for std_tests?
- fully reactive scripts
- widening coercions
- match
- generics




## Backlog

- inlining
- 128-bit ints
- remove data / error keywords - rely on ~ coercion?
- testable docs
- multiple scripts
- analysis caching
- type declarations
- simple multithreading




## Backburner

- comptime / const evaluation
- allocation statistics
- jit statistics
- cleanup and specify bidirectional typechecking
- sourceless parsing
- lsp - needs a project/workspace concept
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend
- improve runtime implementation unsafety



## Far future

- named types
- first-class type variables
- termination proofs
- autodiff