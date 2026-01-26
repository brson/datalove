# Move and clone and argument modes


## Argument modes and callee expressions

- `argument.clone`
- `argument.move`
- `argument.mut`

All binops and unary negation treat their arguments as `ref` arguments.
They do not move.


## Initialization tracking

Some bindings can be uninitialized:

- `out` parameters - caller provides storage, callee must write before return
- `var x: Type` - declared without initializer, must `set` before use

These use runtime tracking bytes to know if they've been written.
Reading before init is a compile error.
At scope exit, uninitialized bindings don't get dropped (nothing to drop).

```datalove
var x: i32
set x = 42
debuglog x
```

Partial field writes to uninitialized bindings are disallowed
because tracking is per-binding, not per-field.
