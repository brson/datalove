# Design Philosophy

Datalove's design is guided by several core principles.

## The Tower of Love

Datalove is intentionally structured as three cleanly-scoped sublanguages:

1. **Datalit** - Pure data, strongly typed, serializable
2. **Datafun** - Pure functions on Datalit types
3. **Datalove** - Full language with procedures and objects

Each layer builds on the previous, adding power while maintaining simplicity.

## Simplicity Through Constraints

### Pure Data Foundation

Datalit's restriction to pure, tree-shaped data enables:

- Total comptime evaluation
- Full or partial memoization
- Undo/redo functionality
- Rewind/replay capabilities
- Prolog-style choice points
- Multi-determinism

### Familiar Surface Syntax

Despite functional purity, Datafun feels imperative:

```datalove
fun increment(accum: u64, amount: u8): ?u64
  ret accum +? amount
end fun
```

Mutable-reference argument modes make pure functions look and feel familiar.

## Mechanical Sympathy

### Memory Management

- No mandatory garbage collection for Datalit/Datafun
- Heap allocation strategy encoded in types
- Local and global heap distinction
- Light syntax for allocating conversions

### Concurrency

For simplicity and OS sympathy:

- Multithreaded, not lightweight tasks
- No async/await (may experiment with callbacks)
- Potential Gleam-style inline continuation syntax

### Total Ordering

All pure data types support total ordering for efficient maps and sets.

Float ordering follows Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

## REPL-First Design

The language is built around REPL interaction:

- Fast incremental compilation
- Hot-reloading
- Rewind-and-replay
- Virtualized I/O for simulation

## Error Handling

- `?` for option types
- `!` for result types
- Dedicated `error` dynamic type
- No panics in pure functions

## Influences

- **Rust**: General syntax, mechanical sympathy, data structures, `total_cmp`
- **Zig**: `?`/`!` operators, `+|` overflow handling, anonymous structs
- **JSON5**: Baseline markup language requirements
- **Python**: Negative inspiration (heavyweight abstractions)
- **Polars/Pandas**: Table operations
- **TigerBeetle**: DST, virtualized I/O
- **Mercury**: Argument modes and logic programming

## See Also

- [Roadmap](roadmap.md)
- [Influences](influences.md)
