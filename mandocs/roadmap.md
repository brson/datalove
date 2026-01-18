# Datalove Roadmap


## Next checkpoint

- documentation
- [x] diagnostics
- module and script compile reactivity
- script interpreter reactivity
- [x] tensors and tables
- table column projections
- closures
- clone ops
- [x] logic ops
- type aliases
- generic functions for built-in generic types
- basic repl,
- [x] script runner
- pipeline tools
- [x] interpreter
- [~] jit
- [x] aot
- compute-only core library
- core->runtime calls
- intrinsics and core functions
- heap types, examples omit
- [x] field projections
- match for enums




## In progress

- usize/isize
- human docs




## On deck

- std.string - waiting for rtcalls
- assert statements - needed for std_tests
- fully reactive scripts
- clone and coerce
- clone, working heap types, and default syntax everywhere
- widening coercions
- jit
- match
- generics
- type aliases
- runtime calls




## Backlog

- 128-bit ints
- remove data / error keywords - rely on ~ coercion?
- testable docs
- multiple scripts
- analysis caching
- updates feed
- type declarations
- simple multithreading




## Backburner

- cleanup and specify bidirectional typechecking
- comptime/const execution
- sourceless parsing
- lsp - needs a project/workspace concept
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend
- improve runtime implementation unsafety
- usize/isize -> index/offset



## Far future

- named types
