## The Datalove Decree

Datalove on various language design topics.




### 2026-01-07 - Logic operators

Booleans support `and`, `or`, `xor`, and `not`.
todo say more


### 2026-01-07 - Module content hashes and memoization

The module graph forms a DAG.
We use this to create strong content hashes
for every module instantiation.

This can be used as a key for various caching purposes.
We specifically use it to verify correct memoization:
Functions on modules should only rerun if their
module's content hash has changed.

This content hash includes
the source code of a module,
and the configuration of that module
including which modules the requires/import demands are bound to.




### 2026-01-05 - Name resolution and mutual recursion

Within a module
name resolution is bidirectional.
Top-level names may only be declared once,
and they may be mutually recursive.

```datalove
// Pretend this is a module.

fun is_even(n: u32): !bool
    if n == @0
        ret ok @true
    else
        ret is_odd(n -! @1)  // Forward reference OK in modules.
    end if
end fun

fun is_odd(n: u32): !bool
    if n == @0
        ret ok @false
    else
        ret is_even(n -! @1)
    end if
end fun
```

Within a script
name resolution is one-directional.
Names may only refer to previous declarations.

```datalove
// Pretend this is a script.

// In scripts, `is_odd` must be defined before `is_even` can call it.

fun is_odd(n: u32): !bool
    if n == @0
        ret ok @false
    else
        ret is_even(n -! @1)  // ERROR: `is_even` not yet defined.
    end if
end fun

fun is_even(n: u32): !bool
    if n == @0
        ret ok @true
    else
        ret is_odd(n -! @1)  // OK: `is_odd` already defined.
    end if
end fun
```




### Unconditional and conditional loops

Unconditional loops are spelled `loop`.
They require a `break` or `ret` to exit.

```datalove
var x = 0

loop
  if x = 0
    set x = 1
  else if x = 10
    set x = 11
    break
  end if
  set x = x + 1
end loop

debuglog x
```

Conditional loops with `loop while`:

```datalove
var x = 0

loop while x != 10
  if x = 0
    set x = 1
  else if x = 10
    set x = 11
  else
    set x = x + 1
  end if
end loop

debuglog x
```



### 2026-01-05 - Explicit data flow with `carry` and `bring`

Datalove's surface syntax is designed to map obviously to SSA IR's
and the machine-level register/stack/heap model.
The `carry` and `bring` keywords create explicit data flow
into and out of loops, which simplifies lowering and
enables advanced features through verification of `total` loops.

Unconditional loop with _carries_ and _brings_:

```datalove
loop carry (x = 0)   // `x` is an SSA variable / loop induction variable
  if x = 0
    continue (1)     // Next iteration x = 1
  else if x = 10
    let x_next = 11  // Extra binding for clarity
    break (x_next)   // Loop exit value
  end if
  continue (x + 1)   // Fallthrough not allowed in unconditional `loop` with carries
end loop bring (x)   // Binds exit value from `break` to outer scope`s `x`

debuglog x
```

`carry` names per-iteration bindings that
are initialized on entry, and must be
re-received each time through the loop via `continue`.
`bring` names post-iteration outer-scope bindings
and must be invoked with `break`.

> Mnemonic help: "continue and carry ; break and bring".

We call these two types of bindings "carries" and "brings";
their operation and semantics is similar to `in`-mode function arguments.

`bring`'s end-of-loop syntactic position forces the new outer-scope bindings to
be visually scanned immediately before they come into scope
(vs expression-based assignment, `let x = loop ...`).

`carry` and `bring` don't need to be paired:
loops with only carries and only brings are valid:

```datalove
loop carry (x = 0)
  if x = 0
    continue (1)
  else
    break
  end if
end loop

set x = 0

loop
  if x = 0
    set x = x + 1
    continue
  else
    break (x)
  end if
end loop bring (y)

debuglog y
```

In conditional loops the `carry` comes before `while`:

```datalove
loop carry (
  x = 0,
) while x != 10 else break (x)
  if x = 0
    continue (1)
  end if
  continue (x + 1)   // `continue` is required
end loop bring (x)

debuglog x
```

When a `while` loop has brings
it must always be paired with `else break`
to handle the termination condition,
including the zero-iteration case.

```datalove
loop carry (
  x = 0,
) while x != 10
  if x = 0
    continue (1)
  end if
  continue (x + 1)   // `continue` is required
end loop
```

A more feature-complete example (w/ nonsense logic):

```datalove

let default: int = 100

// Carry args are named and positional, like function args:
// `continue` must have same arg types as `carry`;
// `break` as `bring`.
//
// Mnemonic help: "continue and carry, break and bring".
loop carry (
  x: int = 0,
  y: int = 0,
) while (
  x != 10
) else break (
  x + 1,
  y - 1,
  x * default,       // Can access outer bindings.
)
  if x = 0
    continue (1, 2)
  else if y = 10
    break (10, 20, 30)
  else
    continue (x + y, x * y)
  end if
  // No `continue` needed because all branches terminate.
end loop bring (
  a: int,
  b: int,
  c: int,
)

debuglog (a, b, c)
```




### 2026-01-05 - `if` statements with _brings_

In the same spirit as `loop` with `carry` and `bring`.

```datalove
let x = 100

if x < 10
  break (1)
else if x < 100
  break (2)
else
  break (3)
end if bring (y)

