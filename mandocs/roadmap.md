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
- heap types, examples omit
- field access
- match for enums


# in progress

- module hashes and salsa verification
- human docs




# on deck

- working heap types, and default syntax everywhere
- field access
- jit
- clone
- match
- generics
- type aliases
- runtime calls
- script unit undo / redo
- "design-log" from design-notes
  - into news feed entries
- clean up demo files in style of learn x in y minutes
- datafun ast roundtrip
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



## backburner

- if with brings - complexity, need to proove usefulness of brings with loop first
