## Features

- Linear types with argument / binding modes for borrowing.
- Scalar types.
- Option and result types.
- List, map, set.
- Tensors (multi-dimensional arrays).
- Tables (dataframes / struct-of-arrays).
- Dynamic types (`data` and `error`).
- Memoized parsing, name resolution,
  typechecking, ownership analysis, and IR lowering (via Salsa).
- Parallelized parsing, name resolution,
  typechecking, ownership analysis, and IR lowering.
- SSA IR.
- IR-based interpreter.
- Function-tracing JIT (via Cranelift) (experimental).
- AOT (via Cranelift)
- AOT to statically-linked executables.
- Incremental compilation and evaluation.
- Script undo/redo.
- REPL.
- WASM-compatible compiler and interpreter.




## Type System Restrictions

- Linear types.
- No interior mutability.
- Pure functions - deterministic, no side effects.
- Almost-total functions:
  Infinite loops are possible;
  we may be able to prove termination in some cases.
- Whole program compilation.
- In / out / ref / mut argument modes.
  Could enable bidirectionality ala Mercury?




## Capabilities Potentially Enabled by Restrictions

- Termination proofs.
- Compile-time evaluation.
- Refinement types.
- Bidirectionality.
- Globally-unioned type-variable instantiations
  and closure instantiations.
- Direct SSA lowering.
- Session types.
- Guaranteed explicit drops.
- Rewind and replay.