debuglog y
```

To avoid ambiguity between `break` targets with nested constructs,
`break` only applies to the innermost control-flow construct
with `bring` bindings:

```datalove
loop
  if 1 < 3
    break (1)        // `break` targets `if bring`
  else if 2 < 3
    break (1)
  else
    break (2)
  end if bring (y)
  break (y)
end loop bring (x)

debuglog (x)
```

`if` with `bring` is exhaustive,
must have `else` branch and all must termimnate with `break`.

If the `if` doesn't "bring":

```datalove
loop
  if true
    break (1)
  else
    break (2)
  end if
end loop bring (x)

debuglog x
```




### Function return types

Functions with return types require `ret` with value.

```datalove
fun choose(a: u32): bool
  if a < 10
    ret true
  else
    ret false
  end if
end fun
```

Functions can have void return types
and allow `ret` statements without values.

```datalove
fun foo()
  // no ret required
end fun

fun choose(a: u32)
  if a < 10
    ret
  end if
end fun
```




### Option and result handling

The option and result types are prefixes like zig:

```datalove
// option
let a: ?u32 = ...
let b: ?[u32] = ...
// result
let c: !u32 = ...
let d: ![u32] = ...
```

Option and result construction
is done with the `some`, `none`, `ok` and `er` keywords.
Note the awkward `er error` construction which
constructs an `er` `result` varriant out of an `error` value.

```datalove
let a: ?u32 = some 3
let b: ?u32 = some u32.min_value()
let c: ?u32 = none

let a: !u32 = ok 3
let b: !u32 = ok u32.min_value()
let c: ?u32 = er error "oops"
```

Destructuring option and result is like zig:

```datalove
let a: ?u8 = 1

var c: u8 = 0
if a |value|
  set c = value
else
  set c = 255
end if
```

```datalove
let a: !u8 = 1

var c: u8 = 0
if a |value|
  set c = value
else |error|
  ret error
end if
```

Branching on error values is not like zig
and uses reflection, not shown here.

Early return with postfix `?` and `!`:

```datalove
fun transform_option(val: ?u32): ?u32
  let val = val? // early none return
  ret val +? 1   // early none return on overflow
end fun

fun transform_result(val: !u32): !u32
  let val = val! // early error return
  ret val +! 1   // early error return on overflow
end fun
```




### Math ops and error handling

Datalove is especially strict about overflow and error
cases in machine-size integers and floats.

Floats support all bare math ops,
`+ - * /` and unary `-`.

Bigints (`int`) support all but div:
`+ - *` and unary `-`.
For div we must use a checked variant to handle divide-by-zero.

Fixed ints support all but div:
`+ - *` and unary `-`,
but _widen_ to bigints.

Fixed ints support checked early-return varieties:

```datalove
// Checked optional arithmetic, early-returning `none`.
let a = 1 +? 1
let a = 1 -? 1
let a = 1 *? 1
let a = 1 /? 1
let a = -?a     // early-return negation

// Checked result arithmetic, early-returning `er`.
let a = 1 +! 1
let a = 1 -! 1
let a = 1 *! 1
let a = 1 /! 1
let a = -!a
```

`?` early-returns `?T` option types and `!` early-returns `!T` result types.

These either result in the same type as the input types or early return -
they do not result in option or result types. Their enclosing function
must be the correct optionr/result type.

Bigints support the early-return division but not the others.
Floats don't support early-return math.

If we decide to let funs panic we'll also add panicking variations.




### Comparison operators
#### Floats and total ordering

All pure data types support a total order,
which is used for maps and sets.

Floats use the typical ordering, like Rust's `total_cmp`:

> -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN

Equality, less than, greater than, etc. behave
the standard way wrt float zeros and NaNs.










### Pure functions + mutable-reference argument modes

Four argument modes: `in`, `out`, `ref`, `mut`

```datalove
fun (
  a: int,     // default `in`
  out b: int,
  ref c: int,
  mut d: int,
)
  // `out` args must be assigned on all code paths
  set b = a
  set d = c + d
end fun
```




### Allocations

Besides encoding heaps in types,
having local and global heaps,
and enforcing a light syntax
on allocating conversions,
Datalove is not concerned with providing
control over the allocator;
but instead only with providing data access patterns
and metadata with which the allocator can perform optimally.




### Error handling

The result type is a language type,
and is dedicated the `!` sigil &mdash;
if you see `!` you are looking at error handling.

The error type is a dynamic type that can hold
any type (an existential type).




### REPL

The script is REPL-first and intends to advance the
state of the art in REPLs.
With pure data statements and virtualized I/O the repl
supports undo/redo and record-and-replay.

The repl is a modern Ratatui application.
The datafun interpreter runs on the web
as a egui_ratatui application.




### Trailing commas and separators

Allowed and optional in all sequence forms of course.




### Generics

Functions can have type parameters:

```datalove
fun unwrap_or<T>(self: ?T, default: T): T where {
  T is move,
}
  if self |value|
    ret value
  else
    ret default
  end if
end fun
```

By default type parameters have no capabilities,
`clone`, `move`, `etc`. `where` clauses are
usually needed.

Data structures can not have type parameters.

TODO: How are generics translated?




### Type aliases

Datafun has a structural type system.
We can asign names with `typealias`.

```datalove
// `Contact` is an alias for a struct.
typealias Contact: {
  name: string,
  age: int,
}
```




---



## Wanted

- operator precedence
