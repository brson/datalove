# Move and clone and argument modes


## Argument modes and callee expressions

Every argument repeats its parameter's mode at the call site:

- `argument@` - clone, for an `in` parameter; the original stays usable
- `argument` - move, for an `in` parameter (copy types are copied)
- `ref argument` - immutable borrow, for a `ref` parameter
- `mut argument` - mutable borrow, for a `mut` parameter
- `out argument` - the callee initializes it, for an `out` parameter

A marker that disagrees with the parameter's mode is an error.

All binops and unary negation treat their arguments as `ref` arguments.
They do not move.


## Initialization tracking

Some bindings can be uninitialized:

- `out` parameters - caller provides storage, callee must write before return
- `var x: Type` - declared without initializer, must `set` before use

These incur a runtime tracking byte for non-copy types
to determine whether they need to be dropped at scope exit.
