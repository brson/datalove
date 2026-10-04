## Datalove Concise Feature List

- Linear types with argument / binding modes for borrowing.
- Scalar types.
- Option and result types.
- List, map, set.
- Atoms, terms, and enums.
- Tensors (multi-dimensional arrays) (literals and first-axis indexing).
- Tables (dataframes) (literals only).
- Dynamic types (`data` and `error`).
- Generic functions (type-erased).
- Compile-time evaluation and const parameters.
- Native riders (functions implemented in Rust).
- Memoized parsing, name resolution,
  typechecking, ownership analysis, and IR lowering (via Salsa).
- Parallelized parsing, name resolution,
  typechecking, ownership analysis, and IR lowering
  (opt-in, `DATALOVE_PARALLEL=1`).
- SSA IR.
- IR-based interpreter.
- Per-function JIT of hot functions (via Cranelift).
- AOT (via Cranelift).
- AOT via C.
- AOT to executables, with the runtime and riders statically linked.
- Incremental compilation and evaluation.
- Script unit editing.
- REPL.
- WASM-compatible build.




## Type System Restrictions

- Linear types.
- No interior mutability.
- Pure functions - deterministic, no side effects.
- Almost-total functions:
  Infinite loops and recursion are possible.
- Whole program compilation.
- In / out / ref / mut argument modes.




## Capabilities Potentially Enabled by Restrictions

- Termination proofs.
- Refinement types.
- Bidirectionality.
- Globally-unioned type-variable instantiations
  and closure instantiations.
- Direct SSA lowering.
- Session types.
- Guaranteed explicit drops.
- Rewind and replay.
- Bidirectionality ala Mercury.
- Choice points and logic programming.
