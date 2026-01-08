# Datalove Roadmap

The MVP is a working bare datafun language with:

- documentation
- modules
- closures
- clone ops
- type aliases
- generic functions for built-in generic types
- repl, script runner
- pipeline tools
- interpreter, jit, aot
- compute-only core library
- core->runtime calls
- decent diagnostics




# in progress

- module hashes and salsa verification
- human docs
- "bringing" ifs


# on deck

- working heap types, and default syntax everywhere
- "design-log" from design-notes
  - into news feed entries
- field access
- jit
- clone
- destructuring
- generics
- type aliases
- runtime calls
- clean up demo files in style of learn x in y minutes
- datafun ast roundtrip
- script unit undo / redo
- analysis caching
- sourceless tokens and whitespace-free tokens
- sourceless parsing
- separate parsing from analysis


----------



# future

- cleanup and specify bidirectional typechecking
- comptime/const execution
- named types
- improve runtime implementation unsafety
- type declarations
- simple multithreading
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend