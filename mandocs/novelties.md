## Novelties

Matched-brace / token-tree lexing:
all braces are matched (`{ }`, `( )`, `[ ]`, `< >`).
Statement oriented, with statements that span multiple lines without statement terminators.
Pascal, Python, and Rust-influenced syntax.

Linear types and argument modes.
No reference types, no GC.
Ergonomic coercions.

Structural and nominal typing.
First-class option and result types (`?T`, `!T`).

Pure functions `fun` and I/O procedures (`proc`).

Fully-incremental compilation.
Whole-world compilation, all modules and other inputs must be known upfront.
Incremental script execution.
Interpreter, JIT and AOT.

Optional type-hint prefixes for all expressions:

```datalove
let foo = {
  a = : u8 / 100,
  b = [: int / 200, 300],
}
```

Heap types: `@` and `#`.

- brace-tree and newline-sensitivity
- reactive repl
- linear types with explicit destructors
- undo/redo, rewind/replay
- virtualized I/O
- zipper heaps
- multi-determinism, choice-points, and logic programming


