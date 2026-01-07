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
- plan-backend-unify
- human docs
- termination detection and refinement types


# on deck

- serialized ir bundles loading
- "design-log" from design-notes
  - into news feed entries
- "bringing" ifs
- field access
- jit
- clone
- destructuring
- generics
- type aliases
- memoization tests
- runtime calls
- boolean ops - and or xor not
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
- heap genericity and clone method
- simple multithreading
- deterministic builds - compiler pipeline is already deterministic
- wasm-component backend